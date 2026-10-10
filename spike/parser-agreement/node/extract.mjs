// Builds the corpora (JSONL: {id, source, section, text}) from the CommonMark spec, the GFM spec extensions,
// the Markdown files that exist in this workspace, and the md-engine sample chapter.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const out = path.resolve("../corpus");
const write = (name, docs) => fs.writeFileSync(path.join(out, name + ".jsonl"), docs.map((d) => JSON.stringify(d)).join("\n") + "\n");
const lf = (s) => s.replace(/\r\n?/g, "\n");

// 1. CommonMark 0.31.2 spec examples (downloaded once as spec.json)
async function cached(name, url) {
  const f = path.join(out, name);
  if (!fs.existsSync(f)) fs.writeFileSync(f, Buffer.from(await (await fetch(url)).arrayBuffer()));
  return fs.readFileSync(f, "utf8");
}
const cm = JSON.parse(await cached("commonmark-0.31.2-spec.json", "https://spec.commonmark.org/0.31.2/spec.json"));
write("commonmark-spec", cm.map((e) => ({ id: `cm-${e.example}`, source: "commonmark-spec", section: e.section, text: lf(e.markdown) })));

// 2. GFM spec: only the extension examples (```` example table ````, strikethrough, tasklist, autolink, ...)
const gfm = (await cached("gfm-spec.txt", "https://raw.githubusercontent.com/github/cmark-gfm/master/test/spec.txt")).split("\n");
const ext = [];
let section = "", i = 0, n = 0;
while (i < gfm.length) {
  const l = gfm[i];
  if (/^#{1,3} /.test(l)) section = l.replace(/^#+ /, "");
  const m = /^`{32} example\s*(.*)$/.exec(l);
  if (m) {
    const md = [];
    i++;
    while (i < gfm.length && gfm[i] !== ".") md.push(gfm[i++]);
    while (i < gfm.length && !/^`{32}$/.test(gfm[i])) i++;
    n++;
    if (m[1].trim()) ext.push({ id: `gfm-${n}`, source: "gfm-extensions", section: `${section} [${m[1].trim()}]`, text: lf(md.join("\n").replace(/→/g, "\t") + "\n") });
  }
  i++;
}
write("gfm-extensions", ext);

// 3. Markdown files in this workspace (real documents: Chinese and English, tables, code, lists, ADRs, specs)
const root = path.join(os.homedir(), "Projects/CoralProjects");
const skip = new Set(["node_modules", "target", ".git", "dist", "dist-cm", "dist-pm", "dist-tiptap", "dist-milkdown", ".cargo", "build"]);
const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    if (e.isDirectory()) { if (!skip.has(e.name)) walk(path.join(d, e.name)); }
    else if (e.name.endsWith(".md") && fs.statSync(path.join(d, e.name)).size < 400_000) files.push(path.join(d, e.name));
  }
})(root);
write("workspace", files.map((f) => ({ id: path.relative(root, f), source: "workspace", section: path.relative(root, f).split("/")[0], text: lf(fs.readFileSync(f, "utf8")) })));
console.log(`commonmark ${cm.length}, gfm extension examples ${ext.length}, workspace files ${files.length}`);
