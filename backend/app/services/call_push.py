"""Short-lived incoming-call signals and narrowly scoped decline credentials."""
import asyncio
import os
from datetime import datetime, timedelta, timezone

import jwt
from sqlalchemy import delete, select

from app.core.security import JWT_ALGORITHM, JWT_SECRET_KEY
from app.database import AsyncSessionLocal
from app.models.auth_session import AuthSession
from app.models.push_device import PushDevice
from app.models.user import User
from app.services.push import _firebase_lock, _delivery_slots, logger


def eligible_devices(user_id):
    return (select(PushDevice, AuthSession.user_id)
        .join(AuthSession, PushDevice.session_id == AuthSession.id)
        .join(User, User.id == AuthSession.user_id)
        .where(User.id == user_id, User.is_blocked.is_(False),
               PushDevice.call_notifications.is_(True), AuthSession.revoked_at.is_(None),
               AuthSession.expires_at > datetime.now(timezone.utc)))


async def can_receive_call_push(user_id):
    if os.environ.get("PUSH_ENABLED", "false").lower() != "true":
        return False
    async with AsyncSessionLocal() as db:
        return (await db.execute(eligible_devices(user_id).limit(1))).first() is not None


def decline_token(call_id, user_id, session_id, binding, expires_at):
    return jwt.encode({"aud": "messenger-call-decline", "sub": str(user_id),
        "call_id": call_id, "session_id": str(session_id), "binding": binding,
        "exp": expires_at}, JWT_SECRET_KEY, algorithm=JWT_ALGORITHM)


def decode_decline_token(token):
    return jwt.decode(token, JWT_SECRET_KEY, algorithms=[JWT_ALGORITHM],
        audience="messenger-call-decline", options={"require": ["exp", "sub", "call_id", "session_id", "binding", "aud"]})


def send_call_batch(rows, event, call_id, info):
    import firebase_admin
    from firebase_admin import messaging
    now = datetime.now(timezone.utc).timestamp()
    ttl = max(0, min(60, info["expires_at"] - now)) if event == "call.incoming" else 90
    if ttl <= 0:
        return []
    with _firebase_lock:
        try:
            firebase_admin.get_app()
        except ValueError:
            firebase_admin.initialize_app(options={"httpTimeout": 10})
    messages = []
    for device, user_id in rows:
        data = {"type": event, "call_id": call_id, "chat_id": str(info["chat_id"]),
            "peer_id": str(info["caller_id"]), "user_id": str(user_id), "binding": device.binding,
            "expires_at": str(int(info["expires_at"] * 1000))}
        if event == "call.incoming":
            data["caller_name"] = info["name"][:120]
            data["decline_token"] = decline_token(call_id, user_id, device.session_id, device.binding, info["expires_at"])
        messages.append(messaging.Message(token=device.token, data=data,
            android=messaging.AndroidConfig(priority="high", ttl=timedelta(seconds=ttl))))
    response = messaging.send_each(messages)
    invalid = []
    for (device, _), result in zip(rows, response.responses):
        if isinstance(result.exception, messaging.UnregisteredError):
            invalid.append((device.token, device.binding))
        elif not result.success:
            logger.warning("Call push failed: %s", type(result.exception).__name__)
    return invalid


async def deliver_call_push(user_id, event, call_id, info):
    if os.environ.get("PUSH_ENABLED", "false").lower() != "true":
        return
    try:
        async with _delivery_slots, AsyncSessionLocal() as db:
            rows = (await db.execute(eligible_devices(user_id))).all()
            for offset in range(0, len(rows), 100):
                invalid = await asyncio.to_thread(send_call_batch, rows[offset:offset + 100], event, call_id, info)
                for token, binding in invalid:
                    await db.execute(delete(PushDevice).where(PushDevice.token == token, PushDevice.binding == binding))
            await db.commit()
    except Exception as error:
        logger.warning("Call push unavailable: %s", type(error).__name__)
