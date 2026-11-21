// Live refresh of the download buttons from GitHub Releases, plus device detection.
// The page already contains a build-time snapshot, so this only matters when a release was
// published after the last build. Failures (offline, rate limit) leave the snapshot in place.
import { describe, pickDownloads, RELEASES_API, type Downloads, type GhRelease, type PlatformId } from "../lib/releases";

const CACHE_KEY = "slaxz:releases:v1";
const CACHE_TTL_MS = 10 * 60 * 1000; // GitHub allows 60 unauthenticated requests per hour and IP.

function detectPlatform(): PlatformId | "web" {
  const ua = navigator.userAgent;
  if (/Android/i.test(ua)) return "android";
  if (/Windows NT/i.test(ua)) return "windows";
  return "web";
}

function markCurrentPlatform() {
  const current = detectPlatform();
  document.querySelector(`[data-platform-card="${current}"]`)?.setAttribute("data-current", "");
  const label = document.querySelector("[data-hero-download-label]");
  if (label && current === "windows") label.textContent = "Download for Windows";
  if (label && current === "android") label.textContent = "Download for Android";
}

function apply(dl: Downloads) {
  for (const [id, d] of Object.entries(dl) as [PlatformId, NonNullable<Downloads[PlatformId]>][]) {
    const link = document.querySelector<HTMLAnchorElement>(`[data-dl-link="${id}"]`);
    const meta = document.querySelector(`[data-dl-meta="${id}"]`);
    const sha = document.querySelector<HTMLAnchorElement>(`[data-dl-sha="${id}"]`);
    if (link) link.href = d.url;
    if (meta) meta.textContent = describe(d);
    if (sha) {
      sha.hidden = !d.sha256Url;
      sha.classList.toggle("hidden", !d.sha256Url);
      if (d.sha256Url) sha.href = d.sha256Url;
    }
  }
}

async function loadReleases(): Promise<GhRelease[]> {
  try {
    const cached = JSON.parse(sessionStorage.getItem(CACHE_KEY) ?? "null");
    if (cached && Date.now() - cached.at < CACHE_TTL_MS) return cached.releases;
  } catch {
    /* storage unavailable */
  }
  const res = await fetch(RELEASES_API, { headers: { Accept: "application/vnd.github+json" } });
  if (!res.ok) throw new Error(`GitHub ${res.status}`);
  const releases = (await res.json()) as GhRelease[];
  try {
    sessionStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), releases }));
  } catch {
    /* storage unavailable */
  }
  return releases;
}

markCurrentPlatform();

// Refresh only when the downloads section is about to be seen (saves API quota).
const section = document.querySelector("[data-downloads]");
if (section && "IntersectionObserver" in window) {
  const io = new IntersectionObserver(
    (entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      io.disconnect();
      loadReleases()
        .then((r) => apply(pickDownloads(r)))
        .catch(() => {
          /* keep the build-time snapshot */
        });
    },
    { rootMargin: "400px" },
  );
  io.observe(section);
}
