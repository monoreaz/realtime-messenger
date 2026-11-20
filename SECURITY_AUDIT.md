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
9. [Deployment and rollback](#9-deployment-and-rollback)
10. [Summary](#10-summary)

---

## 1. Method and environment

- **Code and configuration review** of the whole backend (`backend/app`, ~5.4k lines), the web
  client's security-relevant code (`frontend/src`), `compose.yml`, `Caddyfile`, `.env.example`
  and the git history.
- **Read-only checks against production** (messenger-01): listening sockets, published
  ports, Tailscale serve/funnel state, container list, HTTP status of a few public paths and
  response headers through the local Caddy port, installed package versions (from the image,
  not the running container), whether the application's database role is a superuser, and the
  lengths (not values) of two secrets. No production data was read and nothing was changed.
- **Active tests only in isolated environments with synthetic users and throwaway databases.**
  - Early tests ran in throwaway Docker containers on messenger-01 (own network, tmpfs
    Postgres). See the incident note below.
  - After the incident, all active tests ran on **agent-01** under hard limits: a systemd
    scope (1.5–2 GB memory, no swap, 1.5 CPUs, 256 tasks, maximum runtime) plus `timeout`,
    inside bubblewrap with **no network**, the code mounted read-only and a tmpfs for uploads.
    Database: embedded PostgreSQL 16.2 (`pgserver`), data in tmpfs. Python packages pinned to
    the exact versions installed in the production image. Caddy 2.11.9 (official release,
    checksum verified) with the new Caddyfile; headless Chromium (Playwright) for the
    CSP/UI check.

> **Incident during the audit (2026-11-20, ~22:40–23:11).** A regression test, run against the
> original code to prove it fails, looped forever on that code. A mocked WebSocket kept
> returning the same message and every iteration grew memory. It ran in a container without
> memory limits on messenger-01, exhausted the VM's RAM and made the VM unresponsive until the
> kernel OOM-killed the test process (23:08) and the owner reset the VM (23:11). PostgreSQL
> shut down cleanly; production containers came back automatically. The test now uses a finite
> script of socket messages, and all later active tests ran on agent-01 with the limits above.

## 2. Findings overview

| ID | Severity | Status | Finding | Fix |
|----|----------|--------|---------|-----|
| H-1 | High | Confirmed | Logout, session revocation and password change/reset do not invalidate access tokens or open WebSockets | Implemented, tested |
| H-2 | High | Confirmed | No rate limits on login, registration, password reset or messaging; reset emails have no cooldown | Implemented, tested |
| H-3 | High | Confirmed | Production serves the **Vite development server** (source code, dev endpoints; Vite 8.2.1 dev-server advisories) | Prepared (static build), tested |
| H-4 | High | Confirmed | Unauthenticated clients can send unlimited request bodies, which are written to disk **before** authentication | Implemented, tested |
| M-1 | Medium | Confirmed | Attachments are public by URL, stay downloadable after deletion, and the client-declared type is trusted | Partly fixed (deletion, type check, nosniff); access control open |
| M-2 | Medium | Confirmed | No security headers (CSP, framing, nosniff, referrer policy) | Implemented in Caddy, tested |
| M-3 | Medium | Confirmed | The application's PostgreSQL role is a superuser | Not fixed (database change; steps below) |
| L-1 | Low | Confirmed | CORS allows development origins (including a plain-HTTP LAN origin) with credentials | Implemented, tested |
| L-2 | Low | Confirmed | Interactive API docs and OpenAPI schema are public | Implemented, tested |
| L-3 | Low | Confirmed (config) | Backend cannot see client IPs (all requests appear to come from the proxy) | Prepared; tailscale-serve part requires verification |
| L-4 | Low | Confirmed | WebSocket frames up to 16 MiB; sockets outlive token expiry; typing events unthrottled | Mostly fixed (frame cap, expiry); typing throttle open |
| L-5 | Low | Confirmed | A caller can rewrite the text of server-written call-history entries | Implemented, tested |
| L-6 | Low | Confirmed (config) | Very long session lifetimes (production: non-"remember me" sessions valid ~150 days; example file: 60 h access tokens) | Example fixed; production value is the owner's decision |
| L-7 | Low | Confirmed (config) | Container/host hardening: root containers, writable source mount with `--reload`, coturn as root on host network, `.env` world-readable | Recommendations |
| L-8 | Low | Confirmed (code) | Anyone can add anyone to a group; user blocks do not apply to groups; members cannot leave | Product decision; not fixed |
| L-9 | Low | Code review | Push device token can be re-bound to another account by anyone who knows it | Not fixed |
| L-10 | Low | Confirmed | `certifi` in the backend image bundles CA roots flagged as compromised | Rebuild image |
| L-11 | Low | Confirmed | Account enumeration via registration errors and login timing | Accepted risk; noted |

Operational note (not a vulnerability): messenger-01's root filesystem is **93% full**
(3.6 GB free), mostly 23 GB of unused Docker volumes from desktop builds. Combined with H-4,
a full disk would also stop PostgreSQL. Free space before public release.

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

- **Severity:** Medium · **Status:** confirmed
- **Location:** `backend/app/main.py` (`/uploads` StaticFiles mount without authentication);
  `backend/app/api/messages.py` `delete_message`, `send_image_message`, `send_video_message`;
  `backend/app/api/users.py` `upload_avatar`.
- **Problems:**
  1. Message images and videos are served to **anyone who has the URL**; no membership check.
     The URLs contain 128 random bits, so they cannot be guessed, but they leak through copy
     and paste, logs, screenshots, or a former member's cache.
  2. Deleting a message cleared the database fields but **left the file in place**, still
     downloadable at its old URL.
  3. The server trusted the client's `Content-Type`. Arbitrary bytes (e.g. HTML) were stored as
     `.png`/`.mp4`. They are served as `image/png` and no browser sniffing to HTML was
     observed, but nothing prevented it.
- **Fix (implemented):**
  - The files of a deleted message are removed (`app/services/media.py` `delete_upload`,
    restricted to `/uploads/<dir>/<name>`).
  - Uploads must match the declared type's file signature (JPEG, PNG, WebP, MP4, WebM).
  - All API responses, including `/uploads`, carry `X-Content-Type-Options: nosniff` and
    `Referrer-Policy: no-referrer`.
- **Open (recommended before a public launch):** access control for media. Options:
  short-lived signed URLs returned in message responses (transparent for clients that use the
  URL as given), or an authenticated download endpoint with a membership check. This changes
  the media URLs that the desktop and Android clients cache, so it needs a design decision.
  Files of messages deleted **before** this fix are still on disk and must be cleaned up
  separately.
- **Tests:** `test_html_disguised_as_an_image_is_rejected`,
  `test_deleting_a_message_deletes_its_file`. In the probe, nosniff, deletion and type checks
  PASS; "media requires authentication" remains **VULN** (open item).

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
- **Location:** `compose.yml` (the backend uses `POSTGRES_USER`, the image's bootstrap
  superuser).
- **Impact:** no SQL injection was found (SQLAlchemy ORM throughout, LIKE patterns escaped).
  But if one appears, a superuser can read every database, read and write server files, and
  run programs (`COPY … PROGRAM`) inside the database container.
- **Fix (not implemented; needs a production database change):** create a role
  `messenger_app` with `NOSUPERUSER NOCREATEDB NOCREATEROLE`, owner of the application tables,
  or with DML rights plus a separate migration role. Point `DATABASE_URL` at it and keep the
  superuser for administration only. Plan this with a backup and a maintenance window.

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

### L-8 · Group membership abuse (product decision)

- Any user can create a group containing any other users (up to 99) without their consent.
  Personal blocks deliberately do not apply to groups, and there is no endpoint to leave or
  remove members.
- This is a harassment and spam vector. Recommend invitations or "only from contacts", a
  "leave group" action, and having blocks cover group invites.

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

- **tailscale serve `X-Forwarded-For` behaviour** (L-3), and the client-IP path in production
  after deployment.
- **Internet exposure:** the app is tailnet-only today. A future public setup (Funnel or a
  router port forward) was not tested; re-run §9's checks in that setup.
- **PostgreSQL version:** isolated tests ran on PostgreSQL 16.2; production runs 18.6. No
  version-specific SQL is used.
- **CSP coverage in the browser:** sign-in, realtime connection and styles were tested in
  Chromium. Audio calls (microphone, WebRTC), image/video previews (`blob:`) and the Android
  WebView were not browser-tested under the new CSP. Check them once after deployment.
- **TURN/coturn configuration**, **Firebase push** and **SMTP** were not exercised (disabled or
  out of reach in the isolated environment).
- **Load and denial of service:** not performed, as instructed. Limits were verified
  functionally only.
- **Races:** concurrent double use of one password-reset token, and concurrent refresh-token
  rotation, were reviewed but not tested.
- **Clients:** desktop and Android are out of scope. Note: the desktop client's demo-seeding
  test uploads a fake `.mp4`, which the new type check now rejects (test data only; real
  videos are unaffected).
- **Vite dev-server advisories:** their exploitability in the current container was not
  tested. They become moot once H-3 is deployed.

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

All runs below used the isolated environment on agent-01 with resource limits (§1).

| Suite | Code | Result |
|---|---|---|
| Backend unit/integration (`unittest`, 49 existing + 16 new in `tests/test_security.py`) | fixed (`security/audit`) | **65 passed** |
| New security tests only | original `main` (`6504c55`) | **16 of 16 fail** (each reproduces its vulnerability) |
| Frontend type-check (`tsc -b`) | fixed | OK |
| Frontend lint (`oxlint`) | fixed | OK (exit 0) |
| Frontend unit tests (`node --test`) | fixed | 9 passed |
| Frontend production build (Vite 8.2.6) | fixed | OK |
| `npm audit` (lockfile) | fixed | 0 vulnerabilities |
| `caddy validate` (new Caddyfile) | fixed | valid |
| End-to-end probe (37 checks): Caddy (new Caddyfile) + uvicorn (production flags) + Postgres | fixed | **36 PASS** / 1 VULN = media without authentication (M-1, open by design) |
| Same probe, original code behind the original Caddyfile | original | 18 PASS / **19 VULN** (sessions, limits, uploads, headers, CORS, docs) |
| Chromium: sign-in, realtime, styles under CSP; revocation signs the tab out | fixed | OK, 0 CSP violations |

Re-running: `backend/tests/` runs with `python -m unittest discover` from `backend/tests`
(existing setup: throwaway PostgreSQL, `DATABASE_URL`, `JWT_SECRET_KEY`). The end-to-end probe
is `tests/security_probe.py` (repository root). Run it only against an isolated stack with the
synthetic users it expects, never against production.

## 9. Deployment and rollback

Nothing has been deployed and **no database migration is needed**: there are no schema
changes. Production still runs `main` (`6504c55`).

**Before deploying**

1. Free disk space on messenger-01 (currently 93% used). For example, remove the unused
   desktop build volumes (`desktop_target`, `desktop_cargo`, `desktop_rustup`) and dangling
   images and build cache, after confirming they are not needed.
2. Optional safety copy: `pg_dump` of the production database into `~/backups`.
3. Review the branch: `git -C ~/messenger log --oneline main..security/audit` and
   `git diff main..security/audit`.

**Deploy** (in `~/messenger`, after approval)

```sh
git merge --ff-only security/audit             # backend --reload picks up the code
docker compose run --rm frontend-build         # npm ci (Vite 8.2.6) + build -> frontend/dist
docker compose up -d --build backend           # new uvicorn flags; rebuild also refreshes certifi (L-10)
docker compose up -d caddy                     # new Caddyfile + /srv mount
docker compose stop frontend && docker compose rm -f frontend   # dev server no longer used
```

**Verify**

```sh
curl -sI http://127.0.0.1:8080/ | grep -iE 'content-security-policy|x-frame-options|x-content-type-options'
curl -s http://127.0.0.1:8080/src/main.tsx | head -c 40; echo   # expect "<!doctype html>" (SPA fallback), not TypeScript source
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/docs       # expect 404
curl -s http://127.0.0.1:8080/api/health                                     # expect {"status":"ok",...}
```

Then, in a browser on the tailnet:
- sign in, send a message, send an image and a video, make a short audio call;
- in the devtools console, check there are no CSP violations;
- in Settings, use "log out other sessions" from a second device and confirm the first device
  is signed out.

To verify L-3, temporarily log `request.client.host` in a test endpoint, or check that two
different tailnet devices are rate-limited independently.

**Effect on users:** nobody is logged out. Access tokens issued before the upgrade stay valid
until they expire (at most 30 minutes). Open web tabs, the desktop app and the Android app
reconnect normally.

**Rollback** (no data migration to undo)

```sh
cd ~/messenger
git revert --no-edit 6504c55..<last-audit-commit>   # restores the previous code, Caddyfile and compose.yml
docker compose up -d backend caddy frontend        # old uvicorn flags, Caddy -> Vite dev server again
```

After a rollback, tokens issued by the new code (with `sid`) remain valid until they expire:
the old code ignores the extra claim.

## 10. Summary

1. **What was checked:**
   - authentication and session handling, and authorization on every REST route and
     WebSocket action;
   - XSS, SQL and command injection, SSRF, CSRF, CORS, WebSocket origin, token storage, URL
     leaks and error leaks;
   - uploads and abuse limits;
   - secrets in all of git history, the client bundle and the image;
   - dependency advisories, judged for applicability;
   - deployment: ports, exposure, proxies, containers and the database role.

   Active testing used synthetic users A, B and C on isolated databases.
2. **Confirmed vulnerabilities:**
   - 4 High: H-1 session revocation, H-2 rate limits, H-3 Vite dev server in production,
     H-4 unauthenticated unlimited uploads;
   - 3 Medium: M-1 attachment access, M-2 security headers, M-3 superuser database role;
   - 11 Low.
3. **Fixes implemented and tested:** H-1, H-2, H-3, H-4, M-2, L-1, L-2, L-5 fully; M-1, L-3 and
   L-4 partly (see each finding). 16 new regression tests fail on the original code and pass
   on the fixed code; all 65 backend tests and all frontend checks pass.
4. **Remaining unknowns:**
   - tailscale serve's `X-Forwarded-For` handling;
   - calls, previews and the Android WebView under the new CSP;
   - TURN configuration;
   - behaviour on PostgreSQL 18 (tests ran on 16);
   - a future internet-facing setup;
   - load behaviour.
5. **Should block public release:**
   - deploying H-1 to H-4 and M-2;
   - M-1 media access control (signed or authenticated URLs) and removing the leftover files
     of deleted messages;
   - M-3 a non-superuser database role;
   - freeing disk space;
   - L-8 group-invite abuse controls, if the app opens to strangers;
   - re-running §9's verification in the public setup.
6. **Deployment and rollback:** see §9. No migrations; nobody is logged out; deployment and
   rollback are compose and git operations only. **Not deployed — awaiting approval.**
