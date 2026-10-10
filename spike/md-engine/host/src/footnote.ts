// Footnote definitions for the Lezer Markdown parser used by CodeMirror 6.
//
// Lezer has no footnote support: `[^1]: text` is parsed as a link reference and a second paragraph of the same
// footnote as an indented code block. The Rust parser (pulldown-cmark) sees one FootnoteDefinition block, so the
// editor would disagree about where the block ends. This composite block makes `[^id]: ...` plus any lines indented
// by four spaces (and the blank lines between them) one `FootnoteDefinition`, like pulldown-cmark does.
//
// This file is the single source: `spike/parser-agreement` imports it for its conformance run.
import type { MarkdownConfig } from "@lezer/markdown";

export const Footnote: MarkdownConfig = {
  defineNodes: [
    {
      name: "FootnoteDefinition",
      block: true,
      composite(_cx, line) {
        if (line.next < 0) return true; // blank line: the definition may continue
        if (line.indent >= line.baseIndent + 4) {
          line.moveBaseColumn(line.baseIndent + 4);
          return true;
        }
        return false;
      },
    },
  ],
  parseBlock: [
    {
      name: "FootnoteDefinition",
      before: "LinkReference",
      // `[^2]: ...` directly under another definition's text (or under a paragraph) starts a new definition
      // instead of being swallowed as a lazy continuation line.
      endLeaf(_cx, line) {
        return line.indent < line.baseIndent + 4 && /^\[\^[^\]\s]+\]:/.test(line.text.slice(line.pos));
      },
      parse(cx, line) {
        if (line.indent >= line.baseIndent + 4) return false;
        const m = /^\[\^[^\]\s]+\]:[ \t]*/.exec(line.text.slice(line.pos));
        if (!m) return false;
        cx.startComposite("FootnoteDefinition", line.pos, 4);
        line.moveBase(line.pos + m[0].length);
        return null;
      },
    },
  ],
};
