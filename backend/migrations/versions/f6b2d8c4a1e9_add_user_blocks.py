"""Let users block other users."""
from alembic import op
import sqlalchemy as sa
revision = "f6b2d8c4a1e9"
down_revision = "e5a1c3b7d9f0"
branch_labels = None
depends_on = None

def upgrade():
    op.create_table(
        "user_blocks",
        sa.Column("blocker_id", sa.Uuid(), sa.ForeignKey("users.id", ondelete="CASCADE"), primary_key=True),
        sa.Column("blocked_id", sa.Uuid(), sa.ForeignKey("users.id", ondelete="CASCADE"), primary_key=True),
        sa.Column("created_at", sa.DateTime(timezone=True), server_default=sa.func.now(), nullable=False),
        sa.CheckConstraint("blocker_id <> blocked_id", name="ck_user_blocks_not_self"),
    )
    op.create_index("ix_user_blocks_blocked_id", "user_blocks", ["blocked_id"])

def downgrade():
    op.drop_index("ix_user_blocks_blocked_id", table_name="user_blocks")
    op.drop_table("user_blocks")
