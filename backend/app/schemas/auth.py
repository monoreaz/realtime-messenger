from pydantic import BaseModel
from pydantic import BaseModel, Field
from pydantic import BaseModel, EmailStr, Field
from datetime import datetime
from uuid import UUID



class Token(BaseModel):
    access_token: str
    token_type: str

class EmailVerificationRequest(BaseModel):
    token: str = Field(
        min_length=32,
        max_length=512,
    )
class ForgotPasswordRequest(BaseModel):
    email: EmailStr


class ResetPasswordRequest(BaseModel):
    token: str = Field(
        min_length=32,
        max_length=512,
    )

    new_password: str = Field(
        min_length=8,
        max_length=128,
    )

class ResendVerificationRequest(BaseModel):
    email: EmailStr
class ChangePasswordRequest(BaseModel):
    current_password: str = Field(min_length=8, max_length=128)
    new_password: str = Field(min_length=8, max_length=128)

class AuthSessionResponse(BaseModel):
    id: UUID
    user_agent: str | None
    remember_me: bool
    created_at: datetime
    last_used_at: datetime | None
    expires_at: datetime
    current: bool

