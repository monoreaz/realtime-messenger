import uuid

from fastapi import WebSocket


class ConnectionManager:
    def __init__(self):
        self.active_connections: dict[
            uuid.UUID,
            list[WebSocket],
        ] = {}

    def is_online(
        self,
        user_id: uuid.UUID,
    ) -> bool:
        return bool(
            self.active_connections.get(user_id)
        )

    def connect(
        self,
        user_id: uuid.UUID,
        websocket: WebSocket,
    ) -> bool:
        was_offline = not self.is_online(user_id)

        connections = self.active_connections.setdefault(
            user_id,
            [],
        )

        connections.append(websocket)

        return was_offline

    def disconnect(
        self,
        user_id: uuid.UUID,
        websocket: WebSocket,
    ) -> bool:
        connections = self.active_connections.get(
            user_id
        )

        if connections is None:
            return False

        if websocket in connections:
            connections.remove(websocket)

        if connections:
            return False

        self.active_connections.pop(
            user_id,
            None,
        )

        return True

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