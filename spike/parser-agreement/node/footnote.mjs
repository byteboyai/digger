// The footnote extension now lives in the product (crates/desktop/web), the single source, so the conformance run
// tests exactly what the editor uses. Node strips the TypeScript types when importing it.
export { Footnote } from "../../../crates/desktop/web/src/footnote.ts";
