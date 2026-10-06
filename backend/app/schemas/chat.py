import uuid
from datetime import datetime

from pydantic import BaseModel, Field, field_validator

from app.schemas.user import UserResponse


class LastMessageResponse(BaseModel):
    id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    created_at: datetime


class ChatResponse(BaseModel):
    id: uuid.UUID
    type: str
    peer: UserResponse | None = None
    name: str | None = None
    members: list[UserResponse] = Field(default_factory=list)
    created_at: datetime
    last_message: LastMessageResponse | None = None
    unread_count: int = 0
    peer_last_read_at: datetime | None = None

class GroupCreate(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    member_ids: list[uuid.UUID] = Field(min_length=2, max_length=99)

    @field_validator("name", mode="before")
    @classmethod
    def trim_name(cls, value):
        return value.strip() if isinstance(value, str) else value
