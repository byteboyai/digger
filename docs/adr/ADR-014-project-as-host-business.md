# ADR-014: Project identity, registry and shared memory are host-level business

- Status: Accepted
- Date: 2026-10-15
- Decision owners: Digger Architecture
- Applies to: Project identity and registry, Project-level shared memory, the Dozer/Digger code-sharing boundary
- Supersedes (partially): ADR-003, its definition of what a Project is and where Project identity is stored

## Context

Dozer and Digger both need Projects, and the shared panels (todo, memory, conversations, group chat) are all keyed by `project_id`. ADR-003 made the Project a Digger-private directory with its own `project.db`. Keeping that would give two incompatible Project concepts and force every shared panel to be adapted twice.

## Decision

1. **A Project is a directory on disk.** Its identity (`ProjectInfo`: id, path, name, activity and creation times, git-aware update time), the registry of known Projects, and Project-level shared memory (`MemoryInfo` / `MemoryDetail` and their history) are host-level business, not product-private state.
2. **One implementation, shared as code.** `dozer-store-project` holds `ProjectStore`, `MemoryStore` and the in-process `LocalProjectService` / `LocalMemoryService`. Dozer's daemon and Digger both use it.
3. **Products stay independent at run time.** Each product opens its own SQLite file in its own data root (Digger: `<config>/projects.db`). There is no shared process, port, directory or database. What is shared is source code, pinned by git revision.
4. **The service seam is split.** `ProjectService` is the registry only (open, list, rename, remove). Transcript backfill/delete and summary batches are `TranscriptService`, which only Dozer implements.
5. **Open-tab state is the product's.** Which Project tabs are open, and which is active, stays in each product's shell until the shared top bar is adopted by both.

## Consequences

- ADR-003 no longer defines Project identity or storage. Its transactional design (one `BEGIN IMMEDIATE` transaction committing domain state, Commit, ProjectRevision and Outbox events) concerns Creator Domain state, which does not exist yet; it is re-evaluated when the first Creator Domain state lands, and is not decided here.
- Shared panels can use a real `project_id` in Digger. The first consumer is the todo panel (S3b).
- Unregistering a Project does not delete its memory rows (same as Dozer); ids are never reused, so orphaned rows cannot attach to a different Project.
- Dozer keeps working unchanged: `dozerd` re-exports the stores under their old paths and serves the same protocol.
