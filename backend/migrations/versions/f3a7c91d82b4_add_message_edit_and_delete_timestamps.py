"""Add message edit and deletion timestamps.

Revision ID: f3a7c91d82b4
Revises: 2bd3fc2e57fd
"""

from alembic import op


revision = "f3a7c91d82b4"
down_revision = "2bd3fc2e57fd"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.execute(
        "ALTER TABLE messages ADD COLUMN IF NOT EXISTS edited_at TIMESTAMPTZ NULL"
    )
    op.execute(
        "ALTER TABLE messages ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ NULL"
    )


def downgrade() -> None:
    op.drop_column("messages", "deleted_at")
    op.drop_column("messages", "edited_at")
