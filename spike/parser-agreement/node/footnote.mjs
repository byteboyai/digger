// The footnote extension lives in the editor spike (the single source, so the conformance run tests exactly what
// the editor uses). Node strips the TypeScript types when importing it.
export { Footnote } from "../../md-engine/host/src/footnote.ts";
