import hashlib
import logging
import os
import secrets

from datetime import datetime, timedelta, timezone
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    HTTPException,
    Response,
    status,
)
from fastapi.security import OAuth2PasswordRequestForm
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.security import (
    create_access_token,
    hash_password,
    verify_password,
)
from app.database import get_db
from app.models.email_verification import EmailVerificationToken
from app.models.user import User
from app.schemas.auth import (
    EmailVerificationRequest,
    Token,
)
from app.schemas.user import (
    UserCreate,
    UserResponse,
)


logger = logging.getLogger(__name__)


EMAIL_VERIFICATION_EXPIRE_MINUTES = int(
    os.getenv(
        "EMAIL_VERIFICATION_EXPIRE_MINUTES",
        "30",
    )
)

PUBLIC_APP_URL = os.getenv(
    "PUBLIC_APP_URL",
    "http://localhost:5173",
).rstrip("/")


router = APIRouter(
    prefix="/auth",
    tags=["auth"],
)


@router.post(
    "/register",
    response_model=UserResponse,
    status_code=status.HTTP_201_CREATED,
)
async def register(
    user_data: UserCreate,
    db: AsyncSession = Depends(get_db),
):
    email = str(
        user_data.email
    ).lower()

    username_result = await db.execute(
        select(User).where(
            User.username == user_data.username
        )
    )

    if username_result.scalar_one_or_none():
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Username already exists",
        )

    email_result = await db.execute(
        select(User).where(
            User.email == email
        )
    )

    if email_result.scalar_one_or_none():
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Email already exists",
        )

    user = User(
        username=user_data.username,
        email=email,
        password_hash=hash_password(
            user_data.password
        ),
    )

    db.add(user)

    try:
        await db.flush()

        raw_token = secrets.token_urlsafe(
            32
        )

        token_hash = hashlib.sha256(
            raw_token.encode("utf-8")
        ).hexdigest()

        verification_token = EmailVerificationToken(
            user_id=user.id,
            token_hash=token_hash,
            expires_at=(
                datetime.now(
                    timezone.utc
                )
                + timedelta(
                    minutes=(
                        EMAIL_VERIFICATION_EXPIRE_MINUTES
                    )
                )
            ),
        )

        db.add(
            verification_token
        )

        await db.commit()

    except IntegrityError:
        await db.rollback()

        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=(
                "Username or email "
                "already exists"
            ),
        )

    await db.refresh(user)

    verification_url = (
        f"{PUBLIC_APP_URL}/"
        f"?verify={raw_token}"
    )

    logger.warning(
        "Email verification link for %s: %s",
        user.email,
        verification_url,
    )

    return user


@router.post(
    "/verify-email",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def verify_email(
    verification_data: EmailVerificationRequest,
    db: AsyncSession = Depends(get_db),
):
    token_hash = hashlib.sha256(
        verification_data.token.encode(
            "utf-8"
        )
    ).hexdigest()

    result = await db.execute(
        select(
            EmailVerificationToken
        ).where(
            EmailVerificationToken.token_hash
            == token_hash,
            EmailVerificationToken.used_at.is_(
                None
            ),
        )
    )

    verification_token = (
        result.scalar_one_or_none()
    )

    if verification_token is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid verification token",
        )

    now = datetime.now(
        timezone.utc
    )

    if (
        verification_token.expires_at
        <= now
    ):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Verification token expired",
        )

    user = await db.get(
        User,
        verification_token.user_id,
    )

    if user is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid verification token",
        )

    user.email_verified_at = now

    verification_token.used_at = now

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/login",
    response_model=Token,
)
async def login(
    form_data: Annotated[
        OAuth2PasswordRequestForm,
        Depends(),
    ],
    db: AsyncSession = Depends(get_db),
):
    username = (
        form_data.username
        .strip()
        .lower()
    )

    result = await db.execute(
        select(User).where(
            User.username == username
        )
    )

    user = (
        result.scalar_one_or_none()
    )

    if (
        user is None
        or not verify_password(
            form_data.password,
            user.password_hash,
        )
    ):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=(
                "Incorrect username "
                "or password"
            ),
            headers={
                "WWW-Authenticate": "Bearer",
            },
        )

    if (
        user.email is not None
        and user.email_verified_at
        is None
    ):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Email is not verified",
        )

    access_token = (
        create_access_token(
            str(user.id)
        )
    )

    return Token(
        access_token=access_token,
        token_type="bearer",
    )