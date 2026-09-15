import hashlib
import os
import secrets

from datetime import datetime, timedelta, timezone
from typing import Annotated
from uuid import UUID

from fastapi import (
    APIRouter,
    Depends,
    Form,
    HTTPException,
    Request,
    Response,
    status,
)
from fastapi.security import OAuth2PasswordRequestForm
from sqlalchemy import func, or_, select, update
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_current_user
from app.core.security import (
    create_access_token,
    hash_password,
    verify_password,
)
from app.database import get_db
from app.models.auth_session import AuthSession
from app.models.email_verification import EmailVerificationToken
from app.models.password_reset import PasswordResetToken
from app.models.user import User
from app.schemas.auth import (
    AuthSessionResponse,
    ChangePasswordRequest,
    EmailVerificationRequest,
    ForgotPasswordRequest,
    ResendVerificationRequest,
    ResetPasswordRequest,
    Token,
)
from app.schemas.user import (
    UserCreate,
    UserResponse,
)
from app.services.email import (
    EmailDeliveryError,
    send_password_reset_email,
    send_verification_email,
)


EMAIL_VERIFICATION_EXPIRE_MINUTES = int(
    os.getenv(
        "EMAIL_VERIFICATION_EXPIRE_MINUTES",
        "30",
    )
)

EMAIL_VERIFICATION_RESEND_COOLDOWN_SECONDS = int(
    os.getenv(
        "EMAIL_VERIFICATION_RESEND_COOLDOWN_SECONDS",
        "60",
    )
)

PASSWORD_RESET_EXPIRE_MINUTES = int(
    os.getenv(
        "PASSWORD_RESET_EXPIRE_MINUTES",
        "30",
    )
)

PUBLIC_APP_URL = os.getenv(
    "PUBLIC_APP_URL",
    "http://localhost:5173",
).rstrip("/")

REFRESH_TOKEN_SESSION_HOURS = int(
    os.getenv(
        "REFRESH_TOKEN_SESSION_HOURS",
        "24",
    )
)

REFRESH_TOKEN_REMEMBER_DAYS = int(
    os.getenv(
        "REFRESH_TOKEN_REMEMBER_DAYS",
        "30",
    )
)

REFRESH_COOKIE_NAME = os.getenv(
    "REFRESH_COOKIE_NAME",
    "messenger_refresh",
)

COOKIE_SECURE = (
    os.getenv(
        "COOKIE_SECURE",
        "true",
    ).lower()
    == "true"
)

COOKIE_SAMESITE = os.getenv(
    "COOKIE_SAMESITE",
    "lax",
)


router = APIRouter(
    prefix="/auth",
    tags=["auth"],
)


def hash_raw_token(
    token: str,
) -> str:
    return hashlib.sha256(
        token.encode("utf-8")
    ).hexdigest()


def set_refresh_cookie(
    response: Response,
    token: str,
    remember_me: bool,
) -> None:
    cookie_options = {
        "key": REFRESH_COOKIE_NAME,
        "value": token,
        "httponly": True,
        "secure": COOKIE_SECURE,
        "samesite": COOKIE_SAMESITE,
        "path": "/",
    }

    if remember_me:
        cookie_options["max_age"] = (
            REFRESH_TOKEN_REMEMBER_DAYS
            * 24
            * 60
            * 60
        )

    response.set_cookie(
        **cookie_options
    )


async def create_refresh_session(
    db: AsyncSession,
    user_id: UUID,
    remember_me: bool,
    user_agent: str | None = None,
) -> str:
    raw_token = secrets.token_urlsafe(48)

    now = datetime.now(
        timezone.utc
    )

    if remember_me:
        expires_at = (
            now
            + timedelta(
                days=REFRESH_TOKEN_REMEMBER_DAYS
            )
        )
    else:
        expires_at = (
            now
            + timedelta(
                hours=REFRESH_TOKEN_SESSION_HOURS
            )
        )

    session = AuthSession(
        user_id=user_id,
        token_hash=hash_raw_token(
            raw_token
        ),
        remember_me=remember_me,
        expires_at=expires_at,
        user_agent=user_agent,
    )

    db.add(session)

    await db.flush()

    return raw_token


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
            User.username
            == user_data.username
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

        verification_token = (
            EmailVerificationToken(
                user_id=user.id,
                token_hash=hash_raw_token(
                    raw_token
                ),
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
        )

        db.add(
            verification_token
        )

        verification_url = (
            f"{PUBLIC_APP_URL}/"
            f"?verify={raw_token}"
        )

        try:
            await send_verification_email(
                email,
                verification_url,
            )
        except EmailDeliveryError:
            await db.rollback()

            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail=(
                    "Could not send verification email. "
                    "Please try again."
                ),
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

    return user


@router.post(
    "/verify-email",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def verify_email(
    verification_data: EmailVerificationRequest,
    db: AsyncSession = Depends(get_db),
):
    token_hash = hash_raw_token(
        verification_data.token
    )

    result = await db.execute(
        select(
            EmailVerificationToken
        ).where(
            EmailVerificationToken.token_hash
            == token_hash,
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

    user = await db.get(
        User,
        verification_token.user_id,
    )

    if user is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid verification token",
        )

    if verification_token.used_at is not None:
        if user.email_verified_at is not None:
            return Response(
                status_code=status.HTTP_204_NO_CONTENT
            )

        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid verification token",
        )

    now = datetime.now(
        timezone.utc
    )

    if verification_token.expires_at <= now:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Verification token expired",
        )

    user.email_verified_at = now
    verification_token.used_at = now

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/resend-verification",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def resend_verification(
    data: ResendVerificationRequest,
    db: AsyncSession = Depends(get_db),
):
    email = str(
        data.email
    ).strip().lower()

    result = await db.execute(
        select(User).where(
            User.email == email
        )
    )

    user = result.scalar_one_or_none()

    if (
        user is None
        or user.email_verified_at is not None
    ):
        return Response(
            status_code=status.HTTP_204_NO_CONTENT
        )

    now = datetime.now(
        timezone.utc
    )

    latest_result = await db.execute(
        select(EmailVerificationToken)
        .where(
            EmailVerificationToken.user_id
            == user.id
        )
        .order_by(
            EmailVerificationToken.created_at.desc()
        )
        .limit(1)
    )

    latest_token = (
        latest_result.scalar_one_or_none()
    )

    if (
        latest_token is not None
        and latest_token.created_at is not None
        and (
            now - latest_token.created_at
        ).total_seconds()
        < EMAIL_VERIFICATION_RESEND_COOLDOWN_SECONDS
    ):
        return Response(
            status_code=status.HTTP_204_NO_CONTENT
        )

    await db.execute(
        update(EmailVerificationToken)
        .where(
            EmailVerificationToken.user_id
            == user.id,
            EmailVerificationToken.used_at.is_(
                None
            ),
        )
        .values(
            used_at=now
        )
    )

    raw_token = secrets.token_urlsafe(
        32
    )

    verification_token = EmailVerificationToken(
        user_id=user.id,
        token_hash=hash_raw_token(
            raw_token
        ),
        expires_at=(
            now
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

    await db.flush()

    verification_url = (
        f"{PUBLIC_APP_URL}/"
        f"?verify={raw_token}"
    )

    try:
        await send_verification_email(
            email,
            verification_url,
        )
    except EmailDeliveryError:
        await db.rollback()

        raise HTTPException(
            status_code=(
                status.HTTP_503_SERVICE_UNAVAILABLE
            ),
            detail=(
                "Could not send verification "
                "email. Please try again."
            ),
        )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/change-password",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def change_password(
    data: ChangePasswordRequest,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    if not verify_password(
        data.current_password,
        current_user.password_hash,
    ):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Current password is incorrect",
        )

    if data.current_password == data.new_password:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="New password must be different from current password",
        )

    now = datetime.now(
        timezone.utc
    )

    current_user.password_hash = hash_password(
        data.new_password
    )

    await db.execute(
        update(AuthSession)
        .where(
            AuthSession.user_id
            == current_user.id,
            AuthSession.revoked_at.is_(
                None
            ),
        )
        .values(
            revoked_at=now
        )
    )

    await db.execute(
        update(PasswordResetToken)
        .where(
            PasswordResetToken.user_id
            == current_user.id,
            PasswordResetToken.used_at.is_(
                None
            ),
        )
        .values(
            used_at=now
        )
    )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/forgot-password",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def forgot_password(
    data: ForgotPasswordRequest,
    db: AsyncSession = Depends(get_db),
):
    email = str(
        data.email
    ).strip().lower()

    result = await db.execute(
        select(User).where(
            User.email == email
        )
    )

    user = result.scalar_one_or_none()

    if (
        user is None
        or user.email_verified_at is None
    ):
        return Response(
            status_code=status.HTTP_204_NO_CONTENT
        )

    now = datetime.now(
        timezone.utc
    )

    await db.execute(
        update(PasswordResetToken)
        .where(
            PasswordResetToken.user_id
            == user.id,
            PasswordResetToken.used_at.is_(
                None
            ),
        )
        .values(
            used_at=now
        )
    )

    raw_token = secrets.token_urlsafe(
        32
    )

    reset_token = PasswordResetToken(
        user_id=user.id,
        token_hash=hash_raw_token(
            raw_token
        ),
        expires_at=(
            now
            + timedelta(
                minutes=(
                    PASSWORD_RESET_EXPIRE_MINUTES
                )
            )
        ),
    )

    db.add(reset_token)

    await db.flush()

    reset_url = (
        f"{PUBLIC_APP_URL}/"
        f"?reset={raw_token}"
    )

    try:
        await send_password_reset_email(
            email,
            reset_url,
        )
    except EmailDeliveryError:
        await db.rollback()

        raise HTTPException(
            status_code=(
                status.HTTP_503_SERVICE_UNAVAILABLE
            ),
            detail=(
                "Could not send password "
                "reset email. Please try again."
            ),
        )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/reset-password",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def reset_password(
    data: ResetPasswordRequest,
    db: AsyncSession = Depends(get_db),
):
    token_hash = hash_raw_token(
        data.token
    )

    result = await db.execute(
        select(PasswordResetToken).where(
            PasswordResetToken.token_hash
            == token_hash,
            PasswordResetToken.used_at.is_(
                None
            ),
        )
    )

    reset_token = (
        result.scalar_one_or_none()
    )

    if reset_token is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=(
                "Invalid or already used "
                "password reset link"
            ),
        )

    now = datetime.now(
        timezone.utc
    )

    if reset_token.expires_at <= now:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Password reset link expired",
        )

    user = await db.get(
        User,
        reset_token.user_id,
    )

    if user is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid password reset link",
        )

    user.password_hash = hash_password(
        data.new_password
    )

    reset_token.used_at = now

    await db.execute(
        update(AuthSession)
        .where(
            AuthSession.user_id
            == user.id,
            AuthSession.revoked_at.is_(
                None
            ),
        )
        .values(
            revoked_at=now
        )
    )

    await db.execute(
        update(PasswordResetToken)
        .where(
            PasswordResetToken.user_id
            == user.id,
            PasswordResetToken.used_at.is_(
                None
            ),
            PasswordResetToken.id
            != reset_token.id,
        )
        .values(
            used_at=now
        )
    )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/login",
    response_model=Token,
)
async def login(
    request: Request,
    response: Response,
    form_data: Annotated[
        OAuth2PasswordRequestForm,
        Depends(),
    ],
    remember_me: Annotated[
        bool,
        Form(),
    ] = False,
    db: AsyncSession = Depends(get_db),
):
    identifier = (
        form_data.username
        .strip()
    )

    result = await db.execute(
        select(User).where(
            or_(
                func.lower(User.username)
                == identifier.lower(),
                func.lower(User.email)
                == identifier.lower(),
            )
        )
    )

    user = result.scalar_one_or_none()

    if (
        user is None
        or not verify_password(
            form_data.password,
            user.password_hash,
        )
    ):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Incorrect username, email or password",
            headers={
                "WWW-Authenticate": "Bearer",
            },
        )

    if (
        user.email is not None
        and user.email_verified_at is None
    ):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Email is not verified",
        )

    raw_refresh_token = (
        await create_refresh_session(
            db,
            user.id,
            remember_me,
            request.headers.get(
                "user-agent"
            ),
        )
    )

    await db.commit()

    set_refresh_cookie(
        response,
        raw_refresh_token,
        remember_me,
    )

    access_token = create_access_token(
        str(user.id)
    )

    return Token(
        access_token=access_token,
        token_type="bearer",
    )


@router.post(
    "/refresh",
    response_model=Token,
)
async def refresh_access_token(
    request: Request,
    response: Response,
    db: AsyncSession = Depends(get_db),
):
    raw_token = request.cookies.get(
        REFRESH_COOKIE_NAME
    )

    if not raw_token:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Refresh session not found",
        )

    token_hash = hash_raw_token(
        raw_token
    )

    result = await db.execute(
        select(AuthSession).where(
            AuthSession.token_hash
            == token_hash,
            AuthSession.revoked_at.is_(
                None
            ),
        )
    )

    session = (
        result.scalar_one_or_none()
    )

    if session is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid refresh session",
        )

    now = datetime.now(
        timezone.utc
    )

    if session.expires_at <= now:
        session.revoked_at = now

        await db.commit()

        response.delete_cookie(
            key=REFRESH_COOKIE_NAME,
            path="/",
        )

        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Refresh session expired",
        )

    user = await db.get(
        User,
        session.user_id,
    )

    if user is None:
        session.revoked_at = now

        await db.commit()

        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="User not found",
        )

    if (
        user.email is not None
        and user.email_verified_at is None
    ):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Email is not verified",
        )

    new_raw_token = secrets.token_urlsafe(
        48
    )

    session.token_hash = hash_raw_token(
        new_raw_token
    )

    session.last_used_at = now

    await db.commit()

    set_refresh_cookie(
        response,
        new_raw_token,
        session.remember_me,
    )

    access_token = create_access_token(
        str(user.id)
    )

    return Token(
        access_token=access_token,
        token_type="bearer",
    )


@router.post(
    "/logout",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def logout(
    request: Request,
    response: Response,
    db: AsyncSession = Depends(get_db),
):
    raw_token = request.cookies.get(
        REFRESH_COOKIE_NAME
    )

    if raw_token:
        result = await db.execute(
            select(AuthSession).where(
                AuthSession.token_hash
                == hash_raw_token(
                    raw_token
                ),
                AuthSession.revoked_at.is_(
                    None
                ),
            )
        )

        session = (
            result.scalar_one_or_none()
        )

        if session is not None:
            session.revoked_at = (
                datetime.now(
                    timezone.utc
                )
            )

            await db.commit()

    response.delete_cookie(
        key=REFRESH_COOKIE_NAME,
        path="/",
    )

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.get(
    "/sessions",
    response_model=list[AuthSessionResponse],
)
async def get_sessions(
    request: Request,
    current_user: User = Depends(
        get_current_user
    ),
    db: AsyncSession = Depends(get_db),
):
    now = datetime.now(
        timezone.utc
    )

    raw_refresh_token = request.cookies.get(
        REFRESH_COOKIE_NAME
    )

    current_token_hash = (
        hash_raw_token(
            raw_refresh_token
        )
        if raw_refresh_token
        else None
    )

    result = await db.execute(
        select(AuthSession)
        .where(
            AuthSession.user_id
            == current_user.id,
            AuthSession.revoked_at.is_(
                None
            ),
            AuthSession.expires_at
            > now,
        )
        .order_by(
            AuthSession.created_at.desc()
        )
    )

    sessions = (
        result.scalars().all()
    )

    return [
        AuthSessionResponse(
            id=session.id,
            user_agent=session.user_agent,
            remember_me=session.remember_me,
            created_at=session.created_at,
            last_used_at=session.last_used_at,
            expires_at=session.expires_at,
            current=(
                current_token_hash is not None
                and session.token_hash
                == current_token_hash
            ),
        )
        for session in sessions
    ]


@router.delete(
    "/sessions/{session_id}",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def revoke_session(
    session_id: UUID,
    request: Request,
    current_user: User = Depends(
        get_current_user
    ),
    db: AsyncSession = Depends(get_db),
):
    result = await db.execute(
        select(AuthSession).where(
            AuthSession.id
            == session_id,
            AuthSession.user_id
            == current_user.id,
            AuthSession.revoked_at.is_(
                None
            ),
        )
    )

    session = (
        result.scalar_one_or_none()
    )

    if session is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found",
        )

    raw_refresh_token = request.cookies.get(
        REFRESH_COOKIE_NAME
    )

    if (
        raw_refresh_token
        and session.token_hash
        == hash_raw_token(
            raw_refresh_token
        )
    ):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Use logout to end the current session",
        )

    session.revoked_at = datetime.now(
        timezone.utc
    )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )


@router.post(
    "/sessions/logout-others",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def logout_other_sessions(
    request: Request,
    current_user: User = Depends(
        get_current_user
    ),
    db: AsyncSession = Depends(get_db),
):
    raw_refresh_token = request.cookies.get(
        REFRESH_COOKIE_NAME
    )

    if not raw_refresh_token:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="No active session",
        )

    current_token_hash = hash_raw_token(
        raw_refresh_token
    )

    result = await db.execute(
        select(AuthSession).where(
            AuthSession.user_id
            == current_user.id,
            AuthSession.token_hash
            == current_token_hash,
            AuthSession.revoked_at.is_(
                None
            ),
        )
    )

    current_session = (
        result.scalar_one_or_none()
    )

    if current_session is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="No active session",
        )

    now = datetime.now(
        timezone.utc
    )

    await db.execute(
        update(AuthSession)
        .where(
            AuthSession.user_id
            == current_user.id,
            AuthSession.id
            != current_session.id,
            AuthSession.revoked_at.is_(
                None
            ),
        )
        .values(
            revoked_at=now
        )
    )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )