import asyncio
import json
import uuid
from time import monotonic
from datetime import datetime, timezone

from app.realtime.call_history import save_call_message

from fastapi import (
    APIRouter,
    WebSocket,
    WebSocketDisconnect,
    status,
)
from jwt.exceptions import InvalidTokenError
from sqlalchemy import select

from app.core.security import decode_access_token
from app.database import AsyncSessionLocal
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.realtime.manager import manager


router = APIRouter(
    tags=["realtime"],
)


async def get_peer_user_ids(
    user_id: uuid.UUID,
) -> list[uuid.UUID]:
    async with AsyncSessionLocal() as db:
        chat_result = await db.execute(
            select(ChatMember.chat_id).where(
                ChatMember.user_id == user_id,
            )
        )

        chat_ids = list(
            chat_result.scalars().all()
        )

        if not chat_ids:
            return []

        peer_result = await db.execute(
            select(ChatMember.user_id).where(
                ChatMember.chat_id.in_(chat_ids),
                ChatMember.user_id != user_id,
            )
        )

        return list(
            set(peer_result.scalars().all())
        )


async def get_chat_member_ids(
    chat_id: uuid.UUID,
) -> list[uuid.UUID]:
    async with AsyncSessionLocal() as db:
        result = await db.execute(
            select(ChatMember.user_id).where(
                ChatMember.chat_id == chat_id,
            )
        )

        return list(
            result.scalars().all()
        )


@router.websocket("/ws")
async def websocket_endpoint(
    websocket: WebSocket,
):
    await websocket.accept()

    try:
        auth_message = await asyncio.wait_for(
            websocket.receive_json(),
            timeout=10,
        )

        if auth_message.get("type") != "auth":
            await websocket.close(
                code=status.WS_1008_POLICY_VIOLATION
            )
            return

        token = auth_message.get("token")

        if not isinstance(token, str) or not token:
            await websocket.close(
                code=status.WS_1008_POLICY_VIOLATION
            )
            return

        user_id = decode_access_token(token)

    except WebSocketDisconnect:
        return

    except (
        asyncio.TimeoutError,
        json.JSONDecodeError,
        InvalidTokenError,
        ValueError,
    ):
        await websocket.close(
            code=status.WS_1008_POLICY_VIOLATION
        )
        return

    async with AsyncSessionLocal() as db:
        user = await db.get(
            User,
            user_id,
        )

    if user is None:
        await websocket.close(
            code=status.WS_1008_POLICY_VIOLATION
        )
        return

    became_online = manager.connect(
        user.id,
        websocket,
    )

    peer_user_ids = await get_peer_user_ids(
        user.id
    )

    online_peer_ids = [
        str(peer_id)
        for peer_id in peer_user_ids
        if manager.is_online(peer_id)
    ]

    await websocket.send_json(
        {
            "type": "connection.ready",
            "user_id": str(user.id),
        }
    )

    await websocket.send_json(
        {
            "type": "presence.snapshot",
            "online_user_ids": online_peer_ids,
        }
    )

    if became_online:
        await manager.send_to_users(
            peer_user_ids,
            {
                "type": "presence.online",
                "user_id": str(user.id),
            },
        )

    try:
        while True:
            data = await websocket.receive_json()

            event_type = data.get("type")

            if isinstance(event_type, str) and event_type.startswith("call."):
                await handle_call_signal(websocket, user, data)
                continue

            if event_type == "ping":
                await websocket.send_json(
                    {
                        "type": "pong",
                    }
                )

                continue

            if event_type == "presence.get":
                target_user_id_raw = data.get(
                    "user_id"
                )

                try:
                    target_user_id = uuid.UUID(
                        target_user_id_raw
                    )
                except (
                    TypeError,
                    ValueError,
                    AttributeError,
                ):
                    continue

                peer_user_ids = await get_peer_user_ids(
                    user.id
                )

                if target_user_id not in peer_user_ids:
                    continue

                await websocket.send_json(
                    {
                        "type": "presence.state",
                        "user_id": str(target_user_id),
                        "online": manager.is_online(
                            target_user_id
                        ),
                    }
                )

                continue

            if event_type not in {
                "typing.start",
                "typing.stop",
            }:
                continue

            chat_id_raw = data.get(
                "chat_id"
            )

            try:
                chat_id = uuid.UUID(
                    chat_id_raw
                )
            except (
                TypeError,
                ValueError,
                AttributeError,
            ):
                continue

            member_ids = await get_chat_member_ids(
                chat_id
            )

            if user.id not in member_ids:
                continue

            recipient_ids = [
                member_id
                for member_id in member_ids
                if member_id != user.id
            ]

            await manager.send_to_users(
                recipient_ids,
                {
                    "type": event_type,
                    "chat_id": str(chat_id),
                    "user_id": str(user.id),
                },
            )

    except WebSocketDisconnect:
        pass

    finally:
        for call_id, call in list(active_calls.items()):
            if websocket in (call[2], call[3]):
                await finish_call(call_id, "disconnected")
        became_offline = manager.disconnect(
            user.id,
            websocket,
        )

        if became_offline:
            peer_user_ids = await get_peer_user_ids(
                user.id
            )

            await manager.send_to_users(
                peer_user_ids,
                {
                    "type": "presence.offline",
                    "user_id": str(user.id),
                },
            )

# Signaling is scoped to the authenticated participants and the accepting tab.
active_calls: dict[str, tuple[uuid.UUID, uuid.UUID, WebSocket, WebSocket | None]] = {}


call_started: dict[str, tuple[float, str]] = {}


async def finish_call(call_id: str, reason: str) -> None:
    call = active_calls.pop(call_id, None)
    started = call_started.pop(call_id, None)
    if call is None:
        return
    duration = max(0, int(monotonic() - started[0])) if started else None
    await manager.send_to_users(list(call[:2]), {
        "type": "call.end", "call_id": call_id, "reason": reason,
        "duration_seconds": duration,
    })
    await save_call_message(call_id, call[0], call[1], duration, reason)


async def expire_call(call_id: str) -> None:
    await asyncio.sleep(65)
    call = active_calls.get(call_id)
    if call and call_id not in call_started:
        await finish_call(call_id, "timeout")


async def handle_call_signal(websocket: WebSocket, user: User, data: dict) -> None:
    event = data.get("type")
    if event not in {"call.offer", "call.answer", "call.ice", "call.end", "call.connected"}:
        return
    try:
        call_id = str(uuid.UUID(data.get("call_id")))
        peer_id = uuid.UUID(data.get("peer_id"))
    except (ValueError, TypeError, AttributeError):
        return
    if peer_id == user.id:
        return
    if event in {"call.offer", "call.answer"}:
        sdp = data.get("sdp")
        if not isinstance(sdp, dict) or sdp.get("type") != ("offer" if event == "call.offer" else "answer"):
            return
        if not isinstance(sdp.get("sdp"), str) or len(sdp["sdp"]) > 65536:
            return
    if event == "call.ice":
        candidate = data.get("candidate")
        if not isinstance(candidate, dict) or len(json.dumps(candidate)) > 8192:
            return
    call = active_calls.get(call_id)
    if event == "call.offer":
        if call:
            return
        # Sharing a group does not authorize a private audio call.
        direct_key = ":".join(sorted([str(user.id), str(peer_id)]))
        async with AsyncSessionLocal() as db:
            private_chat = await db.scalar(select(Chat.id).where(
                Chat.type == "private", Chat.direct_key == direct_key,
            ))
        if private_chat is None:
            return
        reason = "offline" if not manager.is_online(peer_id) else None
        if any(user.id in item[:2] or peer_id in item[:2] for item in active_calls.values()):
            reason = "busy"
        if reason:
            await websocket.send_json({"type": "call.end", "call_id": call_id, "reason": reason})
            return
        active_calls[call_id] = (user.id, peer_id, websocket, None)
        asyncio.create_task(expire_call(call_id))
    else:
        if not call or {user.id, peer_id} != set(call[:2]):
            return
        if user.id == call[0] and websocket is not call[2]:
            return
        if user.id == call[1] and call[3] is not None and websocket is not call[3]:
            return
        if event == "call.answer":
            if user.id != call[1] or call[3] is not None:
                return
            active_calls[call_id] = (call[0], call[1], call[2], websocket)
            for other in list(manager.active_connections.get(user.id, [])):
                if other is not websocket:
                    await other.send_json({"type": "call.end", "call_id": call_id})
        if event == "call.connected":
            if call[3] is None:
                return
            if call_id not in call_started:
                call_started[call_id] = (monotonic(), datetime.now(timezone.utc).isoformat())
            payload = {"type": "call.started", "call_id": call_id, "started_at": call_started[call_id][1]}
            await call[2].send_json(payload)
            await call[3].send_json(payload)
            return
        if event == "call.end":
            reason = data.get("reason")
            if reason not in {"busy", "failed", "timeout"}:
                reason = "declined" if user.id == call[1] and call[3] is None else "cancelled"
            await finish_call(call_id, reason)
            return
    payload = {"type": event, "call_id": call_id, "user_id": str(user.id)}
    if event == "call.offer":
        payload["name"] = user.display_name or user.username
    for key in ("sdp", "candidate", "reason"):
        if key in data:
            payload[key] = data[key]
    call = active_calls.get(call_id)
    target = None if not call else (call[2] if peer_id == call[0] else call[3])
    if target is not None:
        await target.send_json(payload)
    else:
        await manager.send_to_user(peer_id, payload)
    if event == "call.end":
        await manager.send_to_user(user.id, payload)
