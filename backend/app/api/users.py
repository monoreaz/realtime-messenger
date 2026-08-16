from typing import Annotated

from fastapi import APIRouter, Depends, Query
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.user import User
from app.schemas.user import UserResponse


router = APIRouter(
    prefix="/users",
    tags=["users"],
)


@router.get(
    "/me",
    response_model=UserResponse,
)
async def get_me(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
):
    return current_user


@router.get(
    "/search",
    response_model=list[UserResponse],
)
async def search_users(
    username: Annotated[
        str,
        Query(
            min_length=1,
            max_length=32,
        ),
    ],
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    query = (
        select(User)
        .where(
            User.id != current_user.id,
            User.username.contains(
                username.lower(),
                autoescape=True,
            ),
        )
        .order_by(User.username)
        .limit(20)
    )

    result = await db.execute(query)

    return list(result.scalars().all())