// Fails when the Rust and Lezer parsers disagree on something that is not in the list of known deviations.
// Known deviations are real differences that were investigated (see ADR-012); a NEW one must be looked at.
import fs from "node:fs";
const read = (f) => fs.readFileSync(f, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
const key = (b) => `${b.class}@${b.startLine}-${b.endLine}`;
const KNOWN = {
  // Lezer deviates from CommonMark: tab after `#`, <textarea> as a raw-text HTML block, an empty list item followed by an indented paragraph
  "commonmark-spec": new Set(["cm-10", "cm-171", "cm-280"]),
  "gfm-extensions": new Set(),
  generated: new Set(),
  edge: new Set(["heading-tab", "html-textarea"]),
};
const MAX_TYPING_STATE_DISAGREEMENT = 0.02; // half-typed tables and quote/list lazy continuation: see ADR-012
let failed = false;
for (const name of [...Object.keys(KNOWN), "edited"]) {
  const rust = new Map(read(`../out/${name}.rust.jsonl`).map((d) => [d.id, d.blocks]));
  const lezer = new Map(read(`../out/${name}.lezer.jsonl`).map((d) => [d.id, d.blocks.filter((b) => b.class !== "linkref")]));
  const diff = [];
  for (const [id, r] of rust) {
    const l = lezer.get(id);
    if (!(r.length === l.length && r.every((b, i) => key(b) === key(l[i])))) diff.push(id);
  }
  if (name === "edited") {
    const rate = diff.length / rust.size;
    const ok = rate <= MAX_TYPING_STATE_DISAGREEMENT;
    console.log(`${ok ? "PASS" : "FAIL"}  edited: ${diff.length}/${rust.size} typing states differ (${(100 * rate).toFixed(2)}%, limit ${100 * MAX_TYPING_STATE_DISAGREEMENT}%)`);
    failed ||= !ok;
  } else {
    const unexpected = diff.filter((id) => !KNOWN[name].has(id));
    const stale = [...KNOWN[name]].filter((id) => !diff.includes(id));
    console.log(`${unexpected.length ? "FAIL" : "PASS"}  ${name}: ${rust.size - diff.length}/${rust.size} identical, ${diff.length - unexpected.length} known deviation(s)${unexpected.length ? `, NEW: ${unexpected.join(", ")}` : ""}${stale.length ? `  (note: no longer differing: ${stale.join(", ")})` : ""}`);
    failed ||= unexpected.length > 0;
  }
}
process.exit(failed ? 1 : 0);
