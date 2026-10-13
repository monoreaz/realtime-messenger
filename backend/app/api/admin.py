import uuid
from datetime import datetime, timezone
from typing import Annotated, Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, ConfigDict, StrictBool
from sqlalchemy import func, or_, select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.dependencies import get_admin_user
from app.database import get_db
from app.models.auth_session import AuthSession
from app.models.chat import Chat
from app.models.message import Message
from app.models.user import User
from app.realtime.manager import manager

router = APIRouter(prefix="/admin", tags=["admin"])
Admin = Annotated[User, Depends(get_admin_user)]


class AdminUserResponse(BaseModel):
    model_config = ConfigDict(from_attributes=True)
    id: uuid.UUID
    username: str
    display_name: str | None
    email: str | None
    email_verified_at: datetime | None
    created_at: datetime
    is_admin: bool
    is_blocked: bool


class AdminUsersResponse(BaseModel):
    users: list[AdminUserResponse]
    total: int


class AdminStats(BaseModel):
    users: int
    blocked_users: int
    online_users: int
    groups: int
    private_chats: int
    messages: int


class BlockUpdate(BaseModel):
    is_blocked: StrictBool


@router.get("/stats", response_model=AdminStats)
async def get_stats(admin: Admin, db: AsyncSession = Depends(get_db)):
    return AdminStats(
        users=await db.scalar(select(func.count()).select_from(User)),
        blocked_users=await db.scalar(select(func.count()).select_from(User).where(User.is_blocked.is_(True))),
        online_users=len(manager.active_connections),
        groups=await db.scalar(select(func.count()).select_from(Chat).where(Chat.type == "group")),
        private_chats=await db.scalar(select(func.count()).select_from(Chat).where(Chat.type == "private")),
        messages=await db.scalar(select(func.count()).select_from(Message).where(Message.deleted_at.is_(None))),
    )


@router.get("/users", response_model=AdminUsersResponse)
async def list_users(
    admin: Admin,
    q: Annotated[str, Query(max_length=100)] = "",
    state: Literal["all", "active", "blocked"] = "all",
    offset: Annotated[int, Query(ge=0)] = 0,
    limit: Annotated[int, Query(ge=1, le=100)] = 25,
    db: AsyncSession = Depends(get_db),
):
    conditions = []
    if q.strip():
        text = q.strip().lower()
        conditions.append(or_(
            func.lower(User.username).contains(text, autoescape=True),
            func.lower(User.display_name).contains(text, autoescape=True),
            func.lower(User.email).contains(text, autoescape=True),
        ))
    if state != "all":
        conditions.append(User.is_blocked.is_(state == "blocked"))
    total = await db.scalar(select(func.count()).select_from(User).where(*conditions))
    users = (await db.scalars(select(User).where(*conditions)
        .order_by(User.created_at.desc(), User.id).offset(offset).limit(limit))).all()
    return AdminUsersResponse(users=[AdminUserResponse.model_validate(user) for user in users], total=total)


@router.patch("/users/{user_id}/block", response_model=AdminUserResponse)
async def set_user_blocked(user_id: uuid.UUID, data: BlockUpdate, admin: Admin, db: AsyncSession = Depends(get_db)):
    target = await db.scalar(select(User).where(User.id == user_id).with_for_update())
    if target is None:
        raise HTTPException(status_code=404, detail="User not found")
    if target.id == admin.id or target.is_admin:
        raise HTTPException(status_code=400, detail="Administrator accounts cannot be blocked")
    target.is_blocked = data.is_blocked
    if data.is_blocked:
        await db.execute(update(AuthSession).where(
            AuthSession.user_id == target.id, AuthSession.revoked_at.is_(None),
        ).values(revoked_at=datetime.now(timezone.utc)))
    await db.commit()
    if data.is_blocked:
        for socket in list(manager.active_connections.get(target.id, [])):
            try:
                await socket.send_json({"type": "account.blocked"})
                await socket.close(code=1008)
            except Exception:
                pass
            # The endpoint's finally block handles presence and active calls.
    return AdminUserResponse.model_validate(target)
