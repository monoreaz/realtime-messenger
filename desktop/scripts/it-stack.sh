#!/usr/bin/env bash
# Throwaway backend for desktop integration tests (never touches production data).
# Runs on the build host: tmpfs PostgreSQL + the existing backend image (code mounted
# read-only) + Caddy with the production routing (/api/* -> backend, /ws).
# Usage: scripts/it-stack.sh up|web|down    (from agent-01; executes over ssh)
set -euo pipefail
HOST=${BUILD_HOST:-messenger-01}
NET=desktop_it
case "${1:-}" in
up)
ssh -o BatchMode=yes "$HOST" bash -s <<'REMOTE'
set -euo pipefail
NET=desktop_it
docker network inspect $NET >/dev/null 2>&1 || docker network create $NET >/dev/null
docker rm -f it-db it-backend it-proxy >/dev/null 2>&1 || true
docker run -d --name it-db --network $NET --tmpfs /var/lib/postgresql:rw \
  -e POSTGRES_USER=it -e POSTGRES_PASSWORD=it -e POSTGRES_DB=it postgres:18-alpine >/dev/null
for i in $(seq 1 30); do docker exec it-db pg_isready -U it -d it >/dev/null 2>&1 && break; sleep 1; done
mkdir -p /tmp/desktop-it && echo dummy-turn-secret > /tmp/desktop-it/turn-secret
cat > /tmp/desktop-it/Caddyfile <<'CADDY'
:80 {
    handle_path /api/* {
        reverse_proxy it-backend:8000
    }
    handle /ws {
        reverse_proxy it-backend:8000
    }
    handle {
        root * /web
        try_files {path} /index.html
        file_server
    }
}
CADDY
docker run -d --name it-backend --network $NET \
  -e DATABASE_URL=postgresql+asyncpg://it:it@it-db:5432/it -e JWT_SECRET_KEY=integration-test-only \
  -e ACCESS_TOKEN_EXPIRE_MINUTES=60 -e COOKIE_SECURE=false -e PUSH_ENABLED=false \
  -e TURN_SECRET_FILE=/it/turn-secret -e PYTHONDONTWRITEBYTECODE=1 \
  -v $HOME/messenger/backend:/app:ro -v /tmp/desktop-it:/it:ro --tmpfs /app/uploads:rw \
  messenger-backend sh -c "alembic upgrade head >/dev/null && uvicorn app.main:app --host 0.0.0.0 --port 8000" >/dev/null
mkdir -p /tmp/desktop-it/web
docker run -d --name it-proxy --network $NET -v /tmp/desktop-it/Caddyfile:/etc/caddy/Caddyfile:ro -v /tmp/desktop-it/web:/web:ro caddy:2-alpine >/dev/null
for i in $(seq 1 40); do docker logs it-backend 2>&1 | grep -q "Application startup complete" && break; sleep 1; done
# Seed verified test accounts (email NULL ⇒ no verification needed); alice is site admin.
docker exec -i it-backend python - <<'PY'
import asyncio
from app.database import AsyncSessionLocal
from app.models.user import User
from app.core.security import hash_password
async def main():
    async with AsyncSessionLocal() as db:
        for name, admin in (("alice", True), ("bob", False), ("carol", False)):
            db.add(User(username=name, display_name=name.title(), password_hash=hash_password("password123"), is_admin=admin))
        await db.commit()
asyncio.run(main())
PY
echo "integration stack ready on network $NET (http://it-proxy)"
REMOTE
;;
web)
# Static build of the current web frontend (served by it-proxy at /), for browser interop tests.
ssh -o BatchMode=yes "$HOST" 'mkdir -p /tmp/desktop-it/web && docker run --rm -u $(id -u):$(id -g) -v $HOME/messenger/frontend:/app:ro \
  -v messenger_frontend_node_modules:/app/node_modules:ro -v /tmp/desktop-it/web:/out -w /app -e HOME=/tmp node:22-alpine \
  node node_modules/vite/bin/vite.js build --configLoader runner --outDir /out --emptyOutDir | tail -2'
;;
down)
ssh -o BatchMode=yes "$HOST" 'docker rm -f it-db it-backend it-proxy >/dev/null 2>&1; docker network rm desktop_it >/dev/null 2>&1; rm -rf /tmp/desktop-it; echo stopped'
;;
*) echo "usage: $0 up|web|down" >&2; exit 2;;
esac
