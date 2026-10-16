// 用本机 Edge 无头模式对 mock 层截图（README 素材，全部演示数据）
import puppeteer from "puppeteer-core";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const EDGE = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const BASE = "http://localhost:1420/#";
const OUT = join(process.cwd(), "docs", "screenshots");
mkdirSync(OUT, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: EDGE,
  headless: "new",
  args: ["--no-first-run", "--disable-extensions", "--force-device-scale-factor=2"],
  defaultViewport: { width: 1240, height: 830, deviceScaleFactor: 2 },
});

const page = await browser.newPage();
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function goto(hash) {
  await page.goto(BASE + hash, { waitUntil: "networkidle0" });
  await wait(1100);
}
async function shot(name) {
  await page.screenshot({ path: join(OUT, name) });
  console.log("✓", name);
}
async function clickByText(sel, text) {
  const clicked = await page.evaluate(
    (s, t) => {
      const els = [...document.querySelectorAll(s)];
      const el = els.find((e) => e.textContent.trim().includes(t));
      if (el) { el.click(); return true; }
      return false;
    },
    sel,
    text
  );
  if (!clicked) throw new Error("click failed: " + text);
}

// ---------- 浅色 ----------
await goto("/");
await shot("dashboard-light.png");

await goto("/proxies");
await clickByText("button", "新建代理");
await wait(400);
await page.screenshot({ path: join(OUT, "proxy-form.png") });
console.log("✓ proxy-form.png");
await clickByText("button", "取消");
await wait(300);

await goto("/proxies");
await shot("proxies.png");

await goto("/tools");
await clickByText("button", "开始扫描");
await wait(2400);
await shot("tools.png");

await goto("/logs");
await wait(2600); // 攒几行演示日志
await shot("logs.png");

// ---------- 深色（侧栏按钮切换，走真实 UI 路径） ----------
await goto("/");
await clickByText("button", "深色模式");
await wait(500);
await shot("dashboard-dark.png");

await browser.close();
console.log("all screenshots done ->", OUT);
