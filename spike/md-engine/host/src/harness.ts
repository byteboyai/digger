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

    // T11–T17: block ranges and ids come from Rust (the authority) and stay valid while the author types
    if (e.rust) {
      const r = e.rust;
      const within = <T>(p: Promise<T>, what: string) => Promise.race([p, new Promise<T>((_, rej) => setTimeout(() => rej(new Error("timeout waiting for Rust: " + what)), 8000))]);
      const heads = (text: string, bs: ReturnType<typeof r.blocks>) => bs.every((b) => Array.from(text.slice(b.from, b.to)).slice(0, 16).join("") === b.head);
      const UNI = "# 😀 第一章\n\n正文一，带 𠮷 字符和 😀 表情。\n\n## 小节\n\n- 列表项一\n- 列表项二\n\n> 引用\n\n末段。\n";

      await within(r.load(UNI), "load");
      let bs = r.blocks();
      result("T11a block ranges from Rust are valid UTF-16 positions (CJK, emoji, astral characters)", bs.length === 6 && heads(r.doc(), bs), `blocks ${bs.length}: ${bs.map((b) => `${b.kind}[${b.from}-${b.to}]`).join(" ")}`);
      result("T11b ids are unique and both sides hold the same text", new Set(bs.map((b) => b.id)).size === bs.length && r.rustHash() === r.editorHash(), `rust ${r.rustHash()} editor ${r.editorHash()}`);

      const ids0 = bs.map((b) => b.id), v0 = r.version();
      const at = r.doc().indexOf("正文一") + 3;
      r.replace(at, at, "字");
      await within(r.settled(), "typing");
      bs = r.blocks();
      result("T12 typing one character: every id kept, ranges follow, texts equal", bs.map((b) => b.id).join() === ids0.join() && r.version() === v0 + 1 && r.rustHash() === r.editorHash() && heads(r.doc(), bs), `version ${v0}→${r.version()} ids same ${bs.map((b) => b.id).join() === ids0.join()}`);

      const idsBefore = new Set(r.blocks().map((b) => b.id));
      const section = Array.from({ length: 30 }, (_, i) => `新增的第${i}段，讲述与本章无关的另一件事情，字数不少。`).join("\n\n") + "\n\n";
      r.replace(0, 0, section);
      await within(r.settled(), "section insert");
      bs = r.blocks();
      const kept = bs.filter((b) => idsBefore.has(b.id)).length;
      result("T13 inserting 30 paragraphs above: all 6 original ids kept, 30 new ones", kept === 6 && bs.length === 36 && r.rustHash() === r.editorHash() && heads(r.doc(), bs), `kept ${kept} of 6, blocks ${bs.length}`);

      // T14: six edits fired back to back; ranges must be valid at every moment, and equal Rust's own parse at the end
      await within(r.load(UNI), "reload");
      const starts = r.blocks().map((b) => b.from);
      const invariantViolations: string[] = [];
      const check = (when: string) => {
        const doc = r.doc();
        const cur = r.blocks();
        if (!cur.every((b, i) => b.from >= 0 && b.to <= doc.length && b.from <= b.to && (i === 0 || cur[i - 1].to <= b.from))) invariantViolations.push(`${when}: ranges not sorted/in bounds`);
        else if (!cur.every((b) => Array.from(doc.slice(b.from, b.from + 12)).slice(0, 3).join("") === Array.from(b.head).slice(0, 3).join(""))) invariantViolations.push(`${when}: a block no longer starts with its own text`);
      };
      for (let i = starts.length - 1; i >= 0; i--) {
        const bl = r.blocks()[i];
        // inside the block, after its first three characters (counted in code points: never inside a surrogate pair)
        const pos = bl.from + Array.from(r.doc().slice(bl.from, bl.from + 12)).slice(0, 3).join("").length;
        r.replace(pos, pos, "a");
        check(`after edit ${starts.length - i} (in flight)`);
      }
      await within(r.settled(), "rapid edits");
      check("settled");
      const mapped = r.blocks().map((b) => [b.from, b.to].join("-")).join(" ");
      const finalDoc = r.doc();
      await within(r.load(finalDoc), "fresh parse");
      const fresh = r.blocks().map((b) => [b.from, b.to].join("-")).join(" ");
      result("T14 six edits in flight at once: ranges valid at every moment and equal to a fresh Rust parse", invariantViolations.length === 0 && mapped === fresh, `${invariantViolations.join("; ")} mapped=[${mapped}] fresh=[${fresh}]`);

      // T15: Rust decides which block holds the caret, also where the editor's own parser disagrees
      const t15: string[] = [];
      await within(r.load("-\n\n  foo\n"), "list case");
      r.caret(r.doc().indexOf("foo") + 1);
      const lez = e.blockKinds?.() ?? [];
      const rb = r.blocks();
      const act = r.activeBlockFrom();
      t15.push(`list case: Lezer sees ${lez.map((b) => `${b.name}[${b.startLine}-${b.endLine}]`).join(" ")}, Rust sees ${rb.map((b) => `${b.kind}[${b.from}-${b.to}]`).join(" ")}, active block starts at ${act}`);
      const ok1 = rb.length === 2 && rb[1].kind === "paragraph" && act === rb[1].from && lez.length === 1;
      await within(r.load("<textarea>\n\n*foo*\n\n</textarea>\n"), "html case");
      r.caret(r.doc().indexOf("*foo*") + 2);
      const ty = r.typed();
      const ok2 = r.blocks().length === 1 && ty?.kind === "html" && ty.blockFrom === 0;
      t15.push(`html case: typed selection ${ty?.kind} ${ty?.blockFrom}-${ty?.blockTo}`);
      result("T15 where the editor's parser disagrees with Rust, the caret's block is the one Rust reports", ok1 && ok2, t15.join(" | "));

      // T16: typed selection for the Agent: block id, kind, range and the headings that enclose it
      const BOOK = "# 书名\n\n## 第一章\n\n### 第一节\n\n段落甲\n\n### 第二节\n\n段落乙\n\n## 第二章\n\n段落丙\n";
      await within(r.load(BOOK), "book");
      const probe = (m: string) => { r.caret(r.doc().indexOf(m) + 1); return r.typed(); };
      const a = probe("段落乙"), c = probe("段落丙"), h = probe("第一节");
      const sel = (() => { const p0 = r.doc().indexOf("段落乙"); r.caret(p0); return r.typed(); })();
      const okT = a?.kind === "paragraph" && a.headingPath.join("/") === "书名/第一章/第二节" && c?.headingPath.join("/") === "书名/第二章" && h?.kind === "heading" && h.headingPath.join("/") === "书名/第一章" && !!a.blockId && a.blockId !== c?.blockId && (sel?.selFrom ?? -1) >= 0;
      result("T16 typed selection carries block id, kind, range and heading path", okT, `乙: ${a?.headingPath.join("/")} 丙: ${c?.headingPath.join("/")} 第一节: ${h?.headingPath.join("/")}`);

      // T17: a patch against an out-of-date version is refused, and reloading recovers
      const before = r.rejects().length;
      window.ipc.postMessage(JSON.stringify({ t: "edit", base: r.version() - 1, changes: [{ from: 0, to: 0, insert: "x" }] }));
      await new Promise((res) => setTimeout(res, 400));
      const refused = r.rejects().length === before + 1 && /StaleBase/.test(r.rejects()[before]);
      // a position inside a surrogate pair (between the two halves of 😀) cannot be a text boundary
      await within(r.load("😀 一\n\n后文\n"), "pair doc");
      const b2 = r.rejects().length;
      window.ipc.postMessage(JSON.stringify({ t: "edit", base: r.version(), changes: [{ from: 1, to: 1, insert: "x" }] }));
      await new Promise((res) => setTimeout(res, 400));
      const pairRefused = r.rejects().length === b2 + 1 && /BadPosition/.test(r.rejects()[b2]);
      await within(r.load(r.doc()), "resync");
      result("T17 stale patch and a position inside a surrogate pair are refused by Rust; a reload resynchronizes", refused && pairRefused && r.rustHash() === r.editorHash(), `rejects: ${JSON.stringify(r.rejects().slice(before))}`);
      // T19–T23: the Agent (Rust) changes the text and the editor integrates it, also while the author is typing
      const until = async (pred: () => boolean, what: string, ms = 8000) => {
        const t0 = performance.now();
        while (!pred()) {
          if (performance.now() - t0 > ms) throw new Error("timeout: " + what);
          await new Promise((res) => setTimeout(res, 15));
        }
      };
      const agentDone = async (apply: () => void, what: string) => {
        const n = r.stats().remoteApplied + r.agentErrors().length;
        apply();
        await until(() => r.stats().remoteApplied + r.agentErrors().length > n, what + " (Agent reply)");
        await within(r.settled(), what);
      };
      const NOTE = "# 题\n\n甲段落的内容在这里，足够长一些。\n\n乙段落的内容也在这里，同样足够长。\n";
      const idOf = (m: string) => r.blocks().find((b) => r.doc().slice(b.from, b.to).includes(m))!.id;
      const same = () => r.rustHash() === r.editorHash();

      // T19: the Agent rewrites a block that is addressed by its id; the author is idle
      await within(r.load(NOTE), "T19 load");
      const idA = idOf("甲段落"), idB = idOf("乙段落"), v19 = r.version();
      await agentDone(() => r.agent({ op: "replace_block", id: idA, text: "Agent 改写后的甲段落😀，更精炼。" }), "T19");
      result("T19 Agent patch addressed by block id reaches the editor; ids of untouched blocks kept; texts equal", r.doc().includes("Agent 改写后的甲段落😀") && r.doc().includes("乙段落") && idOf("乙段落") === idB && r.version() === v19 + 1 && same(), `version ${v19}→${r.version()} equal=${same()}`);

      // T20: the Agent's change reaches Rust first, then the author's keystrokes (sent on the old version) are refused,
      //      transformed over the Agent's change in the editor and sent again; nothing is lost on either side
      await within(r.load(NOTE), "T20 load");
      const idA20 = idOf("甲段落"), t0 = r.stats().remoteTransformed;
      const typeAt = r.doc().indexOf("乙段落") + 3;
      r.agent({ op: "replace_block", id: idA20, text: "Agent 在上方改写这一段。" });
      r.replace(typeAt, typeAt, "★手写★"); // posted right behind the Agent's request: Rust sees them in this order
      await until(() => r.stats().remoteApplied >= 1, "T20 agent reply");
      await within(r.settled(), "T20 settle");
      result("T20 author types while the Agent edits another block: both changes survive and both sides agree", r.doc().includes("Agent 在上方改写这一段。") && r.doc().includes("★手写★") && same() && r.stats().remoteTransformed > t0, `transformed=${r.stats().remoteTransformed - t0} rejects=${JSON.stringify(r.rejects().slice(-2))}`);

      // T21: the author types INSIDE the block the Agent rewrites: the typing is kept next to the new text, not dropped
      await within(r.load(NOTE), "T21 load");
      const idB21 = idOf("乙段落");
      const inside = r.doc().indexOf("乙段落") + 3;
      r.agent({ op: "replace_block", id: idB21, text: "Agent 整段改写了乙。" });
      r.replace(inside, inside, "★我在写★");
      await until(() => r.stats().remoteApplied >= 1, "T21 agent reply");
      await within(r.settled(), "T21 settle");
      result("T21 author types inside the block the Agent rewrites: the Agent's text and the typing both survive", r.doc().includes("Agent 整段改写了乙。") && r.doc().includes("★我在写★") && same(), `result: ${JSON.stringify(r.doc().slice(r.doc().indexOf("甲段落")))}`);

      // T22: one Ctrl-Z takes back the Agent's change and nothing else
      await within(r.load(NOTE), "T22 load");
      const original22 = r.doc();
      const p22 = r.doc().indexOf("甲段落") + 3;
      r.replace(p22, p22, "A"); // the author's own earlier edit
      await within(r.settled(), "T22 own edit");
      const afterOwn = r.doc();
      await new Promise((res) => setTimeout(res, 600)); // outside CodeMirror's grouping window
      await agentDone(() => r.agent({ op: "replace_block", id: idOf("乙段落"), text: "Agent 重写的乙。" }), "T22 agent");
      const afterAgent = r.doc();
      r.undo();
      await within(r.settled(), "T22 undo");
      result("T22 Ctrl-Z reverts exactly the Agent's change; the author's earlier edit stays; both sides agree", afterAgent.includes("Agent 重写的乙。") && r.doc() === afterOwn && afterOwn !== original22 && same(), `after undo equals the state before the Agent: ${r.doc() === afterOwn}`);

      // T23: randomized interleaving of the author's edits, Agent operations and undo, without waiting in between
      await within(r.load(NOTE + "\n- 项一\n- 项二\n\n> 引用一段\n\n末段在这里。\n"), "T23 load");
      let seed = 0x9e3779b1;
      const rnd = (n: number) => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed % Math.max(1, n); };
      const boundary = (p: number) => { const d = r.doc(); const c = d.charCodeAt(p); return c >= 0xdc00 && c <= 0xdfff && p > 0 ? p + 1 : p; };
      const TEXTS = ["字", "ab", "😀", "，", "\n\n", "新的一段话在此。", "𠮷", " "];
      let agentOps = 0;
      const base = r.stats();
      const rejectsBefore = r.rejects().length;
      for (let step = 0; step < 160; step++) {
        const k = rnd(100);
        const d = r.doc();
        if (k < 40) { const p = boundary(rnd(d.length + 1)); r.replace(p, p, TEXTS[rnd(TEXTS.length)]); }
        else if (k < 55 && d.length > 20) { const p = boundary(rnd(d.length - 4)); r.replace(p, boundary(Math.min(d.length, p + 1 + rnd(4))), ""); }
        else if (k < 75) { const bs = r.blocks(); if (bs.length) { agentOps++; r.agent({ op: "replace_block", id: bs[rnd(bs.length)].id, text: `Agent 重写 ${step}：好的😀。` }); } }
        else if (k < 85) { const bs = r.blocks(); if (bs.length) { agentOps++; r.agent({ op: "insert_after", id: bs[rnd(bs.length)].id, text: `Agent 新增 ${step}。` }); } }
        else if (k < 90) { agentOps++; r.agent({ op: "append", text: `Agent 追加 ${step}。` }); }
        else if (k < 94) r.undo();
        else await new Promise((res) => setTimeout(res, rnd(40)));
      }
      await until(() => r.stats().remoteApplied - base.remoteApplied + (r.agentErrors().length) >= agentOps, "T23 all Agent replies", 20000);
      await within(r.settled(), "T23 settle");
      const endDoc = r.doc(), mappedRanges = r.blocks().map((b) => `${b.from}-${b.to}`).join(" "), ids23 = r.blocks().map((b) => b.id);
      const eq = same() && endDoc.length === r.docLength16();
      await within(r.load(endDoc), "T23 fresh parse");
      const freshRanges = r.blocks().map((b) => `${b.from}-${b.to}`).join(" ");
      const mine = r.rejects().slice(rejectsBefore);
      const badRejects = mine.filter((x) => !/StaleBase/.test(x));
      const badAgentErrors = r.agentErrors().filter((x) => !/^no block with id/.test(x));
      result("T23 160 random author edits, Agent operations and undos in flight together: both sides end identical", eq && mappedRanges === freshRanges && new Set(ids23).size === ids23.length && badRejects.length === 0 && badAgentErrors.length === 0, `agent ops ${agentOps} (applied ${r.stats().remoteApplied - base.remoteApplied}, refused ${r.agentErrors().length}), transformed over pending edits ${r.stats().remoteTransformed - base.remoteTransformed}, stale rejections ${mine.length - badRejects.length}, other rejections ${JSON.stringify(badRejects.slice(0, 3))}, equal=${eq}, ranges equal=${mappedRanges === freshRanges}`);

      // round-trip cost of keeping Rust in sync, on long chapters (informational)
      for (const reps of [20, 200]) {
        const text = repeated(reps);
        const t0 = performance.now();
        await within(r.load(text), "long load");
        const loadMs = performance.now() - t0;
        const mid = Math.floor(r.doc().length / 2);
        const lat: number[] = [];
        for (let i = 0; i < 12; i++) {
          const a = performance.now();
          r.replace(mid + i, mid + i, "字");
          await within(r.settled(), "latency");
          lat.push(performance.now() - a);
        }
        lat.sort((x, y) => x - y);
        info(`T18 Rust round trip, ${reps * 14} blocks, ${(text.length / 1000).toFixed(0)}k chars`, `load+parse ${loadMs.toFixed(0)} ms | one-character edit → blocks back: median ${lat[6].toFixed(0)} ms, max ${lat[11].toFixed(0)} ms | ${r.blocks().length} blocks | hashes equal: ${r.rustHash() === r.editorHash()}`);
      }
      r.detach();
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
    result("harness error", false, String((err as Error)?.message ?? err) + " :: " + String((err as Error)?.stack ?? ""));
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
