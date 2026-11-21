# Public deployment: https://chat.slaxz.com and https://slaxz.com

```
visitor ──https──> Cloudflare edge ──tunnel (outbound from VM)──> cloudflared ──> caddy:8081 ─┐
tailnet ──https──> tailscale serve ──> 127.0.0.1:8080 ──────────────────────────> caddy:80  ─┴─> by Host header:
                                                                       slaxz.com → website (website/.deploy/current)
                                                                   www.slaxz.com → 301 to slaxz.com
                                                                     other hosts → backend / static web app
calls   ──udp/tcp 3478, udp 49160-49259──> router 88.146.114.175 ──> coturn on 192.168.30.106
```

- **Web, API, WebSocket:** only via Cloudflare Tunnel. No inbound web ports on the router or VM.
  Caddy's `:8081` listener is not published; the `tunnel` Docker network holds only Caddy and
  cloudflared (172.31.255.2), so the tunnel can't reach the backend or database directly.
- **Client IP:** `:80` trusts `X-Forwarded-For` from tailscale serve, `:8081` trusts
  `Cf-Connecting-Ip` from cloudflared only. Rate limits key on that address.
- **TURN:** `turn.slaxz.com` (DNS only, not proxied), coturn with time-limited HMAC credentials
  from `/api/calls/ice`. It relays only between its own allocations (no relaying to LAN, tailnet
  or arbitrary hosts). Tailnet clients can still use `100.91.201.40`.
- **Tailnet access** (`https://messenger-01.tail1a26d2.ts.net`) is unchanged.

- **Website** (`slaxz.com`): static build from `website/`, same Caddy and tunnel, no extra ports.
  Deploy with `website/scripts/deploy.sh` (atomic symlink switch, no Caddy restart); details in
  `website/README.md`. Test on the VM without DNS: `curl -H Host: slaxz.com http://127.0.0.1:8080/`.

## Files outside git (messenger-01 `~/messenger`)

| File | Content |
|---|---|
| `.env` (0600) | `PUBLIC_APP_URL=https://chat.slaxz.com`, `TURN_URLS=…turn.slaxz.com…`, `COMPOSE_PROFILES=calls,tunnel` |
| `secrets/cloudflared-token` (0600) | tunnel token from the Cloudflare dashboard |
| `secrets/turnserver.conf` (0600) | from `ops/turnserver.conf.example` |

## Operations

```sh
cd ~/messenger
docker compose up -d                       # everything incl. coturn + cloudflared (COMPOSE_PROFILES)
docker compose logs -f cloudflared         # "Registered tunnel connection" x4
docker compose restart backend             # after backend code changes (no --reload)
docker compose run --rm frontend-build     # after frontend changes (static build in frontend/dist)
```

Checks:

```sh
curl -s https://chat.slaxz.com/api/health
curl -s -o /dev/null -w '%{http_code}\n' https://chat.slaxz.com/api/docs      # 404
curl -s -o /dev/null -w '%{http_code}\n' https://chat.slaxz.com/src/main.tsx  # 200 = index.html, not source
```

## Cloudflare settings this setup relies on

- Tunnel public hostname `chat.slaxz.com` → `HTTP` `caddy:8081` (not `:80`, not `localhost`).
- Tunnel public hostnames `slaxz.com` and `www.slaxz.com` → `HTTP` `caddy:8081` (website; Caddy
  picks the site by Host header). Cloudflare creates the proxied DNS records for them.
- DNS `turn` A `88.146.114.175`, **DNS only** (grey cloud). Cloudflare can't proxy TURN.
- Off: Rocket Loader, Email Address Obfuscation, Web Analytics auto-injection (the CSP blocks
  injected scripts), Bot Fight Mode (challenges the desktop and Android API clients).
- Cache Rule: bypass cache for `/api/*` and `/ws` (media is per-user; it is also sent as
  `Cache-Control: private`).
- Free plan request limit is 100 MB; the largest upload (video) is 50 MB.

## Rollback
- Website: `website/scripts/deploy.sh --rollback` (previous build). To take it offline, delete the
  `slaxz.com` / `www.slaxz.com` public hostnames in Cloudflare; the messenger is unaffected.

- Take the site off the internet: `docker compose stop cloudflared` (or delete the public
  hostname in Cloudflare). Tailnet access is unaffected.
- TURN back to tailnet only: restore `secrets/turnserver.conf.pre-public` and the old
  `TURN_URLS` from `~/backups/pre-public-20261121/env`, then `docker compose up -d turn backend`.
