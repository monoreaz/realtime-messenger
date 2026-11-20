# Messenger Desktop — progress

Native Windows client for Realtime Messenger, written in Rust with iced 0.14.
No browser engine, WebView, Electron or Tauri. It talks to the existing FastAPI
backend (`/api/*`, `/ws`) exactly like the web client does.

## Goals
- Feature parity with the web messenger (`frontend/src/App.tsx`), same accounts and data.
- Look and feel of the web "graphite glass" design (colours, layout, bubbles, icons).
- Fast startup, idle CPU ≈ 0, bounded memory, smooth scrolling.
- Windows integration: notifications, taskbar, shortcuts, clipboard, file dialogs, DPAPI.
- Distribution: GitHub Actions Windows build → Inno Setup installer → GitHub Releases,
  signed in-app updates (Telegram-style).

## Current stage
**M1–M6 done, M7 (installer and updates) built in CI but not yet exercised end to end, M8 pending.**
`feature/desktop` is on GitHub. The Windows CI run (tests, clippy, release build, Inno Setup
installer, artifact) **passed** on windows-latest. Native audio calls work, and interop with the web
app in Chromium is verified in both directions. Still open: manually testing the installed app on a
real Windows PC, the first signed release (needs the GitHub secret), and an update test 0.1.0 → 0.1.1.

## Architecture (desktop/src)
| Module | Role |
|---|---|
| `main.rs` | logging, window settings, `iced::application` bootstrap |
| `app.rs` | state machine: `App`/`Screen`/`Main`, `Message`, `update`, `subscription` |
| `api/client.rs` | reqwest client; bearer token + rotating `messenger_refresh` cookie handled manually; single-flight refresh |
| `api/ws.rs` | `/ws` subscription: auth frame, ping/pong, silence timeout, exponential backoff, token refresh on 1008 |
| `api/models.rs` | serde types mirroring `backend/app/schemas` |
| `storage.rs` | `%APPDATA%\Messenger\settings.json`; remembered session in `session.bin`, DPAPI-encrypted on Windows |
| `media.rs` | lazy image loading (via `sensor`), off-thread decode + downscale, LRU per size class |
| `dialogs.rs` | delete confirm, image viewer, incoming call, peer/group profile, create group, pinned list, admin panel |
| `ui/` | `backdrop.rs` (web backdrop + glass window composed per window size), `theme.rs` (design tokens), `icons.rs` (web icon paths as SVG), `widgets.rs`, `auth.rs`, `sidebar.rs`, `chat.rs`, `settings.rs` |
| `update.rs` | manifest check, background download with progress, SHA-256 + Ed25519 verification, silent installer launch |
| `platform.rs` | notifications (WinRT toast via notify-rust), AppUserModelID, open-with-default-app |
| `call/` | native audio calls: `engine.rs` (str0m WebRTC, sans-IO loop), `codec.rs` (pure-Rust libopus), `audio.rs` (cpal WASAPI, resampling, jitter buffer) |
| `ui/call.rs` | floating call card (incoming accept/decline, calling, timer, mute, hang up) |

Key decisions:
- **TLS: native-tls** (SChannel on Windows, so it uses the Windows certificate store). Keeps the build free of C toolchains (no ring or aws-lc).
- **Message list**: `scrollable::anchor_bottom` → loading older pages never moves the viewport;
  only the newest ~160 loaded messages are laid out, more are revealed when scrolling up
  (render window), older pages fetched via `before_message_id` (50/page).
- **Thread cache**: last 12 opened chats stay in memory for instant switching; reconciled with
  the server's latest page on reopen and after WebSocket reconnects.
- **Idle CPU**: no timers unless something time-based is pending (typing expiry, toasts).
- **Jump to message**: custom widget `Operation` measures the target container inside the
  scrollable, then chains a `scroll_to` (offsets are bottom-anchored).
- **Calls**: str0m (pure-Rust WebRTC; crypto = Windows CNG/SChannel on Windows, rust-crypto on
  Linux dev builds) + unsafe-libopus (c2rust libopus, no C toolchain) + cpal (WASAPI). Same `call.*`
  signalling and SDP/ICE JSON as the web client. Host candidates on every non-loopback IPv4 interface
  (Tailscale included); browser TURN relay candidates are used as remote candidates. str0m has no TURN
  client, so the desktop does not allocate relays itself. This works on the tailnet and LAN, and when the
  other side relays.
- **Backdrop / glass window**: `assets/backdrop*.jpg` are rendered from the web CSS by
  `scripts/gen-backdrop.mjs` (rerun after changing the web backdrop). `ui/backdrop.rs` composes scene,
  `--shadow-window`, frosted glass, sheen and rim into one image per window size (off-thread, debounced),
  so GPU and CPU renderers look identical and a frame costs one blit. Same breakpoint as the web (>1100 px:
  floating window). iced paints shadows beneath translucent fills, so translucent surfaces carry no shadow.
  `scripts/web-shoot.mjs` takes matching web screenshots for side-by-side checks.
- **Videos**: there is no in-app decoder. Videos are downloaded to the cache and opened in the default player.

## Feature parity checklist
Legend: ✅ implemented (compiles; logic unit-tested where noted) · 🔶 partial · ❌ not yet · ➖ n/a on desktop.
"Verified" = exercised against a real backend; nothing is verified on Windows yet.

### Account
- ✅ Login with username or email, remember me
- ✅ Session persistence (refresh cookie, DPAPI) and silent restore on start
- ✅ Register, plus a hint to open the verification email · ✅ resend verification
- ✅ Forgot password (sends email; the reset link opens the web app, like for web users)
- ✅ Logout (server + local) · ✅ forced sign-out on expired/blocked session
- ✅ Server address setting (login screen + Settings → Application)

### Chats & messages
- ✅ Chat list sorted by activity; previews ("You: …", Photo/Video); unread badges; list times
- ✅ Empty private chats hidden (like web); groups always listed
- ✅ User search (debounced) → open existing or create private chat
- ✅ History with pagination (older pages on scroll-up) · ✅ date separators · ✅ sender runs
- ✅ Send text · ✅ replies (quote, jump to original incl. loading older pages)
- ✅ Edit (also ↑ in empty composer) · ✅ delete with confirmation
- ✅ Pin/unpin · ✅ pinned bar (cycles) · ✅ pinned list dialog → jump
- ✅ Read receipts (✓ / ✓✓ from `peer_last_read_at`), mark-read when visible & focused
- ✅ Typing indicators (send throttled 3 s, stop after 4 s idle; receive with 6 s expiry)
- ✅ Online/offline presence (snapshot, online/offline events, presence.get on open)
- ✅ Last seen ("Last seen 5 minutes ago" in header and profile; presence.last_seen; 30 s refresh tick)
- ✅ Blocking: block/unblock from the peer profile (with confirmation), blocked composer bar, call disabled, blocks.updated
- ✅ Images: send (file dialog, Ctrl+V paste), inline preview (lazy), viewer with zoom/pan, prev/next, save as
- 🔶 Videos: send ✅, open in system player ✅, inline playback ❌ (no decoder)
- ✅ Call history cards
- ✅ Scroll-to-latest button with unread count · ✅ per-chat drafts
- ✅ Context menu: reply, pin, copy text, save media, edit, delete
- ✅ Realtime: message.new/updated/deleted, chat.created/updated, chat.read, profile.updated, account.blocked; resync after reconnect

### Groups
- ✅ Create group (name, @username, description, contacts + search, ≥2 members)
- ✅ Group info (details, participants with admins first, member profile)
- ✅ Edit group (group admins) · ✅ site-admin "block account" from member context menu

### Settings
- ✅ Profile: display name, username, bio (160), avatar upload/remove
- ✅ Security: change password (signs out everywhere, as the server does)
- ✅ Active sessions: list, revoke, log out all others
- ✅ Privacy page: last seen Everyone/Nobody, blocked users list with Unblock (other options "coming later" as on web)
- ✅ Application (desktop): notifications, previews, auto-update, server, version

### Admin
- ✅ Admin panel: stats, search, status filter, pagination, block/unblock with confirmation

### Calls & push
- ✅ Audio calls, outgoing and incoming: accept/decline, busy handling, mute, timer, hang up, toast with
  end reason. Notification + taskbar flash + focus on incoming. Verified: engine loopback test, and
  **the web app in Chromium ⇄ desktop engine, both directions, audio both ways** (`call_interop_with_browser`).
  Not yet heard on real Windows audio devices.
- 🔶 No own TURN allocation (see Key decisions)
- ➖ Android FCM push (desktop uses native notifications instead)

### Windows integration
- ✅ Native window, icon (runtime), min size, size/maximized persistence
- 🔶 Embedded .exe icon + version info (build.rs/winresource; needs assets/icon.ico)
- ✅ Toast notifications (sender + preview, optional), taskbar flash on new message/call
- ✅ Unread count in window title
- ✅ Shortcuts: Esc (close menu/dialog/edit/reply/search/chat), Ctrl+K/Ctrl+F search,
  Ctrl+Tab / Ctrl+Shift+Tab next/prev chat, ↑ edit last, Ctrl+V paste image, Ctrl+, settings
- ✅ Clipboard copy · ✅ native file open/save dialogs (rfd)
- ✅ Proper shutdown (settings saved on close request)

### Distribution
- ✅ GitHub Actions `.github/workflows/desktop.yml` (windows-latest: test, clippy, release build,
  installer, artifact `messenger-desktop-<ver>-windows`; `desktop-v*` tag → signed `latest.json` +
  **draft** release). First run 44370733605 passed.
- 🔶 Inno Setup `installer/messenger.iss`: per-user (no UAC), Start Menu + optional desktop
  shortcut with AppUserModelID, uninstaller, fixed AppId for upgrades, closes the running app,
  relaunches after silent update (`/UPDATE=1`). Builds in CI; install/upgrade not yet tested by hand.
- ✅ Icon: `assets/icon.ico` (16–256 px, `cargo run --example make_icon`), embedded by build.rs on Windows.
- ✅ In-app updates (client): finds the newest published `desktop-v*` GitHub release via the API,
  downloads in the background with progress, and checks SHA-256 and the Ed25519 signature (covering version +
  hash), re-verifies before running the installer, skip/later, cleanup of old installers.
  Unit-tested (tamper, replay, wrong key, release selection). The real key pair was verified end to end.
- Signing key: private seed at messenger-01 `~/messenger/secrets/desktop-update-signing-key`
  (gitignored, 0600) → must be added as GitHub secret `DESKTOP_UPDATE_SIGNING_KEY`.
  Public key in `update-public-key.txt`. Tool: `cargo run --example update_tool -- keygen|manifest`.

### Release procedure (when asked)
1. Bump `version` in Cargo.toml, add `release-notes/<version>.md`, commit.
2. Tag `desktop-v<version>` and push the tag (ask first) → CI builds, signs, creates a draft release.
3. Owner reviews and publishes the draft → clients see the update.

## Build status / test results (2026-11-18)
- Linux (rust:1-bookworm container on messenger-01): `cargo check` ✅, `cargo test` ✅ (29 unit tests)
- Windows target type-check (`cargo check --target x86_64-pc-windows-msvc`) ✅
- Live end-to-end test against a throwaway backend (`scripts/it-stack.sh up`, then
  `BUILD_NETWORK=desktop_it BUILD_ENV="-e MESSENGER_IT_SERVER=http://it-proxy" scripts/remote-cargo.sh test live_tests`) ✅:
  login, remember-me restore with cookie rotation, errors, search, private chat, realtime delivery, typing, read receipts,
  reply/edit/pin/unpin/delete, validation, pagination, image upload + decoding, profile/avatar,
  groups + permissions, sessions, admin block → account.blocked, logout.
- Headless UI run (Xvfb, tiny-skia; `scripts/shoot.sh`): chat list, conversation, context menu,
  settings, group info, image viewer, new group, narrow layout ✅ (screenshots reviewed).
- Performance probe (`scripts/perf.sh`, release build, Linux Xvfb + CPU renderer = pessimistic):
  RSS 33 MB, 5 threads, idle CPU 0.00% (chat list or chat without focused input); ~5% of one core
  while the composer has focus (caret blink repaints on the CPU renderer). Release binary 31 MB.
- Windows CI (GitHub Actions windows-latest): ✅ tests, clippy, release build, installer (run 44370733605).
- Call engine: loopback test ✅; browser interop ✅ (`scripts/it-stack.sh up && scripts/it-stack.sh web`,
  then run `call_interop` with MESSENGER_IT_CALL=1 together with `scripts/call-interop.mjs` in the
  desktop-browser image (Playwright + Chromium with fake microphone)).

## How to build/test (from agent-01)
```
desktop/scripts/remote-cargo.sh test                      # unit tests (Linux container)
desktop/scripts/remote-cargo.sh check --target x86_64-pc-windows-msvc
```
The script rsyncs `desktop/` to messenger-01 and runs cargo in Docker (capped 3 CPUs / 5 GB).

## Known issues / limitations
- iced feature `web-colors` must stay enabled: without it wgpu blends translucent fills in linear space
  (much brighter than the browser). tiny-skia screenshots don't show the difference.
- Not yet run on Windows itself.
- iced 0.14 bugs avoided: keyed_column state corruption (use column); tiny-skia partial repaint
  artifacts (seen headless; affects the CPU fallback renderer only).
- Composer is single-line (same as web `<input type=text>`).
- Text in bubbles is not selectable (iced limitation); use "Copy text".
- Chat list previews are truncated by an estimated character count.

## Next steps
1. Owner: install the CI artifact on Windows and try it (login, chats, notifications, a call).
2. Owner adds the `DESKTOP_UPDATE_SIGNING_KEY` secret → first `desktop-v0.1.0` draft release
   (ask before tagging/pushing) → test an update 0.1.0 → 0.1.1.
3. Performance pass on Windows (startup time, idle CPU, memory with large chats).
4. Optional: TURN client for calls outside the tailnet; video inline playback; multi-line composer.
