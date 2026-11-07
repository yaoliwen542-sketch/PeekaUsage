// 真机验证：切到「不使用」模式（不保存）再测连接，验证模式切换影响探测 client + 系统代理确实是坏链路
const r = await fetch("http://127.0.0.1:9224/json/list");
const t = await r.json();
const p = t.find((x) => x.type === "page");
const ws = new WebSocket(p.webSocketDebuggerUrl);
await new Promise((res, rej) => {
  ws.addEventListener("open", res, { once: true });
  ws.addEventListener("error", () => rej(new Error("ws")), { once: true });
});
let seq = 1;
const pending = new Map();
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) {
    pending.get(m.id)(m.result);
    pending.delete(m.id);
  }
});
const send = (method, params = {}) =>
  new Promise((res) => {
    const id = seq++;
    pending.set(id, res);
    ws.send(JSON.stringify({ id, method, params }));
  });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await send("Runtime.enable");
// 1. 点「不使用」
await send("Runtime.evaluate", {
  expression: `(() => { const b=[...document.querySelectorAll("button")].find(x=>(x.textContent||"").trim()==="不使用"); if(!b) return "no-btn"; b.click(); return "clicked"; })()`,
  returnByValue: true,
});
await sleep(400);
// 2. 确认分段选中态
const pressed = await send("Runtime.evaluate", {
  expression: `(() => { const b=[...document.querySelectorAll("button")].find(x=>(x.textContent||"").trim()==="不使用"); return b?.getAttribute("aria-pressed"); })()`,
  returnByValue: true,
});
console.log("none-mode aria-pressed:", JSON.stringify(pressed.result?.value));
// 3. 点测试连接（none 模式 = no_proxy 直连，8s×2 最多 16s）
await send("Runtime.evaluate", {
  expression: `(() => { const b=[...document.querySelectorAll("button")].find(x=>(x.textContent||"").trim()==="测试连接"); b?.click(); return true; })()`,
  returnByValue: true,
});
await sleep(18000);
// 4. 读结果
const res = await send("Runtime.evaluate", {
  expression: `(() => { const t = document.body.textContent || ""; const m = t.match(/(连接成功|连接失败)[^\\n]*/) || [""]; return { ok: t.includes("连接成功"), fail: t.includes("连接失败"), snippet: m[0].slice(0, 140) }; })()`,
  returnByValue: true,
});
console.log(JSON.stringify(res.result?.value, null, 1));
process.exit(0);
