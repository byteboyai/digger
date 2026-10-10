import { markdownLanguage } from "@codemirror/lang-markdown";
import { Footnote } from "./footnote.mjs";

const CLASS = {
  Paragraph: "paragraph", Blockquote: "quote", BulletList: "list", OrderedList: "list", FencedCode: "code", CodeBlock: "code",
  FootnoteDefinition: "footnote", HTMLBlock: "html", CommentBlock: "html", ProcessingInstructionBlock: "html", HorizontalRule: "rule", Table: "table", LinkReference: "linkref",
};
export const cls = (name) => (/^(ATX|Setext)Heading/.test(name) ? "heading" : CLASS[name] ?? "other:" + name);
export const makeParser = (footnotes) => (footnotes ? markdownLanguage.parser.configure(Footnote) : markdownLanguage.parser);

export function lineStartsOf(text) {
  const ls = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") ls.push(i + 1);
  return ls;
}
export function blocksOf(tree, text) {
  const ls = lineStartsOf(text);
  const lineOf = (off) => { let lo = 0, hi = ls.length - 1; while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (ls[mid] <= off) lo = mid; else hi = mid - 1; } return lo; };
  const out = [];
  for (let n = tree.topNode.firstChild; n; n = n.nextSibling) {
    const end = n.from + text.slice(n.from, n.to).trimEnd().length;
    out.push({ class: cls(n.name), name: n.name, startLine: lineOf(n.from), endLine: lineOf(Math.max(n.from, end - 1)), start: n.from, end });
  }
  return out;
}
