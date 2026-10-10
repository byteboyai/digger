// Compares the top-level blocks of the Rust parser and the Lezer parser, per document and per corpus.
import fs from "node:fs";
const names = process.argv.slice(2);
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));

// pulldown-cmark does not produce a block for link reference definitions; Lezer does. For the main comparison
// they are dropped from the Lezer side (they are reported separately as the "linkref" difference).
const key = (b) => `${b.class}@${b.startLine}-${b.endLine}`;
const span = (b) => `${b.startLine}-${b.endLine}`;

function compare(rust, lezer, dropLinkRef) {
  const L = dropLinkRef ? lezer.filter((b) => b.class !== "linkref") : lezer;
  const exact = rust.length === L.length && rust.every((b, i) => key(b) === key(L[i]));
  const sameSpans = rust.length === L.length && rust.every((b, i) => span(b) === span(L[i]));
  const rs = new Set(rust.map((b) => b.startLine)), ls = new Set(L.map((b) => b.startLine));
  const inter = [...rs].filter((x) => ls.has(x)).length;
  const f1 = rs.size + ls.size === 0 ? 1 : (2 * inter) / (rs.size + ls.size);
  let first = null;
  for (let i = 0; i < Math.max(rust.length, L.length); i++) {
    if (!rust[i] || !L[i] || key(rust[i]) !== key(L[i])) { first = { i, r: rust[i], l: L[i] }; break; }
  }
  return { exact, sameSpans, f1, first, rustN: rust.length, lezerN: L.length };
}

const pct = (a, b) => (b ? ((100 * a) / b).toFixed(1) + "%" : "-");
const summary = [];
for (const name of names) {
  const docs = new Map(read(`../corpus/${name}.jsonl`).map((d) => [d.id, d]));
  const rust = new Map(read(`../out/${name}.${process.env.RUST ?? "rust"}.jsonl`).map((d) => [d.id, d.blocks]));
  const lezer = new Map(read(`../out/${name}.lezer.jsonl`).map((d) => [d.id, d.blocks]));
  const rows = { strict: [], main: [] };
  const sigs = new Map();
  const worst = [];
  const hasLinkRef = (id) => lezer.get(id).some((b) => b.class === "linkref");
  let linkrefDocs = 0, footnoteDocs = 0;
  for (const [id, d] of docs) {
    const r = rust.get(id), l = lezer.get(id);
    rows.strict.push(compare(r, l, false));
    const c = compare(r, l, true);
    rows.main.push(c);
    if (hasLinkRef(id)) linkrefDocs++;
    if (r.some((b) => b.class === "footnote")) footnoteDocs++;
    if (!c.exact && c.first) {
      const sig = `rust:${c.first.r?.class ?? "∅"} vs lezer:${c.first.l?.class ?? "∅"}${c.first.r && c.first.l && span(c.first.r) !== span(c.first.l) ? " (different extent)" : ""}`;
      const e = sigs.get(sig) ?? { n: 0, ex: [] };
      e.n++;
      if (e.ex.length < 3) e.ex.push(id);
      sigs.set(sig, e);
      worst.push({ id, f1: c.f1 });
    }
  }
  const total = docs.size;
  const exactStrict = rows.strict.filter((x) => x.exact).length;
  const exact = rows.main.filter((x) => x.exact).length;
  const spans = rows.main.filter((x) => x.sameSpans).length;
  const f1 = rows.main.reduce((a, x) => a + x.f1, 0) / total;
  const blocksR = rows.main.reduce((a, x) => a + x.rustN, 0);
  console.log(`\n=== ${name}: ${total} documents, ${blocksR} Rust blocks`);
  console.log(`  identical blocks (class and lines), linkrefs set aside: ${exact}/${total} = ${pct(exact, total)}   [counting linkrefs as blocks: ${pct(exactStrict, total)}]`);
  console.log(`  identical line extents regardless of class:             ${spans}/${total} = ${pct(spans, total)}`);
  console.log(`  block-start F1 (mean per document):                      ${(100 * f1).toFixed(2)}%`);
  console.log(`  documents with link reference definitions: ${linkrefDocs}; with footnote definitions (Rust): ${footnoteDocs}`);
  const top = [...sigs.entries()].sort((a, b) => b[1].n - a[1].n).slice(0, 10);
  if (top.length) console.log("  first disagreement by kind:");
  for (const [sig, e] of top) console.log(`    ${String(e.n).padStart(4)}  ${sig}   e.g. ${e.ex.join(", ")}`);
  summary.push({ name, total, exact, spans, f1 });
}
