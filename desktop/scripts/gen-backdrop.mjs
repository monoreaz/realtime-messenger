// Renders the web app's atmospheric backdrop (frontend/src/index.css) to images the desktop
// client draws behind its window: the sharp scene around the window and the frosted, tinted
// copy seen through the glass. Run in the desktop-browser image with the web sources at /web
// (frontend/src) and the desktop tree at /src:
//   docker run --rm -v ~/messenger/frontend/src:/web:ro -v ~/desktop-build:/src -w /shooter \
//     desktop-browser sh -c 'cp /src/scripts/gen-backdrop.mjs . && node gen-backdrop.mjs'
// CSS gradients here are sized in % of the viewport, so stretching the images to any window
// size reproduces what the browser draws at that size.
import { chromium } from "playwright-core";
import { writeFileSync } from "node:fs";

const W = 1600, H = 1000;
const layers = {
    // body::before in index.css (grain, vignette, tint, scene, canvas).
    "backdrop": "",
    // .messenger::before + ::after without the sheen (drawn natively, it depends on window height).
    "backdrop-frosted": `<div style="position:fixed;inset:0;background:linear-gradient(var(--glass-shell),var(--glass-shell)),var(--atmosphere-frosted),var(--canvas)"></div>`,
};
const browser = await chromium.launch({ executablePath: "/usr/bin/chromium-browser", headless: true, args: ["--no-sandbox"] });
const page = await (await browser.newContext({ viewport: { width: W, height: H } })).newPage();
for (const [name, body] of Object.entries(layers)) {
    const html = `<!doctype html><html><head><link rel="stylesheet" href="file:///web/index.css"></head><body>${body}</body></html>`;
    writeFileSync(`/tmp/${name}.html`, html);
    await page.goto(`file:///tmp/${name}.html`);
    await page.waitForTimeout(500);
    await page.screenshot({ path: `/src/assets/${name}.jpg`, type: "jpeg", quality: 92 });
    console.log("wrote", name);
}
await browser.close();
