"""Add message pinning."""
from alembic import op
revision = "1b3e4f5a6c7d"
down_revision = "a4c2f7e91b63"
branch_labels = None
depends_on = None
def upgrade() -> None:
    op.execute("ALTER TABLE messages ADD COLUMN IF NOT EXISTS pinned_at TIMESTAMPTZ NULL")
    op.execute("ALTER TABLE messages ADD COLUMN IF NOT EXISTS pinned_by_id UUID NULL REFERENCES users(id) ON DELETE SET NULL")
    op.execute("CREATE INDEX IF NOT EXISTS ix_messages_pinned_at ON messages (pinned_at)")
def downgrade() -> None:
    op.drop_index("ix_messages_pinned_at", table_name="messages")
    op.drop_column("messages", "pinned_by_id")
    op.drop_column("messages", "pinned_at")
