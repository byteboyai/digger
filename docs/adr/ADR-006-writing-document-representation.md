# ADR-006: Writing Document Representation

- Status: Accepted
- Date: 2026-10-01
- Decision owners: Digger Architecture
- Applies to: Writing Domain, desktop editor, import/export, Agent changes

## Context

Digger needs one authoritative representation for long-form manuscript content.

The representation must support:

- long-lived Projects independent of a particular editor library;
- semantic Changes and reviewable Agent output;
- stable block identity and provenance;
- deterministic persistence and hashing;
- schema evolution;
- crash recovery without silently changing Official State;
- Markdown, HTML, DOCX and EPUB import/export;
- future editors and non-visual clients such as Agents and MCP.

The principal candidates were:

1. Markdown as authoritative state;
2. ProseMirror JSON as authoritative state;
3. an editor-independent Writing Domain AST;
4. storing both ProseMirror JSON and another canonical representation as co-authoritative.

Markdown cannot represent all structural semantics without dialect-specific conventions. ProseMirror JSON contains editor/plugin concerns and would make the Writing Domain depend on Tiptap/ProseMirror. Two co-authoritative representations create synchronization conflicts.

## Decision

The **Writing Domain AST is the sole authoritative manuscript representation**.

```text
Writing Domain AST       = Official manuscript state
ProseMirror JSON         = active editor session projection
Markdown / HTML          = exchange and preview formats
DOCX / EPUB / PDF        = import, compile or publication formats
```

Tiptap/ProseMirror is an editor adapter. It may manage selections, transactions, plugin state and an editing buffer, but it does not define Official Writing Domain State.

Only the Rust Writing Domain converts a governed Change into a new authoritative AST version.

## Canonical AST envelope

The initial logical form is:

```text
WritingDocument
├── documentId: UUIDv7
├── schemaVersion: integer
├── language?: BCP-47 tag
├── blocks: Block[]
└── extensions?: namespaced values

Block
├── blockId: UUIDv7
├── type
├── attrs
├── content
└── extensions?
```

Every block has a stable `blockId`. IDs survive text edits and moves. Copying content creates new IDs unless the operation explicitly preserves identity within the same document.

The initial block vocabulary is deliberately small:

```text
paragraph
heading
blockquote
codeBlock
orderedList
bulletList
listItem
thematicBreak
sceneBreak
resource
```

Initial inline vocabulary:

```text
text
hardBreak
link
citationRef
footnoteRef
```

Initial text marks:

```text
strong
emphasis
underline
strike
code
```

`resource` points to a `ResourceRef`; binary data and machine-specific filesystem paths never appear in the AST.

Creator-specific concepts such as Chapter, Scene, Canon or Claim are not encoded as generic rich-text marks. They remain Writing Domain objects or explicit references.

## Canonical encoding

The persisted encoding is versioned JSON with these rules:

- UTF-8 strings;
- explicit `schemaVersion`;
- no editor selection, history, decoration or plugin state;
- no `null` fields when absence has the defined meaning;
- object keys serialized deterministically for fingerprints;
- array order is semantically significant;
- UUIDs use lowercase canonical text;
- unknown top-level or node fields are rejected unless stored below a registered namespaced `extensions` key;
- limits apply to nesting depth, block count, text size and extension size.

Canonical fingerprints are computed from normalized serialization, not raw user JSON bytes.

## Storage model

SQLite stores:

```text
writing_document
├── document_id
├── project_id
├── document_type
├── current_version
├── current_ast_json
├── schema_version
├── content_digest
└── updated_at

writing_document_version
├── document_id
├── version
├── project_revision
├── ast_json or snapshot/delta reference
├── content_digest
├── commit_id
└── created_at
```

The first implementation may store full version snapshots. Delta compression is an optimization and must not alter Domain semantics or recovery guarantees.

Chapter and manuscript structure remain separate tables/aggregates that reference `document_id`. One giant AST for an entire book is not required.

## Editor projection

Opening a document follows:

```text
Domain AST at StateRef
  → Rust AST-to-editor projection
  → ProseMirror JSON + projection metadata
  → Tiptap editing session
```

The projection metadata records:

- source `StateRef` and AggregateVersion;
- AST schema version;
- editor schema version;
- block-ID mapping;
- enabled editor extensions;
- conversion warnings.

Saving follows:

```text
ProseMirror transaction state
  → editor adapter validation
  → normalized Domain AST candidate or semantic operations
  → Draft Change with BaseStateRef
  → preview/governance
  → Writing Domain Apply
  → new AST version + Commit
```

ProseMirror-only state is discarded or kept in the editor recovery journal; it cannot leak into Official AST accidentally.

## Supported editor subset

Every enabled Tiptap extension must declare one of:

- lossless Domain AST mapping;
- transient editor-only behavior;
- export-only behavior;
- unsupported.

Extensions with no declared mapping cannot be enabled for authoritative editing.

Examples:

| Editor feature | Classification |
|---|---|
| Cursor/selection | transient |
| Undo stack | transient |
| Comments decoration | transient projection of separate Domain records |
| Bold/italic | lossless AST mapping |
| Heading/list | lossless AST mapping |
| Image upload | maps to `resource` after ResourceRef creation |
| Arbitrary embedded HTML | unsupported in Official AST v1 |

## Change operations

The Writing Domain exposes semantic operations rather than accepting arbitrary replacement JSON as its primary API:

```text
InsertBlock
DeleteBlock
MoveBlock
ReplaceBlockContent
SetBlockAttributes
ReplaceTextRange
SetTextMarks
AttachResource
```

Each operation includes target block identity and appropriate preconditions. The Domain validates document structure after applying the entire ChangeSet.

A full-document replacement is allowed for import, migration or explicit recovery, but it is a high-impact operation with a richer preview.

## Agent interaction

Agents normally receive controlled text/structure projections plus block IDs, not raw ProseMirror JSON.

Agent output may be:

- semantic operations against a `BaseStateRef`;
- a replacement candidate for a bounded block range;
- a new WorkingState document for large rewrites.

Agent output never becomes Official State directly. The Writing Domain validates block identity, structure, stale versions and unsupported constructs before Apply.

Review UI presents semantic block/text differences derived from Domain AST, not a raw JSON diff.

## Import and export

Every converter returns both output and a report:

```text
ConversionResult
├── output
├── warnings[]
├── losses[]
├── unsupported[]
├── sourceFingerprint
└── converterVersion
```

Rules:

- importing creates WorkingState before becoming an Official Change;
- lossy import requires a visible report;
- export is generated from a stable StateRef or Snapshot;
- round-trip fidelity is tested for the supported subset;
- external format metadata is not promoted to Domain semantics automatically;
- raw source files may be retained as Resources for provenance.

Markdown is the first supported exchange format. HTML follows for preview/export. DOCX and EPUB are separate converters and do not shape the core AST.

## Schema evolution

AST schema versions are independent of database schema versions.

```text
AST migration
old canonical AST
  → pure deterministic transform
  → new canonical AST
  → validation
  → migration record
```

Migration requirements:

- transformations are deterministic and side-effect free;
- fixtures cover every supported source version;
- content digests are recalculated;
- unsupported future versions are opened read-only or rejected clearly;
- original snapshots remain recoverable through Project backup/history policy.

## Crash recovery

The editor periodically writes a recovery journal outside Official Domain State:

```text
EditorRecovery
├── sessionId
├── documentId
├── baseStateRef
├── editorSchemaVersion
├── ProseMirror JSON or transactions
├── lastUpdatedAt
└── checksum
```

After a crash, recovery converts the buffer into a new candidate against the recorded base. If Official State advanced, the user receives a conflict/merge flow. Recovery never silently overwrites the current document.

## Consequences

Positive:

- Writing semantics remain independent of Tiptap and any particular UI shell;
- Agents and non-visual clients use a stable protocol;
- semantic diffs, validation and migrations are possible;
- editor replacement is feasible;
- exports cannot accidentally become the source of truth;
- block identity improves review, provenance and concurrency handling.

Costs:

- two conversion adapters are required for every edit session;
- supported editor features must be curated;
- clipboard/import behavior needs explicit normalization;
- Domain AST migrations must be maintained;
- some ProseMirror plugins cannot persist their native state directly.

## Rejected alternatives

### Markdown as authority

Rejected because dialects, embedded HTML and formatting conventions make structural meaning ambiguous. Markdown remains a first-class exchange format.

### ProseMirror JSON as authority

Rejected because editor plugin schemas would become Domain schemas and complicate Agents, migrations and alternative clients.

### HTML as authority

Rejected because presentation, unsafe markup and inconsistent normalization obscure document semantics.

### Dual authoritative representations

Rejected because conflict resolution would require deciding which representation wins—the definition of a missing authority boundary.

## Validation requirements

ADR implementation is complete when tests prove:

1. canonical serialization is deterministic;
2. stable block IDs survive supported edits and moves;
3. AST → ProseMirror → AST is lossless for the supported subset;
4. Markdown round trips preserve the declared supported subset;
5. unsupported editor nodes cannot enter Official State;
6. stale `BaseStateRef` changes conflict;
7. Agent changes generate semantic previews;
8. failed conversion or Apply leaves Official State unchanged;
9. recovery buffers create candidates rather than overwriting state;
10. AST migration fixtures preserve expected content and structure.

