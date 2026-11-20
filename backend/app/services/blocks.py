"""User-to-user blocking.

A block cuts private contact in both directions: neither user can send the
other private messages, calls or typing indicators, and they stop seeing each
other's online status and last seen. Shared group chats are not affected.
Only the blocker can see or lift the block.
"""
import uuid

from fastapi import HTTPException, status
from sqlalchemy import and_, or_, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.chat import Chat, ChatMember
from app.models.user_block import UserBlock


async def blocked_peer_ids(db: AsyncSession, user_id: uuid.UUID) -> set[uuid.UUID]:
    """Users that `user_id` blocked or was blocked by."""
    result = await db.execute(
        select(UserBlock.blocker_id, UserBlock.blocked_id).where(
            or_(UserBlock.blocker_id == user_id, UserBlock.blocked_id == user_id),
        )
    )
    return {blocked if blocker == user_id else blocker for blocker, blocked in result.all()}


async def block_status(db: AsyncSession, user_id: uuid.UUID, peer_id: uuid.UUID) -> str | None:
    """"blocked" if `user_id` blocked `peer_id`, "blocked_by" for the reverse, else None."""
    blockers = set((await db.execute(
        select(UserBlock.blocker_id).where(or_(
            and_(UserBlock.blocker_id == user_id, UserBlock.blocked_id == peer_id),
            and_(UserBlock.blocker_id == peer_id, UserBlock.blocked_id == user_id),
        ))
    )).scalars().all())
    if user_id in blockers:
        return "blocked"
    return "blocked_by" if blockers else None


async def ensure_can_message(db: AsyncSession, chat_id: uuid.UUID, user_id: uuid.UUID) -> None:
    """Reject sending to a private chat when either side blocked the other."""
    peer_id = await db.scalar(
        select(ChatMember.user_id)
        .join(Chat, Chat.id == ChatMember.chat_id)
        .where(Chat.id == chat_id, Chat.type == "private", ChatMember.user_id != user_id)
    )
    if peer_id is None:
        return
    blocked = await block_status(db, user_id, peer_id)
    if blocked == "blocked":
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="Unblock this user to send messages")
    if blocked == "blocked_by":
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="You can't send messages to this user")
