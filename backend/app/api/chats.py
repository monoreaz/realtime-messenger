import uuid
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    HTTPException,
    Response,
    status,
)
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import aliased

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.schemas.chat import ChatResponse
from app.schemas.user import UserResponse


router = APIRouter(
    prefix="/chats",
    tags=["chats"],
)


def make_direct_key(
    first_user_id: uuid.UUID,
    second_user_id: uuid.UUID,
) -> str:
    user_ids = sorted(
        [
            str(first_user_id),
            str(second_user_id),
        ]
    )

    return ":".join(user_ids)


def build_chat_response(
    chat: Chat,
    peer: User,
) -> ChatResponse:
    return ChatResponse(
        id=chat.id,
        type=chat.type,
        peer=UserResponse.model_validate(peer),
        created_at=chat.created_at,
    )


async def get_chat_by_direct_key(
    db: AsyncSession,
    direct_key: str,
) -> Chat | None:
    result = await db.execute(
        select(Chat).where(
            Chat.type == "private",
            Chat.direct_key == direct_key,
        )
    )

    return result.scalar_one_or_none()


@router.post(
    "/private/{user_id}",
    response_model=ChatResponse,
    status_code=status.HTTP_201_CREATED,
)
async def create_private_chat(
    user_id: uuid.UUID,
    response: Response,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    if user_id == current_user.id:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="You cannot create a chat with yourself",
        )

    target_user = await db.get(
        User,
        user_id,
    )

    if target_user is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="User not found",
        )

    direct_key = make_direct_key(
        current_user.id,
        target_user.id,
    )

    existing_chat = await get_chat_by_direct_key(
        db,
        direct_key,
    )

    if existing_chat is not None:
        response.status_code = status.HTTP_200_OK

        return build_chat_response(
            existing_chat,
            target_user,
        )

    chat = Chat(
        type="private",
        direct_key=direct_key,
    )

    db.add(chat)

    try:
        await db.flush()

        db.add_all(
            [
                ChatMember(
                    chat_id=chat.id,
                    user_id=current_user.id,
                ),
                ChatMember(
                    chat_id=chat.id,
                    user_id=target_user.id,
                ),
            ]
        )

        await db.commit()

    except IntegrityError:
        await db.rollback()

        existing_chat = await get_chat_by_direct_key(
            db,
            direct_key,
        )

        if existing_chat is None:
            raise HTTPException(
                status_code=status.HTTP_409_CONFLICT,
                detail="Could not create private chat",
            )

        target_user = await db.get(
            User,
            user_id,
        )

        response.status_code = status.HTTP_200_OK

        return build_chat_response(
            existing_chat,
            target_user,
        )

    await db.refresh(chat)

    return build_chat_response(
        chat,
        target_user,
    )


@router.get(
    "",
    response_model=list[ChatResponse],
)
async def get_chats(
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    own_membership = aliased(ChatMember)
    peer_membership = aliased(ChatMember)

    query = (
        select(
            Chat,
            User,
        )
        .join(
            own_membership,
            own_membership.chat_id == Chat.id,
        )
        .join(
            peer_membership,
            peer_membership.chat_id == Chat.id,
        )
        .join(
            User,
            User.id == peer_membership.user_id,
        )
        .where(
            Chat.type == "private",
            own_membership.user_id == current_user.id,
            peer_membership.user_id != current_user.id,
        )
        .order_by(
            Chat.created_at.desc(),
        )
    )

    result = await db.execute(query)

    return [
        build_chat_response(
            chat,
            peer,
        )
        for chat, peer in result.all()
    ]