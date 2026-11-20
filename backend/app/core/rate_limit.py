"""In-process sliding-window rate limits for abuse-prone endpoints.

The backend runs as a single uvicorn process, so an in-memory limiter is enough and
needs no extra service. With several workers or replicas each one would enforce the
limits separately; move the counters to a shared store (e.g. Redis) before scaling out.

Client IPs come from `request.client.host`. Behind Caddy this is the real client
address only when uvicorn trusts Caddy's X-Forwarded-For (see compose.yml); otherwise
every request appears to come from the proxy and per-IP limits act as global limits.
"""
import math
import os
import time
from collections import deque

from fastapi import HTTPException, Request, status


ENABLED = os.getenv("RATE_LIMIT_ENABLED", "true").lower() == "true"


class RateLimiter:
    # No limit uses a longer window; older keys can be dropped wholesale.
    MAX_WINDOW = 3600.0

    def __init__(self) -> None:
        self._hits: dict[str, deque[float]] = {}
        self._records = 0

    def reset(self) -> None:
        self._hits.clear()

    def _sweep(self, now: float) -> None:
        """Forgets keys idle for longer than any window (bounds memory under key churn)."""
        for key in [key for key, hits in self._hits.items() if not hits or hits[-1] <= now - self.MAX_WINDOW]:
            del self._hits[key]

    def _prune(self, key: str, window: float, now: float) -> int:
        """Drops hits older than `window`; returns how many remain."""
        hits = self._hits.get(key)
        if hits is None:
            return 0
        while hits and hits[0] <= now - window:
            hits.popleft()
        if not hits:
            # Keep the table from growing with one-off keys.
            del self._hits[key]
            return 0
        return len(hits)

    def retry_after(self, key: str, limit: int, window: float) -> int | None:
        """Seconds until `key` may act again, or None if it is under `limit`."""
        now = time.monotonic()
        if self._prune(key, window, now) >= limit:
            return max(1, math.ceil(self._hits[key][0] + window - now))
        return None

    def record(self, key: str) -> None:
        now = time.monotonic()
        self._records += 1
        if self._records % 1000 == 0:
            self._sweep(now)
        self._hits.setdefault(key, deque()).append(now)

    def hit(self, key: str, limit: int, window: float) -> int | None:
        """Count one attempt; returns the retry delay when the limit is exceeded."""
        retry = self.retry_after(key, limit, window)
        if retry is None:
            self.record(key)
        return retry


limiter = RateLimiter()


def too_many(retry_after: int) -> HTTPException:
    return HTTPException(
        status_code=status.HTTP_429_TOO_MANY_REQUESTS,
        detail="Too many attempts. Please try again later.",
        headers={"Retry-After": str(retry_after)},
    )


def client_ip(request: Request) -> str:
    return request.client.host if request.client else "unknown"


def enforce(key: str, limit: int, window: float) -> None:
    if not ENABLED:
        return
    retry = limiter.hit(key, limit, window)
    if retry is not None:
        raise too_many(retry)


def limit_by_ip(scope: str, limit: int, window: float):
    """FastAPI dependency: at most `limit` requests per client IP per `window` seconds."""

    async def dependency(request: Request) -> None:
        enforce(f"{scope}:ip:{client_ip(request)}", limit, window)

    return dependency
