// ProseMirror with source-preserving serialization: every top-level block remembers the Markdown text it
// was parsed from. On save, a block whose normalized form is unchanged is emitted VERBATIM (original text and
// original gap); only changed blocks go through the normalizing serializer. Constructs outside the structural
// schema (tables, HTML) are kept as raw source blocks.
import { defaultMarkdownParser, defaultMarkdownSerializer, schema as mdSchema } from "prosemirror-markdown";
import { Node as PMNode, Schema, type NodeSpec } from "prosemirror-model";
import { EditorState, Plugin, Selection } from "prosemirror-state";
import { Decoration, DecorationSet, EditorView } from "prosemirror-view";
import { history } from "prosemirror-history";
import type { Engine } from "./engine";

const srcAttrs = { src: { default: "" }, norm: { default: "" }, gap: { default: "\n\n" }, from: { default: -1 }, to: { default: -1 } };
const withSrc = (spec: NodeSpec): NodeSpec => ({ ...spec, attrs: { ...(spec.attrs ?? {}), ...srcAttrs } });

const nodes = mdSchema.spec.nodes
  .update("paragraph", withSrc(mdSchema.spec.nodes.get("paragraph")!))
  .update("heading", withSrc(mdSchema.spec.nodes.get("heading")!))
  .update("blockquote", withSrc(mdSchema.spec.nodes.get("blockquote")!))
  .update("bullet_list", withSrc(mdSchema.spec.nodes.get("bullet_list")!))
  .update("ordered_list", withSrc(mdSchema.spec.nodes.get("ordered_list")!))
  .update("code_block", withSrc(mdSchema.spec.nodes.get("code_block")!))
  .update("horizontal_rule", withSrc(mdSchema.spec.nodes.get("horizontal_rule")!))
  .addToEnd("raw", withSrc({ group: "block", content: "text*", marks: "", code: true, defining: true, parseDOM: [{ tag: "pre.raw", preserveWhitespace: "full" }], toDOM: () => ["pre", { class: "raw" }, 0] }));
const schema = new Schema({ nodes, marks: mdSchema.spec.marks });

const STRUCTURAL = new Set(["paragraph_open", "heading_open", "blockquote_open", "bullet_list_open", "ordered_list_open", "fence", "code_block", "hr"]);
const RAW = new Set(["table_open", "html_block"]);

function ser(node: PMNode): string {
  const doc = schema.topNodeType.create(null, node);
  return defaultMarkdownSerializer.serialize(doc as never).replace(/\n+$/, "");
}

interface Parsed { leading: string; blocks: PMNode[] }

function parse(text: string): Parsed {
  const md = defaultMarkdownParser.tokenizer;
  md.enable("table");
  const lineStarts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") lineStarts.push(i + 1);
  lineStarts.push(text.length + 1);
  const off = (line: number) => Math.min(lineStarts[line] ?? text.length, text.length);
  const tokens = md.parse(text, {});
  const spans: { start: number; end: number; raw: boolean }[] = [];
  for (const t of tokens) {
    if (t.level !== 0 || !t.map) continue;
    if (STRUCTURAL.has(t.type) || RAW.has(t.type)) spans.push({ start: off(t.map[0]), end: off(t.map[1]), raw: RAW.has(t.type) });
  }
  const blocks: PMNode[] = [];
  const first = spans.length ? spans[0].start : text.length;
  for (let i = 0; i < spans.length; i++) {
    const s = spans[i];
    let end = s.end;
    while (end > s.start && text[end - 1] === "\n") end--;
    const src = text.slice(s.start, end);
    const nextStart = i + 1 < spans.length ? spans[i + 1].start : text.length;
    const gap = text.slice(end, nextStart);
    let node: PMNode;
    if (s.raw) {
      node = schema.nodes.raw.create({ src, norm: src, gap, from: s.start, to: end }, src ? schema.text(src) : undefined);
    } else {
      const d = defaultMarkdownParser.parse(src) as unknown as PMNode;
      const child = d.firstChild!;
      const bare = schema.nodeFromJSON(child.toJSON());
      node = bare.type.create({ ...bare.attrs, src, gap, from: s.start, to: end }, bare.content, bare.marks);
      node = node.type.create({ ...node.attrs, norm: ser(node) }, node.content);
    }
    blocks.push(node);
  }
  return { leading: text.slice(0, first), blocks };
}

function reveal(): Plugin {
  return new Plugin({
    props: {
      decorations(state) {
        const { $from, $to } = state.selection;
        if ($from.depth < 1) return null;
        const start = $from.before(1);
        const block = state.doc.nodeAt(start)!;
        const decos: Decoration[] = [];
        const w = (pos: number, s: string) => decos.push(Decoration.widget(pos, () => { const e = document.createElement("span"); e.className = "syn"; e.textContent = s; return e; }, { side: -1, key: s + pos }));
        if (block.type.name === "heading") w(start + 1, "#".repeat(block.attrs.level) + " ");
        if (block.type.name === "blockquote") w(start + 2, "> ");
        // inline marks: show their delimiters when the selection is inside the marked span
        const inline = (tb: PMNode, basePos: number) => {
          const open: Record<string, number> = {};
          const close = (name: string, end: number) => {
            const from = open[name]; delete open[name];
            if ($from.pos < from || $to.pos > end) return;
            const d = ({ strong: "**", em: "*", code: "`" } as Record<string, string>)[name];
            if (!d) return;
            w(from, d);
            decos.push(Decoration.widget(end, () => { const e = document.createElement("span"); e.className = "syn"; e.textContent = d; return e; }, { side: 1, key: "e" + d + end }));
          };
          let pos = basePos + 1;
          tb.forEach((child) => {
            const names = child.marks.map((m) => m.type.name);
            for (const k of Object.keys(open)) if (!names.includes(k)) close(k, pos);
            for (const nm of names) if (!(nm in open)) open[nm] = pos;
            pos += child.nodeSize;
          });
          for (const k of Object.keys(open)) close(k, pos);
        };
        // The top-level block may itself be the textblock (paragraph, heading); lists and quotes nest them.
        if (block.isTextblock) inline(block, start);
        else block.descendants((n, p) => { if (n.isTextblock) { inline(n, start + 1 + p); return false; } return true; });
        return DecorationSet.create(state.doc, decos);
      },
    },
  });
}

export function createPm(): Engine {
  let view: EditorView;
  let leading = "";
  let loadedBlocks: { from: number; to: number }[] = [];
  const findText = (marker: string): number => {
    let found = -1;
    view.state.doc.descendants((n, pos) => {
      if (found >= 0) return false;
      if (n.isText && n.text) { const i = n.text.indexOf(marker); if (i >= 0) found = pos + i; }
      return true;
    });
    if (found < 0) throw new Error("marker not found: " + marker);
    return found;
  };
  return {
    name: "prosemirror-source-preserving",
    mount(el) {
      view = new EditorView(el, { state: EditorState.create({ schema, plugins: [history(), reveal()] }) });
      view.dom.style.cssText = "outline:none;min-height:100%";
    },
    load(md) {
      const { leading: lead, blocks } = parse(md);
      leading = lead;
      loadedBlocks = blocks.map((b) => ({ from: b.attrs.from, to: b.attrs.to }));
      const doc = schema.topNodeType.create(null, blocks);
      view.updateState(EditorState.create({ schema, doc, plugins: [history(), reveal()] }));
    },
    getMarkdown() {
      const parts: string[] = [leading];
      const kids: PMNode[] = [];
      view.state.doc.forEach((n) => kids.push(n));
      kids.forEach((n, i) => {
        let text: string;
        if (n.type.name === "raw") text = n.textContent;
        else {
          const unchanged = n.attrs.src !== "" && ser(n.type.create({ ...n.attrs }, n.content, n.marks)) === n.attrs.norm;
          text = unchanged ? n.attrs.src : ser(n);
        }
        parts.push(text, i === kids.length - 1 ? (n.attrs.gap || "\n") : n.attrs.gap);
      });
      return parts.join("");
    },
    blockRanges: () => loadedBlocks,
    insertAfter(m, text) { view.dispatch(view.state.tr.insertText(text, findText(m) + m.length)); },
    caretAfter(m) { view.dispatch(view.state.tr.setSelection(Selection.near(view.state.doc.resolve(findText(m) + m.length)))); },
    caretAtStart() { view.dispatch(view.state.tr.setSelection(Selection.atStart(view.state.doc))); },
    visibleTextOf(m) {
      const pos = findText(m);
      const start = view.state.doc.resolve(pos).before(1);
      const dom = view.nodeDOM(start) as HTMLElement;
      return dom.innerText;
    },
    imgCount: () => view.dom.querySelectorAll("img:not(.ProseMirror-separator)").length,
    moveBlock(from, to) {
      const kids: { node: PMNode; pos: number }[] = [];
      view.state.doc.forEach((node, pos) => kids.push({ node, pos }));
      const a = kids[from];
      const at = to < kids.length ? kids[to].pos : view.state.doc.content.size;
      const tr = view.state.tr;
      if (at <= a.pos) { tr.delete(a.pos, a.pos + a.node.nodeSize); tr.insert(at, a.node); }
      else { tr.insert(at, a.node); tr.delete(a.pos, a.pos + a.node.nodeSize); }
      view.dispatch(tr);
    },
    typeChar(ch) { view.dispatch(view.state.tr.insertText(ch)); },
    sourceOffsetOfCaret() {
      const { $from } = view.state.selection;
      if ($from.depth < 1) return null;
      const block = $from.node(1);
      if (block.attrs.from < 0) return null;
      const before = view.state.doc.textBetween($from.start(1), $from.pos, "\n", "\n");
      const plain = block.textContent.length ? before : "";
      // Greedy alignment of the plain text with the block's source: syntax characters are skipped.
      const src: string = block.attrs.src;
      let q = 0, i = 0;
      while (i < src.length && q < plain.length) { if (src[i] === plain[q]) q++; i++; }
      return block.attrs.from + i;
    },
  };
}
