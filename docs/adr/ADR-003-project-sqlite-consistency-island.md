# ADR-003: Project SQLite Consistency Island and Transaction API

- Status: Accepted; partially superseded by ADR-014 (Project identity, registry and storage). The transactional design for Creator Domain state is unchanged and is re-evaluated when the first Creator Domain state is introduced.
- Date: 2026-10-01
- Decision owners: Digger Architecture
- Applies to: Project persistence, governed mutations, Phase 1 consistency island

## Context

Digger must atomically preserve this invariant for every governed Project mutation:

```text
Official Domain State
  + Commit
  + ProjectRevision
  + transactional Outbox events
```

The desktop-first, local-first product also needs portable Projects, deterministic crash recovery, optimistic conflict detection and offline operation. A mutation can touch Project metadata and one or more Creator Domains, so splitting those records across independent stores would require distributed commit or expose partial state.

WorkingState, model calls, tools, media processing, human review, search indexing and external publication are not part of this short authoritative transaction. They may be durable, but they have different consistency and failure semantics.

The architecture already selects one SQLite database per Project. This ADR makes that choice implementable by fixing the Phase 1 schema contract and the only public authoritative write API.

## Decision

Each Project is a directory containing one authoritative `project.db`. All state in the Project consistency island is stored in that database. There are no cross-Project transactions.

Only Rust persistence code behind `ProjectWriteCoordinator` may open an authoritative write transaction. The UI, Agent sidecar, tools and Creator Domain adapters cannot access SQLite directly or commit independently.

Every governed mutation executes in one `BEGIN IMMEDIATE` transaction and either:

1. returns a previously stored outcome for the same idempotency key;
2. commits Domain state, immutable Commit, one new ProjectRevision, Outbox events and the idempotent outcome together; or
3. leaves all authoritative state unchanged.

SQLite is the durability mechanism, not the Domain model. SQL rows and JSON payloads remain behind repositories and typed Domain interfaces.

## Project database boundary

`project.db` contains:

- Project identity, lifecycle and current revision;
- Official Creator Domain state and aggregate versions;
- ChangeSet and governance state required to authorize Apply;
- immutable Commit history and committed Change membership;
- transactional Outbox envelopes;
- idempotency outcomes;
- durable jobs, resource metadata and rebuild checkpoints when introduced.

It does not contain:

- application-wide recent-project catalog or preferences;
- credentials, provider secrets or OS keychain material;
- mutable editor recovery files or large transient production artifacts;
- exported files as authoritative state;
- raw resource bytes, which live in the Project resource store and are referenced by hash.

Non-authoritative tables may share `project.db` for operational simplicity, but they must be explicitly classified. Their failure must never make a committed revision ambiguous.

## Phase 1 schema contract

The first migration creates the following tables. Names are normative; exact SQLite storage encodings may evolve through migrations.

### `project`

Exactly one row exists.

| Column | Contract |
|---|---|
| `project_id` | Stable typed ID; primary key |
| `project_kind` | Creator-domain composition discriminator |
| `name` | Current Project display name |
| `lifecycle_state` | Explicit enum value |
| `project_revision` | Non-negative integer, initially `0` |
| `created_at`, `updated_at` | UTC timestamps |

`project_revision` is the serialization token for successful authoritative transitions. It is not a timestamp and is never reused.

### `domain_aggregate`

Phase 1 uses one explicit aggregate registry for concurrency and routing; Domain-owned tables hold actual state.

| Column | Contract |
|---|---|
| `aggregate_id` | Stable typed ID |
| `domain_kind` | Owning Creator Domain |
| `aggregate_kind` | Domain aggregate type |
| `aggregate_version` | Non-negative optimistic concurrency version |
| `state_schema_version` | Domain representation version |
| `created_revision`, `updated_revision` | Project revision provenance |

Unique key: `(domain_kind, aggregate_id)`. Domain tables must reference this identity or enforce an equivalent foreign-key relationship. The registry is not a universal JSON object store.

### `change_set` and `change_item`

`change_set` stores candidate identity, owning Actor, base Project revision, canonical fingerprint, governance status and lifecycle. `change_item` stores stable ordering, target aggregate, expected aggregate version, operation kind and a Domain-owned canonical payload or immutable payload reference.

Required constraints:

- `(change_set_id, ordinal)` is unique;
- the fingerprint covers ordered Change identities, targets, expected versions, operation kinds and canonical payload hashes;
- a committed ChangeSet has exactly one `commit_id`;
- a ChangeSet cannot return from `Committed` to a mutable state.

Mutable draft convenience data is not evidence of approval. Governance decisions bind the immutable fingerprint checked during Apply.

### `governance_decision`

Stores the immutable decision, Actor, policy version, bound ChangeSet fingerprint, decision time and optional expiry or conditions. Apply accepts only a current decision whose fingerprint matches the candidate exactly.

### `project_commit` and `commit_change`

`project_commit` is an immutable record of the accepted transition:

| Column | Contract |
|---|---|
| `commit_id` | Stable ID allocated for the operation |
| `project_revision` | Unique committed revision |
| `parent_revision` | Immediately preceding revision |
| `actor_id` | Effective initiating Actor |
| `change_set_id` | Unique governed candidate |
| `request_id`, `correlation_id`, `causation_id` | Trace and causal identity |
| `summary` | Bounded human-readable description |
| `committed_at` | UTC timestamp |

`commit_change` records the stable ordered Change membership and applied target version. Commit rows and their membership are append-only.

### `event_outbox`

Each Domain event produced by Apply is inserted in the same transaction.

| Column | Contract |
|---|---|
| `event_id` | Stable unique ID |
| `project_revision` | Revision that produced the event |
| `event_ordinal` | Stable order within the Commit |
| `domain_kind`, `event_kind` | Typed routing fields |
| `subject_id` | Stable affected subject, when applicable |
| `schema_version` | Event payload schema version |
| `payload_json` | Canonical event envelope payload |
| `correlation_id`, `causation_id` | Causal metadata |
| `created_at` | Commit time |

Unique key: `(project_revision, event_ordinal)`. Delivery state is not encoded by mutating the event envelope. Consumer checkpoints or delivery-attempt tables track progress independently.

### `idempotency_record`

| Column | Contract |
|---|---|
| `actor_scope` | Stable effective-Actor or caller scope |
| `idempotency_key` | Caller-supplied stable key |
| `request_fingerprint` | Canonical semantic request hash |
| `status` | `InProgress` or `Succeeded` in Phase 1 |
| `commit_id`, `project_revision` | Stored success identity |
| `outcome_json` | Bounded typed success response |
| `created_at`, `completed_at` | UTC timestamps |

Primary key: `(actor_scope, idempotency_key)`.

Reusing a key with a different request fingerprint is an error. Because `InProgress` is inserted inside the same SQLite transaction, rollback removes it; a durable Phase 1 record is therefore successful. The status remains explicit to support later multi-stage command types without changing the meaning of existing rows.

## Authoritative transaction API

The application-facing shape is a command, not generic CRUD:

```rust
pub struct ApplyChangeSetCommand {
    pub project_id: ProjectId,
    pub change_set_id: ChangeSetId,
    pub expected_project_revision: ProjectRevision,
    pub actor: EffectiveActor,
    pub idempotency_key: IdempotencyKey,
    pub request_id: RequestId,
    pub correlation_id: CorrelationId,
}

pub struct CommitOutcome {
    pub commit_id: CommitId,
    pub project_revision: ProjectRevision,
    pub affected_state_refs: Vec<StateRef>,
}

pub trait ProjectWriteCoordinator {
    fn apply_change_set(
        &self,
        command: ApplyChangeSetCommand,
    ) -> Result<CommitOutcome, ApplyError>;
}
```

The concrete Rust API may use async for connection acquisition, but once the write transaction begins it must not await network, model, tool, child-process or human work. Creator Domain Apply receives a coordinator-owned transaction capability with only bounded persistence operations; it cannot commit, roll back or acquire a second authoritative connection.

The transaction capability exposes typed operations rather than raw database ownership:

```rust
pub trait ProjectTransaction {
    fn load_state(&mut self, state_ref: &StateRef) -> Result<DomainState, TxError>;
    fn compare_version(&mut self, target: &AggregateRef, expected: u64) -> Result<(), TxError>;
    fn persist_domain_apply(&mut self, apply: DomainApply) -> Result<Vec<StateRef>, TxError>;
    fn append_domain_events(&mut self, events: Vec<DomainEvent>) -> Result<(), TxError>;
}
```

Repositories may implement these methods with SQLx. The trait is intentionally insufficient to finalize a Commit or advance the Project revision; those remain coordinator responsibilities.

## Apply algorithm

For a new idempotency key, the coordinator performs this exact logical order:

```text
BEGIN IMMEDIATE
  1. Read Project row and verify ProjectId/lifecycle.
  2. Look up (actor_scope, idempotency_key).
     - same fingerprint + success: return stored outcome after rollback/read exit;
     - different fingerprint: IdempotencyKeyReused;
  3. Verify expected ProjectRevision.
  4. Load immutable ChangeSet candidate and recompute/verify fingerprint.
  5. Re-check effective Actor, Delegation, permission, policy and governance decision.
  6. Compare every expected AggregateVersion and dependency StateRef.
  7. Insert the operation's idempotency marker.
  8. Execute deterministic Domain Apply in stable Change order.
  9. Persist Domain state and increment affected AggregateVersions.
 10. Compare-and-swap ProjectRevision from N to N + 1.
 11. Insert immutable Commit and ordered Commit membership for N + 1.
 12. Insert ordered Outbox events for N + 1.
 13. Mark the ChangeSet Committed and bind its CommitId.
 14. Store the successful idempotent outcome.
COMMIT
```

Any error rolls back steps 7–14 together. Validation or impact work performed earlier is advisory; all authority and concurrency preconditions are checked again against transaction-local authoritative state.

The implementation may move insert statements within the transaction to satisfy foreign keys, but observable semantics and atomicity must remain identical.

## Concurrency and locking

- One process holds the Project package write lock in v1.
- SQLite uses `foreign_keys = ON`, WAL mode, explicit bounded `busy_timeout` and an application-defined `synchronous` policy.
- `BEGIN IMMEDIATE` acquires the SQLite writer reservation before final checks, so two local write paths cannot both validate and then silently overwrite.
- Expected ProjectRevision detects stale command context.
- Expected AggregateVersion detects stale targets even when a caller intentionally accepts a newer unrelated Project revision in a future command type.
- Long reads use separate read connections and expose their observed ProjectRevision.
- Busy/locked exhaustion returns a typed transient error; it is not reported as a Domain conflict.

The single-process lock reduces accidental concurrent ownership but does not replace database constraints, revision compare-and-swap or aggregate version checks.

## Error contract

Apply returns typed errors sufficient for safe UI and retry behavior:

- `ProjectNotWritable` — lifecycle or package lock prevents mutation;
- `ProjectRevisionConflict { expected, actual }`;
- `AggregateVersionConflict { target, expected, actual }`;
- `ChangeSetFingerprintMismatch`;
- `GovernanceRejectedOrStale`;
- `PermissionDenied`;
- `IdempotencyKeyReused`;
- `DatabaseBusy` — bounded transient contention;
- `InvariantViolation` — corrupted or contradictory authoritative state;
- `PersistenceFailure` — transaction did not commit.

No error may imply success unless the coordinator has confirmed the durable idempotency outcome. If the caller loses the response after commit, retrying the same key returns that outcome.

## Integrity constraints

The database enforces, rather than merely documents:

- foreign keys for authoritative relationships;
- unique Project revision per Commit;
- one Commit per committed ChangeSet;
- unique Outbox ordering within a revision;
- non-negative revisions and versions;
- legal lifecycle/status values through strict tables or checked constraints;
- immutable-history protection through repository API and update/delete-denial tests.

SQLite triggers are reserved for local integrity that cannot be expressed with keys or checks. Domain behavior stays in Rust; hidden trigger workflows must not become an alternate application layer.

## Migration and open behavior

Schema migration mechanics are decided by ADR-004. This ADR requires that Project opening:

1. obtains the package lock before migration or writable access;
2. rejects a newer unsupported schema without modifying it;
3. applies migrations before exposing repositories;
4. runs `foreign_key_check` after non-trivial migration tests and supports explicit integrity diagnostics;
5. never creates a second database to split the authoritative consistency island.

`PRAGMA user_version` may assist bootstrapping, but the migration ledger is the auditable source for applied migration identity, checksum, application version and completion time.

## Alternatives considered

### One application-wide SQLite database

Rejected because Project portability, backup, lifecycle and failure isolation become coupled. Moving or sharing one closed Project would require extracting a consistent subset.

### Separate database per Creator Domain

Rejected for v1 because a multi-domain governed mutation could not atomically update Official State, Commit, revision and Outbox without distributed coordination.

### Event sourcing as the sole authority

Rejected because Digger does not need replay to define all current Domain state, and schema evolution plus large creative documents would make event replay the primary persistence burden. Immutable Commit and events remain history and integration records, not the only state store.

### Generic object table with JSON payloads

Rejected because it weakens typed invariants, migrations, ownership and query planning. JSON remains valid for versioned Domain payloads and immutable envelopes when paired with typed identity, version and ownership columns.

### Optimistic transactions without `BEGIN IMMEDIATE`

Rejected for the final Apply path. Deferred writer acquisition creates avoidable late lock upgrades after authoritative checks. Optimistic versions are still required for semantic conflicts.

## Consequences

### Positive

- The core mutation invariant maps directly to one local ACID transaction.
- A Project is portable and independently recoverable.
- Lost responses and command retries do not duplicate mutations.
- Domain modules remain semantically independent without becoming distributed services.
- Outbox consumers and projections may fail or lag without corrupting Official State.

### Costs

- All authoritative Domain Apply implementations must participate in the Rust transaction boundary.
- SQLite permits one writer at a time per Project, so Apply must stay short and bounded.
- Cross-Project atomic operations are unavailable.
- Schema evolution must coordinate shared core tables and Domain-owned migrations.
- Large payloads require immutable resource references or prepared values rather than long transaction work.

## Validation requirements

ADR implementation is complete when automated tests prove:

1. Domain state, Commit, revision, Outbox and idempotency outcome commit together;
2. an injected failure after each transaction stage leaves all five unchanged;
3. retry after a lost success response returns the original outcome and creates no new Commit;
4. reusing an idempotency key with a different fingerprint fails;
5. stale ProjectRevision and stale AggregateVersion produce distinct conflicts;
6. stale or mismatched governance fingerprint cannot Apply;
7. two competing writers produce at most one successful transition from the same base revision;
8. Outbox event ordering is stable and uniquely constrained;
9. foreign-key and invariant violations cannot create a partially visible Commit;
10. closing, moving and reopening a Project preserves identical authoritative state;
11. read projections report the revision they observed;
12. no Agent, WebView or Domain adapter can obtain an unrestricted SQLite write handle.

