// pair.mjs <A> <B> <corpus...> — how often two block outputs (out/<corpus>.<A>.jsonl, out/<corpus>.<B>.jsonl) agree
import fs from "node:fs";
const [A, B, ...names] = process.argv.slice(2);
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const key = (b) => `${b.class}@${b.startLine}-${b.endLine}`;
for (const n of names) {
  const a = new Map(read(`../out/${n}.${A}.jsonl`).map((d) => [d.id, d.blocks.filter((b) => b.class !== "linkref")]));
  const b = new Map(read(`../out/${n}.${B}.jsonl`).map((d) => [d.id, d.blocks.filter((x) => x.class !== "linkref")]));
  let same = 0;
  for (const [id, x] of a) { const y = b.get(id); if (x.length === y.length && x.every((v, i) => key(v) === key(y[i]))) same++; }
  console.log(`${A} vs ${B} on ${n}: ${same}/${a.size} identical (${(100 * same / a.size).toFixed(1)}%)`);
}
