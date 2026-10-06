"""Add names for group chats."""
from alembic import op
import sqlalchemy as sa

revision = "6eaf2049b713"
down_revision = "1b3e4f5a6c7d"
branch_labels = None
depends_on = None


def upgrade():
    op.add_column("chats", sa.Column("name", sa.String(100), nullable=True))


def downgrade():
    op.drop_column("chats", "name")
