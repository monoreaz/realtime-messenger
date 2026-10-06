import uuid
from datetime import datetime

from pydantic import BaseModel, Field, field_validator

from app.schemas.user import UserResponse


class LastMessageResponse(BaseModel):
    id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    created_at: datetime


class ChatMemberResponse(UserResponse):
    group_role: str = "member"


class ChatResponse(BaseModel):
    id: uuid.UUID
    type: str
    peer: UserResponse | None = None
    name: str | None = None
    username: str | None = None
    description: str | None = None
    members: list[ChatMemberResponse] = Field(default_factory=list)
    created_at: datetime
    last_message: LastMessageResponse | None = None
    unread_count: int = 0
    peer_last_read_at: datetime | None = None

class GroupDetails(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    username: str | None = Field(default=None, min_length=3, max_length=32, pattern=r"^[a-z0-9_]+$")
    description: str | None = Field(default=None, max_length=500)

    @field_validator("name", mode="before")
    @classmethod
    def trim_name(cls, value):
        return value.strip() if isinstance(value, str) else value

    @field_validator("username", mode="before")
    @classmethod
    def normalize_username(cls, value):
        if isinstance(value, str):
            return value.strip().removeprefix("@").lower() or None
        return value

    @field_validator("description", mode="before")
    @classmethod
    def normalize_description(cls, value):
        return value.strip() or None if isinstance(value, str) else value


class GroupCreate(GroupDetails):
    member_ids: list[uuid.UUID] = Field(min_length=2, max_length=99)
