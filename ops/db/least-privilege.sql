-- Least-privilege database roles for Slaxz (SECURITY_AUDIT.md, M-3).
--
--   messenger_migrator  owns the schema and its objects; used only to run migrations.
--   messenger_app       data access only (SELECT/INSERT/UPDATE/DELETE); used by the backend.
--   <bootstrap user>    the image's superuser (POSTGRES_USER), kept for administration.
--
-- Run as the bootstrap superuser, e.g. (passwords come from the environment, never typed
-- on the command line or printed; use URL-safe values such as `openssl rand -hex 24`):
--
--   docker compose exec -T -e APP_DB_PASSWORD -e MIGRATOR_DB_PASSWORD db sh -c \
--     'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
--        -v app_password="$APP_DB_PASSWORD" -v migrator_password="$MIGRATOR_DB_PASSWORD" \
--        -v db="$POSTGRES_DB"' < ops/db/least-privilege.sql
--
-- Idempotent: safe to run again (passwords are reset, ownership/grants re-applied).
-- Undo with ops/db/least-privilege-rollback.sql.

\set ON_ERROR_STOP on

BEGIN;

SELECT format('CREATE ROLE messenger_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'messenger_migrator') \gexec
SELECT format('CREATE ROLE messenger_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS NOINHERIT')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'messenger_app') \gexec

ALTER ROLE messenger_migrator PASSWORD :'migrator_password';
ALTER ROLE messenger_app PASSWORD :'app_password';
-- The backend never needs more than this many connections; bounds runaway pools.
ALTER ROLE messenger_app CONNECTION LIMIT 50;

-- Only these roles (and superusers) may connect; nobody gets TEMP by default.
REVOKE ALL ON DATABASE :"db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO messenger_migrator, messenger_app;

-- The migrator owns the schema and every object in it.
ALTER SCHEMA public OWNER TO messenger_migrator;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;

DO $$
DECLARE
    item record;
BEGIN
    FOR item IN
        SELECT c.relname, c.relkind
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
    LOOP
        EXECUTE format('ALTER TABLE public.%I OWNER TO messenger_migrator', item.relname);
    END LOOP;
    -- Sequences owned by a table column move with the table; move any others too.
    FOR item IN
        SELECT c.relname
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' AND c.relkind = 'S'
          AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = c.oid AND d.deptype IN ('a', 'i'))
    LOOP
        EXECUTE format('ALTER SEQUENCE public.%I OWNER TO messenger_migrator', item.relname);
    END LOOP;
    FOR item IN
        SELECT t.typname
        FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'public' AND t.typtype IN ('e', 'd', 'c')
          AND NOT EXISTS (SELECT 1 FROM pg_class c WHERE c.reltype = t.oid)
    LOOP
        EXECUTE format('ALTER TYPE public.%I OWNER TO messenger_migrator', item.typname);
    END LOOP;
    FOR item IN
        SELECT p.oid::regprocedure AS signature
        FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public'
    LOOP
        EXECUTE format('ALTER ROUTINE %s OWNER TO messenger_migrator', item.signature);
    END LOOP;
END
$$;

-- The application: data only, no DDL, no ownership.
GRANT USAGE ON SCHEMA public TO messenger_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO messenger_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO messenger_app;
-- Tables and sequences created by future migrations get the same grants automatically.
ALTER DEFAULT PRIVILEGES FOR ROLE messenger_migrator IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO messenger_app;
ALTER DEFAULT PRIVILEGES FOR ROLE messenger_migrator IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO messenger_app;

-- Migration bookkeeping is not the application's business.
REVOKE ALL ON TABLE public.alembic_version FROM messenger_app;

COMMIT;

-- Summary for the operator (no secrets).
SELECT rolname, rolsuper, rolcreatedb, rolcreaterole, rolconnlimit
FROM pg_roles WHERE rolname IN ('messenger_app', 'messenger_migrator', current_user) ORDER BY rolname;
SELECT count(*) AS public_objects_owned_by_migrator
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'public' AND pg_get_userbyid(c.relowner) = 'messenger_migrator';
