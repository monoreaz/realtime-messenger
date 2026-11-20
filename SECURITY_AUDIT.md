# Slaxz — Security Audit (web app, backend, deployment)

Date: 2026-11-20 · Branch: `security/audit` (based on `main` at `6504c55`) · Status: **fixes prepared, not deployed**

Scope: web frontend (React/Vite), FastAPI backend and WebSocket, PostgreSQL access, file
uploads, dependencies, and the Docker Compose / Caddy deployment. Desktop and Android clients
are out of scope. The app was treated as reachable by untrusted internet users, even though it
is currently published only inside the tailnet.

This report contains no secrets, tokens or user data. Values of secrets were never printed;
only their locations and types are mentioned.

## Contents

1. [Method and environment](#1-method-and-environment)
2. [Findings overview](#2-findings-overview)
3. [Findings in detail](#3-findings-in-detail)
4. [Checks completed without findings](#4-checks-completed-without-findings)
5. [Not tested / requires verification](#5-not-tested--requires-verification)
6. [Dependencies](#6-dependencies)
7. [What is (and is not) encrypted](#7-what-is-and-is-not-encrypted)
8. [Tests and results](#8-tests-and-results)
9. [Deployment and rollback](#9-deployment-and-rollback) (phases 0–4)
10. [Summary](#10-summary)

Additional sections: D-1 (disk usage) and C-1 (client compatibility) at the end of §3.

---

## 1. Method and environment

- **Code and configuration review** of the whole backend (`backend/app`, ~5.4k lines), the web
  client's security-relevant code (`frontend/src`), `compose.yml`, `Caddyfile`, `.env.example`
  and the git history.
- **Read-only checks against production** (messenger-01): listening sockets, published
  ports, Tailscale serve/funnel state, container list, HTTP status of a few public paths and
  response headers through the local Caddy port, installed package versions (from the image,
  not the running container), whether the application's database role is a superuser, and the
  lengths (not values) of two secrets. After the incident, also read-only: container health,
  Docker volume sizes and the names of top-level entries in unattached volumes, and
  backend and PostgreSQL log summaries for the outage window (counts and status codes, ids
  masked). No message content or other user data was read, and nothing was changed.
- **Active tests only in isolated environments with synthetic users and throwaway databases.**
  - Early tests ran in throwaway Docker containers on messenger-01 (own network, tmpfs
    Postgres). See the incident note below.
  - After the incident, all active tests ran on **agent-01** under hard limits: a systemd
    scope (1.5–2 GB memory, no swap, 1.5 CPUs, 256 tasks, maximum runtime) plus `timeout`,
    inside bubblewrap with **no network**, the code mounted read-only and a tmpfs for uploads.
    The tools:
    - **Databases:** embedded PostgreSQL 16.2 (`pgserver`) for fast runs, and
      **PostgreSQL 18.7** for production parity. The 18.7 build comes from the official
      PGDG packages, with the repository signature and package hashes verified, unpacked
      without installation. Data lives in tmpfs.
    - **Python packages:** pinned to the exact versions installed in the production image.
    - **Caddy 2.11.9:** official release, checksum verified, running the new Caddyfile.
    - **Browsers:** headless Chromium (Playwright) for the CSP, media and call checks, with
      a fake microphone and camera.
    - **Node:** from PyPI's `nodejs-wheel-binaries`.

> **Incident during the audit (2026-11-20, ~22:40–23:11).**
>
> - **What happened:** a regression test was run against the original code to prove it
>   fails. On that code it looped forever: a mocked WebSocket kept returning the same message
>   and each iteration grew memory. It ran in a container on messenger-01 with no memory
>   limit. It exhausted the VM's RAM, so the VM stopped responding to SSH, Tailscale and
>   HTTPS (about 22:40). The kernel OOM-killed the test process (7.4 GB RSS) at 23:08, and
>   the owner reset the VM at 23:11.
> - **What the logs show** (read-only):
>   - PostgreSQL kept running, slowly, through the freeze: timed checkpoints completed at
>     22:43 and 23:03.
>   - It received a *fast shutdown request* at 23:11:09 and wrote a shutdown checkpoint.
>   - On boot it reported "database system was shut down", i.e. no crash recovery was
>     needed.
>   - The backend's access log has no entries between 22:36 and 23:08. All logged
>     responses in the window are 2xx/3xx, plus two failed requests at 23:11:10 while the
>     database was already shutting down.
> - **What this does not prove:** a clean shutdown and healthy checks show the database is
>   consistent. They do **not** prove that nothing was lost:
>   - Requests that could not reach the frozen server never appear in any log. A message
>     someone tried to send in that window may simply have failed on their device.
>   - Push notifications and "last seen" updates in the window may be missing.
>   - The newest database backup is from 11:02 the same day, so the state between 11:02
>     and the incident cannot be compared against anything.
>   - Optional read-only checks are in §9 (Phase 0): `pg_amcheck`, and row counts against the 11:02
>     dump.
> - **What changed:** the test now uses a finite script of socket messages. All later
>   active tests ran on agent-01 under hard memory, CPU, process and time limits, with no
>   network.

## 2. Findings overview

| ID | Severity | Status | Finding | Fix |
|----|----------|--------|---------|-----|
| H-1 | High | Confirmed | Logout, session revocation and password change/reset do not invalidate access tokens or open WebSockets | Implemented, tested |
| H-2 | High | Confirmed | No rate limits on login, registration, password reset or messaging; reset emails have no cooldown | Implemented, tested |
| H-3 | High | Confirmed | Production serves the **Vite development server** (source code, dev endpoints; Vite 8.2.1 dev-server advisories) | Prepared (static build), tested |
| H-4 | High | Confirmed | Unauthenticated clients can send unlimited request bodies, which are written to disk **before** authentication | Implemented, tested |
| M-1 | Medium | Confirmed | Attachments are public by URL, stay downloadable after deletion, and the client-declared type is trusted | Implemented, tested (per-file authorization, Range, cleanup tool) |
| M-2 | Medium | Confirmed | No security headers (CSP, framing, nosniff, referrer policy) | Implemented in Caddy, tested |
| M-3 | Medium | Confirmed | The application's PostgreSQL role is a superuser | Prepared and tested on PostgreSQL 18 (not applied to production) |
| L-1 | Low | Confirmed | CORS allows development origins (including a plain-HTTP LAN origin) with credentials | Implemented, tested |
| L-2 | Low | Confirmed | Interactive API docs and OpenAPI schema are public | Implemented, tested |
| L-3 | Low | Confirmed (config) | Backend cannot see client IPs (all requests appear to come from the proxy) | Prepared; tailscale-serve part requires verification |
| L-4 | Low | Confirmed | WebSocket frames up to 16 MiB; sockets outlive token expiry; typing events unthrottled | Mostly fixed (frame cap, expiry); typing throttle open |
| L-5 | Low | Confirmed | A caller can rewrite the text of server-written call-history entries | Implemented, tested |
| L-6 | Low | Confirmed (config) | Very long session lifetimes (production: non-"remember me" sessions valid ~150 days; example file: 60 h access tokens) | Example fixed; production value is the owner's decision |
| L-7 | Low | Confirmed (config) | Container/host hardening: root containers, writable source mount with `--reload`, coturn as root on host network, `.env` world-readable | Recommendations |
| L-8 | Low | Confirmed (code) | Anyone can add anyone to a group; user blocks do not apply to groups; members cannot leave | Implemented, tested (blocks, rate limit, leave); invite consent open |
| L-9 | Low | Code review | Push device token can be re-bound to another account by anyone who knows it | Not fixed |
| L-10 | Low | Confirmed | `certifi` in the backend image bundles CA roots flagged as compromised | Rebuild image |
| L-11 | Low | Confirmed | Account enumeration via registration errors and login timing | Accepted risk; noted |
| D-1 | Operational | Confirmed | Disk 93% full on messenger-01 | Exact deletion list for approval |
| C-1 | Compatibility | Confirmed | Desktop ≤ 0.1.4 downloads media without credentials, so its media breaks once M-1 is deployed | Desktop update must ship first |


## 3. Findings in detail

### H-1 · Revoked sessions keep working (access tokens and WebSockets)

- **Severity:** High · **Status:** confirmed (isolated stack, probe and unit tests)
- **Location:** `backend/app/core/security.py` `create_access_token` / `decode_access_token`;
  `backend/app/core/dependencies.py` `get_current_user`; `backend/app/api/ws.py`
  `websocket_endpoint`; `backend/app/api/auth.py` (`logout`, `revoke_session`,
  `logout_other_sessions`, `change_password`, `reset_password`).
- **Problem:** access tokens are JWTs that carry only the user id. Revoking the refresh
  session (logout, "log out other sessions", revoke one session, password change or reset)
  did not affect access tokens that were already issued (valid for 30 minutes in production).
  WebSockets were authenticated once and then stayed open indefinitely: they never re-checked
  the session or even the token's expiry.
- **Prerequisites / impact:** an attacker who obtained an access token (or a session on a lost
  device) keeps reading and sending messages after the victim logs out or changes their
  password. Over an open WebSocket they keep receiving every new message **without any time
  limit**.
- **Safe reproduction (isolated stack):** log in as test user A and open a WebSocket with the
  access token; `POST /auth/logout` with A's refresh cookie; `GET /users/me` with the old
  token still returned 200, and the socket still answered `ping`. Same for
  `DELETE /auth/sessions/{id}` against a second session of A.
- **Fix (implemented):**
  - Access tokens now carry the refresh session id (`sid`). `get_current_user` and the
    WebSocket (on connect **and** on every received message) require that session to exist,
    belong to the user, and be neither revoked nor expired.
  - The WebSocket also closes once its access token has expired. The clients reconnect with a
    fresh token.
  - The connection manager records each socket's session. Logout, session revocation and
    "log out others" close that session's sockets immediately. Password change and reset
    close all of the user's sockets.
  - The web client now handles a policy close (1008) by renewing its token. If the refresh
    session is gone, it signs out; before, it retried with the same token every 3 s.
  - **Compatibility:** tokens issued before the upgrade have no `sid`. They are still
    accepted until they expire (at most `ACCESS_TOKEN_EXPIRE_MINUTES`, 30 min in production),
    so nobody is logged out by the deployment. No database migration is needed.
- **Tests:** `test_logout_revokes_the_access_token`,
  `test_revoking_another_session_revokes_its_token_and_socket`,
  `test_password_change_revokes_tokens_and_closes_sockets`,
  `test_websocket_refuses_a_revoked_session`, `test_websocket_closes_once_its_token_expired`.
  The end-to-end probe shows all four session checks PASS. The browser check confirms that an
  open web tab is signed out when another session uses "log out others".

#### Why existing sessions stay valid while revocation is now enforced

There are two kinds of credentials, and the change touches only how one of them is
*checked*:

- **Refresh sessions** are rows in `auth_sessions`, referenced by the HttpOnly refresh
  cookie. They are not changed at all. Nobody is logged out by the deployment, "remember me"
  keeps working, and existing cookies keep refreshing.
- **Access tokens** are short JWTs (30 minutes in production). New tokens carry `sid`, the id
  of their refresh session. A token with `sid` is accepted only while that session row is
  not revoked and not expired. Every revocation path (logout, revoking one session,
  "log out other sessions", password change, password reset, admin block) sets
  `revoked_at`, so from that moment:
  - REST calls with the session's tokens get 401;
  - its WebSockets are closed;
  - its refresh cookie no longer works;
  - its media requests get 401.

**Legacy tokens** were issued before the deployment and have no `sid`:

- They are still accepted (signature and expiry checked), because there is no session to
  look up. This window lasts at most `ACCESS_TOKEN_EXPIRE_MINUTES` (30 minutes) after
  deployment. In that window a legacy token behaves as before: it is not revocable.
- WebSockets opened with a legacy token are closed when that token expires; the client
  reconnects with a fresh, session-bound token.
- Clients replace their tokens on their own:
  - the web app (and the Android app, which wraps it) on page load, every 10 minutes and
    on a WebSocket policy close;
  - the desktop app on 401 or a policy close.

  Each refresh issues a `sid` token from the same cookie, so after at most 30 minutes no
  legacy token remains.
- **To end the window immediately** (optional, owner's decision): rotate `JWT_SECRET_KEY`
  at deployment. All access tokens become invalid at once, and clients transparently get
  new ones from their refresh cookies. Refresh sessions are database rows and do not depend
  on the key, so nobody is logged out. Call-decline tokens in flight (60 s) would also be
  invalidated.
- **Follow-up:** once the window has passed, a small change can require `sid` and remove
  the legacy path.
- **If the code is rolled back,** the old code ignores the `sid` claim. Tokens issued by the
  new code stay valid until they expire, with the old (non-revocable) behaviour.

### H-2 · No rate limits; password-reset email flooding

- **Severity:** High · **Status:** confirmed
- **Location:** `backend/app/api/auth.py` (`login`, `register`, `forgot_password`,
  `resend_verification`, `reset_password`, `verify_email`, `change_password`);
  `backend/app/api/messages.py` (send endpoints).
- **Problem:** unlimited login attempts allowed online password guessing. Each attempt also
  costs an Argon2 hash, so a flood of attempts is a CPU DoS. `forgot-password` sent a new email
  on every request for a verified address, with no cooldown, so it could flood a victim's
  inbox and burn the SMTP sender's reputation. Registration and messaging were unlimited
  (spam).
- **Safe reproduction:** 25 wrong passwords for a synthetic account were all answered 401,
  never 429; 15 `forgot-password` requests were all accepted. Code review confirms the missing
  email cooldown.
- **Fix (implemented):** `backend/app/core/rate_limit.py`, an in-process sliding-window
  limiter with bounded memory.

  | Endpoint | Limit |
  |---|---|
  | Login, per IP | 30 per 5 min |
  | Login, per account (any IP) | 10 failures per 15 min, then 429 |
  | Registration | 10 per hour per IP |
  | `forgot-password` | 10 per 15 min per IP; **one email per address per 60 s** (still answers 204, no enumeration) |
  | `resend-verification` | 10 per 15 min per IP |
  | `reset-password` | 20 per 15 min per IP |
  | `verify-email` | 30 per 15 min per IP |
  | Password change | 10 per 15 min per user |
  | Sending messages | 30 per 10 s per user |

  Configurable with `RATE_LIMIT_ENABLED` and `PASSWORD_RESET_COOLDOWN_SECONDS`.
- **Limitations:**
  - Per-IP limits need real client IPs (see L-3); without them they act as global limits.
  - The limiter is per process; use a shared store (e.g. Redis) before running several
    workers or replicas.
  - The per-account lockout can be triggered by someone else to lock a user out for 15
    minutes. That is the usual trade-off; consider CAPTCHA or e-mail unlock later.
- **Tests:** `test_login_locks_an_account_after_repeated_failures`,
  `test_login_is_limited_per_ip`, `test_password_reset_email_has_a_per_address_cooldown`,
  `test_message_sending_is_rate_limited`. In the probe, the login and reset checks and the
  forged-XFF check all PASS.

### H-3 · Production runs the Vite development server

- **Severity:** High · **Status:** confirmed (read-only check on production)
- **Location:** `compose.yml` service `frontend` (`npm run dev -- --host 0.0.0.0`);
  `Caddyfile` (`reverse_proxy frontend:5173`).
- **Problem:** every request for the web app goes to Vite's dev server. On production,
  `GET /src/main.tsx` and `GET /@vite/client` return 200: unminified source and dev-server
  endpoints are public. The dev server is not designed to face untrusted clients. The
  installed Vite 8.2.1 is affected by dev-server advisories: GHSA-rq7h-c2jc-7f22 (files
  outside `server.fs.allow`), GHSA-vfpm-58rq-9qcg (first bytes of arbitrary files),
  GHSA-9jrq-w75r-8gcw, and the `launch-editor` advisories (mostly Windows). The container holds
  no secrets, which limits the impact of file disclosure, but the exposure is unnecessary.
- **Fix (prepared):**
  - Caddy now serves the static production build (`frontend/dist`, mounted read-only at
    `/srv`, SPA fallback, APK downloads, immutable caching for hashed assets).
  - A one-shot `frontend-build` compose service builds it. The dev server is moved to an
    opt-in `dev` profile and is no longer proxied.
  - Vite is updated to **8.2.6** (minimal patch). The lockfile change also brings the bundler
    `rolldown` 1.2.4 → 1.2.19, which Vite 8.2.6 requires, and `source-map-js` 1.2.1 → 1.2.4.
    `npm audit` now reports 0 vulnerabilities.
- **Tests:** type-check OK; 9/9 frontend unit tests; production build OK. The built app,
  served by the new Caddyfile, works in Chromium: sign-in, realtime connection, styles.
- **Workflow change:** after frontend changes, run `docker compose run --rm frontend-build`.
  There is no hot reload on the public site any more.

### H-4 · Unlimited, pre-authentication request bodies

- **Severity:** High · **Status:** confirmed
- **Location:** `backend/app/main.py` (no body limit); FastAPI parses form bodies before it
  resolves dependencies such as authentication; Caddy had no `request_body` limit.
- **Problem / impact:** an **unauthenticated** client can send a multipart body of any size to
  an upload endpoint. The whole body is parsed and spooled to the container's disk (= the
  host's disk) before the request is rejected with 401. A few parallel requests can fill the
  disk. The disk is shared with PostgreSQL, and only 3.6 GB is currently free.
- **Safe reproduction (isolated stack):** a 1.5 GB anonymous upload to
  `/chats/<random>/messages/image` returned 401 only after the full body had been accepted;
  host disk usage rose by **1500 MB** during the request. Unlinked temporary files are
  invisible to `du`, so this was measured with `df`.
- **Fix (implemented):**
  - `backend/app/core/body_limit.py` is outermost middleware. It enforces per-route limits
    *before* anything reads the body: video 51 MiB, image 11 MiB, avatar 6 MiB (each file
    limit plus 1 MiB envelope), 256 KiB for everything else. Requests are rejected from
    `Content-Length`, and streamed bodies are cut off as soon as they cross the limit; the
    answer is 413.
  - Caddy enforces the same limits per route, so it answers 413 itself instead of a 502. Caddy
    also applies 64 KiB for static paths.
  - uvicorn `--ws-max-size 262144` caps WebSocket frames (L-4).
- **Tests:** `test_oversized_anonymous_upload_is_refused_before_parsing`,
  `test_streamed_body_without_length_is_cut_off`. In the probe, a 40 MB anonymous upload is
  refused immediately and a 2 MB login body gets 413.

### M-1 · Attachments: public URLs, kept after deletion, type not verified

- **Severity:** Medium · **Status:** confirmed · **Fix:** implemented and tested
- **Location (before):** `backend/app/main.py` served `/uploads` with a public StaticFiles
  mount; `backend/app/api/messages.py` and `backend/app/api/users.py` trusted the
  client-declared type and kept files after deletion.
- **Problems:**
  1. Message images and videos were served to **anyone with the URL**: no sign-in and no
     membership check. The URLs contain 128 random bits, but they leak through copy and
     paste, screenshots, logs or a former member's cache.
  2. Deleting a message left its file on disk, still downloadable.
  3. Any bytes were stored under an image or video name if the client claimed that type.
- **Fix:**
  - `backend/app/api/media.py` replaces the public mount **at the same URLs**, so stored
    URLs and clients keep working. Every request is **authenticated**, by Bearer token or by
    the HttpOnly refresh-session cookie: browsers load `<img>`/`<video>` without an
    Authorization header, and the cookie is same-origin and `SameSite=Lax`, so it is never
    sent on cross-site subresource requests. Then the request is **authorized for that
    specific file**:
    - **Message attachments:** the file must belong to a message that is not deleted, in a
      chat the requester is currently a member of. Reply previews and thumbnails use the
      same URL, so the same check covers them. There are no server-generated thumbnails or
      link previews.
    - **Avatars:** the file must be someone's *current* avatar. Any signed-in user may see
      it, because profiles are visible to all signed-in users.
    - Revoked or expired sessions, suspended accounts and anonymous requests get **401**.
      "Not allowed" and "does not exist" both get **404**, so file names cannot be probed.
  - **Range requests** (video seeking) are served by `FileResponse` only after those
    checks.
  - Responses carry `Cache-Control: private, max-age=300`, `nosniff`,
    `Content-Disposition: inline` and the allow-listed content type. Path parameters are
    restricted to the two folders and to safe file names with allowed extensions.
  - Deleting a message deletes its files. Uploads must match the declared type's file
    signature (JPEG, PNG, WebP, MP4, WebM).
  - `python -m app.cli.cleanup_uploads` lists files that no message or profile references,
    such as files of messages deleted before this fix. It is a dry run by default and
    prints counts and sizes only; `--delete` removes them. Files younger than 10 minutes
    are skipped.
- **Tests** (`backend/tests/test_media_groups.py`, `MediaAccessTests`, plus
  `OrphanCleanupTests`):
  - an **unrelated signed-in user gets 404 for another chat's file**, with Bearer, with the
    cookie, and with a Range header (no bytes returned);
  - members can download with Bearer or cookie;
  - Range returns 206 with exactly the requested bytes;
  - anonymous, forged-cookie, revoked-cookie, logged-out-token and suspended-account
    requests get 401;
  - videos, reply previews and avatars (current versus removed) follow the same rules;
  - deleted messages' files are gone even for members;
  - **leaving a group ends access to its files**;
  - traversal attempts are refused.

  On the original code, 8 of the 9 media tests fail. The traversal test passes there too,
  because StaticFiles was already safe.
- **End to end:** two real Chromium sessions under the new CSP: the recipient's browser
  displays the image and loads the video's metadata through the protected route (cookie +
  Range).

### M-2 · Missing security headers

- **Severity:** Medium · **Status:** confirmed (production responses carry none of them)
- **Impact:** without `frame-ancestors`/`X-Frame-Options`, the app can be framed by other
  sites (clickjacking, e.g. "block user" or "log out others"). Without a CSP, any future XSS
  has no second line of defence.
- **Fix (implemented in `Caddyfile`):**
  - `Content-Security-Policy` with `script-src 'self'` (no inline or eval scripts in the
    build), `style-src 'self' 'unsafe-inline'` (React style attributes), `img/media` including
    `blob:`/`data:`, `connect-src 'self' wss://{host}`, `object-src 'none'`,
    `frame-ancestors 'none'`, `base-uri`/`form-action 'self'`.
  - `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`
    (password-reset tokens appear in the URL briefly).
  - `Permissions-Policy` allows only the microphone (calls), plus
    `Cross-Origin-Opener-Policy` and HSTS; the `Server` header is removed.
- **Tests:** probe header checks PASS; Chromium check: **0 CSP violations** while signing in
  and using the app.

### M-3 · The application's database role is a PostgreSQL superuser

- **Severity:** Medium · **Status:** confirmed (read-only `rolsuper` query on production)
  · **Fix:** prepared and tested; **not applied to production**
- **Impact:** no SQL injection was found (SQLAlchemy ORM throughout, LIKE patterns escaped).
  But if one ever appears, a superuser can read every database, read and write server files
  and run programs (`COPY … PROGRAM`) inside the database container. The backend container
  also held the superuser password.
- **Prepared change:**
  - `ops/db/least-privilege.sql` (idempotent; passwords come from the environment, never
    the command line). It creates two roles, both `NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOREPLICATION NOBYPASSRLS NOINHERIT`:

    | Role | Rights |
    |---|---|
    | `messenger_migrator` | Owns the `public` schema and all its tables, sequences and types; used only for migrations |
    | `messenger_app` | `SELECT/INSERT/UPDATE/DELETE` on tables and `USAGE/SELECT` on sequences only, plus default privileges so future migrations' tables are granted automatically; no access to `alembic_version`; connection limit 50 |

    It also revokes `CONNECT`/`TEMP` on the database from `PUBLIC`. The bootstrap superuser
    stays for administration.
  - `ops/db/least-privilege-rollback.sql` reassigns everything to the bootstrap superuser,
    restores the defaults and drops both roles.
  - `compose.yml`:
    - the backend uses `APP_DB_USER`/`APP_DB_PASSWORD`, falling back to the superuser
      until they are set, so the code can ship before the database change;
    - a one-off `migrate` service (`docker compose run --rm migrate`) uses
      `MIGRATOR_DB_*` and the existing image;
    - the long-running backend never holds the migration credentials;
    - fallbacks verified with `docker compose config` and dummy values.
- **Tests on PostgreSQL 18.7** (PGDG build; signature and hashes verified; production runs
  18.6):
  1. Schema built by the bootstrap superuser, as in production today; synthetic data added.
  2. Role script applied **twice**: idempotent, 41 objects owned by the migrator.
  3. The **full backend suite (82 tests) passes as `messenger_app`**, including
     `test_db_privileges.py`. That module proves the role is not privileged and owns
     nothing, that data access works, and that `CREATE/ALTER/DROP/TRUNCATE`, temp tables,
     `alembic_version`, `pg_authid`, `COPY … PROGRAM`, `pg_read_file` and `CREATE ROLE` are
     all refused.
  4. `alembic downgrade -1` / `upgrade head` **work as `messenger_migrator`** and are
     **refused as `messenger_app`** (`InsufficientPrivilege`). The re-created table is
     usable by the app through default privileges.
  5. Row counts unchanged throughout.
  6. Rollback script: everything returns to the superuser, the roles are gone, and the
     suite passes again as superuser.

### L-1 · CORS allowed development origins with credentials

- **Location:** `backend/app/main.py`. Allowed `http://localhost:5173`, `http://127.0.0.1:5173`
  and two LAN/tailnet `http://…:5173` origins, with `allow_credentials=True`.
- **Impact:** low today. The refresh cookie is `SameSite=Lax` and `Secure`, so cross-site
  `fetch` does not send it. Still, any page served from those plain-HTTP origins (e.g. another
  dev tool on a LAN host) was trusted.
- **Fix:** origins now come from `CORS_ALLOWED_ORIGINS` (empty by default). Production is
  same-origin and needs none. **Test:** `test_no_cross_origin_access_for_development_origins`.

### L-2 · Public API documentation

- `/api/docs`, `/api/redoc` and `/api/openapi.json` returned 200 on production and mapped the
  whole API. They are now off unless `ENABLE_API_DOCS=true`. **Test:**
  `test_api_docs_are_not_public`.

### L-3 · Client IP addresses are lost behind the proxies

- **Status:** confirmed by configuration review.
- **Problem:** uvicorn did not trust Caddy's `X-Forwarded-For`, and Caddy did not trust the
  local tailscale serve hop. As a result every request appeared to come from a proxy address:
  no per-client attribution, and per-IP limits would act globally.
- **Fix (prepared):**
  - Caddy trusts only private-range peers (`trusted_proxies static private_ranges` with
    `trusted_proxies_strict`, i.e. the right-most untrusted address). It passes the backend
    one value (`header_up X-Forwarded-For {client_ip}`).
  - uvicorn runs with `--proxy-headers --forwarded-allow-ips '*'`. The backend has no
    published port, so only containers on the Compose network can reach it.
  - If Caddy is ever exposed directly, client-sent `X-Forwarded-For` is ignored, because
    public peers are not trusted.
- **Tests:** in the probe, rotating forged left-most `X-Forwarded-For` values do not avoid the
  per-IP login limit, and a different client address is unaffected.
- **Requires verification:** that tailscale serve sets or appends `X-Forwarded-For` with the
  real tailnet client address. I could not test this without touching production. Check it
  after deployment (see §9).

### L-4 · WebSocket limits

- Frame size was uvicorn's default 16 MiB; now 256 KiB (`--ws-max-size`; SDP offers are at
  most 64 KiB).
- Sockets now close at token expiry (see H-1).
- **Open:** typing events are broadcast without a per-connection rate limit, and an
  unauthenticated socket may wait 10 s for its auth message. Both are low risk.

### L-5 · Call-history entries could be edited by the caller

- `PATCH /chats/{id}/messages/{id}` allowed the caller to rewrite the text of call records,
  which the server writes as messages with UUIDv5 ids. That allowed forged "call" entries.
  Now 403. **Test:** `test_call_history_entries_cannot_be_rewritten`.

### L-6 · Session lifetime settings

- Production sets `REFRESH_TOKEN_SESSION_HOURS=3600`. Sessions **without** "remember me" are
  therefore valid on the server for about 150 days, although their cookie is a browser-session
  cookie.
- `.env.example` suggested `ACCESS_TOKEN_EXPIRE_MINUTES=3600` (60-hour access tokens).
  Production uses 30; the example is now 30 too.
- Recommendation: `REFRESH_TOKEN_SESSION_HOURS=24`. This is the owner's decision: it shortens
  existing sessions only for refreshes after the change.

### L-7 · Container and host hardening (recommendations)

- The backend runs as root with `./backend` bind-mounted **read-write** and `--reload`. Code
  execution in the backend could therefore change the source on the host, persistently.
  Recommendation: mount read-only, drop `--reload` in production, run as a non-root user.
- The frontend container ran as root; it is no longer needed in production after H-3.
- coturn runs as root with `network_mode: host` (needed for media ports). Consider
  `user: nobody` and restricting peer IP ranges in `turnserver.conf`. TURN was **not**
  reviewed in depth.
- `.env` (holds secrets) has mode `0664`. Recommend `chmod 600`. `secrets/` is already `0700`
  with `0600` files.
- No container is privileged; none mounts the Docker socket. Only Caddy publishes a port, on
  `127.0.0.1:8080`. PostgreSQL, the backend and Vite publish no host ports.

### L-8 · Group membership abuse

- **Severity:** Low (High if registration is opened to strangers) · **Status:** confirmed by
  code review · **Fix:** implemented and tested, except invite consent
- **Problem:** public registration is supported (self-service sign-up with email
  verification), so strangers can create accounts. Any user could put any other users (up to
  99) into a group. Personal blocks did not apply to groups, and there was no way to leave
  one, so this was a harassment and spam channel.
- **Fix (`backend/app/api/chats.py`, web `GroupProfileDialog`):**
  - Nobody can add a user who blocked them, or whom they blocked. The error does not reveal
    who blocked whom.
  - Suspended accounts cannot be added.
  - Group creation is limited to 10 per hour per user.
  - **`POST /chats/{id}/leave`** with a "Leave group" button in group info. If the last
    administrator leaves, the longest-standing member becomes administrator; if nobody is
    left, the group (and its messages) is deleted. Leaving ends access to the group's
    messages and files immediately.
  - The web app closes a chat that disappears from the user's list, for example after
    leaving it on another device.
- **Tests** (`GroupInviteTests`): blocks in both directions, suspended accounts, the rate
  limit, leaving, admin hand-over, no access after leaving, last member deletes the group,
  and private chats cannot be "left". All fail on the original code.
- **Open (product decision):** invitations that must be accepted, or "who may add me to
  groups" (everyone / people I have chatted with). Until then, a blocked-then-unblocked or
  unknown user can still add someone once, and that person has to leave.
- **Desktop:** the desktop client has no "leave group" button yet (out of scope), so desktop
  users leave from the web app.

### L-9 · Push device re-binding

- `POST /push/devices` upserts by FCM token. Anyone who knows another device's FCM token could
  attach it to their own session, so the victim's device would receive the attacker's
  notifications and stop receiving its own. FCM tokens are not public, so the risk is low.
  Fix: refuse to move a token between users, or require a proof from the device.

### L-10 · certifi CA bundle

- `certifi 2026.7.22` bundles emSign roots with compromised intermediate keys
  (PYSEC-2026-4320). This affects outbound TLS from the backend (Firebase Cloud Messaging).
- Rebuild the backend image so `pip` picks up ≥ `2026.10.18`: `requirements.txt` is unpinned,
  so a rebuild is enough.

### L-11 · Account enumeration

- Registration answers "Username already exists" / "Email already exists", and login skips
  password hashing for unknown accounts (timing).
- Password reset and resend-verification correctly answer 204 either way.
- Accepted for now. With H-2 in place, enumeration is rate-limited.

### D-1 · Disk usage on messenger-01 (read-only investigation)

- **Status:** the root filesystem is 48 GB, 42 GB used (**93%**, 3.6 GB free).
- **Why it matters:** PostgreSQL, uploads and Docker share this disk. A full disk stops the
  database, and H-4 (now fixed) would have let anyone fill it.
- **Method:** `docker system df -v`, `docker volume inspect`, the containers attached to
  each volume, and the *names* of top-level entries inside unattached volumes. Names were
  listed with a throwaway read-only container: no network, 64 MB memory, read-only mount.
  No file contents were read and nothing was deleted.

**Docker volumes**

| Volume | Size | Attached to | Contents (top level) | Proposal |
|---|---:|---|---|---|
| `desktop_target` | **20.35 GB** | none | Rust build output for the desktop client (`debug/`, `release/`, `x86_64-pc-windows-msvc/`) | **Delete**: rebuilt by `desktop/scripts/remote-cargo.sh build` (takes build time only) |
| `desktop_cargo` | 0.96 GB | none | Cargo registry and download cache (`registry/`, `bin/`, …) | Optional delete: re-downloaded on the next desktop build |
| `desktop_rustup` | 0.78 GB | none | Rust toolchains (`toolchains/`, `downloads/`) | Optional delete: re-downloaded on the next desktop build |
| `messenger_android_gradle` | 0.70 GB | none | Gradle caches and daemon for Android builds | Keep (needed for APK builds; re-downloadable) |
| `messenger_android_sdk` | 0.43 GB | none | Android SDK (build-tools, platforms, licenses) | Keep (needed for APK builds) |
| `messenger_android_signing` | 6 MB | none | `debug.keystore`, `analytics.settings`, `cache/` | **Keep, and back it up.** If released APKs were signed with this key, losing it means future APKs cannot update installed apps |
| `76433891…` (anonymous) | 49 MB | none | A PostgreSQL 18 data directory (`18/`), created 2026-11-20 | **Needs your confirmation.** Probably a throwaway test database from today; not inspected further, because reading it could expose data |
| `messenger_avatar_data` | 91 MB | backend (running) | **All uploads** (avatars and message attachments) despite its name | Keep: production data |
| `messenger_postgres_data` | 67 MB | db (running) | Production database | Keep: production data |
| `messenger_frontend_node_modules` | 137 MB | frontend (running) | Web dependencies (also used by `frontend-build`) | Keep |
| `ce97d657…` (anonymous) | 0 B | turn (running) | coturn's declared volume | Keep |

**Stopped containers and networks**

| Name | Created | Writable layer | Contents | Proposal |
|---|---|---:|---|---|
| `sec-proxy`, `sec-backend`, `sec-db` | 2026-11-20 22:24 | ≤ 45 kB | Audit test stack: Caddy, backend, PostgreSQL with **tmpfs** data (nothing persisted) | Delete |
| `it-proxy`, `it-backend`, `it-db` | 2026-11-20 21:16 | ≤ 50 kB | Desktop integration-test stack, same pattern (tmpfs database) | Delete |
| `elated_chatelet` | 2026-08-15 | 4 kB | `hello-world` image's test run | Optional delete |
| networks `sec_audit`, `desktop_it` | — | — | Networks of those two stacks | Delete with the containers |

**Images, build cache and other usage:**

- `desktop-gui` (2.57 GB) and `desktop-browser` (1.47 GB) are desktop test tooling;
  rebuilding needs their Dockerfiles, so keep them unless you no longer need screenshots or
  call-interop tests.
- `rust:1-bookworm` (2.24 GB) and `gradle:8.11.1-jdk17` (1.15 GB) can be pulled again.
- Build cache: 3.76 GB.
- The systemd journal uses 0.97 GB.
- `/home/artur` uses 2.3 GB.

**Proposal for approval** (explicit names; no `prune`):

```sh
docker rm sec-proxy sec-backend sec-db it-proxy it-backend it-db
docker network rm sec_audit desktop_it
docker volume rm desktop_target        # frees ~20 GB -> about 51% used
# optional, after you confirm: desktop_cargo desktop_rustup, the anonymous 76433891… volume
```

### C-1 · Client compatibility

- **Desktop client 0.1.4 and older: media will stop loading.** Its `Client::download` /
  `download_to` (`desktop/src/api/client.rs`) fetch `/api/uploads/...` **without**
  credentials, because the uploads used to be public. After M-1 those requests get 401, so
  images, avatars and videos fail in the desktop app.
  - **Fix (desktop; outside this audit's scope):** send the same `Authorization`/cookie
    headers as the other API calls (`self.authorized(...)`).
  - **Release order:** release and install the desktop update **before** deploying M-1.
    Otherwise accept that desktop media is broken until the update.
- **Desktop client:** no "leave group" UI yet (L-8); users can leave from the web app.
- **Desktop integration test data:** the demo-seeding test uploads a fake `.mp4`, which the
  signature check now rejects. This affects test data only.
- **Web and Android (WebView of the same web app):**
  - **Tested in Chromium:**
    - sign-in;
    - realtime connection;
    - image and video composer previews (`blob:`);
    - sending, and display for the recipient (protected route, cookie, Range);
    - an **audio call** between two browsers (fake microphone, WebRTC over loopback);
    - session revocation.

    All with **0 CSP violations**.
  - **Not tested:**
    - Firefox and Safari;
    - the Android WebView on a device;
    - calls through the TURN relay (ICE traffic is not governed by CSP, but relay
      connectivity was not exercised);
    - Android push.
- **Open web tabs during deployment** keep running the old JavaScript until reloaded.
  Everything keeps working; only the faster token renewal on WebSocket close (H-1) arrives
  with the reload.

## 4. Checks completed without findings

Each of these was checked by code review and, where marked (P), by the isolated probe with
users A, B and C.

- **Authorization on every route** — all chat and message routes check membership (P: A cannot
  list, send, edit, delete, pin or mark read in B↔C's chat, or edit a group it is not in).
  Edit and delete require message ownership (P). Replies cannot reference another chat's
  message (P). Admin routes require `is_admin` (P); `is_admin` cannot be set by clients (P).
  Group edit requires the group admin role.
- **Tokens** — expired, wrongly signed and `alg=none` tokens are rejected (P). Call-decline
  tokens use a separate audience and cannot be used as access tokens.
- **WebSocket actions** — typing and presence go only to chat peers. `presence.get` for a
  non-peer reveals nothing (P). Call signalling is limited to the two participants of an
  existing private chat and respects blocks. Admin blocking closes sockets.
- **Passwords and tokens at rest** — passwords are hashed with Argon2 (pwdlib defaults).
  Refresh, verification and reset tokens are random and stored only as SHA-256 hashes.
  Refresh tokens rotate on use. The refresh cookie is `HttpOnly`, `Secure` and
  `SameSite=Lax`.
- **SQL injection** — SQLAlchemy ORM only. The single text query is constant. LIKE patterns use
  `autoescape`.
- **Command injection, unsafe deserialization** — no `subprocess`, `os.system`, `pickle` or
  `yaml.load` in the application.
- **SSRF** — the server never fetches user-supplied URLs (no link previews). Its only outbound
  calls are to SMTP and Firebase.
- **XSS** — the frontend has no `dangerouslySetInnerHTML`, `innerHTML` or `eval`. React escapes
  all text, links in messages are not rendered as HTML, and media URLs are generated by the
  server. Email bodies HTML-escape their inputs.
- **Path traversal** — upload filenames are generated by the server (UUID plus allow-listed
  extension); client filenames are never used. Starlette's StaticFiles checks paths.
- **CSRF** — the API authenticates with a Bearer header. The cookie-based `/auth/refresh` and
  `/auth/logout` are protected by `SameSite=Lax` and same-origin CORS.
- **WebSocket origin** — authentication is an in-band token, not a cookie, so cross-site
  WebSocket hijacking does not apply.
- **Token storage and URL leaks** — the access token is kept in memory (not in
  `localStorage`). Verification and reset tokens are removed from the address bar after use.
  No external resources are loaded (Referer leaks). Tokens never appear in API query
  strings.
- **Secrets** — all 96 commits on all branches were scanned for keys, passwords and secrets:
  only placeholders (`change_me`, `your-…`) were found. Real secrets live in the git-ignored
  `.env` and `secrets/`. No `VITE_*` variables or secrets are in the client bundle. The
  backend image contains only `app/` (secrets are mounted at runtime). Push payloads carry
  only ids, not message text.
- **Exposure** — only SSH and tailnet-bound services listen externally. The app is published
  through tailscale serve **inside the tailnet only**; Funnel is off. Caddy binds
  `127.0.0.1:8080`.
- **Error leakage** — no tracebacks in API responses. FastAPI returns generic messages.

## 5. Not tested / requires verification

**Tested since the first version of this report:**

- PostgreSQL 18 (18.7; production runs 18.6);
- calls and image/video previews under the CSP (Chromium);
- media authorization including Range;
- group changes;
- least-privilege roles.

**Still open:**

- **tailscale serve `X-Forwarded-For` behaviour** (L-3). Check after deployment that two
  tailnet devices are rate-limited independently.
- **Browsers and devices:** Firefox, Safari and the Android WebView on a real device were
  not tested under the new CSP and media cookie. Neither were calls through the TURN relay
  (as opposed to direct ICE).
- **Internet exposure:** the app is tailnet-only today. A public setup (Funnel or a port
  forward) needs §9's checks repeated, plus TLS and HSTS checks at the real edge.
- **TURN/coturn configuration**, **Firebase push** and **SMTP delivery** were not
  exercised.
- **Load and denial of service** were not tested, as instructed. Limits were verified
  functionally only.
- **Races** were reviewed, not tested: concurrent double use of one password-reset token;
  concurrent refresh rotation; leaving a group while being re-added.
- **Production data integrity after the incident:** not verified beyond the logs. See the
  incident note in §1 and the optional checks in §9 (Phase 0).
- **Desktop and Android clients:** out of scope apart from the compatibility notes in C-1.

## 6. Dependencies

Versions installed in the production image and lockfile, checked against OSV (Python, via
`pip-audit`) and the npm advisory database:

| Package | Advisory | Applies here? |
|---|---|---|
| vite 8.2.1 (dev server) | GHSA-rq7h-c2jc-7f22, GHSA-vfpm-58rq-9qcg, GHSA-9jrq-w75r-8gcw, launch-editor advisories | **Yes**, because production runs the dev server (H-3). Fixed: 8.2.6 and static serving |
| source-map-js 1.2.1 | GHSA-68fv-2mgg-jv7q (DoS parsing a crafted source map) | Build-time only, own files. Updated to 1.2.4 anyway |
| certifi 2026.7.22 | PYSEC-2026-4320 | Yes, for outbound TLS (L-10). Rebuild image |
| pip 25.0.1 | 6 advisories (archive extraction, install paths) | No: build-time tool, trusted index |
| pyasn1 0.6.4 | 4 quadratic-time DoS advisories | No: only parses trusted Google credentials |
| dnspython 2.9.0 | GHSA-vvf4-9x63-8xvv | No: e-mail validation runs without DNS lookups |
| opentelemetry-api 1.45.1 | GHSA-mf5v-w3w3-f6cg (traceparent parsing) | No: no tracing of incoming requests |

FastAPI 0.143.0, Starlette 1.8.0, uvicorn 0.54.0, PyJWT 2.15.1, python-multipart 0.0.32,
SQLAlchemy 2.1.4, websockets 17.2 and React 19.2.8 had no known advisories at audit time.

## 7. What is (and is not) encrypted

- **In transit:**
  - Browser ↔ messenger-01 is HTTPS, terminated by tailscale serve, and the tailnet itself is
    WireGuard-encrypted.
  - Inside the VM, tailscale serve → Caddy → backend is plain HTTP over loopback and the Docker
    network.
- **At rest:**
  - Messages, attachments and profiles are stored **unencrypted** in PostgreSQL and on disk.
    Disk-level encryption of the VM was not assessed.
  - Passwords are Argon2 hashes; refresh, verification and reset tokens are SHA-256 hashes.
- **There is no end-to-end encryption.** Anyone with access to the server, the database or its
  backups can read all messages and attachments. HTTPS and Tailscale protect only the network
  path; they are not end-to-end encryption.

## 8. Tests and results

All active tests ran on agent-01 inside the limited sandbox (§1), with synthetic users only.

| Suite | Code / database | Result |
|---|---|---|
| Backend suite (`python -m unittest discover`): 49 existing + 30 new security tests + 3 privilege tests | fixed · PostgreSQL 16.2 (superuser) | **82 run: 79 passed, 3 privilege tests skipped** (they apply only to the app role) |
| Same suite as the least-privilege `messenger_app` role | fixed · **PostgreSQL 18.7** | **82 passed** |
| Migrations as `messenger_migrator` (downgrade/upgrade); refused as `messenger_app`; rollback script; suite again as superuser | fixed · PostgreSQL 18.7 | OK; 82 passed after rollback (3 privilege tests skipped); row counts unchanged |
| New security tests (`test_security.py`, `test_media_groups.py`) | **original** `main` (`6504c55`) | **28 of 30 fail** (each reproduces its issue); the 2 that pass check behaviour that was already correct (path traversal refused, Range for members) |
| Frontend `tsc -b` / `oxlint` / `node --test` (9) / production build (Vite 8.2.6) | fixed | all OK |
| `npm audit` (lockfile) | fixed | 0 vulnerabilities |
| `caddy validate` | fixed | valid |
| End-to-end probe `tests/security_probe.py` (40 checks) through Caddy + uvicorn (production flags) + PostgreSQL | fixed | **40 secure, 0 vulnerable** |
| Same probe | original code + original Caddyfile | 19 secure, **21 vulnerable** |
| Chromium, one user: sign-in, realtime, styles under CSP, revoked session signs the tab out | fixed | OK, 0 CSP violations |
| Chromium, two users: image and video previews, recipient display via protected media (cookie + Range), audio call connects and timer runs | fixed | **8/8 OK, 0 CSP violations** |
| `docker compose config` with dummy values: app and migrator credential fallbacks | fixed | OK |

## 9. Deployment and rollback

Nothing has been deployed. Production runs `main` at `6504c55`. Each phase can be approved,
run and rolled back on its own. All commands run in `~/messenger` on messenger-01.

**Phase 0: preparation (no user impact)**

1. **Disk:** remove exactly the items approved from D-1, e.g.
   `docker rm sec-proxy sec-backend sec-db it-proxy it-backend it-db`,
   `docker network rm sec_audit desktop_it`, `docker volume rm desktop_target`.
   Check that `df -h /` shows at least 10 GB free.
2. **Backups:**
   - `docker compose exec -T db sh -c 'pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Fc' > ~/backups/messenger-pre-security-$(date +%Y%m%d).dump`
   - an archive of the uploads volume (`messenger_avatar_data`);
   - copies of `.env` and `secrets/`, kept private (`chmod 600`);
   - **a copy of the `messenger_android_signing` volume.**
3. **Optional read-only integrity checks after the incident:**
   - `docker compose exec db pg_amcheck -U "$POSTGRES_USER" -d "$POSTGRES_DB" --heapallindexed`
   - compare per-table row counts with the 11:02 dump, restored into a throwaway database
     (anything newer than 11:02 can't be compared).
4. **Desktop client** (C-1): release and install the update that sends credentials for media
   downloads, or accept broken desktop media until it ships.

**Phase 1: application code (no database change, nobody logged out)**

```sh
cd ~/messenger
git merge --ff-only security/audit        # backend --reload picks up the code (bind mount)
docker compose run --rm frontend-build    # npm ci (Vite 8.2.6) + build -> frontend/dist
docker compose up -d backend              # new uvicorn flags; NO --build: same image, same dependency versions as tested
docker compose up -d caddy                # new Caddyfile, serves frontend/dist
docker compose stop frontend && docker compose rm -f frontend    # dev server retired
```

Optional, at the same time: set a new `JWT_SECRET_KEY` in `.env` before `up -d backend`.
That ends the legacy-token window immediately (§3 H-1); nobody is logged out.

**Verify Phase 1**

```sh
curl -sI http://127.0.0.1:8080/ | grep -iE 'content-security-policy|x-frame-options|x-content-type-options'
curl -s http://127.0.0.1:8080/src/main.tsx | head -c 40; echo      # "<!doctype html>" (SPA fallback), not source
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/docs                # 404
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/uploads/messages/x.png  # 401
curl -s http://127.0.0.1:8080/api/health                                           # {"status":"ok",...}
```

Then, in a browser on the tailnet:
- sign in and send a text, an image and a video;
- open an existing image and seek in an existing video;
- make a short audio call;
- check the devtools console for CSP violations;
- use "log out other sessions" from a second device and confirm the first device is signed
  out;
- leave a test group.

**Phase 2: least-privilege database roles** (after Phase 1 is confirmed)

1. Generate two URL-safe passwords (`openssl rand -hex 24`) and add them to `.env`:
   `APP_DB_USER=messenger_app`, `APP_DB_PASSWORD=…`, `MIGRATOR_DB_USER=messenger_migrator`,
   `MIGRATOR_DB_PASSWORD=…`.
2. Create the roles and transfer ownership (one transaction; idempotent):

   ```sh
   set -a; . ./.env; set +a
   docker compose exec -T -e APP_DB_PASSWORD -e MIGRATOR_DB_PASSWORD db sh -c \
     'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v app_password="$APP_DB_PASSWORD" -v migrator_password="$MIGRATOR_DB_PASSWORD" -v db="$POSTGRES_DB"' \
     < ops/db/least-privilege.sql
   ```

3. `docker compose up -d backend`: the backend now connects as `messenger_app`.
4. Verify: `/api/health` is ok, sign-in and messaging work, and
   `docker compose exec db psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "select usename, count(*) from pg_stat_activity group by 1"`
   shows `messenger_app`.
5. From now on, run migrations with `docker compose run --rm migrate`, not with the backend
   container.

**Phase 3: orphaned uploads** (after the Phase 0 uploads backup)

```sh
docker compose exec backend python -m app.cli.cleanup_uploads           # dry run: counts and sizes
docker compose exec backend python -m app.cli.cleanup_uploads --delete  # after reviewing the numbers
```

**Phase 4: certifi refresh** (L-10; separate because it changes dependency versions)

Rebuild the backend image, run the backend test suite against the new image, then
`docker compose up -d backend`.

**Rollback**

- **Phase 2:**
  1. Remove or comment out `APP_DB_*`/`MIGRATOR_DB_*` in `.env`.
  2. `docker compose up -d backend` (the backend uses the superuser again).
  3. `docker compose exec -T db sh -c 'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v db="$POSTGRES_DB"' < ops/db/least-privilege-rollback.sql`

  Data is untouched; only ownership and grants change. Tested on PostgreSQL 18.
- **Phase 1:**
  1. `git revert --no-edit 6504c55..<last audit commit>`
  2. `docker compose up -d backend caddy frontend`

  This restores the previous code, Caddyfile and dev server. No migrations to undo; nobody is
  logged out. New-style tokens stay valid until they expire under the old code.
- **Phase 3:** restore files from the uploads backup.

## 10. Summary

1. **What was checked:**
   - authentication, sessions and token handling;
   - authorization on every REST route, every WebSocket action and every media file;
   - XSS, SQL and command injection, SSRF, CSRF, CORS, WebSocket origin, token storage, URL
     leaks and error leaks;
   - uploads, Range requests and abuse limits;
   - group invites;
   - secrets in all of git history, the client bundle and the image;
   - dependency advisories, judged for applicability;
   - deployment: ports, exposure, proxies, containers, the database role and disk.

   Active testing used synthetic users on isolated databases, including PostgreSQL 18, and
   two real browsers under the new CSP.
2. **Confirmed vulnerabilities:**
   - 4 High: H-1 session revocation, H-2 rate limits, H-3 Vite dev server in production,
     H-4 unauthenticated unlimited uploads;
   - 3 Medium: M-1 media access, M-2 security headers, M-3 superuser database role;
   - 11 Low;
   - plus D-1 (disk 93% full) and C-1 (desktop media compatibility).
3. **Fixes implemented and tested:**
   - H-1 to H-4, M-1, M-2, L-1, L-2, L-5;
   - L-8 except invite consent;
   - M-3 prepared and tested on PostgreSQL 18, not applied;
   - L-3 and L-4 partly.

   30 new security tests fail on the original code (28 of 30) and all pass on the fixed
   code. The suite passes as the least-privilege role on PostgreSQL 18. The end-to-end
   probe finds 0 vulnerabilities (21 on the original), and the browser checks show 0 CSP
   violations.
4. **Remaining unknowns:**
   - tailscale serve's `X-Forwarded-For` handling;
   - Firefox, Safari and the Android WebView under the CSP;
   - calls through TURN;
   - TURN, push and SMTP configuration;
   - a future internet-facing setup;
   - load behaviour;
   - production data integrity after the incident beyond what the logs show.
5. **Still blocking a public release:**
   - deploying Phases 1–3, including the database roles;
   - the desktop update before Phase 1 (C-1);
   - freeing disk space (D-1);
   - invite consent or "who can add me" for groups, if strangers can sign up;
   - verifying L-3 and repeating §9's checks in the public setup.
6. **Deployment and rollback:** see §9. Phases 0–4, each with its own verification and
   rollback. No schema migrations. Nobody is logged out. **Not deployed; awaiting
   approval.**
