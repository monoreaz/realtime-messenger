// Picks the newest download per platform from GitHub Releases.
// Used at build time (snapshot in src/data/releases.json) and in the browser (live refresh),
// so both always agree on which file a button points to.

export const REPO = "monoreaz/realtime-messenger";
export const RELEASES_URL = `https://github.com/${REPO}/releases`;
export const RELEASES_API = `https://api.github.com/repos/${REPO}/releases?per_page=50`;

export type PlatformId = "windows" | "android";

export interface PlatformRule {
  /** Release tag prefix, e.g. desktop-v0.1.5 → "desktop-v". */
  tagPrefix: string;
  /** Asset file name of the installer. */
  asset: RegExp;
  /** The Android workflow publishes test builds as pre-releases. */
  allowPrerelease: boolean;
  /**
   * Used while GitHub has no build of this platform at least this new
   * (Android APKs are still published on the messenger server).
   */
  fallback?: { version: string; url: string; sha256Url?: string };
}

export const PLATFORMS: Record<PlatformId, PlatformRule> = {
  windows: {
    tagPrefix: "desktop-v",
    asset: /^Messenger-Setup-[\d.]+\.exe$/,
    allowPrerelease: false,
  },
  android: {
    tagPrefix: "android-v",
    asset: /\.apk$/,
    allowPrerelease: true,
    fallback: {
      version: "0.3.5",
      url: "https://chat.slaxz.com/downloads/messenger-android-0.3.5.apk",
      sha256Url: "https://chat.slaxz.com/downloads/messenger-android-0.3.5.apk.sha256",
    },
  },
};

/** The subset of the GitHub API release object that we keep. */
export interface GhAsset {
  name: string;
  size: number;
  browser_download_url: string;
}
export interface GhRelease {
  tag_name: string;
  html_url: string;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  assets: GhAsset[];
}

export interface Download {
  version: string;
  url: string;
  /** Bytes, when known. */
  size?: number;
  sha256Url?: string;
  /** ISO date, when known. */
  publishedAt?: string;
  /** Release page (GitHub) or undefined for the server fallback. */
  releaseUrl?: string;
  source: "github" | "server";
}

export type Downloads = Partial<Record<PlatformId, Download>>;

/** Compares dotted versions numerically; pre-release suffixes sort before the release. */
export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => {
    const [core, pre] = v.split("-", 2);
    return { nums: core.split(".").map((n) => parseInt(n, 10) || 0), pre: pre ?? "" };
  };
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < Math.max(x.nums.length, y.nums.length); i++) {
    const d = (x.nums[i] ?? 0) - (y.nums[i] ?? 0);
    if (d !== 0) return d;
  }
  if (x.pre === y.pre) return 0;
  if (!x.pre) return 1;
  if (!y.pre) return -1;
  return x.pre < y.pre ? -1 : 1;
}

export function pickDownload(releases: GhRelease[], rule: PlatformRule): Download | undefined {
  let best: Download | undefined;
  for (const r of releases) {
    if (r.draft || (r.prerelease && !rule.allowPrerelease)) continue;
    if (!r.tag_name.startsWith(rule.tagPrefix)) continue;
    const asset = r.assets.find((a) => rule.asset.test(a.name));
    if (!asset) continue;
    const version = r.tag_name.slice(rule.tagPrefix.length);
    if (best && compareVersions(version, best.version) <= 0) continue;
    const sha = r.assets.find((a) => a.name === `${asset.name}.sha256`);
    best = {
      version,
      url: asset.browser_download_url,
      size: asset.size,
      sha256Url: sha?.browser_download_url,
      publishedAt: r.published_at ?? undefined,
      releaseUrl: r.html_url,
      source: "github",
    };
  }
  const fb = rule.fallback;
  if (fb && (!best || compareVersions(best.version, fb.version) < 0)) {
    return { version: fb.version, url: fb.url, sha256Url: fb.sha256Url, source: "server" };
  }
  return best;
}

export function pickDownloads(releases: GhRelease[]): Downloads {
  const out: Downloads = {};
  for (const id of Object.keys(PLATFORMS) as PlatformId[]) {
    const d = pickDownload(releases, PLATFORMS[id]);
    if (d) out[id] = d;
  }
  return out;
}

export function formatSize(bytes?: number): string {
  if (!bytes) return "";
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

export function formatDate(iso?: string): string {
  if (!iso) return "";
  return new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}

/** "Version 0.1.5 · 9.2 MB · 21 Nov 2026" */
export function describe(d: Download): string {
  return [`Version ${d.version}`, formatSize(d.size), formatDate(d.publishedAt)].filter(Boolean).join(" · ");
}
