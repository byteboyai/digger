export interface Engine {
  name: string;
  mount(el: HTMLElement): void;
  load(md: string): void;
  getMarkdown(): string;
  /** Top-level blocks as source ranges of the text that was loaded. */
  blockRanges(): { from: number; to: number }[];
  /** Edit through the editor model: insert `text` right after the first occurrence of `marker`. */
  insertAfter(marker: string, text: string): void;
  /** Place the caret right after `marker`. */
  caretAfter(marker: string): void;
  /** Place the caret in the block that contains `marker` is not wanted: put it at the very start of the document. */
  caretAtStart(): void;
  /** What the user sees (innerText) for the rendered block that contains `marker`. */
  visibleTextOf(marker: string): string;
  imgCount(): number;
  /** Move top-level block `from` to the position before block `to` (indexes in the current block order). */
  moveBlock(from: number, to: number): void;
  /** Type one character at the caret through the editor model. */
  typeChar(ch: string): void;
  /** Source offset (in the loaded text) of the caret, or null if the engine cannot say. */
  sourceOffsetOfCaret(): number | null;
}
