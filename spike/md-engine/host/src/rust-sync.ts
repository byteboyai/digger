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
  agentErrors: string[] = [];
  /** number of Agent patches integrated, and how many of them had to be transformed over unconfirmed local edits */
  remoteApplied = 0;
  remoteTransformed = 0;
  inflight = false;
  /** the patch that is out (relative to the last confirmed version) and edits made since (relative to the doc after `sent`) */
  private sent: ChangeSet | null = null;
  queued: ChangeSet | null = null;
  private waiters: (() => void)[] = [];

  constructor(private post: (o: unknown) => void, private onBlocks: () => void, private applyRemote: (changes: ChangeSet) => void) {
    (window as unknown as { __md: unknown }).__md = { receive: (j: string) => this.receive(JSON.parse(j)) };
  }

  /** A new document: Rust re-parses it and answers with fresh ids. */
  load(text: string): Promise<void> {
    this.active = true;
    this.blocks = [];
    this.queued = null;
    this.sent = null;
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
    this.sent = q;
    this.inflight = true;
    const changes: { from: number; to: number; insert: string }[] = [];
    q.iterChanges((a, b, _c, _d, ins) => changes.push({ from: a, to: b, insert: ins.toString() }));
    this.post({ t: "edit", base: this.version, changes });
  }

  private mapBlocks(blocks: RBlock[], cs: ChangeSet | null): RBlock[] {
    return cs ? blocks.map((b) => ({ ...b, from: cs.mapPos(b.from, -1), to: cs.mapPos(b.to, 1) })) : blocks;
  }

  private receive(m: { t: string; version: number; hash: string; len16: number; blocks: RBlock[]; reason?: string; changes?: { from: number; to: number; insert: string }[] }) {
    if (m.t === "update") {
      // An Agent patch, relative to the last version we confirmed. Our own unconfirmed edits (the one that is out
      // and the ones queued behind it) happened concurrently: transform both ways so everybody ends in the same text.
      const delta = m.changes!.reduce((a, c) => a + c.insert.length - (c.to - c.from), 0);
      let R = ChangeSet.of(m.changes!.map((c) => ({ from: c.from, to: c.to, insert: c.insert })), m.len16 - delta);
      if (this.sent || this.queued) this.remoteTransformed++;
      if (this.sent) {
        const A = this.sent, R0 = R;
        this.sent = A.map(R0, true); // our typing goes before the Agent's insertion at the same place
        R = R0.map(A, false);
      }
      if (this.queued) {
        const Q = this.queued, R0 = R;
        this.queued = Q.map(R0, true);
        R = R0.map(Q, false);
      }
      this.applyRemote(R); // R now applies to the editor's current document
      this.remoteApplied++;
      this.version = m.version;
      this.hash = m.hash;
      this.len16 = m.len16;
      this.blocks = this.mapBlocks(this.mapBlocks(m.blocks, this.sent), this.queued);
      this.onBlocks();
      // `sent` (if any) is now stale: Rust will refuse it; the reject below re-sends it, transformed
      this.wake();
      return;
    }
    if (m.t === "agent_error") {
      this.agentErrors.push(m.reason ?? "?");
      return;
    }
    if (m.t === "blocks") {
      this.version = m.version;
      this.hash = m.hash;
      this.len16 = m.len16;
      this.inflight = false;
      this.sent = null;
      const q = this.queued;
      this.blocks = q ? m.blocks.map((b) => ({ ...b, from: q.mapPos(b.from, -1), to: q.mapPos(b.to, 1) })) : m.blocks;
      this.onBlocks();
      this.flush();
    } else if (m.t === "reject") {
      this.rejects.push(m.reason ?? "?");
      this.inflight = false;
      if (/StaleBase/.test(m.reason ?? "") && this.sent) {
        // the Agent got there first; our patch has been transformed over its change: send it again on the new version
        this.queued = this.queued ? this.sent.compose(this.queued) : this.sent;
        this.sent = null;
        this.flush();
        return;
      }
      this.sent = null;
    }
    this.wake();
  }

  private wake() {
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
