import { Editor, Extension } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { Plugin, PluginKey } from "@tiptap/pm/state";

declare global {
  interface Window {
    ipc: { postMessage(msg: string): void };
    __DIGGER_BINDING__: { projectId: string; panelId: string; documentId: string };
    __digger: { receive(json: string): void };
  }
}

const binding = window.__DIGGER_BINDING__;
let seq = 0;
let baseRev = "";
let rev = 0;
let loading = false;
let editDebounceMs = 1200;
// One edit in flight at a time. `dirty` = changed again while in flight; `editTimer` = debounced, unsent.
// "Clean" (nothing unsent, nothing in flight) is the only state in which the host may replace its document.
let inflight = false;
let dirty = false;
let editTimer: number | undefined;

function send(type: string, payload: unknown) {
  const env = { hostKind: "editor", binding, seq: ++seq, type, payload };
  window.ipc.postMessage(JSON.stringify(env));
}

const isClean = () => !inflight && !dirty && editTimer === undefined;

// Stable block identity: assign a fresh id to any top-level block that has none
// or whose id duplicates an earlier block (PM copies attrs when splitting).
// crypto.randomUUID needs a secure context; WKWebView `with_html` pages are not one (spike finding).
function newId(): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

const BlockId = Extension.create({
  name: "blockId",
  addGlobalAttributes() {
    return [{ types: ["paragraph", "heading"], attributes: { blockId: { default: null, keepOnSplit: false } } }];
  },
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey("blockIdAssign"),
        appendTransaction(trs, _old, state) {
          if (!trs.some((t) => t.docChanged)) return null;
          const seen = new Set<string>();
          const tr = state.tr;
          let changed = false;
          state.doc.forEach((node, offset) => {
            const id = node.attrs.blockId as string | null;
            if (!id || seen.has(id)) {
              tr.setNodeMarkup(offset, undefined, { ...node.attrs, blockId: newId() });
              changed = true;
            } else {
              seen.add(id);
            }
          });
          return changed ? tr : null;
        },
      }),
    ];
  },
});

const editor = new Editor({
  element: document.getElementById("editor")!,
  extensions: [StarterKit.configure({ heading: { levels: [1, 2] } }), BlockId],
  content: "",
  onUpdate({ editor }) {
    if (loading) return;
    scheduleSnapshot();
    scheduleEdit();
  },
  onSelectionUpdate({ editor }) {
    const { from, to } = editor.state.selection;
    send("selection", { from, to });
  },
});

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
  send("edit", { baseRev, doc: editor.getJSON() });
}
function acked() {
  inflight = false;
  if (dirty) sendEdit();
}

function applyDoc(doc: unknown) {
  const { from, to } = editor.state.selection;
  loading = true;
  editor.commands.setContent(doc as any, { emitUpdate: false });
  loading = false;
  const max = editor.state.doc.content.size;
  try { editor.commands.setTextSelection({ from: Math.min(from, max - 1), to: Math.min(to, max - 1) }); } catch {}
}

const banner = document.getElementById("conflict")!;
function showConflict(items: { blockId: string; mine: string; official: string }[]) {
  document.getElementById("conflict-text")!.textContent =
    `${items.length} 个块同时被修改：` + items.map((i) => `“${i.mine}” ↔ “${i.official}”`).join("；");
  banner.style.display = "block";
}
const hideConflict = () => { banner.style.display = "none"; };
document.getElementById("conflict-mine")!.addEventListener("click", () => send("resolve", { choice: "mine" }));
document.getElementById("conflict-official")!.addEventListener("click", () => send("resolve", { choice: "official" }));

let snapTimer: number | undefined;
function scheduleSnapshot() {
  clearTimeout(snapTimer);
  snapTimer = window.setTimeout(() => send("snapshot", { baseRev, doc: editor.getJSON() }), 300);
}

window.addEventListener("error", (e) => send("failed", { message: String(e.message), line: e.lineno }));
window.addEventListener("unhandledrejection", (e) => send("failed", { message: String((e as PromiseRejectionEvent).reason) }));

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// Spike-only scripted edits, so the round trip can be verified without a human.
async function selfTest() {
  editor.commands.focus("end");
  // 1. type into the first block
  editor.chain().focus().setTextSelection(editor.state.doc.firstChild!.nodeSize - 1).insertContent("（自测追加）").run();
  await sleep(120);
  // 2. split a paragraph: both halves must end up with distinct blockIds
  editor.chain().focus().setTextSelection(4).splitBlock().run();
  await sleep(120);
  // 3. delete the last block
  const last = editor.state.doc.lastChild!;
  const end = editor.state.doc.content.size;
  editor.chain().focus().deleteRange({ from: end - last.nodeSize, to: end }).run();
  await sleep(120);
  while (inflight || dirty) await sleep(20);
  await sleep(700); // let the debounced recovery snapshot fire
  send("selftest_done", {});
}

function blockEnd(index: number): number {
  let off = 0;
  for (let i = 0; i < editor.state.doc.childCount; i++) {
    const n = editor.state.doc.child(i);
    if (i === index) return off + n.nodeSize - 1;
    off += n.nodeSize;
  }
  throw new Error("no such block " + index);
}

window.__digger = {
  receive(json: string) {
    const msg = JSON.parse(json) as { type: string; payload: any };
    const p = msg.payload;
    switch (msg.type) {
      case "load":
        baseRev = p.stateRef;
        rev = p.rev;
        editDebounceMs = p.editDebounceMs;
        applyDoc(p.doc);
        break;
      case "committed":
        baseRev = p.stateRef;
        rev = Math.max(rev, p.rev);
        acked();
        break;
      case "rejected":
        acked();
        break;
      case "official_advanced":
        // Never replace a document that has unsent edits: the pending edit will go stale and Rust merges it.
        if (p.rev > rev && isClean()) {
          applyDoc(p.doc);
          baseRev = p.stateRef;
          rev = p.rev;
        }
        break;
      case "rebase":
        inflight = false;
        if (editTimer === undefined && !dirty) {
          applyDoc(p.doc);
          baseRev = p.stateRef;
          rev = p.rev;
          hideConflict();
        } else if (dirty) {
          sendEdit(); // typed after the answered edit: let Rust merge it against the original base
        }
        break;
      case "conflict":
        inflight = false;
        showConflict(p.conflicts);
        break;
      case "selftest":
        void selfTest();
        break;
      // ---- spike-only scripted commands ----
      case "st_type":
        editor.chain().insertContentAt(blockEnd(p.block), p.text).run();
        break;
      case "st_flush":
        flushEdit();
        break;
      case "st_click":
        (document.getElementById("conflict-" + p.which) as HTMLElement).click();
        break;
      case "st_report":
        send("selftest_report", {
          doc: editor.getJSON(), rev, baseRev, clean: isClean(),
          conflictVisible: banner.style.display !== "none",
        });
        break;
    }
  },
};

const viewport = () => ({ w: window.innerWidth, h: window.innerHeight });
window.addEventListener("resize", () => send("title", { viewport: viewport() }));
send("ready", { editorSchemaVersion: 1, viewport: viewport() });
