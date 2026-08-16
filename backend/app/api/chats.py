import uuid
from datetime import datetime, timezone
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    HTTPException,
    Response,
    status,
)
from sqlalchemy import func, select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import aliased

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.chat import Chat, ChatMember
from app.models.message import Message
from app.models.user import User
from app.schemas.chat import ChatResponse, LastMessageResponse
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


async def build_chat_response(
    db: AsyncSession,
    chat: Chat,
    peer: User,
    current_user_id: uuid.UUID,
) -> ChatResponse:
    last_message_result = await db.execute(
        select(Message)
        .where(
            Message.chat_id == chat.id,
        )
        .order_by(
            Message.created_at.desc(),
        )
        .limit(1)
    )

    last_message = last_message_result.scalar_one_or_none()

    membership_result = await db.execute(
        select(ChatMember).where(
            ChatMember.chat_id == chat.id,
            ChatMember.user_id == current_user_id,
        )
    )

    membership = membership_result.scalar_one()

    unread_conditions = [
        Message.chat_id == chat.id,
        Message.sender_id != current_user_id,
    ]

    if membership.last_read_at is not None:
        unread_conditions.append(
            Message.created_at > membership.last_read_at
        )

    unread_result = await db.execute(
        select(
            func.count(Message.id)
        ).where(
            *unread_conditions
        )
    )

    unread_count = unread_result.scalar_one()

    last_message_response = None

    if last_message is not None:
        last_message_response = LastMessageResponse(
            id=last_message.id,
            sender_id=last_message.sender_id,
            content=last_message.content,
            created_at=last_message.created_at,
        )

    return ChatResponse(
        id=chat.id,
        type=chat.type,
        peer=UserResponse.model_validate(peer),
        created_at=chat.created_at,
        last_message=last_message_response,
        unread_count=unread_count,
    )


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

        return await build_chat_response(
            db,
            existing_chat,
            target_user,
            current_user.id,
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

        response.status_code = status.HTTP_200_OK

        return await build_chat_response(
            db,
            existing_chat,
            target_user,
            current_user.id,
        )

    await db.refresh(chat)

    return await build_chat_response(
        db,
        chat,
        target_user,
        current_user.id,
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
    )

    result = await db.execute(query)

    chats = []

    for chat, peer in result.all():
        chat_response = await build_chat_response(
            db,
            chat,
            peer,
            current_user.id,
        )

        chats.append(chat_response)

    chats.sort(
        key=lambda chat: (
            chat.last_message.created_at
            if chat.last_message
            else chat.created_at
        ),
        reverse=True,
    )

    return chats


@router.post(
    "/{chat_id}/read",
    status_code=status.HTTP_204_NO_CONTENT,
)
async def mark_chat_read(
    chat_id: uuid.UUID,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    result = await db.execute(
        select(ChatMember).where(
            ChatMember.chat_id == chat_id,
            ChatMember.user_id == current_user.id,
        )
    )

    membership = result.scalar_one_or_none()

    if membership is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Chat not found",
        )

    membership.last_read_at = datetime.now(
        timezone.utc
    )

    await db.commit()

    return Response(
        status_code=status.HTTP_204_NO_CONTENT
    )