from pydantic import BaseModel
from pydantic import BaseModel, Field

class Token(BaseModel):
    access_token: str
    token_type: str

class EmailVerificationRequest(BaseModel):
    token: str = Field(
        min_length=32,
        max_length=512,
    )

    