// How many of the 652 CommonMark examples does each parser split into the same top-level blocks as commonmark.js?
import fs from "node:fs";
import MarkdownIt from "markdown-it";
import { referenceBlocks } from "./reference.mjs";
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const docs = read("../corpus/commonmark-spec.jsonl");
const REF = { heading: "heading", paragraph: "paragraph", list: "list", block_quote: "quote", code_block: "code", html_block: "html", thematic_break: "rule" };
const sig = (bs) => bs.filter((b) => b.class !== "linkref").map((b) => `${REF[b.class] ?? b.class}@${b.startLine}-${b.endLine}`).join(" ");
const md = new MarkdownIt("commonmark");
const OPEN = { paragraph_open: "paragraph", heading_open: "heading", blockquote_open: "quote", bullet_list_open: "list", ordered_list_open: "list" };
const SINGLE = { fence: "code", code_block: "code", hr: "rule", html_block: "html" };
const mdit = (text) => md.parse(text, {}).filter((t) => t.level === 0 && t.map && (OPEN[t.type] || SINGLE[t.type])).map((t) => ({ class: OPEN[t.type] ?? SINGLE[t.type], startLine: t.map[0], endLine: t.map[1] - 1 }));
const get = (name) => new Map(read(`../out/commonmark-spec.${name}.jsonl`).map((d) => [d.id, d.blocks]));
const parsers = { "pulldown-cmark (Rust)": get("rust"), "comrak (Rust)": get("comrak"), "Lezer (CodeMirror, with footnotes)": get("lezer") };
const scores = Object.fromEntries([...Object.keys(parsers), "markdown-it (JS)"].map((k) => [k, 0]));
for (const d of docs) {
  const ref = sig(referenceBlocks(d.text));
  for (const [k, m] of Object.entries(parsers)) if (sig(m.get(d.id)) === ref) scores[k]++;
  if (sig(mdit(d.text)) === ref) scores["markdown-it (JS)"]++;
}
console.log(`top-level block structure equal to commonmark.js on the ${docs.length} CommonMark 0.31.2 examples:`);
for (const [k, v] of Object.entries(scores).sort((a, b) => b[1] - a[1])) console.log(`  ${String(v).padStart(3)}/${docs.length}  ${(100 * v / docs.length).toFixed(1)}%  ${k}`);
