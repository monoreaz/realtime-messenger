import uuid
from datetime import datetime

from pydantic import BaseModel

from app.schemas.user import UserResponse


class LastMessageResponse(BaseModel):
    id: uuid.UUID
    sender_id: uuid.UUID
    content: str
    created_at: datetime


class ChatResponse(BaseModel):
    id: uuid.UUID
    type: str
    peer: UserResponse
    created_at: datetime
    last_message: LastMessageResponse | None = None
    unread_count: int = 0