import os
import uuid
from datetime import datetime, timedelta, timezone
from typing import NamedTuple

import jwt
from jwt.exceptions import InvalidTokenError
from pwdlib import PasswordHash


JWT_SECRET_KEY = os.environ["JWT_SECRET_KEY"]
JWT_ALGORITHM = "HS256"
ACCESS_TOKEN_EXPIRE_MINUTES = int(
    os.environ.get("ACCESS_TOKEN_EXPIRE_MINUTES", "30")
)


password_hash = PasswordHash.recommended()


def hash_password(password: str) -> str:
    return password_hash.hash(password)


def verify_password(password: str, hashed_password: str) -> bool:
    return password_hash.verify(
        password,
        hashed_password,
    )


def create_access_token(user_id: str, session_id: uuid.UUID | str | None = None) -> str:
    now = datetime.now(timezone.utc)

    payload = {
        "sub": user_id,
        "iat": now,
        "exp": now + timedelta(
            minutes=ACCESS_TOKEN_EXPIRE_MINUTES
        ),
    }

    # The refresh session the token belongs to: revoking that session (logout,
    # "log out other sessions", password change/reset, admin block) revokes the token.
    if session_id is not None:
        payload["sid"] = str(session_id)

    return jwt.encode(
        payload,
        JWT_SECRET_KEY,
        algorithm=JWT_ALGORITHM,
    )


class AccessClaims(NamedTuple):
    user_id: uuid.UUID
    # None only for tokens issued before session binding was introduced; they
    # expire within ACCESS_TOKEN_EXPIRE_MINUTES of the upgrade.
    session_id: uuid.UUID | None
    expires_at: datetime


def decode_access_claims(token: str) -> AccessClaims:
    payload = jwt.decode(
        token,
        JWT_SECRET_KEY,
        algorithms=[JWT_ALGORITHM],
        options={"require": ["exp", "sub"]},
    )

    subject = payload.get("sub")

    if not isinstance(subject, str):
        raise InvalidTokenError("Token subject is missing")

    session = payload.get("sid")

    if session is not None and not isinstance(session, str):
        raise InvalidTokenError("Invalid session claim")

    return AccessClaims(
        uuid.UUID(subject),
        uuid.UUID(session) if session else None,
        datetime.fromtimestamp(payload["exp"], timezone.utc),
    )


def decode_access_token(token: str) -> uuid.UUID:
    return decode_access_claims(token).user_id