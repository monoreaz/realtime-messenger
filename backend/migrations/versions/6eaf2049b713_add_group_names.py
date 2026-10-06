"""Add names for group chats."""
from alembic import op
import sqlalchemy as sa

revision = "6eaf2049b713"
down_revision = "1b3e4f5a6c7d"
branch_labels = None
depends_on = None


def upgrade():
    # Earlier main deployments already had the pin revision before video support
    # was inserted into its ancestry. Repair that schema when upgrading to groups.
    columns = {column["name"] for column in sa.inspect(op.get_bind()).get_columns("messages")}
    if "video_url" not in columns:
        op.add_column("messages", sa.Column("video_url", sa.String(500), nullable=True))
    op.add_column("chats", sa.Column("name", sa.String(100), nullable=True))


def downgrade():
    op.drop_column("chats", "name")
