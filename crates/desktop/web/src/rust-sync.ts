// The editor's side of the block range sync with Rust (see src/sync.rs).
//
// Rust owns the text and the block structure. The editor sends every change as a text patch in UTF-16 positions
// (one in flight at a time; later changes are composed while it is out) and gets back the version and the block list
// (id, kind, range). While a patch is in flight, block ranges are mapped through the editor's own unconfirmed changes,
// so they stay valid at every moment.
//
// Recovery is explicit: a stale base is retried only when the in-flight patch was transformed over the change that
// made it stale; any other rejection, timeout or malformed reply stops sending, keeps the draft, and requests a
// read-only snapshot. A snapshot that cannot prove the draft safe ends in `conflict`; the author chooses between the
// authoritative text and committing the draft as a new edit. Waiting for `settled()` never hangs: every terminal state
// rejects all waiters with a typed `SyncError`.
import { ChangeSet, Text } from "@codemirror/state";

export const PROTOCOL_VERSION = 2;

export type SyncState = "opening" | "ready" | "sending" | "recovering" | "conflict" | "failed" | "closed";

export type SyncErrorCode =
  | "stale_base"
  | "bad_position"
  | "malformed_changes"
  | "unauthorized"
  | "unsupported_protocol"
  | "duplicate_request"
  | "out_of_order_request"
  | "internal"
  | "timeout"
  | "closed"
  | "conflict"
  | "protocol";

export class SyncError extends Error {
  constructor(readonly code: SyncErrorCode, message: string) {
    super(message);
    this.name = "SyncError";
  }
}

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

/** Rust 对 ready 的回答:初始文档与它的块。 */
export interface OpenMsg {
  t: "open";
  protocol: number;
  text: string;
  version: number;
  hash: string;
  len16: number;
  blocks: RBlock[];
}

export interface SnapshotMsg {
  t: "snapshot";
  request_id?: number;
  text: string;
  version: number;
  hash: string;
  len16: number;
  blocks: RBlock[];
}

export interface SyncOptions {
  isComposing?: () => boolean;
  onInputLock?: (locked: boolean) => void;
  /** the editor's current text, for an explicit "commit my draft" choice after a conflict */
  getText?: () => string;
  /** called whenever the recovery state changes (the page uses it to show the conflict banner) */
  onState?: (state: SyncState) => void;
  /** self-test only: compute the full-text FNV locally when Rust intentionally omits it */
  computeHash?: boolean;
  /** bounded wait for an edit / snapshot reply */
  timeoutMs?: number;
  /** injectable timer; returns a cancel function (tests use a fake clock) */
  schedule?: (fn: () => void, ms: number) => () => void;
}

interface Waiter {
  resolve: () => void;
  reject: (e: SyncError) => void;
}

interface BlockResult {
  request_id?: number;
  version: number;
  hash: string;
  len16: number;
  blocks: RBlock[];
}
type WireChange = { from: number; to: number; insert: string };
export type OutMsg =
  | OpenMsg
  | SnapshotMsg
  | ({ t: "blocks" } & BlockResult)
  | ({ t: "incremental"; base_version: number; removed_from: number; removed_to: number; changes: WireChange[] } & BlockResult)
  | ({ t: "update"; changes: WireChange[] } & BlockResult)
  | { t: "reject"; request_id?: number; code: SyncErrorCode; reason: string }
  | { t: "agent_error"; reason: string }
  | { t: "flush"; request_id: number; freeze?: boolean }
  | { t: "input_lock"; locked: boolean };

/** Validate the untrusted native boundary before touching sync state or CodeMirror. */
export function decodeOutbound(value: unknown): OutMsg {
  const record = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
  const uint = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
  const bad = (): never => { throw new SyncError("protocol", "Rust 回包字段不合法"); };
  if (!record(value)) return bad();
  const m = value;
  if (m.request_id !== undefined && !uint(m.request_id)) return bad();
  if (m.t === "input_lock") { if (typeof m.locked !== "boolean") return bad(); return m as unknown as OutMsg; }
  if (m.t === "flush") {
    if (!uint(m.request_id) || (m.freeze !== undefined && typeof m.freeze !== "boolean")) return bad();
    return m as unknown as OutMsg;
  }
  if (m.t === "reject") {
    if (!isErrorCode(m.code) || typeof m.reason !== "string") return bad();
  } else if (m.t === "agent_error") {
    if (typeof m.reason !== "string") return bad();
  } else if (["open", "snapshot", "blocks", "incremental", "update"].includes(String(m.t))) {
    if (!uint(m.version) || !uint(m.len16) || typeof m.hash !== "string" || !Array.isArray(m.blocks)) return bad();
    const ids = new Set<string>();
    let end = 0;
    for (const b of m.blocks) {
      if (!record(b) || typeof b.id !== "string" || ids.has(b.id) || typeof b.kind !== "string" ||
          !(b.level === null || (uint(b.level) && b.level >= 1 && b.level <= 6)) ||
          !(b.title === null || typeof b.title === "string") || typeof b.how !== "string" || typeof b.head !== "string" ||
          !uint(b.from) || !uint(b.to) || b.from < end || b.from > b.to || b.to > m.len16) return bad();
      ids.add(b.id); end = b.to;
    }
    if (m.t === "open" || m.t === "snapshot") {
      if (typeof m.text !== "string" || m.text.length !== m.len16) return bad();
      if (m.t === "open" && !uint(m.protocol)) return bad();
    }
    if (m.t === "incremental" || m.t === "update") {
      if (!Array.isArray(m.changes)) return bad();
      let previous = 0, delta = 0;
      for (const c of m.changes) {
        if (!record(c) || !uint(c.from) || !uint(c.to) || c.from < previous || c.from > c.to || typeof c.insert !== "string") return bad();
        previous = c.to; delta += c.insert.length - (c.to - c.from);
      }
      if (previous > m.len16 - delta || m.len16 - delta < 0) return bad();
      if (m.t === "incremental" && (!uint(m.base_version) || !uint(m.removed_from) || !uint(m.removed_to) || m.removed_from > m.removed_to)) return bad();
    }
  } else return bad();
  return m as unknown as OutMsg;
}

/** Bound the automatic snapshot loop: an edit failure plus one snapshot retry, then a terminal state. */
const MAX_RECOVERY_ATTEMPTS = 2;

const ERROR_CODES: readonly SyncErrorCode[] = [
  "stale_base",
  "bad_position",
  "malformed_changes",
  "unauthorized",
  "unsupported_protocol",
  "duplicate_request",
  "out_of_order_request",
  "internal",
];

function isErrorCode(value: unknown): value is SyncErrorCode {
  return typeof value === "string" && (ERROR_CODES as readonly string[]).includes(value);
}

function parseCode(m: Extract<OutMsg, { t: "reject" }>): SyncErrorCode {
  if (isErrorCode(m.code)) return m.code;
  // 兼容旧回包:reason 里仍有 Rust Debug 名字。
  if (/StaleBase/.test(m.reason ?? "")) return "stale_base";
  if (/BadPosition/.test(m.reason ?? "")) return "bad_position";
  if (/Malformed/.test(m.reason ?? "")) return "malformed_changes";
  return "internal";
}

export class RustSync {
  /** blocks as displayed at the current editor text (confirmed blocks mapped through pending edits) */
  blocks: RBlock[] = [];
  /** blocks exactly as Rust last confirmed them (base coordinates for incremental results) */
  private confirmedBlocks: RBlock[] = [];
  /** the last Rust-confirmed version (never advanced by local or rejected edits) */
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
  private _state: SyncState = "opening";

  get state(): SyncState {
    return this._state;
  }

  private set state(next: SyncState) {
    if (this._state === next) return;
    this._state = next;
    this.opts.onState?.(next);
  }

  /** the authoritative snapshot that ended in `conflict` (usable for copy / export) */
  conflictInfo: { snapshot: SnapshotMsg; draft: string } | null = null;
  /** a visible terminal failure (recovery exhausted / protocol error), with the draft kept */
  failureInfo: { message: string; draft: string; retryable: boolean } | null = null;
  /** the patch that is out (relative to the last confirmed version) and edits made since (relative to the doc after `sent`) */
  private sent: ChangeSet | null = null;
  queued: ChangeSet | null = null;
  private waiters: Waiter[] = [];
  private nextRequestId = 0;
  private sentRequest: number | null = null;
  private snapshotRequest: number | null = null;
  /** true only after an Agent update transformed the in-flight patch onto the new version */
  private sentTransformed = false;
  /** bounded stale replays per request (each replay is a new request, but only with proof) */
  private staleRetries = 0;
  private recoveryAttempts = 0;
  private awaitingLoad = false;
  private awaitingOpen = false;
  private failureError: SyncError | null = null;
  private cancelTimer: (() => void) | null = null;
  private flushWaiters = new Map<number, () => void>();
  private timeoutMs: number;
  private schedule: (fn: () => void, ms: number) => () => void;

  constructor(
    private post: (o: unknown) => void,
    private onBlocks: () => void,
    private applyRemote: (changes: ChangeSet) => void,
    private onOpen: (m: OpenMsg) => void = () => {},
    private opts: SyncOptions = {},
  ) {
    this.timeoutMs = opts.timeoutMs ?? 8_000;
    this.schedule =
      opts.schedule ??
      ((fn, ms) => {
        const id = setTimeout(fn, ms);
        return () => clearTimeout(id);
      });
    if (typeof window !== "undefined") {
      (window as unknown as { __md: unknown }).__md = { receive: (j: string) => this.receiveJson(j) };
    }
  }

  /** The production ready handshake participates in the same bounded wait as edits and snapshots. */
  start(engine: string): void {
    if (this.state !== "opening" || this.awaitingOpen || this.active) return;
    this.awaitingOpen = true;
    this.inflight = true;
    this.cancelTimerNow();
    this.cancelTimer = this.schedule(() => {
      if (this.awaitingOpen) this.fail(new SyncError("timeout", "Rust 未在限时内回答 ready"));
    }, this.timeoutMs);
    this.post({ t: "ready", protocol: PROTOCOL_VERSION, engine });
  }

  /** A new document: Rust re-parses it and answers with fresh ids. */
  load(text: string): Promise<void> {
    this.active = true;
    this.blocks = [];
    this.confirmedBlocks = [];
    this.queued = null;
    this.sent = null;
    this.inflight = true;
    this.state = "opening";
    this.conflictInfo = null;
    this.failureInfo = null;
    this.sentRequest = null;
    this.snapshotRequest = null;
    this.awaitingLoad = true;
    this.awaitingOpen = false;
    this.failureError = null;
    this.recoveryAttempts = 0;
    this.staleRetries = 0;
    this.sentTransformed = false;
    this.cancelTimerNow();
    this.cancelTimer = this.schedule(() => {
      if (this.state === "opening") {
        this.awaitingLoad = false;
        this.fail(new SyncError("timeout", "Rust 未在限时内回答 load"));
      }
    }, this.timeoutMs);
    this.post({ t: "load", text });
    return this.settled();
  }

  /** Rust gave the document (answer to `ready`): adopt its version and blocks; the editor text is already in place. */
  adopt(m: OpenMsg) {
    if (m.protocol !== PROTOCOL_VERSION) {
      this.fail(new SyncError("unsupported_protocol", `Rust protocol ${m.protocol}, page protocol ${PROTOCOL_VERSION}`));
      return;
    }
    this.active = true;
    this.version = m.version;
    this.hash = m.hash;
    this.len16 = m.len16;
    this.confirmedBlocks = m.blocks;
    this.blocks = m.blocks;
    this.queued = null;
    this.sent = null;
    this.inflight = false;
    this.sentRequest = null;
    this.snapshotRequest = null;
    this.conflictInfo = null;
    this.failureInfo = null;
    this.awaitingLoad = false;
    this.awaitingOpen = false;
    this.failureError = null;
    this.recoveryAttempts = 0;
    this.staleRetries = 0;
    this.sentTransformed = false;
    this.cancelTimerNow();
    this.state = "ready";
    this.onBlocks();
    this.wake();
    this.wakeFlushWaiters();
  }

  /** The editor's document changed (not called while loading). */
  local(changes: ChangeSet) {
    if (!this.active || this.loading || this.state === "closed" || this.state === "failed") return;
    this.blocks = this.mapBlocks(this.blocks, changes);
    this.queued = this.queued ? this.queued.compose(changes) : changes;
    this.onBlocks();
    this.flush();
  }

  /** Stop sending and settle every waiter with a typed error (page teardown / window close). */
  close(reason = "editor closed") {
    if (this.state === "closed") return;
    this.rejectFlushWaiters(reason);
    this.state = "closed";
    this.inflight = false;
    this.queued = null;
    this.sent = null;
    this.cancelTimerNow();
    this.rejectWaiters(new SyncError("closed", reason));
  }

  private fail(error: SyncError) {
    this.rejectFlushWaiters(error.message);
    this.failureError = error;
    this.awaitingOpen = false;
    this.awaitingLoad = false;
    this.failureInfo = { message: error.message, draft: this.draftText(), retryable: false };
    this.state = "failed";
    this.inflight = false;
    this.cancelTimerNow();
    this.rejectWaiters(error);
  }

  private flush() {
    if (this.state !== "ready" || this.inflight || !this.queued) {
      this.wakeFlushWaiters();
      return;
    }
    const q = this.queued;
    this.queued = null;
    this.sent = q;
    this.inflight = true;
    this.state = "sending";
    this.sentRequest = ++this.nextRequestId;
    this.sentTransformed = false;
    this.staleRetries = 0;
    const changes: { from: number; to: number; insert: string }[] = [];
    q.iterChanges((a, b, _c, _d, ins) => changes.push({ from: a, to: b, insert: ins.toString() }));
    this.armTimeout(this.sentRequest);
    this.post({ t: "edit", request_id: this.sentRequest, base: this.version, changes });
  }

  /** A host save barrier: wait until sent/queued are empty and Rust has acknowledged them. */
  private requestFlush(requestId: number, freeze = false) {
    const reply = (error: string | null) =>
      this.post({ t: "flushed", request_id: requestId, version: this.version, error });
    if (this.state === "closed" || this.state === "failed") {
      reply(this.failureError?.message ?? this.state);
      return;
    }
    if (this.state === "conflict" || this.state === "recovering") {
      reply(`页面正在${this.state === "conflict" ? "冲突" : "恢复"}，无法确认保存水位`);
      return;
    }
    this.flushWaiters.set(requestId, () => { if (freeze) this.opts.onInputLock?.(true); reply(null); });
    this.flush();
    this.wakeFlushWaiters();
  }

  private wakeFlushWaiters() {
    if (this.state !== "ready" || this.inflight || this.queued || this.opts.isComposing?.()) return;
    const waiters = [...this.flushWaiters.values()];
    this.flushWaiters.clear();
    for (const wake of waiters) wake();
  }

  compositionEnded() { this.flush(); this.wakeFlushWaiters(); }

  private rejectFlushWaiters(error: string) {
    const waiters = [...this.flushWaiters.keys()];
    this.flushWaiters.clear();
    for (const requestId of waiters) {
      this.post({ t: "flushed", request_id: requestId, version: this.version, error });
    }
  }

  private armTimeout(request: number) {
    this.cancelTimerNow();
    this.cancelTimer = this.schedule(() => {
      this.cancelTimer = null;
      if (this.state !== "sending" && this.state !== "recovering") return;
      if (this.state === "sending" && this.sentRequest !== request) return;
      if (this.state === "recovering" && this.snapshotRequest !== request) return;
      this.enterRecovery(`timed out waiting for Rust (request ${request})`, true);
    }, this.timeoutMs);
  }

  private cancelTimerNow() {
    this.cancelTimer?.();
    this.cancelTimer = null;
  }

  /**
   * Keep the draft and ask for the authoritative text. We only call this when the in-flight patch cannot be proven
   * safe to replay (non-stale rejection, malformed reply, timeout) — never to silently overwrite the author.
   */
  private enterRecovery(reason: string, _fromTimeout: boolean) {
    if (this.state === "closed" || this.state === "failed") return;
    if (this.state !== "recovering") this.recoveryAttempts = 0;
    this.recoveryAttempts += 1;
    if (this.recoveryAttempts > MAX_RECOVERY_ATTEMPTS) {
      this.failRecovery(`连续恢复失败: ${reason}`);
      return;
    }
    this.state = "recovering";
    this.inflight = true;
    this.snapshotRequest = ++this.nextRequestId;
    this.sentRequest = null;
    this.armTimeout(this.snapshotRequest);
    this.agentErrors.push(`recovery: ${reason}`);
    this.post({ t: "snapshot", request_id: this.snapshotRequest });
  }

  /** Recovery exhausted: a visible terminal state, draft kept, waiters settled. */
  private failRecovery(reason: string) {
    this.rejectFlushWaiters(reason);
    this.failureError = new SyncError("timeout", reason);
    this.failureInfo = { message: reason, draft: this.draftText(), retryable: true };
    this.state = "failed";
    this.inflight = false;
    this.snapshotRequest = null;
    this.sentRequest = null;
    this.cancelTimerNow();
    this.rejectWaiters(new SyncError("timeout", reason));
  }

  /** Explicit manual retry after recovery exhausted. The draft (sent + queued) is kept. */
  retrySync(): boolean {
    if (this.state !== "failed" && this.state !== "conflict") return false;
    this.recoveryAttempts = 0;
    this.conflictInfo = null;
    this.failureInfo = null;
    this.failureError = null;
    this.state = "recovering";
    this.inflight = true;
    this.snapshotRequest = ++this.nextRequestId;
    this.sentRequest = null;
    this.armTimeout(this.snapshotRequest);
    this.post({ t: "snapshot", request_id: this.snapshotRequest });
    return true;
  }

  private mapBlocks(blocks: RBlock[], cs: ChangeSet | null): RBlock[] {
    return cs ? blocks.map((b) => ({ ...b, from: cs.mapPos(b.from, -1), to: cs.mapPos(b.to, 1) })) : blocks;
  }

  receiveJson(json: string): void {
    try { this.receive(JSON.parse(json)); }
    catch (error) { this.invalidReply(error); }
  }

  private invalidReply(error: unknown): void {
    if (this.state === "closed" || this.state === "failed") return;
    if (this.awaitingOpen || this.awaitingLoad) this.fail(new SyncError("protocol", String(error)));
    else this.enterRecovery(`invalid Rust reply: ${String(error)}`, false);
  }

  private receive(value: unknown) {
    if (this.state === "closed" || this.state === "failed") return;
    // Identity filtering precedes payload validation: a malformed replay cannot spend the
    // recovery budget of the newer request. It never reaches state mutation.
    const isFlush =
      typeof value === "object" && value !== null && ["flush", "input_lock"].includes(String((value as { t?: unknown }).t));
    if (!isFlush && typeof value === "object" && value !== null && "request_id" in value) {
      const id = value.request_id;
      const expected = this.state === "sending" ? this.sentRequest : this.snapshotRequest;
      if (typeof id === "number" && Number.isSafeInteger(id) && id >= 0 && id <= this.nextRequestId && id !== expected) return;
    }
    try { this.receiveValidated(decodeOutbound(value)); }
    catch (error) { this.invalidReply(error); }
  }

  private receiveValidated(m: OutMsg) {
    if (this.state === "closed" || this.state === "failed") return;
    if (m.t === "input_lock") { if (!m.locked) this.flushWaiters.clear(); this.opts.onInputLock?.(m.locked); return; }
    if (m.t === "flush") {
      this.requestFlush(m.request_id, m.freeze);
      return;
    }
    if (m.t === "open") {
      if (!this.awaitingOpen) return;
      const open = m as unknown as OpenMsg;
      if (open.protocol !== PROTOCOL_VERSION) {
        this.fail(new SyncError("unsupported_protocol", `Rust protocol ${open.protocol}, page protocol ${PROTOCOL_VERSION}`));
        return;
      }
      this.adopt(open);
      this.onOpen(open);
      return;
    }
    // Only the identity currently being awaited may change state. In particular, after recovery
    // clears sentRequest, an old edit rejection must not consume the snapshot's retry budget.
    const requestId = "request_id" in m ? m.request_id : undefined;
    const expectedRequest = this.state === "sending" ? this.sentRequest : this.snapshotRequest;
    if (requestId !== undefined && requestId !== expectedRequest && requestId <= this.nextRequestId) return;
    if (m.t === "snapshot") {
      if (this.state !== "recovering" || m.request_id !== this.snapshotRequest) return;
      this.cancelTimerNow();
      this.inflight = false;
      this.snapshotRequest = null;
      // Prepare the conflict data BEFORE notifying: the UI reads conflictInfo inside onState.
      this.conflictInfo = {
        snapshot: m as unknown as SnapshotMsg,
        draft: this.opts.getText?.() ?? "",
      };
      this.state = "conflict";
      this.rejectWaiters(new SyncError("conflict", "同步失败,需要作者选择保留本地草稿还是采用 Rust 版本"));
      return;
    }
    if (m.t === "update") {
      // An Agent patch, relative to the last version we confirmed. Our own unconfirmed edits (the one that is out
      // and the ones queued behind it) happened concurrently: transform both ways so everybody ends in the same text.
      if (this.state === "recovering" || this.state === "conflict") {
        this.agentErrors.push("Agent update ignored while the editor is recovering");
        return;
      }
      const delta = m.changes.reduce((a, c) => a + c.insert.length - (c.to - c.from), 0);
      if (m.version <= this.version) return; // duplicate Agent update
      if (m.version !== this.version + 1 || this.len16 + delta !== m.len16) {
        this.enterRecovery("Agent update version/length discontinuity", false);
        return;
      }
      let R = ChangeSet.of(m.changes ?? [], (m.len16 ?? 0) - delta);
      if (this.sent || this.queued) this.remoteTransformed++;
      if (this.sent) {
        const A = this.sent, R0 = R;
        this.sent = A.map(R0, true); // our typing goes before the Agent's insertion at the same place
        R = R0.map(A, false);
        this.sentTransformed = true;
      }
      if (this.queued) {
        const Q = this.queued, R0 = R;
        this.queued = Q.map(R0, true);
        R = R0.map(Q, false);
      }
      this.applyRemote(R); // R now applies to the editor's current document
      this.remoteApplied++;
      this.version = m.version ?? this.version;
      this.hash = m.hash || (this.opts.computeHash && this.opts.getText ? fnv(this.opts.getText()) : this.hash);
      this.len16 = m.len16 ?? this.len16;
      this.confirmedBlocks = m.blocks ?? [];
      this.blocks = this.mapBlocks(this.mapBlocks(this.confirmedBlocks, this.sent), this.queued);
      this.onBlocks();
      // `sent` (if any) is now transformed to the new version. Rust will refuse the old base; a stale reject
      // below re-sends it safely, because we can prove the transform covers every change in between.
      this.wake();
      return;
    }
    if (m.t === "agent_error") {
      this.agentErrors.push(m.reason ?? "?");
      return;
    }
    if (m.t === "incremental") {
      if (this.state === "recovering" || this.state === "conflict") return;
      if (!this.sent || this.state !== "sending") {
        this.enterRecovery("incremental result without an in-flight edit", false);
        return;
      }
      if (m.request_id !== this.sentRequest || m.base_version !== this.version || m.version !== this.version + 1) {
        this.enterRecovery(`incremental ids/base mismatch (${m.request_id}/${m.base_version}/${m.version})`, false);
        return;
      }
      let cs: ChangeSet;
      try {
        cs = ChangeSet.of(m.changes ?? [], this.len16);
      } catch (e) {
        this.enterRecovery(`invalid incremental changes: ${String(e)}`, false);
        return;
      }
      const mapped = this.mapBlocks(this.confirmedBlocks, cs).map((b) => ({ ...b, how: "Exact" }));
      const from = m.removed_from ?? -1;
      const to = m.removed_to ?? -1;
      const nextLen = m.len16 ?? this.len16;
      const fresh = m.blocks ?? [];
      const valid =
        from >= 0 && from <= to && to <= mapped.length &&
        fresh.every((b) => Number.isInteger(b.from) && Number.isInteger(b.to) && b.from >= 0 && b.from <= b.to && b.to <= nextLen) &&
        fresh.every((b, i) => i === 0 || fresh[i - 1].to <= b.from) &&
        (from === 0 || fresh.length === 0 || fresh[0].from >= mapped[from - 1].to) &&
        (to >= mapped.length || fresh.length === 0 || fresh[fresh.length - 1].to <= mapped[to].from);
      // The preserved blocks must stay sorted through the change set.
      const preservedOk =
        mapped.slice(0, from).every((b, i) => i === 0 || mapped[i - 1].to <= b.from) &&
        mapped.slice(to).every((b, i) => i === 0 || mapped[to + i - 1].to <= b.from);
      if (!valid || !preservedOk) {
        this.enterRecovery("incremental result is inconsistent", false);
        return;
      }
      const next = [...mapped.slice(0, from), ...fresh, ...mapped.slice(to)];
      if (new Set(next.map((b) => b.id)).size !== next.length || !next.every((b, i) => i === 0 || next[i - 1].to <= b.from)) {
        this.enterRecovery("incremental result overlaps", false);
        return;
      }
      this.cancelTimerNow();
      this.version = m.version;
      this.hash = m.hash || (this.opts.computeHash && this.opts.getText ? fnv(this.opts.getText()) : this.hash);
      this.len16 = nextLen;
      this.inflight = false;
      this.sent = null;
      this.sentRequest = null;
      this.state = "ready";
      this.confirmedBlocks = next;
      this.blocks = this.mapBlocks(next, this.queued);
      this.onBlocks();
      this.flush();
      this.wake();
      this.wakeFlushWaiters();
      return;
    }
    if (m.t === "blocks") {
      if (this.state === "recovering") return;
      // A load reply is accepted only while the explicit load wait is open. An old ack while idle
      // must never overwrite the confirmed version or the queued draft.
      if (this.sent === null) {
        if (m.request_id !== undefined || !this.awaitingLoad) return;
        this.adoptBlocks(m);
        return;
      }
      if (m.request_id !== this.sentRequest) {
        this.enterRecovery(`unexpected request id ${m.request_id} (sent ${this.sentRequest})`, false);
        return;
      }
      const expected = this.version + 1;
      if ((m.version ?? 0) !== expected) {
        this.enterRecovery(`version ${m.version} after ${this.version}`, false);
        return;
      }
      this.cancelTimerNow();
      this.version = m.version ?? this.version;
      this.hash = m.hash || (this.opts.computeHash && this.opts.getText ? fnv(this.opts.getText()) : this.hash);
      this.len16 = m.len16 ?? this.len16;
      this.inflight = false;
      this.sent = null;
      this.sentRequest = null;
      this.state = "ready";
      this.confirmedBlocks = m.blocks ?? [];
      this.blocks = this.mapBlocks(this.confirmedBlocks, this.queued);
      this.onBlocks();
      this.flush();
      this.wake();
      this.wakeFlushWaiters();
      return;
    }
    if (m.t === "reject") {
      const code = parseCode(m);
      if (this.awaitingOpen || this.awaitingLoad) {
        this.fail(new SyncError(code, m.reason ?? `handshake rejected (${code})`));
        return;
      }
      if (this.state === "conflict" || this.state === "ready") return;
      if (
        this.state === "recovering" && m.request_id !== this.snapshotRequest
      ) {
        return;
      }
      this.rejects.push(m.reason ?? code);
      if (m.request_id !== undefined && this.snapshotRequest !== null && m.request_id === this.snapshotRequest) {
        this.enterRecovery(`snapshot rejected (${code})`, false);
        return;
      }
      if (code === "unauthorized" || code === "unsupported_protocol" || code === "malformed_changes") {
        // Not an edit failure caused by concurrency: keep the draft, but do not spin on automatic retries.
        if (this.sent || this.queued) this.enterRecovery(`edit rejected (${code})`, false);
        return;
      }
      if (code === "stale_base") {
        this.inflight = false;
        if (this.sent && this.sentTransformed && this.staleRetries < 1) {
          // Replay only with proof: an Agent update was received and transformed this patch onto
          // the new version. One bounded replay per request; otherwise ask for a snapshot.
          this.staleRetries += 1;
          this.queued = this.queued ? this.sent.compose(this.queued) : this.sent;
          this.sent = null;
          this.sentRequest = null;
          this.state = "ready";
          this.cancelTimerNow();
          this.flush();
          this.wake();
        } else {
          this.enterRecovery("stale base without a received transform", false);
        }
        return;
      }
      // Non-stale rejection while an edit is out: preserve sent + queued and recover from a snapshot.
      if (this.sent || this.queued) {
        this.enterRecovery(`edit rejected (${code})`, false);
      } else {
        this.inflight = false;
        this.state = "ready";
        this.wake();
      }
    }
  }

  private adoptBlocks(m: Extract<OutMsg, { t: "blocks" }>) {
    this.cancelTimerNow();
    this.version = m.version ?? this.version;
    this.hash = m.hash ?? "";
    this.len16 = m.len16 ?? this.len16;
    this.confirmedBlocks = m.blocks ?? [];
    this.blocks = this.confirmedBlocks;
    this.sent = null;
    this.queued = null;
    this.inflight = false;
    this.sentRequest = null;
    this.snapshotRequest = null;
    this.awaitingLoad = false;
    this.recoveryAttempts = 0;
    this.staleRetries = 0;
    this.sentTransformed = false;
    this.failureInfo = null;
    this.state = "ready";
    this.onBlocks();
    this.wake();
    this.wakeFlushWaiters();
  }

  /** Explicit author choice: take Rust's authoritative text, discarding the local draft after it was exported. */
  resolveWithSnapshot(): boolean {
    const info = this.conflictInfo;
    if (this.state !== "conflict" || !info) return false;
    const open: OpenMsg = {
      t: "open",
      protocol: PROTOCOL_VERSION,
      text: info.snapshot.text,
      version: info.snapshot.version,
      hash: info.snapshot.hash,
      len16: info.snapshot.len16,
      blocks: info.snapshot.blocks,
    };
    this.conflictInfo = null;
    this.confirmedBlocks = info.snapshot.blocks;
    this.queued = null;
    this.sent = null;
    this.inflight = false;
    this.state = "ready";
    this.onOpen(open);
    this.adopt(open);
    return true;
  }

  /** Explicit author choice: commit the local draft as a new edit against the last confirmed version. */
  commitDraft(): boolean {
    const info = this.conflictInfo;
    if (this.state !== "conflict" || !info || !this.opts.getText) return false;
    const draft = this.opts.getText();
    const snapshot = info.snapshot;
    // Commit against the snapshot that the author was shown, not the pre-conflict version.
    this.version = snapshot.version;
    this.len16 = snapshot.len16;
    this.hash = snapshot.hash;
    this.confirmedBlocks = snapshot.blocks;
    this.conflictInfo = null;
    this.queued = null;
    this.sent = ChangeSet.of({ from: 0, to: snapshot.len16, insert: draft }, snapshot.len16);
    this.inflight = true;
    this.state = "sending";
    this.sentRequest = ++this.nextRequestId;
    this.sentTransformed = false;
    this.staleRetries = 0;
    this.armTimeout(this.sentRequest);
    const changes: { from: number; to: number; insert: string }[] = [];
    this.sent.iterChanges((a, b, _c, _d, ins) => changes.push({ from: a, to: b, insert: ins.toString() }));
    this.post({ t: "edit", request_id: this.sentRequest, base: snapshot.version, changes });
    return true;
  }

  private wake() {
    if (this.state === "failed" || this.state === "closed" || this.state === "conflict") return;
    if (!this.inflight && !this.queued) {
      const w = this.waiters;
      this.waiters = [];
      w.forEach((x) => x.resolve());
    }
  }

  private rejectWaiters(error: SyncError) {
    const w = this.waiters;
    this.waiters = [];
    w.forEach((x) => x.reject(error));
  }

  /** Resolves when nothing is in flight or queued; rejects on failed / conflict / closed. */
  settled(): Promise<void> {
    if (this.state === "conflict") {
      return Promise.reject(new SyncError("conflict", "同步冲突待作者选择"));
    }
    if (this.state === "failed") {
      return Promise.reject(this.failureError ?? new SyncError("protocol", "编辑器同步已停止"));
    }
    if (this.state === "closed") {
      return Promise.reject(new SyncError("closed", "编辑器已关闭"));
    }
    if (!this.inflight && !this.queued) return Promise.resolve();
    return new Promise((resolve, reject) => this.waiters.push({ resolve, reject }));
  }

  blockAt(pos: number): RBlock | undefined {
    // blocks are sorted and disjoint; a caret at a block's end still belongs to it
    return this.blocks.find((b) => pos >= b.from && pos <= b.to);
  }

  /** The local text captured for a conflict, if any. Kept so the page can offer copy / export. */
  draftText(): string {
    return this.conflictInfo?.draft ?? this.opts.getText?.() ?? "";
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

/** Exported for the pure sync tests: build a Text from the same lines CodeMirror uses. */
export function textOf(value: string): Text {
  return Text.of(value.split("\n"));
}
