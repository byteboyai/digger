# ByteBoy Digger Runtime Architecture v0.1

> Status: Runtime Architecture — Working Draft  
> Product: ByteBoy Digger — AI Creator  
> Baseline: `ByteBoy_Digger_Logical_Domain_Model_v0.3`  
> Scope: From Actor intent to Official Project State and external effects  
> Version: v0.1

---

## 1. Purpose

This document turns the Logical Domain Model into an executable runtime model.

It answers five questions:

1. Which runtime components does an operation traverse?
2. Where are authorization, validation and governance enforced?
3. Where are the transaction and consistency boundaries?
4. Which work may be asynchronous?
5. How does the runtime recover from conflicts, retries and partial failure?

This document does not select Rust crates, UI frameworks, databases, queues, serialization formats or deployment topology. Those belong to Technical Architecture v0.1.

The primary runtime invariant is inherited unchanged:

> No finalized Commit exists without corresponding Official Domain State, and no governed Official Domain State mutation exists without a corresponding finalized Commit.

---

## 2. Runtime Design Goals

The runtime SHALL:

- preserve Domain ownership of creator semantics;
- give Human, Agent, Automation and Importer the same mutation path;
- keep exploratory and generated work outside Official State until governed;
- provide optimistic concurrency at Aggregate level and a Project-wide logical revision;
- preserve causation and correlation from Intent through observation;
- isolate irreversible external effects from internal transactions;
- allow expensive production and assessment work to run asynchronously;
- recover deterministically after process interruption;
- avoid requiring Event Sourcing or distributed transactions.

The runtime SHALL NOT:

- allow an Agent or UI to write Creator Domain storage directly;
- infer Domain meaning from generic diffs;
- hold a database transaction open across model, tool, network or human work;
- publish mutable current state to an external destination;
- use a message broker as the source of truth;
- turn every Domain Context into a network service.

---

## 3. Architectural Decision: Project Consistency Island

For v0.1, each Project is a **consistency island**.

```text
Project Consistency Island
│
├── Project operational state
├── Official Creator Domain state
├── Governance and Commit records
├── ProjectRevision allocation
└── Transactional Event Outbox
```

All authoritative writes for one Project pass through one logical `Project Write Coordinator`. Writes may use optimistic concurrency and need not execute on one OS thread, but finalization is serialized by Project revision allocation and expected versions.

The following transition is one atomic consistency boundary:

```text
re-check authority and policy
        +
re-check base versions and preconditions
        +
apply Domain state
        +
finalize Commit
        +
advance ProjectRevision
        +
append event envelopes to Outbox
```

This is a logical storage requirement, not a database choice. Technical Architecture MUST choose a persistence design capable of making that boundary atomic.

### 3.1 Why the Domain Runtime is in the island

Creator Domains are modular semantic owners, but they are not independent remote services in v0.1. Their governed `Apply` handlers execute inside the Project consistency boundary.

This avoids a distributed transaction between Governance, Commit and Official Domain State. Read and assessment contracts may be hosted or scaled separately later, but the authoritative Apply path must preserve the same atomic semantics.

### 3.2 Multi-domain ChangeSets

A ChangeSet that touches several Creator Domains is committed atomically only when every affected Domain can participate in the same consistency island.

Otherwise it must be decomposed into causally linked ChangeSets with explicit intermediate states and compensation semantics. The runtime must not advertise false atomicity across independent stores.

---

## 4. Runtime Component Model

```text
 Human UI     Agent     Automation     Importer
     │          │           │             │
     └──────────┴───────────┴─────────────┘
                         │
                 Runtime API / Gateway
                         │
              Identity + Request Context
                         │
                Capability Router
                         │
      ┌──────────────────┼──────────────────┐
      │                  │                  │
      ↓                  ↓                  ↓
 Context Engine   Production Runtime   Command Dispatcher
      │                  │                  │
      │            Agent / Tool Host        │
      │                  │                  │
      └──────────┬───────┴──────────┬───────┘
                 ↓                  ↓
          Domain Runtime     Governance Runtime
                 │                  │
                 └──────────┬───────┘
                            ↓
                  Project Write Coordinator
                            │
                   Project Consistency Island
                            │
                     Event Outbox / History
                            │
                  Projection + Job Dispatch
                            │
                       External Runtime
                            │
                     External Adapters
```

### 4.1 Runtime API / Gateway

Accepts requests from every Actor type and creates a canonical request envelope. It handles authentication, request size limits and protocol adaptation, but owns no business semantics.

### 4.2 Capability Router

Resolves a Domain capability and routes by `EffectClass`:

| EffectClass | Runtime path |
|---|---|
| `ReadOnly` | Context/read execution |
| `WorkingState` | Production Runtime |
| `ProjectMutation` | Change + Governance + Commit |
| `ExternalSideEffect` | Snapshot + External authorization + External Runtime |

The declared class is a lower bound on governance. Runtime policy may escalate it but may not downgrade it.

### 4.3 Project Runtime

Owns Project lifecycle, Actor/Delegation checks, request coordination, ProjectRevision and authoritative command dispatch. It is the entry point to the consistency island.

### 4.4 Domain Runtime

Hosts Social, Writing and Site Domain contracts:

- State: resolve, read and describe targets;
- Context: return side-effect-free Domain context;
- Governance: validate, analyze impact, preview and apply;
- Capability: describe meaningful Domain actions.

Only the governed `Apply` handler mutates Official Creator State.

### 4.5 Production Runtime

Owns ProductionRun, Action, Result and WorkingState execution. It coordinates Agent sessions and tools but does not own Official Creator State.

### 4.6 Agent / Tool Host

Runs reasoning, model calls and tools under an explicit Delegation, budget and cancellation scope. It emits execution telemetry and returns Results or proposed Changes through the Application boundary.

### 4.7 Governance Runtime

Combines permission, Domain validation, impact, risk and Policy into a governance decision. It owns review state and assessment freshness, not Domain invariants.

### 4.8 External Runtime

Executes authorized ExternalActions against stable payload snapshots. It owns attempt, idempotency, observation and reconciliation mechanics; the Creator Domain owns the business meaning of publication, deployment or distribution.

### 4.9 Context Engine

Assembles bounded, reproducible context from Knowledge, Authority, Official Domain State and WorkingState. Assembly is read-only with respect to Official State.

### 4.10 History and Event Infrastructure

Stores immutable Commit, Decision, Observation and event records; maintains causal links; publishes Outbox events; builds disposable projections. It distinguishes Domain History from technical execution telemetry.

### 4.11 Resource Layer

Stores large immutable or versioned resources and returns `ResourceRef`s. Domain state owns the meaning of a resource, never the Resource Layer itself.

---

## 5. Canonical Runtime Envelopes

### 5.1 RequestEnvelope

```text
RequestEnvelope
├── RequestId
├── IdempotencyKey?
├── ProjectId
├── ActorRef
├── DelegationRef?
├── CapabilityKey / CommandType
├── Target?
├── Input
├── ExpectedVersion?
├── CauseRef?
├── CorrelationId
├── Deadline?
└── TraceContext
```

`ActorRef` is authenticated identity. `DelegationRef` is required when an Agent acts using delegated authority. Neither client-provided field is trusted without server-side resolution.

### 5.2 RuntimeOutcome

```text
RuntimeOutcome
├── OutcomeId
├── Status
│   ├── Completed
│   ├── Accepted
│   ├── AwaitingReview
│   ├── Conflict
│   ├── Rejected
│   └── Failed
├── ResultRef?
├── ProductionRunRef?
├── ChangeSetRef?
├── CommitRef?
├── ExternalActionRef?
├── ProjectRevision?
└── Error?
```

`Accepted` means durable asynchronous work was created; it does not mean the requested business outcome succeeded.

### 5.3 RuntimeError

```text
RuntimeError
├── Code
├── Category
│   ├── Authentication
│   ├── Authorization
│   ├── Validation
│   ├── Conflict
│   ├── Policy
│   ├── Budget
│   ├── Dependency
│   └── Internal
├── Retryability
├── SafeMessage
├── DetailsRef?
└── TraceId
```

Domain failures remain typed (`InvariantViolation`, `TargetMissing`, `PreconditionFailed`) rather than being flattened into generic infrastructure errors.

---

## 6. End-to-End Operation Paths

### 6.1 ReadOnly

```text
Actor
  → Gateway
  → authenticate + authorize read scope
  → Capability Router
  → State / Context Contract
  → controlled projection
  → Actor
```

Reads may be served from projections when their staleness is explicit. Reads used for governance or Apply must use authoritative state or verify versions before Commit.

### 6.2 WorkingState / Production

```text
Intent
  → Create ProductionRun
  → assemble ContextBundle
  → schedule Action
  → Agent / Tool execution
  → Result
  → WorkingState
  → optionally create ChangeSet
```

Model calls, tools and human pauses occur outside authoritative write transactions. Each durable lifecycle transition is a short transaction.

Completing a ProductionRun does not commit its Changes and does not fulfill its Intent automatically.

### 6.3 ProjectMutation

```text
Actor edit / Agent result / Import / Automation
  → create or amend Draft ChangeSet
  → freeze as Ready
  → Domain validate + impact + preview
  → Policy evaluation
  ├── reject/block
  ├── await review
  └── auto-apply / approved
        → Project Write Coordinator
        → re-check freshness, permission and policy
        → Domain Apply
        → finalize Commit + ProjectRevision + Outbox atomically
        → return new StateRefs
```

An immediate Human edit uses the same path. For low-risk trusted edits, Draft, assessment and auto-commit may complete within one user interaction, but none of the logical checks are bypassed.

### 6.4 Operational Command

Operational commands such as `PauseRun` or `ResolveIssue` do not require a ChangeSet unless they mutate Creator Domain State.

```text
Command
  → authenticate
  → permission + lifecycle invariant
  → owning Aggregate
  → state transition + ProjectRevision? + Outbox
```

ProjectRevision advances when authoritative Project-visible state changes, even if there is no Commit.

### 6.5 ExternalSideEffect

```text
Domain intent (Publish / Deploy / Distribute)
  → resolve immutable PayloadSnapshotRef
  → create ExternalAction
  → permission + risk + reversibility policy
  → authorize
  → durable dispatch
  → adapter execute with idempotency key
  → record attempt
  → observe / reconcile
  → Succeeded | Failed | Cancelled
  → Observation
  → Domain interpretation
```

No external network call runs inside the Project transaction. Authorization fixes the destination, operation and payload snapshot; execution may not silently substitute current mutable state.

---

## 7. ChangeSet Assessment and Freshness

A Ready ChangeSet is frozen. Assessments bind to an exact candidate fingerprint:

```text
CandidateFingerprint
= hash(
    ChangeSet content,
    BaseStateRefs,
    relevant Authority versions,
    Policy version,
    Domain contract version
  )
```

Validation, impact, preview, risk and approval records include this fingerprint. A material change invalidates them and returns the ChangeSet to Draft.

Before Apply, the coordinator must re-check:

- ChangeSet is still Ready and governed;
- Actor/Delegation remains valid;
- approval and policy inputs remain fresh;
- every `BaseStateRef` / `ExpectedVersion` still matches;
- preconditions still hold;
- Domain invariants still hold.

Stale state produces `Conflict`; it is not silently rebased. A revised candidate is a new assessment cycle.

---

## 8. Commit Protocol

### 8.1 Atomic path

```text
1. Begin Project write boundary
2. Lock/compare expected Project and Aggregate versions
3. Resolve effective Actor permission and Delegation
4. Verify governance decision and candidate fingerprint
5. Domain re-validates current authoritative state
6. Domain applies all Changes in deterministic order
7. Persist new Domain state and AggregateVersions
8. Append immutable Commit
9. Allocate next ProjectRevision
10. Append Domain/Event envelopes to transactional Outbox
11. Mark ChangeSet Committed
12. Commit write boundary
13. Publish Outbox asynchronously
```

Steps 2–11 succeed or fail together.

The Commit ID and event IDs are allocated before the transaction completes so retries can recognize the same operation. A successful response may be lost; retrying the same idempotency key returns the recorded outcome instead of applying the mutation twice.

### 8.2 Ordering

Within one Commit:

- Changes have a stable declared order;
- Domain Apply is deterministic for the same authoritative state and payload;
- one ProjectRevision identifies the committed transition;
- emitted event envelopes retain Domain-defined event order.

Across Projects there is no global ordering guarantee.

### 8.3 Large ChangeSets

Expensive generation, parsing, resource creation and validation happen before Commit. Apply should persist already-prepared immutable values or switch versioned roots/pointers within the short transaction.

If final Apply cannot fit within a bounded transaction, the ChangeSet must be decomposed. Partial visibility of a nominally atomic ChangeSet is not allowed.

---

## 9. Consistency Model

| Concern | Required consistency |
|---|---|
| Official state + Commit | Atomic / strong |
| AggregateVersion check + mutation | Atomic / strong |
| ProjectRevision allocation | Serialized per Project |
| Governance decision at Apply | Freshly verified |
| Outbox append + authoritative mutation | Atomic / strong |
| Projection updates | Eventual |
| Search index / embeddings | Eventual |
| Context cache | Bounded stale; version tagged |
| WorkingState | Durable, but non-authoritative |
| Execution telemetry | Best effort or durable by policy |
| External world | Reconciled, never assumed transactional |

### 9.1 Read-your-writes

After a successful Commit, the authoritative State Contract must return at least the returned `ProjectRevision`. Projections may lag and must expose their observed revision so clients can wait, fall back to authoritative read, or show pending indexing.

### 9.2 Cache safety

Cached context and state projections carry source `StateRef`s, Authority versions and ProjectRevision. Caches must not be used to authorize Apply without authoritative re-checks.

---

## 10. Asynchronous Work Model

The following work may be asynchronous:

- Agent/model/tool execution;
- Context retrieval and enrichment;
- Domain validation, impact analysis and preview for expensive candidates;
- large resource transforms;
- projection, search and embedding updates;
- event delivery and notification;
- external execution, observation and reconciliation.

The following must remain synchronous within the final Project write boundary:

- expected-version comparison;
- final permission/delegation check;
- governance freshness check;
- Domain invariant re-check;
- Domain Apply;
- Commit finalization;
- ProjectRevision allocation;
- Outbox append.

### 10.1 Durable jobs

```text
Job
├── JobId
├── Kind
├── ProjectId
├── InputRef
├── CausationId
├── CorrelationId
├── Attempt
├── Lease
├── Deadline?
├── Status
└── OutcomeRef?
```

Jobs use at-least-once delivery. Therefore handlers must be idempotent or deduplicate by a stable operation key. A lease timeout permits recovery after worker failure; it does not prove the previous attempt produced no side effect.

---

## 11. Context Runtime

### 11.1 Context assembly

```text
ContextRequest
  → resolve Actor + Delegation scope
  → establish Target and task scope
  → read Official State at explicit StateRefs
  → resolve effective Authority
  → retrieve relevant Knowledge
  → read permitted WorkingState
  → ask Domain Context Contract
  → rank and budget
  → create immutable ContextManifest
```

```text
ContextManifest
├── ContextId
├── ProjectId
├── ActorRef
├── IntentRef?
├── TargetRefs[]
├── StateRefs[]
├── AuthorityRefs[]
├── KnowledgeRefs[]
├── WorkingStateRefs[]
├── DomainContextRef
├── AssemblyPolicyVersion
├── Budget
└── CreatedAt
```

The manifest records what the runtime supplied; it need not duplicate all content. This supports reproducibility and provenance without pretending model execution is deterministic.

### 11.2 Security trimming

Every provider receives a resolved scope and returns only authorized projections. Filtering after unrestricted context assembly is insufficient because retrieval traces and model prompts can leak restricted data.

---

## 12. Agent and Production Runtime

### 12.1 Execution boundary

An Agent Session is an execution detail inside a ProductionRun. Sessions may restart, move workers or use different models without changing the logical identity of the Run.

```text
ProductionRun
├── ContextManifest(s)
├── AgentSession(s)
├── Action(s)
├── Result(s)
├── WorkingState
└── proposed ChangeSet(s)
```

### 12.2 Tool mediation

All tool calls pass through a mediator that checks:

- the capability is allowed by the active Delegation;
- target and resource scopes are permitted;
- budget and deadline remain available;
- effect classification matches the requested operation;
- external-effect tools route through External Runtime;
- output and provenance are recorded at the required level.

A tool labeled read-only must not mutate Project or external state. Unknown effects are treated as external side effects, not as read-only.

### 12.3 Cancellation and pause

Cancellation is cooperative. It prevents new Actions and asks active work to stop, but does not erase completed Results or assume an in-flight tool had no effect.

Human review, policy blocks, budget exhaustion and external waits pause the ProductionRun with a reason. Resume revalidates Delegation, context freshness and budget.

---

## 13. External Runtime

### 13.1 ExternalAction immutability

Once authorized, these fields are immutable:

- destination;
- operation;
- payload snapshot;
- authorization decision;
- idempotency key;
- declared reversibility.

Any change creates a new ExternalAction.

### 13.2 Attempt model

```text
ExternalAction
  └── Attempt 1..n
       ├── request fingerprint
       ├── adapter result
       ├── ExternalReference?
       ├── started/completed time
       └── certainty: Confirmed | Unknown | NotApplied
```

A timeout normally yields `Unknown`, not `NotApplied`. Before retry, the adapter should observe or reconcile using the idempotency key and destination.

### 13.3 Compensation

Compensation is a new governed ExternalAction linked by `compensates`. History never rewrites a successful action as though it did not happen.

---

## 14. Event Delivery, History and Projections

### 14.1 Transactional Outbox

Domain events are appended to the Outbox in the same transaction as authoritative mutation. A dispatcher publishes them at least once. Consumers deduplicate by `EventId`.

This guarantees:

- committed state cannot omit its event record;
- event publication can resume after a crash;
- consumers do not participate in the Project transaction.

It does not guarantee exactly-once delivery.

### 14.2 Consumer rules

Consumers:

- must be idempotent;
- may assume per-Project revision ordering only when the transport/consumer explicitly preserves it;
- must detect revision gaps before building order-sensitive projections;
- must not mutate another Context's authoritative state directly;
- issue a Command or create a Change when a reaction has Domain meaning.

### 14.3 Three records, three purposes

```text
Commit / state history  → what officially changed
Causal history          → why it changed
Execution trace         → how work was technically performed
```

Telemetry retention or sampling must never delete required Commit or causal history.

---

## 15. Resource Runtime

Large binaries and generated assets live outside the authoritative transaction as immutable/versioned resources.

```text
ResourceRef
├── ResourceId
├── Version / ContentDigest
├── MediaType
├── Size
├── Integrity
└── StorageClass?
```

Recommended lifecycle:

```text
upload/stage resource
  → verify integrity
  → create immutable ResourceRef
  → reference it from a Change
  → commit Domain meaning atomically
  → asynchronously collect unreachable staged resources after retention period
```

A resource upload alone does not mutate Official Creator State. Access control is evaluated through Project and Domain references, not by guessable storage locations.

---

## 16. Failure and Recovery Semantics

| Failure point | Runtime outcome |
|---|---|
| Before durable Run/Job creation | Request fails; safe client retry with idempotency key |
| During Agent/Tool work | Preserve completed Results; Run may fail/pause; Official State unchanged |
| During assessment | Assessment fails or expires; ChangeSet remains uncommitted |
| Version conflict before Apply | Return `Conflict`; no mutation |
| Failure inside Commit transaction | Roll back Domain state, Commit, revision and Outbox together |
| Crash after Commit, before response | Retry returns recorded outcome by idempotency key |
| Crash before Outbox publish | Dispatcher publishes later |
| Projection consumer failure | Replay/deduplicate from Outbox/event log |
| External timeout | Mark certainty Unknown; observe/reconcile before retry |
| Worker lease loss | Another worker may retry; handler deduplicates |

### 16.1 Poison work

Jobs and events that repeatedly fail move to a quarantined state with diagnostics and operator actions. Quarantine is visible and auditable; it must not silently skip authoritative transitions.

### 16.2 Recovery invariant scan

The runtime should support periodic verification of:

- finalized Commit references resolve to expected Domain StateRefs;
- committed ChangeSets have one Commit;
- ProjectRevision is monotonic per Project;
- Outbox contains event envelopes for finalized transitions;
- projections have no unreported revision gaps;
- authorized/executing ExternalActions are not abandoned without reconciliation.

---

## 17. Concurrency and Idempotency

### 17.1 Optimistic concurrency

`AggregateVersion` protects local state. `ProjectRevision` orders Project-visible authoritative transitions but is not used to reject all independent edits indiscriminately.

A Change carries exact `BaseStateRef`s for targets whose semantics it depends on. Domain impact analysis may add dependency versions. Apply rejects stale dependencies when they can change validity or meaning.

### 17.2 Idempotency scopes

| Scope | Key behavior |
|---|---|
| API command | Same Actor + Project + key returns same durable outcome |
| Commit Apply | Same ChangeSet/fingerprint cannot finalize twice |
| Job | Same job/operation can be retried safely |
| Event consumer | Same EventId has one projection effect |
| External adapter | Stable action key is reused across uncertain retries |

Idempotency records must live at least as long as the maximum supported retry window; Commit and ExternalAction identities are permanent history.

---

## 18. Security and Governance Enforcement

Checks occur at multiple boundaries because each answers a different question:

```text
Gateway              → who is calling?
Capability Router    → may this Actor request this capability/scope?
Tool Mediator        → may this delegated execution use this tool now?
Governance Runtime   → what review/risk path does this candidate require?
Commit Boundary      → are permission, delegation, policy and state still valid?
External Runtime     → is this exact payload/destination authorized to execute?
```

Rules:

- deny by default;
- resolve permission server-side;
- never trust Agent self-asserted authority;
- bind approval to an immutable candidate or payload fingerprint;
- redact secrets from prompts, Results, events and traces;
- keep credentials in an adapter-facing secret boundary;
- audit privileged and external actions with Actor, cause and correlation.

---

## 19. Observability

Every request, Run, Action, Job, Commit and ExternalAction propagates:

```text
TraceId
ProjectId
ActorRef
CorrelationId
CausationId
```

Minimum signals:

- request latency and outcomes by capability/effect class;
- Run and job queue age, retries and terminal outcomes;
- context size, source mix and assembly latency;
- model/tool usage, budget and failures;
- ChangeSet time in Draft/Ready/Review/Blocked;
- commit conflicts and transaction latency;
- Outbox lag and projection revision lag;
- external unknown outcomes and reconciliation age;
- invariant scan failures.

Business history and sensitive content must not be reconstructed solely from logs. Logs are diagnostic infrastructure, not the authoritative Project record.

---

## 20. Runtime Deployment Shape

v0.1 defines logical components, not mandatory processes.

A valid initial deployment may be:

```text
Interactive Application / API Process
├── Gateway
├── Project Runtime
├── Domain Runtime modules
├── Governance Runtime
├── Context Engine
└── Write Coordinator

Worker Process(es)
├── Production / Agent workers
├── assessment workers
├── projection/index workers
├── Outbox dispatcher
└── External workers

Shared Runtime Resources
├── authoritative transactional store
├── job transport/store
├── resource store
├── search/vector projections
└── telemetry backend
```

This can run locally in one application or be deployed across processes. Process boundaries must not weaken the Project consistency island.

---

## 21. Runtime Contract Tests

Every Creator Domain implementation must pass a common runtime suite:

1. State and Context reads do not mutate Official State.
2. ProjectMutation capability cannot bypass ChangeSet governance.
3. Apply rejects stale `BaseStateRef`s and failed preconditions.
4. Apply re-checks Domain invariants after approval.
5. A failed Apply creates neither finalized Commit nor visible partial state.
6. A successful Apply creates Domain state, Commit, ProjectRevision and Outbox entries atomically.
7. Retrying the same operation does not apply twice.
8. Assessment becomes invalid when candidate fingerprint changes.
9. Outbox replay does not duplicate projection effects.
10. External execution uses the authorized immutable snapshot.
11. Unknown external outcomes require reconciliation before unsafe retry.
12. Agent, Human, Automation and Import paths enforce the same mutation invariants.

Domain-specific suites additionally verify semantic validation, impact discovery, preview and deterministic Apply behavior.

---

## 22. Key Runtime Sequences

### 22.1 Agent-produced change

```text
Agent       Production      Context      Domain       Governance     Coordinator
  │              │              │           │              │              │
  │ start Run ──>│              │           │              │              │
  │              │ assemble ───>│           │              │              │
  │<──── ContextManifest ────────│           │              │              │
  │ tools/actions │              │           │              │              │
  │── Result ────>│              │           │              │              │
  │              │ create ChangeSet ────────>│              │              │
  │              │              │ validate/impact ────────>│              │
  │              │              │           │<── policy ───│              │
  │              │              │           │     governed │              │
  │              │              │           │              │── commit ───>│
  │              │              │           │<──── Apply transaction ─────│
  │              │<──────── Commit + StateRefs + Revision ────────────────│
```

### 22.2 External publication

```text
Domain       Governance       External Runtime       Adapter        World
  │               │                  │                  │             │
  │ payload snapshot              │                  │             │
  │── ExternalAction ─────────────>│                  │             │
  │               │<── assess ─────│                  │             │
  │               │── authorize ──>│                  │             │
  │               │                  │── execute(key) ─>│             │
  │               │                  │                  │── request ─>│
  │               │                  │                  │<── result ──│
  │               │                  │<── reference ────│             │
  │<──────── Observation / status ─│                  │             │
```

---

## 23. Decisions Deferred to Technical Architecture

Runtime Architecture v0.1 deliberately leaves these choices open:

- transactional database and schema strategy;
- event/outbox transport;
- local-first synchronization and multi-device conflict strategy;
- process isolation and plugin ABI;
- Rust crate/module layout;
- Agent framework and LLM gateway;
- resource store implementation;
- search/vector implementation;
- API and serialization protocol;
- secret manager and credential storage;
- deployment packaging and scaling topology.

Technical choices must demonstrate, through tests or proof, that they preserve the consistency and failure semantics in this document.

---

## 24. Open Questions for v0.2

1. Must v1 support offline multi-device writes, or only one authoritative Project writer at a time?
2. What is the maximum atomic ChangeSet size and Apply duration?
3. Which operational transitions advance ProjectRevision?
4. How are Knowledge and Authority changes included in the same consistency island?
5. Which assessment artifacts require permanent retention?
6. What guarantees do Domain plugins need across version upgrades and replay?
7. How are long-running migrations represented without exposing partial Official State?
8. What external adapters can provide reliable idempotency or observation?
9. Which projection staleness bounds are acceptable per user interaction?
10. Which resource retention rules balance recovery, privacy and storage cost?

---

## 25. Runtime Architecture Principles

1. **A Project is the unit of runtime consistency.**
2. **Official Domain State, Commit, ProjectRevision and Outbox cross one atomic boundary.**
3. **Long-running work never holds the authoritative transaction open.**
4. **Production may propose; only governed Apply makes state official.**
5. **Domain owns semantics; Runtime owns routing, execution and consistency.**
6. **Every Apply revalidates current state—approval is not a mutation backdoor.**
7. **Asynchronous delivery is at least once, so idempotency is mandatory.**
8. **External state is observed and reconciled, never assumed transactional.**
9. **Context is assembled, scoped, versioned and provenance-aware.**
10. **History is authoritative; observability is diagnostic.**
11. **References and projections cross Context boundaries, not foreign Aggregates.**
12. **Logical components do not imply microservices.**

---

## Appendix A — Responsibility Matrix

| Concern | Authoritative owner | Runtime executor |
|---|---|---|
| Project lifecycle | Project Context | Project Runtime |
| Creator semantics/state | Creator Domain | Domain Runtime |
| WorkingState/Result | Production Context | Production Runtime |
| Agent session/tool trace | Runtime infrastructure | Agent / Tool Host |
| Permission/delegation | Project Context | Project Runtime / Tool Mediator |
| Domain validation/impact | Creator Domain | Domain Runtime |
| Policy/review decision | Governance Context | Governance Runtime |
| Commit/ProjectRevision | Project/Governance mechanics | Write Coordinator |
| Domain event semantics | Originating Domain | Domain Runtime |
| Event envelope/delivery | Core infrastructure | Outbox Dispatcher |
| Publication/deployment meaning | Creator Domain | Domain Runtime |
| External attempt/reconciliation | External Context | External Runtime |
| Resource bytes | Resource Layer | Resource Runtime |
| Resource meaning | Creator Domain | Domain Runtime |
| Search/vector views | Projection infrastructure | Async workers |

## Appendix B — Normative Terms

- **MUST / SHALL**: required to preserve the architecture.
- **SHOULD**: expected default; deviation requires an explicit rationale.
- **MAY**: permitted implementation choice.
- **Official State**: authoritative Project-visible state owned by a Context.
- **Consistency island**: state that must participate in one atomic authoritative write boundary.
- **Outbox**: durable event records committed atomically with the state transition and published asynchronously.

