# ByteBoy Digger Technical Architecture v0.1

> Status: Technical Architecture — Working Draft  
> Product: ByteBoy Digger — AI Creator  
> Baselines: Logical Domain Model v0.3, Runtime Architecture v0.1  
> Product shape: Desktop-first, Local-first  
> Version: v0.1

---

## 1. Purpose

This document selects the initial technology stack and physical boundaries required to implement Digger v1.

It is intentionally concrete enough to create the repository skeleton, persistence layer and first vertical slice. It is not a permanent commitment to every library.

Confirmed decisions:

1. Digger v1 is a desktop-first, local-first application.
2. Rust owns authoritative Project state, permissions, governance and commit consistency.
3. The desktop UI is Rust: `iced` 0.14 with the shared `byteui` components, with `wry` child webviews hosting only the rich-text editor and preview content. This is the same stack as Dozer; Tauri and React are not used. UI code owns presentation and interaction, not authoritative state.
4. A TypeScript sidecar owns LLM, MCP, Skill and Agent execution.
5. Python is an optional task-specific worker, not the core runtime.

---

## 2. Architecture Summary

```text
┌──────────────────── Digger Desktop ────────────────────┐
│                                                       │
│  iced UI (+ byteui)         wry editor/preview hosts  │
│          │                          │ bridge envelope │
│          │ in-process typed calls   │ (JSON, bound)   │
│          │ + event messages ────────┘                 │
│          ↓                                            │
│  Rust Application Runtime (single trusted process)    │
│  ├── Project Runtime                                  │
│  ├── Creator Domain modules                           │
│  ├── Production Runtime                               │
│  ├── Governance Runtime                               │
│  ├── Context Engine                                   │
│  ├── External Runtime                                 │
│  ├── SQLite persistence                               │
│  └── Resource Store                                   │
│          │                                            │
│          │ JSON-RPC 2.0 over stdio                    │
│          ↓                                            │
│  Supervised TypeScript Agent Sidecar                  │
│  ├── LLM gateway                                      │
│  ├── Agent loop                                       │
│  ├── Skill loader                                     │
│  ├── MCP client/server adapters                       │
│  └── Tool clients                                     │
│          │                                            │
│          ├── isolated tools / Python workers          │
│          └── external model and service APIs          │
│                                                       │
└───────────────────────────────────────────────────────┘
```

Authoritative flow:

```text
UI or Agent
  → Rust Command API
  → permission / capability routing
  → ChangeSet + governance
  → SQLite transaction
       Domain Apply
       + Commit
       + ProjectRevision
       + Outbox
  → durable outcome
```

---

## 3. Technology Decisions

| Concern | v1 choice | Reason |
|---|---|---|
| Desktop shell and UI | Rust + `iced` 0.14 + shared `byteui` (git tag) | Same stack as Dozer; one language for UI and authority boundary; shared components |
| Embedded web hosts | `wry` child webviews overlaid on the iced window | Hosts content that needs a browser engine; webview always renders above iced content |
| UI state | iced `Model`/`update`, Rust-owned projections | No second state store in another language; revision-tagged projections |
| Rich-text editor | Tiptap / ProseMirror in a wry editor host (TypeScript + Vite bundle, no React) | Structured editing, transactions and extensibility, framework-independent |
| Core runtime | Stable Rust | Ownership, concurrency, strong types and native integration |
| Async runtime | Tokio | Sidecars, jobs, IO and cancellation |
| Persistence | SQLite | Local-first atomic store with no external service |
| SQL access | SQLx with explicit SQL | Compile-time query support without hiding transaction semantics |
| Migrations | Versioned SQL migrations | Reviewable and deterministic schema evolution |
| Serialization | Serde + JSON | Shared IPC/event representation and debuggability |
| Schema generation | JSON Schema via `schemars` | Contract validation; TypeScript generation input for the Agent protocol and host bridge |
| Full-text search | SQLite FTS5 | Transaction-friendly local search |
| Vector search | Deferred interface; optional `sqlite-vec` | Avoid premature operational dependency |
| Agent runtime | TypeScript on Node.js LTS | Strongest overlap with MCP and AI SDK ecosystems |
| Agent IPC | JSON-RPC 2.0 over stdio | Local, private, streamable and easy to supervise |
| Media tools | FFmpeg/ffprobe child processes | Proven codec and transformation coverage |
| Application host | `bytehost` embedded in-process (ADR-011) | Reuses accepted install-plan, gateway and per-app isolation; no extra daemon |
| Secret storage | OS Keychain | Credentials stay outside Project databases and prompts |
| Logging | Rust `tracing`; structured sidecar logs | Correlated local diagnostics |
| IDs | UUIDv7 | Sortable, globally unique identifiers |
| Time | UTC RFC 3339 at boundaries | Unambiguous persistence and exchange |

### 3.1 Deliberate exclusions from the first slice

- PostgreSQL;
- Kubernetes or microservices;
- Kafka/RabbitMQ/Redis;
- Electron;
- Event Sourcing;
- CRDT collaboration;
- arbitrary in-process native plugins;
- a vector database service;
- Python as the application server.

---

## 4. Process Architecture

### 4.1 Desktop process

The desktop Rust process (iced UI and runtime) is the trusted application boundary. It owns:

- opening and locking Projects;
- database connections and migrations;
- every authoritative write;
- filesystem authorization;
- OS Keychain access;
- job scheduling and leases;
- sidecar supervision;
- tool authorization;
- external action state;
- audit and causal history.

The Rust process must remain usable when no model provider is configured. Manual editing, Project browsing, history and deterministic tools are not Agent-dependent.

### 4.2 WebView hosts

The editor and preview webviews are wry child views inside the desktop process. They are untrusted relative to Project storage. A host may only send allow-listed bridge messages (§9.2) and receives only data the Rust side chooses to push.

Hosts are loaded from bundled assets or a scoped custom protocol, never from arbitrary `file://` paths. Application webviews (ADR-011) are a separate, less privileged class with their own builder.

It does not receive:

- raw database handles;
- arbitrary filesystem paths;
- provider API keys;
- unrestricted shell execution;
- permission to spawn processes.

### 4.3 Agent sidecar

One supervised Agent sidecar is started on demand per application instance in v1. It can execute several logical Agent Sessions but holds no authoritative state.

Each task receives a short-lived capability grant containing:

```text
TaskGrant
├── GrantId
├── ProjectId
├── ProductionRunId
├── ActorRef / DelegationRef
├── AllowedCapabilities[]
├── AllowedTargets[]
├── ToolAllowlist[]
├── ResourceScopes[]
├── Budget
├── Deadline
└── ExpiresAt
```

The sidecar asks Rust to execute privileged tools. Possessing a tool name is not authorization.

### 4.4 Tool and Python worker processes

Untrusted, crash-prone or dependency-heavy tools run out of process. Python is permitted for ML/media/document tasks with a narrow JSON contract.

Worker rules:

- explicit executable and arguments;
- isolated working directory;
- bounded input/output sizes;
- deadline and cancellation;
- captured structured result;
- no inherited secrets unless required;
- no direct Project database access.

### 4.5 Packaging decision

Development uses Node.js LTS directly. Release packaging must produce a self-contained Agent executable, initially through Node Single Executable Applications or another verified bundling route. Bun compilation may be evaluated, but is not the contract.

The IPC protocol, not the JavaScript packaging method, is the stable boundary.

### 4.6 Application host (bytehost)

Installable user applications (static Web apps in v1; Python/Node apps later) are provided by the sibling `bytehost` library, embedded in the desktop process (iced + wry, same stack as Dozer) through `bytehost-client::in_process::InProcess` over `bytehost-apps::service::AppService`. See ADR-011.

- It is an adapter-layer concern behind `crates/app-host`; Domain, runtime and persistence crates never depend on it.
- Its state lives in the per-user application data directory, not in a Project directory, and is excluded from Project export and backup.
- Applications are never part of a ChangeSet, Commit or Outbox event, and receive no Project handle.
- A loopback gateway serves every app at `http://<app-id>.localhost:<fixed-port>/`, selected by `Host` header.
- Each app renders in its own wry child webview built by a dedicated restrictive builder: no custom protocol, nonce-guarded minimal IPC, per-app data store, same-origin-only navigation. It shares no builder or bridge with the editor/preview webviews and is strictly less privileged than them.
- Install approval is a human action enforced in Rust. The Agent sidecar cannot install or approve an app.
- Application processes stop when Digger exits; orphans are reclaimed at next start.

---

## 5. Repository Structure

```text
digger/
├── apps/
│   └── desktop/                          # iced application (bin), wry host builders
│       └── src/
│           ├── app/                      # Model / update / view
│           ├── panels/
│           ├── platform/                 # native windows, overlay windows
│           └── hosts/                    # wry builders: editor, preview, app (ADR-011)
├── hosts/
│   └── editor-host/                      # TypeScript + Vite + Tiptap bundle (no React)
│       ├── src/
│       │   └── generated/                # generated bridge types
│       └── dist/                         # built bundle embedded in the binary
├── crates/
│   ├── digger-types/                     # IDs, envelopes, shared DTOs
│   ├── project-domain/                   # Project/Actor/Authority
│   ├── knowledge-domain/
│   ├── production-domain/
│   ├── governance-domain/
│   ├── external-domain/
│   ├── project-runtime/
│   ├── context-engine/
│   ├── job-runtime/
│   ├── persistence-sqlite/
│   ├── resource-store/
│   ├── search-fts/
│   ├── agent-bridge/
│   └── app-host/                         # bytehost adapter (ADR-011)
├── domains/
│   ├── domain-protocol/
│   ├── writing-domain/
│   ├── social-domain/
│   └── site-domain/
├── runtimes/
│   └── agent-runtime/
│       ├── src/
│       │   ├── protocol/
│       │   ├── agents/
│       │   ├── llm/
│       │   ├── skills/
│       │   ├── mcp/
│       │   └── tools/
│       └── package.json
├── migrations/
│   ├── catalog/
│   └── project/
├── schemas/                              # generated JSON Schemas
├── fixtures/
├── tests/
│   ├── contract/
│   ├── integration/
│   └── e2e/
└── docs/
```

Crates correspond to ownership and dependency direction, not one crate per entity. If the initial implementation becomes ceremony-heavy, adjacent domain crates may begin as modules inside fewer crates while preserving the same boundaries.

### 5.1 Dependency direction

```text
UI (iced) / host bridges
            ↓
Application / Runtime services
            ↓
Domain models + protocols
            ↑
Persistence / tool / external adapters
```

Domain crates must not depend on iced, wry, SQLx, HTTP clients or the TypeScript runtime.

---

## 6. Local Project Layout

Each Project is portable as a directory:

```text
My Project.digger/
├── project.db
├── manifest.json
├── resources/
│   └── objects/
│       └── ab/cd/<sha256>
├── working/
│   ├── runs/
│   └── previews/
├── exports/
├── backups/
└── logs/                    # optional, privacy policy applies
```

Application-level state is separate:

```text
AppData/Digger/
├── catalog.db               # recent Projects and app preferences
├── cache/                   # disposable shared cache
├── sidecars/
└── logs/
```

Rules:

- `project.db` and referenced immutable resources are the authoritative portable Project package;
- `working/` contains recoverable but non-authoritative production artifacts;
- `exports/` contains user-facing outputs, not authoritative state;
- secrets never live in the Project directory;
- absolute machine paths are not stored when a Project-relative reference is possible;
- moving a closed Project directory must preserve usability.

Installed applications, their data and managed runtimes are **not** part of the Project directory. They live under the per-user application data directory (`<app-data>/digger/bytehost/`) and are excluded from Project move, export and backup.

### 6.1 Project lock

Only one local process may hold the authoritative write lock for a Project in v1. A second opener receives read-only mode or a clear conflict message.

This is process ownership, not a substitute for AggregateVersion checks.

---

## 7. SQLite Architecture

### 7.1 Database split

`catalog.db` owns application discovery and preferences. Each Project owns one `project.db` containing all state inside its consistency island.

There are no cross-Project transactions.

### 7.2 Connection configuration

On open, the persistence adapter verifies or sets:

```text
foreign_keys = ON
journal_mode = WAL
synchronous = NORMAL by default
busy_timeout = bounded and explicit
```

Durability-sensitive export/publication checkpoints may use stronger synchronization if benchmarks justify it.

The application uses multiple read connections and a bounded authoritative write path. Long reads must not retain transactions unnecessarily.

### 7.3 Core table families

The accepted Project database boundary, Phase 1 schema contract and concurrency semantics are specified in [ADR-003](adr/ADR-003-project-sqlite-consistency-island.md).

The initial schema is organized by responsibility:

```text
project_*             identity, lifecycle, revision
actor_*               actors, roles, permissions, delegations
knowledge_*           sources, assertions, authority bindings
production_*          intents, runs, actions, results, working refs
change_*              changes, change sets, assessments
governance_*          policies, reviews, decisions
commit_*              commits and committed change membership
domain_*              official Creator Domain state
external_*            actions, attempts, references, observations
history_*             causal edges and snapshots
runtime_job           durable asynchronous jobs
event_outbox          transactional event envelopes
idempotency_record    stable command outcomes
resource_metadata     immutable resource descriptors
search_*              FTS content and indexing checkpoints
```

Domain tables are explicit rather than one universal JSON object table. JSON columns are appropriate for Domain-owned payloads and immutable envelopes, but identity, ownership, versions and query-critical fields remain typed columns.

### 7.4 Revision allocation

The Project row contains the current `project_revision`. The authoritative transaction updates it using compare-and-swap semantics:

```sql
UPDATE project
SET project_revision = project_revision + 1
WHERE project_id = ? AND project_revision = ?;
```

Failure to update exactly one row is a conflict. The new revision is attached to the Commit, affected state records and Outbox envelopes.

### 7.5 Migration rules

- migrations are monotonic and checked into source control;
- opening a newer unsupported schema is rejected without mutation;
- a backup is created before non-trivial migrations;
- migrations run while holding the Project write lock;
- migration success is recorded with application version and timestamp;
- downgrade is restore/export based, not reverse-SQL based;
- migration fixtures cover the oldest supported Project version.

---

## 8. Authoritative Transaction Implementation

`ProjectWriteCoordinator` owns the only public governed Apply method.

```rust
pub trait CreatorDomain {
    fn validate(&self, request: ValidationRequest) -> DomainResult<ValidationReport>;
    fn analyze_impact(&self, request: ImpactRequest) -> DomainResult<ImpactAssessment>;
    fn preview(&self, request: PreviewRequest) -> DomainResult<Preview>;
    fn apply(
        &self,
        tx: &mut ProjectTransaction,
        request: ApplyRequest,
    ) -> DomainResult<ApplyResult>;
}
```

The actual API may differ, but `apply` must receive the coordinator-controlled transaction. Domain code cannot commit, open a second authoritative connection, call a model or perform network IO.

Transaction sequence:

```text
BEGIN IMMEDIATE
  → read current ProjectRevision
  → verify idempotency record
  → verify ChangeSet fingerprint and governed state
  → re-check Actor/Delegation/Policy
  → compare AggregateVersions and dependency StateRefs
  → Domain Apply
  → persist new AggregateVersions
  → insert Commit
  → increment ProjectRevision
  → insert Outbox events
  → mark ChangeSet committed
  → store idempotent outcome
COMMIT
```

No `await` involving network, model, tool, human or child process is permitted inside this transaction.

---

## 9. UI and Host Contract

### 9.1 Application API

The iced UI calls the Rust application API in-process. There is no IPC hop between UI and runtime. Operation categories keep the same names:

```text
query_*       side-effect-free controlled projections
command_*     operational state transitions
change_*      draft/change/governance operations
run_*         ProductionRun lifecycle
external_*    external action lifecycle
resource_*    scoped resource operations
app_*         application host: query, install-plan, install, start, stop, uninstall (ADR-011)
```

The API accepts and returns explicit DTOs. It does not expose repository methods or SQL-shaped generic CRUD. Long operations run on Tokio and report back as iced messages; the UI thread never blocks on I/O.

### 9.2 Host bridge

The wry editor and preview hosts talk to Rust through a single bridge envelope, not through general commands:

```text
HostEnvelope
├── hostKind: editor | preview
├── binding: projectId / panelId / documentId   # validated by Rust on every message
├── seq
├── type: ready | failed | edit | selection | title | ...
└── payload
```

Rules:

- Rust validates that the binding matches the host it created; messages from a stale or foreign binding are dropped;
- the message-type allow-list is closed and versioned;
- Rust DTOs for the bridge derive Serde and JSON Schema. CI generates matching TypeScript types into `hosts/editor-host/src/generated/` and fails if committed generated files are stale;
- runtime validation remains on both sides even when types compile;
- the editor host sends semantic edits and selections, never SQL-shaped or path-shaped requests;
- `edit` and `snapshot` payloads are **block-scoped deltas**, never the whole document (ADR-001):

```text
EditPayload
├── baseRev: StateRef            # the version the host's baseline equals; Rust holds it in history
├── changed: Block[]             # blocks that differ from the baseline (new or edited), full node JSON
├── removed: BlockId[]           # baseline blocks that no longer exist
└── placements?: { id, after: BlockId | null }[]   # only blocks whose predecessor changed
```

  In-place typing sends one block and no placements; an insert or move sends one or two placements. Rust reconstructs the candidate by applying the delta to `history[baseRev]` (not to the current Official version), rejecting any inconsistency (unknown or duplicate ids, a new block without a placement, a cycle, two blocks claiming one predecessor). The host advances its baseline only when Rust confirms (`committed`, `rebase`, `official_advanced`, `load`), so a rejected or merged edit leaves later deltas cumulative and correct. `edit` and `snapshot` frames have a tighter size limit (512 KB) than other frames.

Downstream (Rust → host) uses the same delta shape. Only `load` carries a whole document:

```text
official_advanced   { fromRev, rev, stateRef, delta }   # delta against the host baseline history[fromRev]
rebase              { ackSeq,  rev, stateRef, delta, reason }   # delta against the snapshot the host sent in edit #ackSeq
load                { stateRef, rev, doc, editDebounceMs }      # full document: first load and resync
```

- The host applies a downstream delta **only when it is clean** (nothing unsent, nothing in flight) **and** the message matches its state (`fromRev` equals its baseline revision; `ackSeq` equals its last sent edit). A host with unsent edits ignores `official_advanced` and lets its pending edit go stale, which Rust merges; a mismatch makes the host send `resync`, and Rust answers with a full `load`. A delta is never applied to a state it was not computed against.
- The host applies the delta in place and moves only the blocks that must move (longest increasing subsequence of the surviving blocks). Untouched blocks keep their ProseMirror node identity, so the caret, selection and IME composition outside the changed blocks are undisturbed. An inconsistent delta is detected before anything is dispatched and also leads to `resync`.

### 9.3 Events

Commands return durable acceptance or outcomes. Progress and change notifications reach the UI as iced messages:

```text
runtime.job.progress
production.run.changed
agent.session.delta
changeset.changed
project.revision.advanced
external.action.changed
projection.checkpoint.changed
```

Each event includes `projectId`, stable subject ID, sequence/checkpoint and correlation ID. The UI refreshes or patches its projections on events; events are not the source of truth.

### 9.4 UI state rule

```text
iced Model       = Rust-owned revision-tagged projections + ephemeral interaction state
Tiptap           = active editor transaction state, inside the editor host only
SQLite           = authoritative state
```

Official Domain objects must not be silently persisted from UI state. Because the webview always renders above iced content, any iced overlay that can cover a host must be a separate native window or must explicitly push down or hide the host rectangle.

---

## 10. Editor Architecture

Tiptap/ProseMirror is an editor adapter, not the Writing Domain model.

```text
Official passage StateRef
        ↓
Editor session / local buffer
        ↓
semantic edit extraction
        ↓
Draft Change
        ↓
preview / governance
        ↓
Commit
```

For trusted low-risk Human edits, debounce and auto-commit policy may make the interaction feel like ordinary autosave. The implementation still produces a Change and Commit.

Editor crash recovery uses a separate buffer journal. Recovery offers a candidate change; it does not overwrite Official State automatically.

The accepted representation decision is:

```text
Writing Domain AST = authoritative manuscript state
ProseMirror JSON   = editor session projection
Markdown / HTML    = exchange and preview
DOCX / EPUB / PDF  = import, compile or publication
```

The canonical AST uses versioned deterministic JSON and stable block IDs. HTML is never authoritative. Conversion, semantic operations, recovery and schema evolution are specified in [ADR-006](adr/ADR-006-writing-document-representation.md).

---

## 11. Job Runtime and Outbox

### 11.1 SQLite-backed durable jobs

The first version uses `runtime_job` in Project SQLite rather than an external queue.

Worker claim is atomic:

```text
Queued
  → compare status / lease
  → Running(owner, lease_until, attempt)
  → Succeeded | Failed | Paused | Quarantined
```

Jobs are short coordination records; large inputs and outputs use StateRefs/ResourceRefs.

### 11.2 Job classes

- context indexing;
- assessment and preview;
- Agent production;
- media transformation;
- Outbox dispatch;
- projection rebuilding;
- external execution and reconciliation;
- maintenance and integrity scans.

### 11.3 Scheduling

Tokio tasks execute trusted lightweight jobs in-process. Agent, media, Python and risky adapters execute out of process.

Concurrency limits exist per:

- Project;
- provider;
- job kind;
- CPU/GPU resource class;
- external destination.

### 11.4 Outbox consumers

Local consumers checkpoint by consumer name and ProjectRevision/EventId. Rebuildable projections may be deleted and replayed. Authoritative Domain state may not be reconstructed solely from projection tables.

---

## 12. Agent IPC Protocol

### 12.1 Transport

Rust starts the sidecar with private stdin/stdout pipes:

- stdin/stdout: newline-delimited JSON-RPC messages;
- stderr: structured diagnostics only;
- protocol handshake includes version and supported capabilities;
- malformed or oversized frames terminate the session;
- no localhost port is opened by default.

### 12.2 Core methods

```text
agent.initialize
agent.startSession
agent.cancelSession
agent.resumeSession
agent.shutdown

runtime.getContext
runtime.invokeTool
runtime.reportAction
runtime.storeResult
runtime.proposeChangeSet
runtime.requestHumanInput

session.started
session.delta
session.actionStarted
session.actionCompleted
session.paused
session.completed
session.failed
```

Agent-to-Rust methods are checked against the active `TaskGrant`.

### 12.3 Session durability

The sidecar may keep transient model stream state, but Rust persists:

- AgentSession identity and lifecycle;
- model/tool call metadata required by policy;
- Action and Result references;
- checkpoints required for resume;
- token/cost budget;
- causal and correlation IDs.

After a sidecar crash, the Rust supervisor marks affected sessions interrupted and decides whether they are resumable. It never assumes an in-flight external tool had no effect.

### 12.4 Model gateway

The TypeScript gateway normalizes:

```text
ModelRequest
├── provider/model
├── messages or response input
├── tools
├── structured output schema?
├── sampling settings
├── budget/deadline
└── trace metadata

ModelResponse
├── text/content parts
├── tool calls
├── usage
├── provider request id
├── finish reason
└── safety/error metadata
```

Provider-specific features are retained in namespaced metadata rather than flattened away.

---

## 13. Skill and MCP Architecture

### 13.1 Skills

A Skill is versioned know-how and resource metadata, not executable authority.

```text
SkillPackage
├── manifest
├── instructions
├── references
├── schemas
├── scripts?
├── integrity digest
└── trust level
```

Skill discovery and prompt loading occur in the Agent sidecar. Any script/tool invocation returns through Rust authorization.

Project-installed Skills are pinned by version/digest. Updating a Skill is an explicit Project operation so Production provenance remains explainable.

### 13.2 MCP

Digger may act as both:

- MCP client: consume external capabilities;
- scoped MCP server: expose approved Project capabilities to external Agents.

MCP tools are mapped to Digger `CapabilityDefinition`s and `EffectClass` before execution. External MCP descriptions are not trusted classifications.

For v1:

- local stdio MCP is preferred;
- remote MCP requires explicit server approval;
- credentials remain in Keychain;
- Project Mutation tools create Changes;
- External Side-Effect tools create ExternalActions;
- no MCP server receives raw SQLite access.

---

## 14. Resource Store

Resources are immutable objects addressed by SHA-256 content digest.

Write path:

```text
stream to temporary file
  → enforce size/media policy
  → hash while writing
  → verify/decode as required
  → atomic rename into objects/<prefix>/<digest>
  → insert resource metadata
  → return ResourceRef
```

Metadata includes media type, byte size, digest, original name, provenance and optional derived-resource relationship.

Derived media is a new ResourceRef:

```text
original video
  ├── proxy video
  ├── waveform
  ├── thumbnail
  └── transcript
```

Garbage collection deletes only resources that are unreachable from Official State, retained history, active WorkingState or protected recovery windows.

Hosts read resources through a scoped custom protocol registered on the editor and preview hosts only (never on application webviews), or through a stream call from the iced side, not arbitrary `file://` paths.

---

## 15. Search and Context Engine

### 15.1 Phase 1 search

SQLite FTS5 indexes controlled textual projections:

- Knowledge text;
- Writing manuscript passages;
- Social content and variants;
- Site entries/pages;
- titles, labels and selected metadata.

Index rows retain DomainTarget, StateRef and ProjectRevision. Outbox consumers update FTS asynchronously; UI shows the index checkpoint when freshness matters.

### 15.2 Vector interface

The Context Engine defines a retrieval interface independent of storage:

```rust
trait SemanticIndex {
    async fn upsert(&self, documents: Vec<SemanticDocument>) -> Result<()>;
    async fn remove(&self, refs: Vec<StateRef>) -> Result<()>;
    async fn search(&self, query: SemanticQuery) -> Result<Vec<SemanticHit>>;
}
```

The first implementation may be disabled or use `sqlite-vec`. A remote vector service is not required for v1.

### 15.3 Context assembly

Rust resolves authorization, Authority, StateRefs and Project scope. The Agent sidecar may assist with ranking/compression but cannot expand the authorized source set.

Every delivered context has a persisted ContextManifest.

---

## 16. External Integration Architecture

External adapters are Rust traits with out-of-process implementations permitted when SDK availability requires TypeScript or Python.

```rust
pub trait ExternalAdapter {
    async fn execute(&self, request: ExternalExecutionRequest) -> ExternalResult;
    async fn observe(&self, request: ExternalObservationRequest) -> ObservationResult;
    async fn reconcile(&self, request: ReconcileRequest) -> ReconcileResult;
}
```

The Rust External Runtime owns ExternalAction lifecycle and durable attempts. The adapter only talks to the destination.

Requirements:

- stable idempotency key where supported;
- immutable payload snapshot;
- explicit timeout certainty (`NotApplied`, `Unknown`, `Confirmed`);
- response body redaction;
- provider rate limits;
- credentials resolved just in time from Keychain;
- reconciliation before unsafe retry.

---

## 17. Security Model

### 17.1 Trust zones

```text
Trusted
  Rust domain/runtime and Project database

Constrained
  WebView hosts, Agent sidecar, first-party adapters

Untrusted by default
  imported Skills, MCP servers, scripts, documents,
  model output, web content and external APIs
```

### 17.2 WebView host capabilities

Hosts get no shell, filesystem, HTTP or process access. Their only channel is the closed bridge allow-list (§9.2). Each host builder pins its wry settings explicitly (downloads, developer tools, media permissions, navigation, new-window) rather than relying on wry defaults, and a test asserts them.

Application webviews (ADR-011) are untrusted content. They are built by a dedicated wry builder with no custom protocol and no Digger IPC beyond a nonce-guarded whitelist. Navigation is restricted to the app's own origin; `window.open`, downloads and developer tools are denied, and wry's default media-permission auto-approval is not relied upon.

### 17.3 Prompt injection boundary

Retrieved documents and web content are data, not authority. Model output cannot:

- change a TaskGrant;
- install a Skill;
- reveal a secret;
- classify its own effect as read-only;
- commit a Change;
- authorize an ExternalAction.

### 17.4 Secrets

Keychain records use opaque credential IDs. Project state stores only those IDs and non-secret configuration. Logs, events, model prompts and exported Projects must redact secrets.

### 17.5 Filesystem access

Import/export starts from an explicit user file selection or Project-scoped path. Paths are canonicalized and checked against allowed roots. Archive extraction rejects traversal, symlink escape and decompression bombs.

---

## 18. Reliability and Recovery

On startup, the application:

1. verifies Project lock ownership;
2. validates schema version;
3. opens SQLite and runs integrity prechecks;
4. expires abandoned job leases;
5. marks orphaned Agent Sessions interrupted;
6. resumes Outbox dispatch;
7. schedules projection gap repair;
8. surfaces uncertain ExternalActions for reconciliation;
9. preserves editor recovery buffers.

Backup policy:

- use SQLite online backup API, not raw copying of an open WAL database;
- include referenced resources and manifest;
- write to a temporary archive and atomically publish it;
- validate a backup before rotating older copies;
- never treat cloud folder syncing as a transactional backup while a Project is open.

---

## 19. Observability and Privacy

Rust uses `tracing` spans; the TypeScript sidecar emits compatible structured records.

Required identifiers:

- TraceId;
- ProjectId;
- ActorRef;
- ProductionRunId/JobId;
- CorrelationId;
- CausationId.

Default local logs exclude prompt bodies, manuscript text, uploaded documents, secrets and raw provider responses. Content-bearing diagnostics require explicit user export.

Metrics remain local by default. Any product telemetry is opt-in and uses a separate pipeline from Project History.

---

## 20. Testing Strategy

### 20.1 Rust

- pure Domain unit tests;
- SQL repository integration tests against temporary SQLite files;
- migration tests from versioned fixtures;
- transaction rollback and conflict tests;
- property tests for lifecycle and state invariants;
- crash/restart tests for jobs and Outbox.

### 20.2 TypeScript Agent runtime

- provider adapter contract tests;
- recorded model response tests;
- JSON-RPC framing and cancellation tests;
- Skill loading and digest tests;
- MCP effect-classification and authorization tests;
- Agent checkpoint/resume tests.

### 20.3 Cross-process

- Rust ↔ sidecar protocol compatibility suite;
- sidecar crash and malformed-frame tests;
- oversized output and deadline tests;
- tool capability-denial tests;
- no-secret-in-log assertions.

### 20.4 Desktop E2E

The iced `update`/`view` layer is tested through message-level state-machine tests against the Rust API. The editor host bundle is tested with Playwright in a browser against a stubbed bridge. A small native smoke suite covers wry layering and host binding. Command-level integration tests cover the Rust API. The first golden flow is:

```text
Create Writing Project
→ create chapter
→ Human edit auto-commit
→ ask Agent to revise
→ inspect Result/Diff
→ approve ChangeSet
→ verify Commit and history
→ export manuscript
→ restart and recover identical Official State
```

---

## 21. First Vertical Slice

The first implementation should prove architecture, not maximize features.

### Slice scope

```text
Writing Project
├── Project metadata
├── Work
├── Outline
├── Chapter
├── Markdown/structured manuscript
├── one Human edit path
├── one Agent rewrite capability
├── ChangeSet preview and approval
├── Commit + ProjectRevision + Outbox
├── basic history
└── Markdown export
```

### Acceptance criteria

1. A Project directory can be created, closed, moved and reopened.
2. Human edits use the governed commit path.
3. Agent execution cannot access SQLite directly.
4. Agent rewrite produces WorkingState and a ChangeSet, not immediate Official State.
5. Approving the ChangeSet atomically updates manuscript, Commit, revision and Outbox.
6. Stale chapter versions produce a visible conflict.
7. Killing the sidecar cannot corrupt the Project.
8. Restart resumes Outbox and exposes interrupted work.
9. Export reads a stable StateRef/Snapshot.
10. Contract and migration tests run in CI.

---

## 22. Implementation Phases

### Phase 0 — Workspace foundation

- Rust workspace, editor host bundle and Agent runtime;
- formatting, linting, unit test and schema-generation CI;
- shared IDs, errors, envelopes and protocol versioning;
- iced shell with a wry editor-host placeholder and a closed bridge allow-list.

### Phase 1 — Project consistency island

- Project directory and lock;
- SQLite connection/migrations;
- Project, revision, ChangeSet, Commit, Outbox and idempotency tables;
- `ProjectWriteCoordinator` and invariant tests.

### Phase 2 — Writing vertical slice

- Writing state and Domain Protocol;
- chapter editor and recovery buffer;
- Human edit Change path;
- history and Markdown export.

### Phase 3 — Agent runtime

- supervised sidecar and JSON-RPC handshake;
- Keychain-backed model configuration;
- ContextManifest;
- one rewrite capability with streamed progress;
- Diff preview and governed apply.

### Phase 4 — Durable background work

- SQLite jobs and leases;
- Outbox dispatcher and FTS projection;
- crash recovery and diagnostics;
- resource store.

### Phase 5 — Domain expansion

- deeper Writing structure/Canon;
- Social Domain;
- Site Domain;
- external publishing and deployment adapters.

### Phase 6 — Application host

- in-process `AppService` + iced/wry spike (ADR-011 validation);
- `crates/app-host`: `InProcess` wiring, `bytehost-panel` state and event bridge;
- approval flow, app panel and per-app webview policy;
- Excalidraw-class static app end to end, then process-type apps.

Cloud sync and collaboration are separate architecture phases after local invariants are proven.

---

## 23. Architecture Decision Records Required Next

The following decisions should be captured as ADRs during implementation:

1. ADR-001: iced + wry desktop shell on the Dozer stack (byteui shared; editor as a wry host). **Proposed.**
2. ADR-002: Rust authoritative runtime and TypeScript Agent sidecar.
3. ADR-003: one SQLite database per Project consistency island. **Accepted.**
4. ADR-004: SQLx and explicit migration strategy.
5. ADR-005: JSON-RPC 2.0 stdio Agent protocol.
6. ADR-006: Writing Domain AST as the canonical document representation. **Accepted.**
7. ADR-007: resource hashing, deduplication and garbage collection.
8. ADR-008: Node sidecar release packaging.
9. ADR-009: first model provider abstraction and structured output policy.
10. ADR-010: Project backup/export package format.
11. ADR-011: embedding bytehost as the application host. **Proposed.**

---

## 24. Open Decisions

These do not block Phase 0–1:

- visual design tokens and which byteui components Digger reuses or extends;
- Node SEA versus another self-contained sidecar packaging method;
- initial LLM provider set;
- whether `sqlite-vec` is needed in v1;
- signing and distribution channels for macOS/Windows/Linux;
- local plugin sandbox technology;
- future cloud account and sync protocol;
- exact maximum Project/resource size;
- whether one app process may open several writable Projects concurrently;
- whether apps ever outlive the Digger process (ADR-011 chose in-process; a Dozer-style daemon stays possible).

The Project database schema and transaction API are resolved by ADR-003, and the manuscript representation is resolved by ADR-006. The next implementation-blocking decision is ADR-004: SQLx migration identity, checksums, backup policy and compatibility testing.

---

## 25. Technical Principles

1. **The Rust process is the authority boundary.**
2. **The WebView hosts and Agent sidecar are clients, not alternate backends.**
3. **One Project database implements one Project consistency island.**
4. **Official state, Commit, revision and Outbox are one SQLite transaction.**
5. **Network, model, human and media work never occur inside that transaction.**
6. **Long-running work is durable, cancelable and idempotent.**
7. **Agent output enters WorkingState before governance.**
8. **Capabilities are granted per task and checked by Rust.**
9. **Project data is portable; credentials are not embedded in it.**
10. **Search and projections are rebuildable; Domain state is authoritative.**
11. **Process boundaries contain failures and dependencies.**
12. **The first vertical slice must prove recovery and invariants, not only UI.**

---

## Appendix A — Initial Workspace Dependencies

Indicative dependencies, subject to version pinning during bootstrap:

```text
Rust
├── iced 0.14 / winit / wry
├── byteui (git tag, shared with Dozer)
├── bytegit (git tag, shared with Dozer)
├── tokio
├── serde / serde_json
├── schemars
├── sqlx (sqlite)
├── uuid (v7)
├── time
├── thiserror
├── tracing / tracing-subscriber
├── sha2
├── tempfile
├── keyring
├── reqwest (adapter layer only)
└── bytehost-apps / bytehost-client / bytehost-webview / bytehost-panel (git tag, adapter layer only)

TypeScript Editor Host
├── @tiptap/*
├── zod
└── vite

TypeScript Agent Runtime
├── MCP TypeScript SDK
├── zod
├── provider SDK adapters
└── JSON-RPC protocol package or small internal codec
```

Dependencies must be introduced for implemented capability, not installed speculatively.

## Appendix B — Definition of Done for the Architecture Skeleton

- desktop app starts on macOS, Windows and Linux CI targets;
- the iced UI renders one revision-tagged projection from a Rust query;
- a wry editor host completes a bridge round trip with binding validation;
- Project database is created and migrated;
- governed test Change commits atomically with revision and Outbox;
- intentional failure rolls all four back;
- sidecar handshake, cancellation and crash recovery pass;
- no WebView host path can read arbitrary files or secrets;
- generated bridge and Agent-protocol TypeScript contracts are reproducible;
- a Project fixture survives close/reopen and migration tests;
- architecture contract tests run without network access.
