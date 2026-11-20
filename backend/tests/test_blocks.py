"""User blocking: the block list API and what a block cuts off, against a rolled-back test database."""
import unittest
import uuid
from contextlib import asynccontextmanager
from unittest.mock import AsyncMock, patch

import test_admin
from test_call_history import Socket
from app.api import messages, ws
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.realtime.manager import manager
from app.services import presence


class BlockTests(unittest.IsolatedAsyncioTestCase):
    asyncTearDown = test_admin.AdminTests.asyncTearDown
    request = test_admin.AdminTests.request

    async def asyncSetUp(self):
        await test_admin.AdminTests.asyncSetUp(self)
        suffix = uuid.uuid4().hex[:12]
        self.alice, self.bob, self.carol = [
            User(username=f"{name}_{suffix}", password_hash="unused") for name in ("alice", "bob", "carol")
        ]
        self.db.add_all([self.alice, self.bob, self.carol])
        await self.db.flush()
        self.private = Chat(type="private", direct_key=":".join(sorted([str(self.alice.id), str(self.bob.id)])))
        self.group = Chat(type="group", name="Group")
        self.db.add_all([self.private, self.group])
        await self.db.flush()
        self.db.add_all([ChatMember(chat_id=self.private.id, user_id=user.id) for user in (self.alice, self.bob)]
                        + [ChatMember(chat_id=self.group.id, user_id=user.id) for user in (self.alice, self.bob, self.carol)])
        await self.db.flush()

        @asynccontextmanager
        async def database():
            yield self.db
        self.enterContext(patch.object(ws, "AsyncSessionLocal", database))
        self.enterContext(patch.object(messages, "deliver_message_push", new_callable=AsyncMock))
        self.sent = self.enterContext(patch.object(manager, "send_to_user", new_callable=AsyncMock))
        self.broadcast = self.enterContext(patch.object(manager, "send_to_users", new_callable=AsyncMock))

    def events(self, event_type):
        return {call.args[0]: call.args[1] for call in self.sent.call_args_list if call.args[1]["type"] == event_type}

    async def send(self, chat, user, text="hi"):
        return (await self.request(f"/chats/{chat.id}/messages", user, "POST", {"content": text}))[0]

    async def test_block_list_endpoints(self):
        path = f"/users/me/blocked/{self.bob.id}"
        self.assertEqual((await self.request("/users/me/blocked"))[0], 401)
        self.assertEqual((await self.request(f"/users/me/blocked/{self.alice.id}", self.alice, "PUT"))[0], 400)
        self.assertEqual((await self.request(f"/users/me/blocked/{uuid.uuid4()}", self.alice, "PUT"))[0], 404)
        self.assertEqual(await self.request("/users/me/blocked", self.alice), (200, []))

        status, blocked = await self.request(path, self.alice, "PUT")
        self.assertEqual(status, 200)
        self.assertEqual((blocked["id"], blocked["username"]), (str(self.bob.id), self.bob.username))
        self.assertIn("blocked_at", blocked)
        self.assertEqual(self.events("blocks.updated").keys(), {self.alice.id})
        self.assertEqual(self.events("presence.snapshot").keys(), {self.alice.id, self.bob.id})

        self.sent.reset_mock()
        self.assertEqual(await self.request(path, self.alice, "PUT"), (200, blocked))
        self.sent.assert_not_called()
        self.assertEqual(await self.request("/users/me/blocked", self.alice), (200, [blocked]))
        # The blocked user cannot see who blocked them.
        self.assertEqual(await self.request("/users/me/blocked", self.bob), (200, []))

        self.assertEqual((await self.request(path, self.alice, "DELETE"))[0], 204)
        self.assertEqual(self.events("blocks.updated").keys(), {self.alice.id})
        self.assertEqual(await self.request("/users/me/blocked", self.alice), (200, []))
        self.sent.reset_mock()
        self.assertEqual((await self.request(path, self.alice, "DELETE"))[0], 204)
        self.sent.assert_not_called()

    async def test_block_stops_private_messages_both_ways_but_not_groups(self):
        self.assertEqual(await self.send(self.private, self.bob), 201)
        await self.request(f"/users/me/blocked/{self.bob.id}", self.alice, "PUT")
        status, body = await self.request(f"/chats/{self.private.id}/messages", self.bob, "POST", {"content": "hi"})
        self.assertEqual((status, body["detail"]), (403, "You can't send messages to this user"))
        status, body = await self.request(f"/chats/{self.private.id}/messages", self.alice, "POST", {"content": "hi"})
        self.assertEqual((status, body["detail"]), (403, "Unblock this user to send messages"))
        self.assertEqual(await self.send(self.group, self.bob), 201)
        await self.request(f"/users/me/blocked/{self.bob.id}", self.alice, "DELETE")
        self.assertEqual(await self.send(self.private, self.bob), 201)

    async def test_blocked_pairs_lose_presence_even_through_groups(self):
        self.assertEqual(set(await presence.peer_user_ids(self.db, self.alice.id)), {self.bob.id, self.carol.id})
        await self.request(f"/users/me/blocked/{self.bob.id}", self.alice, "PUT")
        self.assertEqual(set(await presence.peer_user_ids(self.db, self.alice.id)), {self.carol.id})
        self.assertEqual(set(await presence.peer_user_ids(self.db, self.bob.id)), {self.carol.id})
        self.assertEqual(set(await ws.get_peer_user_ids(self.bob.id)), {self.carol.id})

    async def test_calls_are_refused_and_ended(self):
        await self.request(f"/users/me/blocked/{self.alice.id}", self.bob, "PUT")
        caller, call_id = Socket(), str(uuid.uuid4())
        with patch.object(manager, "is_online", return_value=True):
            await ws.handle_call_signal(caller, self.alice, {
                "type": "call.offer", "call_id": call_id, "peer_id": str(self.bob.id),
                "sdp": {"type": "offer", "sdp": "v=0"},
            })
        self.assertEqual(caller.events, [{"type": "call.end", "call_id": call_id, "reason": "blocked"}])
        self.assertNotIn(call_id, ws.active_calls)

        await self.request(f"/users/me/blocked/{self.alice.id}", self.bob, "DELETE")
        ws.active_calls[call_id] = (self.alice.id, self.bob.id, caller, None)
        with patch.object(ws, "save_call_message", new_callable=AsyncMock), patch.object(ws, "schedule_call_push"):
            await self.request(f"/users/me/blocked/{self.alice.id}", self.bob, "PUT")
        self.assertNotIn(call_id, ws.active_calls)


if __name__ == "__main__":
    unittest.main()
