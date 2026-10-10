// Cuts the region around every disagreement out of its document (top-level blocks are independent), so the
// reference implementation can judge documents that also contain tables or footnotes elsewhere.
import fs from "node:fs";
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const span = (b) => `${b.startLine}-${b.endLine}`;
const key = (b) => `${b.class}@${span(b)}`;
const out = [];
for (const name of process.argv.slice(2)) {
  const docs = new Map(read(`../corpus/${name}.jsonl`).map((d) => [d.id, d]));
  const rust = new Map(read(`../out/${name}.rust.jsonl`).map((d) => [d.id, d.blocks]));
  const lezer = new Map(read(`../out/${name}.lezer.jsonl`).map((d) => [d.id, d.blocks.filter((b) => b.class !== "linkref")]));
  for (const [id, d] of docs) {
    const r = rust.get(id), l = lezer.get(id);
    // blocks present on only one side
    const rk = new Set(r.map(key)), lk = new Set(l.map(key));
    const odd = [...r.filter((b) => !lk.has(key(b))), ...l.filter((b) => !rk.has(key(b)))];
    if (!odd.length) continue;
    const from = Math.min(...odd.map((b) => b.startLine)), to = Math.max(...odd.map((b) => b.endLine));
    const lines = d.text.split("\n");
    out.push({ id: `${name}:${id}`, source: name, section: `lines ${from}-${to}`, text: lines.slice(from, to + 1).join("\n") + "\n" });
  }
}
fs.writeFileSync("../corpus/snippets.jsonl", out.map((x) => JSON.stringify(x)).join("\n") + "\n");
console.log(`${out.length} snippets`);
