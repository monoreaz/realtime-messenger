"""The application's database role has data access only (SECURITY_AUDIT.md, M-3).

Runs when EXPECT_LEAST_PRIVILEGE=1, i.e. when DATABASE_URL uses the `messenger_app` role
created by ops/db/least-privilege.sql; skipped otherwise (developer setups often use a
superuser).
"""
import os
import unittest
import uuid

from sqlalchemy import text
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import create_async_engine

from app.database import DATABASE_URL


@unittest.skipUnless(os.getenv("EXPECT_LEAST_PRIVILEGE") == "1", "database role is not the least-privilege app role")
class AppRolePrivilegeTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.engine = create_async_engine(DATABASE_URL)
        self.connection = await self.engine.connect()

    async def asyncTearDown(self):
        await self.connection.close()
        await self.engine.dispose()

    async def refused(self, statement):
        transaction = await self.connection.begin()
        try:
            await self.connection.execute(text(statement))
        except DBAPIError:
            return True
        finally:
            await transaction.rollback()
        return False

    async def test_role_is_not_privileged(self):
        row = (await self.connection.execute(text(
            "SELECT rolsuper, rolcreatedb, rolcreaterole, rolbypassrls FROM pg_roles WHERE rolname = current_user"
        ))).one()
        self.assertEqual(tuple(row), (False, False, False, False))
        owned = await self.connection.scalar(text(
            "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
            "WHERE n.nspname = 'public' AND c.relowner = (SELECT oid FROM pg_roles WHERE rolname = current_user)"
        ))
        self.assertEqual(owned, 0)

    async def test_data_access_works(self):
        transaction = await self.connection.begin()
        try:
            user_id = uuid.uuid4()
            await self.connection.execute(text(
                "INSERT INTO users (id, username, password_hash, created_at) VALUES (:id, :name, 'x', now())"
            ), {"id": user_id, "name": f"priv_{user_id.hex[:12]}"})
            await self.connection.execute(text("UPDATE users SET bio = 'b' WHERE id = :id"), {"id": user_id})
            self.assertEqual(await self.connection.scalar(text("SELECT count(*) FROM users WHERE id = :id"), {"id": user_id}), 1)
            await self.connection.execute(text("DELETE FROM users WHERE id = :id"), {"id": user_id})
        finally:
            await transaction.rollback()

    async def test_schema_changes_and_dangerous_operations_are_refused(self):
        for statement in (
            "CREATE TABLE public.audit_probe (id int)",
            "ALTER TABLE users ADD COLUMN audit_probe int",
            "DROP TABLE messages",
            "TRUNCATE messages",
            "CREATE TEMP TABLE audit_probe (id int)",
            "SELECT * FROM alembic_version",
            "SELECT rolpassword FROM pg_authid",
            "COPY (SELECT 1) TO PROGRAM 'true'",
            "SELECT pg_read_file('/etc/hostname')",
            "CREATE ROLE audit_probe",
        ):
            self.assertTrue(await self.refused(statement), statement)
