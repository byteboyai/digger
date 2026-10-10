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

// Indices of one longest strictly increasing subsequence of `a`, O(n log n).
function lis(a: number[]): number[] {
  const tails: number[] = []; // tails[l] = index in `a` of the smallest tail of an increasing run of length l+1
  const prev: number[] = new Array(a.length).fill(-1);
  for (let i = 0; i < a.length; i++) {
    let lo = 0, hi = tails.length;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (a[tails[mid]] < a[i]) lo = mid + 1; else hi = mid; }
    if (lo > 0) prev[i] = tails[lo - 1];
    tails[lo] = i;
  }
  const out: number[] = [];
  for (let i = tails.length ? tails[tails.length - 1] : -1; i >= 0; i = prev[i]) out.push(i);
  return out;
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

  // ---- block-scoped edits -------------------------------------------------------------------------------
  // `baseline` = the blocks Rust last confirmed (== Rust's history[baseRev]). An edit/snapshot carries only the
  // blocks that differ from it (ProseMirror nodes are immutable and structurally shared, so "unchanged" is a
  // pointer comparison), the ids that disappeared, and the full id order only when the block set/order changed.
  type Blocks = { ids: string[]; nodes: Map<string, any> };
  const blocksNow = (): Blocks => {
    const ids: string[] = [];
    const nodes = new Map<string, any>();
    view.state.doc.forEach((n: any) => { ids.push(n.attrs.blockId); nodes.set(n.attrs.blockId, n); });
    return { ids, nodes };
  };
  let baseline: Blocks = { ids: [], nodes: new Map() };
  function buildDelta(cur: Blocks, base: Blocks) {
    const changed: unknown[] = [];
    for (const id of cur.ids) {
      const n = cur.nodes.get(id);
      const o = base.nodes.get(id);
      if (!o || (o !== n && !o.eq(n))) changed.push(n.toJSON());
    }
    const removed = base.ids.filter((id) => !cur.nodes.has(id));
    // Order is expressed by anchors: only blocks whose predecessor differs from the baseline's
    // (minus removed blocks) are listed. In-place edits send none; an insert/move sends one or two per site.
    const keptPred = new Map<string, string | null>();
    let prev: string | null = null;
    for (const id of base.ids) {
      if (!cur.nodes.has(id)) continue;
      keptPred.set(id, prev);
      prev = id;
    }
    const placements: { id: string; after: string | null }[] = [];
    prev = null;
    for (const id of cur.ids) {
      if (!keptPred.has(id) || keptPred.get(id) !== prev) placements.push({ id, after: prev });
      prev = id;
    }
    return { changed, removed, ...(placements.length ? { placements } : {}) };
  }
  let sent: Blocks | undefined;

  const send = (type: string, payload: unknown): number => {
    const mine = ++seq;
    window.ipc.postMessage(JSON.stringify({ hostKind: "editor", binding, seq: mine, type, payload }));
    return mine;
  };
  let lastSentSeq = 0; // seq of the in-flight/last edit: a `rebase` is only valid for the edit it answers
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
    sent = blocksNow();
    lastSentSeq = send("edit", { baseRev, ...buildDelta(sent, baseline) });
  }
  function acked() {
    inflight = false;
    if (dirty) sendEdit();
  }
  function scheduleSnapshot() {
    clearTimeout(snapTimer);
    snapTimer = window.setTimeout(() => send("snapshot", { baseRev, ...buildDelta(blocksNow(), baseline) }), 300);
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
    baseline = blocksNow(); // the document now equals what Rust sent
    lastLoadMs = performance.now() - t0;
  }

  // Block-scoped downstream: apply Rust's delta to the live document in place. Untouched blocks keep their
  // node identity, so the caret, selection and IME composition outside the changed blocks are undisturbed.
  // Throws before dispatching anything if the delta is inconsistent (the caller then asks for a resync).
  let lastApplyMs = 0;
  function applyDelta(d: { changed: any[]; removed: string[]; placements?: { id: string; after: string | null }[] }) {
    const t0 = performance.now();
    const schema = view.state.schema;
    const tr = view.state.tr;
    type Ent = { id: string; node: any };
    const cur: Ent[] = [];
    view.state.doc.forEach((n: any) => cur.push({ id: n.attrs.blockId, node: n }));
    const changed = new Map<string, any>();
    for (const j of d.changed) changed.set(j.attrs.blockId, schema.nodeFromJSON(j));
    const removed = new Set(d.removed);
    const startsOf = () => {
      const out: number[] = [];
      let p = 0;
      for (const e of cur) { out.push(p); p += e.node.nodeSize; }
      return out;
    };
    // 1. removals and 2. in-place replacements, back to front so earlier positions stay valid.
    let starts = startsOf();
    for (let i = cur.length - 1; i >= 0; i--) {
      if (removed.has(cur[i].id)) { tr.delete(starts[i], starts[i] + cur[i].node.nodeSize); cur.splice(i, 1); }
    }
    starts = startsOf();
    for (let i = cur.length - 1; i >= 0; i--) {
      const n = changed.get(cur[i].id);
      if (n) { tr.replaceWith(starts[i], starts[i] + cur[i].node.nodeSize, n); cur[i].node = n; }
    }
    // 3. order: predecessor map (baseline minus removed, overridden by placements) → one chain.
    const pred = new Map<string, string | null>();
    let prev: string | null = null;
    for (const e of cur) { pred.set(e.id, prev); prev = e.id; }
    for (const pl of d.placements ?? []) pred.set(pl.id, pl.after);
    const next = new Map<string | null, string>();
    for (const [id, p] of pred) {
      if (next.has(p)) throw new Error("two blocks claim one predecessor");
      next.set(p, id);
    }
    const target: string[] = [];
    for (let c: string | null = null; next.has(c); ) {
      const id: string = next.get(c)!;
      target.push(id);
      c = id;
      if (target.length > pred.size) throw new Error("cycle in placements");
    }
    if (target.length !== pred.size) throw new Error("placements do not form one chain");
    // Move the fewest blocks: the longest increasing subsequence of the surviving blocks (by target position)
    // stays where it is; every other surviving block is lifted out and re-inserted, new blocks are inserted.
    const targetIdx = new Map<string, number>();
    target.forEach((id, i) => targetIdx.set(id, i));
    const seqIdx = cur.map((e) => targetIdx.get(e.id));
    if (seqIdx.some((x) => x === undefined)) throw new Error("block missing from target order");
    const keep = new Set<number>(lis(seqIdx as number[]));
    const lifted = new Map<string, any>();
    starts = startsOf();
    for (let i = cur.length - 1; i >= 0; i--) {
      if (keep.has(i)) continue;
      tr.delete(starts[i], starts[i] + cur[i].node.nodeSize);
      lifted.set(cur[i].id, cur[i].node);
      cur.splice(i, 1);
    }
    // Walk the target order left to right; everything before `pos` is already final, so no rescanning.
    let pos = 0;
    let k = 0; // index into the kept blocks
    for (const id of target) {
      if (k < cur.length && cur[k].id === id) { pos += cur[k].node.nodeSize; k++; continue; }
      const node = lifted.get(id) ?? changed.get(id);
      if (!node) throw new Error("unknown block " + id);
      tr.insert(pos, node);
      pos += node.nodeSize;
    }
    hooks.loading = true;
    try { view.dispatch(tr.setMeta("addToHistory", false)); } finally { hooks.loading = false; }
    baseline = blocksNow();
    lastApplyMs = performance.now() - t0;
  }
  // Apply a downstream delta or ask Rust for the whole document. Returns true if applied.
  function applyOrResync(d: any): boolean {
    try { applyDelta(d); return true; } catch (e) {
      send("resync", { haveRev: rev, reason: String(e) });
      return false;
    }
  }
  let marked: Map<string, any> | undefined; // self-test: node identity probe

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
    const dt = performance.now();
    const delta = buildDelta(blocksNow(), baseline);
    const deltaMs = performance.now() - dt;
    const deltaBytes = JSON.stringify(delta).length;
    // structural edit: split a block in the middle → needs the id order
    view.dispatch(view.state.tr.split(pos));
    const st = buildDelta(blocksNow(), baseline);
    const structuralBytes = JSON.stringify(st).length;
    txMs.sort((x, y) => x - y);
    send("selftest_report", {
      perf: {
        adapter, blocks: view.state.doc.childCount, loadMs: lastLoadMs, getJsonMs, jsonBytes,
        txMeanMs: txMs.reduce((x, y) => x + y, 0) / N, txP95Ms: txMs[Math.floor(N * 0.95)], txToPaintMs: paintMs, deltaMs, deltaBytes, structuralBytes,
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
          if (sent) baseline = sent; // Rust now holds exactly what we sent
          sent = undefined;
          acked();
          break;
        case "rejected":
          acked();
          break;
        case "official_advanced":
          // Never touch a document that has unsent edits: the pending edit goes stale and Rust merges it.
          if (p.rev <= rev || !isClean()) break;
          if (p.fromRev !== rev) { send("resync", { haveRev: rev, reason: "fromRev mismatch" }); break; }
          if (applyOrResync(p.delta)) { baseRev = p.stateRef; rev = p.rev; }
          break;
        case "rebase":
          inflight = false;
          if (editTimer === undefined && !dirty) {
            if (p.ackSeq !== lastSentSeq) { send("resync", { haveRev: rev, reason: "ackSeq mismatch" }); break; }
            if (applyOrResync(p.delta)) { baseRev = p.stateRef; rev = p.rev; }
          } else if (dirty) sendEdit(); // typed after the answered edit: let Rust merge it against the original base
          break;
        case "conflict":
          // The choice is made in the Rust-owned (iced) conflict bar, never in the page. The host keeps its text
          // and just stops waiting for an answer; the eventual `rebase` arrives after the user decides.
          inflight = false;
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
        case "st_mark": marked = new Map(blocksNow().nodes); break;
        case "st_forge_resolve": send("resolve", { choice: "mine" }); break; // a compromised page trying to decide
        case "st_report":
          const cur = blocksNow().nodes;
          const identity: Record<string, boolean> = {};
          marked?.forEach((n, id) => { if (cur.has(id)) identity[id] = cur.get(id) === n; });
          send("selftest_report", { doc: getJSON(), rev, baseRev, clean: isClean(), hasFocus: document.hasFocus(), identity, lastApplyMs });
          break;
      }
    },
  };

  send("ready", { editorSchemaVersion: 1, adapter, viewport: viewport() });
}
