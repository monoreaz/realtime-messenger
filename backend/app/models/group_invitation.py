import uuid
from datetime import datetime

from sqlalchemy import DateTime, ForeignKey, Uuid, func
from sqlalchemy.orm import Mapped, mapped_column

from app.database import Base


class GroupInvitation(Base):
    """A pending invitation of `invitee_id` to the group `chat_id`.

    An invitation grants nothing: messages, attachments, typing and presence all require a
    ChatMember row, which only the invitee can create by accepting.
    """

    __tablename__ = "group_invitations"

    chat_id: Mapped[uuid.UUID] = mapped_column(
        Uuid(as_uuid=True), ForeignKey("chats.id", ondelete="CASCADE"), primary_key=True,
    )
    invitee_id: Mapped[uuid.UUID] = mapped_column(
        Uuid(as_uuid=True), ForeignKey("users.id", ondelete="CASCADE"), primary_key=True, index=True,
    )
    inviter_id: Mapped[uuid.UUID | None] = mapped_column(
        Uuid(as_uuid=True), ForeignKey("users.id", ondelete="SET NULL"), nullable=True,
    )
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False,
    )
