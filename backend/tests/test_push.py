"""Push registration and recipient isolation against a disposable PostgreSQL DB."""
import os
import unittest
import uuid
from contextlib import asynccontextmanager
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock, patch

import test_admin
from app.api.auth import REFRESH_COOKIE_NAME, hash_raw_token
from app.models.auth_session import AuthSession
from app.models.chat import Chat, ChatMember
from app.models.push_device import PushDevice
from app.models.user import User
from app.services import push


class PushTests(unittest.IsolatedAsyncioTestCase):
    asyncSetUp = test_admin.AdminTests.asyncSetUp
    asyncTearDown = test_admin.AdminTests.asyncTearDown
    request = test_admin.AdminTests.request

    async def session(self, user, **values):
        raw = uuid.uuid4().hex
        session = AuthSession(user_id=user.id, token_hash=hash_raw_token(raw),
            expires_at=values.pop("expires_at", datetime.now(timezone.utc) + timedelta(days=1)), **values)
        self.db.add(session)
        await self.db.flush()
        return session, [(b"cookie", f"{REFRESH_COOKIE_NAME}={raw}".encode())]

    async def test_registration_requires_matching_active_session(self):
        session, cookie = await self.session(self.member)
        body = {"token": "device-token-" + uuid.uuid4().hex, "binding": str(uuid.uuid4())}
        self.assertEqual((await self.request("/push/devices", method="POST", body=body))[0], 401)
        self.assertEqual((await self.request("/push/devices", self.member, "POST", body))[0], 401)
        self.assertEqual((await self.request("/push/devices", self.admin, "POST", body, headers=cookie))[0], 401)
        self.assertEqual((await self.request("/push/devices", self.member, "POST", body, headers=cookie))[0], 204)
        device = await self.db.get(PushDevice, body["token"])
        self.assertEqual(device.session_id, session.id)
        self.assertEqual(device.binding, body["binding"])
        self.assertEqual((await self.request("/auth/logout", method="POST", headers=cookie))[0], 204)
        self.assertEqual((await self.request("/push/devices", self.member, "POST", body, headers=cookie))[0], 401)

    async def test_rotation_and_account_switch_replace_binding(self):
        _, cookie = await self.session(self.member)
        body = {"token": "old-token-" + uuid.uuid4().hex, "binding": str(uuid.uuid4())}
        await self.request("/push/devices", self.member, "POST", body, headers=cookie)
        old = body["token"]
        body["token"] = "new-token-" + uuid.uuid4().hex
        await self.request("/push/devices", self.member, "POST", body, headers=cookie)
        self.assertIsNone(await self.db.get(PushDevice, old))
        other_session, other_cookie = await self.session(self.admin)
        body["binding"] = str(uuid.uuid4())
        await self.request("/push/devices", self.admin, "POST", body, headers=other_cookie)
        device = await self.db.get(PushDevice, body["token"])
        self.assertEqual(device.session_id, other_session.id)
        self.assertEqual(device.binding, body["binding"])

    async def test_expired_session_and_bad_input_rejected(self):
        _, cookie = await self.session(self.member, expires_at=datetime.now(timezone.utc) - timedelta(seconds=1))
        body = {"token": "device-token-" + uuid.uuid4().hex, "binding": str(uuid.uuid4())}
        self.assertEqual((await self.request("/push/devices", self.member, "POST", body, headers=cookie))[0], 401)
        _, cookie = await self.session(self.member)
        for invalid in [{**body, "token": "short"}, {**body, "token": "a" * 2049}, {**body, "binding": "bad"}]:
            self.assertEqual((await self.request("/push/devices", self.member, "POST", invalid, headers=cookie))[0], 422)

    async def recipient_setup(self):
        chat = Chat(type="group", name="Push tests")
        self.db.add(chat)
        await self.db.flush()
        self.db.add_all([ChatMember(chat_id=chat.id, user_id=u.id) for u in (self.admin, self.member)])
        session, _ = await self.session(self.member)
        token = "recipient-" + uuid.uuid4().hex
        binding = str(uuid.uuid4())
        self.db.add(PushDevice(token=token, binding=binding, session_id=session.id))
        # Sender also has a device, but must not receive their own push.
        sender_session, _ = await self.session(self.admin)
        self.db.add(PushDevice(token="sender-" + uuid.uuid4().hex, binding=str(uuid.uuid4()), session_id=sender_session.id))
        outsider = User(username="outsider_" + uuid.uuid4().hex[:12], password_hash="unused")
        self.db.add(outsider)
        await self.db.flush()
        outsider_session, _ = await self.session(outsider)
        self.db.add(PushDevice(token="outsider-" + uuid.uuid4().hex, binding=str(uuid.uuid4()), session_id=outsider_session.id))
        await self.db.flush()
        return chat, session, token, binding

    def database_context(self):
        @asynccontextmanager
        async def database():
            yield self.db
        return database

    async def test_only_eligible_recipients_and_invalid_token_cleanup(self):
        chat, session, token, binding = await self.recipient_setup()
        with patch.dict(os.environ, PUSH_ENABLED="true"), patch.object(push, "AsyncSessionLocal", self.database_context()), patch.object(push, "send_batch", return_value=[]) as send:
            await push.deliver_message_push(chat.id, uuid.uuid4(), self.admin.id)
            self.assertEqual(list(send.call_args.args[0]), [(token, binding, self.member.id)])
            for field, value in [("revoked_at", datetime.now(timezone.utc)), ("expires_at", datetime.now(timezone.utc) - timedelta(seconds=1))]:
                setattr(session, field, value)
                await self.db.flush()
                send.reset_mock()
                await push.deliver_message_push(chat.id, uuid.uuid4(), self.admin.id)
                send.assert_not_called()
                session.revoked_at = None
                session.expires_at = datetime.now(timezone.utc) + timedelta(days=1)
            self.member.is_blocked = True
            await self.db.flush()
            await push.deliver_message_push(chat.id, uuid.uuid4(), self.admin.id)
            send.assert_not_called()
            self.member.is_blocked = False
            await self.db.flush()
            send.return_value = [(token, binding, self.member.id)]
            await push.deliver_message_push(chat.id, uuid.uuid4(), self.admin.id)
            self.assertIsNone(await self.db.get(PushDevice, token))

    async def test_push_failure_does_not_break_delivery(self):
        chat, _, _, _ = await self.recipient_setup()
        with patch.dict(os.environ, PUSH_ENABLED="true"), patch.object(push, "AsyncSessionLocal", self.database_context()), patch.object(push, "send_batch", side_effect=RuntimeError("offline")):
            with self.assertLogs(push.logger, level="WARNING"):
                await push.deliver_message_push(chat.id, uuid.uuid4(), self.admin.id)

    async def test_message_endpoint_schedules_push_after_save(self):
        chat, _, _, _ = await self.recipient_setup()
        with patch("app.api.messages.deliver_message_push", new_callable=AsyncMock) as deliver:
            status, message = await self.request(f"/chats/{chat.id}/messages", self.admin, "POST", {"content": "Hello"})
        self.assertEqual(status, 201)
        deliver.assert_awaited_once_with(chat.id, uuid.UUID(message["id"]), self.admin.id)

    async def test_edit_and_delete_do_not_send_new_message_push(self):
        chat, _, _, _ = await self.recipient_setup()
        with patch("app.api.messages.deliver_message_push", new_callable=AsyncMock) as deliver:
            status, message = await self.request(f"/chats/{chat.id}/messages", self.admin, "POST", {"content": "Original"})
            self.assertEqual(status, 201)
            deliver.reset_mock()
            path = f"/chats/{chat.id}/messages/{message['id']}"
            self.assertEqual((await self.request(path, self.admin, "PATCH", {"content": "Edited"}))[0], 200)
            self.assertEqual((await self.request(path, self.admin, "DELETE"))[0], 200)
            deliver.assert_not_awaited()


class PushPayloadTests(unittest.TestCase):
    def test_payload_contains_no_message_text_and_unregistered_tokens_are_removed(self):
        import firebase_admin
        from firebase_admin import messaging
        from types import SimpleNamespace
        devices = [("device-one", str(uuid.uuid4()), uuid.uuid4()),
                   ("device-two", str(uuid.uuid4()), uuid.uuid4())]
        chat_id, message_id = uuid.uuid4(), uuid.uuid4()
        responses = SimpleNamespace(responses=[
            SimpleNamespace(success=True, exception=None),
            SimpleNamespace(success=False, exception=messaging.UnregisteredError("gone")),
        ])
        with patch.object(firebase_admin, "get_app"), patch.object(messaging, "send_each", return_value=responses) as send:
            self.assertEqual(push.send_batch(devices, chat_id, message_id), [devices[1]])
        payload = send.call_args.args[0][0]
        self.assertIsNone(payload.notification)
        self.assertEqual(payload.data, {"chat_id": str(chat_id), "message_id": str(message_id),
            "user_id": str(devices[0][2]), "binding": devices[0][1]})
        self.assertEqual(payload.android.priority, "high")
        self.assertEqual(payload.android.ttl, timedelta(hours=1))
