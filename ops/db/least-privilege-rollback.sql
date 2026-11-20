-- Undo ops/db/least-privilege.sql: give everything back to the bootstrap superuser and
-- drop the two roles. Run as the bootstrap superuser AFTER the backend has been switched
-- back to the superuser credentials (otherwise its connections fail when the role is
-- dropped):
--
--   docker compose exec -T db sh -c \
--     'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v db="$POSTGRES_DB"' \
--     < ops/db/least-privilege-rollback.sql

\set ON_ERROR_STOP on

BEGIN;

-- Close remaining sessions of the roles (none expected after the backend switch).
SELECT pg_terminate_backend(pid) FROM pg_stat_activity
WHERE usename IN ('messenger_app', 'messenger_migrator') AND pid <> pg_backend_pid();

-- Ownership back to the bootstrap superuser (the user running this script).
SELECT format('REASSIGN OWNED BY messenger_migrator TO %I', current_user)
WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'messenger_migrator') \gexec
-- PostgreSQL's default owner of the public schema.
ALTER SCHEMA public OWNER TO pg_database_owner;
GRANT CONNECT, TEMPORARY ON DATABASE :"db" TO PUBLIC;

-- Drop grants and default privileges held by / for the roles, then the roles.
SELECT 'DROP OWNED BY messenger_app' WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'messenger_app') \gexec
SELECT 'DROP OWNED BY messenger_migrator' WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'messenger_migrator') \gexec
DROP ROLE IF EXISTS messenger_app;
DROP ROLE IF EXISTS messenger_migrator;

COMMIT;

SELECT count(*) AS public_objects_not_owned_by_current_user
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'public' AND c.relowner <> (SELECT oid FROM pg_roles WHERE rolname = current_user);
