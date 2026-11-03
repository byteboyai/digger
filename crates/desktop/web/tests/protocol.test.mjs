// The page side of the shared Rust/TypeScript protocol contract (crates/editor/tests/protocol-fixtures.json).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { ChangeSet } from "@codemirror/state";
import { RustSync, PROTOCOL_VERSION, decodeOutbound } from "../.test-dist/src/rust-sync.js";

const fixtures = JSON.parse(
  readFileSync(new URL("../../../editor/tests/protocol-fixtures.json", import.meta.url), "utf8"),
);

test("the fixture protocol version matches the page", () => {
  assert.equal(fixtures.protocol_version, PROTOCOL_VERSION);
});

test("every error code in the fixture is a snake_case wire value", () => {
  for (const code of fixtures.error_codes) {
    assert.equal(typeof code, "string");
    assert.match(code, /^[a-z_]+$/);
  }
});

test("the shared reject fixture drives the same state machine branch as Rust", () => {
  const posts = [];
  const timers = [];
  const sync = new RustSync((o) => posts.push(o), () => {}, () => {}, () => {}, {
    schedule: (fn) => { timers.push(fn); return () => {}; },
  });
  sync.adopt({ t: "open", protocol: 2, text: "abcd", version: 1, hash: "h", len16: 4, blocks: [] });
  sync.local(ChangeSet.of({ from: 0, to: 0, insert: "x" }, 4));
  const reject = fixtures.outbound.find((c) => c.name === "stale-reject").json;
  sync.receive(reject);
  // This fixture has no received update chain proving a safe transform. It must recover,
  // rather than replay an edit on the same stale base.
  assert.equal(sync.state, "recovering");
  assert.equal(posts[posts.length - 1].t, "snapshot");
  sync.close("test done");
});


test("every shared outbound fixture passes the production decoder", () => {
  for (const fixture of fixtures.outbound) assert.equal(decodeOutbound(fixture.json).t, fixture.json.t, fixture.name);
});

test("the shared incremental contract replaces the specified old interval", () => {
  const fixture = fixtures.outbound.find(c => c.name === "incremental-with-request-id").json;
  const sync = new RustSync(()=>{}, ()=>{}, ()=>{}, ()=>{}, {schedule:()=>()=>{}});
  sync.adopt({t:"open",protocol:2,text:"abcd",version:1,hash:"",len16:4,blocks:[{...fixture.blocks[0],how:"Fresh",to:4,head:"abcd"}]});
  sync.nextRequestId = 6;
  sync.local(ChangeSet.of({from:2,to:2,insert:"x"},4));
  sync.receive(fixture);
  assert.equal(sync.state,"ready");
  assert.equal(sync.version,2);
  assert.deepEqual(sync.blocks,fixture.blocks);
});
