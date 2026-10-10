// show.mjs <corpus> <id>  — print the document and both parsers' blocks
import fs from "node:fs";
const [name, id] = process.argv.slice(2);
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const d = read(`../corpus/${name}.jsonl`).find((x) => x.id === id);
const r = read(`../out/${name}.rust.jsonl`).find((x) => x.id === id).blocks;
const l = read(`../out/${name}.lezer.jsonl`).find((x) => x.id === id).blocks;
const lines = d.text.split("\n");
console.log(`--- ${id}: ${d.text.length} chars`);
if (d.text.length < 600) console.log(JSON.stringify(d.text));
const f = (bs) => bs.map((b) => `${b.class}[${b.startLine}-${b.endLine}]`).join(" ");
console.log("rust :", f(r));
console.log("lezer:", f(l));
// show the lines around the first difference
const key = (b) => `${b.class}@${b.startLine}-${b.endLine}`;
const lz = l.filter((b) => b.class !== "linkref");
for (let i = 0; i < Math.max(r.length, lz.length); i++) {
  if (!r[i] || !lz[i] || key(r[i]) !== key(lz[i])) {
    const at = Math.min(r[i]?.startLine ?? 1e9, lz[i]?.startLine ?? 1e9);
    console.log(`first difference near line ${at}:`);
    for (let k = Math.max(0, at - 1); k < Math.min(lines.length, at + 9); k++) console.log(`  ${String(k).padStart(4)}| ${lines[k].slice(0, 110)}`);
    break;
  }
}
