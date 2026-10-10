// A Lezer extension that makes `[^id]: ...` a composite FootnoteDefinition block whose content is whatever follows,
// plus any lines indented by four spaces (and blank lines in between), like pulldown-cmark does.
export const Footnote = {
  defineNodes: [
    {
      name: "FootnoteDefinition",
      block: true,
      composite(cx, line) {
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
