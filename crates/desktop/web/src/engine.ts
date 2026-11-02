import type { RBlock, SyncState, TypedSelection } from "./rust-sync";

/** Block ranges that come from Rust (the authority) instead of the editor's own parser. */
export interface RustApi {
  /** Start the bounded production ready/open handshake. */
  start(engine: string): void;
  load(text: string): Promise<void>;
  settled(): Promise<void>;
  blocks(): RBlock[];
  version(): number;
  /** hash of the text Rust holds / hash of the text the editor holds */
  rustHash(): string;
  editorHash(): string;
  /** replace [from,to) with `insert` through the editor, like typing would */
  replace(from: number, to: number, insert: string): void;
  caret(pos: number): void;
  doc(): string;
  typed(): TypedSelection | null;
  /** which top-level block the live preview treats as the one holding the caret */
  activeBlockFrom(): number | null;
  rejects(): string[];
  /** length (UTF-16 units) of the text Rust holds */
  docLength16(): number;
  /** ask the Agent stand-in in Rust to change the document (addressed by block id) */
  agent(op: { op: "replace_block" | "insert_after" | "append"; id?: string; text: string }): void;
  agentErrors(): string[];
  /** IME composition is active in the editor right now */
  composing(): boolean;
  /** an edit of ours is out / more are queued behind it */
  pending(): { inflight: boolean; queued: boolean };
  /** explicit recovery state (opening / ready / sending / recovering / conflict / failed / closed) */
  syncState(): SyncState;
  /** after a conflict: the authoritative snapshot and the local draft, for display / export */
  conflict(): { snapshotText: string; draft: string } | null;
  /** explicit author choice: take Rust's text */
  resolveWithSnapshot(): void;
  /** explicit author choice: commit the local draft as a new edit */
  commitDraft(): void;
  /** visible terminal failure (recovery exhausted) with the draft kept */
  failure(): { message: string; draft: string; retryable: boolean } | null;
  /** manual retry after recovery exhausted */
  retrySync(): void;
  /** log of composition and Agent-patch events (manual testing) */
  onEvent(cb: (line: string) => void): void;
  /** the author's Ctrl-Z */
  undo(): void;
  /** press a key chord on the editor like the keyboard does (key event dispatched on the content element) */
  pressKey(init: KeyboardEventInit): void;
  stats(): { remoteApplied: number; remoteTransformed: number };
  detach(): void;
}

export interface Engine {
  rust?: RustApi;
  name: string;
  mount(el: HTMLElement): void;
  load(md: string): void;
  getMarkdown(): string;
  /** Top-level blocks by the engine's own parser (name and 0-based line extent); only engines with a Lezer tree implement it. */
  blockKinds?(): { name: string; startLine: number; endLine: number }[];
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
