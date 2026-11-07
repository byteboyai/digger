// Pure, window-free tests for the sync state machine. They compile `src/rust-sync.ts` with tsc and run on
// Node's built-in test runner; no browser or webview is involved.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ChangeSet } from "@codemirror/state";
import { RustSync, SyncError } from "../.test-dist/src/rust-sync.js";

function openText(sync, text = "abcd", version = 1, protocol = 2) {
  sync.adopt({
    t: "open",
    protocol,
    text,
    version,
    hash: "h",
    len16: text.length,
    blocks: [{ id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to: text.length, head: text }],
  });
}

function harness(options = {}) {
  const posts = [];
  const blocks = [];
  const opened = [];
  const timers = [];
  const sync = new RustSync(
    (o) => posts.push(o),
    () => blocks.push(sync.blocks),
    () => {},
    (m) => opened.push(m),
    {
      getText: () => "local draft",
      timeoutMs: 50,
      schedule: (fn) => {
        const timer = { fn, cancelled: false };
        timers.push(timer);
        return () => { timer.cancelled = true; };
      },
      ...options,
    },
  );
  const fireLast = () => {
    const live = timers.filter((t) => !t.cancelled);
    const t = live[live.length - 1];
    assert.ok(t, "a timer is armed");
    t.fn();
  };
  return { sync, posts, blocks, opened, timers, fireLast };
}

const insert = (text, from, to, insert) => ChangeSet.of({ from, to, insert }, text.length);
const baseBlocks = (to) => [{ id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to, head: "x" }];

test("a non-stale rejection keeps sent and queued and asks for a snapshot", async () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.local(insert("abcdx", 0, 0, "y"));
  assert.equal(posts[0].t, "edit");
  assert.equal(posts[0].base, 1);

  sync.receive({ t: "reject", code: "internal", reason: "boom", request_id: posts[0].request_id });
  assert.equal(sync.state, "recovering");
  assert.equal(posts[1].t, "snapshot", "recovery asks Rust for the authoritative text");
  assert.equal(sync.queued !== null, true, "the queued draft is preserved");

  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUST",
    version: 7,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  assert.equal(sync.state, "conflict");
  assert.equal(sync.conflictInfo.snapshot.text, "RUST");
  assert.match(sync.conflictInfo.draft, /draft/);
  await assert.rejects(sync.settled(), (e) => e instanceof SyncError && e.code === "conflict");
});

test("a stale base without a received transform asks for a snapshot instead of replaying", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({ t: "reject", code: "stale_base", reason: "StaleBase", request_id: posts[0].request_id });
  assert.equal(sync.state, "recovering");
  assert.equal(posts[1].t, "snapshot");
});

test("a stale base replays once when an Agent update transformed the patch", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  // Agent update gives us a complete transform of the in-flight patch onto version 2.
  sync.receive({
    t: "update",
    origin: "agent",
    changes: [{ from: 0, to: 0, insert: "z" }],
    version: 2,
    hash: "h2",
    len16: 5,
    blocks: baseBlocks(5),
  });
  sync.receive({ t: "reject", code: "stale_base", reason: "StaleBase", request_id: posts[0].request_id });
  assert.equal(sync.state, "sending");
  assert.equal(posts[posts.length - 1].t, "edit");
  assert.equal(posts[posts.length - 1].base, 2);
  assert.equal(posts[posts.length - 1].request_id > posts[0].request_id, true);
});

test("a second stale rejection after one replay stops automatic retries", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({
    t: "update", origin: "agent", changes: [{ from: 0, to: 0, insert: "z" }],
    version: 2, hash: "h2", len16: 5, blocks: baseBlocks(5),
  });
  sync.receive({ t: "reject", code: "stale_base", reason: "StaleBase", request_id: posts[0].request_id });
  const replay = posts[posts.length - 1];
  sync.receive({ t: "reject", code: "stale_base", reason: "StaleBase", request_id: replay.request_id });
  assert.equal(sync.state, "recovering");
  assert.equal(posts[posts.length - 1].t, "snapshot");
});

test("an ack advances the version, clears sent and flushes the queued patch", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.local(insert("abcdx", 0, 0, "y"));
  sync.receive({
    t: "blocks",
    request_id: posts[0].request_id,
    version: 2,
    hash: "h2",
    len16: 6,
    blocks: [{ id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to: 5, head: "abcdx" }],
  });
  assert.equal(sync.version, 2);
  assert.equal(posts[1].t, "edit");
  assert.equal(posts[1].base, 2);
  sync.receive({
    t: "blocks",
    request_id: posts[1].request_id,
    version: 3,
    hash: "h3",
    len16: 7,
    blocks: [{ id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to: 7, head: "yabcdx" }],
  });
  assert.equal(sync.state, "ready");
  assert.equal(sync.inflight, false);
  assert.equal(sync.queued, null);
  return sync.settled();
});

test("an out-of-order or duplicate ack never corrupts the confirmed version", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  // An ack older than the in-flight request is ignored: the current request is still out.
  sync.receive({ t: "blocks", request_id: 0, version: 9, hash: "bad", len16: 99, blocks: [] });
  assert.equal(sync.state, "sending");
  assert.equal(sync.version, 1);
  // A future / unknown ack cannot be proven safe: stop and recover instead of jumping to version 9.
  sync.receive({ t: "blocks", request_id: 999, version: 9, hash: "bad", len16: 99, blocks: [] });
  assert.equal(sync.state, "recovering");
  assert.equal(sync.version, 1);
  assert.equal(posts[posts.length - 1].t, "snapshot");
});

test("a timeout is bounded and starts recovery without losing the draft", async () => {
  const { sync, posts, fireLast } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  fireLast();
  assert.equal(sync.state, "recovering");
  assert.equal(posts[1].t, "snapshot");
  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUST",
    version: 2,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  assert.equal(sync.state, "conflict");
  await assert.rejects(sync.settled(), (e) => e.code === "conflict");
});

test("closing settles every waiter with a typed error", async () => {
  const { sync } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const waiting = sync.settled();
  sync.close("window gone");
  assert.equal(sync.state, "closed");
  await assert.rejects(waiting, (e) => e instanceof SyncError && e.code === "closed");
});

test("resolving a conflict with the snapshot adopts Rust's text through onOpen", () => {
  const { sync, posts, opened } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({ t: "reject", code: "internal", reason: "x", request_id: posts[0].request_id });
  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUST",
    version: 7,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  assert.equal(sync.resolveWithSnapshot(), true);
  assert.equal(sync.state, "ready");
  assert.equal(sync.version, 7);
  assert.equal(sync.conflictInfo, null);
  assert.equal(opened.length, 1);
  assert.equal(opened[0].text, "RUST");
});

test("committing the draft after a conflict sends one whole-document edit", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({ t: "reject", code: "internal", reason: "x", request_id: posts[0].request_id });
  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUSTVERS",
    version: 7,
    hash: "h",
    len16: 8,
    blocks: baseBlocks(8),
  });
  assert.equal(sync.commitDraft(), true);
  const edit = posts[posts.length - 1];
  assert.equal(edit.t, "edit");
  assert.equal(edit.base, 7, "the draft is applied against the recovered snapshot");
  assert.deepEqual(edit.changes, [{ from: 0, to: 8, insert: "local draft" }]);
  assert.equal(sync.state, "sending");
});

test("continuous snapshot timeouts end in a typed terminal state with the draft kept", async () => {
  const { sync, posts, fireLast } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const waiting = sync.settled();
  // edit timeout -> snapshot request
  fireLast();
  assert.equal(sync.state, "recovering");
  // snapshot timeout -> one bounded retry
  fireLast();
  assert.equal(sync.state, "recovering");
  // second snapshot timeout -> failed, no more requests
  const snapshots = posts.filter((p) => p.t === "snapshot").length;
  fireLast();
  assert.equal(sync.state, "failed");
  assert.equal(posts.filter((p) => p.t === "snapshot").length, snapshots);
  await assert.rejects(waiting, (e) => e instanceof SyncError && e.code === "timeout");
  assert.equal(sync.draftText(), "local draft");
  assert.equal(sync.retrySync(), true, "manual retry is available");
  assert.equal(sync.state, "recovering");
});

test("a late full acknowledgement while idle cannot move the version backwards", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const first = posts[0];
  sync.receive({
    t: "blocks", request_id: first.request_id, version: 2, hash: "", len16: 5, blocks: baseBlocks(5),
  });
  sync.local(insert("abcdx", 0, 0, "y"));
  const second = posts[1];
  sync.receive({
    t: "blocks", request_id: second.request_id, version: 3, hash: "", len16: 6, blocks: baseBlocks(6),
  });
  assert.equal(sync.version, 3);
  // Replay the first acknowledgement now that nothing is in flight.
  sync.receive({
    t: "blocks", request_id: first.request_id, version: 2, hash: "", len16: 5, blocks: baseBlocks(5),
  });
  assert.equal(sync.version, 3, "an old ack is ignored");
  assert.equal(sync.len16, 6);
  assert.equal(sync.blocks[0].to, 6);
});

test("a protocol mismatch fails closed and never sends edits", async () => {
  const { sync, posts } = harness();
  openText(sync, "abcd", 1, 1);
  assert.equal(sync.state, "failed");
  sync.local(insert("abcd", 4, 4, "x"));
  assert.equal(posts.length, 0);
  await assert.rejects(sync.settled(), (e) => e.code === "unsupported_protocol");
});

test("a snapshot reply is read-only and does not apply remote changes", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({ t: "reject", code: "internal", reason: "x", request_id: posts[0].request_id });
  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUST",
    version: 7,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  assert.equal(sync.version, 1, "the snapshot is not adopted automatically");
  assert.equal(sync.conflictInfo.snapshot.version, 7);
});

test("the first conflict notification already carries the snapshot and draft", () => {
  const seen = [];
  let sync;
  const { sync: s, posts } = harness({
    onState: (state) => seen.push({ state, hasConflict: !!sync?.conflictInfo, draft: sync?.conflictInfo?.draft }),
  });
  sync = s;
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({ t: "reject", code: "internal", reason: "x", request_id: posts[0].request_id });
  sync.receive({
    t: "snapshot",
    request_id: posts[1].request_id,
    text: "RUST",
    version: 7,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  const first = seen.find((x) => x.state === "conflict");
  assert.ok(first, "conflict was notified");
  assert.equal(first.hasConflict, true, "conflictInfo is ready when onState runs");
  assert.equal(first.draft, "local draft");
});

test("an incremental result replaces the announced block range and maps preserved blocks", () => {
  const { sync, posts } = harness();
  const base = [
    { id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to: 4, head: "abcd" },
    { id: "2", kind: "paragraph", level: null, title: null, how: "Exact", from: 6, to: 10, head: "efgh" },
  ];
  sync.adopt({ t: "open", protocol: 2, text: "abcd\n\nefgh", version: 1, hash: "h", len16: 10, blocks: base });
  sync.local(insert("abcd\n\nefgh", 1, 1, "x"));
  sync.receive({
    t: "incremental",
    request_id: posts[0].request_id,
    base_version: 1,
    version: 2,
    hash: "h2",
    len16: 11,
    changes: [{ from: 1, to: 1, insert: "x" }],
    removed_from: 0,
    removed_to: 1,
    blocks: [{ id: "1", kind: "paragraph", level: null, title: null, how: "Exact", from: 0, to: 5, head: "axbcd" }],
  });
  assert.equal(sync.state, "ready");
  assert.equal(sync.version, 2);
  assert.deepEqual(sync.blocks.map((b) => `${b.id}:${b.from}-${b.to}`), ["1:0-5", "2:7-11"]);
});

test("an inconsistent incremental result stops and recovers instead of corrupting ranges", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive({
    t: "incremental",
    request_id: posts[0].request_id,
    base_version: 1,
    version: 2,
    hash: "h2",
    len16: 5,
    changes: [{ from: 4, to: 4, insert: "x" }],
    removed_from: 5,
    removed_to: 9,
    blocks: [],
  });
  assert.equal(sync.state, "recovering");
  assert.equal(posts[posts.length - 1].t, "snapshot");
});

test("old edit rejections cannot consume the current snapshot recovery budget", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const edit = posts[0];
  const reject = { t: "reject", code: "internal", request_id: edit.request_id };
  sync.receive(reject);
  const snapshot = posts.at(-1);
  for (let i = 0; i < 5; i++) sync.receive(reject);
  assert.equal(sync.state, "recovering");
  assert.equal(posts.length, 2, "old rejections do not create another snapshot");
  sync.receive({ t: "snapshot", request_id: snapshot.request_id, text: "abcd", version: 1, hash: "h", len16: 4, blocks: baseBlocks(4) });
  assert.equal(sync.state, "conflict");
  sync.receive(reject);
  assert.equal(sync.state, "conflict", "old rejections cannot dismiss a conflict");
  sync.close();
});

test("duplicate incremental acknowledgements are ignored while idle and while sending", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const ack = {
    t: "incremental", request_id: posts[0].request_id, base_version: 1, version: 2, hash: "",
    len16: 5, changes: [{ from: 4, to: 4, insert: "x" }], removed_from: 0, removed_to: 1, blocks: baseBlocks(5),
  };
  sync.receive(ack);
  sync.receive(ack);
  assert.equal(sync.state, "ready");
  assert.equal(posts.length, 1);
  sync.local(insert("abcdx", 5, 5, "y"));
  const pending = posts.at(-1);
  sync.receive(ack);
  assert.equal(sync.state, "sending");
  assert.equal(sync.version, 2);
  assert.equal(posts.at(-1), pending);
  sync.close();
});

test("the normal ready handshake times out and rejects all waiters", async () => {
  const { sync, posts, fireLast } = harness();
  sync.start("test-engine");
  assert.deepEqual(posts, [{ t: "ready", protocol: 2, engine: "test-engine" }]);
  const waiting = sync.settled();
  fireLast();
  assert.equal(sync.state, "failed");
  await assert.rejects(waiting, (e) => e.code === "timeout");
  await assert.rejects(sync.settled(), (e) => e.code === "timeout");
  sync.receive({ t: "open", protocol: 2, text: "late", version: 9, len16: 4, hash: "h", blocks: [] });
  assert.equal(sync.state, "failed", "a late open cannot revive a timed-out handshake");
});

test("normal handshake rejections fail visibly even without an edit in flight", async () => {
  for (const code of ["unsupported_protocol", "unauthorized"]) {
    const { sync } = harness();
    sync.start("test-engine");
    const waiting = sync.settled();
    sync.receive({ t: "reject", code, reason: "handshake rejected" });
    assert.equal(sync.state, "failed");
    assert.match(sync.failureInfo.message, /handshake rejected/);
    await assert.rejects(waiting, (e) => e.code === code);
  }
});

test("a successful normal handshake cancels its timeout and late opens preserve edits", async () => {
  const { sync, timers, opened } = harness();
  sync.start("test-engine");
  const waiting = sync.settled();
  const open = { t: "open", protocol: 2, text: "abcd", version: 1, len16: 4, hash: "h", blocks: baseBlocks(4) };
  sync.receive(open);
  await waiting;
  assert.equal(timers[0].cancelled, true);
  sync.local(insert("abcd", 4, 4, "x"));
  sync.receive(open);
  assert.equal(sync.state, "sending");
  assert.equal(opened.length, 1);
  sync.close();
});

test("replayed old replies preserve recovering, conflict, failed and closed states", () => {
  for (const phase of ["recovering", "conflict", "failed", "closed"]) {
    const { sync, posts, fireLast } = harness();
    openText(sync);
    sync.local(insert("abcd", 4, 4, "x"));
    const id = posts[0].request_id;
    sync.receive({ t: "reject", code: "internal", request_id: id });
    if (phase === "conflict") {
      sync.receive({ t: "snapshot", request_id: posts.at(-1).request_id, text: "abcd", version: 1, len16: 4, hash: "h", blocks: baseBlocks(4) });
    } else if (phase === "failed") {
      fireLast(); fireLast();
    } else if (phase === "closed") sync.close();
    assert.equal(sync.state, phase);
    const before = { state: sync.state, version: sync.version, length: sync.len16, blocks: sync.blocks, posts: posts.length };
    for (const t of ["blocks", "incremental", "reject", "snapshot"]) {
      sync.receive({ t, request_id: id, version: 99, len16: 999, code: "internal", blocks: [] });
    }
    assert.deepEqual({ state: sync.state, version: sync.version, length: sync.len16, blocks: sync.blocks, posts: posts.length }, before);
    sync.close();
  }
});


test("invalid wire fields and JSON preserve drafts and converge through bounded recovery", async () => {
  for (const reply of [null, [], {t: "unknown"}, {t: "blocks", version: -1},
      {t: "snapshot", version: 1, text: "x", len16: 2, hash: "", blocks: []},
      {t: "update", version: 2, len16: 4, hash: "", blocks: [], changes: [{from: 5, to: 1, insert: ""}]}]) {
    const {sync, posts, fireLast} = harness();
    openText(sync);
    sync.local(insert("abcd", 4, 4, "x"));
    const waiter = sync.settled();
    sync.receive(reply);
    assert.equal(sync.state, "recovering");
    assert.equal(sync.draftText(), "local draft");
    assert.equal(posts.at(-1).t, "snapshot");
    fireLast(); fireLast();
    await assert.rejects(waiter, SyncError);
    assert.equal(sync.state, "failed");
  }
  const {sync} = harness();
  sync.start("cm");
  const waiter = sync.settled();
  assert.doesNotThrow(() => sync.receiveJson("{invalid"));
  await assert.rejects(waiter, e => e.code === "protocol");
});


test("Agent version gaps recover without applying remote text", () => {
  let applied = 0;
  const posts = [];
  const sync = new RustSync(o=>posts.push(o), ()=>{}, ()=>applied++, ()=>{}, {schedule:()=>()=>{}, getText:()=>"draft"});
  openText(sync);
  sync.receive({t:"update",version:3,hash:"",len16:5,blocks:baseBlocks(5),changes:[{from:4,to:4,insert:"x"}]});
  assert.equal(sync.state,"recovering");
  assert.equal(applied,0);
  assert.equal(sync.draftText(),"draft");
  sync.close();
});

test("a flush barrier reports the confirmed version once sent and queued are empty", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.receive({ t: "flush", request_id: 7 });
  assert.deepEqual(
    posts.at(-1),
    { t: "flushed", request_id: 7, version: 1, error: null },
    "idle flush is immediate",
  );

  sync.local(insert("abcd", 4, 4, "x"));
  const edit = posts.find((p) => p.t === "edit");
  assert.ok(edit);
  sync.receive({ t: "flush", request_id: 8 });
  assert.equal(
    posts.filter((p) => p.t === "flushed" && p.request_id === 8).length,
    0,
    "flush must wait for the in-flight edit acknowledgement",
  );
  sync.receive({
    t: "blocks",
    request_id: edit.request_id,
    version: 2,
    hash: "h",
    len16: 5,
    blocks: baseBlocks(5),
  });
  const flushed = posts.find((p) => p.t === "flushed" && p.request_id === 8);
  assert.ok(flushed, "the barrier is released after the ack");
  assert.equal(flushed.version, 2);
  assert.equal(flushed.error, null);
});

test("a flush barrier reports an error instead of a save watermark during conflict", () => {
  const { sync, posts } = harness();
  openText(sync);
  sync.local(insert("abcd", 4, 4, "x"));
  const edit = posts.find((p) => p.t === "edit");
  sync.receive({ t: "reject", code: "internal", reason: "boom", request_id: edit.request_id });
  const snapshot = posts.find((p) => p.t === "snapshot");
  sync.receive({
    t: "snapshot",
    request_id: snapshot.request_id,
    text: "RUST",
    version: 7,
    hash: "h",
    len16: 4,
    blocks: [],
  });
  assert.equal(sync.state, "conflict");
  sync.receive({ t: "flush", request_id: 9 });
  const flushed = posts.find((p) => p.t === "flushed" && p.request_id === 9);
  assert.ok(flushed);
  assert.match(flushed.error, /冲突/);
});

test("a transition flush waits for IME to finish before freezing the input", () => {
  let composing = true;
  const locks = [];
  const { sync, posts } = harness({ isComposing: () => composing, onInputLock: locked => locks.push(locked) });
  openText(sync);
  sync.receive({ t: "flush", request_id: 91, freeze: true });
  assert.equal(posts.filter(p => p.t === "flushed").length, 0);
  assert.deepEqual(locks, []);
  composing = false;
  sync.compositionEnded();
  assert.deepEqual(locks, [true]);
  assert.equal(posts.at(-1).request_id, 91);
  sync.receive({ t: "input_lock", locked: false });
  assert.deepEqual(locks, [true, false]);
});

test("cancelling a transition flush cannot freeze the page after a late IME end", () => {
  let composing = true;
  const locks = [];
  const { sync, posts } = harness({ isComposing: () => composing, onInputLock: locked => locks.push(locked) });
  openText(sync);
  sync.receive({ t: "flush", request_id: 92, freeze: true });
  sync.receive({ t: "input_lock", locked: false });
  composing = false;
  sync.compositionEnded();
  assert.deepEqual(locks, [false]);
  assert.equal(posts.filter(p => p.t === "flushed" && p.request_id === 92).length, 0);
});

test("unsolicited wire rejections do not mutate an idle ready session", () => {
  const { sync, posts } = harness();
  openText(sync, "unchanged", 7);
  const blocks = sync.blocks;
  for (const code of ["stale_base", "bad_position", "malformed_changes", "unauthorized"]) {
    sync.receiveJson(JSON.stringify({ t: "reject", code, reason: "foreign probe", version: 7 }));
    assert.equal(sync.state, "ready");
    assert.equal(sync.version, 7);
    assert.equal(sync.blocks, blocks);
    assert.equal(sync.failureInfo, null);
  }
  assert.equal(posts.length, 0);
  assert.equal(sync.rejects.length, 0);
});
