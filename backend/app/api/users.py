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
from sqlalchemy import delete, select
from sqlalchemy.dialects.postgresql import insert
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.chat import ChatMember
from app.models.user import User
from app.models.user_block import UserBlock
from app.realtime.manager import manager
from app.services.media import content_matches
from app.schemas.user import (
    BlockedUserResponse,
    PrivacySettings,
    PrivacyUpdate,
    UserResponse,
    UserUpdate,
)
from app.services.presence import (
    announce_last_seen,
    presence_snapshot,
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


@router.get(
    "/me/privacy",
    response_model=PrivacySettings,
)
async def get_privacy(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
):
    return PrivacySettings(
        last_seen=current_user.last_seen_visibility,
    )


@router.patch(
    "/me/privacy",
    response_model=PrivacySettings,
)
async def update_privacy(
    settings: PrivacyUpdate,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    if (
        settings.last_seen is not None
        and settings.last_seen != current_user.last_seen_visibility
    ):
        current_user.last_seen_visibility = settings.last_seen
        await db.commit()

        # Peers lose or regain this user's last seen, and because the rule
        # is mutual, this user's own view of their peers changes too.
        await announce_last_seen(
            db,
            current_user.id,
            "presence.last_seen",
        )
        await manager.send_to_user(
            current_user.id,
            await presence_snapshot(db, current_user.id),
        )

    return PrivacySettings(
        last_seen=current_user.last_seen_visibility,
    )


async def sync_block_change(
    db: AsyncSession,
    blocker_id: uuid.UUID,
    blocked_id: uuid.UUID,
) -> None:
    from app.api.ws import end_calls_between

    await end_calls_between(blocker_id, blocked_id)
    # Both sides gain or lose each other's presence and last seen.
    for user_id in (blocker_id, blocked_id):
        await manager.send_to_user(user_id, await presence_snapshot(db, user_id))
    await manager.send_to_user(blocker_id, {"type": "blocks.updated"})


@router.get(
    "/me/blocked",
    response_model=list[BlockedUserResponse],
)
async def list_blocked_users(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    result = await db.execute(
        select(User, UserBlock.created_at)
        .join(UserBlock, UserBlock.blocked_id == User.id)
        .where(UserBlock.blocker_id == current_user.id)
        .order_by(UserBlock.created_at.desc())
    )

    return [
        BlockedUserResponse(
            **UserResponse.model_validate(user).model_dump(),
            blocked_at=blocked_at,
        )
        for user, blocked_at in result.all()
    ]


@router.put(
    "/me/blocked/{user_id}",
    response_model=BlockedUserResponse,
)
async def block_user(
    user_id: uuid.UUID,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    if user_id == current_user.id:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="You cannot block yourself",
        )

    target_user = await db.get(User, user_id)

    if target_user is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="User not found",
        )

    user_data = UserResponse.model_validate(target_user).model_dump()

    blocked_at = await db.scalar(
        insert(UserBlock)
        .values(blocker_id=current_user.id, blocked_id=user_id)
        .on_conflict_do_nothing()
        .returning(UserBlock.created_at)
    )
    created = blocked_at is not None

    if not created:
        blocked_at = await db.scalar(
            select(UserBlock.created_at).where(
                UserBlock.blocker_id == current_user.id,
                UserBlock.blocked_id == user_id,
            )
        )

    await db.commit()

    if created:
        await sync_block_change(db, current_user.id, user_id)

    return BlockedUserResponse(**user_data, blocked_at=blocked_at)


@router.delete(
    "/me/blocked/{user_id}",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def unblock_user(
    user_id: uuid.UUID,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    result = await db.execute(
        delete(UserBlock).where(
            UserBlock.blocker_id == current_user.id,
            UserBlock.blocked_id == user_id,
        )
    )
    await db.commit()

    if result.rowcount:
        await sync_block_change(db, current_user.id, user_id)


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

    if not content_matches(file.content_type or "", content[:16]):
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Avatar must be JPEG, PNG, or WebP",
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