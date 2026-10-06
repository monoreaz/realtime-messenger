"""Add message video URL.

Revision ID: a4c2f7e91b63
Revises: f3a7c91d82b4
"""

from alembic import op
import sqlalchemy as sa


revision = "a4c2f7e91b63"
down_revision = "f3a7c91d82b4"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.add_column(
        "messages",
        sa.Column(
            "video_url",
            sa.String(length=500),
            nullable=True,
        ),
    )


def downgrade() -> None:
    op.drop_column("messages", "video_url")
