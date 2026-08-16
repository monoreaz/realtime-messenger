from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    HTTPException,
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
from app.models.user import User
from app.schemas.auth import Token
from app.schemas.user import UserCreate, UserResponse


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
    user = User(
        username=user_data.username,
        password_hash=hash_password(user_data.password),
    )

    db.add(user)

    try:
        await db.commit()
    except IntegrityError:
        await db.rollback()

        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Username already exists",
        )

    await db.refresh(user)

    return user


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
    username = form_data.username.lower()

    result = await db.execute(
        select(User).where(
            User.username == username
        )
    )

    user = result.scalar_one_or_none()

    if user is None or not verify_password(
        form_data.password,
        user.password_hash,
    ):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Incorrect username or password",
            headers={
                "WWW-Authenticate": "Bearer",
            },
        )

    access_token = create_access_token(
        str(user.id)
    )

    return Token(
        access_token=access_token,
        token_type="bearer",
    )