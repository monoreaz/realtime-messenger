import uuid
from datetime import datetime

from pydantic import BaseModel

from app.schemas.user import UserResponse


class ChatResponse(BaseModel):
    id: uuid.UUID
    type: str
    peer: UserResponse
    created_at: datetime