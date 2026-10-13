"""Admin API tests using ASGI requests and rollback-only database transactions."""
import json
import unittest
import uuid
from datetime import datetime, timedelta, timezone
from unittest.mock import AsyncMock, patch
from types import SimpleNamespace
from fastapi import HTTPException, Response

from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.database import DATABASE_URL, get_db
from app.main import app
from app.models.user import User
from app.models.auth_session import AuthSession
from app.core.security import create_access_token, hash_password
from app.api.auth import hash_raw_token, REFRESH_COOKIE_NAME, login
from app.api import ws
from app.realtime.manager import manager


class AdminTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.engine = create_async_engine(DATABASE_URL)
        self.connection = await self.engine.connect()
        self.transaction = await self.connection.begin()
        self.db = AsyncSession(bind=self.connection, expire_on_commit=False, join_transaction_mode="create_savepoint")
        suffix = uuid.uuid4().hex[:12]
        self.admin = User(username=f"admin_{suffix}", password_hash="unused", is_admin=True)
        self.member = User(username=f"member_{suffix}", password_hash=hash_password("test-password"))
        self.db.add_all([self.admin, self.member])
        await self.db.flush()
        async def database():
            yield self.db
        app.dependency_overrides[get_db] = database

    async def asyncTearDown(self):
        app.dependency_overrides.clear()
        await self.db.close()
        await self.transaction.rollback()
        await self.connection.close()
        await self.engine.dispose()

    async def request(self, path, user=None, method="GET", body=None, query="", headers=None):
        payload = b"" if body is None else json.dumps(body).encode()
        request_headers = [(b"content-type", b"application/json")]
        if user:
            request_headers.append((b"authorization", f"Bearer {create_access_token(str(user.id))}".encode()))
        request_headers.extend(headers or [])
        events = []
        async def receive():
            return {"type": "http.request", "body": payload, "more_body": False}
        async def send(event):
            events.append(event)
        await app({
            "type": "http", "asgi": {"version": "3.0"}, "http_version": "1.1",
            "method": method, "scheme": "http", "path": path,
            "raw_path": path.encode(), "query_string": query.encode(),
            "root_path": "", "headers": request_headers,
            "client": ("127.0.0.1", 1000), "server": ("test", 80),
        }, receive, send)
        status = next(item["status"] for item in events if item["type"] == "http.response.start")
        response = b"".join(item.get("body", b"") for item in events if item["type"] == "http.response.body")
        return status, json.loads(response) if response else None

    async def test_admin_routes_reject_anonymous_and_ordinary_users(self):
        for path, method, body in (
            ("/admin/stats", "GET", None),
            ("/admin/users", "GET", None),
            (f"/admin/users/{self.member.id}/block", "PATCH", {"is_blocked": True}),
        ):
            self.assertEqual((await self.request(path, method=method, body=body))[0], 401)
            self.assertEqual((await self.request(path, self.member, method, body))[0], 403)

    async def test_search_pagination_stats_and_no_password_exposure(self):
        status, data = await self.request("/admin/users", self.admin, query=f"q={self.member.username}&limit=1")
        self.assertEqual(status, 200)
        self.assertEqual(data["total"], 1)
        self.assertEqual(data["users"][0]["username"], self.member.username)
        self.assertNotIn("password_hash", data["users"][0])
        status, data = await self.request("/admin/users", self.admin, query=f"q={self.member.username}&offset=1&limit=1")
        self.assertEqual(status, 200)
        self.assertEqual(data["users"], [])
        self.assertEqual(data["total"], 1)
        status, stats = await self.request("/admin/stats", self.admin)
        self.assertEqual(status, 200)
        self.assertGreaterEqual(stats["users"], 2)
        self.assertEqual((await self.request("/admin/users", self.admin, query="limit=10000"))[0], 422)

    async def test_block_revokes_sessions_closes_sockets_and_denies_existing_tokens(self):
        raw_token = uuid.uuid4().hex
        session = AuthSession(user_id=self.member.id, token_hash=hash_raw_token(raw_token),
            expires_at=datetime.now(timezone.utc) + timedelta(days=1))
        self.db.add(session)
        await self.db.flush()
        socket = AsyncMock()
        with patch.dict(manager.active_connections, {self.member.id: [socket]}):
            status, result = await self.request(f"/admin/users/{self.member.id}/block", self.admin, "PATCH", {"is_blocked": True})
        self.assertEqual(status, 200)
        self.assertTrue(result["is_blocked"])
        await self.db.refresh(session)
        self.assertIsNotNone(session.revoked_at)
        socket.close.assert_awaited_once_with(code=1008)
        socket.send_json.assert_awaited_once_with({"type": "account.blocked"})
        self.assertEqual((await self.request("/users/me", self.member))[0], 403)
        status, result = await self.request("/auth/refresh", method="POST", headers=[(b"cookie", f"{REFRESH_COOKIE_NAME}={raw_token}".encode())])
        self.assertEqual(status, 401)
        status, result = await self.request("/admin/users", self.admin, query=f"state=blocked&q={self.member.username}")
        self.assertEqual(result["total"], 1)
        status, result = await self.request(f"/admin/users/{self.member.id}/block", self.admin, "PATCH", {"is_blocked": False})
        self.assertEqual(status, 200)
        self.assertFalse(result["is_blocked"])
        self.assertEqual((await self.request("/users/me", self.member))[0], 200)
        await self.db.refresh(session)
        self.assertIsNotNone(session.revoked_at)

    async def test_admin_protection_and_strict_input(self):
        status, _ = await self.request(f"/admin/users/{self.admin.id}/block", self.admin, "PATCH", {"is_blocked": True})
        self.assertEqual(status, 400)
        other_admin = User(username="other_" + uuid.uuid4().hex[:16], password_hash="unused", is_admin=True)
        self.db.add(other_admin)
        await self.db.flush()
        self.assertEqual((await self.request(f"/admin/users/{other_admin.id}/block", self.admin, "PATCH", {"is_blocked": True}))[0], 400)
        self.assertEqual((await self.request(f"/admin/users/{uuid.uuid4()}/block", self.admin, "PATCH", {"is_blocked": True}))[0], 404)
        self.assertEqual((await self.request(f"/admin/users/{self.member.id}/block", self.admin, "PATCH", {"is_blocked": "false"}))[0], 422)

    async def test_profile_cannot_grant_admin_access(self):
        self.assertFalse(self.member.is_admin)
        await self.request("/users/me", self.member, "PATCH", {"is_admin": True})
        await self.db.refresh(self.member)
        self.assertFalse(self.member.is_admin)

    async def test_blocked_user_cannot_connect_websocket(self):
        self.member.is_blocked = True
        await self.db.flush()
        socket = AsyncMock()
        socket.receive_json.return_value = {"type": "auth", "token": create_access_token(str(self.member.id))}
        context = AsyncMock()
        context.__aenter__.return_value = self.db
        with patch.object(ws, "AsyncSessionLocal", return_value=context), patch.object(manager, "connect") as connect:
            await ws.websocket_endpoint(socket)
        socket.close.assert_awaited_once_with(code=1008)
        connect.assert_not_called()


    async def test_blocked_account_cannot_login_or_refresh_unrevoked_session(self):
        self.member.is_blocked = True
        raw_token = uuid.uuid4().hex
        self.db.add(AuthSession(user_id=self.member.id, token_hash=hash_raw_token(raw_token),
            expires_at=datetime.now(timezone.utc) + timedelta(days=1)))
        await self.db.flush()
        with self.assertRaises(HTTPException) as error:
            await login(SimpleNamespace(headers={}), Response(),
                SimpleNamespace(username=self.member.username, password="test-password"), False, self.db)
        self.assertEqual(error.exception.status_code, 403)
        status, _ = await self.request("/auth/refresh", method="POST",
            headers=[(b"cookie", f"{REFRESH_COOKIE_NAME}={raw_token}".encode())])
        self.assertEqual(status, 403)
