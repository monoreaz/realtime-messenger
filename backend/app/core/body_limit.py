"""Request body size limits, enforced before FastAPI parses (and spools) the body.

FastAPI reads form bodies before it resolves dependencies such as authentication, so
without this an anonymous client could make the server buffer an unbounded upload to
disk. Requests over the limit are rejected from Content-Length when present, and
otherwise as soon as the streamed body crosses the limit.
"""
import json
import re

# Each upload's own limit plus room for the multipart envelope and form fields.
_ENVELOPE = 1024 * 1024
LIMITS: tuple[tuple[re.Pattern[str], int], ...] = (
    (re.compile(r"^/chats/[^/]+/messages/video$"), 50 * 1024 * 1024 + _ENVELOPE),
    (re.compile(r"^/chats/[^/]+/messages/image$"), 10 * 1024 * 1024 + _ENVELOPE),
    (re.compile(r"^/users/me/avatar$"), 5 * 1024 * 1024 + _ENVELOPE),
)
DEFAULT_LIMIT = 256 * 1024


def limit_for(path: str) -> int:
    for pattern, limit in LIMITS:
        if pattern.match(path):
            return limit
    return DEFAULT_LIMIT


class _BodyTooLarge(Exception):
    pass


class BodySizeLimitMiddleware:
    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http":
            return await self.app(scope, receive, send)

        limit = limit_for(scope["path"])
        for name, value in scope.get("headers", []):
            if name == b"content-length":
                try:
                    too_large = int(value) > limit
                except ValueError:
                    too_large = True
                if too_large:
                    return await self._reject(send)

        received = 0
        exceeded = False
        response_started = False

        async def limited_receive():
            nonlocal received, exceeded
            message = await receive()
            if message["type"] == "http.request":
                received += len(message.get("body", b""))
                if received > limit:
                    exceeded = True
                    raise _BodyTooLarge()
            return message

        async def guarded_send(message):
            # FastAPI turns errors raised while parsing a body into its own 400
            # response; once the limit was hit, answer 413 instead.
            nonlocal response_started
            if exceeded:
                if message["type"] == "http.response.start" and not response_started:
                    response_started = True
                    await self._reject(send)
                return
            if message["type"] == "http.response.start":
                response_started = True
            await send(message)

        try:
            await self.app(scope, limited_receive, guarded_send)
        except _BodyTooLarge:
            if not response_started:
                await self._reject(send)

    @staticmethod
    async def _reject(send):
        body = json.dumps({"detail": "Request body is too large"}).encode()
        await send({
            "type": "http.response.start",
            "status": 413,
            "headers": [(b"content-type", b"application/json"), (b"content-length", str(len(body)).encode()), (b"connection", b"close")],
        })
        await send({"type": "http.response.body", "body": body})


class SecurityHeadersMiddleware:
    """Headers every API response (including /uploads files) should carry."""

    HEADERS = (
        (b"x-content-type-options", b"nosniff"),
        (b"referrer-policy", b"no-referrer"),
    )

    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http":
            return await self.app(scope, receive, send)

        async def send_with_headers(message):
            if message["type"] == "http.response.start":
                existing = {name.lower() for name, _ in message.get("headers", [])}
                message = dict(message)
                message["headers"] = list(message.get("headers", [])) + [
                    header for header in self.HEADERS if header[0] not in existing
                ]
            await send(message)

        await self.app(scope, receive, send_with_headers)
