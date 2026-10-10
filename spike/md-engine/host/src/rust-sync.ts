// The editor's side of the block range sync with Rust (see src/sync.rs).
//
// Rust owns the text and the block structure. The editor sends every change as a text patch in UTF-16 positions
// (one in flight at a time; later changes are composed while it is out) and gets back the version and the block list
// (id, kind, range). While a patch is in flight, block ranges are mapped through the editor's own unconfirmed changes,
// so they stay valid at every moment.
import { ChangeSet } from "@codemirror/state";

export interface RBlock {
  id: string;
  kind: string;
  level: number | null;
  title: string | null;
  how: string;
  from: number;
  to: number;
  head: string;
}

export interface TypedSelection {
  blockId: string;
  kind: string;
  blockFrom: number;
  blockTo: number;
  selFrom: number;
  selTo: number;
  /** titles of the headings that enclose the block, outermost first */
  headingPath: string[];
}

export function fnv(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

export class RustSync {
  blocks: RBlock[] = [];
  version = 0;
  hash = "";
  len16 = 0;
  active = false;
  loading = false;
  rejects: string[] = [];
  private inflight = false;
  private queued: ChangeSet | null = null;
  private waiters: (() => void)[] = [];

  constructor(private post: (o: unknown) => void, private onBlocks: () => void) {
    (window as unknown as { __md: unknown }).__md = { receive: (j: string) => this.receive(JSON.parse(j)) };
  }

  /** A new document: Rust re-parses it and answers with fresh ids. */
  load(text: string): Promise<void> {
    this.active = true;
    this.blocks = [];
    this.queued = null;
    this.inflight = true;
    this.post({ t: "load", text });
    return this.settled();
  }

  /** The editor's document changed (not called while loading). */
  local(changes: ChangeSet) {
    if (!this.active || this.loading) return;
    this.blocks = this.blocks.map((b) => ({ ...b, from: changes.mapPos(b.from, -1), to: changes.mapPos(b.to, 1) }));
    this.queued = this.queued ? this.queued.compose(changes) : changes;
    this.onBlocks();
    this.flush();
  }

  private flush() {
    if (this.inflight || !this.queued) return;
    const q = this.queued;
    this.queued = null;
    this.inflight = true;
    const changes: { from: number; to: number; insert: string }[] = [];
    q.iterChanges((a, b, _c, _d, ins) => changes.push({ from: a, to: b, insert: ins.toString() }));
    this.post({ t: "edit", base: this.version, changes });
  }

  private receive(m: { t: string; version: number; hash: string; len16: number; blocks: RBlock[]; reason?: string }) {
    if (m.t === "blocks") {
      this.version = m.version;
      this.hash = m.hash;
      this.len16 = m.len16;
      this.inflight = false;
      const q = this.queued;
      this.blocks = q ? m.blocks.map((b) => ({ ...b, from: q.mapPos(b.from, -1), to: q.mapPos(b.to, 1) })) : m.blocks;
      this.onBlocks();
      this.flush();
    } else if (m.t === "reject") {
      this.rejects.push(m.reason ?? "?");
      this.inflight = false;
    }
    if (!this.inflight && !this.queued) {
      const w = this.waiters;
      this.waiters = [];
      w.forEach((f) => f());
    }
  }

  /** Resolves when nothing is in flight or queued. */
  settled(): Promise<void> {
    if (!this.inflight && !this.queued) return Promise.resolve();
    return new Promise((r) => this.waiters.push(r));
  }

  blockAt(pos: number): RBlock | undefined {
    // blocks are sorted and disjoint; a caret at a block's end still belongs to it
    return this.blocks.find((b) => pos >= b.from && pos <= b.to);
  }

  typed(selFrom: number, selTo: number): TypedSelection | null {
    const b = this.blockAt(selFrom);
    if (!b) return null;
    const path: { level: number; title: string }[] = [];
    for (const x of this.blocks) {
      if (x === b) break;
      if (x.kind === "heading" && x.level != null) {
        while (path.length && path[path.length - 1].level >= x.level) path.pop();
        path.push({ level: x.level, title: x.title ?? "" });
      }
    }
    return { blockId: b.id, kind: b.kind, blockFrom: b.from, blockTo: b.to, selFrom, selTo, headingPath: path.map((p) => p.title) };
  }
}
