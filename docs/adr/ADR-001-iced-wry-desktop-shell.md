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

1. **Editor host spike (macOS):** iced window with one wry editor host loading a Tiptap document from a Rust-provided projection; edits return as `edit`; Rust turns one into a Draft Change; IME composition (Chinese and Japanese) works.
2. **Binding and allow-list tests:** foreign binding, unknown type, oversized frame and replayed `seq` are all rejected without crashing.
3. **Stale-base test:** an Official advance during a dirty session produces `rebase`/`conflict` and never overwrites unsent edits.
4. **Recovery test:** kill the process mid-edit; next launch offers a candidate from the journal and Official State is unchanged until the user accepts.
5. **Layering test:** a native overlay window and a push-down iced bar both display correctly over the editor host.
6. **Pinned-settings tests** for editor, preview and application builders.
7. **Bundle freshness and generated-type staleness checks** run in CI.
8. Large chapter (target size to be set during the spike) opens and edits without blocking the iced event loop.

## Open questions

- Whether review/Diff views are pure iced or a preview host (leaning iced for semantic block diffs derived from the AST).
- Whether the editor host operates on ProseMirror steps, a normalized document candidate, or both as the `edit` payload.
- Initial size and latency budgets for the editor host.
- Windows/Linux editor-host verification schedule and IME coverage.
- Which byteui components Digger reuses unchanged versus proposes upstream.
