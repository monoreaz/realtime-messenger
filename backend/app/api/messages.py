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
from app.schemas.message import MessageCreate, MessageResponse


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

    message = Message(
        chat_id=chat_id,
        sender_id=current_user.id,
        content=message_data.content,
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

    message_response = MessageResponse.model_validate(
        message
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

    return messages