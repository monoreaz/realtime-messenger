"""Store when users were last online and who may see it."""
from alembic import op
import sqlalchemy as sa
revision = "e5a1c3b7d9f0"
down_revision = "d91f02a34b56"
branch_labels = None
depends_on = None

def upgrade():
    op.add_column("users", sa.Column("last_seen_at", sa.DateTime(timezone=True), nullable=True))
    op.add_column("users", sa.Column("last_seen_visibility", sa.String(16), nullable=False, server_default="everyone"))
    op.create_check_constraint("ck_users_last_seen_visibility", "users", "last_seen_visibility IN ('everyone', 'nobody')")

def downgrade():
    op.drop_constraint("ck_users_last_seen_visibility", "users", type_="check")
    op.drop_column("users", "last_seen_visibility")
    op.drop_column("users", "last_seen_at")
