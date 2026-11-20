"""Regression tests for the 2026-11 security audit (see SECURITY_AUDIT.md).

Each test reproduces a confirmed vulnerability: it fails on the code before the fix
and passes after it. Requests go through the real ASGI app against a rolled-back
test database.
"""
import json
import os
import struct
import unittest
import uuid
import zlib
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import AsyncMock, patch

from fastapi import WebSocketDisconnect
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine

from app.api import auth, messages, ws
from app.core.security import create_access_token, hash_password
from app.database import DATABASE_URL, get_db
from app.main import app
from app.models.chat import Chat, ChatMember
from app.models.message import Message
from app.models.user import User
from app.realtime.manager import manager

# Absent in the code before the fix; the tests still run (and fail) against it.
try:
    from app.core.rate_limit import limiter
except ImportError:
    limiter = None
try:
    from app.core.security import decode_access_claims
except ImportError:
    decode_access_claims = None

PASSWORD = "Synthetic-pass-123"


def png() -> bytes:
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + chunk(b"IEND", b""))


def multipart(content: bytes, content_type: str, filename="file.bin"):
    boundary = uuid.uuid4().hex
    body = (f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n"
            f"Content-Type: {content_type}\r\n\r\n").encode() + content + f"\r\n--{boundary}--\r\n".encode()
    return body, f"multipart/form-data; boundary={boundary}"


class Socket:
    """Stands in for an open WebSocket in the connection manager."""

    def __init__(self):
        self.closed_with = None

    async def send_json(self, data):
        pass

    async def close(self, code=1000):
        self.closed_with = code


class SecurityTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        if limiter is not None:
            limiter.reset()
        self.engine = create_async_engine(DATABASE_URL)
        self.connection = await self.engine.connect()
        self.transaction = await self.connection.begin()
        self.db = AsyncSession(bind=self.connection, expire_on_commit=False, join_transaction_mode="create_savepoint")
        suffix = uuid.uuid4().hex[:10]
        self.a = User(username=f"sec_a_{suffix}", password_hash=hash_password(PASSWORD))
        self.b = User(username=f"sec_b_{suffix}", password_hash=hash_password(PASSWORD))
        self.db.add_all([self.a, self.b])
        await self.db.flush()
        self.chat = Chat(type="private", direct_key=":".join(sorted([str(self.a.id), str(self.b.id)])))
        self.db.add(self.chat)
        await self.db.flush()
        self.db.add_all([ChatMember(chat_id=self.chat.id, user_id=user.id) for user in (self.a, self.b)])
        await self.db.flush()

        async def database():
            yield self.db
        app.dependency_overrides[get_db] = database
        self.enterContext(patch.object(messages, "deliver_message_push", new_callable=AsyncMock))
        self.enterContext(patch.object(manager, "send_to_users", new_callable=AsyncMock))

    async def asyncTearDown(self):
        app.dependency_overrides.clear()
        manager.active_connections.clear()
        getattr(manager, "socket_sessions", {}).clear()
        await self.db.close()
        await self.transaction.rollback()
        await self.connection.close()
        await self.engine.dispose()

    async def call(self, method, path, *, token=None, json_body=None, body=b"", content_type=None,
                   cookie=None, headers=None, chunks=None, client="203.0.113.10"):
        request_headers = list(headers or [])
        if json_body is not None:
            body = json.dumps(json_body).encode()
            content_type = "application/json"
        if content_type:
            request_headers.append((b"content-type", content_type.encode()))
        if token:
            request_headers.append((b"authorization", f"Bearer {token}".encode()))
        if cookie:
            request_headers.append((b"cookie", cookie.encode()))
        if chunks is None:
            request_headers.append((b"content-length", str(len(body)).encode()))
            pending = [body]
        else:
            pending = list(chunks)  # streamed without Content-Length
        events = []

        async def receive():
            if pending:
                part = pending.pop(0)
                return {"type": "http.request", "body": part, "more_body": bool(pending)}
            return {"type": "http.disconnect"}

        async def send(event):
            events.append(event)

        path_only, _, query = path.partition("?")
        await app({
            "type": "http", "asgi": {"version": "3.0"}, "http_version": "1.1", "method": method,
            "scheme": "http", "path": path_only, "raw_path": path_only.encode(), "query_string": query.encode(),
            "root_path": "", "headers": request_headers, "client": (client, 1000), "server": ("test", 80),
        }, receive, send)
        start = next(event for event in events if event["type"] == "http.response.start")
        response_headers = {}
        for name, value in start["headers"]:
            response_headers.setdefault(name.decode().lower(), []).append(value.decode())
        raw = b"".join(event.get("body", b"") for event in events if event["type"] == "http.response.body")
        try:
            data = json.loads(raw) if raw else None
        except ValueError:
            data = raw
        return start["status"], response_headers, data

    async def login(self, user, password=PASSWORD, client="203.0.113.10"):
        body = f"username={user.username}&password={password}".encode()
        status, headers, data = await self.call("POST", "/auth/login", body=body,
                                                content_type="application/x-www-form-urlencoded", client=client)
        if status != 200:
            return status, None, None
        cookie = next(value for value in headers["set-cookie"] if value.startswith(auth.REFRESH_COOKIE_NAME))
        return status, data["access_token"], cookie.split(";")[0]

    # --- Session revocation -------------------------------------------------------

    async def test_logout_revokes_the_access_token(self):
        _, token, cookie = await self.login(self.a)
        self.assertEqual((await self.call("GET", "/users/me", token=token))[0], 200)
        self.assertEqual((await self.call("POST", "/auth/logout", cookie=cookie))[0], 204)
        self.assertEqual((await self.call("GET", "/users/me", token=token))[0], 401)

    async def test_revoking_another_session_revokes_its_token_and_socket(self):
        _, token, cookie = await self.login(self.a)
        _, other_token, _ = await self.login(self.a)
        status, _, sessions = await self.call("GET", "/auth/sessions", token=token, cookie=cookie)
        other = next(session for session in sessions if not session["current"])
        socket = Socket()
        if decode_access_claims is not None:
            manager.connect(self.a.id, socket, decode_access_claims(other_token).session_id)
        else:
            manager.connect(self.a.id, socket)

        self.assertEqual((await self.call("DELETE", f"/auth/sessions/{other['id']}", token=token, cookie=cookie))[0], 204)
        self.assertEqual((await self.call("GET", "/users/me", token=other_token))[0], 401)
        self.assertEqual(socket.closed_with, 1008)
        self.assertEqual((await self.call("GET", "/users/me", token=token))[0], 200)

    async def test_password_change_revokes_tokens_and_closes_sockets(self):
        _, token, cookie = await self.login(self.a)
        _, other_token, _ = await self.login(self.a)
        socket = Socket()
        manager.connect(self.a.id, socket)
        status, _, _ = await self.call("POST", "/auth/change-password", token=token,
                                       json_body={"current_password": PASSWORD, "new_password": "Another-pass-456"})
        self.assertEqual(status, 204)
        self.assertEqual((await self.call("GET", "/users/me", token=other_token))[0], 401)
        self.assertEqual((await self.call("GET", "/users/me", token=token))[0], 401)
        self.assertEqual(socket.closed_with, 1008)

    async def test_websocket_refuses_a_revoked_session(self):
        _, token, cookie = await self.login(self.a)
        await self.call("POST", "/auth/logout", cookie=cookie)
        socket = AsyncMock()
        # Finite script: on vulnerable code the handler would otherwise loop forever.
        socket.receive_json.side_effect = [{"type": "auth", "token": token}, WebSocketDisconnect()]
        context = AsyncMock()
        context.__aenter__.return_value = self.db
        with patch.object(ws, "AsyncSessionLocal", return_value=context), patch.object(manager, "connect") as connect:
            await ws.websocket_endpoint(socket)
        socket.close.assert_awaited_once_with(code=1008)
        connect.assert_not_called()

    async def test_websocket_closes_once_its_token_expired(self):
        expired = create_access_token(str(self.a.id))
        socket = AsyncMock()
        socket.receive_json.side_effect = [{"type": "auth", "token": expired}, {"type": "ping"}, WebSocketDisconnect()]
        context = AsyncMock()
        context.__aenter__.return_value = self.db
        later = datetime.now(timezone.utc) + timedelta(days=1)

        class Clock(datetime):
            @classmethod
            def now(cls, tz=None):
                return later

        with patch.object(ws, "AsyncSessionLocal", return_value=context), patch.object(ws, "datetime", Clock), \
                patch.object(ws, "mark_seen", new_callable=AsyncMock), patch.object(ws, "announce_last_seen", new_callable=AsyncMock):
            await ws.websocket_endpoint(socket)
        socket.close.assert_awaited_with(code=1008)
        self.assertNotIn({"type": "pong"}, [call.args[0] for call in socket.send_json.await_args_list])

    # --- Rate limits ---------------------------------------------------------------

    async def test_login_locks_an_account_after_repeated_failures(self):
        statuses = [(await self.login(self.a, "wrong-password", client=f"198.51.100.{i}"))[0] for i in range(12)]
        self.assertIn(429, statuses)
        # Even the right password is refused while the lockout lasts, from any IP.
        self.assertEqual((await self.login(self.a, client="198.51.100.200"))[0], 429)
        # Other accounts are unaffected.
        self.assertEqual((await self.login(self.b))[0], 200)

    async def test_login_is_limited_per_ip(self):
        # Spread over many usernames, so only the per-IP limit can stop it.
        statuses = [(await self.call("POST", "/auth/login", body=f"username=nobody{i}&password=x".encode(),
                                     content_type="application/x-www-form-urlencoded", client="192.0.2.7"))[0]
                    for i in range(40)]
        self.assertEqual(statuses[0], 401)
        self.assertIn(429, statuses)
        # A different client is not affected.
        self.assertEqual((await self.login(self.b, client="192.0.2.8"))[0], 200)

    async def test_password_reset_email_has_a_per_address_cooldown(self):
        self.a.email = f"{self.a.username}@example.com"
        self.a.email_verified_at = datetime.now(timezone.utc)
        await self.db.flush()
        with patch.object(auth, "send_password_reset_email", new_callable=AsyncMock) as send:
            for i in range(3):
                status, _, _ = await self.call("POST", "/auth/forgot-password", json_body={"email": self.a.email},
                                               client=f"198.51.100.{i}")
                self.assertEqual(status, 204)
        self.assertEqual(send.await_count, 1)

    async def test_message_sending_is_rate_limited(self):
        token = create_access_token(str(self.a.id))
        statuses = [(await self.call("POST", f"/chats/{self.chat.id}/messages", token=token,
                                     json_body={"content": f"m{i}"}))[0] for i in range(40)]
        self.assertEqual(statuses[0], 201)
        self.assertIn(429, statuses)

    # --- Request size --------------------------------------------------------------

    async def test_oversized_anonymous_upload_is_refused_before_parsing(self):
        body, content_type = multipart(b"\0" * (12 * 1024 * 1024), "image/png")
        status, _, _ = await self.call("POST", f"/chats/{uuid.uuid4()}/messages/image", body=body, content_type=content_type)
        self.assertEqual(status, 413)

    async def test_streamed_body_without_length_is_cut_off(self):
        chunks = [b"username=x&password=" + b"y" * 65536] * 8
        status, _, _ = await self.call("POST", "/auth/login", chunks=chunks,
                                       content_type="application/x-www-form-urlencoded")
        self.assertEqual(status, 413)

    # --- Uploads --------------------------------------------------------------------

    async def test_html_disguised_as_an_image_is_rejected(self):
        token = create_access_token(str(self.a.id))
        body, content_type = multipart(b"<html><script>alert(1)</script></html>", "image/png", "x.png")
        status, _, _ = await self.call("POST", f"/chats/{self.chat.id}/messages/image", token=token,
                                       body=body, content_type=content_type)
        self.assertEqual(status, 400)
        body, content_type = multipart(b"<html>not a video</html>", "video/mp4", "x.mp4")
        status, _, _ = await self.call("POST", f"/chats/{self.chat.id}/messages/video", token=token,
                                       body=body, content_type=content_type)
        self.assertEqual(status, 400)

    async def test_deleting_a_message_deletes_its_file(self):
        token = create_access_token(str(self.a.id))
        body, content_type = multipart(png(), "image/png", "pic.png")
        status, _, message = await self.call("POST", f"/chats/{self.chat.id}/messages/image", token=token,
                                             body=body, content_type=content_type)
        self.assertEqual(status, 201)
        path = Path("/app") / message["image_url"].lstrip("/")
        self.assertTrue(path.exists())
        status, headers, _ = await self.call("GET", message["image_url"])
        self.assertEqual(status, 200)
        self.assertEqual(headers.get("x-content-type-options"), ["nosniff"])
        status, _, _ = await self.call("DELETE", f"/chats/{self.chat.id}/messages/{message['id']}", token=token)
        self.assertEqual(status, 200)
        self.assertFalse(path.exists())
        self.assertEqual((await self.call("GET", message["image_url"]))[0], 404)

    # --- Misc hardening ---------------------------------------------------------------

    async def test_call_history_entries_cannot_be_rewritten(self):
        record = Message(id=uuid.uuid5(uuid.NAMESPACE_URL, uuid.uuid4().hex), chat_id=self.chat.id,
                         sender_id=self.a.id, content='{"type":"call","status":"missed"}')
        self.db.add(record)
        await self.db.flush()
        status, _, _ = await self.call("PATCH", f"/chats/{self.chat.id}/messages/{record.id}",
                                       token=create_access_token(str(self.a.id)), json_body={"content": "forged"})
        self.assertEqual(status, 403)

    async def test_no_cross_origin_access_for_development_origins(self):
        for origin in ("http://localhost:5173", "http://192.168.0.111:5173", "https://evil.example"):
            _, headers, _ = await self.call("OPTIONS", "/auth/refresh", headers=[
                (b"origin", origin.encode()), (b"access-control-request-method", b"POST")])
            self.assertNotIn("access-control-allow-origin", headers, origin)

    async def test_api_docs_are_not_public(self):
        for path in ("/docs", "/redoc", "/openapi.json"):
            self.assertEqual((await self.call("GET", path))[0], 404, path)


if __name__ == "__main__":
    unittest.main()
