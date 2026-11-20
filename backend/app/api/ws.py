import asyncio
import json
import uuid
from time import monotonic
from datetime import datetime, timezone

from app.realtime.call_history import save_call_message

from fastapi import (
    APIRouter,
    HTTPException,
    WebSocket,
    WebSocketDisconnect,
    status,
)
from jwt.exceptions import InvalidTokenError
from sqlalchemy import select
from pydantic import BaseModel, Field
from app.models.auth_session import AuthSession
from app.models.push_device import PushDevice
from app.services.call_push import can_receive_call_push, deliver_call_push, decode_decline_token

from app.core.dependencies import session_is_active
from app.core.security import decode_access_claims
from app.database import AsyncSessionLocal
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.realtime.manager import manager
from app.services.blocks import block_status, blocked_peer_ids
from app.services.presence import announce_last_seen, iso, mark_seen, presence_snapshot, visible_last_seen


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
            - await blocked_peer_ids(db, user_id)
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

        claims = decode_access_claims(token)
        user_id = claims.user_id

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
        session_ok = user is not None and (
            claims.session_id is None
            or await session_is_active(db, claims.session_id, user.id)
        )

    if user is None or user.is_blocked or not session_ok:
        await websocket.close(
            code=status.WS_1008_POLICY_VIOLATION
        )
        return

    became_online = manager.connect(
        user.id,
        websocket,
        claims.session_id,
    )

    peer_user_ids = await get_peer_user_ids(
        user.id
    )

    async with AsyncSessionLocal() as db:
        if became_online:
            await mark_seen(db, user.id)
        snapshot = await presence_snapshot(db, user.id)

    await websocket.send_json(
        {
            "type": "connection.ready",
            "user_id": str(user.id),
        }
    )

    await websocket.send_json(snapshot)

    await replay_ringing_calls(websocket, user.id)

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

            # The socket lives only as long as its token and session: an expired token,
            # a revoked session or a blocked account ends it (the client reconnects
            # with a fresh token if it still has a valid session).
            async with AsyncSessionLocal() as db:
                allowed_user = await db.get(User, user.id)
                allowed = (
                    allowed_user is not None
                    and not allowed_user.is_blocked
                    and claims.expires_at > datetime.now(timezone.utc)
                    and (claims.session_id is None or await session_is_active(db, claims.session_id, user.id))
                )
                if not allowed:
                    await websocket.close(code=status.WS_1008_POLICY_VIOLATION)
                    break

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

                async with AsyncSessionLocal() as db:
                    last_seen = await visible_last_seen(
                        db,
                        user.id,
                        [target_user_id],
                    )

                await websocket.send_json(
                    {
                        "type": "presence.state",
                        "user_id": str(target_user_id),
                        "online": manager.is_online(
                            target_user_id
                        ),
                        "last_seen_at": iso(
                            last_seen.get(target_user_id)
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

            blocked_ids: set[uuid.UUID] = set()
            async with AsyncSessionLocal() as db:
                if await db.scalar(select(Chat.type).where(Chat.id == chat_id)) == "private":
                    blocked_ids = await blocked_peer_ids(db, user.id)

            recipient_ids = [
                member_id
                for member_id in member_ids
                if member_id != user.id and member_id not in blocked_ids
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
            async with AsyncSessionLocal() as db:
                await mark_seen(db, user.id)
                await announce_last_seen(
                    db,
                    user.id,
                    "presence.offline",
                )

# Signaling is scoped to the authenticated participants and the accepting tab.
active_calls: dict[str, tuple[uuid.UUID, uuid.UUID, WebSocket, WebSocket | None]] = {}


call_started: dict[str, tuple[float, str]] = {}
ringing_calls: dict[str, dict] = {}
call_push_tasks: set[asyncio.Task] = set()


def schedule_call_push(user_id, event, call_id, info):
    task = asyncio.create_task(deliver_call_push(user_id, event, call_id, dict(info)))
    call_push_tasks.add(task)
    task.add_done_callback(call_push_tasks.discard)


async def replay_ringing_calls(websocket, user_id, requested_id=None):
    found = False
    for call_id, call in list(active_calls.items()):
        info = ringing_calls.get(call_id)
        if (call[1] != user_id or call[3] is not None or info is None
            or (requested_id is not None and requested_id != call_id)):
            continue
        if info["expires_at"] <= datetime.now(timezone.utc).timestamp():
            continue
        found = True
        await websocket.send_json(info["offer"])
        for candidate in info["candidates"]:
            await websocket.send_json(candidate)
    if requested_id is not None and not found:
        await websocket.send_json({"type": "call.end", "call_id": requested_id, "reason": "ended"})


class DeclineCall(BaseModel):
    token: str = Field(min_length=20, max_length=4096)


@router.post("/calls/{call_id}/decline", status_code=204)
async def decline_push_call(call_id: uuid.UUID, body: DeclineCall):
    try:
        claims = decode_decline_token(body.token)
        if claims["call_id"] != str(call_id):
            raise ValueError()
        user_id = uuid.UUID(claims["sub"])
        session_id = uuid.UUID(claims["session_id"])
    except (InvalidTokenError, ValueError, TypeError, KeyError):
        raise HTTPException(status_code=401, detail="Invalid call action")
    async with AsyncSessionLocal() as db:
        device = await db.scalar(select(PushDevice).join(AuthSession).join(User).where(
            PushDevice.session_id == session_id, PushDevice.binding == claims["binding"],
            PushDevice.call_notifications.is_(True),
            AuthSession.user_id == user_id, AuthSession.revoked_at.is_(None),
            AuthSession.expires_at > datetime.now(timezone.utc), User.is_blocked.is_(False),
        ))
    if device is None:
        raise HTTPException(status_code=401, detail="Call session is no longer active")
    call = active_calls.get(str(call_id))
    if call is not None and call[1] == user_id and call[3] is None:
        await finish_call(str(call_id), "declined")



async def finish_call(call_id: str, reason: str) -> None:
    call = active_calls.pop(call_id, None)
    started = call_started.pop(call_id, None)
    info = ringing_calls.pop(call_id, None)
    if call is None:
        return
    if info is not None:
        schedule_call_push(call[1], "call.dismiss", call_id, info)
    duration = max(0, int(monotonic() - started[0])) if started else None
    await manager.send_to_users(list(call[:2]), {
        "type": "call.end", "call_id": call_id, "reason": reason,
        "duration_seconds": duration,
    })
    await save_call_message(call_id, call[0], call[1], duration, reason)


async def end_calls_between(user_id: uuid.UUID, peer_id: uuid.UUID) -> None:
    for call_id, call in list(active_calls.items()):
        if set(call[:2]) == {user_id, peer_id}:
            await finish_call(call_id, "cancelled")


async def expire_call(call_id: str) -> None:
    await asyncio.sleep(65)
    call = active_calls.get(call_id)
    if call and call_id not in call_started:
        await finish_call(call_id, "timeout")


async def handle_call_signal(websocket: WebSocket, user: User, data: dict) -> None:
    event = data.get("type")
    if event == "call.resume":
        try:
            requested_id = str(uuid.UUID(data.get("call_id")))
        except (ValueError, TypeError, AttributeError):
            return
        await replay_ringing_calls(websocket, user.id, requested_id)
        return
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
            blocked = private_chat is not None and await block_status(db, user.id, peer_id) is not None
        if private_chat is None:
            return
        if blocked:
            await websocket.send_json({"type": "call.end", "call_id": call_id, "reason": "blocked"})
            return
        reason = "offline" if not manager.is_online(peer_id) and not await can_receive_call_push(peer_id) else None
        if any(user.id in item[:2] or peer_id in item[:2] for item in active_calls.values()):
            reason = "busy"
        if reason:
            await websocket.send_json({"type": "call.end", "call_id": call_id, "reason": reason})
            return
        active_calls[call_id] = (user.id, peer_id, websocket, None)
        expires_at = datetime.now(timezone.utc).timestamp() + 60
        ringing_calls[call_id] = {"chat_id": private_chat, "caller_id": user.id,
            "name": user.display_name or user.username, "expires_at": expires_at,
            "candidates": [], "offer": {"type": "call.offer", "call_id": call_id,
                "user_id": str(user.id), "name": user.display_name or user.username,
                "sdp": data["sdp"], "expires_at": int(expires_at * 1000)}}
        schedule_call_push(peer_id, "call.incoming", call_id, ringing_calls[call_id])
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
            if call_id in ringing_calls and ringing_calls[call_id]["expires_at"] <= datetime.now(timezone.utc).timestamp():
                await finish_call(call_id, "timeout")
                return
            active_calls[call_id] = (call[0], call[1], call[2], websocket)
            if call_id in ringing_calls:
                schedule_call_push(call[1], "call.dismiss", call_id, ringing_calls[call_id])
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
        payload["expires_at"] = int(ringing_calls[call_id]["expires_at"] * 1000)
        payload["name"] = user.display_name or user.username
    for key in ("sdp", "candidate", "reason"):
        if key in data:
            payload[key] = data[key]
    call = active_calls.get(call_id)
    if event == "call.ice" and call and user.id == call[0] and call[3] is None:
        info = ringing_calls.get(call_id)
        if info is not None and len(info["candidates"]) < 128:
            info["candidates"].append(payload)
    target = None if not call else (call[2] if peer_id == call[0] else call[3])
    if target is not None:
        await target.send_json(payload)
    else:
        await manager.send_to_user(peer_id, payload)
    if event == "call.end":
        await manager.send_to_user(user.id, payload)
