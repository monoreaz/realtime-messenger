import uuid

from sqlalchemy import select

from app.api.messages import broadcast_message, build_message_response
from app.database import AsyncSessionLocal
from app.models.chat import Chat, ChatMember
from app.models.message import Message


def call_summary(duration: int | None, reason: str) -> str:
    if duration is not None:
        minutes, seconds = divmod(max(0, duration), 60)
        return f"Audio call · {minutes} min {seconds:02d} sec"
    return {
        "declined": "Audio call declined",
        "busy": "Audio call · Busy",
        "cancelled": "Audio call cancelled",
        "failed": "Audio call · Could not connect",
    }.get(reason, "Missed audio call")


async def save_call_message(call_id: str, caller_id: uuid.UUID, recipient_id: uuid.UUID,
                            duration: int | None, reason: str) -> None:
    async with AsyncSessionLocal() as db:
        # Both users must belong to the same private chat.
        chat_id = await db.scalar(
            select(Chat.id).where(
                Chat.type == "private",
                Chat.id.in_(select(ChatMember.chat_id).where(ChatMember.user_id == caller_id)),
                Chat.id.in_(select(ChatMember.chat_id).where(ChatMember.user_id == recipient_id)),
            )
        )
        if chat_id is None:
            return
        # Deterministic ID makes a call history entry idempotent.
        message_id = uuid.uuid5(uuid.NAMESPACE_URL, f"messenger:call:{call_id}")
        if await db.get(Message, message_id) is not None:
            return
        message = Message(id=message_id, chat_id=chat_id, sender_id=caller_id,
                          content=call_summary(duration, reason))
        db.add(message)
        await db.commit()
        await db.refresh(message)
        await broadcast_message(db, chat_id, build_message_response(message))
