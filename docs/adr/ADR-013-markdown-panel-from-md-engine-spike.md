# ADR-013: A First-Party Markdown Panel from the `md-engine` Spike

- Status: Accepted (2026-10-14; all eight validation items closed, see Implementation status)
- Date: 2026-10-13
- Decision owners: Digger Architecture
- Applies to: desktop shell, panel catalog, editor host, crate layout, web bundle
- Related: ADR-001 (shell and editor host boundary), ADR-011 (bytehost application host), ADR-012 (Markdown text as authoritative content)
- Input: `spike/md-engine` (engine spike, harness T1–T23), `spike/block-identity` (parse + reconcile)

## Context

ADR-012 settled the direction and then validated it in `spike/md-engine`: CodeMirror 6 live
preview is the chapter editor, Rust is authoritative for block boundaries and ids, and the
block-range sync protocol between Rust and the editor was exercised end to end in a real
WKWebView (tests T1–T23, plus a 1,500-race property test on the Rust side). The spike is a
throwaway binary: `src/main.rs` (192 lines) builds one wry child webview over an iced bar,
`src/sync.rs` (486 lines) holds the text, and `host/` is a vite bundle embedded with
`include_str!`.

Meanwhile the product shell exists but has no editor. `crates/desktop` registers three panels
(`digger.info`, `digger.apps`, `digger.usage`), all pure iced; `ProductShell::on_activated` /
`on_closed` are still no-ops outside tests, so no Surface has ever been created in Digger.

This ADR decides how the spike's editor becomes a panel in the product, and — just as
importantly — what it does **not** become yet. ADR-012's full scope (Project database, Commit,
`.md` mirror, block tables, Agent runtime) is not in this slice.

### What bytehost offers, and what it does not

| bytehost piece | Usable for this panel? |
|---|---|
| `bytehost_core::registry::{PanelCatalog, PanelMeta}` | Yes — `PanelMeta::single_surface(id, title)` declares `SurfaceSlot::MAIN` |
| `bytehost_core::surface::{SurfaceKind::Builtin, Surface, SurfaceId}` | Yes — Builtin class, `ReadyPolicy::Handshake`, generation routing |
| `bytehost_desktop::surface::SurfaceExecutor` | Yes — state/routing/push de-duplication |
| `bytehost_desktop::{Layout, Rail}`, `layout::atomic_save_layout` | Yes — the panel joins the saved layout |
| `bytehost_desktop::panel::build_child` | **No** — takes only `(slot, html, rect)`: no `with_ipc_handler`, no `with_initialization_script`, no pinned settings |
| `bytehost_desktop::app_webview::build_app_child` | **No** — Application class: fail-closed on `<id>.localhost:<port>`, nonce-guarded `AppIpc` whitelist (focus/mouseup/zoom only), per-app data store |

So bytehost supplies the **panel machinery** (identity, catalog, layout, Surface lifecycle,
geometry, generation routing) but not a builder for a first-party content host. That gap is the
main thing this ADR decides.

## Decision

### 1. It is a Builtin panel, not a bytehost Application

`digger.markdown` is registered in the product's `PanelCatalog` as a first-party panel with
`SurfaceKind::Builtin`, beside `digger.info` / `digger.apps` / `digger.usage`. It is **not**
installed, supervised or gatewayed through `bytehost-apps`.

ADR-011 §2 is explicit that an installed application receives no Project handle and no access to
authoritative state; ADR-012 puts the editor on the trusted side of that line (Rust owns the
text, the editor is a projection of it). Shipping the editor as an Application would put the
document authority behind a sandbox boundary built for untrusted third-party code, and would
forbid the very bridge (`edit` / `blocks` / `update`) the design depends on.

Consequence accepted: `crates/app-host` is untouched by this ADR, and `digger.markdown` never
appears in the app list.

### 2. Crate layout: the Rust side is a business crate

```text
crates/editor/          # digger-editor  — text, blocks, ids, protocol (NO bytehost-*)
crates/desktop/         # digger-desktop — editor-host builder, panel view, Surface wiring
crates/desktop/web/     # the CodeMirror bundle (generated artifact, see §6)
```

`digger-editor` holds what `spike/md-engine/src/sync.rs` holds today: the document text, the
version counter, block parse + id reconciliation, UTF-16 ↔ byte offset mapping, the FNV-1a text
hash, the Agent plan/step rules (`resolve` at execution time, `expect` precondition, all-or-
nothing rollback), and the typed bridge messages of §5.

Its name matches `BUSINESS_PREFIX` in `scripts/check-host-deps.sh`
(`^digger-(domain|store|…|editor|…)(-.*)?$`), so the existing gate picks it up automatically — no
script change is needed for the crate to be checked. But the gate as written bans only
`bytehost-*`; it would **not** catch `digger-editor` growing an iced or wry dependency, which is
the failure mode that actually matters here (the Rust side must stay a pure library so the harness
and a future `markdown-core` can use it). This ADR therefore extends the gate: for business
crates, ban `bytehost-*` **and** `iced*` **and** `wry`, with the existing `--self-test` mutation
probe covering the new names. `digger-editor` must not depend on any of them.

**Where parse/reconcile live.** `spike/block-identity` is excluded from the workspace and is
contractually deletable, so a production crate may not path-depend on it. ADR-012 §7 wants this
code in a shared `markdown-core` repository, which does not exist yet. Decision: move
`parse.rs` and `reconcile.rs` into `digger-editor::blocks`, keeping the module boundary exactly
where `markdown-core`'s would be, so the later extraction is a move and not a rewrite.

The simulator moves with them, but as **test-only** code: `tests/properties.rs` drives
`simulate.rs`, and those 11 property tests are the real guard on the reconciler (ADR-012 cites
them for every measured figure). Leaving the simulator in a deletable spike would leave the
production reconciler guarded by unit tests alone. So `simulate.rs` and `tests/properties.rs`
move into `digger-editor` behind `#[cfg(test)]` / `tests/`, never in the crate's public surface;
only `bin/report.rs` (the measurement CLI that produced ADR-012's tables) stays in the spike.

New workspace dependencies: `pulldown-cmark` 0.13 (`default-features = false`) and `similar` 2.

### 3. Digger writes its own editor-host builder

Neither bytehost builder fits (§"What bytehost offers"), and ADR-001 §2 already requires the
editor host to have its own builder, not shared with the application webview. So
`crates/desktop` gains one first-party builder, modelled on Dozer's first-party webview path:

1. every wry setting is pinned explicitly and a test asserts the pinned values by reading the
   builder's own production source (the technique `spike/editor-host` used for
   `build_app_child`, so the probe measures the real builder and not a copy):
   `devtools(false)`, `download_started_handler → false`, `new_window_req_handler → Deny`,
   `navigation_handler` allowing only the embedded document, `allow_link_preview(false)`.
   **Clipboard is deliberately left enabled**, unlike `build_app_child`, which pins it off:
   paste is a core authoring action here, and this host runs first-party code. The difference is
   recorded in the pin test so it cannot drift unnoticed;
2. content is loaded from the binary-embedded bundle only — never from a `file://` path, never
   from a URL, and no custom protocol is registered in this slice;
3. one `with_ipc_handler` carries the bridge of §5, and one `with_initialization_script` carries
   the handshake nonce (the page cannot forge messages without it);
4. the webview handle is keyed by `SurfaceSlot` and lives in a main-thread table, reached only
   through `iced::window::run` — the same discipline `bytehost_desktop::panel` uses, so
   create/destroy/resize stay on the window thread.

This builder must not be reachable from the application-webview path, and
`bytehost_desktop::build_app_child` must not be used for the editor; a test asserts the two
handle tables never share a slot.

**Upstream follow-up (not a blocker).** Ask bytehost for a Builtin-class
`build_builtin_child(window, spec, on_ipc, on_ready, wrap)` with the same pinning discipline as
`build_app_child`, so Dozer and Digger stop each owning a copy. Until it lands, Digger's builder
is the product's own and stays in the host adapter layer.

### 4. Surface identity, geometry and lifecycle

The panel is created and destroyed by Rust, one instance per open panel (ADR-001 §4):

- identity is `(PanelInstanceId, SurfaceSlot::MAIN, generation)`; every uplink message is routed
  through `SurfaceExecutor::route` and a stale generation is dropped, never applied;
- `ReadyPolicy::Handshake`: the page posts `ready`, Rust answers with `open` (§5). Nothing is
  pushed before Ready;
- geometry is the panel's logical rect **inset by the iced bar height** (the spike's
  `TOP_H = 56.0`), because a wry child always renders above iced content (ADR-001 §3). The bar
  carries the document state, the sync/version readout and the Agent-stand-in controls;
- switching away from the panel **hides** the Surface (`SurfaceEvent::Hidden`) rather than
  leaving an invisible webview on top of another panel; switching back resumes it. Closing the
  panel destroys it and drops the document;
- window resize recomputes the rect and calls `set_bounds`.

`ProductShell::on_activated` / `on_closed` stop being no-ops: they now drive these transitions.

### 5. Bridge protocol: adopt the tested one, typed and hardened

The wire protocol is the one already validated by T11–T23, unchanged in shape. Positions are
UTF-16 code units; `hash` is FNV-1a over the UTF-8 text; one patch is in flight and later edits
are composed; a stale `base` is refused and re-sent transformed.

```text
page → Rust   ready {}                                             handshake (new)
Rust → page   open  { text, version, hash, len16, blocks }         initial document (new)
page → Rust   edit  { base, changes: [{ from, to, insert }] }      unchanged
Rust → page   blocks { version, hash, len16, blocks: [ … ] }       unchanged
Rust → page   update { version, changes, hash, len16, blocks }     unchanged (Agent patch)
Rust → page   agent_error { reason, stale }                        unchanged (a normal outcome)
Rust → page   reject { reason, version }                           unchanged
```

Three changes from the spike, all hardening:

1. **Typed messages.** `serde_json::Value` + `v["t"]` string matching is replaced by
   `EditorInbound` / `EditorOutbound` enums in `digger-editor`. An unknown or unparsable message
   always answers `reject` — the spike already found that a silently dropped message (a lone
   surrogate in a patch) hangs the editor forever.
2. **`agent` is gated, and off in normal operation.** In the spike the page can post
   `{t:"agent"}` and make Rust mutate the document by block id. In the product that inbound
   message is accepted **only** while the panel runs in the selftest mode of §"Validation" item 4
   (a Rust-side flag, set by the environment, that also selects the harness page). In normal
   operation it is refused like any unknown message: the Agent stand-in is driven by the iced bar
   in Rust, and a webview must not be able to address blocks by id and rewrite them. Gating rather
   than deleting is what lets T19–T23 run against the production builder and bridge instead of a
   parallel test-only one.
3. **Rust offers the document.** The spike's page chose its own corpus and posted it up as
   `load`. In the product, Rust owns the text and pushes it down in `open`; `load` remains
   accepted for the harness's test seeding only and is documented as such.

### 6. The web bundle is a checked-in generated artifact

`host/` (vite + `vite-plugin-singlefile`, ~1,565 lines of TypeScript) moves to
`crates/desktop/web/`, and the built single-file `cm.html` (~513 KB) is **committed** and
embedded with `include_str!`. `scripts/build-editor-web.sh` regenerates it.

The ProseMirror variant (`pm-engine.ts`, `dist-pm/`, ~340 KB) is **dropped**: ADR-012's engine
decision is CodeMirror 6, and the source-preserving ProseMirror variant exists only as the
comparison arm. It stays recoverable from the spike's history.

Costs accepted: `npm` enters the build path for anyone touching the editor page, and a generated
artifact is in Git (its source revision is recorded in a header comment; the build script fails
if the committed bundle does not match a fresh build). The alternative — building at package
time — is listed below.

**Two cross-spike couplings the move has to cut.** The bundle is not self-contained today:

- `host/scripts/make-fixtures.mjs` generates `fixtures.ts` (harness T10, editor parser vs Rust
  parser) by shelling out to the release binary of `spike/parser-agreement/rust`. Once `parse.rs`
  lives in `digger-editor`, the generator must be repointed at a `digger-editor` fixture bin
  instead, so T10 conformance-tests the **production** parser rather than a spike's copy of it.
- `spike/parser-agreement/node/footnote.mjs` re-exports `../../md-engine/host/src/footnote.ts` —
  the Lezer footnote extension is deliberately one file, shared by the editor and the conformance
  run (ADR-012: "it tests exactly what the editor uses"). After the move that import must point at
  `crates/desktop/web/src/footnote.ts`, and `spike/parser-agreement/run-all.sh` stays the
  conformance gate. The file's new home is in a product crate; the spike keeps only a reference to
  it, which is the right direction of dependency but must not be silently broken.

### 7. What this slice deliberately does not do

Recorded so nothing downstream assumes it:

- **No persistence.** No Project, no `project.db`, no Commit, no ChangeSet, no `.md` mirror, no
  Outbox. The document is an embedded sample chapter held in memory; closing the panel discards
  it. ADR-003 and ADR-012 §5 are untouched.
- **Block ids are ephemeral.** They are stable across edits within one panel session, which is
  what T11–T23 test, but they are not written to any table and must not be referenced by
  anything that outlives the session.
- **No Agent runtime.** The `replace_block` / `insert_after` / `append` operations are a stand-in
  driven by a button in the iced bar, exactly as in the spike. No model, no plan source, no
  `block_agent_history`.
- **No assets, no images beyond the sample's, no outline panel, no export, no source mode.**
- **The panel must say so.** The iced bar shows "未持久化 / 示例章节" — the product does not
  present an in-memory slice as a saved document.

## Alternatives considered

1. **Ship the editor as a bytehost Application.** Rejected: §1 — no Project handle, no bridge,
   untrusted-class webview for trusted first-party code.
2. **Use `bytehost_desktop::panel::build_child` as-is.** Rejected: no IPC handler means no
   uplink at all, no initialization script means no handshake nonce, and unpinned wry defaults
   contradict ADR-001 §2 ("wry defaults are not treated as safe").
3. **Wait for `markdown-core` to be extracted before touching the product.** Rejected: the
   extraction is a repository-boundary decision that does not need to precede a working panel,
   and §2 keeps the seam where the extraction will cut.
4. **Path-depend on `spike/block-identity` from `digger-editor`.** Rejected: spikes are excluded
   from the workspace and documented as deletable at any time; a production crate may not rest on
   one.
5. **Build the web bundle at package time instead of committing it.** Kept as an option (§6):
   it removes a 513 KB generated file from Git, at the cost of requiring `npm ci` in every build
   and packaging path, and making "the bundle I tested is the bundle I shipped" harder to assert.
6. **Keep both engines and switch at runtime.** Rejected: the ProseMirror arm doubles the bundle
   and the test surface for a comparison that ADR-012 already settled.

## Consequences

Positive:

- The editor stops being a spike and becomes reachable in the product shell, with the panel
  catalog, layout persistence and Surface lifecycle doing the work they were built for.
- Digger's first real Surface exists, so ADR-001 §3's layering rule and the push-down geometry
  become exercised code rather than documentation.
- `digger-editor` becomes the gate's second business crate (today it examines one, `digger-store`),
  and extending the gate to ban iced/wry makes the "pure Rust library" rule enforceable rather than
  a review habit.
- T1–T23 keep guarding behaviour, now on the production path.

Negative / costs:

- Digger gains a first-party wry webview and an `npm` toolchain dependency in the editor path.
- The binary grows by the embedded bundle (~513 KB).
- A second webview builder exists beside bytehost's application builder; the two must not drift,
  and the upstream `build_builtin_child` ask is the way to end that duplication.
- The panel looks like an editor but saves nothing. If the "未持久化" label is weak, this is the
  most likely way to mislead an author.

## Validation required before Accepted

1. **Gate:** `scripts/check-host-deps.sh` reports two business crates with `digger-editor`
   present, `cargo tree -p digger-editor` shows no `bytehost-*`, no iced and no wry, and
   `--self-test` fails on an injected iced dependency as well as on an injected `bytehost-*` one.
2. **Rust tests move green:** the 10 `sync.rs` tests, the 13 `parse` / `reconcile` unit tests and
   the 11 simulator property tests all pass in `digger-editor` — including the 1,500-race test and
   `results_are_deterministic`.
3. **Pinned settings:** the builder's pin-assertion test fails when any one of
   devtools/downloads/new-window/navigation is relaxed, and also fails if clipboard is pinned off
   (mutation check on both directions).
4. **Harness on the product path:** `scripts/run-editor-selftest.sh` boots the shipped `digger`
   binary with the harness page and reports the same 23 checks passing, exiting non-zero on any
   failure. The harness runs against the production builder and the production bridge — the
   lesson ADR-012 recorded (an automated check that calls a command directly does not prove the
   user can reach it) applies to the builder too.
5. **Surface lifecycle:** switching panels hides the webview (verified by a test that no slot
   stays visible while another panel is active), resizing follows the bar inset, and closing the
   panel destroys the handle (`handle_count()` returns to its prior value).
6. **Generation routing:** a message arriving from an old generation is dropped, with a test.
7. **Conformance still runs after the move:** `spike/parser-agreement/run-all.sh` passes with
   `footnote.ts` imported from its new product home, and `fixtures.ts` regenerated from
   `digger-editor` is byte-identical to the committed file (so T10 tests the production parser).
8. **Manual:** the sample chapter opens, Chinese and Japanese IME composition works with the Rust
   sync attached, the Agent-stand-in button rewrites a block and Cmd-Z takes exactly that back —
   the same observations ADR-012 recorded for the spike, repeated in the product window.

## Open questions — resolved

- **The harness ships inside the production bundle**, inert unless `DIGGER_EDITOR_SELFTEST` is set
  (the page reads `window.__DIGGER__.mode`, which only the host's initialization script sets). The
  page that is tested is the page that ships. Decided 2026-10-14.
- **Opening an arbitrary `.md` file stays out of this slice**: without the ADR-012 §5 mirror it has no
  import path and no honest save. Decided 2026-10-14.
- **Default layout: a third rail** (`digger.markdown` after `digger.info` and `digger.apps`), plus an
  "编辑器" tab; the panel does not open on launch. Decided 2026-10-14.
- **`markdown-core` extraction** (whether `digger-editor::blocks` is deleted or re-exported) is still
  open and does not block anything here.

## Implementation status (2026-10-14)

Landed in `crates/editor` (`digger-editor`), `crates/desktop/src/markdown/` and `crates/desktop/web/`.

Deviations from the text above, all small:

- **`allow_link_preview(false)` cannot be pinned**: wry 0.55.1's `WebViewBuilder` has no such option.
  Every other pin in §3 is in place. Clipboard is `with_clipboard(true)`; on macOS wry has no switch at
  all (always on), so the call only records the intent. Revisit when wry exposes the option.
- **The product shell has no `on_activated` / `on_closed` Surface routing yet** (`apps` and `usage`
  already work the same way): the panel follows the shell's existing pattern, a tab plus a per-panel
  `sync_native`, instead of `SurfaceExecutor`. Generation routing is done inside the panel (§4): every
  upstream message carries the webview's generation and a stale one is dropped. Moving all panels onto
  `SurfaceExecutor` is a shell-wide change and is not part of this slice.
- **The Agent stand-in is driven by a button in the iced bar** through `Bridge::stand_in`, as §5
  intends; the page cannot trigger it.
- **`digger-editor` also holds `Bridge`**, the window-free "raw page message in, reply out" state machine
  (handshake, nonce, selftest gating), so those rules are tested without a window.

Validation status:

| # | Item | Status |
|---|------|--------|
| 1 | Gate | Done: `check-host-deps.sh` bans `bytehost-*`, `iced*`, `wry`; `--self-test` injects each |
| 2 | Rust tests move green | Done: 34 unit + 11 property tests in `digger-editor` (more than the 10 + 13 + 11 counted above, because `Doc` gained incremental re-parse and persistence tests) |
| 3 | Pinned settings | Done: tests read `host.rs`'s production source; relaxing any pin, or pinning the clipboard off, fails |
| 4 | Harness on the product path | Done: `scripts/run-editor-selftest.sh`, 24 of 24 on the shipped binary (the 23 checks plus T17b for the nonce) |
| 5 | Surface lifecycle | Done: `scripts/run-editor-selftest.sh lifecycle` drives the real shell (switch tabs, resize while shown and while hidden, close) and reads the real NSView after each step: 8 of 8. Mutation-checked: not hiding, not resizing, not destroying each fail the matching checks |
| 6 | Generation routing | Done: `a_message_from_an_old_generation_is_dropped` |
| 7 | Conformance after the move | Done: `run-all.sh` passes with `footnote.ts` from `crates/desktop/web`; `make-fixtures.mjs --check` is byte-identical to the committed `fixtures.ts` |
| 8 | Manual (IME, Agent button, Cmd-Z) | Done 2026-10-14: input-method composition checked by hand in the product window and reported fine |

All eight items are closed, so the status is **Accepted**.

Found while writing item 5: wry 0.55.1's `WebView::bounds()` getter does not handle a flipped parent view (its setter does), so it reports a wrong y. The lifecycle probe reads the NSView frame itself.
