"""Best-effort FCM delivery after the message transaction has committed."""
import asyncio
import logging
import os
import threading
from datetime import datetime, timedelta, timezone

from sqlalchemy import delete, select

from app.database import AsyncSessionLocal
from app.models.auth_session import AuthSession
from app.models.chat import ChatMember
from app.models.push_device import PushDevice
from app.models.user import User

logger = logging.getLogger(__name__)
_firebase_lock = threading.Lock()
_delivery_slots = asyncio.Semaphore(4)


def send_batch(devices, chat_id, message_id):
    # Import lazily so existing deployments keep working with push disabled.
    import firebase_admin
    from firebase_admin import messaging

    with _firebase_lock:
        try:
            firebase_admin.get_app()
        except ValueError:
            firebase_admin.initialize_app(options={"httpTimeout": 10})
    messages = [messaging.Message(
        token=token,
        data={"chat_id": str(chat_id), "message_id": str(message_id),
              "user_id": str(user_id), "binding": binding},
        android=messaging.AndroidConfig(priority="high", ttl=timedelta(hours=1)),
    ) for token, binding, user_id in devices]
    result = messaging.send_each(messages)
    invalid = []
    for device, response in zip(devices, result.responses):
        if isinstance(response.exception, messaging.UnregisteredError):
            invalid.append(device)
        elif not response.success:
            # Never log tokens, payloads or credential-bearing exception strings.
            logger.warning("FCM delivery failed: %s", type(response.exception).__name__)
    return invalid


async def deliver_message_push(chat_id, message_id, sender_id):
    if os.environ.get("PUSH_ENABLED", "false").lower() != "true":
        return
    try:
        async with _delivery_slots, AsyncSessionLocal() as db:
            devices = (await db.execute(
                select(PushDevice.token, PushDevice.binding, AuthSession.user_id)
                .join(AuthSession, PushDevice.session_id == AuthSession.id)
                .join(User, User.id == AuthSession.user_id)
                .join(ChatMember, ChatMember.user_id == User.id)
                .where(ChatMember.chat_id == chat_id, User.id != sender_id,
                       User.is_blocked.is_(False), AuthSession.revoked_at.is_(None),
                       AuthSession.expires_at > datetime.now(timezone.utc))
            )).all()
            for start in range(0, len(devices), 100):
                invalid = await asyncio.to_thread(send_batch, devices[start:start + 100], chat_id, message_id)
                for token, binding, _ in invalid:
                    await db.execute(delete(PushDevice).where(
                        PushDevice.token == token, PushDevice.binding == binding,
                    ))
            await db.commit()
    except Exception as error:
        logger.warning("Push delivery unavailable: %s", type(error).__name__)
