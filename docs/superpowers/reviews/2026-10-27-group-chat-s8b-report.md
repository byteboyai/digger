# S8b group chat list — delivery report (2026-10-27)

## Scope and base

- Plan: `dozer/docs/superpowers/plans/2026-10-27-digger-handoff-11-s8b-group-chat-list.md`
  and `digger/docs/superpowers/plans/2026-10-18-group-chat-s8b-digger-list.md`.
- Worktree: `../digger-s8b`; branch `feat/group-chat-list`.
- Base: Digger `main` at `f149e23` (S7c merged).
- Pin: `dozer-panel-group-chat` / `dozer-services` at `libs-2026-10-26b`
  (`c76302fc56297e0263f7c56670217dea81bcaace`).
- No Dozer, protocol, shared-library or `dozer-panel-group-chat` source changes.

## What landed

### Task 1 — shell registration and migration

- New catalog panel id `digger.group_chat`, title 群聊, icon
  `SquareSparkles`, default right rail
  `[agent, group_chat, conversations, apps, usage]`.
- Right-side pair rules and `min_first_px` reuse the conversations geometry.
- New idempotent migration `group_chat_after_agent` runs after the existing
  conversations migration, so old files get `agent, group_chat, conversations`
  without changing the selected panel. Files that already contain the id are
  untouched.

### Task 2 — host, list and placeholder content

- `adapters/group_chat.rs`: per-project `WorkspaceState` and
  `GroupChatDisplay`; project-scoped routing; late results after project close
  are dropped; project switch sets the focused project.
- `Daemon::services()` builds one real `Client` for session, conversation,
  summary, transcript and group chat; unused services are explicit
  `UnavailableService` placeholders.
- The pair view is library list | native content placeholder. The placeholder
  shows the selected topic and member handles plus “聊天内容即将可用”; there is
  no composer, send, retry, cancel or Todo-push entry point.
- `adapters/notice.rs` adds the shared `NoticeBar` (one line, dismiss button,
  `eprintln!`), now held by Agent, Conversations and Group chat. A
  `Deref<Target=Option<String>>` keeps existing read-only call sites working.
- Library list/card messages are mapped to `Msg::GroupChat` at the product
  boundary; the display remains `PanelDisplay<dozer_panel_group_chat::Message>`.

### Task 3 — polling

- Subscription is on only when the panel is visible, the focused project's
  library state wants polling, and the shell is not closing. Interval is 500 ms.
- Hidden ticks return immediately; only the focused project is polled.
- Load/poll in-flight suppression, one request per project and the 5 s backoff
  come from the shared library state, not a second host-level timer.

### Task 4 — new-group modal

- The library `＋` opens the library new-group dialog; the product renders it in
  an iced `stack!` modal over the whole body.
- Backdrop click is a no-op; Esc and cancel close; the dialog re-runs the
  library submit path once and only uses `create_group`/`add_group_member`.
  Tests assert no `post_group_message` / `retry_group_message` calls.
- Opening the group dialog closes the Agent picker; switching project cancels
  the old project's dialog. Terminal key delivery is blocked while the current
  project's dialog is open.
- Failures go to the dismissible notice bar and stderr; no Toast.

### Task 5 — verification assets

- `crates/desktop/examples/group_chat_gallery.rs`: synthetic two groups
  (`S8b 列表与新建群`, `发布前回归`) and one `Running` reply. `--check` validates
  selection and state headlessly; the normal mode opens the real library list.
- `scripts/sandbox/group-chat/launch.sh [run|check]`: isolated `HOME`/`TMPDIR`,
  no daemon, no installed app, no user data.
- Known gaps: `docs/superpowers/notes/2026-10-27-group-chat-s8b.md`.

## Verification

- `cargo test -p digger-desktop --lib`: **620 passed, 0 failed**.
- `cargo test --workspace --no-fail-fast`: see final gate record below.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p digger-desktop --all-targets -- -D warnings`: clean.
- `scripts/check-shared-deps.sh`: 0 blocking, 0 warnings.
- `scripts/check-host-deps.sh`: ok.
- `scripts/check-shell-regions.sh`: ok.
- `group_chat_gallery -- --check`: `2 groups, selected=1, one Running reply`.

Baseline and final full-workspace counts are recorded after the clean archive
run; no baseline test was deleted or renamed.

## User acceptance

1. In a scratch worktree build, open Digger and toggle 群聊 from the right rail.
2. Confirm the list opens next to Agent on old layouts and the selected panel
   does not change.
3. With `diggerd` stopped, confirm a single dismissible notice appears in the
   group list and stderr has the reason; start `diggerd` and reopen the panel.
4. Create a group with two agents; confirm the dialog is modal, backdrop clicks
   do nothing, Esc/cancel closes it, and the new group is selected.
5. Confirm the content side says “聊天内容即将可用” and there is no composer or
   send action.
6. Confirm no agent processes are spawned and no group message is posted by
   create/select/delete.
7. Repeat in two projects; confirm polling and dialogs never cross projects.
8. Run the gallery: `scripts/sandbox/group-chat/launch.sh check` and `run`.

Stop point: branch left on `feat/group-chat-list`; no merge, tag, push, release or
install performed.
