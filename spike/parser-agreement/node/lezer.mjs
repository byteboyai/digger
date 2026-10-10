// Prints, for each document of a corpus, the top-level blocks according to the parser CodeMirror 6 uses
// (@codemirror/lang-markdown `markdownLanguage` = CommonMark + GFM tables, task lists, strikethrough),
// optionally with the footnote extension (--footnotes).
import fs from "node:fs";
import { makeParser, blocksOf } from "./blocks.mjs";
const args = process.argv.slice(2);
const [file] = args.filter((a) => !a.startsWith("--"));
const parser = makeParser(args.includes("--footnotes"));
for (const line of fs.readFileSync(file, "utf8").split("\n").filter(Boolean)) {
  const d = JSON.parse(line);
  console.log(JSON.stringify({ id: d.id, blocks: blocksOf(parser.parse(d.text), d.text) }));
}
