"""Last-seen bookkeeping and privacy rules.

A user's last-seen time is visible to a viewer only when both share it
("everyone"): hiding your own last seen also hides everyone else's from you.
Presence itself only ever reaches users who share a chat with the target.
"""
import uuid
from datetime import datetime, timezone

from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.user import User
from app.services.blocks import blocked_peer_ids

LAST_SEEN_VISIBILITY = ("everyone", "nobody")


def iso(value: datetime | None) -> str | None:
    return value.isoformat() if value else None


async def mark_seen(db: AsyncSession, user_id: uuid.UUID) -> datetime:
    now = datetime.now(timezone.utc)
    await db.execute(update(User).where(User.id == user_id).values(last_seen_at=now))
    await db.commit()
    return now


async def get_visibility(db: AsyncSession, user_ids: list[uuid.UUID]) -> dict[uuid.UUID, str]:
    if not user_ids:
        return {}
    result = await db.execute(select(User.id, User.last_seen_visibility).where(User.id.in_(user_ids)))
    return dict(result.all())


async def visible_last_seen(
    db: AsyncSession,
    viewer_id: uuid.UUID,
    target_ids: list[uuid.UUID],
) -> dict[uuid.UUID, datetime]:
    """Last-seen times of `target_ids` that `viewer_id` is allowed to see."""
    if not target_ids:
        return {}
    viewer = await db.get(User, viewer_id)
    if viewer is None or viewer.last_seen_visibility != "everyone":
        return {}
    result = await db.execute(
        select(User.id, User.last_seen_at).where(
            User.id.in_(target_ids),
            User.last_seen_visibility == "everyone",
            User.last_seen_at.is_not(None),
        )
    )
    return dict(result.all())


async def peer_user_ids(db: AsyncSession, user_id: uuid.UUID) -> list[uuid.UUID]:
    from app.models.chat import ChatMember

    chat_ids = select(ChatMember.chat_id).where(ChatMember.user_id == user_id)
    result = await db.execute(
        select(ChatMember.user_id).where(
            ChatMember.chat_id.in_(chat_ids),
            ChatMember.user_id != user_id,
        )
    )
    return list(set(result.scalars().all()) - await blocked_peer_ids(db, user_id))


async def presence_snapshot(db: AsyncSession, user_id: uuid.UUID) -> dict:
    from app.realtime.manager import manager

    peers = await peer_user_ids(db, user_id)
    online = [peer for peer in peers if manager.is_online(peer)]
    offline = [peer for peer in peers if not manager.is_online(peer)]
    last_seen = await visible_last_seen(db, user_id, offline)
    return {
        "type": "presence.snapshot",
        "online_user_ids": [str(peer) for peer in online],
        "last_seen": {str(peer): iso(seen) for peer, seen in last_seen.items()},
    }


async def announce_last_seen(db: AsyncSession, user_id: uuid.UUID, event_type: str) -> None:
    """Send `event_type` about `user_id` to each peer, with last_seen_at only where allowed."""
    from app.realtime.manager import manager

    peers = await peer_user_ids(db, user_id)
    if not peers:
        return
    user = await db.get(User, user_id)
    visibility = await get_visibility(db, peers)
    shared = user is not None and user.last_seen_visibility == "everyone"
    for peer in peers:
        allowed = shared and visibility.get(peer) == "everyone"
        await manager.send_to_user(
            peer,
            {
                "type": event_type,
                "user_id": str(user_id),
                "last_seen_at": iso(user.last_seen_at) if allowed else None,
            },
        )
