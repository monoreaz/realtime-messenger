"""Opt Android devices in to native incoming call notifications."""
from alembic import op
import sqlalchemy as sa
revision = "d91f02a34b56"
down_revision = "c8e7d9012a34"
branch_labels = None
depends_on = None

def upgrade():
    op.add_column("push_devices", sa.Column("call_notifications", sa.Boolean(), nullable=False, server_default=sa.false()))

def downgrade():
    op.drop_column("push_devices", "call_notifications")
