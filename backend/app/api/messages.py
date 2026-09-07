import uuid
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    HTTPException,
    Query,
    status,
)
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.chat import ChatMember
from app.models.message import Message
from app.models.user import User
from app.realtime.manager import manager
from app.schemas.message import (
    MessageCreate,
    MessageResponse,
    ReplyMessageResponse,
)


router = APIRouter(
    prefix="/chats",
    tags=["messages"],
)


async def check_chat_membership(
    db: AsyncSession,
    chat_id: uuid.UUID,
    user_id: uuid.UUID,
) -> None:
    result = await db.execute(
        select(ChatMember).where(
            ChatMember.chat_id == chat_id,
            ChatMember.user_id == user_id,
        )
    )

    membership = result.scalar_one_or_none()

    if membership is None:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Chat not found",
        )


def build_message_response(
    message: Message,
    replied_message: Message | None = None,
) -> MessageResponse:
    reply_response = None

    if replied_message is not None:
        reply_response = ReplyMessageResponse(
            id=replied_message.id,
            sender_id=replied_message.sender_id,
            content=replied_message.content,
        )

    return MessageResponse(
        id=message.id,
        chat_id=message.chat_id,
        sender_id=message.sender_id,
        content=message.content,
        created_at=message.created_at,
        reply_to_message_id=message.reply_to_message_id,
        reply_to_message=reply_response,
    )


async def build_message_responses(
    db: AsyncSession,
    messages: list[Message],
) -> list[MessageResponse]:
    reply_ids = {
        message.reply_to_message_id
        for message in messages
        if message.reply_to_message_id is not None
    }

    replied_messages: dict[
        uuid.UUID,
        Message,
    ] = {}

    if reply_ids:
        result = await db.execute(
            select(Message).where(
                Message.id.in_(reply_ids)
            )
        )

        replied_messages = {
            message.id: message
            for message in result.scalars().all()
        }

    return [
        build_message_response(
            message,
            replied_messages.get(
                message.reply_to_message_id
            ),
        )
        for message in messages
    ]


@router.post(
    "/{chat_id}/messages",
    response_model=MessageResponse,
    status_code=status.HTTP_201_CREATED,
)
async def send_message(
    chat_id: uuid.UUID,
    message_data: MessageCreate,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    db: AsyncSession = Depends(get_db),
):
    await check_chat_membership(
        db,
        chat_id,
        current_user.id,
    )

    replied_message = None

    if message_data.reply_to_message_id:
        result = await db.execute(
            select(Message).where(
                Message.id
                == message_data.reply_to_message_id,
                Message.chat_id == chat_id,
            )
        )

        replied_message = (
            result.scalar_one_or_none()
        )

        if replied_message is None:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=(
                    "Reply message not found "
                    "in this chat"
                ),
            )

    message = Message(
        chat_id=chat_id,
        sender_id=current_user.id,
        content=message_data.content,
        reply_to_message_id=(
            replied_message.id
            if replied_message
            else None
        ),
    )

    db.add(message)

    await db.commit()
    await db.refresh(message)

    result = await db.execute(
        select(ChatMember.user_id).where(
            ChatMember.chat_id == chat_id,
        )
    )

    member_ids = list(
        result.scalars().all()
    )

    message_response = build_message_response(
        message,
        replied_message,
    )

    await manager.send_to_users(
        member_ids,
        {
            "type": "message.new",
            "message": message_response.model_dump(
                mode="json"
            ),
        },
    )

    return message_response


@router.get(
    "/{chat_id}/messages",
    response_model=list[MessageResponse],
)
async def get_messages(
    chat_id: uuid.UUID,
    current_user: Annotated[
        User,
        Depends(get_current_user),
    ],
    limit: Annotated[
        int,
        Query(
            ge=1,
            le=100,
        ),
    ] = 50,
    db: AsyncSession = Depends(get_db),
):
    await check_chat_membership(
        db,
        chat_id,
        current_user.id,
    )

    result = await db.execute(
        select(Message)
        .where(
            Message.chat_id == chat_id,
        )
        .order_by(
            Message.created_at.desc(),
        )
        .limit(limit)
    )

    messages = list(
        result.scalars().all()
    )

    messages.reverse()

    return await build_message_responses(
        db,
        messages,
    )