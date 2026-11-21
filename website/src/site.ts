// Site-wide constants. Downloads come from GitHub Releases (src/lib/releases.ts).
import { RELEASES_URL, REPO } from "./lib/releases";

export const SITE = {
  name: "Slaxz",
  title: "Slaxz — chats, groups and calls on a server we run ourselves",
  description:
    "Slaxz is a realtime messenger for chats, groups, photos, video messages and voice calls. Use it in your browser, on Windows or on Android.",
  webApp: "https://chat.slaxz.com",
  releases: RELEASES_URL,
  source: `https://github.com/${REPO}`,
} as const;

export const NAV = [
  { href: "#features", label: "Features" },
  { href: "#download", label: "Download" },
  { href: "#security", label: "Security" },
  { href: "#faq", label: "FAQ" },
] as const;
