"""Last-seen tracking and its privacy setting, against a rolled-back test database."""
import unittest
import uuid
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock, patch

import test_admin
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.realtime.manager import manager
from app.services import presence


class LastSeenTests(unittest.IsolatedAsyncioTestCase):
    asyncTearDown = test_admin.AdminTests.asyncTearDown
    request = test_admin.AdminTests.request

    async def asyncSetUp(self):
        await test_admin.AdminTests.asyncSetUp(self)
        self.seen = datetime.now(timezone.utc) - timedelta(minutes=5)
        suffix = uuid.uuid4().hex[:12]
        self.alice, self.bob, self.carol, self.stranger = [
            User(username=f"{name}_{suffix}", password_hash="unused", last_seen_at=self.seen)
            for name in ("alice", "bob", "carol", "stranger")
        ]
        self.db.add_all([self.alice, self.bob, self.carol, self.stranger])
        await self.db.flush()
        for peer in (self.bob, self.carol):
            chat = Chat(type="private", direct_key=f"{self.alice.id}:{peer.id}")
            self.db.add(chat)
            await self.db.flush()
            self.db.add_all([ChatMember(chat_id=chat.id, user_id=self.alice.id), ChatMember(chat_id=chat.id, user_id=peer.id)])
        await self.db.flush()
        self.sent = self.enterContext(patch.object(manager, "send_to_user", new_callable=AsyncMock))

    def events(self, event_type):
        return {call.args[0]: call.args[1] for call in self.sent.call_args_list if call.args[1]["type"] == event_type}

    async def test_privacy_endpoint_defaults_validates_and_saves(self):
        self.assertEqual((await self.request("/users/me/privacy"))[0], 401)
        self.assertEqual(await self.request("/users/me/privacy", self.alice), (200, {"last_seen": "everyone"}))
        self.assertEqual((await self.request("/users/me/privacy", self.alice, "PATCH", {"last_seen": "friends"}))[0], 422)
        self.assertEqual(await self.request("/users/me/privacy", self.alice, "PATCH", {"last_seen": "nobody"}), (200, {"last_seen": "nobody"}))
        await self.db.refresh(self.alice)
        self.assertEqual(self.alice.last_seen_visibility, "nobody")
        self.assertEqual(await self.request("/users/me/privacy", self.alice), (200, {"last_seen": "nobody"}))
        # Profile responses never carry last seen; it travels only over presence events.
        status, me = await self.request("/users/me", self.alice)
        self.assertEqual(status, 200)
        self.assertNotIn("last_seen_at", me)
        self.assertNotIn("last_seen_visibility", me)

    async def test_visibility_is_mutual(self):
        targets = [self.bob.id, self.carol.id]
        self.assertEqual(await presence.visible_last_seen(self.db, self.alice.id, targets), {self.bob.id: self.seen, self.carol.id: self.seen})
        self.bob.last_seen_visibility = "nobody"
        await self.db.flush()
        self.assertEqual(await presence.visible_last_seen(self.db, self.alice.id, targets), {self.carol.id: self.seen})
        self.assertEqual(await presence.visible_last_seen(self.db, self.bob.id, [self.alice.id]), {})

    async def test_offline_announcement_respects_each_peer_and_skips_strangers(self):
        self.carol.last_seen_visibility = "nobody"
        await self.db.flush()
        seen = await presence.mark_seen(self.db, self.alice.id)
        await self.db.refresh(self.alice)
        await presence.announce_last_seen(self.db, self.alice.id, "presence.offline")
        events = self.events("presence.offline")
        self.assertEqual(set(events), {self.bob.id, self.carol.id})
        self.assertEqual(events[self.bob.id], {"type": "presence.offline", "user_id": str(self.alice.id), "last_seen_at": seen.isoformat()})
        self.assertIsNone(events[self.carol.id]["last_seen_at"])

    async def test_snapshot_lists_last_seen_only_for_offline_peers(self):
        with patch.object(manager, "is_online", side_effect=lambda user_id: user_id == self.bob.id):
            snapshot = await presence.presence_snapshot(self.db, self.alice.id)
        self.assertEqual(snapshot["online_user_ids"], [str(self.bob.id)])
        self.assertEqual(snapshot["last_seen"], {str(self.carol.id): self.seen.isoformat()})

    async def test_hiding_last_seen_updates_peers_and_own_view(self):
        await self.request("/users/me/privacy", self.alice, "PATCH", {"last_seen": "nobody"})
        updates = self.events("presence.last_seen")
        self.assertEqual(set(updates), {self.bob.id, self.carol.id})
        self.assertTrue(all(event["last_seen_at"] is None for event in updates.values()))
        self.assertEqual(self.events("presence.snapshot")[self.alice.id]["last_seen"], {})

        self.sent.reset_mock()
        await self.request("/users/me/privacy", self.alice, "PATCH", {"last_seen": "everyone"})
        self.assertEqual(self.events("presence.last_seen")[self.bob.id]["last_seen_at"], self.seen.isoformat())
        self.assertEqual(len(self.events("presence.snapshot")[self.alice.id]["last_seen"]), 2)

        self.sent.reset_mock()
        await self.request("/users/me/privacy", self.alice, "PATCH", {"last_seen": "everyone"})
        self.sent.assert_not_called()


if __name__ == "__main__":
    unittest.main()
