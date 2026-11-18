# Realtime Messenger — notes for AI coding sessions

## Layout
- `backend/` — FastAPI + SQLAlchemy (async) + PostgreSQL + Alembic. REST under `/api` (Caddy strips the
  prefix), realtime on `/ws` (JSON events; see `backend/app/api/ws.py`).
- `frontend/` — React 19 + Vite web client ("graphite glass" design; tokens in `src/index.css`).
- `android/` — WebView wrapper app; built by `.github/workflows/android.yml` (do not break it).
- `desktop/` — **native Windows client in Rust + iced 0.14** (no WebView/Electron/Tauri).
  Read `desktop/PROGRESS.md` first: architecture, parity checklist, build status, next steps.

## Deployment facts
- Production runs on VM messenger-01 (`~/messenger`, Docker Compose: db, backend, frontend (Vite dev),
  caddy, turn). Public URL (tailnet only): https://messenger-01.tail1a26d2.ts.net
- Never modify the production database or compose stack without asking the owner.
- Secrets (`.env`, `secrets/`, `android/app/google-services.json`) are gitignored — never commit them.

## Desktop development
- Build/test in Docker on the Linux build host: `desktop/scripts/remote-cargo.sh test` and
  `desktop/scripts/remote-cargo.sh check --target x86_64-pc-windows-msvc`. Windows binaries are built
  by GitHub Actions (`windows-latest`).
- TLS is native-tls (SChannel on Windows). Keep dependencies pure-Rust where possible, so the Windows
  type-check works from Linux.
- Wire types mirror `backend/app/schemas`; when the backend API changes, update `desktop/src/api/`.
- Update signing: only the public key lives in `desktop/update-public-key.txt`; the private key is a
  GitHub Actions secret. Never commit it.
- Keep `desktop/PROGRESS.md` current after meaningful progress.

## Git
- Work on feature branches; ask before pushing to GitHub or publishing releases.
- Do not rewrite or delete existing branches/worktrees (several belong to earlier Codex sessions).
