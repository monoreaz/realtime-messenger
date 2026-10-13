"""Bind Android push tokens to refresh sessions."""
from alembic import op
import sqlalchemy as sa

revision = "c8e7d9012a34"
down_revision = "b7c41e2980da"
branch_labels = None
depends_on = None


def upgrade():
    op.create_table(
        "push_devices",
        sa.Column("token", sa.String(2048), primary_key=True),
        sa.Column("session_id", sa.Uuid(), sa.ForeignKey("auth_sessions.id", ondelete="CASCADE"), nullable=False),
        sa.Column("binding", sa.String(36), nullable=False),
        sa.Column("updated_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
    )
    op.create_index("ix_push_devices_session_id", "push_devices", ["session_id"])


def downgrade():
    op.drop_table("push_devices")
