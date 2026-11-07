// 阶段一全局代理自动化 GUI 验证（零依赖：Node 24 全局 WebSocket + Edge CDP）
// 用法：先起 vite dev（--port 5201），再 node preview-shots/verify-proxy.mjs
// 覆盖：设置页代理分区渲染/模式切换/测试连接成功与失败/保存流/暗色主题/控制台错误收集
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const EDGE = "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const PORT = 9223;
const BASE = "http://localhost:5201/preview.html";
const SHOT_DIR = "preview-shots";

const results = [];
let consoleErrors = [];

function record(name, ok, detail = "") {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---- CDP 客户端 ----
class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.seq = 1;
    this.pending = new Map();
    ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) reject(new Error(JSON.stringify(msg.error)));
        else resolve(msg.result);
      }
    });
  }
  send(method, params = {}) {
    const id = this.seq++;
    const payload = JSON.stringify({ id, method, params });
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(payload);
    });
  }
}

async function connectCdp() {
  for (let i = 0; i < 50; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
      const targets = await res.json();
      const page = targets.find((t) => t.type === "page");
      if (page) {
        const ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((resolve, reject) => {
          ws.addEventListener("open", resolve, { once: true });
          ws.addEventListener("error", () => reject(new Error("WebSocket 连接失败")), { once: true });
        });
        return new Cdp(ws);
      }
    } catch (error) {
      if (error?.message === "WebSocket 连接失败") throw error;
      // Edge 未就绪，重试
    }
    await sleep(300);
  }
  throw new Error("无法连接 Edge CDP");
}

async function evaluate(cdp, expression) {
  const res = await cdp.send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (res.exceptionDetails) {
    throw new Error("页面脚本异常: " + JSON.stringify(res.exceptionDetails.exception?.description ?? res.exceptionDetails.text));
  }
  return res.result.value;
}

async function navigate(cdp, url) {
  await cdp.send("Page.navigate", { url });
  await sleep(2500); // React 挂载 + mock 数据就位
}

async function clickButtonByText(cdp, text) {
  const ok = await evaluate(
    cdp,
    `(() => {
      const buttons = [...document.querySelectorAll("button")];
      const target = buttons.find((b) => (b.textContent || "").trim().includes(${JSON.stringify(text)}));
      if (!target) return false;
      target.click();
      return true;
    })()`,
  );
  if (!ok) throw new Error(`找不到按钮: ${text}`);
  await sleep(400);
}

function setNativeValue(selector, value) {
  return `(() => {
    const input = document.querySelector(${JSON.stringify(selector)});
    if (!input) return false;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    setter.call(input, ${JSON.stringify(value)});
    input.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  })()`;
}

async function openSettings(cdp) {
  const clicked = await evaluate(
    cdp,
    `(() => {
      const btn = document.querySelector('button[title="设置"]');
      if (!btn) return false;
      btn.click();
      return true;
    })()`,
  );
  if (!clicked) {
    const dump = await evaluate(
      cdp,
      `[...document.querySelectorAll("button")].map((b) => b.getAttribute("aria-label") + "|" + (b.textContent || "").trim().slice(0, 16)).join(" ; ")`,
    );
    throw new Error(`找不到设置按钮。页面按钮: ${dump}`);
  }
  await sleep(800);
}

async function assertProxySection(cdp, theme) {
  const state = await evaluate(cdp, `(() => {
    const textOf = (el) => (el.textContent || "").trim();
    const buttons = [...document.querySelectorAll("button")];
    const titleHit = [...document.querySelectorAll("h3")].some((h) => textOf(h).includes("网络代理"));
    const modeBtn = (label) => {
      const b = buttons.find((x) => textOf(x) === label);
      return b ? b.getAttribute("aria-pressed") : null;
    };
    return {
      titleHit,
      none: modeBtn("不使用"),
      system: modeBtn("跟随系统"),
      custom: modeBtn("自定义"),
      urlInputVisible: !!document.querySelector('input[placeholder*="7890"], input[inputmode="url"]'),
      hintVisible: textOf(document.body).includes("用量查询请求的出站方式"),
    };
  })()`);
  const active = Object.entries(state).filter(([k, v]) => v === "true").map(([k]) => k);
  record(
    `代理分区渲染(${theme})`,
    state.titleHit && state.hintVisible && state.system === "true" && active.length === 1,
    `标题=${state.titleHit} 提示=${state.hintVisible} 默认模式=${active.join(",")}`,
  );
}

async function runTestConnection(cdp, theme, expectOk) {
  await clickButtonByText(cdp, "自定义");
  const filled = await evaluate(cdp, setNativeValue('input[inputmode="url"]', "http://127.0.0.1:7890"));
  if (!filled) throw new Error("代理地址输入框未找到");
  await clickButtonByText(cdp, "测试连接");
  await sleep(600);
  const bodyText = await evaluate(cdp, "document.body.textContent");
  const okShown = bodyText.includes("连接成功");
  const failShown = bodyText.includes("连接失败");
  const match = expectOk ? okShown && !failShown : failShown;
  record(
    `测试连接(${theme}/${expectOk ? "成功路径" : "失败路径"})`,
    match,
    expectOk
      ? bodyText.match(/连接成功[^\n]*/)?.[0]?.slice(0, 80)
      : bodyText.match(/连接失败[^\n]*/)?.[0]?.slice(0, 80),
  );
}

async function runSave(cdp, theme) {
  await clickButtonByText(cdp, "保存");
  await sleep(700);
  const state = await evaluate(cdp, `(() => {
    const buttons = [...document.querySelectorAll("button")];
    const textOf = (el) => (el.textContent || "").trim();
    return {
      customPressed: buttons.find((x) => textOf(x) === "自定义")?.getAttribute("aria-pressed"),
      urlValue: document.querySelector('input[inputmode="url"]')?.value ?? "",
      passwordEmpty: (document.querySelector('input[type="password"]')?.value ?? "") === "",
    };
  })()`);
  record(
    `保存流(${theme})`,
    state.customPressed === "true" && state.urlValue === "http://127.0.0.1:7890" && state.passwordEmpty,
    `URL草稿=${state.urlValue}`,
  );
}

async function main() {
  mkdirSync(SHOT_DIR, { recursive: true });
  const profile = mkdtempSync(join(tmpdir(), "peeka-edge-"));
  const edge = spawn(EDGE, [
    "--headless=new",
    "--disable-gpu",
    `--remote-debugging-port=${PORT}`,
    `--user-data-dir=${profile}`,
    "--window-size=560,900",
    "about:blank",
  ], { stdio: "ignore" });

  try {
    const cdp = await connectCdp();
    await cdp.send("Page.enable");
    await cdp.send("Runtime.enable");
    await cdp.send("Log.enable");
    cdp.ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") {
        consoleErrors.push(String(msg.params.args?.[0]?.value ?? msg.params.args?.[0]?.description ?? "unknown"));
      }
      if (msg.method === "Log.entryAdded" && msg.params.entry.level === "error") {
        consoleErrors.push(msg.params.entry.text);
      }
    });

    // 亮色：渲染 → 测试成功路径 → 保存
    await navigate(cdp, `${BASE}?scene=widget&theme=light`);
    await openSettings(cdp);
    await assertProxySection(cdp, "light");
    await runTestConnection(cdp, "light", true);
    await runSave(cdp, "light");

    // 失败路径（?proxyTest=fail）
    await navigate(cdp, `${BASE}?scene=widget&theme=light&proxyTest=fail`);
    await openSettings(cdp);
    await runTestConnection(cdp, "light", false);

    // 暗色 + 截图留档
    await navigate(cdp, `${BASE}?scene=widget&theme=dark`);
    await openSettings(cdp);
    await runTestConnection(cdp, "dark", true);
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" });
    const { writeFileSync } = await import("node:fs");
    writeFileSync(join(SHOT_DIR, "proxy-dark.png"), Buffer.from(shot.data, "base64"));
    record("暗色主题截图", true, "preview-shots/proxy-dark.png");

    const errors = consoleErrors.filter((e) => !e.includes("favicon") && !e.includes("Autofill"));
    record("控制台零错误", errors.length === 0, errors.slice(0, 3).join(" | "));
  } finally {
    edge.kill();
  }

  const failed = results.filter((r) => !r.ok);
  console.log(`\n==== 汇总: ${results.length - failed.length}/${results.length} 通过 ====`);
  process.exit(failed.length > 0 ? 1 : 0);
}

main().catch((error) => {
  console.error("验证脚本异常:", error);
  process.exit(2);
});
