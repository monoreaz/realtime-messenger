"""Add group profiles and per-group administrator roles."""
from alembic import op
import sqlalchemy as sa

revision = "b7c41e2980da"
down_revision = "6eaf2049b713"
branch_labels = None
depends_on = None


def upgrade():
    op.add_column("chats", sa.Column("username", sa.String(32), nullable=True))
    op.create_unique_constraint("uq_chats_username", "chats", ["username"])
    op.add_column("chats", sa.Column("description", sa.String(500), nullable=True))
    op.add_column("chat_members", sa.Column("group_role", sa.String(16), nullable=False, server_default="member"))


def downgrade():
    op.drop_column("chat_members", "group_role")
    op.drop_column("chats", "description")
    op.drop_constraint("uq_chats_username", "chats", type_="unique")
    op.drop_column("chats", "username")
