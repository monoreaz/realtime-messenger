import asyncio
import os
import unittest
import uuid
from contextlib import asynccontextmanager
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock, patch

import test_admin
from test_call_history import Socket
from app.api import ws
from app.api.auth import hash_raw_token
from app.core.security import decode_access_token
from app.models.auth_session import AuthSession
from app.models.chat import Chat, ChatMember
from app.models.push_device import PushDevice
from app.services import call_push
from jwt.exceptions import InvalidTokenError


class BackgroundCallTests(unittest.IsolatedAsyncioTestCase):
    request = test_admin.AdminTests.request

    async def asyncSetUp(self):
        await test_admin.AdminTests.asyncSetUp(self)
        self.cid = str(uuid.uuid4())
        self.caller, self.receiver = Socket(), Socket()
        self.chat = Chat(type="private", direct_key=":".join(sorted([str(self.admin.id), str(self.member.id)])))
        self.db.add(self.chat)
        await self.db.flush()
        self.db.add_all([ChatMember(chat_id=self.chat.id, user_id=user.id) for user in (self.admin, self.member)])
        self.session = AuthSession(user_id=self.member.id, token_hash=hash_raw_token(uuid.uuid4().hex),
            expires_at=datetime.now(timezone.utc) + timedelta(days=1))
        self.db.add(self.session)
        await self.db.flush()
        self.device = PushDevice(token="call-device-" + uuid.uuid4().hex, session_id=self.session.id,
            binding=str(uuid.uuid4()), call_notifications=True)
        self.db.add(self.device)
        await self.db.flush()
        @asynccontextmanager
        async def database():
            yield self.db
        self.enterContext(patch.object(ws, "AsyncSessionLocal", database))
        self.enterContext(patch.object(call_push, "AsyncSessionLocal", database))
        self.enterContext(patch.dict(os.environ, PUSH_ENABLED="true"))
        self.push = self.enterContext(patch.object(ws, "schedule_call_push"))
        self.enterContext(patch.object(ws, "expire_call", new_callable=AsyncMock))
        self.save = self.enterContext(patch.object(ws, "save_call_message", new_callable=AsyncMock))
        self.enterContext(patch.object(ws.manager, "send_to_user", new_callable=AsyncMock))
        self.enterContext(patch.object(ws.manager, "send_to_users", new_callable=AsyncMock))
        self.enterContext(patch.object(ws.manager, "is_online", return_value=False))

    async def asyncTearDown(self):
        ws.active_calls.pop(self.cid, None)
        ws.ringing_calls.pop(self.cid, None)
        ws.call_started.pop(self.cid, None)
        await test_admin.AdminTests.asyncTearDown(self)

    async def offer(self):
        await ws.handle_call_signal(self.caller, self.admin, {"type": "call.offer", "call_id": self.cid,
            "peer_id": str(self.member.id), "sdp": {"type": "offer", "sdp": "test-offer"}})

    async def test_offline_device_receives_call_and_replay_with_buffered_ice(self):
        await self.offer()
        self.assertIn(self.cid, ws.active_calls)
        self.assertEqual(self.push.call_args.args[:3], (self.member.id, "call.incoming", self.cid))
        await ws.handle_call_signal(self.caller, self.admin, {"type": "call.ice", "call_id": self.cid,
            "peer_id": str(self.member.id), "candidate": {"candidate": "candidate:test"}})
        await ws.replay_ringing_calls(self.receiver, self.member.id)
        self.assertEqual([event["type"] for event in self.receiver.events], ["call.offer", "call.ice"])
        self.assertEqual(self.receiver.events[0]["sdp"]["sdp"], "test-offer")
        outsider = Socket()
        await ws.replay_ringing_calls(outsider, uuid.uuid4())
        self.assertEqual(outsider.events, [])

    async def test_old_app_or_revoked_session_cannot_receive_background_calls(self):
        self.device.call_notifications = False
        await self.db.flush()
        await self.offer()
        self.assertNotIn(self.cid, ws.active_calls)
        self.assertEqual(self.caller.events[-1]["reason"], "offline")
        self.device.call_notifications = True
        self.session.revoked_at = datetime.now(timezone.utc)
        await self.db.flush()
        await self.offer()
        self.assertNotIn(self.cid, ws.active_calls)

    async def test_answer_cancels_ringing_and_second_device_cannot_take_over(self):
        await self.offer()
        answer = {"type": "call.answer", "call_id": self.cid, "peer_id": str(self.admin.id),
            "sdp": {"type": "answer", "sdp": "answer"}}
        await ws.handle_call_signal(self.receiver, self.member, answer)
        self.assertIs(ws.active_calls[self.cid][3], self.receiver)
        self.assertEqual(self.push.call_args.args[1], "call.dismiss")
        other = Socket()
        await ws.handle_call_signal(other, self.member, answer)
        self.assertIs(ws.active_calls[self.cid][3], self.receiver)
        await ws.replay_ringing_calls(other, self.member.id, self.cid)
        self.assertEqual(other.events[-1]["type"], "call.end")

    async def test_cancel_and_expired_call_are_not_replayed(self):
        await self.offer()
        ws.ringing_calls[self.cid]["expires_at"] = 1
        await ws.replay_ringing_calls(self.receiver, self.member.id, self.cid)
        self.assertEqual(self.receiver.events[-1]["type"], "call.end")
        await ws.finish_call(self.cid, "cancelled")
        self.assertNotIn(self.cid, ws.ringing_calls)
        self.assertEqual(self.push.call_args.args[1], "call.dismiss")
        self.save.assert_awaited_once()

    def action_token(self, **overrides):
        values = dict(call_id=self.cid, user_id=self.member.id, session_id=self.session.id,
            binding=self.device.binding, expires_at=datetime.now(timezone.utc) + timedelta(seconds=60))
        values.update(overrides)
        return call_push.decline_token(**values)

    async def test_native_decline_is_scoped_and_idempotent(self):
        await self.offer()
        token = self.action_token()
        with self.assertRaises(InvalidTokenError):
            decode_access_token(token)
        status, _ = await self.request(f"/calls/{uuid.uuid4()}/decline", method="POST", body={"token": token})
        self.assertEqual(status, 401)
        for _ in range(2):
            status, _ = await self.request(f"/calls/{self.cid}/decline", method="POST", body={"token": token})
            self.assertEqual(status, 204)
        self.assertNotIn(self.cid, ws.active_calls)
        self.save.assert_awaited_once()

    async def test_decline_rejects_expired_revoked_or_rebound_device(self):
        await self.offer()
        for token in [self.action_token(expires_at=datetime.now(timezone.utc) - timedelta(seconds=1)),
                      self.action_token(binding=str(uuid.uuid4()))]:
            self.assertEqual((await self.request(f"/calls/{self.cid}/decline", method="POST", body={"token": token}))[0], 401)
        token = self.action_token()
        self.session.revoked_at = datetime.now(timezone.utc)
        await self.db.flush()
        self.assertEqual((await self.request(f"/calls/{self.cid}/decline", method="POST", body={"token": token}))[0], 401)
        self.assertIn(self.cid, ws.active_calls)

    async def test_ice_endpoint_requires_active_account(self):
        self.assertEqual((await self.request("/calls/ice"))[0], 401)
        with patch.dict(os.environ, TURN_URLS=""):
            status, config = await self.request("/calls/ice", self.member)
            self.assertEqual(status, 200)
            self.assertIn("iceServers", config)
        self.member.is_blocked = True
        await self.db.flush()
        self.assertEqual((await self.request("/calls/ice", self.member))[0], 403)

    async def test_ring_buffer_is_bounded(self):
        await self.offer()
        for _ in range(140):
            await ws.handle_call_signal(self.caller, self.admin, {"type": "call.ice", "call_id": self.cid,
                "peer_id": str(self.member.id), "candidate": {"candidate": "test"}})
        self.assertEqual(len(ws.ringing_calls[self.cid]["candidates"]), 128)


class CallPayloadTests(unittest.TestCase):
    def test_incoming_payload_has_short_expiry_and_no_webrtc_or_audio_data(self):
        import firebase_admin
        from firebase_admin import messaging
        from types import SimpleNamespace
        user = uuid.uuid4()
        device = SimpleNamespace(token="device-token", session_id=uuid.uuid4(), binding=str(uuid.uuid4()))
        info = {"chat_id": uuid.uuid4(), "caller_id": uuid.uuid4(), "name": "Caller",
            "expires_at": datetime.now(timezone.utc).timestamp() + 30, "offer": {"sdp": "private-sdp"}}
        result = SimpleNamespace(responses=[SimpleNamespace(success=True, exception=None)])
        with patch.object(firebase_admin, "get_app"), patch.object(messaging, "send_each", return_value=result) as send:
            call_push.send_call_batch([(device, user)], "call.incoming", str(uuid.uuid4()), info)
        payload = send.call_args.args[0][0]
        self.assertLessEqual(payload.android.ttl.total_seconds(), 30)
        self.assertGreater(payload.android.ttl.total_seconds(), 0)
        self.assertEqual(payload.data["type"], "call.incoming")
        self.assertNotIn("sdp", payload.data)
        self.assertEqual(call_push.decode_decline_token(payload.data["decline_token"])["session_id"], str(device.session_id))
        info["expires_at"] = 1
        with patch.object(messaging, "send_each") as send:
            self.assertEqual(call_push.send_call_batch([(device, user)], "call.incoming", str(uuid.uuid4()), info), [])
            send.assert_not_called()
