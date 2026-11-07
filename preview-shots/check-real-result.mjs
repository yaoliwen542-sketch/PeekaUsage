// 读取真机测试连接结果（临时检查脚本）
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
const expression = `(() => {
  const t = document.body.textContent || "";
  const m = t.match(/(连接成功|连接失败)[^\\n]*/) || [""];
  return { ok: t.includes("连接成功"), fail: t.includes("连接失败"), snippet: m[0].slice(0, 140) };
})()`;
const res = await send("Runtime.evaluate", { expression, returnByValue: true });
console.log(JSON.stringify(res.result?.value, null, 1));
process.exit(0);
