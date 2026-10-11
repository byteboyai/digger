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
4. A split or merge is recognized only when at least two of the parts are real fragments of the other side (most of their own words or characters come from it) and together they still add up to it. Which part keeps the id is a policy: `LargestPart` (the part with the most text; the default after the spike) or `FirstPart`. Each match records a confidence; low-confidence matches are marked so Agent history anchored to them can be shown as approximate.

The refined algorithm and its measured behaviour are in "Block identity spike results" below.

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

Two candidates follow: a **CodeMirror 6 live-preview editor** (the document is the Markdown text, syntax is hidden by decorations outside the caret; its change sets are range edits, which match text patches, and its collaboration module rebases updates over concurrent ones) and a **ProseMirror editor with source-preserving serialization** (untouched blocks keep their original text, only changed blocks are reserialized). The engine spike below compares them; it favours CodeMirror 6, pending the manual IME check.

A second parser exists in the editor (for example Lezer for CodeMirror) beside the Rust parser. **Rust is authoritative for block boundaries.** The two must agree on the dialect's corpus, which is a validation item.

## Engine spike results (2026-10-10, `spike/md-engine`)

Both engines run the same checks in a real WKWebView against one realistic chapter (Chinese prose, setext headings, `_`/`__` emphasis, `1)` lists, hard breaks, reference links, footnotes, a table, an HTML comment, an image). The ProseMirror side is a source-preserving variant I built for the test: each top-level block remembers its original Markdown and gap; an unchanged block is emitted verbatim, a changed block goes through `prosemirror-markdown`'s normalizing serializer; tables and HTML are raw blocks. Versions: CodeMirror view 6.43, ProseMirror model 1.25.

| Check | CodeMirror 6 live preview | ProseMirror, source-preserving |
|---|---|---|
| T1 open and save without edits is byte-exact | pass | pass |
| T2 a one-character edit changes only that character | **18 of 18 blocks** | **9 of 18**: edits in a setext heading, `_`/`__` emphasis, a hard-break paragraph, `1)` lists, nested lists, quotes, reference-link and footnote paragraphs rewrote 38–107 extra characters of the block |
| T3 heading marker hidden away from the caret, shown with it | pass | pass |
| T4 inline marks hidden away, shown with the caret | pass (whole block) | pass (only the marked span) |
| T5a image rendered as an image | pass | pass |
| T5b image source revealed and editable as Markdown | pass | **fail** (would need a custom node view) |
| T7 move a block: blocks verbatim, nothing lost or invented | pass | pass |
| T9 caret → exact source offset (8 probes) | 8/8, native | 8/8, via a greedy text alignment heuristic that is not general |
| 130k chars (~3,200 blocks): load, keystroke mean, DOM nodes | 33 ms, 1.4 ms, 137 | 100 ms, 2.0 ms, 7,800 |
| 658k chars (~16,000 blocks) | 33 ms, 1.4 ms, 140 | 383 ms, 12.3 ms, 39,000 |
| Tables, footnotes, HTML comments | plain text, always preserved | tables/HTML raw blocks, footnote text re-escaped on edit, reference definitions live in gaps and are invisible in the editor |

Reading the results:

- The deciding difference is T2. Under the minimal-diff rule (a requirement of this ADR) block-level preservation is not enough: as soon as the author edits a block written in a non-default style, the whole block is normalized. About half of the block kinds in a realistic chapter did. Avoiding that in ProseMirror means mapping each editing step back to a text patch on the original source, which makes the source text the real model again.
- Chapter-sized documents (a few thousand to tens of thousands of characters) perform acceptably in both. CodeMirror virtualizes (about 140 DOM nodes regardless of size); ProseMirror does not and degrades past roughly 300k characters (12 ms per keystroke at 658k).
- Progressive syntax reveal works in both. ProseMirror reveals only the marked span, CodeMirror the whole block. The CodeMirror version needs no extra mapping because the document already is the source.
- The ProseMirror variant is my minimal implementation and the CodeMirror one is a first live-preview plugin; both are test-sized, not product-quality. A finer ProseMirror variant was not built.

**Not verified here (manual or later):**

- **IME composition with live-preview decorations** is the main risk for CodeMirror: hidden syntax and widgets changing while a composition is active. The plugin keeps decorations mapped during composition and rebuilds after it, but this was not exercised with a real IME. Run `ENGINE=cm cargo run` in `spike/md-engine` and type Chinese and Japanese, including at the start of a line, next to `**` and inside a heading.
- Caret movement across hidden ranges, selection toolbars, drag handles for blocks, visual table editing, accessibility, undo semantics around Agent changes.
- That the Rust parser and Lezer agree on block boundaries (validation 5), and that block identity reconciliation works (validation 2); neither was part of this spike.
- Reuse with Dozer's existing CodeMirror host (validation 7).

**Recommendation, pending the IME check:** use CodeMirror 6 live preview as the Digger chapter editor, and share the Markdown language and live-preview extension with Dozer's CodeMirror host. Keep ProseMirror only as a fallback if the IME check fails or if rich structured blocks (visual tables) turn out to need it.

## Block identity spike results (2026-10-10, `spike/block-identity`)

A reconciliation algorithm was implemented in Rust on top of pulldown-cmark block ranges, and tested against a simulator that knows the true identity of every block. The simulator renders structured content (headings including setext, paragraphs including wrapped lines, bullet and ordered lists with varying markers, quotes, code, tables, rules, images) to Markdown, applies authoring operations, re-parses the text and reconciles it exactly as a Commit would, then compares with the truth.

**Algorithm, as it ended up** (each step was driven by a measured failure):

1. Exact content hash along the longest common subsequence, then exact hash out of order (moved blocks).
2. Identical blocks are assigned by their immediate neighbours, not by order alone.
3. Between those anchors: split/merge detection (real fragments only), then an order-preserving similarity alignment.
4. A global pass for blocks that were both edited and moved.
5. Everything left gets a new id; ids are never reused.

Similarity is the larger of a token Dice score and the share of unchanged sentences/lines, with an edit-distance fallback in the 0.25–0.5 band. Tokens are character bigrams for CJK text and whole words for alphabetic text (Latin bigrams make unrelated English paragraphs look alike). Very short blocks use a character edit ratio, and below four significant characters only an identical text matches. Thresholds from a sweep over low/high/kind-change: same-kind pairing 0.5, global pass 0.6, kind change 0.8; the sweep was flat, so the choice is not sensitive.

**Properties that hold in every run (0 violations):** ids are unique, reconciliation is deterministic, and committing unchanged text changes nothing.

**Measured on 24 documents of 60 blocks × 40 commits per scenario** (plus look-alike documents where 60% of the paragraphs follow one template). "Kept" is the share of blocks that existed before, were not rewritten wholesale, and still carry their id.

| Scenario (edits between two commits) | Kept | Lost although ≥50% of the text survived | Id attached to the wrong block |
|---|---|---|---|
| Typing (1 / 5) | 100% / 100% | 0 / 0 | 0 / 0 |
| Mixed editing (1 / 5 / 20) | 100% / 99.99% / 99.97% | 0 / 2 / 2 | 0 / 0 / 2 |
| Agent rewrite, section insert, move (1 / 5 / 20) | 99.998% / 99.998% / 99.995% | 0 / 0 / 1 | 0 / 0 / 0 |
| Restructure: move, copy, delete (1 / 5 / 20) | 99.998% / 99.99% / 99.97% | 0 / 0 / 0 | 0 / 0 / 0 |
| Look-alike documents, same operations | 99.97–100% | ≤ 2 per scenario | 0 for typing; see below |

- The figures marked as lost or wrongly attached **exclude** two kinds of case that content cannot decide: batches that copy a block and then edit one of the copies (which copy is the original?) and ids that belonged to a block rewritten wholesale in the same commit (the rewrite removed the evidence). They are counted separately and make up almost all remaining failures: for example 0.011% of blocks in the heaviest restructure batches (20 operations between commits), 0.4% of newly created blocks inheriting a look-alike's id.
- In look-alike documents with 20 agent operations between commits, up to about 0.1% of blocks can be attached to a template twin; for ordinary documents the bound used by the test is 0.005%.
- Inserting a 30-block section (thousands of characters) anywhere: every original block kept its id in 60 of 60 trials. Reformatting the whole document into other Markdown styles (setext↔ATX, wrapping, list markers): at least 99.9% kept. Typing `## ` or removing it: at least 99.5% kept, none attached to another block. A block rewritten wholesale never takes another block's id.
- Split and merge: the id survives on exactly one resulting block in at least 98% of 300 trials; `FirstPart` puts it on the first part and `LargestPart` on the larger part in at least 96–98%. The remaining gap is near-ties in size.
- Cost: linear, about 10 µs per block. Reconciling a chapter with ten small edits takes 6 ms for 500 blocks, 16 ms for 1,500, 65 ms for 6,000 and 210 ms for 20,000. Almost all of it is computing similarity signatures for blocks that did not change; computing them lazily for unmatched blocks only would remove most of it (not done).

**Failures the tests found and the fixes:**

- Split detection treated an almost unchanged paragraph plus an unrelated new block next to it as a split and gave the id to the unrelated, longer block. Fix: a split needs at least two real fragments.
- Short blocks (a short quote or heading) lost their id on any edit. Fix: character edit ratio for short text.
- Paragraphs where half the sentences were replaced lost their id. Fix: sentence/line overlap as a second similarity signal.
- Look-alike English text matched wrongly with character bigrams. Fix: word tokens for alphabetic text.
- Identical blocks went to whichever copy came first. Fix: neighbour context.
- Adding a relative-position tie-break to the global pass changed nothing and was removed.

**Known limits:**

- Which copy of an identical block is the original is undecidable from content alone. If an author duplicates a block and edits the original in the same commit, the exact copy may take the id.
- Blocks with fewer than four significant characters (a heading "1.", a rule) keep their id only when their text is unchanged.
- Evidence destroyed by a wholesale rewrite cannot be recovered; the new text becomes a new block unless it resembles an old one.
- The property thresholds were set just inside the measured values, so they catch regressions but not every possible bug: breaking the split rule was caught by a dedicated unit test and **not** by the stream thresholds. The simulator also uses synthetic text and a fixed set of operations; real manuscripts and real editor behaviour (autosave timing, paste of whole sections, IME) are not covered.
- The block structure here comes from pulldown-cmark. Agreement with the editor's own parser (validation item 5) is untested.

## Parser agreement results (2026-10-10, `spike/parser-agreement`)

The question: the Rust parser (pulldown-cmark, authoritative for block boundaries and ids) and the editor's parser (Lezer, as used by CodeMirror 6) must split a chapter into the same top-level blocks. Both were run over the same texts and compared per document on (kind, first line, last line) of every top-level block, with commonmark.js (the CommonMark reference implementation) as the judge of disagreements. Link reference definitions are not blocks for pulldown-cmark, so Lezer's `LinkReference` nodes were set aside. Lezer was run with a footnote extension written for this spike (below).

| Corpus | Documents | Identical block structure |
|---|---|---|
| CommonMark 0.31.2 examples | 652 | 649 (99.5%) |
| GFM extension examples (tables, task lists, strikethrough, autolinks) | 24 | 24 (100%) |
| Markdown files of this workspace (Chinese and English, 144,000 blocks) | 1,519 | 1,515 (99.7%) |
| Random chapters from the block-identity generator | 300 | 300 (100%) |
| 80 hand-written manuscript cases (footnotes, front matter, math, CJK indents, table edge cases, CRLF, BOM, emoji, 100k-character line…) | 80 | 78 (97.5%); 67 of the first 74 (90.5%) without the footnote extension |
| 2,100 intermediate states while typing | 2,100 | 2,065 (98.3%) |

- **Lezer's incremental tree equals its full parse** in all 2,100 edits (random insertions and deletions that create half-typed fences, tables, lists, quotes and HTML). The tree the editor holds is the tree a fresh parse would give.
- **Every static disagreement was judged by commonmark.js, and pulldown-cmark was right in all of them.** Lezer deviates from CommonMark in: a tab after `#` (not a heading), `<textarea>` (not a raw-text HTML block), an empty list item followed by an indented paragraph (the list should end), a list whose continuation is indented by four spaces, and an HTML block starting with an `<img …>` tag line. These are 7 of 2,570 static documents, 4 of them in this workspace's real files.
- **Conformance to the CommonMark reference on the 652 examples:** pulldown-cmark 650 (99.7%), Lezer 647 (99.2%), markdown-it 643 (98.6%), comrak 640 (98.2%). This supports pulldown-cmark as the Rust parser; comrak (a port of GitHub's cmark-gfm) was tried as an alternative and is less conformant on these examples.
- **Footnotes need an editor-side extension.** Lezer has no footnote support: `[^1]: …` becomes a link reference, and a multi-paragraph footnote becomes a link reference plus a code block. A composite-block extension (about 40 lines) makes the footnote cases agree, including multi-paragraph, in-list, and definitions with a list or a fenced block inside. **It is now part of the editor's CodeMirror configuration** (`spike/md-engine/host/src/footnote.ts`, passed as a `markdown({ extensions })` entry; footnote blocks also get their own look in the live preview). The conformance run imports that same file, so it tests exactly what the editor uses. Building it into the editor exposed one more case: two definitions on adjacent lines were swallowed as one block (the second line was taken as a lazy continuation of the first), so the extension also declares that a line starting `[^id]:` ends a paragraph (`endLeaf`). The webview test T10 now parses 12 texts with the editor's parser and compares them with what the Rust parser produced (fixtures generated from pulldown-cmark by `scripts/make-fixtures.mjs`): 12 of 12 identical.
- **Half-typed tables and one quote/list case disagree during typing (35 of 2,100 states, five root causes).** A table row that is only `|`, a row with a line break inside, or a malformed delimiter row such as `|---- --|`: pulldown-cmark and comrak end the table or accept the table differently from Lezer and markdown-it (the two Rust parsers agree with each other on 31 of these 35 states; Lezer agrees with markdown-it). There is no authoritative judge for these: the GFM text does not settle them and implementations split. A quote followed by a line starting `2.`: commonmark.js agrees with the Rust parser. All of these are transient; they resolve when the line is finished.
- **Front matter:** `---` YAML front matter is parsed by both parsers as a thematic break plus a setext heading. Neither treats it as metadata unless the corresponding option is enabled on each side. If front matter is adopted, both must be configured identically.

**What this means for the design**

- The existing rule stands and is now backed by data: **Rust is authoritative for block boundaries and ids.** Disagreement with the editor's tree is about 0.3% of real documents and about 1.7% of half-typed states, so the editor must not use its own tree for anything that changes data. Block ranges for typed selection, Agent context and id lookup come from Rust, sent to the editor after each Commit; the Lezer tree is used only for live-preview decoration.
- A disagreement only moves where syntax is revealed while the caret is in that region; it cannot change block identity.
- Add the footnote extension to the CodeMirror configuration, and keep `spike/parser-agreement/run-all.sh` as a conformance test: it fails on any new disagreement outside a short list of known, explained deviations, and on more than 2% disagreement in typing states.
- Autosave can commit a half-typed table. The Rust blocks at that moment (for example a table followed by a one-line paragraph) differ from the finished table, so a row being typed can briefly be its own block with its own id and then be absorbed. The id reconciliation tolerates this, but Agent history should not be anchored to a block whose kind changed in the same Commit.

**Limits of this check:** only top-level block boundaries were compared, not inline structure (emphasis, links) or nested blocks. The workspace documents are mostly engineering text, not prose manuscripts. The typing states come from a random edit generator, not from real sessions. Only macOS-independent logic was tested (both parsers run outside the webview; Lezer is the same package version as in the editor bundle). Whether pulldown-cmark or Lezer is closer to what GitHub renders for half-typed tables was not settled.

## Block range sync from Rust to the editor (2026-10-10, `spike/md-engine`)

The decision above that Rust is authoritative for block boundaries and ids was implemented between the Rust side and the CodeMirror editor and tested end to end in the webview (tests T11 to T18).

**Protocol** (messages between the page and Rust; positions are UTF-16 code units, as in CodeMirror):

```text
editor → Rust   load  { text }                                  a new document: fresh ids
editor → Rust   edit  { base, changes: [{ from, to, insert }] }  text patches in the coordinates of version `base`
Rust → editor   blocks { version, hash, len16, blocks: [{ id, kind, level, title, from, to, head, how }] }
Rust → editor   reject { reason }                                stale base, position inside a surrogate pair, unparsable message
```

- Rust holds the text. It applies a patch, re-parses, reconciles ids against the previous version (the algorithm from the block-identity spike) and answers with the new version and block list. `hash` (FNV-1a over the UTF-8 text) lets the editor check both sides hold the same text.
- One patch is in flight at a time; edits made meanwhile are composed (CodeMirror `ChangeSet.compose`) and sent after the answer. While patches are unconfirmed, block ranges are mapped through the editor's own changes (block start stays put when typing at its first position, block end moves), so the ranges are valid at every moment. On each answer Rust's list is mapped through whatever is still queued.
- The live preview uses Rust's blocks to decide which block holds the caret. The editor's own tree is no longer consulted for that, and is the fallback only before the first answer.
- Typed selection for the Agent comes from the same blocks: block id, kind, range, the selection, and the titles of the enclosing headings (for example "书名 / 第一章 / 第二节").

**What the tests check (all pass on three consecutive runs):**

- Ranges are correct UTF-16 positions in a document with CJK text, emoji and an astral character (T11a); ids are unique and the text hashes on both sides are equal (T11b).
- Typing one character keeps every id, advances the version by one and keeps the texts equal (T12). Inserting 30 paragraphs at the top keeps all 6 original ids and adds 30 (T13).
- Six edits fired back to back: block ranges stay sorted, in bounds and anchored at their own text after every single edit while the answers are still pending, and at the end equal what a fresh Rust parse gives (T14). Without the mapping of ranges through unconfirmed changes this test fails.
- Where the editor's parser disagrees with Rust (an empty list item followed by an indented paragraph; a `<textarea>` HTML block), the block that holds the caret is the one Rust reports (T15).
- Typed selection carries id, kind, range and the heading path (T16).
- A patch against an out-of-date version, and a position between the two halves of 😀, are refused; reloading resynchronizes (T17).
- Cost (release build): one-character edit until the new block list is back, median 17 ms at 300 blocks and 22 ms at 3,000 blocks (130k characters); loading a 130k-character chapter, 45 ms. This is about one frame and is dominated by the full re-parse on the Rust side, not by the transport.

**Found by the tests:**

- A patch containing a lone surrogate (for example an edit between the two halves of an emoji) is not valid JSON text for the Rust side; it was dropped silently and the editor waited forever. Rust now answers every unparsable message with a `reject`, and positions inside a surrogate pair are refused explicitly (`BadPosition`).
- The test first compared 3 UTF-16 units with 3 code points and failed on emoji; block heads are therefore defined in code points while positions are in UTF-16 units. Any code that mixes the two will be wrong on exactly the text this product is about (CJK with emoji).

**Not done:** re-parsing only the changed region instead of the whole document (22 ms is fine at this size, the cost is linear and would matter for much longer documents).

**Manual IME check (2026-10-10, macOS, run by the project owner on `spike/md-engine`, `ENGINE=cm`):** Chinese and Japanese composition in ordinary paragraphs, at the start of a line, next to `**bold**` and inside a heading, with the Rust sync running, and **with the Agent rewriting another block every 2 seconds and then the block holding the caret every 2 seconds while composing**, was reported as passing: no problems observed. Specific observations (for example whether the composition candidate window stayed in place) were not recorded. Windows and Linux IMEs are untested.

### Agent patches: Rust changes the text and the editor integrates it (tests T19 to T23)

An Agent stand-in in Rust performs operations addressed by **block id** (`replace_block`, `insert_after`, `append`). Rust resolves the id to a range at the moment it runs, applies the patch to its current version, and pushes it:

```text
Rust → editor   update { version, changes: [{ from, to, insert }], hash, len16, blocks }   changes are relative to the previous version
```

The editor may have edits of its own that Rust has not confirmed (one out, more queued). They happened concurrently with the Agent's patch, so the editor transforms both ways with CodeMirror's `ChangeSet.map` (OT on text ranges): its pending edits are rebased over the Agent's patch, and the Agent's patch is rebased over the pending edits before it is applied to the current document. Both sides end in the same text. Rust refuses the author's old edit as stale (it arrives on an older version); the editor then sends the transformed edit again on the new version. Rust needs no log of past versions: it sends every Agent patch to the editor immediately, in order, before it refuses anything, so the editor always integrates the patch before the refusal arrives.

Tie-break at the same position: the author's typing goes before the Agent's insertion. The Agent's change goes into the author's undo history as its own step.

What the tests show (three consecutive runs, a mutation check, and 22 of 22 checks passing):

- **Idle author (T19):** the Agent rewrites a block (with an emoji in the text); the editor's text equals Rust's, the version advances by one, ids of untouched blocks are kept.
- **Author types while the Agent edits another block (T20):** the Agent's request is posted first and the keystrokes right behind it, so Rust applies the Agent patch first and then refuses the keystrokes. Both changes survive and both sides agree; the transform path was exercised.
- **Author types inside the block the Agent rewrites (T21):** neither side loses text. The typing is kept next to the Agent's new text (`…Agent 整段改写了乙。★我在写★`). Where exactly it lands is a consequence of position mapping, not a merge of meaning; the author's characters are never dropped.
- **Undo (T22):** one Ctrl-Z reverts exactly the Agent's change (an edit made by the author earlier stays), and the revert itself goes to Rust as an ordinary edit.
- **Randomized (T23):** 160 steps of author inserts and deletes (at code-point boundaries, with CJK, emoji and astral characters), Agent operations of three kinds and Ctrl-Z, fired without waiting. Around 40 to 50 Agent patches were integrated, almost all of them over pending edits, with 6 to 10 stale refusals. At the end both sides hold the same text, the editor's mapped block ranges equal a fresh Rust parse, ids are unique, and no refusal other than "stale" occurred.
- Removing the transform from the editor makes the tests fail at once.

**Findings and limits:**

- An Agent operation names a block id. In the randomized run 7 to 19 of 58 operations were refused with "no block with id": a block rewritten wholesale gets a new id, so a second operation aimed at its old id (issued from the editor's slightly outdated list) finds nothing. This is the intended behaviour (an explicit error, never a wrong target), but a real Agent runtime should resolve ids inside Rust at execution time and treat "no such block" as a normal outcome.
- A patch that lands inside the author's current paragraph (T21) does not ask the author anything; the choice made was "keep both". For prose, whether the Agent's rewrite should be applied at all while the author is typing in the same paragraph is a product decision (queue it as a suggestion, or apply and offer undo); this spike shows that applying it is safe, not that it is the right experience.
- **Undo had no keyboard shortcut.** Hand-testing found that Cmd-Z did nothing, while the automated undo test passed. The test called the undo *command*; the editor configuration never bound the history keys (`historyKeymap`). Fixed, and the test now presses Cmd-Z and Shift-Cmd-Z as key events (T22b; with the binding removed it fails). General lesson for this spike: an automated check that calls a command directly does not prove that the user can reach it.
- Undo restores text, not ids: after undoing a wholesale rewrite the block gets a new id, because the id of the old text was not kept by the reconciler for the rewrite (see the block identity results).
- The CodeMirror collaboration package was not used; the transform is a few lines over `ChangeSet`, with the authority's version counter in Rust. Whether to adopt `@codemirror/collab` for its update log and rebase helpers is open.

## Text merge results (2026-10-10, `spike/text-merge`)

Validation item 3: a text-level, block-aware three-way merge. Tokens are UAX#29 words (CJK per character); the diff first aligns blocks, then tokens inside changed blocks; hunks from the two sides are clustered diff3-style; overlapping clusters that the two sides spelled differently are settled by comparing the resulting text, and otherwise become a conflict.

- Edits in different blocks never conflict; disjoint edits in one long paragraph (the weakness of block-level merge) merge cleanly.
- Randomised campaign, 100,000 merges of generated Chinese/English chapters with edits chosen so the union is the one right answer: 96,162 clean, 3,838 conflicts, **0 clean-but-wrong**. 158,614 re-merges of a merged text with either side: **0 silent changes, 0 spurious conflicts**. The same campaign (30,000 cases) is a test.
- Three bugs found only by the campaign (each is now a deterministic regression test): `similar` reports `Insert.old_index` unreliably (use a running cursor); deleting one of two equal neighbours is the same change whichever one is named, so hunks are canonicalised by sliding left; and two sides naming different equal tokens could collapse into one deletion.
- **Verification gate (kept, required):** a clean result is returned only if re-merging it against each side reproduces it; otherwise the whole region is reported as a conflict. It costs about 1% extra conflicts and is the guarantee that a "clean" merge never loses or duplicates an edit.
- Mutation checks: removing the sliding, or the gate, is caught by tests.
- Limits: ambiguity with repeated tokens falls back to conflict, never to a guess; prose diff display and a real-manuscript corpus are not covered; performance on book-length input is not measured here (merge runs per changed block).

## Markdown mirror and external-edit import results (2026-10-10, `spike/md-mirror`)

Validation item 4, with SQLite (rusqlite) as the authority and the text-merge spike as the merge.

- **Write path:** the commit, its history row and an outbox row are one transaction. `drain` writes a temp file under `.digger/tmp/`, fsyncs, renames, fsyncs the directory, and only then records the mirror hash and drops the outbox row (only if no newer version was committed meanwhile). Crashes injected after the temp write and after the rename both converge after restart (`recover` deletes temp files, `import` recognises its own half-finished write because the file's hash equals the database's), with no loss.
- **Import path:** the file's base is the history row whose hash equals the recorded mirror hash, then a three-way merge against the current text. Clean: new version with actor External, the outbox rewrites the file. Overlap: a `pending` conflict, database and file both untouched, repeated imports do not duplicate it, and the outbox refuses to overwrite the file while it is open. No recorded or findable base (e.g. restored from Git): a working state with the full file text, nothing applied. Resolution commits and moves the mirror hash.
- **Guard:** the outbox never overwrites a file whose hash is neither the recorded mirror hash nor the database's (an unimported external edit); without it three tests fail, including the random campaign.
- **Normalisation:** a BOM and CRLF alone are not an edit; invalid UTF-8 is reported and ignored; a deleted mirror file is rewritten, never treated as deleting the chapter. The author's imported file text is kept in history so it can be a base later.
- **Randomised check:** 300 random interleavings (app edits, external edits, drains, imports, crashes with restart) of uniquely marked edits: no marker is lost silently (each is in the text, in a parked conflict, or still in the file), and with nothing parked the file equals the database. 182 of the runs ended with a parked item, which is expected because both sides often insert at the same place.
- **Not covered:** the check-then-rename race (an editor saving between the guard and the rename; needs an OS file lock or a post-rename hash check, to be decided), a real file watcher (the spike calls `import` explicitly), several documents, renames and moves of chapter files, `book.yaml`, and Windows/Linux rename and fsync semantics.

### Agent runtime: ids are resolved in Rust when the step runs (`spike/md-engine`, `sync.rs`)

Follow-up to the finding that 7 to 19 of 58 randomized Agent operations were refused with "no block with id". The rules now implemented in `Doc::agent_plan` / `agent_step`:

- **Resolution at execution time.** A step names the block id the Agent read; Rust resolves it against the current version when the step runs. When a rewrite gives the block a new id, a forwarding entry (old id to new id, tagged with the plan run) is recorded, so a later step of the same plan that still holds the old id reaches the rewritten block.
- **Precondition for `replace_block`.** The step carries `expect`, the hash of the block text the Agent read. If the block now holds different text (the author changed it), the step is refused as `Stale` with the current text, and nothing is overwritten. The precondition is skipped only when the target is reached through forwarding made by the same run (the Agent's own rewrite); forwarding from an earlier run does not bypass it. `insert_after` and `append` depend only on position and have no precondition. A step without `expect` overwrites on purpose.
- **Plans are all or nothing.** If any step is `NoBlock` or `Stale`, the document (text, version, forwarding) is restored exactly. Each step yields its own patch in the coordinates of the version before it, so the existing `update` message and the editor's transform are unchanged. `NoBlock` and `Stale` are reported to the Agent as ordinary outcomes.
- **Results.** 1,500 randomized races (author edits, deletions and new paragraphs between the Agent's read and its plan; plans of 1 to 3 rewrites): 1,073 applied, 204 refused as stale, 223 refused because the block was gone; in every applied plan all author edits survive and every rewrite is present, and every refused plan left the document byte-identical. Removing the precondition, the same-run rule or the rollback each fails tests. The editor suite is unchanged (`ENGINE=cm MODE=test`: 23 passed).
- **Not done:** what the Agent does after `Stale` (re-read and re-plan, or offer the author a choice) is runtime/product behaviour; forwarding survives only in memory, not across restarts, and is not yet recorded in `block_agent_history`; refusing versus keeping both when the author is typing in the same block (T21) remains a product decision.

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
2. **Block id survival:** property tests over simulated edit streams (typing, paste, split, merge, move, thousands of characters inserted earlier) measuring how many ids survive and how many matches are wrong. **Done in simulation; see "Block identity spike results".** Real manuscripts and editor behaviour remain to be checked.
3. **Text merge:** automatic merge of disjoint edits inside one block, conflict on overlap, fuzzed against a reference.
4. **Mirror:** atomic file write, crash between Commit and mirror write, external edit imported through the merge, wholesale replacement handled as WorkingState, no silent loss in either direction.
5. **Parser agreement:** the Rust and editor parsers agree on block boundaries over the CommonMark and GFM example corpora plus a real manuscript. **Done; see "Parser agreement results".** A real prose manuscript and real typing sessions are still to be added to the corpus.
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
