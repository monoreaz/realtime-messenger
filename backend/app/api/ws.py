import asyncio
import json

from fastapi import (
    APIRouter,
    WebSocket,
    WebSocketDisconnect,
    status,
)
from jwt.exceptions import InvalidTokenError

from app.core.security import decode_access_token
from app.database import AsyncSessionLocal
from app.models.user import User
from app.realtime.manager import manager


router = APIRouter(
    tags=["realtime"],
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

    manager.connect(
        user.id,
        websocket,
    )

    await websocket.send_json(
        {
            "type": "connection.ready",
            "user_id": str(user.id),
        }
    )

    try:
        while True:
            data = await websocket.receive_json()

            if data.get("type") == "ping":
                await websocket.send_json(
                    {
                        "type": "pong",
                    }
                )

    except WebSocketDisconnect:
        manager.disconnect(
            user.id,
            websocket,
        )