import uuid
from datetime import datetime

from pydantic import BaseModel, ConfigDict, Field, field_validator


class UserCreate(BaseModel):
    username: str = Field(
        min_length=3,
        max_length=32,
        pattern=r"^[A-Za-z0-9_]+$",
    )

    password: str = Field(
        min_length=8,
        max_length=128,
    )

    @field_validator("username")
    @classmethod
    def normalize_username(cls, value: str) -> str:
        return value.lower()


class UserResponse(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: uuid.UUID
    username: str
    created_at: datetime