"""Group invitations: invited users must accept before they become members."""
from alembic import op
import sqlalchemy as sa
revision = "a7c3e9d1f2b4"
down_revision = "f6b2d8c4a1e9"
branch_labels = None
depends_on = None

def upgrade():
    op.create_table(
        "group_invitations",
        sa.Column("chat_id", sa.Uuid(), sa.ForeignKey("chats.id", ondelete="CASCADE"), primary_key=True),
        sa.Column("invitee_id", sa.Uuid(), sa.ForeignKey("users.id", ondelete="CASCADE"), primary_key=True),
        sa.Column("inviter_id", sa.Uuid(), sa.ForeignKey("users.id", ondelete="SET NULL"), nullable=True),
        sa.Column("created_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
    )
    op.create_index("ix_group_invitations_invitee_id", "group_invitations", ["invitee_id"])

def downgrade():
    op.drop_index("ix_group_invitations_invitee_id", table_name="group_invitations")
    op.drop_table("group_invitations")
