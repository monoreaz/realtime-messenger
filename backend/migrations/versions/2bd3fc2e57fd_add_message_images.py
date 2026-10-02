"""Add message images.

Revision ID: 2bd3fc2e57fd
Revises: 248edda99f23
"""

from alembic import op
import sqlalchemy as sa


revision = "2bd3fc2e57fd"
down_revision = "248edda99f23"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.add_column(
        "messages",
        sa.Column(
            "image_url",
            sa.String(length=500),
            nullable=True,
        ),
    )


def downgrade() -> None:
    op.drop_column("messages", "image_url")
