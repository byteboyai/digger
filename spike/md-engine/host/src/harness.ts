import type { Engine } from "./engine";
import { CORPUS, MARKERS, repeated } from "./corpus";
import { FIXTURES } from "./fixtures";

declare global {
  interface Window { ipc: { postMessage(m: string): void }; __MODE__?: string }
}
const post = (o: unknown) => window.ipc.postMessage(JSON.stringify(o));
const result = (name: string, pass: boolean, detail = "") => post({ t: "result", name, pass, detail });
const info = (name: string, detail: string) => post({ t: "info", name, detail });
const raf2 = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));

function region(a: string, b: string) {
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  let s = 0;
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++;
  return { p, aEnd: a.length - s, bEnd: b.length - s };
}
const sortedChars = (s: string) => [...s].sort().join("");

async function run(e: Engine) {
  try {
    // T1: open and save without editing → identical bytes
    e.load(CORPUS);
    const same = e.getMarkdown() === CORPUS;
    const r1 = region(CORPUS, e.getMarkdown());
    result("T1 round trip without edits is byte-exact", same, same ? "" : `first difference at ${r1.p}: ${JSON.stringify(CORPUS.slice(r1.p, r1.p + 30))} → ${JSON.stringify(e.getMarkdown().slice(r1.p, r1.p + 30))}`);

    // T2: edit one block → only that range changes, with zero collateral churn
    let exact = 0;
    const lines: string[] = [];
    for (const m of MARKERS) {
      e.load(CORPUS);
      const ranges = e.blockRanges();
      const at = CORPUS.indexOf(m);
      const blk = ranges.findIndex((b) => at >= b.from && at <= b.to);
      e.insertAfter(m, "字");
      const out = e.getMarkdown();
      const r = region(CORPUS, out);
      const churn = r.aEnd - r.p + (r.bEnd - r.p - 1);
      const inside = blk >= 0 && r.p >= ranges[blk].from && r.aEnd <= ranges[blk].to;
      if (churn === 0 && inside) exact++;
      if (churn !== 0 || !inside) lines.push(`${m}: churn ${churn}${inside ? "" : ", OUTSIDE its block"}${churn ? ` (${JSON.stringify(CORPUS.slice(r.p, Math.min(r.aEnd, r.p + 24)))}…)` : ""}`);
    }
    result(`T2 one-character edit changes only that character (${exact}/${MARKERS.length} blocks exact)`, exact === MARKERS.length, lines.join("; "));

    // T3–T5: progressive syntax reveal
    e.load(CORPUS);
    e.caretAfter("MARK_END");
    const hOut = e.visibleTextOf("MARK_H1");
    e.caretAfter("MARK_H1");
    const hIn = e.visibleTextOf("MARK_H1");
    result("T3 heading: marker hidden away from the caret, revealed with it", !/^#/.test(hOut.trim()) && /^#\s/.test(hIn.trim()), `away=${JSON.stringify(hOut.slice(0, 14))} caret=${JSON.stringify(hIn.slice(0, 14))}`);
    e.caretAfter("MARK_END");
    const pOut = e.visibleTextOf("MARK_P1");
    e.caretAfter("MARK_BOLD");
    const pIn = e.visibleTextOf("MARK_P1");
    result("T4 inline marks: hidden away from the caret, revealed with it", !/[*`]/.test(pOut) && pIn.includes("**"), `away=${JSON.stringify(pOut.slice(0, 40))} caret=${JSON.stringify(pIn.slice(0, 40))}`);
    e.load(CORPUS);
    const imgsFresh = e.imgCount();
    e.caretAfter("MARK_END");
    const iOut = e.visibleTextOf("MARK_CAP");
    const imgs = e.imgCount();
    info("T5 image count probe", `fresh load: ${imgsFresh}, after caret move: ${imgs}`);
    e.caretAfter("MARK_CAP");
    const iIn = e.visibleTextOf("MARK_CAP");
    result("T5a image renders as an image away from the caret", imgs >= 1 && !iOut.includes("!["), `img=${imgs} text=${JSON.stringify(iOut.slice(0, 24))}`);
    result("T5b image source can be revealed and edited as Markdown", iIn.includes("!["), `caret text=${JSON.stringify(iIn.slice(0, 24))}`);

    // T7: moving a block keeps every block verbatim and loses/invents nothing
    e.load(CORPUS);
    const rg = e.blockRanges();
    const idx = (m: string) => rg.findIndex((b) => CORPUS.indexOf(m) >= b.from && CORPUS.indexOf(m) <= b.to);
    const iUl = idx("MARK_UL"), iP1 = idx("MARK_P1");
    e.moveBlock(iUl, iP1);
    const moved = e.getMarkdown();
    const verbatim = rg.every((b) => moved.includes(CORPUS.slice(b.from, b.to)));
    const order = moved.indexOf("MARK_UL") < moved.indexOf("MARK_P1");
    const perm = sortedChars(moved) === sortedChars(CORPUS);
    result("T7 block move: blocks verbatim, order changed, nothing lost or invented", verbatim && order && perm, `verbatim=${verbatim} order=${order} sameCharacters=${perm}`);

    // T9: caret → source offset (Agent typed selection, Source Map)
    let exactOff = 0;
    const probe = ["MARK_P1", "MARK_UNDER", "MARK_OL1", "MARK_NEST", "MARK_QUOTE2", "MARK_REF", "MARK_BOLD", "MARK_END"];
    const bad: string[] = [];
    for (const m of probe) {
      e.load(CORPUS);
      e.caretAfter(m);
      const got = e.sourceOffsetOfCaret();
      const want = CORPUS.indexOf(m) + m.length;
      if (got === want) exactOff++; else bad.push(`${m}: ${got} vs ${want}`);
    }
    result(`T9 caret maps to the exact source offset (${exactOff}/${probe.length})`, exactOff === probe.length, bad.join("; "));

    // T10: the editor's own parser splits the text into the same top-level blocks as the Rust parser
    if (e.blockKinds) {
      const CLASS: Record<string, string> = { Paragraph: "paragraph", Blockquote: "quote", BulletList: "list", OrderedList: "list", FencedCode: "code", CodeBlock: "code", FootnoteDefinition: "footnote", HTMLBlock: "html", CommentBlock: "html", ProcessingInstructionBlock: "html", HorizontalRule: "rule", Table: "table", LinkReference: "linkref" };
      const cls = (n: string) => (/^(ATX|Setext)Heading/.test(n) ? "heading" : CLASS[n] ?? "other:" + n);
      let same = 0;
      const bad: string[] = [];
      for (const f of FIXTURES) {
        e.load(f.text);
        const got = e.blockKinds().map((b) => ({ class: cls(b.name), startLine: b.startLine, endLine: b.endLine })).filter((b) => b.class !== "linkref");
        const key = (b: { class: string; startLine: number; endLine: number }) => `${b.class}[${b.startLine}-${b.endLine}]`;
        if (got.length === f.rust.length && got.every((b, i) => key(b) === key(f.rust[i]))) same++;
        else bad.push(`${f.name}: editor ${got.map(key).join(" ")} vs Rust ${f.rust.map(key).join(" ")}`);
      }
      result(`T10 editor parser = Rust parser on ${FIXTURES.length} texts incl. footnotes (${same}/${FIXTURES.length})`, same === FIXTURES.length, bad.join("; "));
    }

    // T6: performance on long chapters
    for (const reps of [20, 200, 1000]) {
      const text = repeated(reps);
      const t0 = performance.now();
      e.load(text);
      await raf2();
      const loadMs = performance.now() - t0;
      const t1 = performance.now();
      const md = e.getMarkdown();
      const saveMs = performance.now() - t1;
      e.caretAfter(`M${Math.floor(reps / 2)}_UL`);
      const tx: number[] = [];
      for (let i = 0; i < 30; i++) { const a = performance.now(); e.typeChar("字"); tx.push(performance.now() - a); }
      const a = performance.now();
      e.typeChar("尾");
      await raf2();
      const paint = performance.now() - a;
      tx.sort((x, y) => x - y);
      const dom = document.getElementById("editor")!.querySelectorAll("*").length;
      info(`T6 ${reps} chapters (~${(text.match(/\n\n/g) || []).length} blocks), ${(text.length / 1000).toFixed(0)}k chars`, `load+paint ${loadMs.toFixed(0)} ms | serialize ${saveMs.toFixed(0)} ms | keystroke mean ${(tx.reduce((x, y) => x + y, 0) / tx.length).toFixed(2)} ms p95 ${tx[Math.floor(tx.length * 0.95)].toFixed(2)} ms | key→paint ${paint.toFixed(0)} ms | DOM nodes ${dom} | output ${(md.length / 1000).toFixed(0)}k chars`);
    }
  } catch (err) {
    result("harness error", false, String(err && (err as Error).stack || err));
  }
  post({ t: "done" });
}

export function start(e: Engine) {
  const style = document.createElement("style");
  style.textContent = `html,body{margin:0;height:100%;background:#fbfaf7;color:#1d1d1b}
  #editor{height:100%;overflow:auto;font:17px/1.8 -apple-system,"PingFang SC",sans-serif}
  .ProseMirror{outline:none;min-height:100%;max-width:720px;margin:0 auto;padding:24px 40px;white-space:pre-wrap}
  .ProseMirror p{margin:0 0 .8em}.ProseMirror h1,.ProseMirror h2{margin:.6em 0 .4em}
  .syn{color:#aaa}pre.raw{background:#f1efe9;padding:6px;white-space:pre-wrap;margin:0 0 .8em}
  .ProseMirror pre{background:#f1efe9;padding:6px}.ProseMirror blockquote{border-left:3px solid #bbb;margin:0 0 .8em;padding-left:10px;color:#555}`;
  document.head.appendChild(style);
  e.mount(document.getElementById("editor")!);
  window.addEventListener("error", (ev) => post({ t: "result", name: "page error", pass: false, detail: ev.message }));
  if (window.__MODE__ === "test") { void run(e); }
  else { e.load(CORPUS); e.caretAtStart(); post({ t: "ready", engine: e.name }); }
}
