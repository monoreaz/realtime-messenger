import asyncio
import json
import uuid

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
from app.models.chat import ChatMember
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