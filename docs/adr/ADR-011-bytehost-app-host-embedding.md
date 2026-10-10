# ADR-011: Embedding bytehost as the Application Host

- Status: Proposed
- Date: 2026-10-10
- Decision owners: Digger Architecture
- Applies to: desktop shell, application runtime, security model, local data layout

## Context

Digger should let a user install third-party Web applications (for example Excalidraw) or Agent-generated Python/Node programs, and open each one as an isolated panel inside the desktop shell.

The sibling repository `bytehost` provides the headless part of this capability, extracted from Dozer:

| Crate | Responsibility |
|---|---|
| `bytehost-apps` | application model, two-phase install plan, lifecycle state, registry, sources (local dir / archive / https URL), managed runtimes, process supervision, local gateway (`server` feature) |
| `bytehost-client` | transport-neutral `AppHostApi`, `InProcess` implementation (`in-process` feature), conformance tests |
| `bytehost-webview` | pure webview security policy: `AppOrigin` navigation allow-list, per-app `data_store_identifier`, IPC nonce and init script, data-store removal queue |
| `bytehost-panel` | UI-framework-neutral state machines for the app panel, install approval and log viewer |

bytehost is agent-neutral, UI-framework-neutral and webview-library-neutral. Dozer is its first consumer; its design spec leaves "how Digger implements the supervisor" open (A8).

**Digger uses the same technology stack as Dozer; Tauri is not used.** Concretely: Rust workspace, GUI in `iced` 0.14 with the shared `byteui` component repository (git tag), preview and editor hosts as `wry` child webviews overlaid on the iced window, shared `bytegit`, Tokio. Digger has no daemon of its own: the desktop process is the single trusted process (Technical Architecture §4.1). Installed applications are not Project content; a Project is a portable directory whose database is a consistency island (ADR-003).

## Decision

### 1. Embed in-process; do not add a daemon

Digger starts `bytehost-apps::service::AppService` inside the desktop process and drives it through `bytehost-client::in_process::InProcess`. This resolves bytehost spec item A8 for Digger as "in-process embedding".

```text
iced UI ── messages ──> app-host adapter ──> AppHostApi (InProcess) ──> AppService
   │                    (bytehost-panel state machines)                  ├─ manager / registry
   │                                                                     ├─ runtime adapters (static_web, process)
   └─ wry child webview per app ── http://<app-id>.localhost:<port>/ ──> └─ gateway (loopback, fixed port)
        policy from bytehost-webview
```

Consequences accepted: application processes live and die with the Digger process. On clean exit Digger calls `AppService::shutdown()`; after a crash, bytehost's startup reconciliation reclaims orphaned process groups. Apps do not keep running while Digger is closed. `AppHostApi` keeps a Dozer-style daemon reachable later without UI changes.

### 2. Applications are application-level, not Project-level

bytehost state lives under the per-user application data directory (alongside the catalog database), not inside `*.digger`:

```text
<app-data>/digger/bytehost/
├── apps/<app-id>/{manifest.toml,state.json,package/<ver>/,data/,cache/,logs/}
├── runtimes/
├── downloads/
└── gateway-port           # persisted; see §4
```

- Opening, moving, exporting or backing up a Project never includes installed apps or their data (the ADR-010 export format must exclude this tree).
- No bytehost type enters `project.db`, Domain state, ChangeSets, Commits or the Outbox. Installing or running an app is not a governed Change.
- An app receives no Project handle. Any future "app reads/writes Project content" feature must go through a Rust-side capability with an explicit grant and its own ADR.

### 3. Dependency direction

bytehost is an adapter-layer dependency (Technical Architecture §5.1). One crate owns all contact with it:

```text
crates/app-host/     # adapter: AppService lifecycle, InProcess wiring, event bridge,
                     # bytehost-panel state, Digger trust policy
<desktop crate>      # iced views for app panel / install approval / logs,
                     # and the wry app-webview builder
```

- Domain, runtime and persistence crates must not depend on `bytehost-*`.
- Only `crates/app-host` depends on `bytehost-apps` (`server`), `bytehost-client` (`in-process`) and `bytehost-panel`.
- The desktop crate depends on `bytehost-webview` (policy only) and builds the wry webview itself; bytehost never depends on iced or wry.
- `bytehost-panel` is adopted: the same state machines as Dozer drive the app panel and install flow, so behaviour and failure categories match.

Dependencies are consumed by git tag, per bytehost's compatibility promise (crates are `publish = false`), always one common tag:

```toml
bytehost-apps    = { git = "https://github.com/byteboyai/bytehost.git", tag = "v0.1.1", features = ["server"] }
bytehost-client  = { git = "https://github.com/byteboyai/bytehost.git", tag = "v0.1.1", features = ["in-process"] }
bytehost-webview = { git = "https://github.com/byteboyai/bytehost.git", tag = "v0.1.1" }
bytehost-panel   = { git = "https://github.com/byteboyai/bytehost.git", tag = "v0.1.1" }
```

Local development overrides with an uncommitted `[patch]` in `.cargo/config.toml`, as for byteui and bytegit. A CI gate runs `cargo tree` to prove no `bytehost-*` crate appears in the closure of domain, runtime or persistence crates.

### 4. Gateway and origin

Adopt bytehost's gateway decisions unchanged:

- origin is `http://<app-id>.localhost:<port>/`; one fixed port serves all apps by `Host` header;
- the port is chosen once from 20000–32767, persisted, and never silently changed (the origin includes the port; changing it loses all app-local storage);
- if the persisted port is taken, startup reports an explicit error and Settings offers a confirmed change;
- Digger and Dozer persist separate ports, so both can run on one machine.

The gateway binds loopback only and rejects any request whose `Host` is not a registered app origin (DNS-rebinding defence).

### 5. App webview policy (security-critical)

Each app opens in its own wry child webview, built by a dedicated function that is **not** shared with Digger's editor/preview webview builder. Following the rule Dozer learned in `build_app_webview`: the preview path installs a custom protocol and a full IPC bridge, which untrusted app code must not reach. The app webview builder must:

1. register no custom protocol and expose no Digger IPC; accept only bytehost's nonce-guarded `AppIpc` whitelist (focus, mouseup, zoom), forwarded only for real user events;
2. use `data_store_identifier(app_id)` from `bytehost-webview` (macOS 14+; below that, fall back to origin-only isolation and say so in the install plan). Do not copy the algorithm;
3. allow top-level navigation only where `AppOrigin::allows_navigation` returns true (single source of truth), and deny `window.open`;
4. explicitly disable every wry default that is not a safe default: downloads allowed, developer tools in debug builds, and automatic approval of camera/microphone requests. A test pins these settings;
5. keep a strict CSP on served content and never relax it for an app.

Two bytehost-documented hazards apply:

- wry has no hook for media permissions, so Digger must not declare camera/microphone usage descriptions until an authorization gate keyed on app grants exists;
- `LocalDir` installs copy the whole source directory including secrets such as `.env`; the approval view must warn before approving a `LocalDir` plan.

**Layering.** The webview always renders above iced content. Any iced overlay that can cover an app panel must either be a separate native child window (the pattern Dozer uses for modal overlays) or explicitly push down or hide the app webview rectangle. New overlays default to separate native windows.

### 6. Trust policy is a Digger product choice

bytehost produces install plans and enforcement levels; Digger decides when confirmation is required. Initial policy:

- every install requires explicit human approval of the `InstallPlan`, shown in a Rust-owned approval view;
- Agent-generated and URL-sourced apps are `Untrusted` and may only be `static_web` in v1 (bytehost A6h allows network sources for static apps only);
- `advisory` and `unsupported` enforcement levels are shown verbatim, never as "isolated". Outbound network for process apps remains `Advisory`;
- the Agent sidecar cannot approve its own install plan, and model output cannot install an app (§17.3). `TaskGrant` gains no app-install capability in v1.

bytehost itself treats approval as a product convention (in-process callers fill `approver` and `trust` themselves). Therefore `crates/app-host` must be the only caller of `install`, and it must accept only a plan it issued and a human approval event.

### 7. UI surface

Unlike a webview-based UI, no IPC command layer exists between UI and Rust: iced messages call `crates/app-host` directly. Long operations (`prepare`, `start`) are bytehost task handles; progress and state changes arrive as `AppEvent` (`Installed`, `StateChanged`, `EndpointChanged`, `Progress`, `LogAvailable`, `RuntimeUnavailable`) and become iced messages. They are independent of Project revision events and carry no `projectRevision`. Failures use bytehost's categorised `AppErrorKind`; "runtime unavailable" is a persistent panel state, shown in the app's own panel by a host-provided page (not served from the app origin), not a one-off toast.

## Alternatives considered

1. **Tauri 2 shell with child webviews.** Rejected: Digger shares Dozer's stack, and sharing byteui, bytegit and the bytehost integration pattern avoids maintaining two UI stacks and two webview policies.
2. **Reimplement an app host inside Digger.** Duplicates a tested install/approval/gateway/supervision stack. Rejected.
3. **Run a daemon like Dozer's `dozerd`.** Adds a second trusted process and a UDS protocol to an architecture whose principle is "the Rust process is the authority boundary". Rejected for v1.
4. **Open apps in the editor/preview webview.** Shares capabilities and storage with trusted hosts. Rejected.
5. **Store app data inside the Project.** Breaks portability and puts untrusted code beside authoritative state. Rejected.
6. **Custom URI scheme instead of the gateway.** Lacks same-origin WebSocket, true streaming, cookies and service workers (bytehost spike). Rejected.

## Consequences

Positive:

- Reuses the app model, install-plan TOCTOU protection, archive hardening and per-app storage isolation already accepted in bytehost, plus Dozer's integration experience.
- Same GUI and webview stack as Dozer; no second framework.
- No new daemon. The Project consistency island is untouched.

Negative / costs:

- Digger gains a loopback HTTP listener and child processes whenever apps run.
- Apps stop when Digger exits.
- The wry-over-iced layering constraint applies to every overlay near an app panel.
- A pinned git tag couples Digger releases to bytehost releases. Wire protocol is append-only, but the data-store-identifier algorithm must never change.
- macOS is the only platform bytehost currently pins managed runtimes for.

## Validation required before Accepted

1. Embed `AppService` in-process in a minimal iced + wry window: install and start a static app and a `py-notes`-style process app; verify the gateway returns 200 with the session cookie and 403 without, and that `shutdown()` closes the port.
2. Build the app webview with the pinned restrictive settings; verify cross-origin navigation and `window.open` are denied, and that a second app cannot read the first app's localStorage or cookies.
3. Verify app panel layering with a native overlay window and with an iced in-window overlay that uses the push-down rule.
4. Crash test: kill Digger with a running Python app; the next launch reclaims the orphan.
5. CI dependency-closure check from §3.

## Open questions

- Whether a Project may pin "recommended apps" (a manifest hint only, never auto-install).
- Whether apps may later be granted scoped, read-only access to Project resources through a Rust capability.
- Windows/Linux: process-group handling, orphan reclamation, `*.localhost` behaviour and per-app data stores (bytehost spike items V4/V5 are open).
