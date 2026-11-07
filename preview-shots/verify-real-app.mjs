// 真机冒烟验证：连真实 Tauri 应用 WebView2 CDP，验证代理设置链路（只读操作，不点保存）
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

const PORT = 9224;
const results = [];
function record(name, ok, detail = "") {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.seq = 1;
    this.pending = new Map();
    ws.addEventListener("message", (e) => {
      const m = JSON.parse(e.data);
      if (m.id && this.pending.has(m.id)) {
        this.pending.get(m.id)(m.result);
        this.pending.delete(m.id);
      }
    });
  }
  send(method, params = {}) {
    const id = this.seq++;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}

async function main() {
  let ws = null;
  for (let i = 0; i < 20; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/json/list`);
      const t = await r.json();
      const p = t.find((x) => x.type === "page");
      if (p) {
        ws = new WebSocket(p.webSocketDebuggerUrl);
        await new Promise((res, rej) => {
          ws.addEventListener("open", res, { once: true });
          ws.addEventListener("error", () => rej(new Error("ws")), { once: true });
        });
        break;
      }
    } catch {}
    await sleep(1000);
  }
  if (!ws) throw new Error("无法连接应用 CDP");
  const cdp = new Cdp(ws);
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");

  const evaluate = async (expression) => {
    const res = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    return res.result?.value;
  };

  await sleep(2000); // 等首屏数据
  const boot = await evaluate(`(() => ({
    hasShell: !!document.querySelector("#app"),
    providerRows: [...document.querySelectorAll("button[title='设置']")].length,
    bodySample: (document.body.textContent || "").slice(0, 120),
  }))()`);
  record("真机主界面渲染", boot?.hasShell && (boot.bodySample || "").length > 0, JSON.stringify(boot?.bodySample ?? ""));

  const clicked = await evaluate(`(() => {
    const btn = document.querySelector('button[title="设置"]');
    if (!btn) return false;
    btn.click();
    return true;
  })()`);
  await sleep(1200);
  record("真机打开设置页", clicked === true);

  const proxy = await evaluate(`(() => ({
    title: [...document.querySelectorAll("h3")].some((h) => (h.textContent || "").includes("网络代理")),
    systemPressed: [...document.querySelectorAll("button")].find((b) => (b.textContent || "").trim() === "跟随系统")?.getAttribute("aria-pressed"),
    testBtn: [...document.querySelectorAll("button")].some((b) => (b.textContent || "").trim() === "测试连接"),
  }))()`);
  record(
    "真机代理分区(真实 config)",
    proxy?.title && proxy?.systemPressed === "true" && proxy?.testBtn,
    `默认模式=${proxy?.systemPressed}`,
  );

  await evaluate(`(() => {
    const btn = [...document.querySelectorAll("button")].find((b) => (b.textContent || "").trim() === "测试连接");
    btn?.click();
    return true;
  })()`);
  await sleep(12000); // 双端点真实网络探测（系统模式=直连）
  const result = await evaluate(`(() => {
    const text = document.body.textContent || "";
    return {
      ok: text.includes("连接成功"),
      fail: text.includes("连接失败"),
      snippet: (text.match(/(连接成功|连接失败)[^\\n]*/) || [""])[0].slice(0, 100),
    };
  })()`);
  record(
    "真机测试连接(系统模式直连)",
    result?.ok || result?.fail,
    `${result?.snippet}（成功或带原因失败均可接受）`,
  );

  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n==== 真机冒烟: ${results.length - failed}/${results.length} 通过 ====`);
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error("真机验证异常:", e);
  process.exit(2);
});
