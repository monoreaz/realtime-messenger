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
9. [Deployment and rollback](#9-deployment-and-rollback) (release order, phases 0 and A–D)
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
| L-3 | Low | Confirmed (config) | Backend cannot see client IPs (all requests appear to come from the proxy) | Implemented; tailscale serve behaviour verified in its source; live check after deployment |
| L-4 | Low | Confirmed | WebSocket frames up to 16 MiB; sockets outlive token expiry; typing events unthrottled | Mostly fixed (frame cap, expiry); typing throttle open |
| L-5 | Low | Confirmed | A caller can rewrite the text of server-written call-history entries | Implemented, tested |
| L-6 | Low | Confirmed (config) | Very long session lifetimes (production: non-"remember me" sessions valid ~150 days; example file: 60 h access tokens) | Example fixed; production value is the owner's decision |
| L-7 | Low | Confirmed (config) | Container/host hardening: root containers, writable source mount with `--reload`, coturn as root on host network, `.env` world-readable | Recommendations |
| L-8 | Low | Confirmed (code) | Anyone can add anyone to a group; user blocks do not apply to groups; members cannot leave | Implemented, tested (invitation consent, blocks, rate limit, leave) |
| L-9 | Low | Code review | Push device token can be re-bound to another account by anyone who knows it | Not fixed |
| L-10 | Low | Confirmed | `certifi` bundles CA roots flagged as compromised; dependencies and base image unpinned, so rebuilds install untested versions | Fixed: hash-pinned lockfile, digest-pinned base image; exact image tested |
| L-11 | Low | Confirmed | Account enumeration via registration errors and login timing | Accepted risk; noted |
| D-1 | Operational | Confirmed | Disk 93% full on messenger-01 | Approved items removed (93% → 50%); verified backups taken |
| C-1 | Compatibility | Confirmed | Desktop ≤ 0.1.4 downloads media without credentials, so its media breaks once M-1 is deployed | Desktop 0.1.5 prepared and tested (not published); must ship first |


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
- **The remaining gap, stated explicitly:**
  - For up to **30 minutes** after the backend switch (`ACCESS_TOKEN_EXPIRE_MINUTES`), an
    access token issued *before* the deployment keeps working even after its session is
    ended by logout, "log out other sessions", revoking that session, a password change or a
    password reset.
  - A WebSocket opened with such a token stays open until the token expires.
  - **Not affected:**
    - admin blocking: blocked accounts are rejected on every request, as before;
    - media downloads in browsers: they are checked against the refresh session
      itself.
  - After 30 minutes no legacy token can exist.
- **Key rotation would close the gap immediately, but has not been done** (owner's
  decision). Verified on agent-01 with the desktop client and a real browser (§8):
  - **Nobody is logged out.** Refresh sessions are database rows holding a hash of a random
    token; they do not depend on `JWT_SECRET_KEY`. The tests confirmed:
    - a running desktop app with "remember me" and one without both carried on: their
      first 401 triggered a refresh from the cookie;
    - a restarted desktop app restored its saved session;
    - an open web tab stayed signed in and renewed its token.
  - **Rotation means restarting the backend, so every WebSocket drops.**
    - The desktop app and the *new* web app reconnect: the old token is refused with a
      policy close, they refresh, and they reconnect (seconds).
    - A web tab still running the *old* JavaScript retries with its old token until its
      10-minute refresh or a reload. It shows "connecting" and failing actions in the
      meantime, but is not logged out.
  - **Also invalidated:** call-decline tokens in flight (JWT, at most 60 s).
  - **Not affected:**
    - email verification and password-reset links (database hashes);
    - TURN credentials (separate secret);
    - desktop update signatures (separate key).
  - **If wanted,** rotate only after desktop 0.1.5 is installed and web tabs have reloaded
    the new app. Otherwise simply let the 30-minute window pass.
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

- **Status:** confirmed by configuration review · **Fix:** implemented and tested; the
  tailscale serve hop is verified by reading its source; a live check is due after
  deployment.
- **Problem:** uvicorn did not trust Caddy's `X-Forwarded-For`, and Caddy did not trust the
  local tailscale serve hop. Every request appeared to come from a proxy address: no
  per-client attribution, and per-IP limits would act as global limits.
- **How a client address travels now:**
  1. **tailscale serve** (version 1.104.6 on messenger-01; `ipn/ipnlocal/serve.go`) proxies
     with Go's `httputil.ReverseProxy` in `Rewrite` mode. In that mode Go removes all
     incoming `Forwarded`/`X-Forwarded-*` headers before `Rewrite` runs. Tailscale then
     **sets** `X-Forwarded-For` to the connection's real source address: the client's
     tailnet IP, or the original public address for Funnel. It also removes and re-sets
     its `Tailscale-User-*` identity headers. A client therefore cannot forge the value,
     and it is a single address.
  2. **Caddy** receives the request from the Docker port mapping on `127.0.0.1:8080`. Its peer
     is the bridge gateway (`172.18.0.1` on `messenger_default`) via `docker-proxy`. Caddy
     trusts `X-Forwarded-For` only from loopback and the Docker bridge range
     (`trusted_proxies static 127.0.0.0/8 ::1/128 172.16.0.0/12`). Tailnet and LAN
     addresses are not trusted, so if Caddy were ever reachable directly, client-sent
     headers would be ignored. In strict mode Caddy takes the right-most untrusted address
     and forwards exactly that value (`header_up X-Forwarded-For {client_ip}`).
  3. **uvicorn** (`--proxy-headers --forwarded-allow-ips '*'`) uses that value. The backend
     has no published port; only containers on the Compose network reach it.
- **Do the rate limits tell users apart?**
  - **Per-IP limits** (login 30 per 5 min, registration, password reset, verification) apply
    per **tailnet device**: each device has its own tailnet address. Two devices of one
    person have separate buckets. People sharing one device share one.
  - Through Funnel or another public setup, people behind the same NAT would share a
    bucket.
  - **Per-account limits do not depend on IP:** 10 failed logins per account in 15 min,
    password change per user, 30 messages per 10 s per user, 10 groups per hour per user,
    and one reset email per address per minute.
- **Tests:**
  - the probe sends rotating forged left-most `X-Forwarded-For` values through the new
    Caddyfile: they do not avoid the per-IP login limit, and a different client address is
    unaffected;
  - `docker network inspect` confirms the production bridge subnet and gateway.
- **Live check after deployment (Phase B):**
  1. On one tailnet device, make 31 failed sign-ins within 5 minutes to a test username:
     expect 429.
  2. From a second device, sign in normally: expect success.
  3. If both are limited, the client address is not arriving and the per-IP limits act
     globally; roll back the Caddyfile or report it.

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
  code review · **Fix:** implemented and tested
- **Problem:** public registration is supported (self-service sign-up with email
  verification), so strangers can create accounts. Any user could put any other users (up to
  99) into a group. Personal blocks did not apply to groups, and there was no way to leave
  one, so this was a harassment and spam channel.
- **Fix (`backend/app/api/chats.py`, `app/models/group_invitation.py`, migration
  `a7c3e9d1f2b4`, web app):**
  - **Invitation consent.** Creating a group makes only the creator a member (and admin).
    Everyone selected gets a **pending invitation** in the new table `group_invitations`.
  - **A pending invitation grants nothing.** Every access check (messages, sending, pins,
    read state, attachments including Range requests, typing, presence, push, the chat
    list) requires a `chat_members` row. New messages are not delivered to invitees.
  - **Only the invited user can accept or decline.**
    - `POST /chats/{id}/invitation/accept|decline` look up the invitation by
      *(chat, current user)*. Nobody else, not the inviter, not an admin, not an
      outsider, can act on it.
    - Accepting creates the membership and deletes the invitation.
    - Declining deletes it.
    - Either way it cannot be reused.
  - `GET /chats/invitations` lists the user's pending invitations (group name, inviter,
    member count).
  - **Web app:** a "Group invitations" section above the chat list with Join and Decline.
    The "New group" dialog now says that people are invited.
  - **Other rules:**
    - nobody can invite a user who blocked them, or whom they blocked;
    - blocking someone withdraws that person's pending invitations to you;
    - suspended accounts cannot be invited;
    - 10 groups per hour per user;
    - anyone can leave a group (`POST /chats/{id}/leave`, "Leave group" button); the
      admin role passes to the longest-standing member, and an empty group is deleted;
    - leaving ends access to messages and files immediately.
  - **Existing groups are unchanged:** their members stay members.
- **Tests:**
  - `InvitationConsentTests`:
    - a pending invitee gets 404 on messages, sending, pins, read and the group's image
      (also with Range), the group is missing from their chat list, and they receive no
      new messages;
    - the inviter and an outsider cannot accept, and an unknown chat gets 404;
    - after accepting, access works and the invitation is used up;
    - declined invitations cannot be accepted later;
    - blocking the inviter withdraws the invitation.

    All 4 fail on the original code.
  - `GroupInviteTests` (blocks, suspended accounts, rate limit, leave and admin hand-over)
    and the updated `test_groups.py` (invitees accept first).
  - Browser: an invited user sees the invitation, the group is absent until Join, then it
    appears with its messages; 0 CSP violations.
- **Desktop:** 0.1.5 can list and accept invitations through its API client (used by its
  tests) but has **no invitation UI yet**. Desktop users join groups from the web app until
  a later desktop release adds it.

### L-9 · Push device re-binding

- `POST /push/devices` upserts by FCM token. Anyone who knows another device's FCM token could
  attach it to their own session, so the victim's device would receive the attacker's
  notifications and stop receiving its own. FCM tokens are not public, so the risk is low.
  Fix: refuse to move a token between users, or require a proof from the device.

### L-10 · certifi CA bundle; unpinned dependencies and base image

- **Problem:**
  - `certifi 2026.7.22` bundles emSign roots with compromised intermediate keys
    (PYSEC-2026-4320). This affects outbound TLS from the backend (Firebase Cloud
    Messaging).
  - More generally, `requirements.txt` was unpinned and the Dockerfile used the floating tag
    `python:3.12-slim`. Any rebuild silently installed whatever versions were newest at the
    time, untested.
- **Fix:**
  - `backend/requirements.lock` holds exact versions **with hashes** for all 67 packages.
    It was generated from `requirements.txt` with the versions in the tested image as
    constraints, so the **only** version change is `certifi` → `2026.11.20`.
  - The Dockerfile installs it with `pip install --require-hashes --only-binary=:all:`, and
    the base image is pinned by digest
    (`python:3.12-slim@sha256:5e2bdb1b0d54…a4fa5b`).
  - `requirements.txt` stays as the human-edited list of direct dependencies; the lockfile
    is regenerated deliberately.
- **The exact image intended for deployment:**
  - `messenger-backend:security-20261121`, image ID `sha256:c8aeb46d2ff3…df88496a`.
  - Built on messenger-01 with its own tag; production's `messenger-backend:latest`
    (`012697edef9d…`) is untouched.
  - Contents: Python 3.12.16, OpenSSL 3.5.7, `certifi 2026.11.20`, and FastAPI, Starlette,
    uvicorn, SQLAlchemy, PyJWT and python-multipart unchanged.
  - It was **tested as that image:** its filesystem was exported to agent-01, and the test
    suite ran on the image's own Python and libraries (via the image's dynamic loader)
    against PostgreSQL 18.7:
    - 86 tests as the least-privilege role;
    - migrations up and down as the migration role;
    - rollback.
- **Remaining advisories in the locked set (not applicable, §6):**
  - `pyasn1`: parses only trusted Google credentials;
  - `dnspython`: email validation runs without DNS lookups;
  - `opentelemetry-api`: no incoming trace headers are parsed.

  pip 25.0.1 in the base image is build-time only.

### L-11 · Account enumeration

- Registration answers "Username already exists" / "Email already exists", and login skips
  password hashing for unknown accounts (timing).
- Password reset and resend-verification correctly answer 204 either way.
- Accepted for now. With H-2 in place, enumeration is rate-limited.

### D-1 · Disk usage on messenger-01 (read-only investigation)

- **Status at the time of the investigation:** the root filesystem is 48 GB, 42 GB used (**93%**, 3.6 GB free). The approved cleanup is described under *Cleanup and backups* below.
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

#### Cleanup and backups (done 2026-11-21, approved)

- **Removed, after checking each one:**
  - volume `desktop_target`: exact name confirmed, no container attached, no build
    running, and all 24,812 files were Cargo output under `debug/`, `release/` and
    `x86_64-pc-windows-msvc/`;
  - the stopped test containers `sec-proxy`, `sec-backend`, `sec-db`, `it-proxy`,
    `it-backend` and `it-db` (each confirmed `exited`);
  - their networks `sec_audit` and `desktop_it` (confirmed empty).

  No `prune` was used. Disk went from 93% to **50%** used.
- **Kept as instructed:**
  - the unidentified PostgreSQL 18 volume `76433891…`;
  - the production database and uploads;
  - `messenger_android_signing`;
  - `desktop_cargo` and `desktop_rustup`.
- **Desktop builds since then:** compiling the desktop 0.1.5 tests on messenger-01 recreated
  `desktop_target` (about 7 GB), and the new backend image added about 0.1 GB. Disk is now
  about 68% used. `desktop_target` can be removed again the same way once the desktop
  release is built.
- **Backups:**
  - **Location:** `~/backups/pre-security-20261121/` on messenger-01, outside the repository.
    The directory is `0700` and every file is `0600`, owned by the deployment user.
  - **Contents:**
    - `messenger-db.dump` (`pg_dump -Fc`) and `messenger-db-globals.sql` (roles);
    - `uploads.tar.gz` (the uploads volume);
    - `android-signing.tar.gz`;
    - `env-and-secrets.tar.gz` (`.env` and `secrets/`);
    - `production-state.txt` (git `6504c55`, backend image `012697edef9d`);
    - `SHA256SUMS`.
  - **Verified usable:**
    - all checksums match;
    - the dump was restored into a throwaway PostgreSQL 18 container (no network, tmpfs,
      memory and CPU limits) and **all 10 tables have identical row counts** to
      production; only counts were compared, never content;
    - the uploads archive unpacks to **33 files with identical SHA-256** to the live
      volume;
    - the keystore in the signing archive matches the volume;
    - all 5 config and secret files are byte-identical to the live ones.
  - **Limitation:** the backups are on the same VM and disk as production. They protect
    against mistakes during deployment, not against losing the VM. Copying them off the
    host is the owner's decision; they contain secrets and user data.

### C-1 · Client compatibility

| Client | Current server (`6504c55`) | New server |
|---|---|---|
| Desktop ≤ 0.1.4 | Works | Media (images, avatars, videos) fails with 401. Invited users cannot join groups from the app. Everything else works |
| **Desktop 0.1.5** (prepared, `452fd59`, not published) | **Works** (tested: downloads with credentials against the original backend) | **Works** (tested); invitations are accepted in the web app (no desktop UI yet) |
| Web, new JavaScript | n/a | Tested (Chromium): sign-in, realtime, media previews and display, audio call, invitations, revocation, key rotation |
| Web tab still on old JavaScript during the switch | Works | Works. Needs a reload for the invitations UI, the leave button, and quick token renewal after a backend restart |
| Android app (WebView of the web app) | Works | Should behave like the web app; not tested on a device |

**Desktop 0.1.5** (`desktop/src/api/client.rs`):
- `Client::download` and `download_to` now request media through the authenticated path
  (Bearer token plus refresh cookie, with the usual refresh-on-401) **only** if
  `is_server_media(url)` holds: same scheme, host and port as the configured server, no
  user info, and the path under `/api/uploads/`.
- Every other URL is fetched without credentials. reqwest additionally drops
  `Authorization`/`Cookie` on cross-host redirects.
- **Tests (run on agent-01 against the new backend):**
  - **Unit:** the trust check refuses other hosts, look-alike hosts, scheme or port changes,
    user info, non-upload paths and `../` normalization.
  - **Live `media_needs_a_member_session`:**
    - members download images and videos;
    - an unrelated user gets 404;
    - a byte range returns 206 with the right bytes, and the unrelated user's range gets
      404;
    - no session means no access;
    - after logout, and after "log out other sessions" from another device, downloads
      fail;
    - a URL on another origin receives **no** `Authorization` or `Cookie` header (a local
      listener records the request).
  - The existing desktop end-to-end suite passes against the new backend. It was updated
    for invitations and for the 30-messages-per-10-s limit, which it now respects by
    backing off.
  - The JWT-rotation test passes (H-1).
  - clippy shows no new warnings; the Windows type-check passes; `Cargo.lock` is unchanged
    (`--locked`).
- **Release order:** publish 0.1.5 and let installed apps update **before** the backend
  switch. The app checks for updates at start-up.

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

**Verified since the previous version of this report:**
- tailscale serve's client-address handling (source of the installed version);
- the exact deployment image;
- desktop media against both servers;
- key rotation with the desktop client and a browser;
- invitation consent;
- backups, by restore.

**Still open:**
- **Live client-IP check** after Phase B (L-3: two devices, one rate-limited, the other
  not).
- **Desktop 0.1.5 on Windows.** The tests ran on a Linux build; the Windows build passed only
  its type-check. CI builds the Windows installer when the release is tagged. Before
  publishing, install the draft on Windows, then open an image, play a video, open the
  Settings dialog, log out and back in.
- **Browsers and devices:** Firefox, Safari, and the Android WebView on a real device.
  Calls through the TURN relay.
- **Desktop invitation UI:** not implemented. Desktop users join groups from the web app.
- **Internet exposure:** the app is tailnet-only. A public setup (Funnel or a port
  forward) needs the verification steps in §9 repeated at the real edge, plus TLS and HSTS
  checks.
- **TURN/coturn configuration**, **Firebase push** and **SMTP delivery** were not
  exercised.
- **Load and denial of service** were not tested, as instructed.
- **Races** were reviewed, not tested: concurrent double use of one password-reset token;
  concurrent refresh rotation; accepting an invitation while the group is deleted.
- **Production data integrity after the incident:** not verified beyond the logs. The
  verified dump from 2026-11-21 now gives a reference point for later comparisons.

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

All active tests ran on agent-01 inside the limited, network-less sandbox (§1), with
synthetic users and throwaway databases. Desktop test binaries were compiled on
messenger-01 in a container limited to 2 CPUs and 4 GB, and executed on agent-01.

| Suite | Code / environment | Result |
|---|---|---|
| Backend suite (49 existing + 34 new security tests + 3 privilege tests) | final code · PostgreSQL 16.2 | **86 run: 83 passed, 3 skipped** (privilege tests apply only to the app role) |
| Same suite as `messenger_app` (least privilege), with role script, migrations as migrator (incl. new `a7c3e9d1f2b4`), app role refused, rollback, suite again | final code · **PostgreSQL 18.7** | **86 passed**; migrations OK; `InsufficientPrivilege` for the app role; rollback OK; row counts unchanged |
| Same PostgreSQL 18.7 sequence on the **exact deployment image** `messenger-backend:security-20261121` (`c8aeb46d2ff3`): the image's own Python 3.12.16 and libraries | final code + image | **86 passed** as app role; migrations and rollback OK |
| New security tests on the **original** code (`6504c55`) | original | `test_security`/`test_media_groups`: **28 of 30 fail**; invitation consent: **4 of 4 fail** (each reproduces its issue). The 2 that pass check behaviour that was already correct |
| Frontend `tsc -b` / `oxlint` / `node --test` (9) / production build (Vite 8.2.6) | final | all OK |
| `npm audit` · `pip-audit` of the lockfile | final | 0 npm vulnerabilities · only the non-applicable Python advisories (§6) |
| `caddy validate` | final Caddyfile | valid |
| End-to-end probe `tests/security_probe.py` (40 checks) through Caddy + uvicorn (production flags) | final | **40 secure, 0 vulnerable** |
| Same probe | original code + original Caddyfile | 19 secure, **21 vulnerable** |
| Chromium, two users: image and video previews, recipient display via protected media (cookie + Range), audio call | final | **8/8, 0 CSP violations** |
| Chromium: sign-in, realtime, styles under CSP; revoked session signs the tab out | final | OK, 0 CSP violations |
| Chromium: group invitation shown, group absent until Join, then present with messages | final | **4/4, 0 CSP violations** |
| Chromium: open tab during JWT key rotation (backend restart with a new key) | final | stays signed in, back online, token renewed from cookie, still signed in after reload |
| Desktop 0.1.5: unit tests (incl. credential trust check) | Linux build | **38 passed** |
| Desktop 0.1.5: existing end-to-end suite, media access test, JWT-rotation test | new backend | **all pass** |
| Desktop 0.1.5 media download against the **original** backend | original | downloads work (stops only at "unrelated user gets 404", which the old public server cannot do) |
| Desktop 0.1.5: clippy · Windows type-check · `--locked` | messenger-01 build | no new warnings · OK · OK |
| Backups: checksums, restore into PostgreSQL 18 (row counts), archive contents (checksums) | messenger-01, throwaway containers | all identical |
| `docker compose config` with dummy values: credential fallbacks | final compose.yml | OK |

## 9. Deployment and rollback

**Nothing has been deployed.** Production runs `main` at `6504c55` with backend image
`012697edef9d`.

**Prepared changes:**

| Where | What |
|---|---|
| messenger-01 `~/messenger-security` | branch `security/audit`: `3b5d89c`, `782e573`, `6d1a4a5`, and the report commit; not merged, not pushed |
| messenger-01 Docker | backend image `messenger-backend:security-20261121` (`sha256:c8aeb46d2ff3…`) |
| agent-01 `~/work/messenger-desktop` | desktop 0.1.5: `feature/desktop` at `452fd59`; not pushed, not tagged |

Each phase below needs your approval. Run the commands in `~/messenger` on messenger-01
unless noted otherwise.

### Release order

The order matters, because of compatibility (§3 C-1):

1. **Phase A, desktop 0.1.5 first.** It works with the current *and* the new server.
2. **Phase B, backend and web.** Desktop ≤ 0.1.4 loses media from this moment, so wait
   until active desktop sessions run 0.1.5.
3. **Phase C, database roles.** Independent of the code; after B is confirmed.
4. **Phase D, orphaned uploads.**
5. **Later, optional:** key rotation, the follow-up that requires `sid`, and removing
   `desktop_target` again.

### Phase 0: preparation (done)

- Approved disk cleanup (§3 D-1).
- Verified backups in `~/backups/pre-security-20261121/`.
- **Open decisions:**
  - an off-host copy of the backups;
  - the optional read-only integrity check
    `docker compose exec db pg_amcheck -U "$POSTGRES_USER" -d "$POSTGRES_DB" --heapallindexed`.

### Phase A: desktop 0.1.5 (release procedure; publishes a release)

1. Push and tag:
   - **on agent-01:**

     ```sh
     cd ~/work/messenger-desktop
     git tag desktop-v0.1.5
     git push vm feature/desktop desktop-v0.1.5
     ```

   - **on messenger-01:**

     ```sh
     git -C ~/messenger push origin feature/desktop desktop-v0.1.5
     ```

   CI builds the Windows installer and a **draft** release.
2. Install the draft on Windows and run the smoke test in §5. Then publish the draft.
3. Wait until desktop sessions have updated (read-only, counts only):

   ```sh
   docker compose exec -T db sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "select substring(user_agent from '"'"'MessengerDesktop/[0-9.]+'"'"') as version, count(*) from auth_sessions where revoked_at is null and expires_at > now() and user_agent like '"'"'MessengerDesktop/%'"'"' group by 1"' </dev/null
   ```

   On 2026-11-21 at 01:15 UTC this showed **4 active desktop sessions: 3 on 0.1.1 and 1 on
   0.1.4**. All of them would lose media at Phase B. Proceed when they show 0.1.5, or when
   you accept the breakage for the remaining old installs; signing in again after updating
   creates a 0.1.5 session.

**Rollback:** unpublish or replace the release. 0.1.5 also works with the current
server, so no server change is involved.

### Phase B: backend and web (brief backend restart; nobody logged out)

```sh
cd ~/messenger
git status --short && git rev-parse --short HEAD          # expect clean, 6504c55
df -h / | tail -1                                          # expect >= 10 GB free

# 1. Migration first: adds group_invitations; the running old code ignores the table.
docker compose run --rm --no-deps -T -v ~/messenger-security/backend:/app backend alembic upgrade head </dev/null
docker compose run --rm --no-deps -T -v ~/messenger-security/backend:/app backend alembic current </dev/null   # a7c3e9d1f2b4 (head)

# 2. Code (backend --reload picks up the bind-mounted code at once).
git merge --ff-only security/audit

# 3. Image: keep the current one for rollback, switch to the tested one (no rebuild).
docker tag messenger-backend:latest messenger-backend:pre-security-20261121
docker tag messenger-backend:security-20261121 messenger-backend:latest

# 4. Web app as static files.
docker compose run --rm frontend-build

# 5. Restart with the new image, flags and Caddy configuration; retire the dev server.
docker compose up -d backend
docker inspect -f '{{.Image}}' messenger-backend-1             # sha256:c8aeb46d2ff3...
docker compose up -d caddy
docker compose stop frontend && docker compose rm -f frontend
```

**Verify Phase B**

```sh
curl -s http://127.0.0.1:8080/api/health                                           # {"status":"ok",...}
curl -sI http://127.0.0.1:8080/ | grep -iE 'content-security-policy|x-frame-options|x-content-type-options'
curl -s http://127.0.0.1:8080/src/main.tsx | head -c 40; echo                      # "<!doctype html>", not source
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/docs                # 404
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/uploads/messages/x.png  # 401
```

Then in browsers on the tailnet, and in desktop 0.1.5:
- sign in;
- send a text, an image and a video;
- open an older image and seek in an older video;
- make a short audio call;
- check the browser console for CSP violations;
- create a test group: invitees see the invitation, and the group appears only after Join;
- leave the test group;
- use "log out other sessions" from a second device and confirm the first device is signed
  out.

Finally, the **L-3 live check** (§3).

**Effect on users:**
- The backend restarts for a few seconds and realtime connections reconnect.
- Nobody is logged out.
- For up to 30 minutes, tokens issued before the switch can't be revoked (§3 H-1).
- Open web tabs should be reloaded to get the new app.

**Rollback B**

```sh
cd ~/messenger
docker tag messenger-backend:pre-security-20261121 messenger-backend:latest
git reset --keep 6504c55                 # only if nothing else was committed to main since
docker compose up -d backend caddy frontend      # old code, old flags, Caddy -> Vite dev server again
# Optional: drop the invitations table (loses pending invitations).
# docker compose run --rm --no-deps -T -v ~/messenger-security/backend:/app backend alembic downgrade f6b2d8c4a1e9 </dev/null
```

**Compatibility after rolling back B:**
- The old code ignores `group_invitations`. Invitations still pending at that point are
  stranded; the invitees are simply not members.
- Groups created in the meantime keep the members who accepted.
- Tokens issued by the new code carry `sid`, which the old code ignores. They stay valid
  until they expire.
- Desktop 0.1.5 keeps working.
- Media becomes public by URL again, and the other fixes are gone.
- No data migration needs undoing.

### Phase C: least-privilege database roles (after B is confirmed)

1. Add to `.env`, with two new values from `openssl rand -hex 24`: `APP_DB_USER=messenger_app`,
   `APP_DB_PASSWORD=…`, `MIGRATOR_DB_USER=messenger_migrator`, `MIGRATOR_DB_PASSWORD=…`.
2. Create the roles:

   ```sh
   set -a; . ./.env; set +a
   docker compose exec -T -e APP_DB_PASSWORD -e MIGRATOR_DB_PASSWORD db sh -c \
     'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -v app_password="$APP_DB_PASSWORD" -v migrator_password="$MIGRATOR_DB_PASSWORD" -v db="$POSTGRES_DB"' \
     < ops/db/least-privilege.sql
   ```

3. `docker compose up -d backend`
4. Verify:
   - `/api/health` is ok, and sign-in and messaging work;
   - `pg_stat_activity` shows the backend connected as `messenger_app`.
5. From then on, run migrations with `docker compose run --rm migrate`.

**Rollback C:**
1. Remove `APP_DB_*`/`MIGRATOR_DB_*` from `.env`.
2. `docker compose up -d backend`
3. Run `ops/db/least-privilege-rollback.sql` as in its header comment. This changes
   ownership and grants only; tested on PostgreSQL 18.

### Phase D: orphaned uploads

```sh
docker compose exec backend python -m app.cli.cleanup_uploads            # dry run: counts and sizes
docker compose exec backend python -m app.cli.cleanup_uploads --delete   # after reviewing the numbers
```

**Rollback D:** restore files from `~/backups/pre-security-20261121/uploads.tar.gz`.

### Last resort: restoring data

`pg_restore` from `messenger-db.dump` returns the database to its state at 2026-11-21 00:07 UTC.
Anything written after that would be lost, so use it only if the database itself is
damaged.

## 10. Summary

1. **What was checked:**
   - authentication, sessions and tokens;
   - authorization on every REST route, WebSocket action, media file and group invitation;
   - XSS, SQL and command injection, SSRF, CSRF, CORS, WebSocket origin, token storage, URL
     leaks and error leaks;
   - uploads, Range requests and abuse limits;
   - secrets in all of git history, the client bundle and the image;
   - dependencies (advisories judged for applicability, now hash-pinned);
   - deployment: ports, exposure, proxies and client IPs through tailscale serve,
     containers, the database role and disk;
   - desktop media compatibility;
   - JWT key rotation.

   Active testing used synthetic users on isolated databases (PostgreSQL 16 and 18), the
   exact deployment image, two real browsers under the CSP, and the desktop client against
   both the old and the new server.
2. **Confirmed vulnerabilities:**
   - 4 High: session revocation, rate limits, Vite dev server in production,
     unauthenticated unlimited uploads;
   - 3 Medium: media access, security headers, superuser database role;
   - 11 Low;
   - plus disk (D-1) and desktop compatibility (C-1).
3. **Fixes implemented and tested:**
   - all High and Medium findings;
   - L-1, L-2, L-3 (live check pending), L-5, L-8 with invitation consent, and L-10 with
     pinning;
   - L-4 mostly;
   - M-3 prepared and tested, to be applied in Phase C;
   - desktop 0.1.5 prepared and tested.

   The new tests fail on the original code and pass on the fixed code. The exact image
   passes on PostgreSQL 18 as the least-privilege role. The probe finds 0 vulnerabilities
   (21 on the original). The browser checks show 0 CSP violations.
4. **Remaining unknowns:**
   - the live client-IP check;
   - desktop 0.1.5 on Windows (smoke test before publishing);
   - Firefox, Safari and the Android WebView;
   - TURN, push and SMTP;
   - a future internet-facing setup;
   - load behaviour;
   - production data integrity after the incident beyond the logs.
5. **Still blocking a public release:**
   - Phases A–D;
   - the L-3 live check;
   - repeating §9's verification in the public setup;
   - desktop users can't accept group invitations in the app (they need the web app) until
     a desktop UI exists.
6. **Deployment and rollback:** see §9:
   - desktop 0.1.5 first;
   - then the migration plus backend and web on the tested image, with the previous image
     kept for rollback;
   - then the database roles and the upload cleanup.

   Every phase has its own verification and rollback. Nobody is logged out.
   **Not deployed; awaiting approval.**
