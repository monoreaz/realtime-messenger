from typing import Annotated

from fastapi import Depends, HTTPException, status
from fastapi.security import OAuth2PasswordBearer
from jwt.exceptions import InvalidTokenError
from sqlalchemy.ext.asyncio import AsyncSession

from datetime import datetime, timezone

from app.core.security import decode_access_claims
from app.models.auth_session import AuthSession
from app.database import get_db
from app.models.user import User


oauth2_scheme = OAuth2PasswordBearer(
    tokenUrl="/auth/login"
)


async def session_is_active(db: AsyncSession, session_id, user_id) -> bool:
    """The refresh session still exists for this user and is neither revoked nor expired."""
    session = await db.get(AuthSession, session_id)
    return (
        session is not None
        and session.user_id == user_id
        and session.revoked_at is None
        and session.expires_at > datetime.now(timezone.utc)
    )


async def get_current_user(
    token: Annotated[str, Depends(oauth2_scheme)],
    db: AsyncSession = Depends(get_db),
) -> User:
    credentials_exception = HTTPException(
        status_code=status.HTTP_401_UNAUTHORIZED,
        detail="Could not validate credentials",
        headers={
            "WWW-Authenticate": "Bearer",
        },
    )

    try:
        claims = decode_access_claims(token)
    except (InvalidTokenError, ValueError):
        raise credentials_exception

    user = await db.get(
        User,
        claims.user_id,
    )

    if user is None:
        raise credentials_exception

    if claims.session_id is not None and not await session_is_active(db, claims.session_id, user.id):
        raise credentials_exception

    if user.is_blocked:
        raise HTTPException(status_code=403, detail="Your account is blocked")

    return user

async def get_admin_user(current_user: Annotated[User, Depends(get_current_user)]) -> User:
    if not current_user.is_admin:
        raise HTTPException(status_code=403, detail="Administrator access required")
    return current_user
