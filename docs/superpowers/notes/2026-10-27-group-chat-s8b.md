# S8b group chat list — known gaps (2026-10-27)

1. **GUI not exercised.** `group_chat_gallery --check` validates the synthetic
   two-group fixture and one `Running` reply headlessly. No window was opened;
   modal stacking, backdrop hit testing, focus, IME and the real list layout
   remain unverified.
2. **No live daemon run.** Unit tests use fake `GroupChatService`; the real
   `diggerd` path, slow/failing sockets and 5s backoff were not exercised.
3. **Library submit failure loses the dialog/topic.** The pinned
   `dozer-panel-group-chat` `NewGroupSubmit` closes the dialog and clears the
   text editor before the async create result. A failed create therefore cannot
   keep the user's draft without a library change. The host reports the failure
   in the notice bar but cannot restore the dialog.
4. **List collapse button interface.** `PanelDisplay<Message>::list_collapse_button`
   can only return the library `Message`, not a host `ShellMsg`. The product
   adds a real direction-aware collapse button above the library list and an
   expand button in the collapsed content slot; the adapter's required trait
   method remains a no-op placeholder until the library widens the host message
   type or provides a callback seam.
5. **No group-content chat.** The content column is the S8b native placeholder
   (selected topic, member handles, “聊天内容即将可用”). Sending, streaming,
   retry, cancel and Todo push are intentionally absent until S8c.
6. **Polling timing is structural, not wall-clock tested.** Tests cover hidden
   ticks, focus gating, single project state routing and daemon failure notices.
   Exact 500ms cadence, max-one-in-flight and 5s backoff are delegated to the
   shared library and were not measured in a real event loop.
7. **NoticeBar migration is API-compatible.** Agent and Conversations now hold
   the shared `NoticeBar` (with `Deref<Target=Option<String>>` for old read-only
   call sites), but their existing bespoke row rendering was kept; only the
   group list uses `NoticeBar::view`. Visual parity was not checked.
8. **Modal keyboard scope.** Esc closes the dialog and the product marks
   terminal keys as blocked while the dialog is open. Focus/trap behavior of the
   iced text editor under a real window manager was not verified.
9. **Gallery uses a product-level fixture, not a daemon fixture.** It proves the
   library list/state render seam, not backend authorization or member creation.

10. **Pre-existing test flake.** Two `agent_hooks` install-timeout tests failed
    once under parallel full-workspace load; both pass on immediate rerun and the
    desktop lib passes 620/620. This is not a group-chat failure.
