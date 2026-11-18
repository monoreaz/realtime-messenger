// Browser side of the call interop test: alice uses the real web app in Chromium
// (fake microphone) and calls/answers the desktop engine. Run in the desktop-browser
// image on the desktop_it network with /src mounted (see PROGRESS.md).
import { chromium } from "playwright-core";
import { existsSync } from "node:fs";

const base = "http://it-proxy";
const wait = async (file, ms = 180000) => { const end = Date.now() + ms; while (!existsSync(file)) { if (Date.now() > end) throw new Error("timeout waiting for " + file); await new Promise(r => setTimeout(r, 300)); } };

const login = await fetch(`${base}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: "username=alice&password=password123" });
const { access_token: token } = await login.json();
await wait("/src/shots/interop-ready");

const browser = await chromium.launch({
    executablePath: "/usr/bin/chromium-browser", headless: true,
    args: ["--no-sandbox", "--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream", `--unsafely-treat-insecure-origin-as-secure=${base}`],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 860 } });
await context.addInitScript(`window.testPeers=[];const O=window.RTCPeerConnection;window.RTCPeerConnection=class extends O{constructor(c){super(c);window.testPeers.push(this);}};`);
const page = await context.newPage();
page.on("console", m => { if (m.type() === "error") console.log("browser:", m.text()); });
await page.route("**/api/auth/refresh", route => route.fulfill({ json: { access_token: token, token_type: "bearer" } }));
await page.goto(base);
await page.locator(".chat-item").filter({ hasText: "Bob" }).click();
const callButton = page.getByRole("button", { name: "Start audio call", exact: true });
await callButton.waitFor();
await page.waitForFunction(() => !document.querySelector('button[aria-label="Start audio call"]')?.disabled, null, { timeout: 30000 });

const stats = () => page.evaluate(async () => {
    const pc = window.testPeers.at(-1); const report = await pc.getStats(); let inbound = 0, outbound = 0;
    report.forEach(s => { if (s.type === "inbound-rtp" && s.kind === "audio") inbound += s.bytesReceived; if (s.type === "outbound-rtp" && s.kind === "audio") outbound += s.bytesSent; });
    return { state: pc.connectionState, inbound, outbound };
});
const connected = () => page.waitForFunction(() => document.querySelector(".audio-call-status")?.textContent === "Connected", null, { timeout: 40000 });
const results = {};

// A: browser calls desktop.
await callButton.click();
await connected();
await page.waitForTimeout(3000);
results.browserToDesktop = await stats();
await page.locator(".audio-call-dialog").waitFor({ state: "detached", timeout: 30000 });

// B: desktop calls browser.
await wait("/src/shots/interop-phase-b");
await page.getByRole("button", { name: "Answer", exact: true }).click({ timeout: 30000 });
await connected();
await page.waitForTimeout(3000);
results.desktopToBrowser = await stats();
await page.locator(".audio-call-dialog").waitFor({ state: "detached", timeout: 30000 });
await page.screenshot({ path: "/src/shots/interop-browser.png" });
console.log("BROWSER RESULTS", JSON.stringify(results));
await browser.close();
const ok = Object.values(results).every(r => r.inbound > 2000 && r.outbound > 2000);
process.exit(ok ? 0 : 1);
