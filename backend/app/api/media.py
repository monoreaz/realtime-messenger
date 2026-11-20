"""Uploaded files, served only to users who may see them.

Replaces the former public StaticFiles mount at the same URLs (`/uploads/...`), so stored
URLs and clients keep working. Every request is authenticated and then authorized for the
specific file:

- message attachments: the file must belong to a message that is not deleted, in a chat
  the requesting user is a member of (reply previews and thumbnails use the same URL, so
  the same check covers them);
- avatars: the file must be someone's current avatar; any signed-in user may see it, as
  profiles (and their photos) are visible to every signed-in user.

Browsers load media with <img>/<video> tags, which cannot send an Authorization header,
so the HttpOnly refresh cookie (same-origin, SameSite=Lax, never sent on cross-site
subresource requests) is accepted as well as a Bearer token. Range requests are served
by FileResponse after the same checks.
"""
import re
from datetime import datetime, timezone
from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException, Request, status
from fastapi.responses import FileResponse
from jwt.exceptions import InvalidTokenError
from sqlalchemy import or_, select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import REFRESH_COOKIE_NAME, hash_raw_token
from app.core.dependencies import session_is_active
from app.core.security import decode_access_claims
from app.database import get_db
from app.models.auth_session import AuthSession
from app.models.chat import ChatMember
from app.models.message import Message
from app.models.user import User

router = APIRouter(tags=["media"])

UPLOAD_ROOT = Path("/app/uploads")
MEDIA_TYPES = {
    ".jpg": "image/jpeg",
    ".png": "image/png",
    ".webp": "image/webp",
    ".mp4": "video/mp4",
    ".webm": "video/webm",
}
# Server-generated names ("<uuid>-<hex>.<ext>"); the pattern is deliberately loose so older
# naming schemes keep working - access is decided by the database lookup, not the name.
FILENAME = re.compile(r"^[A-Za-z0-9_-]{1,128}\.(jpg|png|webp|mp4|webm)$")

NOT_FOUND = HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Not found")


async def media_user(request: Request, db: AsyncSession) -> User:
    """The signed-in user, from a Bearer access token or the refresh-session cookie."""
    unauthorized = HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED,
        detail="Could not validate credentials",
        headers={"WWW-Authenticate": "Bearer"},
    )
    authorization = request.headers.get("authorization", "")
    user = None
    if authorization.lower().startswith("bearer "):
        try:
            claims = decode_access_claims(authorization[7:].strip())
        except (InvalidTokenError, ValueError):
            raise unauthorized
        user = await db.get(User, claims.user_id)
        if user is not None and claims.session_id is not None and not await session_is_active(db, claims.session_id, user.id):
            user = None
    elif raw := request.cookies.get(REFRESH_COOKIE_NAME):
        session = await db.scalar(select(AuthSession).where(
            AuthSession.token_hash == hash_raw_token(raw),
            AuthSession.revoked_at.is_(None),
            AuthSession.expires_at > datetime.now(timezone.utc),
        ))
        if session is not None:
            user = await db.get(User, session.user_id)
    if user is None or user.is_blocked:
        raise unauthorized
    return user


async def may_see(db: AsyncSession, user: User, folder: str, url: str) -> bool:
    if folder == "messages":
        chat_id = await db.scalar(
            select(Message.chat_id)
            .join(ChatMember, (ChatMember.chat_id == Message.chat_id) & (ChatMember.user_id == user.id))
            .where(or_(Message.image_url == url, Message.video_url == url), Message.deleted_at.is_(None))
            .limit(1)
        )
        return chat_id is not None
    if folder == "avatars":
        return await db.scalar(select(User.id).where(User.avatar_url == url).limit(1)) is not None
    return False


@router.get("/uploads/{folder}/{filename}")
async def get_upload(folder: str, filename: str, request: Request, db: AsyncSession = Depends(get_db)):
    user = await media_user(request, db)
    if folder not in ("messages", "avatars") or not FILENAME.match(filename):
        raise NOT_FOUND
    url = f"/uploads/{folder}/{filename}"
    # Not found and not allowed look the same, so file names cannot be probed.
    if not await may_see(db, user, folder, url):
        raise NOT_FOUND
    path = UPLOAD_ROOT / folder / filename
    if not path.is_file():
        raise NOT_FOUND
    return FileResponse(
        path,
        media_type=MEDIA_TYPES[path.suffix],
        headers={
            # Per-user content: browsers may keep it briefly, shared caches must not.
            "Cache-Control": "private, max-age=300",
            "Content-Disposition": "inline",
        },
    )

