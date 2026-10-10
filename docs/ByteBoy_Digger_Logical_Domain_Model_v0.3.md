# ByteBoy Digger Logical Domain Model v0.3

> Status: Logical Domain Model — Frozen Draft  
> Product: ByteBoy Digger — AI Creator  
> Scope: Project-centric creator runtime and domain boundary model  
> Version: v0.3

---

## 1. Purpose

Digger is a Project-centric AI Creator for content/media-centric creation rather than a universal workspace.

Its current creator domains are:

- Social Creator
- Long-form Writing
- AGC/OGC Website

The central design principle is:

> **Do not only optimize the Output; optimize the Project that produces the Output.**

Outputs are consumed. Projects accumulate:

```text
Knowledge
+ Rules
+ Structure
+ Decisions
+ Skills
+ Workflows
+ Templates
+ Assets
+ Feedback
+ Production History
```

Digger Core does not attempt to understand every creator artifact. It provides the mechanics by which Humans and Agents understand a Project, act on it, govern changes, preserve history, interact with the external world, and learn from observations.

> **Core owns evolution mechanics; Domain owns creator semantics.**

---

# 2. Project-centric Boundary

## 2.1 Project

A Project is:

> A long-lived boundary within which Humans, Agents and Tools continuously work toward a creator goal, carrying its resources, knowledge, rules, history and production activity.

```text
Project
≠ Content
≠ Work
≠ Account
≠ Site
```

Project is best understood as:

```text
Boundary
+ Runtime
+ Persistent Context
```

Conceptually:

```text
Project
│
├── Identity
├── Goal / Intent
├── Actors
│   ├── Human
│   └── Agent
├── Resources
├── Knowledge
├── Rules
├── Tools / Skills
├── History
└── Domain
```

These are conceptual responsibilities, not necessarily one database entity each.

---

# 3. Creator Domains

## 3.1 Social Project

Domain Root: **Brand / Creator**

```text
Social Project
│
├── Brand / Creator
│   ├── Identity
│   ├── Positioning
│   ├── Voice / Style
│   ├── Audience
│   └── Guidelines
│
├── Channel
│   ├── Platform
│   ├── Account
│   ├── Audience Profile
│   └── Platform Rules
│
├── Strategy
│   ├── Topic
│   ├── Content Pillar
│   ├── Series
│   ├── Campaign
│   └── Calendar
│
├── Content
│   ├── Idea
│   ├── Research
│   ├── Source
│   ├── Copy / Script
│   ├── Asset
│   └── Variant
│
├── Media Production
│   ├── Image Project
│   ├── Video Project
│   └── Audio Project
│
├── Publication
│   ├── Channel
│   ├── Variant
│   ├── Schedule
│   ├── Status
│   └── External Result
│
└── Feedback
    ├── Metrics
    ├── Audience Response
    ├── Attribution
    └── Learning
```

Important relationship:

```text
Content ≠ Publication
```

A Content object can have many platform-specific Publications.

---

## 3.2 Writing Project

Domain Root: **Work**

```text
Writing Project
│
├── Work
│   ├── Title
│   ├── Premise / Thesis
│   ├── Intent
│   ├── Audience
│   ├── Genre / Type
│   └── Style
│
├── Structure
│   ├── Outline
│   ├── Part
│   ├── Chapter
│   ├── Section
│   └── Scene
│
├── Knowledge
│   ├── Source
│   ├── Note
│   ├── Concept
│   └── Reference
│
├── Truth
│   ├── Narrative
│   │   ├── Character
│   │   ├── World
│   │   ├── Timeline
│   │   └── Canon
│   └── Research
│       ├── Claim
│       ├── Evidence
│       └── Citation
│
├── Manuscript
│   ├── Draft
│   ├── Passage
│   └── Revision
│
└── Edition
    ├── Version
    ├── Compile
    └── Export
```

Semantic separation:

```text
Structure = where writing belongs
Truth     = what writing must obey
Knowledge = what writing can draw from
Manuscript = what has actually been written
```

Narrative writing is primarily constrained by **Canon**; research/non-fiction writing is primarily constrained by **Evidence**.

---

## 3.3 Site Project

Domain Root: **Site**

```text
Site Project
│
├── Site
│   ├── Identity
│   ├── Purpose
│   ├── Audience
│   ├── Domain
│   └── Configuration
│
├── Information Architecture
│   ├── Route
│   ├── Navigation
│   ├── Taxonomy
│   └── Relation
│
├── Content Model
│   ├── Schema
│   ├── Collection
│   ├── Field
│   └── Relation
│
├── Content Program
│   ├── Topic Space
│   ├── Keyword Space
│   ├── Audience Intent
│   ├── Generation Rule
│   ├── Update Rule
│   └── Internal Linking Rule
│
├── Content
│   ├── Entry
│   ├── Page
│   ├── Block
│   └── Asset
│
├── Presentation
│   ├── Design Token
│   ├── Component
│   ├── Template
│   └── Layout
│
├── Publication
│   ├── Draft
│   ├── Preview
│   ├── Build
│   ├── Deploy
│   └── Published State
│
└── Feedback
    ├── Search
    ├── Traffic
    ├── Engagement
    ├── Conversion
    └── Content Performance
```

Production chain:

```text
Content Program
       ↓ what to produce
Content Model
       ↓ shape
Knowledge / Data
       ↓ material
Template
       ↓ presentation
Entry / Page
       ↓
Publication
       ↓
Feedback
       ↓
Content Program
```

---

# 4. Core vs Domain Boundary

Digger uses a three-layer mental model:

```text
Digger
=
Project Runtime
+
Creator Domains
+
Agent Runtime
```

Core does **not** own creator-specific concepts such as:

```text
Content
Document
Artifact
Page
Post
Video
Chapter
Publication
Edition
Deployment
Campaign
Workflow Definition
Analytics
Feedback
Canon
Claim
Schema
```

These remain Domain semantics.

Core owns the mechanics by which Project state evolves.

> **Core owns transitions; Domain owns meaning.**

---

# 5. Bounded Contexts

The logical contexts are organized around responsibilities rather than technical modules.

```text
Digger Core
├── Project Context
├── Knowledge Context
├── Production Context
├── Governance Context
└── External Context

Creator Domains
├── Social Context
├── Writing Context
└── Site Context
```

Cross-cutting mechanisms such as Context Assembly, Project Revision, Provenance and Causal History are not necessarily independent Bounded Contexts.

---

# 6. Project Knowledge and Project Truth

## 6.1 Knowledge chain

```text
Source
   ↓
Knowledge
   ↓
Assertion
   ↓
Decision
   ↓
Project Truth
```

Not every piece of information traverses every stage.

### Source

Where did this information come from?

### Knowledge

What can the Project retrieve and understand?

### Assertion

A judgeable statement about the Project or world.

### Decision

A deliberate choice made for the Project.

### Project Truth

Not a generic memory store, but authoritative state that future production must respect.

---

## 6.2 Knowledge ≠ Truth

Project Truth is better modeled as **Authority over state/knowledge**, not a separate universal memory bucket.

```text
Knowledge Graph
       │
       ├── ordinary knowledge
       ├── proposed assertion
       ├── accepted assertion
       ├── decision
       └── authoritative
                ↑
          Project Truth
```

Authority may apply to:

```text
Fact
Canon
Decision
Rule
Contract
Accepted Claim
Strategy
Preference
```

The common semantic is not the label but how strongly the subject constrains future Actor behavior.

Authority needs concepts such as:

```text
Scope
Priority
Provenance
Rationale
Validity
Status
```

---

## 6.3 Evidence as relationship semantics

Evidence should not become a universal Core Entity.

Prefer:

```text
Knowledge
├── supports → Assertion
├── contradicts → Assertion
└── contextualizes → Assertion
```

---

# 7. Context Assembly

Agent Context is assembled, not stored as one giant object.

```text
                 Current Task
                      │
                      ↓
                Context Scope
                      │
          ┌───────────┼───────────┐
          ↓           ↓           ↓
      Knowledge    Authority    Working State
          │           │           │
       retrieve     resolve      inspect
          └───────────┼───────────┘
                      ↓
                Agent Context
                      ↓
                    Agent
```

Core provides Project-level context; Creator Domains provide domain-specific context.

```text
Agent Context
=
Relevant Knowledge
+ Resolved Authority
+ Current Working State
+ Relevant Domain Context
```

---

# 8. Actor Model

```text
Project
├── Actor
│   ├── Human
│   └── Agent
└── Capability
    ├── Tool
    ├── Skill
    └── Service
```

Actor:

> A subject holding Intent and capable of initiating Action within Project Context.

## 8.1 Agent

```text
Agent
├── Identity
├── Role
├── Instructions
├── Context Policy
├── Authority
├── Skills
├── Tool Access
├── Model
└── Working State
```

LLM ≠ Agent.

Prefer:

```text
Agent → Role → Responsibility → Authority → Capability
```

rather than persona-centric modeling.

---

## 8.2 Permission

Actor permission answers:

> Who can perform which kind of operation within which scope?

Possible permissions include:

```text
Read
Create
Propose
Modify
Approve
Publish
Delete
Configure
```

Important:

```text
Create ≠ Propose ≠ Modify ≠ Approve
```

Project Authority and Actor Permission are different concepts:

```text
Project Authority = which information/rules are authoritative
Actor Permission  = who may perform which actions
```

---

## 8.3 Delegation

Agent autonomy is modeled as delegation.

```text
Delegation
├── Goal
├── Role
├── Scope
├── Permissions
├── Tools
├── Skills
├── Budget
├── Duration
└── Review Policy
```

Autonomy is bounded by:

```text
Delegated Scope
+ Permission
+ Risk Policy
+ Budget
+ Time
```

A Delegation exists only once authority has actually been granted. A proposed delegation belongs to Proposal/Decision rather than a `PendingDelegation` lifecycle.

---

# 9. Tool, Skill, Workflow and Policy

```text
Workflow = WHAT / WHEN
Role     = WHO
Skill    = HOW WELL
Tool     = HOW
Policy   = WHETHER ALLOWED
```

Tool is a capability interface used to execute an Action.

Skill is know-how.

Workflow is a multi-step process.

Policy determines governance/authorization consequences.

---

# 10. Issue, Proposal, Intent and Candidate

These concepts form different stages of Project deliberation.

```text
Issue = What is wrong?
Proposal = What should we do?
Candidate = What exactly should change?
```

## 10.1 Issue

```text
Issue
├── Target
├── Type
├── Severity
├── Detected By
├── Evidence / References
├── Description
└── Status
```

Issue Resolution should preserve causal references, for example:

```text
Issue
→ Proposal
→ Commit
```

rather than only `status = done`.

## 10.2 Proposal

Proposal expresses a possible future direction or action.

Proposal acceptance creates a Decision, not necessarily an Intent.

```text
Proposal
    ↓
ProjectDecision
    ↓
Effect
   ↙ ↓ ↘
Intent
Change
Delegation
Authority
...
```

## 10.3 Intent

Intent means the Project has committed to pursuing a goal.

```text
Proposal = consideration
Intent   = commitment to act
```

One Intent can produce many Production Runs.

Run completion does not imply Intent fulfillment.

## 10.4 Candidate

Candidate should not be treated as a universal independent Entity.

It is better understood as the governance role of a concrete proposed Change/ChangeSet that has not yet entered Official State.

```text
Proposal ≠ Candidate
```

Proposal asks whether to pursue something.

Candidate asks whether to apply a concrete state change.

---

# 11. Production Model

Production is the work performed by Actors to advance Project Intent.

It may include:

```text
Research
Generate
Edit
Transform
Organize
Analyze
Compose
Review
Publish preparation
Maintain
```

## 11.1 Production Run

Production Run is a logical work unit for one Intent.

It is not an Agent Session, chat session or OS process.

```text
Production Run
├── Intent
├── Actors
├── Context
├── Inputs
├── Actions
├── Working State
├── Intermediate Results
├── Changes
├── Commits
└── Outcome
```

One Production Run may span multiple Agent Sessions and Human Review Sessions.

---

## 11.2 Action

```text
Action
├── Actor
├── Intent
├── Target
├── Operation
├── Input
├── Context
├── Tool
└── Result
```

Action describes work execution.

It does not imply mutation of Official State.

---

## 11.3 Result

Result is the immediate output of an Action.

```text
Result ≠ Change
```

A search result, generated draft, analysis or rendered preview may remain in Working State without becoming an Official Project mutation.

---

## 11.4 Working State

Working State holds intermediate, mutable production state that is not yet authoritative Project state.

```text
Working State ≠ Official State
```

---

# 12. Change Model

Digger Core should not attempt to unify all content types.

It unifies **change**.

```text
Intent → Action → Result → Change → Governance → Official State
```

## 12.1 Change

A Change is a semantic transition:

```text
S₀ → S₁
```

Logical structure:

```text
Change
├── ChangeId
├── Target
├── BaseStateRef
├── Operation
├── Payload
├── Preconditions
└── Provenance
```

Core owns the protocol fields:

```text
Target
BaseStateRef
Preconditions
Provenance
```

Domain owns:

```text
Operation
Payload
```

Principle:

> **Core owns change protocol; Domain owns change meaning.**

---

## 12.2 Change Set

Bulk and cross-object work requires Change Set.

```text
Change Set
├── Intent
├── Changes[]
├── Scope
├── Summary
├── Impact
├── Validation
└── Governance
```

Examples:

- Site generation changes 5,000 entries.
- Writing terminology rename affects 80 manuscript locations.
- Social campaign update affects many variants/publications.

A material modification to a Change Set invalidates previous governance assessments.

---

# 13. Validation and Impact

## 13.1 Domain Validation

Answers:

> Is this Change legal and meaningful within the Creator Domain?

Examples:

- Chapter exists.
- Schema field type is valid.
- Canon relationship is valid.
- Publication variant structure is valid.

## 13.2 Governance Validation

Answers:

> Has this Change satisfied the conditions required to enter Official State?

Examples:

- Actor authorized.
- Review completed.
- Impact assessed.
- Base state not stale.
- Policy satisfied.

```text
Validation
├── Domain Validation
└── Governance Validation
```

---

## 13.3 Impact Analysis

Domain discovers semantic impact; Core evaluates governance consequences.

Example:

```text
Site Schema change
      ↓
Domain Impact Analysis
      ↓
8,732 Entries
12 Templates
4 Linking Rules
3 Generation Rules
      ↓
Governance Policy
      ↓
Review requirement
```

Important principle:

> **Change Size ≠ Impact Size.**

Risk should be driven by semantic impact, not diff size alone.

---

# 14. Governance

Governance is not synonymous with Human Approval.

```text
Candidate / ChangeSet
   ↓
Policy Evaluation
   ↓
Auto Apply / Review / Reject / Block
   ↓
Commit
   ↓
Official State
```

Low-risk work can be automatically committed by policy.

Human is one possible Reviewer, not the definition of Governance.

---

## 14.1 Permission vs Policy

Permission answers:

> Can this Actor perform this class of operation?

Policy answers:

> Given this Change, Risk and Impact, what governance path is required?

An Agent may have permission to propose or modify Manuscript state while policy still requires Human review before Commit.

---

# 15. Commit

Commit is:

> An immutable governance record stating that one or more governed Changes have successfully entered Official Project State.

```text
Commit ≠ Domain Event
```

Commit records governance/state-transition semantics.

Domain Event records a business fact resulting from a successful transition.

A Commit may contain multiple Changes and may cause multiple Domain Events.

---

## 15.1 Commit Protocol

Logical protocol:

```text
ChangeSet Ready
      ↓
GovernanceDecision
      ↓
Prepare Commit
      ↓
Domain validates current state
      ↓
Domain Apply Command
      │
      ├── Conflict / Invariant failure → Blocked
      │
      └── Success
             ↓
       New StateRefs
             ↓
       Finalize Commit
             ↓
      ProjectRevision
             ↓
       Domain Events
```

Hard invariants:

> **No finalized Commit without corresponding official Domain State.**

and:

> **No governed official Domain mutation without corresponding Commit.**

This is a logical consistency requirement, not a decision about database transactions or Event Sourcing.

---

# 16. Snapshot

Snapshot is:

> An immutable reference to Project State or a scoped portion of State at a point in time.

```text
Snapshot
├── Scope
├── State Reference
├── Created At
├── Provenance
└── Label
```

Commit and Snapshot differ:

```text
Commit   = state transition record
Snapshot = stable state reference
```

Publications, Editions and Deployments should refer to stable snapshots/versions rather than mutable current state.

---

# 17. Identity and Versioning

## 17.1 DomainTarget

`DomainTarget` identifies **what** object or sub-object is addressed.

Example conceptual target:

```text
Context: Writing
Aggregate: Manuscript M1
Entity: Chapter C82
Path: title
```

Core can carry the address, but only Writing Domain understands its semantics.

## 17.2 StateRef

`StateRef` identifies **which state/version** is being referenced.

## 17.3 AggregateVersion

Local optimistic concurrency/versioning for an Aggregate.

## 17.4 ProjectRevision

A Project-wide logical clock for authoritative Project-visible state.

```text
AggregateVersion
= local concurrency

ProjectRevision
= global project consistency
```

ProjectRevision is not equivalent to Commit sequence because authoritative operational state transitions can also advance the Project.

---

# 18. Lifecycle / State Machines

Not every Entity should have a `status`.

State machines are reserved for objects whose lifecycle stages change allowed behavior, permissions or invariants.

## 18.1 Project

```text
Active
  ↓ pause
Paused
  ↓ resume
Active

Active / Paused
  ↓ archive
Archived
```

Core lifecycle remains intentionally thin:

```text
Active
Paused
Archived
```

---

## 18.2 Agent

```text
Active
Disabled
Retired
```

Agent lifecycle is different from Agent Session execution state.

```text
Agent Status ≠ Agent Execution Status
```

---

## 18.3 Delegation

```text
Active
├── Completed
├── Revoked
└── Expired
```

Only Active Delegation authorizes actions.

Completed/Revoked/Expired Delegations are terminal. New authorization creates a new Delegation rather than resurrecting old history.

---

## 18.4 Issue

```text
Open
├── Resolved
└── Dismissed
```

Resolution should preserve causal references.

---

## 18.5 Proposal

```text
Proposed
├── Accepted
├── Rejected
├── Deferred
└── Superseded

Deferred → Proposed
```

Accepted/Rejected/Superseded are terminal for that Proposal record.

---

## 18.6 Intent

```text
Active
├── Fulfilled
└── Cancelled
```

Intent has no Pending state because an unaccepted idea is a Proposal.

---

## 18.7 Production Run

```text
Created
   ↓ start
Running
   ├── pause → Paused → resume → Running
   ├── complete → Completed
   ├── fail → Failed
   └── cancel → Cancelled
```

Failed is terminal. Retry creates a new Production Run with a causal reference such as `retries R1`.

Paused may carry a PauseReason such as:

```text
AwaitingHuman
AwaitingExternal
BudgetExceeded
PolicyBlocked
ManualPause
```

without turning each reason into a separate state.

ProductionRun Completed does not imply its Changes were committed.

---

## 18.8 Assertion

```text
Proposed
├── Accepted
├── Rejected
└── Superseded

Accepted
├── Superseded
└── Retracted
```

Rejected means never accepted.

Retracted means previously accepted and later withdrawn.

AuthorityBinding validity is temporal and should not be confused with lifecycle state.

---

## 18.9 Change Set

```text
Draft
  ↓ submit
Ready
  ↓ governance
├── Committed
├── Rejected
└── Blocked
     ↓ revise
    Draft
```

Semantics:

```text
Draft = mutable
Ready = frozen for evaluation
```

A material modification returns the Change Set to Draft and invalidates prior validation/impact/risk/governance assessments.

Committed and Rejected are terminal.

Revert is represented as a new Change Set, not by reopening a committed one.

---

## 18.10 External Action

```text
Created
  ↓ authorize
Authorized
  ↓ execute
Executing
├── Succeeded
├── Failed
└── Cancelled
```

Failed is terminal. Retry creates a new External Action.

External retries must not assume a previous failed attempt had no side effect; Observe/Reconcile may be required before retry.

---

# 19. Stateful Objects vs Immutable Records

## 19.1 Stateful Objects

```text
Project
Agent
Delegation
Issue
Proposal
Intent
ProductionRun
Assertion
ChangeSet
ExternalAction
```

## 19.2 Immutable Records

```text
Decision
Commit
Snapshot
Observation
ValidationReport
ImpactAssessment
RiskAssessment
```

Immutable records should prefer:

```text
append
supersede
correct
```

over rewriting history.

This distinction forms the basis of Project Causal History without requiring Event Sourcing.

---

# 20. Command Model

Command is:

> An explicit request to an owning Aggregate to perform a business state transition.

```text
Command
├── CommandId
├── ProjectId
├── Target Aggregate
├── ActorRef
├── Operation
├── Parameters
├── ExpectedVersion?
├── CauseRef?
└── CorrelationRef?
```

Command is imperative.

Domain Event is a past-tense fact.

Example:

```text
AcceptProposal   → command
ProposalAccepted → event
```

A Command may be rejected by authorization checks or domain invariants.

---

# 21. Action vs Command

```text
Action
= work execution

Command
= authoritative state transition request
```

Actions such as web search, reading sources or generating draft alternatives need not modify Official State.

An Action may produce a Change, and a governed Commit may later result in a Domain Command.

Typical Agent path:

```text
Action
 ↓
Result
 ↓
Change
 ↓
Governance
 ↓
Commit
 ↓
Domain Command
 ↓
Domain Aggregate
```

Operational Commands such as `PauseProject` do not necessarily require a Change Set.

---

# 22. Official Creator State Mutation Rule

A central v0.3 invariant is:

> **Any mutation to official Creator Domain State is represented as a Change before it becomes official.**

Sources include:

```text
Human Edit
Agent Production
Import
Migration
External Sync
Automation
```

All use the same logical path:

```text
Mutation Intent
      ↓
Change / ChangeSet
      ↓
Governance Policy
      ↓
Commit
      ↓
Domain Apply
      ↓
Official State
```

Human direct editing can still feel immediate because trusted low-risk changes may auto-commit by policy.

Governance does not mean mandatory review.

---

# 23. Command Classes

Three important classes:

```text
Command
│
├── Operational Command
├── Project Mutation Command
└── External Side-Effect Command
```

### Operational

Examples:

```text
StartRun
PauseRun
ResolveIssue
```

### Project Mutation

Examples:

```text
ApplyWritingChanges
UpdateBrandStrategy
ModifySiteSchema
```

These must respect Change Governance.

### External Side Effect

Examples:

```text
PublishPost
DeploySite
DistributeEdition
```

These require External authorization/governance.

---

# 24. Domain Events

Domain Event is:

> An immutable, domain-significant fact that has already occurred inside a Bounded Context.

Examples:

```text
Project.ProjectPaused
Production.ProductionRunCompleted
Governance.ChangeSetCommitted
Writing.ManuscriptChanged
External.ExternalActionSucceeded
```

Not every field mutation deserves a Domain Event.

Detailed semantic differences belong to Change.

```text
Change
= semantic delta

Domain Event
= business fact
```

Similarly:

```text
Commit
= governance transition record

Domain Event
= domain fact
```

One Command may produce zero or many Domain Events.

---

# 25. Domain Event Envelope

Cross-context propagation may use a common envelope:

```text
DomainEventEnvelope
├── EventId
├── ProjectId
├── Context
├── EventType
├── AggregateRef
├── AggregateVersion
├── ProjectRevision
├── ActorRef?
├── CauseRef?
├── CorrelationId
├── CausationId
├── Timestamp
└── Payload
```

Event semantics remain owned by the originating Context.

---

# 26. Causation and Correlation

## 26.1 Causation

Answers:

> What directly caused this event/object to exist?

## 26.2 Correlation

Answers:

> Which larger Project story does this belong to?

```text
Causation = immediate why
Correlation = broader story
```

Example:

```text
Observation
 ↓
Issue
 ↓
Proposal
 ↓
Decision
 ↓
Intent
 ↓
ProductionRun
 ↓
ChangeSet
 ↓
Commit
```

The chain can share one CorrelationId while each step records its direct CausationId.

---

# 27. Project History

Project History is not an Activity Log.

## 27.1 State History

Answers:

> What changed?

Includes:

```text
Commit
Aggregate Version
ProjectRevision
Snapshot
```

## 27.2 Causal History

Answers:

> Why did it change?

Includes relationships among:

```text
Issue
Proposal
Decision
Intent
ProductionRun
ChangeSet
Commit
Observation
```

## 27.3 Execution Trace

Answers:

> How was it executed technically?

Includes:

```text
Agent Session
Tool Call
Model Call
Runtime Error
Token Usage
```

Execution Trace belongs primarily to Runtime/Observability rather than Core Project History.

> **Observability is infrastructure; History is domain projection.**

---

# 28. Agent Runtime Boundary

Agent Runtime performs reasoning, planning and tool execution.

```text
Agent Session
   ↓
Reason / Plan
   ↓
Tool Call
   ↓
Result
```

Most of this is Runtime/Observability.

When Agent behavior has Project meaning, it enters the Domain through Commands or governed Changes.

```text
Agent Runtime
      │
reasoning / tools / sessions
      │
      ↓
Application Layer
      │
    Command
      ↓
Domain Context
```

Agent Runtime must not directly persist Creator Domain State.

Human UI, Agent, Automation and Importer are symmetric clients of the same Domain rules:

```text
Human UI ──┐
Agent ─────┼→ Command / Change → Domain
Automation ┤
Importer ──┘
```

No actor gets a mutation backdoor.

---

# 29. Capability Effect Class

Domain capabilities should declare their effect class.

```text
EffectClass
│
├── ReadOnly
├── WorkingState
├── ProjectMutation
└── ExternalSideEffect
```

Routing implications:

```text
ReadOnly
→ execute

WorkingState
→ Production Run

ProjectMutation
→ Change Governance

ExternalSideEffect
→ External Authorization
```

This forms a stable bridge between Domain Capability and Runtime/Governance routing.

---

# 30. External Interaction

Publication is first-class in Social, Deployment in Site, Distribution in Writing, but none should become a universal Core object.

Core abstraction:

```text
External Action
├── Destination
├── Payload Snapshot
├── Permission
├── Risk
├── Reversibility
├── Result
└── External Reference
```

External Action operates on stable payload state.

```text
Created ExternalAction
→ Destination fixed
→ PayloadSnapshotRef fixed
```

It must not authorize one payload and later execute against arbitrary mutable current state.

---

## 30.1 Reversibility

External actions can be difficult or impossible to reverse.

Low reversibility should increase approval/governance requirements.

Compensation is modeled as a new External Action:

```text
EA1: Publish → Succeeded
EA2: Delete  → Succeeded
EA2 compensates EA1
```

rather than rewriting EA1 to `Compensated`.

---

## 30.2 Retry and Idempotency

A failed external attempt may have produced a real-world side effect despite a timeout/error.

Therefore:

> **Retrying an External Action must never assume the previous attempt had no side effect.**

Potential path:

```text
Failed attempt
 ↓
Observe / Reconcile External State
 ↓
Retry / Compensate / Adopt existing result
```

External integration should support idempotency/reconciliation where possible.

---

# 31. External State and Observation

Digger distinguishes:

```text
Internal State
Expected External State
Observed External State
External Drift
```

Observation is:

> An immutable fact-of-observation about Project state or the external world.

Observation should not be silently rewritten when later found inaccurate; a later Observation/Assertion can correct or reinterpret it.

Key distinction:

```text
Observation = what I saw
Issue       = what deserves handling
Assertion   = what I believe it means
```

Typical flow:

```text
Observation
   ↓ interpretation
 ↙      ↘
Issue  Assertion
```

---

# 32. Feedback and Learning

Feedback is Domain semantics, not a universal Core object.

Core sees external Observation; Domain interprets it as:

```text
Social → engagement / audience response
Writing → reader/reviewer feedback
Site → traffic / SEO / conversion
```

Raw feedback must not directly mutate Project Truth.

Preferred path:

```text
Raw Feedback
→ Observation
→ Analysis
→ Assertion
→ Learning Proposal / Candidate
→ Decision
→ Project Change
```

Thus Project can evolve from external feedback without surrendering governance.

---

# 33. Digger Domain Protocol

The Core-to-Creator boundary is intentionally small.

Four principal Creator Domain Contracts are sufficient:

```text
Creator Domain Contract
│
├── State Contract
├── Context Contract
├── Governance Contract
└── Capability Contract
```

Domain Events form a common output protocol rather than a fifth independent business contract.

---

# 34. State Contract

Answers:

> What is this object and what state/version is it in?

Logical responsibilities:

```text
State Contract
├── Resolve Target
├── Read State
├── Resolve Version
└── Describe Target
```

Core defines a stable addressing protocol such as `DomainTarget` and `StateRef`.

Only the Domain understands whether a Chapter, Page, Variant, Schema Field, etc. exists and what it means.

StateView is a controlled projection, not a dump of the entire Domain object.

```text
State ≠ StateView
```

Domain always owns State.

---

# 35. Context Contract

Answers:

> Given Actor, Intent and Target, what domain-specific context should the Agent see?

Input may include:

```text
Context Request
├── Actor
├── Intent
├── Target
├── Task Type
├── Scope
└── Budget
```

Domain returns a projection such as:

```text
Domain Context
├── Relevant State
├── Domain Constraints
├── Domain Relationships
├── Domain Guidance
└── Authority-capable Subjects
```

The Domain exposes authority-capable subjects; Knowledge Context owns AuthorityBinding and effective authority resolution.

Context Contract must be side-effect free with respect to Official Project State.

---

# 36. Governance Contract

Answers:

> What does this Change mean in the Domain, what does it affect, and can it be safely applied?

Logical surface:

```text
Governance Contract
├── Validate
├── Analyze Impact
├── Preview
└── Apply
```

### Validate

Checks Domain semantics/invariants.

### Analyze Impact

Discovers semantic dependencies and affected state.

### Preview

Produces a domain-appropriate human/agent projection.

Preview is broader than textual Diff.

Examples:

```text
Writing → text/outline/canon preview
Site → schema/page/dependency preview
Social → post/platform/storyboard preview
```

### Apply

The only mutation side of this contract.

It must re-check current target/version/preconditions/domain invariants.

Possible result:

```text
ApplyResult
├── PreviousStateRefs
├── NewStateRefs
├── AffectedTargets
└── DomainEventPayloads
```

Possible failure:

```text
Conflict
InvariantViolation
TargetMissing
PreconditionFailed
```

Important boundary:

```text
ASSESSMENT SIDE
validate / impact / preview
read-only

──────── Commit Boundary ────────

APPLY SIDE
state mutation
```

Governance approval never bypasses Domain invariants.

---

# 37. Capability Contract

Answers:

> What meaningful business actions does this Creator Domain support?

Capability is not Tool.

```text
Capability = domain semantic action
Tool       = implementation mechanism
```

Example capability definition:

```text
CapabilityDefinition
├── Key
├── Intent Types
├── Input Contract
├── Target Types
├── Required Permission
├── Effect Class
├── Possible Outputs
└── Tool Requirements?
```

Examples:

### Writing

```text
DraftSection
RewritePassage
MoveChapter
CheckConsistency
CompileEdition
```

### Social

```text
ResearchTopic
CreateContent
CreateVariant
SchedulePublication
```

### Site

```text
GenerateEntry
UpdateSchema
RegenerateCollection
BuildSite
```

Domain defines WHAT can be done.

Agent Runtime determines HOW to perform it.

A Capability classified as ProjectMutation must not bypass Change Governance.

---

# 38. Domain Event Contract

Domain owns event semantics.

Core owns the common envelope/history infrastructure.

Domain returns semantic event payloads; Core adds:

```text
EventId
ProjectId
ContextKey
AggregateRef
AggregateVersion
ProjectRevision
ActorRef
CausationId
CorrelationId
Timestamp
```

Core must never infer domain-specific event meaning from a generic diff.

---

# 39. External Protocol

External integration is parallel to Creator Domain Protocol rather than embedded into it.

```text
Digger External Protocol
│
├── Destination
├── ExternalReference
├── PayloadSnapshotRef
├── ExternalOperation
├── Reversibility
├── Idempotency
├── Execute
├── Observe
└── Reconcile
```

External adapters implement real-world integration.

Creator Domain owns business meaning such as Publication/Deployment/Distribution.

External Context owns API/service side effects.

---

# 40. Cross-context Contract Rules

## 40.1 Exchange references/projections, not foreign Aggregates

Do not pass the entire Governance Aggregate into Writing, or the entire Agent Aggregate into Context Provider.

Prefer small contract types such as:

```text
ProjectId
DomainTarget
StateRef
ActorContext
ChangeSetView
ContextRequest
Assessment
ApplyResult
CapabilityDefinition
```

## 40.2 Each semantic state has one authoritative owner

Contexts may reference another Context's state, but they must not duplicate ownership.

## 40.3 Context/Assessment are side-effect free

Reading context, validating, previewing and analyzing impact must not silently mutate Official State.

## 40.4 Mutable Creator Domains must support Apply

If a Domain can participate in Official Project State, it must provide a governed apply path.

---

# 41. Resource / Asset Boundary

Resources such as images, video, audio, PDFs, source files and datasets can be reused across Creator Domains.

Do not therefore create a universal semantic Artifact model.

Distinguish:

```text
physical/logical Resource
vs
Domain meaning of that Resource
```

The same binary image can simultaneously be:

```text
Writing Figure
Social Illustration
Site Hero Image
```

Core may eventually provide a `ResourceRef`, while each Domain owns what the resource means.

This remains a Runtime/Resource Architecture issue rather than a new Creator Domain concept in v0.3.

---

# 42. Domain Pressure Tests

## 42.1 Social

```text
State:
Brand / Channel / Content / Variant / Publication

Context:
Brand voice / audience / platform rules / campaign / performance

Governance:
Brand validation / platform suitability / variant impact / preview / apply

Capability:
ResearchTopic / CreateContent / CreateVariant / SchedulePublication
```

No Social-specific object needs to leak into Core.

## 42.2 Writing

```text
State:
Work / Manuscript / Chapter / Canon

Context:
Thesis / outline / neighboring chapters / style / claims / sources

Governance:
Terminology validation / canon conflict / TOC impact / diff / apply

Capability:
DraftSection / RewritePassage / MoveChapter / CheckConsistency / CompileEdition
```

No Writing-specific object needs to leak into Core.

## 42.3 Site

```text
State:
Site / Schema / Entry / Taxonomy / Template / ContentProgram

Context:
Schema / IA / Content Program / SEO rules / related entries / source data

Governance:
Schema validation / dependency analysis / migration preview / apply

Capability:
GenerateEntry / UpdateSchema / RegenerateCollection / BuildSite
```

No Site-specific object needs to leak into Core.

## 42.4 Future Presentation Domain

```text
State:
Deck / Slide / Element / Theme

Context:
Storyline / audience / visual language / neighboring slides

Governance:
Layout validation / theme consistency / affected slides / preview / apply

Capability:
CreateSlide / RewriteSlide / ReorderSlides / GenerateVisual
```

Core remains unchanged, validating the boundary model.

---

# 43. Digger Project Evolution Loop

The complete logical loop is:

```text
Issue
 ↓
Proposal
 ↓
Decision
 ↓
Intent
 ↓
ProductionRun
 ↓
Action / Result / Working State
 ↓
Change / ChangeSet
 ↓
Validation + Impact
 ↓
Governance
 ↓
Commit
 ↓
Official Domain State
 ↓
ProjectRevision
 ↓
Snapshot
 ↓
ExternalAction
 ↓
External State
 ↓
Observation
 ↓
Knowledge / Assertion
 ↓
Issue / Proposal
        ↺
```

This is a typical causal path, **not a mandatory workflow**.

Human, Agent and Automation can enter at different points subject to permissions, policies and invariants.

Digger Core defines legal transitions and governance semantics, not one universal BPM workflow.

---

# 44. Logical Architecture v0.3

```text
                         PROJECT
                            │
        ┌───────────────────┼────────────────────┐
        ↓                   ↓                    ↓
      Actor              Knowledge            Domain
 Human / Agent      Source / Assertion     Social/Writing/Site
        │             Decision/Authority          │
        └──────────────┬────┴────────────────────┘
                       ↓
                Context Assembly
                       ↓
             Issue / Proposal / Intent
                       ↓
                 Production Run
                       ↓
             Action → Result
                       ↓
                     Change
                       ↓
                  Change Set
                       ↓
            Validation + Impact
                       ↓
                  Governance
                       ↓
                     Commit
                       ↓
              Official Domain State
                       ↓
                Project Revision
                       ↓
                    Snapshot
                       ↓
                External Action
                       ↓
                 External State
                       ↓
                  Observation
                       │
                ┌──────┴──────┐
                ↓             ↓
              Issue        Assertion
                │             │
                └──────┬──────┘
                       ↓
                    Proposal
                       ↓
                  Project(t+1)
```

Project History / Causality traverses the entire process.

---

# 45. Context Architecture v0.3

```text
                         DIGGER
                           │
              ┌────────────┴────────────┐
              │                         │
         PROJECT CORE              CREATOR DOMAIN
              │                         │
    ┌─────────┼─────────┐               │
    │         │         │               │
 Project  Knowledge  Production         │
    │         │         │               │
    └─────────┼─────────┘               │
              │                         │
         Governance ←── Domain Protocol ┤
              │                         │
              │                   ┌─────┼─────┐
              │                   │     │     │
              │                Social Writing Site
              │
           External
              │
       External Protocol
              │
           Adapters
              │
             World
```

Cross-cutting:

```text
Context Assembly
Project Revision
Domain Events
Causal History
Provenance
```

These are mechanisms, not automatically independent Bounded Contexts.

---

# 46. Core Concept Set v0.3

## Project State

```text
Project
Authority
Snapshot
ProjectRevision
History
```

## Actor

```text
Actor
├── Human
└── Agent

Role
Delegation
Permission
Capability
EffectClass
```

## Knowledge & Deliberation

```text
Source
Knowledge
Assertion
Decision
Issue
Proposal
Intent
```

## Production & Change

```text
ProductionRun
├── Action
├── Result
└── WorkingState

Change
ChangeSet
Validation
Impact
Policy
Commit
```

## External Interaction

```text
ExternalAction
ExternalReference
ExternalState
Observation
```

Cross-cutting:

```text
Context Assembly
Provenance
Causation
Correlation
Project History
```

---

# 47. Concepts Explicitly NOT in Core

```text
Content
Document
Artifact
Page
Post
Video
Chapter
Publication
Edition
Deployment
Campaign
Workflow Definition
Analytics
Feedback
Canon
Claim
Schema
```

Also avoid universal inheritance such as:

```text
Everything extends ProjectObject
```

and universal semantic objects such as:

```text
UniversalArtifact
UniversalDocument
UniversalContent
```

They erase meaningful domain boundaries.

---

# 48. Twelve v0.3 Architecture Principles

1. **Project is the boundary; Domain is the semantic owner.**
2. **Each semantic state has exactly one authoritative owner.**
3. **Cross-context relationships use references/projections, not foreign object ownership.**
4. **DomainTarget identifies what; StateRef identifies which version.**
5. **AggregateVersion handles local concurrency; ProjectRevision provides project-wide logical consistency.**
6. **Production owns work; it does not own official Creator state.**
7. **Every official Creator-state mutation is represented as a Change.**
8. **Governance determines whether a Change may become official; Domain determines whether it is semantically valid.**
9. **A Commit is finalized only when the owning Domain has successfully applied the governed transition.**
10. **Domain Events describe facts; Changes describe deltas; History preserves causality.**
11. **Human, Agent, Automation and Importer are all clients of the same Domain rules—none has a mutation backdoor.**
12. **Core owns evolution mechanics; Domain owns creator semantics.**

---

# 49. Six Stable Design Principles from Domain Pressure Tests

1. **Project is the boundary, not the content.**
2. **Core owns transitions; Domain owns meaning.**
3. **Knowledge is not Authority.**
4. **Proposal is not Candidate.**
   - Proposal: Should we do this?
   - Candidate: Should we apply this concrete change?
5. **Agent output is not automatically a Candidate or Commit.**
   - Outcome depends on Permission, Policy, Effect Class and Impact.
6. **Project evolution must preserve causality.**
   - Record not only what changed, but why, based on what, proposed by whom, authorized by whom and what happened afterward.

---

# 50. What v0.3 Deliberately Does Not Decide

This document intentionally does **not** decide:

```text
Rust crate structure
Tauri / iced / wry UI architecture
SQLite / PostgreSQL / event store
Git-based persistence
filesystem layout
vector database
search engine
MCP implementation
plugin loading
Agent framework
LLM gateway
API protocol
transaction implementation
message broker
serialization format
```

Those belong to Runtime Architecture and Physical Technical Architecture.

Likewise, v0.3 does not require Event Sourcing.

```text
Change Governance
≠ Event Sourcing
```

---

# 51. Next Phase

Logical Domain Model v0.3 is sufficiently complete to freeze as a working baseline.

The next phase is:

```text
Digger Logical Domain Model v0.3
                ↓
       Runtime Architecture v0.1
                ↓
      Technical Architecture v0.1
```

The first Runtime Architecture question should be:

> **When a Human or Agent acts, what runtime components does the operation traverse from Intent to Official State, where are the transaction/consistency boundaries, where may work be asynchronous, and where must state remain strongly consistent?**

Initial runtime candidates:

```text
Project Runtime
      │
      ├── Domain Runtime
      ├── Agent / Production Runtime
      ├── Governance Runtime
      └── External Runtime

Cross-cutting:
Context Engine
History / Event Infrastructure
Resource Layer
```

These are only starting hypotheses for Runtime Architecture v0.1, not decisions made by the Logical Domain Model.

---

# Appendix A — Compact Vocabulary

| Term | Meaning |
|---|---|
| Project | Long-lived creator work boundary and persistent context |
| Domain | Semantic owner of creator-specific concepts |
| Actor | Human or Agent capable of initiating Project actions |
| Delegation | Bounded authorization granted to an Agent |
| Source | Origin of knowledge |
| Knowledge | Retrievable/understandable information in Project context |
| Assertion | Judgeable statement |
| Decision | Immutable record of a Project choice |
| Authority | Binding force applied to knowledge/domain state |
| Issue | Something that deserves handling |
| Proposal | Suggested future direction/action |
| Intent | Accepted goal the Project commits to pursue |
| ProductionRun | Logical execution unit advancing an Intent |
| Action | Work performed by an Actor |
| Result | Immediate output of an Action |
| WorkingState | Intermediate production state not yet official |
| Change | Semantic proposed state delta |
| ChangeSet | Governable collection of Changes |
| Validation | Check of semantic/governance correctness |
| Impact | Semantic consequences/dependencies of a Change |
| Policy | Rule deciding governance path |
| Commit | Immutable record of governed Changes entering Official State |
| StateRef | Stable reference to a specific Domain state/version |
| ProjectRevision | Project-wide authoritative-state logical clock |
| Snapshot | Immutable reference to Project/scoped state |
| Command | Request for an Aggregate state transition |
| Domain Event | Immutable domain-significant fact that occurred |
| ExternalAction | Governed interaction causing real-world side effects |
| Observation | Immutable record of what was observed |
| EffectClass | Runtime/governance class of a Capability's effects |
| Causation | Direct reason something occurred |
| Correlation | Broader Project story linking related activity |

---

# Appendix B — Core vs Domain Quick Reference

| Concern | Core owns | Domain owns |
|---|---|---|
| Project boundary | Yes | No |
| Creator semantics | No | Yes |
| Actor / Delegation | Yes | Domain may define role usage |
| Knowledge / Authority mechanics | Yes | Authority-capable subjects |
| Production runtime concepts | Yes | Domain-specific production meaning |
| Change protocol | Yes | Operation/Payload semantics |
| Validation governance | Yes | Domain invariant validation |
| Impact governance | Yes | Semantic dependency discovery |
| Policy | Yes | Domain risk facts only |
| Commit protocol | Yes | Apply semantics |
| Domain state | No | Yes |
| Domain events | Envelope/history | Semantic payload/type |
| Publication/Edition/Deployment | No | Yes |
| External API side effect | External Context | Business intent belongs to Domain |
| History mechanics | Yes | Emits meaningful domain facts |

---

# Appendix C — Minimal Creator Domain Protocol

```text
Creator Domain
│
├── State Contract
│   ├── Resolve Target
│   ├── Read State
│   └── Resolve Version
│
├── Context Contract
│   ├── Build Domain Context
│   └── Expose Authority Subjects
│
├── Governance Contract
│   ├── Validate
│   ├── Analyze Impact
│   ├── Preview
│   └── Apply
│
├── Capability Contract
│   ├── Describe Capability
│   ├── Required Permission
│   └── Effect Class
│
└── Domain Event Payloads
```

This is the primary logical boundary between Digger Core and Creator Domains in v0.3.

---

**End of ByteBoy Digger Logical Domain Model v0.3**
