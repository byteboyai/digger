// Simulates typing: apply random edits to documents and, after EVERY edit, check
//   (a) the incrementally updated Lezer tree (what CodeMirror holds) gives the same blocks as a full parse, and
//   (b) writes the intermediate text to a corpus so Rust and Lezer can be compared on half-typed states.
import fs from "node:fs";
import { TreeFragment } from "@lezer/common";
import { makeParser, blocksOf } from "./blocks.mjs";

const parser = makeParser(true);
let seed = 0x2545f491;
const rnd = (n) => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % Math.max(1, n); };
const SNIPPETS = ["\n\n", "\n", "# ", "## ", "- ", "1. ", "> ", "```", "~~~", "| a | b |\n|---|---|\n", "|", "字", "，", "  ", "\t", "    ", "**", "_", "`", "[^1]: ", "[a]: x\n", "<div>", "</div>", "<!--", "-->", "---", "===", "[ ] ", "$$", "\n\n\n"];
const key = (b) => `${b.class}@${b.startLine}-${b.endLine}`;
const sig = (bs) => bs.map(key).join(" ");

const docs = fs.readFileSync("../corpus/generated.jsonl", "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).slice(0, 150);
const out = [];
let checks = 0, incrementalMismatch = 0;
const examples = [];
for (const d of docs) {
  let text = d.text;
  let tree = parser.parse(text);
  let fragments = TreeFragment.addTree(tree);
  for (let k = 0; k < 14; k++) {
    // an edit: insert a snippet or delete a short range, at a random place
    const at = rnd(text.length + 1);
    let from = at, to = at, ins = "";
    if (rnd(100) < 60) ins = SNIPPETS[rnd(SNIPPETS.length)];
    else { to = Math.min(text.length, at + 1 + rnd(30)); }
    const next = text.slice(0, from) + ins + text.slice(to);
    fragments = TreeFragment.applyChanges(fragments, [{ fromA: from, toA: to, fromB: from, toB: from + ins.length }]);
    const inc = parser.parse(next, fragments);
    fragments = TreeFragment.addTree(inc, fragments);
    const full = parser.parse(next);
    checks++;
    if (sig(blocksOf(inc, next)) !== sig(blocksOf(full, next))) {
      incrementalMismatch++;
      if (examples.length < 3) examples.push({ id: d.id, k, edit: JSON.stringify({ at, ins, del: to - from }) });
    }
    text = next;
    out.push({ id: `${d.id}#${k}`, source: "edited", section: "typing", text });
  }
}
fs.writeFileSync("../corpus/edited.jsonl", out.map((x) => JSON.stringify(x)).join("\n") + "\n");
console.log(`incremental vs full parse (Lezer): ${checks} edits, ${incrementalMismatch} mismatches`);
for (const e of examples) console.log("  ", JSON.stringify(e));
