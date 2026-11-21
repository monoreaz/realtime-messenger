// @ts-check
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  site: "https://slaxz.com",
  output: "static",
  trailingSlash: "ignore",
  integrations: [sitemap()],
  build: {
    // External files only, so Caddy's CSP can stay at script-src/style-src 'self'.
    inlineStylesheets: "never",
    assets: "assets",
  },
  vite: {
    plugins: [tailwindcss()],
    build: { assetsInlineLimit: 0 },
  },
  devToolbar: { enabled: false },
});
