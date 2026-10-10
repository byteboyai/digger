# ADR-001: iced + wry Desktop Shell and Editor Host Boundary

- Status: Proposed
- Date: 2026-10-10
- Decision owners: Digger Architecture
- Applies to: desktop shell, editor, preview, webview hosts, bridge contract, packaging
- Related: ADR-003 (Project consistency island), ADR-006 (Writing Document Representation), ADR-011 (bytehost application host)

## Context

Digger is a desktop-first, local-first application whose authority boundary is a single Rust process (Technical Architecture §4.1, principle 1). The UI must show Project structure, history, Diff/Result review and Agent activity, and must host a structured rich-text editor.

Dozer, the sibling product, already ships a Rust desktop stack: `iced` 0.14 for the GUI, `wry` child webviews overlaid on the iced window for content that needs a browser engine (CodeMirror editor, JSON tree, HTML preview), and the shared `byteui` and `bytegit` repositories consumed by git tag. Its operating experience, including layering rules and webview hardening, is directly reusable.

Two constraints pull against each other:

- rich-text editing is best served by a mature browser-based editor (Tiptap/ProseMirror), and ADR-006 already fixes it as an *editor adapter*, not the Writing Domain model;
- the Rust process must remain the only authority, and the WebView must stay untrusted relative to Project storage.

Earlier drafts assumed Tauri 2 + React. That choice is withdrawn: it would introduce a second UI stack, a second component library, an IPC command layer and a TypeScript state store, none of which Dozer needs.

## Decision

### 1. Shell: Rust + iced 0.14 + byteui

The desktop application is one Rust binary. All application chrome is iced: Project browser, outline and chapter tree, history, Result/Diff review, governance and approval views, Agent session views, job status, settings.

- Components come from `byteui` (git tag). The iced version must equal byteui's; upgrades move together.
- New elements should reuse byteui's icon-button and tab components. Divergence needs a stated reason.
- UI state is the iced `Model`. It holds Rust-owned, revision-tagged projections plus ephemeral interaction state. Official Domain state is never persisted from UI state.
- The UI calls the Rust application API in-process (Technical Architecture §9.1). No IPC layer exists between UI and runtime. I/O runs on Tokio and returns as messages; `update` and `view` never block.
- Shared git functionality uses `bytegit`; Digger adds no second git wrapper.

### 2. Web content only through wry hosts

wry child webviews are used only where a browser engine is required. There are three host classes, each with its own builder and trust level:

| Host | Content | Trust | Bridge |
|---|---|---|---|
| Editor host | Tiptap/ProseMirror bundle for one document | untrusted relative to Project storage; first-party code | `HostEnvelope`, allow-list |
| Preview host | rendered Markdown/HTML, Diff and resource preview | untrusted; may render user or imported content | read-only envelope, no edit messages |
| Application webview | installed third-party apps (ADR-011) | untrusted, third-party | none beyond bytehost's nonce whitelist |

Rules common to all hosts:

- every builder pins its wry settings explicitly (downloads, developer tools in release, media permissions, navigation, new-window, custom protocols). wry defaults are not treated as safe. A test per builder asserts the pinned values;
- the editor and preview builders must not be reused for application webviews, and application webviews never get Digger's custom protocol or bridge;
- hosts are loaded from assets embedded in the binary or from a scoped custom protocol, never from arbitrary `file://` paths. Resource access is scoped per Project (Technical Architecture §14);
- no host receives database handles, filesystem paths outside its scope, provider keys, shell access or process-spawning ability.

### 3. Layering rule

A wry child view always renders above iced content. Therefore:

- any overlay that must appear over a host (modals, search, context menus near a host, toasts) is a **separate native child window** by default;
- an in-window iced overlay is allowed only with an explicit rule that pushes down or hides the host rectangle while it is open, and a test for it;
- new overlays do not use the in-window route without a recorded reason;
- every iced bar placed beside a host (find bar, conflict bar, recovery bar) reduces the host rectangle by its height.

### 4. Editor host boundary

One editor host instance edits one document (`documentId`). It is created and destroyed by Rust.

**The editor host owns, transiently:**

- the Tiptap/ProseMirror editing session, selection, IME composition state and undo stack;
- input handling, decorations (including projections of separate Domain records such as comments), and find-in-document.

**The editor host never owns:**

- Official Writing Domain State, AST versions, Commits or revisions;
- persistence, filesystem or network access;
- Agent, MCP, Skill or credential access;
- the decision to commit, approve or apply anything.

**Open.** Rust projects the Domain AST at a `StateRef` into ProseMirror JSON plus projection metadata (ADR-006) and pushes it with a `load` message. The metadata carries `StateRef`, `AggregateVersion`, AST and editor schema versions, block-ID mapping and enabled extensions.

**Edit.** The host sends `edit` messages carrying the base `StateRef` and either semantic operations or a normalized document candidate. It does not write to anything. Rust validates against the supported-subset table in ADR-006 and turns accepted input into a Draft Change with a `BaseStateRef`. Only the Writing Domain applies a Change. Trusted, low-risk Human edits may auto-commit by policy, but still pass through Change → Commit.

**Stale base.** If the Official `StateRef` advances while the host has uncommitted edits (Agent apply, other Change), Rust sends `rebase` or `conflict` and the host shows the choice. The host never silently reloads over unsent edits, and Rust never overwrites a dirty session.

**Recovery journal.** The host periodically sends buffer snapshots (debounced, size-bounded). Rust persists them in a recovery journal separate from Official State. After a crash, Rust offers a *candidate change* from the journal. It never writes it to Official State automatically. A journal write failure surfaces as a persistent warning on the editor panel.

**Close.** The host sends `closing` with any unsent edit state; Rust acknowledges before destroying the view.

**Unsupported constructs.** Anything outside the declared extension mapping is rejected with a conversion warning, not stored in Official AST (ADR-006).

### 4.1 Editor library choice

The editor library is an adapter choice under ADR-006, so what matters is the native document model and block-ID support, not direct Markdown I/O. Markdown remains an exchange format.

| Candidate | Core | Fit |
|---|---|---|
| Tiptap (chosen) | ProseMirror | framework-independent; nodes can carry `blockId`; each extension declares its AST mapping |
| Milkdown (Crepe) | ProseMirror + remark | framework-independent; strongest Markdown typing feel; block IDs need a custom plugin |
| ProseMirror directly | ProseMirror | maximum control, but toolbar, menus and keymaps are built by hand |
| CodeMirror 6 | text + decorations | no structured model; suitable as a secondary source mode only (already used in Dozer) |
| Lexical | Meta's own | usable, but diverges from the ADR-006 / Tiptap line with no clear gain |
| BlockNote | ProseMirror/Tiptap | block model fits the AST, but depends on React, which conflicts with this ADR |
| Vditor | own DOM | no structured model; hard to map to AST and semantic operations |

Decisions:

- Tiptap is the primary editor.
- A CodeMirror 6 **source mode** is offered as a secondary view of the same document. Switching goes through Rust (AST ↔ Markdown conversion with its `ConversionResult` report). The two views never synchronize text with each other directly.
- Milkdown is compared against Tiptap in the validation spike. Switching is low-cost because both are ProseMirror.
- Maintenance status and versions of the candidates were not re-verified when this ADR was written; confirm them in the spike before the library is pinned.

### 5. Bridge contract

All host ↔ Rust traffic is the `HostEnvelope` defined in Technical Architecture §9.2:

```text
HostEnvelope { hostKind, binding{projectId,panelId,documentId}, seq, type, payload }
```

- Rust creates the binding with the host and validates it on every inbound message. Stale or foreign bindings are dropped and logged.
- The inbound `type` set is a closed, versioned allow-list per host kind (editor: `ready`, `failed`, `edit`, `selection`, `title`, `snapshot`, `closing`, `search_state`; preview: `ready`, `failed`, `title`, `link_request`). Unknown types are rejected.
- Messages are size-bounded and rate-limited. Oversized or malformed frames fail the host, not the process.
- IPC messages must originate from real user input where they represent user intent (focus, edit, mouseup, zoom). Pages cannot forge them.
- Rust DTOs derive Serde and JSON Schema; CI generates TypeScript types into `hosts/editor-host/src/generated/` and fails on stale output. Both sides validate at runtime.
- A host failure (`failed`, crash, bridge timeout) renders a Rust-provided fallback panel with retry and read-only text. It does not take down the app or lose the recovery journal.
- Links requested by a host are opened by Rust after policy checks, never by the page.

### 6. Editor host build and packaging

- `hosts/editor-host/` is TypeScript + Vite + Tiptap with no UI framework. Its built output is embedded in the Rust binary.
- Built artifacts are committed or reproducibly built in CI; whichever is chosen, a freshness check fails the build if source and bundle diverge.
- Runtime dependency on Node is **not** introduced for the editor: the host bundle runs in the system WebView. (The Agent sidecar's Node dependency is separate; ADR-008.)
- Content Security Policy for hosts is strict: no remote script, no inline script outside the bundle's nonce, no network in the editor host.

### 7. Platform scope

macOS first, matching Dozer, with no Swift/AppKit-only capability in core code. System WebView differences (WKWebView, WebView2, WebKitGTK) are an acknowledged risk for the editor host and must not leak into the Domain. The Technical Architecture skeleton target of macOS, Windows and Linux stays as a CI goal but the editor host is verified on macOS first.

## Alternatives considered

1. **Tauri 2 + React.** Mature, but adds a second UI stack and component library, an IPC command layer and a TypeScript state store, and diverges from Dozer's webview hardening, byteui and layering experience. Rejected.
2. **Pure iced, including a native rich-text editor.** Removes the webview, but iced has no production structured rich-text editor, and building one delays the Writing slice and would lack ProseMirror's mature schema and transaction model that ADR-006 builds on. Rejected for v1; the ADR-006 AST keeps this reachable.
3. **Electron.** Excluded in Technical Architecture §3.1 (size, second runtime in the trusted process). Rejected.
4. **Editor as a separate sidecar process with its own window.** Adds IPC and window management without removing the webview. Rejected.
5. **Editor webview with direct command access to the runtime (broad bridge).** Collapses the authority boundary. Rejected in favour of the closed envelope.

## Consequences

Positive:

- One language and one toolchain for the shell and the authority boundary. Reuse of byteui, bytegit and Dozer's webview hardening and layering rules.
- No UI-to-runtime IPC. The only serialization boundary toward the UI is the narrow host bridge.
- Editing stays a Domain-governed Change path; the editor cannot become a second source of truth.

Negative / costs:

- Native iced UI is more code than web UI for dense forms and review views; byteui must cover what Digger needs.
- Layering constraints apply to every overlay near a host.
- A TypeScript/Vite build exists for the editor host only; CI must keep the bundle fresh.
- System WebView differences can affect editing behaviour (IME, selection, clipboard) across platforms.
- Hot-reload and visual tooling are weaker than a web stack for UI iteration.

## Validation required before Accepted

1. **Editor host spike (macOS):** iced window with one wry editor host loading a Tiptap document from a Rust-provided projection; edits return as `edit`; Rust turns one into a Draft Change; IME composition (Chinese and Japanese) works. Build a minimal Milkdown variant for comparison on IME, `blockId` stability across edits and large-chapter performance; also verify the CodeMirror 6 source-mode round trip through Rust.
2. **Binding and allow-list tests:** foreign binding, unknown type, oversized frame and replayed `seq` are all rejected without crashing.
3. **Stale-base test:** an Official advance during a dirty session produces `rebase`/`conflict` and never overwrites unsent edits.
4. **Recovery test:** kill the process mid-edit; next launch offers a candidate from the journal and Official State is unchanged until the user accepts.
5. **Layering test:** a native overlay window and a push-down iced bar both display correctly over the editor host.
6. **Pinned-settings tests** for editor, preview and application builders.
7. **Bundle freshness and generated-type staleness checks** run in CI.
8. Large chapter (target size to be set during the spike) opens and edits without blocking the iced event loop.

## Spike results (2026-10-10, `spike/editor-host`, macOS, wry 0.55.1, iced 0.14, Tiptap 3.31)

Automated (`cargo test` and `SELFTEST=1`) — passed:

- iced window with a wry child view built via `iced::window::run` + `build_as_child`; host bounds equal window minus left panel and top bar (760×680) and follow a window resize (900×600 → 560×560). This is the push-down rule from §3 for one bar.
- Round trip: Rust pushes a ProseMirror projection (`load`); scripted edits return as `edit`; Rust parses them, derives semantic operations (`ReplaceBlockContent`, `InsertBlock`, `DeleteBlock`) and commits; `committed` acks update the host's base.
- `blockId` stability: untouched blocks keep their id across edits; splitting a block yields a new unique id; a deleted block's id disappears. Rust rejects candidates with missing/duplicate ids or unsupported nodes.
- Bridge validation (unit tests): foreign binding, unknown type, wrong host kind, oversize, malformed JSON and replayed `seq` are rejected, and a rejected message does not advance `seq`.
- Debounced recovery snapshots reach a journal file.
- **Stale base → rebase/conflict (validation item 3), `SELFTEST=merge`:** the host debounces edits, so it can hold unsent edits while the official state advances. Rust pushes `official_advanced`; a host with unsent edits **keeps its document** (verified: it neither takes the official change nor loses its own text). When the pending edit arrives on a stale base, Rust runs a block-level three-way merge (base = the stored version the host edited from, local = host candidate, official = current):
  - disjoint block edits auto-merge, are committed, and come back as `rebase`; the host then holds both changes and is clean on the latest revision;
  - same-block edits (including delete-vs-edit) return `conflict` and **nothing is committed**; the host shows a banner and keeps the user's text; `resolve{mine|official}` commits the chosen result and returns `rebase`;
  - a clean host applies `official_advanced` directly and produces no spurious edit or commit.
- **Crash recovery (validation item 4), `run_recovery_test.sh`:** process A is aborted (SIGABRT) right after the journal is fsynced with uncommitted buffer content; official state on disk is unchanged. On restart process B loads the **official** state into the host (not the journal), derives a candidate Draft Change from the journal, and shows an iced recovery bar. The host rectangle is pushed down by the bar (viewport 680 → 636) and restored afterwards. Accept commits the candidate as a new revision and removes the journal; Discard leaves official state untouched. A journal whose base revision is behind a newer official revision (Agent edited while the app was closed) is rebased through the same merge and accepted without conflict. All three variants pass.

Findings:

1. `crypto.randomUUID()` is **undefined** in a `with_html` WKWebView page (not a secure context). The host must not rely on it. Preferred: block ids are minted by Rust; host-side temporary ids use `getRandomValues`. Check whether the scoped custom protocol gives a secure context.
2. Exactly one edit in flight (send, wait for `committed`/`rejected`) is needed so `baseRev` never goes stale because of the host's own edits. This should be part of the bridge contract.
3. Host JS errors must be forwarded as `failed`; without it a script error looked like a silent hang.
4. Tiptap/ProseMirror copies node attributes on split, so unique `blockId` needs an `appendTransaction` guard. Rust must still validate uniqueness.
5. Edits typed after an answered edit but before its `rebase` arrives cannot be replayed onto the merged document without step-level rebasing. The spike's rule: while the host has unsent edits it ignores `rebase`/`official_advanced` and lets its next edit go stale again, so Rust merges it against the original base. This kept the tested flows correct but means the host can see a merged document only when it is clean.
6. `edit` currently carries the whole document JSON. That is fine for the spike but will not scale to large chapters; steps or block-scoped payloads should be evaluated.
7. The merge does not merge block moves: order follows the official version, and local insertions land after their local predecessor. A real implementation must treat concurrent reordering explicitly.
8. The conflict choice lives in the host DOM and the self-test triggers it with a synthetic click. A production host must only send `resolve` for real user input, and Rust should not rely on the host to enforce that alone. Placing the conflict bar in iced (like the recovery bar) would avoid the problem and is worth deciding in the next spike.
9. Persistence in the spike is JSON files (atomic rename + fsync), not SQLite; crash testing used SIGABRT, not power loss. Data loss window on a crash is bounded by the snapshot interval (300 ms in the spike), which was not measured under load.
10. The spike's CSP uses `script-src 'unsafe-inline'`; the nonce/hash policy from §6 is not yet implemented.

Manual check (2026-10-10, macOS, run by the project owner): Chinese and Japanese IME composition in the Tiptap editor host works with no problems observed. Details such as candidate-window position were not separately recorded. Windows/Linux IME remains unverified.

Not done (still required before Accepted): CodeMirror source-mode round trip; layering with a native overlay window (the in-window push-down rule is verified for two iced bars, the native-window route is not); pinned-settings tests for the builder (the spike only sets `devtools=false`); large-chapter performance; screenshot-based visual verification (screen capture was not permitted in this environment).

## Milkdown comparison (2026-10-10, same spike, `HOST=milkdown`)

Both adapters run behind the same host core (bridge, debounce/in-flight protocol, rebase/conflict, scripted tests); each only builds a ProseMirror `EditorView` and installs two shared plugins (`blockId`, change notification). Versions: Tiptap 3.31.4, Milkdown kit 7.22.2 (commonmark preset + history only). Rust side is identical.

Automated results — identical on both: `basic` (block-id stability, split, delete, viewport), `merge` (auto-merge, conflict, keep-mine/take-official, clean host), crash recovery (accept/discard/stale journal), and the Markdown input-rule test below. All pass on both.

| Aspect | Tiptap | Milkdown |
|---|---|---|
| Adding a `blockId` attribute | `addGlobalAttributes` on a list of node types; one declaration | No global API: `extendSchema` per node returns a **new** plugin that must be registered *after* the preset, otherwise it is silently overridden (first attempt failed this way: every block lost its id) |
| Same ProseMirror JSON from Rust accepted | yes (`paragraph`/`heading`, `level`) | yes; heading also gets its own `id` attr from the preset, ignored by Rust |
| Bundle (single file, minified) | 388 KB | 383 KB |
| Load of N blocks (release build, ms) | 500: 26 · 2000: 70 · 6000: 174 | 500: 27 · 2000: 71 · 6000: 187 |
| Per-keystroke transaction, mean (ms) | 0.35 · 0.90 · 1.88 | 0.33 · 0.85 · 2.17 |
| Transaction → paint (ms) | 5 · 16 · 10 | 9 · 10 · 20 |
| Markdown typing (`## ` etc.) | StarterKit input rules | preset input rules; heading rule reads the *selection*, not the typed position |
| Markdown (de)serialization | needs an extension; not used (Rust converts) | built in; not used (Rust converts) |

Timer resolution in WKWebView is ~1 ms, so sub-millisecond differences and the single-sample paint times are noise. For this document model the two are equivalent in measured performance.

Library-independent findings from the comparison:

1. **An in-place type change loses the block id in both libraries.** Typing `## ` at the start of a paragraph re-creates the node with default attributes, so the Markdown input rule silently changed the block's identity. The shared `blockId` plugin now maps a block without an id back through the transaction and inherits the old id; both adapters pass `SELFTEST=inputrule` with it. Without this, every "turn paragraph into heading" would look like delete + insert to Rust and break provenance, comments and Agent references.
2. Any host-side rule that reads the editor selection (Milkdown's heading rule) must be tested with the caret in place; a synthetic call with the wrong selection produced level 3 instead of 2.
3. **Rust's `doc::diff` is quadratic** (26.7 ms for 6000 blocks, one changed). Fix before large chapters; it does not depend on the editor.
4. A 6000-paragraph chapter (~450k Chinese characters) sends a 1.48 MB `edit` frame against the 2 MB bridge limit, and `getJSON` stays under 1 ms but the frame parse/diff cost grows linearly. Confirms that whole-document `edit` payloads must become block-scoped before chapters of that size.

Manual check on Milkdown (2026-10-10, macOS, run by the project owner via `HOST=milkdown cargo run`): no problems reported with IME input or Markdown typing. Specific observations were not recorded.

**Recommendation:** keep **Tiptap** as the primary editor. Measured behaviour is equal, so the deciding factors are structural: Tiptap's global-attribute and per-extension model matches ADR-006's "every extension declares its AST mapping" rule directly, and does not have the silent schema-override ordering pitfall. Milkdown's advantage is built-in Markdown (de)serialization, which this design does not use because Rust performs the conversion. Both libraries passed the manual check, so nothing contradicts that choice; Milkdown remains a drop-in alternative because both sit on ProseMirror and share the host core.

## Open questions

- Whether review/Diff views are pure iced or a preview host (leaning iced for semantic block diffs derived from the AST).
- Whether the editor host operates on ProseMirror steps, a normalized document candidate, or both as the `edit` payload.
- Initial size and latency budgets for the editor host.
- Windows/Linux editor-host verification schedule and IME coverage.
- Which byteui components Digger reuses unchanged versus proposes upstream.
- Whether the CodeMirror source mode ships in v1 or follows the first Writing slice.
- Confirm Tiptap as the editor library (recommendation above; the manual check found no problem with either).
