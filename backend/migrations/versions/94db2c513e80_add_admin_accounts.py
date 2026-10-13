"""Add administrator and blocked account flags."""
from alembic import op
import sqlalchemy as sa

revision = "94db2c513e80"
down_revision = "6eaf2049b713"
branch_labels = None
depends_on = None


def upgrade():
    op.add_column("users", sa.Column("is_admin", sa.Boolean(), nullable=False, server_default=sa.false()))
    op.add_column("users", sa.Column("is_blocked", sa.Boolean(), nullable=False, server_default=sa.false()))


def downgrade():
    op.drop_column("users", "is_blocked")
    op.drop_column("users", "is_admin")
