# Slaxz website (https://slaxz.com)

Official website, served by the messenger's Caddy. Everything here is independent from the
messenger: own dependencies (`package.json`), own build container (`compose.yml`, Compose project
`slaxz-website`), own CI workflow (`.github/workflows/website.yml`, runs only for `website/**`).
Building or deploying the site never rebuilds or restarts a messenger service.

Stack: Astro 7 (static output, zero JS by default) + Tailwind CSS 4, Inter (self-hosted).
Design: the messenger's "graphite glass" tokens (`src/styles/global.css`) and brand mark
(`src/assets/brand-mark.webp`, copied from `frontend/`). Sections were planned with the
UI UX Pro Max skill; the features bento and download grid are adapted from 21st.dev components
(credited in `BentoCard.astro` and `Downloads.astro`).

## Deploy (messenger-01)

```sh
cd ~/messenger/website
scripts/deploy.sh              # build in node:24-alpine, publish atomically
scripts/deploy.sh --list       # published builds (* = live)
scripts/deploy.sh --rollback   # back to the previous build
```

`deploy.sh` copies `dist/` to `.deploy/releases/<timestamp>/` and switches the
`.deploy/current` symlink with an atomic rename. Caddy serves `.deploy/current` (read-only bind
mount at `/srv/website`), so a deploy needs no Caddy reload. The last 5 builds are kept.
`.deploy/`, `dist/` and `node_modules/` are git-ignored.

Local development: `docker compose --profile dev up dev` → http://127.0.0.1:4321.

## Downloads (GitHub Releases)

`src/lib/releases.ts` picks the newest installer per platform from the public releases of
`monoreaz/realtime-messenger`:

| Platform | Tag | Asset |
|---|---|---|
| Windows | `desktop-v*` (releases only) | `Messenger-Setup-<version>.exe` + `.sha256` |
| Android | `android-v*` (pre-releases allowed) | `*.apk` + `.sha256` |

- At build time `scripts/fetch-releases.mjs` snapshots the API into `src/data/releases.json`
  (kept as is when GitHub is unreachable). Set `GITHUB_TOKEN` only to raise the rate limit.
- In the browser `src/scripts/downloads.ts` refreshes the buttons from the API (cached 10 min in
  `sessionStorage`), so a new release appears without redeploying.
- Android fallback: GitHub has no current Android build yet (newest is 0.1.2), so the
  `fallback` in `PLATFORMS.android` (server APK 0.3.5) wins until a newer `android-v*` release
  exists. Raise or remove it when publishing Android releases on GitHub.

## Serving (Caddy, Cloudflare Tunnel)

- `Caddyfile`: requests for `slaxz.com` get the `(website)` snippet, `www.slaxz.com` redirects to
  it, every other host keeps getting the messenger, on both listeners (`:80` tailnet,
  `:8081` tunnel). The website has its own CSP (`connect-src` allows `api.github.com`).
- Cloudflare: the tunnel needs public hostnames `slaxz.com` and `www.slaxz.com` → `HTTP`
  `caddy:8081` (same target as `chat.slaxz.com`). See `ops/PUBLIC.md`.
- Test without DNS: `curl -H 'Host: slaxz.com' http://127.0.0.1:8080/` on messenger-01.
