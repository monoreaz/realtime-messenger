import uuid
from datetime import datetime

from pydantic import (
    BaseModel,
    Field,
    field_validator,
)


class MessageCreate(BaseModel):
    content: str = Field(
        min_length=1,
        max_length=4000,
    )

    reply_to_message_id: uuid.UUID | None = None

    @field_validator("content")
    @classmethod
    def validate_content(
        cls,
        value: str,
    ) -> str:
        value = value.strip()

        if not value:
            raise ValueError(
                "Message cannot be empty"
            )

        return value

class MessageUpdate(BaseModel):
    content: str = Field(
        min_length=1,
        max_length=4000,
    )

    @field_validator("content")
    @classmethod
    def validate_content(
        cls,
        value: str,
    ) -> str:
        value = value.strip()

        if not value:
            raise ValueError(
                "Message cannot be empty"
            )

        return value

class ReplyMessageResponse(BaseModel):
    id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    image_url: str | None = None


class MessageResponse(BaseModel):
    id: uuid.UUID
    chat_id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    image_url: str | None = None
    created_at: datetime
    reply_to_message_id: uuid.UUID | None = None
    reply_to_message: ReplyMessageResponse | None = None
    edited_at: datetime | None = None
    deleted_at: datetime | None = None