import uuid
from datetime import datetime

from pydantic import BaseModel, ConfigDict, Field, field_validator


class MessageCreate(BaseModel):
    content: str = Field(
        min_length=1,
        max_length=4000,
    )

    @field_validator("content")
    @classmethod
    def validate_content(cls, value: str) -> str:
        value = value.strip()

        if not value:
            raise ValueError("Message cannot be empty")

        return value


class MessageResponse(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: uuid.UUID
    chat_id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    created_at: datetime