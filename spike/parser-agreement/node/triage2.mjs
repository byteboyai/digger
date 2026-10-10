// Third opinions on the 35 typing-state disagreements: commonmark.js (no tables) and markdown-it (CommonMark + tables).
import fs from "node:fs";
import MarkdownIt from "markdown-it";
import { referenceBlocks } from "./reference.mjs";
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const md = new MarkdownIt("commonmark").enable("table");
const OPEN = { paragraph_open: "paragraph", heading_open: "heading", blockquote_open: "quote", bullet_list_open: "list", ordered_list_open: "list", table_open: "table" };
const SINGLE = { fence: "code", code_block: "code", hr: "rule", html_block: "html" };
function mditBlocks(text) {
  const out = [];
  for (const t of md.parse(text, {})) {
    if (t.level !== 0 || !t.map) continue;
    const c = OPEN[t.type] ?? SINGLE[t.type];
    if (c) out.push({ class: c, startLine: t.map[0], endLine: t.map[1] - 1 });
  }
  return out;
}
const REF = { heading: "heading", paragraph: "paragraph", list: "list", block_quote: "quote", code_block: "code", html_block: "html", thematic_break: "rule" };
const sig = (bs) => bs.map((b) => `${REF[b.class] ?? b.class}@${b.startLine}-${b.endLine}`).join(" ");
const S = read("../corpus/snippets.jsonl");
const R = new Map(read("../out/snippets.rust.jsonl").map((d) => [d.id, d.blocks]));
const L = new Map(read("../out/snippets.lezer.jsonl").map((d) => [d.id, d.blocks.filter((b) => b.class !== "linkref")]));
const tally = { rust: 0, lezer: 0, both: 0, neither: 0 };
const bySig = new Map();
for (const s of S) {
  const r = sig(R.get(s.id)), l = sig(L.get(s.id));
  const hasTable = R.get(s.id).some((b) => b.class === "table") || L.get(s.id).some((b) => b.class === "table");
  const who = hasTable ? "markdown-it" : "commonmark.js";
  const ref = sig(hasTable ? mditBlocks(s.text) : referenceBlocks(s.text));
  const v = r === ref && l === ref ? "both" : r === ref ? "rust" : l === ref ? "lezer" : "neither";
  tally[v]++;
  const k = `${r.slice(0, 60)} | ${l.slice(0, 60)} → ${v} (${who})`;
  bySig.set(k, (bySig.get(k) ?? 0) + 1);
}
console.log(`35 typing-state disagreements: reference agrees with Rust ${tally.rust}, with Lezer ${tally.lezer}, neither ${tally.neither}`);
for (const [k, n] of [...bySig.entries()].sort((a, b) => b[1] - a[1])) console.log(`  ${String(n).padStart(2)}x ${k}`);
