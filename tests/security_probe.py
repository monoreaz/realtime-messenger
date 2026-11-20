"""End-to-end security probe (see SECURITY_AUDIT.md).

Run it ONLY against an isolated test stack with synthetic users usera/userb/userc
(password below) - never against production or real accounts. It logs in, creates chats,
uploads files and makes failed logins on purpose.

    BASE=http://127.0.0.1:8080 python security_probe.py [all|authz|uploads|sessions|ratelimit|proxy|limits|web]

Each check prints PASS (secure behaviour) or VULN (insecure behaviour observed).
Requires PyJWT and websockets (both in backend/requirements.txt).
"""
import asyncio
import io
import json
import os
import struct
import sys
import time
import urllib.error
import urllib.request
import uuid
import zlib
from datetime import datetime, timedelta, timezone

import jwt
import websockets

BASE = os.environ["BASE"]
API = BASE + "/api"
PASSWORD = "Synthetic-pass-123"
results = []


def report(name, secure, detail=""):
    results.append((name, secure))
    print(f"{'PASS' if secure else 'VULN'}  {name}  {detail}")


def http(method, path, token=None, body=None, headers=None, cookie=None, raw=None, ctype=None):
    url = path if path.startswith("http") else API + path
    data = raw
    hdrs = dict(headers or {})
    if body is not None:
        data = json.dumps(body).encode()
        hdrs["Content-Type"] = "application/json"
    if ctype:
        hdrs["Content-Type"] = ctype
    if token:
        hdrs["Authorization"] = f"Bearer {token}"
    if cookie:
        hdrs["Cookie"] = cookie
    request = urllib.request.Request(url, data=data, method=method, headers=hdrs)
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, {k.lower(): v for k, v in response.headers.items()}, response.read()
    except urllib.error.HTTPError as error:
        return error.code, {k.lower(): v for k, v in error.headers.items()}, error.read()


def login(username, remember=False):
    form = f"username={username}&password={PASSWORD}" + ("&remember_me=true" if remember else "")
    status, headers, body = http("POST", "/auth/login", raw=form.encode(), ctype="application/x-www-form-urlencoded")
    assert status == 200, (status, body)
    cookie = headers.get("set-cookie", "").split(";")[0]
    return json.loads(body)["access_token"], cookie


def png_bytes():
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = b"\x00\xff\x00\x00"
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def multipart(field, filename, content, content_type, extra=None):
    boundary = uuid.uuid4().hex
    parts = []
    for key, value in (extra or {}).items():
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n".encode())
    parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n".encode() + content + b"\r\n")
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


async def ws_connect(token):
    socket = await websockets.connect(BASE.replace("http", "ws") + "/ws", open_timeout=10)
    await socket.send(json.dumps({"type": "auth", "token": token}))
    first = json.loads(await asyncio.wait_for(socket.recv(), 5))
    assert first["type"] == "connection.ready", first
    return socket


async def ws_alive(socket, wait=2.0):
    """True if the server keeps the socket open and still answers a ping."""
    try:
        await socket.send(json.dumps({"type": "ping"}))
        deadline = time.monotonic() + wait
        while time.monotonic() < deadline:
            message = json.loads(await asyncio.wait_for(socket.recv(), deadline - time.monotonic()))
            if message.get("type") == "pong":
                return True
        return False
    except Exception:
        return False


async def main():
    section = sys.argv[1] if len(sys.argv) > 1 else "all"
    token_a, cookie_a = login("usera")
    token_b, cookie_b = login("userb")
    me_a = json.loads(http("GET", "/users/me", token_a)[2])
    me_b = json.loads(http("GET", "/users/me", token_b)[2])
    me_c = json.loads(http("GET", "/users/me", login("userc")[0])[2])

    # B and C have a private chat with an image; A is not a member.
    token_c, _ = login("userc")
    chat_bc = json.loads(http("POST", f"/chats/private/{me_c['id']}", token_b)[2])
    status, _, body = http("POST", f"/chats/{chat_bc['id']}/messages", token_b, {"content": "private text for C"})
    secret_message = json.loads(body)
    data, ctype = multipart("file", "pic.png", png_bytes(), "image/png")
    status, _, body = http("POST", f"/chats/{chat_bc['id']}/messages/image", token_b, raw=data, ctype=ctype)
    image_message = json.loads(body)

    if section in ("all", "authz"):
        report("unauthenticated /chats is rejected", http("GET", "/chats")[0] == 401)
        expired = jwt.encode({"sub": me_a["id"], "iat": datetime.now(timezone.utc) - timedelta(hours=2),
                              "exp": datetime.now(timezone.utc) - timedelta(hours=1)}, "wrong-or-unknown", algorithm="HS256")
        report("expired/foreign-signed token is rejected", http("GET", "/chats", expired)[0] == 401)
        none_alg = jwt.encode({"sub": me_a["id"]}, None, algorithm="none")
        report("alg=none token is rejected", http("GET", "/chats", none_alg)[0] == 401)
        report("A cannot list B/C chat messages", http("GET", f"/chats/{chat_bc['id']}/messages", token_a)[0] == 404)
        report("A cannot send into B/C chat", http("POST", f"/chats/{chat_bc['id']}/messages", token_a, {"content": "x"})[0] == 404)
        report("A cannot read B/C pinned messages", http("GET", f"/chats/{chat_bc['id']}/pinned-messages", token_a)[0] == 404)
        report("A cannot edit B's message", http("PATCH", f"/chats/{chat_bc['id']}/messages/{secret_message['id']}", token_a, {"content": "x"})[0] in (403, 404))
        report("A cannot delete B's message", http("DELETE", f"/chats/{chat_bc['id']}/messages/{secret_message['id']}", token_a)[0] in (403, 404))
        report("A cannot pin in B/C chat", http("POST", f"/chats/{chat_bc['id']}/messages/{secret_message['id']}/pin", token_a)[0] == 404)
        report("A cannot mark B/C chat read", http("POST", f"/chats/{chat_bc['id']}/read", token_a)[0] == 404)
        report("A cannot edit group it is not in", http("PATCH", f"/chats/{chat_bc['id']}/group", token_a, {"name": "x"})[0] == 404)
        # Reply to a message of another chat must not leak it.
        chat_ab = json.loads(http("POST", f"/chats/private/{me_b['id']}", token_a)[2])
        status, _, _ = http("POST", f"/chats/{chat_ab['id']}/messages", token_a, {"content": "x", "reply_to_message_id": secret_message["id"]})
        report("cross-chat reply reference is rejected", status == 400)
        report("non-admin cannot use admin API", http("GET", "/admin/users", token_a)[0] == 403)
        status, _, body = http("PATCH", "/users/me", token_a, {"username": "usera", "is_admin": True})
        report("is_admin is not client-settable", status == 200 and json.loads(body)["is_admin"] is False)
        status, _, body = http("GET", "/users/search?username=user", token_a)
        report("user search does not expose email/password", status == 200 and all("email" not in u and "password_hash" not in u for u in json.loads(body)))
        socket = await ws_connect(token_a)
        await socket.send(json.dumps({"type": "typing.start", "chat_id": chat_bc["id"]}))
        await socket.send(json.dumps({"type": "presence.get", "user_id": me_c["id"]}))
        leaked = False
        try:
            while True:
                event = json.loads(await asyncio.wait_for(socket.recv(), 1.5))
                if event.get("type") == "presence.state":
                    leaked = True
        except asyncio.TimeoutError:
            pass
        report("WS presence.get for a non-peer reveals nothing", not leaked)
        await socket.close()

    if section in ("all", "uploads"):
        url = image_message["image_url"]
        status, headers, _ = http("GET", BASE + "/api" + url)
        report("message image requires authentication", status in (401, 403, 404), f"(GET without auth -> {status})")
        status, _, _ = http("GET", BASE + "/api" + url, token_a)
        report("signed-in user outside the chat cannot download its image", status == 404, f"(user A -> {status})")
        status, _, _ = http("GET", BASE + "/api" + url, token_a, headers={"Range": "bytes=0-3"})
        report("...nor a byte range of it", status == 404, f"({status})")
        status, _, body = http("GET", BASE + "/api" + url, token_c, headers={"Range": "bytes=0-3"})
        report("chat member gets the byte range", status == 206 and body == png_bytes()[:4], f"({status})")
        report("uploads send X-Content-Type-Options: nosniff", headers.get("x-content-type-options", "").lower() == "nosniff")
        http("DELETE", f"/chats/{chat_bc['id']}/messages/{image_message['id']}", token_b)
        status, _, _ = http("GET", BASE + "/api" + url, token_b)
        report("deleted message image is no longer served (even to chat members)", status == 404, f"(after delete -> {status})")
        html = b"<html><script>alert(document.domain)</script></html>"
        data, ctype = multipart("file", "x.png", html, "image/png")
        status, _, body = http("POST", f"/chats/{chat_bc['id']}/messages/image", token_b, raw=data, ctype=ctype)
        if status == 201:
            stored = json.loads(body)["image_url"]
            status, headers, _ = http("GET", BASE + "/api" + stored)
            report("HTML disguised as PNG is rejected", False, f"(stored, served as {headers.get('content-type')})")
        else:
            report("HTML disguised as PNG is rejected", True, f"({status})")

    if section in ("all", "sessions"):
        token, cookie = login("usera")
        socket = await ws_connect(token)
        http("POST", "/auth/logout", cookie=cookie)
        report("access token stops working after logout", http("GET", "/users/me", token)[0] == 401)
        report("WebSocket is closed after logout", not await ws_alive(socket))
        await socket.close()

        token, cookie = login("usera")
        other_token, other_cookie = login("usera")
        socket = await ws_connect(other_token)
        sessions = json.loads(http("GET", "/auth/sessions", token, cookie=cookie)[2])
        other = next(s for s in sessions if not s["current"])
        http("DELETE", f"/auth/sessions/{other['id']}", token, cookie=cookie)
        report("revoked session's access token stops working", http("GET", "/users/me", other_token)[0] == 401)
        report("revoked session's WebSocket is closed", not await ws_alive(socket))
        await socket.close()
        http("POST", "/auth/logout", cookie=cookie)

    if section in ("all", "ratelimit"):
        statuses = []
        for _ in range(25):
            form = "username=userc&password=wrong-password-xyz"
            statuses.append(http("POST", "/auth/login", raw=form.encode(), ctype="application/x-www-form-urlencoded")[0])
        report("login is rate limited (25 wrong passwords)", 429 in statuses, f"(statuses: {sorted(set(statuses))})")
        statuses = [http("POST", "/auth/forgot-password", body={"email": "nobody@example.com"})[0] for _ in range(15)]
        report("forgot-password is rate limited (15 requests)", 429 in statuses, f"(statuses: {sorted(set(statuses))})")

    if section in ("all", "proxy"):
        # Simulates tailscale serve (a trusted local proxy) forwarding one real client
        # (100.64.0.9, a tailnet address) that prepends forged X-Forwarded-For values.
        statuses = []
        for i in range(40):
            form = f"username=ghost{i}&password=x"
            spoof = {"X-Forwarded-For": f"203.0.113.{i}, 100.64.0.9"}
            statuses.append(http("POST", "/auth/login", raw=form.encode(), ctype="application/x-www-form-urlencoded", headers=spoof)[0])
        report("forged X-Forwarded-For prefixes cannot dodge the per-IP login limit", 429 in statuses, f"(statuses: {sorted(set(statuses))})")
        other = {"X-Forwarded-For": "100.64.0.10"}
        status = http("POST", "/auth/login", raw=b"username=ghostx&password=x", ctype="application/x-www-form-urlencoded", headers=other)[0]
        report("another client IP is not affected by that limit", status == 401, f"({status})")

    if section in ("all", "limits"):
        size = 40 * 1024 * 1024
        data, ctype = multipart("file", "big.png", b"\0" * size, "image/png")
        started = time.monotonic()
        try:
            status = http("POST", f"/chats/{uuid.uuid4()}/messages/image", raw=data, ctype=ctype)[0]
        except urllib.error.URLError as error:
            # The server answered 413 and closed before the client finished sending.
            status = f"connection closed early ({type(error.reason).__name__})"
        report("oversized unauthenticated upload refused without reading it", status == 413 or "closed early" in str(status),
               f"({status} after {time.monotonic() - started:.2f}s for 40 MB)")
        try:
            status = http("POST", "/auth/login", raw=b"x" * (2 * 1024 * 1024), ctype="application/x-www-form-urlencoded")[0]
        except urllib.error.URLError as error:
            status = f"connection closed early ({type(error.reason).__name__})"
        report("2 MB login body refused (413 / closed before reading)", status == 413 or "closed early" in str(status), f"({status})")

    if section in ("all", "web"):
        status, headers, _ = http("GET", "/chats", token_a, headers={"Origin": "https://evil.example"})
        report("CORS does not allow arbitrary origins", "access-control-allow-origin" not in {k.lower() for k in headers})
        status, headers, _ = http("OPTIONS", "/auth/refresh", headers={"Origin": "http://localhost:5173", "Access-Control-Request-Method": "POST"})
        report("CORS does not allow dev origins with credentials", headers.get("access-control-allow-origin") is None, f"({headers.get('access-control-allow-origin')})")
        status, headers, _ = http("GET", BASE + "/")
        lower = {k.lower(): v for k, v in headers.items()}
        for header in ("content-security-policy", "x-content-type-options", "referrer-policy"):
            report(f"site sends {header}", header in lower)
        report("site forbids framing", "frame-ancestors" in lower.get("content-security-policy", "") or "x-frame-options" in lower)
        report("OpenAPI docs not publicly exposed", http("GET", "/docs")[0] == 404 and http("GET", "/openapi.json")[0] == 404)

    vulnerable = [name for name, secure in results if not secure]
    print(f"\n{len(results) - len(vulnerable)} secure, {len(vulnerable)} vulnerable")


asyncio.run(main())
