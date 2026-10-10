import { Annotation, EditorState, StateEffect, Transaction, type Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { history, undo } from "@codemirror/commands";
import type { Engine, RustApi } from "./engine";
import { RustSync, fnv } from "./rust-sync";
import { Footnote } from "./footnote";

class ImageWidget extends WidgetType {
  constructor(readonly src: string, readonly alt: string) { super(); }
  eq(o: ImageWidget) { return o.src === this.src && o.alt === this.alt; }
  toDOM() {
    const i = document.createElement("img");
    i.src = this.src; i.alt = this.alt; i.style.maxWidth = "100%"; i.style.verticalAlign = "middle";
    return i;
  }
}
class HrWidget extends WidgetType {
  toDOM() { const h = document.createElement("hr"); return h; }
}

const refresh = StateEffect.define<null>();

/** marks a transaction that applies a change which came from Rust (an Agent patch): it must not be sent back */
const fromRust = Annotation.define<boolean>();

/** When set, the "block that holds the caret" comes from Rust, not from the editor's own tree. */
let rustTops: () => { from: number; to: number }[] | null = () => null;
const hide = Decoration.replace({});

/** Live preview: syntax is hidden except in the top-level block that holds the selection. */
function build(view: EditorView): DecorationSet {
  const { state } = view;
  const tree = syntaxTree(state);
  const active: { from: number; to: number }[] = [];
  const tops = rustTops() ?? (() => { const t: { from: number; to: number }[] = []; for (let n = tree.topNode.firstChild; n; n = n.nextSibling) t.push({ from: n.from, to: n.to }); return t; })();
  for (const n of tops) {
    if (state.selection.ranges.some((r) => r.from <= n.to && r.to >= n.from)) active.push({ from: n.from, to: n.to });
  }
  const isActive = (from: number, to: number) => active.some((a) => from >= a.from && to <= a.to);
  const out: Range<Decoration>[] = [];
  const doc = state.doc;
  const hideRange = (from: number, to: number) => { if (to > from) out.push(hide.range(from, to)); };
  for (const vr of view.visibleRanges) {
    tree.iterate({
      from: vr.from, to: vr.to,
      enter: (node) => {
        const name = node.name;
        const h = /^ATXHeading(\d)$/.exec(name);
        if (h) out.push(Decoration.line({ class: `cm-h${h[1]}` }).range(doc.lineAt(node.from).from));
        if (name === "SetextHeading1" || name === "SetextHeading2") {
          out.push(Decoration.line({ class: "cm-h1" }).range(doc.lineAt(node.from).from));
        }
        if (name === "Emphasis") out.push(Decoration.mark({ class: "cm-em" }).range(node.from, node.to));
        if (name === "StrongEmphasis") out.push(Decoration.mark({ class: "cm-strong" }).range(node.from, node.to));
        if (name === "InlineCode") out.push(Decoration.mark({ class: "cm-icode" }).range(node.from, node.to));
        if (name === "FootnoteDefinition") {
          for (let p = doc.lineAt(node.from).number; p <= doc.lineAt(node.to).number; p++) {
            out.push(Decoration.line({ class: "cm-footnote" }).range(doc.line(p).from));
          }
        }
        if (name === "Blockquote") {
          for (let p = doc.lineAt(node.from).number; p <= doc.lineAt(node.to).number; p++) {
            out.push(Decoration.line({ class: "cm-quote" }).range(doc.line(p).from));
          }
        }
        const act = isActive(node.from, node.to);
        if (act) return;
        if (name === "HeaderMark") {
          const after = doc.sliceString(node.to, node.to + 1) === " " ? 1 : 0;
          if (doc.lineAt(node.from).from === node.from) hideRange(node.from, node.to + after);
          else hideRange(node.from, node.to);
        } else if (name === "EmphasisMark" || name === "CodeMark" && node.matchContext(["InlineCode"]) || name === "StrikethroughMark") {
          hideRange(node.from, node.to);
        } else if (name === "QuoteMark") {
          const after = doc.sliceString(node.to, node.to + 1) === " " ? 1 : 0;
          hideRange(node.from, node.to + after);
        } else if (name === "Link") {
          // [text](url "title"): hide the brackets and the destination, keep the text.
          const kids: { name: string; from: number; to: number }[] = [];
          for (let c = node.node.firstChild; c; c = c.nextSibling) kids.push({ name: c.name, from: c.from, to: c.to });
          const marks = kids.filter((k) => k.name === "LinkMark");
          if (marks.length >= 2) {
            hideRange(marks[0].from, marks[0].to);
            hideRange(marks[1].from, node.to);
            out.push(Decoration.mark({ class: "cm-link" }).range(marks[0].to, marks[1].from));
          }
        } else if (name === "Image") {
          const url = node.node.getChild("URL");
          const marks = node.node.getChildren("LinkMark");
          if (url && marks.length >= 2) {
            const alt = doc.sliceString(marks[0].to, marks[1].from);
            out.push(Decoration.replace({ widget: new ImageWidget(doc.sliceString(url.from, url.to), alt) }).range(node.from, node.to));
          }
          return false;
        } else if (name === "HorizontalRule") {
          out.push(Decoration.replace({ widget: new HrWidget() }).range(node.from, node.to));
        }
      },
    });
  }
  return Decoration.set(out, true);
}

const livePreview = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) { this.decorations = build(view); }
    update(u: ViewUpdate) {
      const forced = u.transactions.some((t) => t.effects.some((e) => e.is(refresh)));
      if (u.view.composing) {
        // Never rebuild structure under an IME composition; keep the decorations mapped and rebuild afterwards.
        if (u.docChanged) this.decorations = this.decorations.map(u.changes);
        return;
      }
      if (u.docChanged || u.selectionSet || u.viewportChanged || forced) this.decorations = build(u.view);
    }
  },
  {
    decorations: (v) => v.decorations,
    provide: (p) => EditorView.atomicRanges.of((view) => view.plugin(p)?.decorations ?? Decoration.none),
    eventHandlers: {
      compositionend(_e, view) { setTimeout(() => view.dispatch({ effects: refresh.of(null) }), 0); },
    },
  },
);

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "17px" },
  ".cm-scroller": { fontFamily: '-apple-system, "PingFang SC", sans-serif', lineHeight: "1.8", padding: "24px 40px" },
  ".cm-content": { maxWidth: "720px", margin: "0 auto" },
  ".cm-h1": { fontSize: "1.6em", fontWeight: "700" },
  ".cm-h2": { fontSize: "1.35em", fontWeight: "700" },
  ".cm-h3": { fontSize: "1.15em", fontWeight: "700" },
  ".cm-em": { fontStyle: "italic" },
  ".cm-strong": { fontWeight: "700" },
  ".cm-icode": { fontFamily: "ui-monospace, monospace", background: "#eee" },
  ".cm-link": { color: "#2a62c9", textDecoration: "underline" },
  ".cm-footnote": { fontSize: "0.9em", color: "#555", borderLeft: "2px solid #ddd", paddingLeft: "10px" },
  ".cm-quote": { borderLeft: "3px solid #bbb", paddingLeft: "10px", color: "#555" },
});

export function createCm(post: (o: unknown) => void = () => {}): Engine {
  let view: EditorView;
  let sync: RustSync;
  const rust: RustApi = {
    async load(text) {
      sync.loading = true;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: { anchor: 0 } });
      sync.loading = false;
      rustTops = () => (sync.active ? sync.blocks : null);
      await sync.load(text);
    },
    settled: () => sync.settled(),
    blocks: () => sync.blocks,
    version: () => sync.version,
    rustHash: () => sync.hash,
    editorHash: () => fnv(view.state.doc.toString()),
    replace(from, to, insert) { view.dispatch({ changes: { from, to, insert } }); },
    caret(pos) { view.dispatch({ selection: { anchor: pos } }); },
    doc: () => view.state.doc.toString(),
    typed() { const r = view.state.selection.main; return sync.typed(r.from, r.to); },
    activeBlockFrom() { const b = sync.blockAt(view.state.selection.main.head); return b ? b.from : null; },
    rejects: () => sync.rejects,
    docLength16: () => sync.len16,
    agent(op) { post({ t: "agent", ...op }); },
    agentErrors: () => sync.agentErrors,
    undo() { undo(view); },
    stats: () => ({ remoteApplied: sync.remoteApplied, remoteTransformed: sync.remoteTransformed }),
    detach() { sync.active = false; rustTops = () => null; view.dispatch({ effects: refresh.of(null) }); },
  };
  const blocks = (s: EditorState) => {
    const out: { from: number; to: number }[] = [];
    for (let n = syntaxTree(s).topNode.firstChild; n; n = n.nextSibling) out.push({ from: n.from, to: n.to });
    return out;
  };
  const posAfter = (m: string) => {
    const i = view.state.doc.toString().indexOf(m);
    if (i < 0) throw new Error("marker not found: " + m);
    return i + m.length;
  };
  let loaded: { from: number; to: number }[] = [];
  return {
    name: "codemirror6-live-preview",
    rust,
    mount(el) {
      view = new EditorView({
        parent: el,
        state: EditorState.create({
          doc: "",
          extensions: [
            history(),
            markdown({ base: markdownLanguage, extensions: [Footnote] }),
            EditorView.lineWrapping,
            livePreview,
            theme,
            EditorView.updateListener.of((u) => { if (u.docChanged && !u.transactions.some((t) => t.annotation(fromRust))) sync.local(u.changes); }),
          ],
        }),
      });
      // not inside an update (this is called from the update listener): rebuild the live preview right after it
      sync = new RustSync(
        post,
        () => queueMicrotask(() => view.dispatch({ effects: refresh.of(null) })),
        // The Agent's change goes into the author's undo history like any edit: one Ctrl-Z takes it back.
        (changes) => view.dispatch({ changes, annotations: [fromRust.of(true), Transaction.userEvent.of("agent")] }),
      );
    },
    load(md) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: md }, selection: { anchor: 0 } });
      // Make sure the parser has finished before blocks are read (large documents parse incrementally).
      syntaxTree(view.state);
      loaded = blocks(view.state);
    },
    getMarkdown: () => view.state.doc.toString(),
    blockRanges: () => loaded,
    blockKinds() {
      syntaxTree(view.state);
      const out: { name: string; startLine: number; endLine: number }[] = [];
      const doc = view.state.doc;
      for (let n = syntaxTree(view.state).topNode.firstChild; n; n = n.nextSibling) {
        const end = n.from + doc.sliceString(n.from, n.to).trimEnd().length;
        out.push({ name: n.name, startLine: doc.lineAt(n.from).number - 1, endLine: doc.lineAt(Math.max(n.from, end - 1)).number - 1 });
      }
      return out;
    },
    insertAfter(m, text) { view.dispatch({ changes: { from: posAfter(m), insert: text } }); },
    caretAfter(m) { view.dispatch({ selection: { anchor: posAfter(m) } }); },
    caretAtStart() { view.dispatch({ selection: { anchor: 0 } }); },
    visibleTextOf(m) {
      const pos = posAfter(m) - 1;
      const b = blocks(view.state).find((x) => pos >= x.from && pos <= x.to)!;
      const lines: string[] = [];
      view.contentDOM.querySelectorAll<HTMLElement>(".cm-line").forEach((el) => {
        const p = view.posAtDOM(el);
        if (p >= b.from && p <= b.to) lines.push(el.innerText);
      });
      return lines.join("\n");
    },
    // CodeMirror inserts <img class="cm-widgetBuffer"> spacers around widgets; they are not content.
    imgCount: () => view.contentDOM.querySelectorAll("img:not(.cm-widgetBuffer)").length,
    moveBlock(from, to) {
      const bs = blocks(view.state);
      const gapEnd = (i: number) => (i + 1 < bs.length ? bs[i + 1].from : view.state.doc.length);
      const unit = { from: bs[from].from, to: gapEnd(from) };
      const text = view.state.doc.sliceString(unit.from, unit.to);
      const at = to < bs.length ? bs[to].from : view.state.doc.length;
      view.dispatch({ changes: [{ from: unit.from, to: unit.to }, { from: at, insert: text }] });
    },
    typeChar(ch) { view.dispatch(view.state.replaceSelection(ch)); },
    sourceOffsetOfCaret: () => view.state.selection.main.head,
  };
}
