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
**Milestones 2–4 in progress.** The app compiles on Linux. The unit tests pass. The Windows
type-check status is under *Build status*. No Windows build yet (needs GitHub Actions, push
pending approval).

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
| `ui/` | `theme.rs` (design tokens), `icons.rs` (web icon paths as SVG), `widgets.rs`, `auth.rs`, `sidebar.rs`, `chat.rs`, `settings.rs` |
| `update.rs` | manifest check, background download with progress, SHA-256 + Ed25519 verification, silent installer launch |
| `platform.rs` | notifications (WinRT toast via notify-rust), AppUserModelID, open-with-default-app |

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
- **Calls**: WebRTC audio is not implemented natively yet. Incoming calls show a dialog and a notification
  (Dismiss / Decline). The phone button explains that calls are coming.
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
- ✅ Privacy page (same "coming later" placeholders as web)
- ✅ Application (desktop): notifications, previews, auto-update, server, version

### Admin
- ✅ Admin panel: stats, search, status filter, pagination, block/unblock with confirmation

### Calls & push
- ❌ Audio calls (WebRTC, Opus, TURN) — planned; needs webrtc-rs + cpal + opus
- 🔶 Incoming call: notification + dialog (dismiss / decline)
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
- ❌ GitHub Actions Windows workflow
- ❌ Inno Setup installer (per-user, Start Menu, AppUserModelID, uninstall, upgrades)
- 🔶 In-app updates: client side done + unit-tested (signature, replay, tamper, version);
  manifest publishing + signing key + release workflow not done

## Build status
- Linux (rust:1-bookworm container on messenger-01): `cargo check` ✅, `cargo test` ✅ (25 tests)
- Windows target type-check (`cargo check --target x86_64-pc-windows-msvc`): see the latest run below
- Windows build on GitHub Actions: not run yet

## How to build/test (from agent-01)
```
desktop/scripts/remote-cargo.sh test                      # unit tests (Linux container)
desktop/scripts/remote-cargo.sh check --target x86_64-pc-windows-msvc
```
The script rsyncs `desktop/` to messenger-01 and runs cargo in Docker (capped 3 CPUs / 5 GB).

## Known issues / limitations
- Not yet run against a real backend or on Windows.
- Composer is single-line (same as web `<input type=text>`).
- Text in bubbles is not selectable (iced limitation); use "Copy text".
- Chat list previews are truncated by an estimated character count.

## Next steps
1. Integration-test the client against a throwaway backend stack (backend image + tmpfs postgres).
2. Headless screenshots for visual comparison with the web UI.
3. GitHub Actions Windows workflow (ask before pushing).
4. icon.ico, installer, update signing key + release workflow.
5. Native audio calls.
