# ADR-012: Markdown Text as the Authoritative Chapter Content

- Status: Accepted (direction chosen by the project owner, 2026-10-10); details to be validated as listed below
- Date: 2026-10-10
- Decision owners: Digger Architecture
- Amends: ADR-006 (Writing Document Representation) — the AST is no longer authoritative
- Related: ADR-001 (shell and editor host), ADR-003 (Project consistency island), ADR-011 (application host)
- Input: [`byteboy_markdown_editor_initial_requirements.md`](../byteboy_markdown_editor_initial_requirements.md)

## Context

### Product positioning

- **Dozer** is an engineering document editor: short to medium documents, clear structure, code, tables and diagrams, frequent Agent edits, Source-first.
- **Digger** is a writing workbench: long-form, multi-chapter, image and material heavy, continuous authoring, Agent assisted, Visual-first.

### What the requirements document establishes for Digger

1. Markdown is an open file format: the author can always take the `.md` files, and they work with Git. Markdown stays the source of truth as far as possible, to avoid being locked into a closed rich-text schema.
2. Visual-first editing backed by Markdown, with **progressive syntax reveal** (syntax shows when the caret enters a block) and a Markdown source mode as a secondary view.
3. CommonMark and GFM, with footnotes and citations as strong features.
4. A manuscript is Book → Chapter → Section; chapters are separate files; chapter reorder, cross-chapter search, incremental rendering.
5. **Stable block identity** that survives edits elsewhere: after thousands of characters are added earlier in the chapter, the system must still know which paragraph the Agent changed yesterday (`block_id`, range, hash, `semantic_id`, asset refs, Agent history).
6. Images are assets (original, optimized, thumbnail, caption, alt, credit, copyright, prompt, generation history); Markdown holds a stable reference.
7. Agent-native editing from the start: typed selection, shared viewport, structured mutation primitives at paragraph/block/section/chapter/book granularity, immediate change feedback with locate, highlight and undo, book context and memory.
8. Prose diff (words, paragraph and section moves, image replacement, caption changes), with Git hidden from the author.
9. Export to Markdown, DOCX, PDF, EPUB and Web.
10. A shared Markdown Core (parser, tree, serializer, source map, block identity, mutation primitives) used by Dozer and Digger. The document sequences Core → Dozer Markdown V1 → Agent-native validation → Digger Writing Editor.

### Conflict with the accepted design

ADR-006 made a Writing Domain AST the sole authority and Markdown an exchange format, with a project directory whose SQLite database is the only consistency island (ADR-003). That conflicts with items 1 and 2 above: editing `.md` outside Digger is not supported, saving normalizes Markdown (noisy Git diffs), and the stored form is a schema the author does not own.

Three options were considered: keep ADR-006; make the `.md` files on disk authoritative; or make Markdown text authoritative inside the project database and mirror it to files. The owner chose the third.

## Decision

### 1. Markdown text is the authoritative content of a Document

Each chapter is a Document with a stable `DocumentId` (UUIDv7). Its authoritative content is a **Markdown text** (UTF-8, LF) stored in the Project database and changed only through Change → Commit in the ADR-003 transaction. Chapter order and book structure stay in separate aggregates that reference `document_id`, as in ADR-006.

- **Dialect:** CommonMark plus GFM (tables, task lists, strikethrough, autolinks), footnotes, and fenced code (including Mermaid for reuse from Dozer). Anything outside the declared dialect is kept verbatim as text, never dropped or rewritten.
- **Minimal-diff rule:** Digger never rewrites text it was not asked to change. A Change touches only the ranges it targets. Normalization happens only on an explicit "format" action. Editors must satisfy this (see "Editor engine").
- Line endings are normalized to LF in the database; the original style is recorded per Document for the mirror.

### 2. Structure is derived; block identity is authoritative metadata

The Rust side parses the text into a block tree with source ranges (the source map: block ↔ range, rendered position ↔ source position). The tree and ranges are **derived and rebuildable** on every Commit.

Block identity is different. `block_id`, `semantic_id` and Agent history cannot be recovered from the text, so they live in tables **inside the Project database** (part of the island, not a disposable cache):

```text
block(block_id, document_id, kind, start, end, content_hash, heading_path, semantic_id, updated_at_version)
block_asset(block_id, asset_id)
block_agent_history(block_id, run_id, change_id, at_version)
```

By default IDs are **not** written into the Markdown, so the files stay clean. An optional embedding (comment metadata) may be added later behind the same interface.

**Reconciliation at Commit** keeps IDs stable across edits:

1. A Change produced by a mutation primitive carries the exact block mapping.
2. For free typing (a text diff), match old and new blocks by content hash along the longest common subsequence of hash sequences, so unchanged blocks keep their ID wherever they moved or however much text was inserted before them.
3. In the remaining unmatched regions, pair blocks of the same kind in order when similarity exceeds a threshold; anything else is a new or deleted block.
4. A split keeps the ID on the first part; a merge keeps the first block's ID. Each match records a confidence; low-confidence matches are marked so Agent history anchored to them can be shown as approximate.

### 3. Changes are text patches with preconditions

A Change carries `base_version`, `base_hash` and a list of edits `{block_id?, range, replacement}`. The semantic operations from ADR-006 (`InsertBlock`, `DeleteBlock`, `MoveBlock`, `ReplaceBlockContent`, `SetBlockAttributes`, `ReplaceTextRange`, `SetTextMarks`, `AttachResource`) remain the vocabulary for Agents and review. Each compiles deterministically to a text patch (for example `SetTextMarks` becomes inserting `**`). A ChangeSet may carry both the operation list (for semantic and prose diff) and the compiled patch. If `base_hash` no longer matches the current version, the Change is rebased by merge (next section) instead of being applied blindly.

### 4. Merge is text-level and block-aware

The three-way merge from the spike moves from whole blocks to text. Edits in different blocks never conflict. Within one block, disjoint ranges merge automatically; overlapping ranges are a conflict, shown in the Rust-owned iced conflict bar with a prose diff. This removes the weakness found in the spike, where two edits to different sentences of the same long paragraph were a block conflict. The tokenization (characters, words or sentences) is an implementation choice to be validated.

### 5. The `.md` files are a mirror

The Project is a normal folder with a user-visible tree and an internal directory:

```text
MyBook/
├── book.yaml            # mirror: title, chapter order, metadata
├── chapters/
│   ├── 01-introduction.md   # mirror of each Document
│   └── ...
├── assets/              # mirror of image assets, stable file names
└── .digger/             # authoritative state; ignored by Git by default
    ├── project.db
    ├── manifest.json
    ├── resources/objects/…
    ├── working/  backups/  logs/
```

This replaces the `My Project.digger/` listing in Technical Architecture §6; the contents of that listing move under `.digger/`.

- **Direction database → files:** an Outbox consumer writes each Document to its file atomically (temporary file, then rename) after the Commit and records the file's content hash. A failed or delayed mirror never blocks a Commit; the UI shows the sync state.
- **Direction files → database:** a watcher detects a file whose hash differs from the recorded mirror hash. Its base is found by that hash in the Document's history, and the edit enters as an **import Change** (actor: External) through the same three-way merge against the current version. Overlaps use the conflict bar. If the base cannot be found (the file was replaced wholesale), the import becomes a WorkingState with a full preview and is never applied silently.
- Git, if the author uses it, operates on the mirror. Restoring a project from Git alone is an import, not a restore of authority; this is a deliberate limit.
- The database is the single authority. The mirror is a projection with an explicit, conflict-aware import path.

### 6. Assets

Assets stay content-addressed in the resource store (Technical Architecture §14) with the metadata in the requirements document. The Markdown text holds a stable reference to a file under `assets/` whose name carries the immutable asset id, so the text stays valid in other tools. The exact reference and caption syntax is open (see Open questions).

### 7. A shared `markdown-core`

The pieces that do not depend on Digger belong in a separate repository consumed by git tag, like `bytegit` and `byteui`: the parser and block tree, the source map, text patches, text diff and three-way merge, block reconciliation, and the mutation primitives. Dozer (Source-first) and Digger (Visual-first) use the same Core. Digger keeps its own parts: manuscript model, chapters, assets, book context, prose diff, export. The sequencing in the requirements document (Core and Dozer first) is a planning input, not something this ADR fixes.

### What stays and what changes from ADR-006

| ADR-006 element | Status |
|---|---|
| Canonical AST JSON, deterministic encoding, AST hash, AST schema evolution | **Removed.** The text is authoritative; the tree is derived |
| ProseMirror JSON as an editor projection | **Conditional** on the editor engine decision |
| Semantic change operations, Agent interaction rules (projections plus block ids) | **Kept**, compiled to text patches |
| Import/export with conversion reports | **Kept**; Markdown import/export is now the identity for the dialect |
| Per-extension "declare your mapping" rule | **Kept** as "every editor feature must map to Markdown syntax in the declared dialect, or be transient" |
| Crash recovery journal that never overwrites Official State | **Kept** |
| Chapter and manuscript structure separate from document content | **Kept** |

## Editor engine implication

ADR-001 chose Tiptap/ProseMirror under the premise that an AST is authoritative. That premise no longer holds, so the **editor engine is re-opened** (ADR-001 is updated). The requirements are now:

- untouched text is preserved byte for byte, and a one-paragraph edit changes only that range (minimal-diff rule);
- progressive syntax reveal, with a source mode as a secondary view;
- IME correctness for Chinese and Japanese, long chapters, inline image widgets, block drag and reorder;
- compatible with Dozer's source-first Markdown editing (Dozer already hosts CodeMirror).

Two candidates follow: a **CodeMirror 6 live-preview editor** (the document is the Markdown text, syntax is hidden by decorations outside the caret; its change sets are range edits, which match text patches, and its collaboration module rebases updates over concurrent ones) and a **ProseMirror editor with source-preserving serialization** (untouched blocks keep their original text, only changed blocks are reserialized). Neither is chosen here; a spike decides.

A second parser exists in the editor (for example Lezer for CodeMirror) beside the Rust parser. **Rust is authoritative for block boundaries.** The two must agree on the dialect's corpus, which is a validation item.

## Consequences

Positive:

- Matches the requirements: open Markdown, minimal diffs, Git-compatible files, no closed schema, external tools and Agents can edit the mirror.
- Keeps the ADR-003 single-transaction island and its guarantees.
- Sub-block merge fixes the long-paragraph conflict weakness.
- Block ids survive edits elsewhere by construction (hash-based reconciliation).
- One Core can serve Dozer and Digger.

Negative / costs:

- Two representations of the same content (database text and mirror file) and a sync protocol with conflict handling.
- Block identity depends on a reconciliation heuristic; it can be wrong, and external edits weaken it.
- Block structure comes from parsing, so parser disagreement and dialect edge cases matter.
- ADR-006's structured operations on a typed tree are now operations on parsed ranges.

## What this means for the editor-host spike

Editor-independent and kept: the bridge envelope and allow-list, the pinned webview settings and probes, the Rust-owned recovery and conflict bars with host push-down, the persistent history (its records change from block deltas to text patches, checkpoints become full texts), and the crash recovery journal.

To be reworked: the block model and block delta (`doc.rs`), the block-level merge (`merge.rs`), and the host's `blockId` plugin, `applyDelta` and delta computation, which are ProseMirror-specific. The Milkdown/Tiptap comparison in ADR-001 was made under the AST premise and must be repeated against the minimal-diff rule.

## Validation required

1. **Minimal diff:** for each candidate engine, open a realistic Markdown chapter, edit one paragraph, and verify only that range changed (hash of the rest unchanged), including lists, nested quotes, reference links, emphasis styles and long lines.
2. **Block id survival:** property tests over simulated edit streams (typing, paste, split, merge, move, thousands of characters inserted earlier) measuring how many ids survive and how many matches are wrong.
3. **Text merge:** automatic merge of disjoint edits inside one block, conflict on overlap, fuzzed against a reference.
4. **Mirror:** atomic file write, crash between Commit and mirror write, external edit imported through the merge, wholesale replacement handled as WorkingState, no silent loss in either direction.
5. **Parser agreement:** the Rust and editor parsers agree on block boundaries over the CommonMark and GFM example corpora plus a real manuscript.
6. **CodeMirror live-preview spike:** progressive reveal, Chinese and Japanese IME, image widgets, long-chapter performance, block drag; compare with ProseMirror under the same checks.
7. **Dozer reuse:** confirm how much of Dozer's existing CodeMirror host and Markdown handling can be shared.
8. **Prose diff:** design the word-level diff plus paragraph and section moves on top of text patches.

## Open questions

- Reference form for images and captions in the Markdown text (relative path with an id-prefixed file name is the default candidate; figure/caption syntax is undecided).
- Front matter versus `book.yaml` for chapter metadata, and whether a chapter file may hold several sections that the outline treats separately.
- Whether to offer optional embedded block ids for authors who want them to survive wholesale file rewrites.
- Handling of CRLF files and files with a BOM on import.
- The size of the mirror sync state shown to the author, and whether Git integration through `bytegit` is offered at all.
- Where `markdown-core` lives and how it is versioned relative to Dozer's current Markdown preview.
