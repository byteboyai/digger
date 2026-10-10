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
let loading = false;
// One edit in flight at a time: the next one is sent after Rust acks (committed/rejected),
// so baseRev never goes stale because of our own edits.
let inflight = false;
let dirty = false;

function send(type: string, payload: unknown) {
  const env = { hostKind: "editor", binding, seq: ++seq, type, payload };
  window.ipc.postMessage(JSON.stringify(env));
}

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
    sendEdit();
  },
  onSelectionUpdate({ editor }) {
    const { from, to } = editor.state.selection;
    send("selection", { from, to });
  },
});

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

let snapTimer: number | undefined;
function scheduleSnapshot() {
  clearTimeout(snapTimer);
  snapTimer = window.setTimeout(() => send("snapshot", { baseRev, doc: editor.getJSON() }), 500);
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

window.__digger = {
  receive(json: string) {
    const msg = JSON.parse(json) as { type: string; payload: any };
    if (msg.type === "load") {
      loading = true;
      baseRev = msg.payload.stateRef;
      editor.commands.setContent(msg.payload.doc, { emitUpdate: false });
      loading = false;
    } else if (msg.type === "committed") {
      baseRev = msg.payload.stateRef;
      acked();
    } else if (msg.type === "rejected") {
      // Spike policy: on rejection, drop the in-flight edit and wait for Rust to rebase/reload.
      acked();
    } else if (msg.type === "selftest") {
      void selfTest();
    }
  },
};

const viewport = () => ({ w: window.innerWidth, h: window.innerHeight });
window.addEventListener("resize", () => send("title", { viewport: viewport() }));
send("ready", { editorSchemaVersion: 1, viewport: viewport() });
