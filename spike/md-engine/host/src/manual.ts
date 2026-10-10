// Manual test page for IME composition together with the live preview and the Rust sync.
import type { Engine } from "./engine";
import { CORPUS } from "./corpus";

export async function startManual(e: Engine) {
  const r = e.rust!;
  await r.load(CORPUS);
  e.caretAtStart();

  const box = document.createElement("div");
  box.style.cssText = "position:fixed;right:8px;top:8px;width:300px;background:rgba(255,255,255,.96);border:1px solid #ccc;border-radius:6px;padding:8px;font:12px/1.5 -apple-system,'PingFang SC',sans-serif;z-index:20;box-shadow:0 2px 8px rgba(0,0,0,.15)";
  box.innerHTML = `
    <b>输入法 + 同步 手动测试</b>
    <div id="st" style="margin:4px 0;white-space:pre-wrap"></div>
    <div style="display:flex;flex-wrap:wrap;gap:4px;margin-bottom:4px">
      <button id="b1">Agent 改写下一个块</button>
      <button id="b2">Agent 改写光标所在块</button>
      <button id="b3">Agent 追加一段</button>
    </div>
    <label><input type="checkbox" id="auto1"> 每 2 秒改写<b>下一个</b>块</label><br>
    <label><input type="checkbox" id="auto2"> 每 2 秒改写<b>光标所在</b>块(最苛刻)</label>
    <div style="margin:6px 0 2px"><b>事件</b></div>
    <div id="log" style="max-height:170px;overflow:auto;font:11px/1.4 ui-monospace,monospace;color:#333"></div>
    <div style="margin-top:6px;color:#555">试什么:见窗口上方说明。自动改写打开后,在被改写的块里用输入法连续打字。</div>`;
  document.body.appendChild(box);
  const $ = (id: string) => box.querySelector<HTMLElement>("#" + id)!;
  const lines: string[] = [];
  const log = (s: string) => {
    lines.push(`${(performance.now() / 1000).toFixed(1).padStart(6)}  ${s}`);
    if (lines.length > 60) lines.shift();
    $("log").textContent = lines.slice(-14).join("\n");
    $("log").scrollTop = 1e9;
  };
  e.rust!.onEvent(log);

  let n = 0;
  const textFor = () => `Agent 改写 #${++n}：这一段是 Agent 在你输入时改写的,用来检查输入法组合是否被打断😀。`;
  const current = () => r.typed();
  const next = () => {
    const t = r.typed();
    const bs = r.blocks();
    const i = t ? bs.findIndex((b) => b.id === t.blockId) : -1;
    // the next paragraph after the one holding the caret (skip anything that is not a paragraph)
    for (let k = i + 1; k < bs.length; k++) if (bs[k].kind === "paragraph") return bs[k];
    return bs.find((b) => b.kind === "paragraph");
  };
  $("b1").onclick = () => { const b = next(); if (b) r.agent({ op: "replace_block", id: b.id, text: textFor() }); };
  $("b2").onclick = () => { const t = current(); if (t) r.agent({ op: "replace_block", id: t.blockId, text: textFor() }); };
  $("b3").onclick = () => r.agent({ op: "append", text: textFor() });
  let t1 = 0, t2 = 0;
  ($("auto1") as HTMLInputElement).onchange = (ev) => { clearInterval(t1); if ((ev.target as HTMLInputElement).checked) t1 = window.setInterval(() => $("b1").click(), 2000); };
  ($("auto2") as HTMLInputElement).onchange = (ev) => { clearInterval(t2); if ((ev.target as HTMLInputElement).checked) t2 = window.setInterval(() => $("b2").click(), 2000); };

  setInterval(() => {
    const p = r.pending();
    const synced = !p.inflight && !p.queued;
    const st = r.stats();
    const ty = r.typed();
    $("st").textContent =
      `输入法组合: ${r.composing() ? "进行中" : "否"}\n` +
      `同步: 版本 ${r.version()} ${synced ? "· 已同步" : "· 有未确认编辑"}${synced ? ` · 两边文本${r.rustHash() === r.editorHash() ? "一致 ✓" : "不一致 ✗"}` : ""}\n` +
      `Agent 补丁: 已应用 ${st.remoteApplied},其中与未确认编辑交错 ${st.remoteTransformed}\n` +
      `拒绝: ${r.rejects().length}(过期 ${r.rejects().filter((x) => /StaleBase/.test(x)).length})\n` +
      `光标所在块: ${ty ? `${ty.kind} ${ty.blockId} [${ty.headingPath.join(" / ")}]` : "—"}`;
  }, 250);
  log("ready — 已连接 Rust,示例章节已加载");
}
