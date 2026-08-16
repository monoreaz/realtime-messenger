import uuid

from fastapi import WebSocket


class ConnectionManager:
    def __init__(self):
        self.active_connections: dict[
            uuid.UUID,
            list[WebSocket],
        ] = {}

    def connect(
        self,
        user_id: uuid.UUID,
        websocket: WebSocket,
    ) -> None:
        connections = self.active_connections.setdefault(
            user_id,
            [],
        )

        connections.append(websocket)

    def disconnect(
        self,
        user_id: uuid.UUID,
        websocket: WebSocket,
    ) -> None:
        connections = self.active_connections.get(
            user_id
        )

        if connections is None:
            return

        if websocket in connections:
            connections.remove(websocket)

        if not connections:
            self.active_connections.pop(
                user_id,
                None,
            )

    async def send_to_user(
        self,
        user_id: uuid.UUID,
        data: dict,
    ) -> None:
        connections = list(
            self.active_connections.get(
                user_id,
                [],
            )
        )

        for websocket in connections:
            try:
                await websocket.send_json(data)
            except Exception:
                self.disconnect(
                    user_id,
                    websocket,
                )

    async def send_to_users(
        self,
        user_ids: list[uuid.UUID],
        data: dict,
    ) -> None:
        for user_id in set(user_ids):
            await self.send_to_user(
                user_id,
                data,
            )


manager = ConnectionManager()