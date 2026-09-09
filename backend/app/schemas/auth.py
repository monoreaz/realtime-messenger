from pydantic import BaseModel
from pydantic import BaseModel, Field
from pydantic import BaseModel, EmailStr, Field

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
    