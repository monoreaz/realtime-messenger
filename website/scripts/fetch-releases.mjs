// Prebuild step: snapshot the public GitHub Releases into src/data/releases.json.
// The page is built from this snapshot and refreshes it live in the browser. If GitHub is
// unreachable or rate-limited, the existing snapshot is kept so the build never fails on it.
// Optional: GITHUB_TOKEN raises the API rate limit (never committed, never required).

import { readFile, writeFile } from "node:fs/promises";

const REPO = "monoreaz/realtime-messenger";
const OUT = new URL("../src/data/releases.json", import.meta.url);

const headers = { Accept: "application/vnd.github+json", "User-Agent": "slaxz-website-build" };
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

try {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=50`, {
    headers,
    signal: AbortSignal.timeout(15000),
  });
  if (!res.ok) throw new Error(`GitHub API ${res.status} ${res.statusText}`);
  const releases = (await res.json()).map((r) => ({
    tag_name: r.tag_name,
    html_url: r.html_url,
    draft: r.draft,
    prerelease: r.prerelease,
    published_at: r.published_at,
    assets: r.assets.map((a) => ({ name: a.name, size: a.size, browser_download_url: a.browser_download_url })),
  }));
  const json = JSON.stringify(releases, null, 2) + "\n";
  const old = await readFile(OUT, "utf8").catch(() => "");
  if (json !== old) await writeFile(OUT, json);
  console.log(`releases: ${releases.length} releases from GitHub${json === old ? " (unchanged)" : " (updated)"}`);
} catch (err) {
  console.warn(`releases: keeping the existing snapshot (${err.message})`);
}
