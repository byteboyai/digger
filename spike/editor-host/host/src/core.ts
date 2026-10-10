// Editor-library-independent host core: bridge envelope, edit debounce/in-flight protocol,
// rebase/conflict handling, scripted self-test commands. Both adapters (Tiptap, Milkdown)
// only construct a ProseMirror EditorView and install the two plugins below.
/* eslint-disable @typescript-eslint/no-explicit-any */

declare global {
  interface Window {
    ipc: { postMessage(msg: string): void };
    __DIGGER_BINDING__: { projectId: string; panelId: string; documentId: string };
    __digger: { receive(json: string): void };
  }
}

export const hooks = { onChange: () => {}, loading: false };

// crypto.randomUUID needs a secure context; WKWebView `with_html` pages are not one (spike finding).
export function newId(): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

// Stable block identity. A top-level block with no id (new, or re-created by an in-place type change such as the
// Markdown rule "## ") inherits the id of the old block it maps back to; ids that are missing or duplicated
// (ProseMirror copies attrs when splitting) get a fresh one.
export function blockIdPlugin(Plugin: any, PluginKey: any) {
  return new Plugin({
    key: new PluginKey("diggerBlockId"),
    appendTransaction(trs: any[], oldState: any, state: any) {
      if (!trs.some((t) => t.docChanged)) return null;
      const present = new Set<string>();
      state.doc.forEach((n: any) => { if (n.attrs.blockId) present.add(n.attrs.blockId); });
      const inverse = trs.map((t) => t.mapping.invert()).reverse();
      const seen = new Set<string>();
      const tr = state.tr;
      let changed = false;
      state.doc.forEach((node: any, offset: number) => {
        let id = node.attrs.blockId as string | null;
        if (!id) {
          let pos = offset;
          for (const m of inverse) pos = m.map(pos, 1);
          if (pos >= 0 && pos <= oldState.doc.content.size) {
            const $p = oldState.doc.resolve(pos);
            const cand = $p.depth === 0 ? $p.nodeAfter?.attrs.blockId : null;
            if (cand && !present.has(cand) && !seen.has(cand)) id = cand;
          }
        }
        if (!id || seen.has(id)) id = newId();
        if (id !== node.attrs.blockId) {
          tr.setNodeMarkup(offset, undefined, { ...node.attrs, blockId: id });
          changed = true;
        }
        seen.add(id);
      });
      return changed ? tr : null;
    },
  });
}

export function changePlugin(Plugin: any, PluginKey: any) {
  return new Plugin({
    key: new PluginKey("diggerChange"),
    view: () => ({
      update(view: any, prev: any) {
        if (!hooks.loading && !view.state.doc.eq(prev.doc)) hooks.onChange();
      },
    }),
  });
}

export function startHost(view: any, adapter: string) {
  const binding = window.__DIGGER_BINDING__;
  let seq = 0;
  let baseRev = "";
  let rev = 0;
  let editDebounceMs = 1200;
  // One edit in flight at a time. "Clean" (nothing unsent, nothing in flight) is the only state in which
  // the host may replace its document.
  let inflight = false;
  let dirty = false;
  let editTimer: number | undefined;
  let snapTimer: number | undefined;
  let lastLoadMs = 0;

  const send = (type: string, payload: unknown) =>
    window.ipc.postMessage(JSON.stringify({ hostKind: "editor", binding, seq: ++seq, type, payload }));
  const isClean = () => !inflight && !dirty && editTimer === undefined;
  const getJSON = () => view.state.doc.toJSON();
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  function scheduleEdit() {
    if (editDebounceMs <= 0) return flushEdit();
    clearTimeout(editTimer);
    editTimer = window.setTimeout(flushEdit, editDebounceMs);
  }
  function flushEdit() {
    clearTimeout(editTimer);
    editTimer = undefined;
    sendEdit();
  }
  function sendEdit() {
    if (inflight) { dirty = true; return; }
    inflight = true;
    dirty = false;
    send("edit", { baseRev, doc: getJSON() });
  }
  function acked() {
    inflight = false;
    if (dirty) sendEdit();
  }
  function scheduleSnapshot() {
    clearTimeout(snapTimer);
    snapTimer = window.setTimeout(() => send("snapshot", { baseRev, doc: getJSON() }), 300);
  }
  hooks.onChange = () => { scheduleSnapshot(); scheduleEdit(); };

  function applyDoc(doc: any) {
    const t0 = performance.now();
    const { from, to } = view.state.selection;
    hooks.loading = true;
    try {
      const node = view.state.schema.nodeFromJSON(doc);
      const tr = view.state.tr.replaceWith(0, view.state.doc.content.size, node.content).setMeta("addToHistory", false);
      view.dispatch(tr);
    } finally {
      hooks.loading = false;
    }
    const max = view.state.doc.content.size;
    try {
      const sel = view.state.selection.constructor;
      view.dispatch(view.state.tr.setSelection((sel as any).between(view.state.doc.resolve(Math.min(from, max)), view.state.doc.resolve(Math.min(to, max)))));
    } catch { /* selection restore is best-effort */ }
    lastLoadMs = performance.now() - t0;
  }

  const banner = document.getElementById("conflict")!;
  const hideConflict = () => { banner.style.display = "none"; };
  function showConflict(items: { mine: string; official: string }[]) {
    document.getElementById("conflict-text")!.textContent =
      `${items.length} 个块同时被修改：` + items.map((i) => `“${i.mine}” ↔ “${i.official}”`).join("；");
    banner.style.display = "block";
  }
  document.getElementById("conflict-mine")!.addEventListener("click", () => send("resolve", { choice: "mine" }));
  document.getElementById("conflict-official")!.addEventListener("click", () => send("resolve", { choice: "official" }));

  window.addEventListener("error", (e) => send("failed", { message: String(e.message), line: e.lineno }));
  window.addEventListener("unhandledrejection", (e) => send("failed", { message: String((e as PromiseRejectionEvent).reason) }));
  const viewport = () => ({ w: window.innerWidth, h: window.innerHeight });
  window.addEventListener("resize", () => send("title", { viewport: viewport() }));

  function blockEnd(index: number): number {
    let off = 0;
    for (let i = 0; i < view.state.doc.childCount; i++) {
      const n = view.state.doc.child(i);
      if (i === index) return off + n.nodeSize - 1;
      off += n.nodeSize;
    }
    throw new Error("no such block " + index);
  }

  // Scripted edits (basic self-test) through raw ProseMirror transactions, identical for every adapter.
  async function basicSelfTest() {
    view.focus();
    view.dispatch(view.state.tr.insertText("（自测追加）", blockEnd(0)));
    await sleep(120);
    view.dispatch(view.state.tr.split(4)); // PM copies attrs: the plugin must de-duplicate blockId
    await sleep(120);
    const last = view.state.doc.lastChild;
    const end = view.state.doc.content.size;
    view.dispatch(view.state.tr.delete(end - last.nodeSize, end));
    await sleep(120);
    while (inflight || dirty) await sleep(20);
    await sleep(700); // let the debounced recovery snapshot fire
    send("selftest_done", {});
  }

  // Performance probe on whatever document is currently loaded.
  async function perf() {
    const raf = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
    const t = (f: () => void) => { const a = performance.now(); f(); return performance.now() - a; };
    await raf();
    const getJsonMs = Array.from({ length: 5 }, () => t(() => getJSON())).reduce((a, b) => a + b, 0) / 5;
    const jsonBytes = JSON.stringify(getJSON()).length;
    const mid = Math.floor(view.state.doc.childCount / 2);
    const pos = blockEnd(mid);
    hooks.loading = true; // measure the editor itself, not our debounce scheduling
    const N = 40;
    const txMs: number[] = [];
    for (let i = 0; i < N; i++) {
      txMs.push(t(() => view.dispatch(view.state.tr.insertText("字", pos + i))));
    }
    const a = performance.now();
    view.dispatch(view.state.tr.insertText("尾", pos + N));
    await raf();
    const paintMs = performance.now() - a;
    hooks.loading = false;
    txMs.sort((x, y) => x - y);
    send("selftest_report", {
      perf: {
        adapter, blocks: view.state.doc.childCount, loadMs: lastLoadMs, getJsonMs, jsonBytes,
        txMeanMs: txMs.reduce((x, y) => x + y, 0) / N, txP95Ms: txMs[Math.floor(N * 0.95)], txToPaintMs: paintMs,
      },
    });
  }

  window.__digger = {
    receive(json: string) {
      const msg = JSON.parse(json) as { type: string; payload: any };
      const p = msg.payload;
      switch (msg.type) {
        case "load":
          baseRev = p.stateRef; rev = p.rev; editDebounceMs = p.editDebounceMs;
          applyDoc(p.doc);
          break;
        case "committed":
          baseRev = p.stateRef; rev = Math.max(rev, p.rev);
          acked();
          break;
        case "rejected":
          acked();
          break;
        case "official_advanced":
          // Never replace a document that has unsent edits: the pending edit goes stale and Rust merges it.
          if (p.rev > rev && isClean()) { applyDoc(p.doc); baseRev = p.stateRef; rev = p.rev; }
          break;
        case "rebase":
          inflight = false;
          if (editTimer === undefined && !dirty) {
            applyDoc(p.doc); baseRev = p.stateRef; rev = p.rev; hideConflict();
          } else if (dirty) sendEdit();
          break;
        case "conflict":
          inflight = false;
          showConflict(p.conflicts);
          break;
        case "selftest": void basicSelfTest(); break;
        case "st_perf": void perf(); break;
        case "st_inputrule": {
          // Real Markdown typing path: "##" + space at the start of block 1 triggers the editor's input rule.
          const before = view.state.doc.child(1).attrs.blockId;
          const start = blockEnd(0) + 2;
          view.dispatch(view.state.tr.insertText("##", start));
          const pos = start + 2;
          // Real typing has the caret here; some rules (Milkdown's heading rule) read the selection, not `pos`.
          const Sel = view.state.selection.constructor as any;
          view.dispatch(view.state.tr.setSelection(Sel.near(view.state.doc.resolve(pos))));
          const handled = !!view.someProp("handleTextInput", (f: any) => f(view, pos, pos, " "));
          const n = view.state.doc.child(1);
          send("selftest_report", { inputrule: { handled, before, after: n.attrs.blockId, type: n.type.name, level: n.attrs.level ?? null, text: n.textContent } });
          break;
        }
        case "st_type": view.dispatch(view.state.tr.insertText(p.text, blockEnd(p.block))); break;
        case "st_flush": flushEdit(); break;
        case "st_click": (document.getElementById("conflict-" + p.which) as HTMLElement).click(); break;
        case "st_report":
          send("selftest_report", { doc: getJSON(), rev, baseRev, clean: isClean(), conflictVisible: banner.style.display !== "none" });
          break;
      }
    },
  };

  send("ready", { editorSchemaVersion: 1, adapter, viewport: viewport() });
}
