// reference.mjs <corpus> <id> — top-level blocks according to commonmark.js (the CommonMark reference implementation)
import fs from "node:fs";
import { Parser } from "commonmark";
export function referenceBlocks(text) {
  const root = new Parser().parse(text);
  const out = [];
  for (let n = root.firstChild; n; n = n.next) {
    const [[sl], [el]] = n.sourcepos;
    out.push({ class: n.type, startLine: sl - 1, endLine: el - 1 });
  }
  return out;
}
if (process.argv[1].endsWith("reference.mjs")) {
  const [name, id] = process.argv.slice(2);
  const d = fs.readFileSync(`../corpus/${name}.jsonl`, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).find((x) => x.id === id);
  console.log(referenceBlocks(d.text).map((b) => `${b.class}[${b.startLine}-${b.endLine}]`).join(" "));
}
