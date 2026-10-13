import uuid
from datetime import datetime, timezone
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, Field
from sqlalchemy import delete, select
from sqlalchemy.dialects.postgresql import insert
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import REFRESH_COOKIE_NAME, hash_raw_token
from app.core.dependencies import get_current_user
from app.database import get_db
from app.models.auth_session import AuthSession
from app.models.push_device import PushDevice
from app.models.user import User

router = APIRouter(prefix="/push", tags=["push"])


class DeviceRegistration(BaseModel):
    token: str = Field(min_length=20, max_length=2048, pattern=r"^\S+$")
    binding: uuid.UUID


@router.post("/devices", status_code=204)
async def register_device(
    data: DeviceRegistration,
    request: Request,
    current_user: Annotated[User, Depends(get_current_user)],
    db: AsyncSession = Depends(get_db),
):
    raw_token = request.cookies.get(REFRESH_COOKIE_NAME, "")
    session = await db.scalar(select(AuthSession).where(
        AuthSession.token_hash == hash_raw_token(raw_token),
        AuthSession.user_id == current_user.id,
        AuthSession.revoked_at.is_(None),
        AuthSession.expires_at > datetime.now(timezone.utc),
    ))
    if not raw_token or session is None:
        raise HTTPException(status_code=401, detail="Active refresh session required")
    # A WebView session belongs to one installation. Replace rotated FCM tokens.
    await db.execute(delete(PushDevice).where(
        PushDevice.session_id == session.id, PushDevice.token != data.token,
    ))
    values = dict(session_id=session.id, binding=str(data.binding), updated_at=datetime.now(timezone.utc))
    await db.execute(insert(PushDevice).values(token=data.token, **values).on_conflict_do_update(
        index_elements=[PushDevice.token], set_=values,
    ))
    await db.commit()
