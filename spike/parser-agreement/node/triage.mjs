// For every document where Rust and Lezer disagree, ask commonmark.js (the reference) which one is right.
// The reference has no tables or footnotes, so documents containing those are reported as "no reference".
import fs from "node:fs";
import { referenceBlocks } from "./reference.mjs";
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const span = (b) => `${b.startLine}-${b.endLine}`;
const REF = { heading: "heading", paragraph: "paragraph", list: "list", block_quote: "quote", code_block: "code", html_block: "html", thematic_break: "rule" };
const spans = (bs) => bs.map((b) => `${REF[b.class] ?? b.class}@${span(b)}`).join(" ");
const out = [];
for (const name of process.argv.slice(2)) {
  const docs = new Map(read(`../corpus/${name}.jsonl`).map((d) => [d.id, d]));
  const rust = new Map(read(`../out/${name}.rust.jsonl`).map((d) => [d.id, d.blocks]));
  const lezer = new Map(read(`../out/${name}.lezer.jsonl`).map((d) => [d.id, d.blocks.filter((b) => b.class !== "linkref")]));
  let dis = 0, rustRight = 0, lezerRight = 0, neither = 0, noRef = 0;
  const lines = [];
  for (const [id, d] of docs) {
    const r = rust.get(id), l = lezer.get(id);
    const same = r.length === l.length && r.every((b, i) => b.class === l[i].class && span(b) === span(l[i]));
    if (same) continue;
    dis++;
    if (r.some((b) => b.class === "table" || b.class === "footnote") || l.some((b) => b.class === "table")) { noRef++; lines.push(`  ${id}: no reference (table or footnote)`); continue; }
    const ref = referenceBlocks(d.text);
    const rr = spans(ref) === spans(r), lr = spans(ref) === spans(l);
    if (rr && !lr) rustRight++; else if (lr && !rr) lezerRight++; else neither++;
    lines.push(`  ${id}: ${rr && !lr ? "Rust matches the reference" : lr && !rr ? "Lezer matches the reference" : "neither matches (or both)"}`);
  }
  console.log(`${name}: ${dis} disagreements → Rust right ${rustRight}, Lezer right ${lezerRight}, neither ${neither}, no reference ${noRef}`);
  for (const l of lines.slice(0, 8)) console.log(l.slice(0, 190));
}
