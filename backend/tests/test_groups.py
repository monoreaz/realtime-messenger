"""Integration tests against the migrated database, with all writes rolled back."""
import unittest
import uuid
from datetime import datetime, timezone, timedelta
from sqlalchemy import update
from app.models.chat import ChatMember
from unittest.mock import AsyncMock, patch

from fastapi import HTTPException, Response
from pydantic import ValidationError
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.database import DATABASE_URL
from app.models.user import User
from app.api.chats import create_group_chat, create_private_chat, get_chats, mark_chat_read
from app.api.messages import send_message, check_chat_membership
from app.schemas.chat import GroupCreate
from app.schemas.message import MessageCreate


class GroupTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.engine = create_async_engine(DATABASE_URL)
        self.connection = await self.engine.connect()
        self.transaction = await self.connection.begin()
        self.db = AsyncSession(bind=self.connection, expire_on_commit=False, join_transaction_mode="create_savepoint")
        self.users = [User(username="test_" + uuid.uuid4().hex[:20], password_hash="test-only") for _ in range(4)]
        self.db.add_all(self.users)
        await self.db.flush()
        self.broadcast = self.enterContext(patch("app.realtime.manager.manager.send_to_users", new_callable=AsyncMock))

    async def asyncTearDown(self):
        await self.db.close()
        await self.transaction.rollback()
        await self.connection.close()
        await self.engine.dispose()

    async def make_group(self):
        return await create_group_chat(GroupCreate(name="  Team  ", member_ids=[u.id for u in self.users[1:3]]), self.users[0], self.db)

    async def test_group_visible_to_all_members_and_not_outsider(self):
        group = await self.make_group()
        self.assertEqual(group.name, "Team")
        self.assertIsNone(group.peer)
        self.assertEqual({u.id for u in group.members}, {u.id for u in self.users[:3]})
        recipients, event = self.broadcast.call_args.args
        self.assertEqual(set(recipients), {u.id for u in self.users[:3]})
        self.assertEqual(event["type"], "chat.created")
        for user in self.users[:3]:
            self.assertIn(group.id, [chat.id for chat in await get_chats(user, self.db)])
        self.assertNotIn(group.id, [chat.id for chat in await get_chats(self.users[3], self.db)])
        with self.assertRaises(HTTPException) as error:
            await check_chat_membership(self.db, group.id, self.users[3].id)
        self.assertEqual(error.exception.status_code, 404)

    async def test_messages_broadcast_and_unread_are_per_member(self):
        group = await self.make_group()
        # PostgreSQL now() is constant inside the outer rollback transaction.
        # Simulate the earlier read time that separate real requests would have.
        await self.db.execute(update(ChatMember).where(ChatMember.chat_id == group.id).values(
            last_read_at=datetime.now(timezone.utc) - timedelta(minutes=1),
        ))
        message = await send_message(group.id, MessageCreate(content="Hello team"), self.users[0], self.db)
        recipients, event = self.broadcast.call_args.args
        self.assertEqual(set(recipients), {u.id for u in self.users[:3]})
        self.assertEqual(event["message"]["id"], str(message.id))
        for user in self.users[1:3]:
            chats = await get_chats(user, self.db)
            self.assertEqual(next(chat for chat in chats if chat.id == group.id).unread_count, 1)
        await mark_chat_read(group.id, self.users[1], self.db)
        chats = await get_chats(self.users[1], self.db)
        self.assertEqual(next(chat for chat in chats if chat.id == group.id).unread_count, 0)
        with self.assertRaises(HTTPException):
            await send_message(group.id, MessageCreate(content="Unauthorized"), self.users[3], self.db)

    async def test_invalid_participants_and_name(self):
        for ids in ([self.users[1].id] * 2, [self.users[0].id, self.users[1].id], [self.users[1].id, uuid.uuid4()]):
            with self.assertRaises(HTTPException):
                await create_group_chat(GroupCreate(name="Team", member_ids=ids), self.users[0], self.db)
        for name in ("   ", "x" * 101):
            with self.assertRaises(ValidationError):
                GroupCreate(name=name, member_ids=[u.id for u in self.users[1:3]])

    async def test_private_chat_still_has_peer_and_no_duplicates(self):
        await self.make_group()
        chat = await create_private_chat(self.users[1].id, Response(), self.users[0], self.db)
        repeated = await create_private_chat(self.users[1].id, Response(), self.users[0], self.db)
        self.assertEqual(chat.id, repeated.id)
        self.assertEqual(chat.peer.id, self.users[1].id)
        chats = await get_chats(self.users[0], self.db)
        self.assertEqual(len(chats), 2)
