import uuid
from pathlib import Path
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    File,
    HTTPException,
    Query,
    UploadFile,
    status,
)
from fastapi.encoders import jsonable_encoder
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.chat import ChatMember
from app.models.user import User
from app.realtime.manager import manager
from app.schemas.user import (
    UserResponse,
    UserUpdate,
)


router = APIRouter(
    prefix="/users",
    tags=["users"],
)


AVATAR_DIR = Path("/app/uploads/avatars")

ALLOWED_AVATAR_TYPES = {
    "image/jpeg": ".jpg",
    "image/png": ".png",
    "image/webp": ".webp",
}

MAX_AVATAR_SIZE = 5 * 1024 * 1024


async def get_peer_user_ids(
    db: AsyncSession,
    user_id: uuid.UUID,
) -> list[uuid.UUID]:
    chat_ids = select(
        ChatMember.chat_id
    ).where(
        ChatMember.user_id == user_id
    )

    result = await db.execute(
        select(ChatMember.user_id).where(
            ChatMember.chat_id.in_(chat_ids),
            ChatMember.user_id != user_id,
        )
    )

    return list(
        set(result.scalars().all())
    )


async def notify_profile_updated(
    db: AsyncSession,
    user: User,
) -> None:
    peer_user_ids = await get_peer_user_ids(
        db,
        user.id,
    )

    user_data = jsonable_encoder(
        UserResponse.model_validate(user)
    )

    await manager.send_to_users(
        peer_user_ids,
        {
            "type": "profile.updated",
            "user": user_data,
        },
    )


def delete_avatar_file(
    avatar_url: str | None,
) -> None:
    if not avatar_url:
        return

    filename = Path(avatar_url).name
    avatar_path = AVATAR_DIR / filename

    try:
        avatar_path.unlink(
            missing_ok=True
        )
    except OSError:
        pass


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


@router.patch(
    "/me",
    response_model=UserResponse,
)
async def update_me(
    user_data: UserUpdate,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    updates = user_data.model_dump(
        exclude_unset=True
    )

    if (
        "username" in updates
        and updates["username"] is not None
    ):
        current_user.username = updates[
            "username"
        ]

    if "display_name" in updates:
        current_user.display_name = updates[
            "display_name"
        ]

    if "bio" in updates:
        current_user.bio = updates["bio"]

    try:
        await db.commit()
    except IntegrityError:
        await db.rollback()

        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Username already exists",
        )

    await db.refresh(current_user)

    await notify_profile_updated(
        db,
        current_user,
    )

    return current_user


@router.post(
    "/me/avatar",
    response_model=UserResponse,
)
async def upload_avatar(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    file: UploadFile = File(...),
    db: AsyncSession = Depends(get_db),
):
    extension = ALLOWED_AVATAR_TYPES.get(
        file.content_type or ""
    )

    if extension is None:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Avatar must be JPEG, PNG, or WebP",
        )

    content = await file.read(
        MAX_AVATAR_SIZE + 1
    )

    if len(content) > MAX_AVATAR_SIZE:
        raise HTTPException(
            status_code=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE,
            detail="Avatar must be smaller than 5 MB",
        )

    if not content:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Avatar file is empty",
        )

    AVATAR_DIR.mkdir(
        parents=True,
        exist_ok=True,
    )

    filename = (
        f"{current_user.id}-"
        f"{uuid.uuid4().hex}"
        f"{extension}"
    )

    avatar_path = AVATAR_DIR / filename

    avatar_path.write_bytes(content)

    old_avatar_url = current_user.avatar_url

    current_user.avatar_url = (
        f"/uploads/avatars/{filename}"
    )

    await db.commit()
    await db.refresh(current_user)

    delete_avatar_file(
        old_avatar_url
    )

    await notify_profile_updated(
        db,
        current_user,
    )

    return current_user


@router.delete(
    "/me/avatar",
    response_model=UserResponse,
)
async def remove_avatar(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    old_avatar_url = current_user.avatar_url

    current_user.avatar_url = None

    await db.commit()
    await db.refresh(current_user)

    delete_avatar_file(
        old_avatar_url
    )

    await notify_profile_updated(
        db,
        current_user,
    )

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

    return list(
        result.scalars().all()
    )