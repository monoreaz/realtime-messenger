// Screenshots of the real web app (Chromium) against the integration stack, for
// side-by-side design comparison with scripts/shoot.sh. Run in the desktop-browser image
// on the desktop_it network with /src mounted. Writes /src/shots/web-*.png.
import { chromium } from "playwright-core";

const base = "http://it-proxy";
const W = Number(process.env.SHOT_WIDTH || 1280), H = Number(process.env.SHOT_HEIGHT || 800);
const login = await fetch(`${base}/api/auth/login`, {
    method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: `username=${process.env.SHOT_USER || "alice"}&password=password123`,
});
const { access_token: token } = await login.json();

const browser = await chromium.launch({ executablePath: "/usr/bin/chromium-browser", headless: true, args: ["--no-sandbox"] });
const page = await (await browser.newContext({ viewport: { width: W, height: H } })).newPage();
await page.route("**/api/auth/refresh", route => route.fulfill({ json: { access_token: token, token_type: "bearer" } }));
const shot = async (name, ms = 800) => { await page.waitForTimeout(ms); await page.screenshot({ path: `/src/shots/web-${name}.png` }); console.log("shot", name); };

await page.goto(base);
await page.locator(".chat-item").first().waitFor();
await shot("01-chat-list", 1500);
await page.locator(".chat-item").first().click();
await shot("02-conversation", 2000);
await page.mouse.move(34, 101);
await shot("03-rail-hover", 500);
// Sign-in screen in a fresh context (no session).
const auth = await (await browser.newContext({ viewport: { width: W, height: H } })).newPage();
await auth.goto(base);
await auth.waitForTimeout(1500);
await auth.screenshot({ path: "/src/shots/web-10-auth.png" });
console.log("shot 10-auth");
await browser.close();
