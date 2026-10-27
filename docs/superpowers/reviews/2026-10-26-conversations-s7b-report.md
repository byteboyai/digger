# S7b Digger 会话列表交付报告

> **状态更新(2026-10-27):** 已合并进 `main`;用户已完成人工 GUI 验收。下文「GUI 未验 / 未达到人工验收」为写报告时的状态,已不再成立。全 workspace 中与基线相同的那条失败未在此重新核对。

日期：2026-10-26。分支：`feat/conversations-list`。实现基线：Digger `main` 的 `682fc53`；Task 1–4 实现 HEAD：`e90c6c9`。

**当前状态：Task 1–4 实现及各任务的 spec / quality review 已完成；Task 5 自动验证结束，全 workspace 712 passed / 1 failed / 0 ignored，唯一失败与基线相同。构建、lint、格式和门禁通过。当前环境未能启动 GUI，截图与九项用户点击验收尚未完成；尚未达到人工验收完成状态。**

## 交付范围

右栏登记持久化 id `digger.conversations`，默认顺序为 Agent、会话、应用、用量，默认仍选 Agent。会话面板按配对规则渲染列表和“审阅详情即将可用”占位，支持搜索、Agent 筛选、分页及列表折叠。每项目保存独立库状态，真实服务来自同一个 `Daemon` 客户端。列表结果通过 `Message::result_project()` 和库的 RequestToken 路由与校验。

只有 `TurnEnded` 联动刷新；隐藏时记录 dirty，再次可见时刷新。macOS 筛选使用原生菜单，无新增 Overlay；非 macOS 保留库的 iced 回退。历史导入只由明确按钮发起，后台调用当前项目的真实 `TranscriptService`；没有在激活、刷新、Agent 回合结束或 Project 修复时静默补录。

能力保持 `ConversationsCapabilities::default()`，`LinkedTodos` 为空实现，没有生成总结或关联待办入口。点击会话行正常交给库执行 `get_conversation_turns(cid, -1, 200)` 并接收详情状态；本期内容仍为占位，真实审阅 webview 留给 S7c。

## 提交与测试计数

| Task | 实现提交 | 新增测试 | Desktop library 累计 |
|---|---|---:|---:|
| 基线 | `682fc53` | — | 442 |
| 1：登记、配对、迁移 | `bf6cfa3` | 9 | 451 |
| 2：数据、视图、错误与隔离 | `e644fd9`、`3b9367d`、`c412a3e` | 35 | 486 |
| 3：原生筛选、回合刷新、搜索焦点 | `03eb1e2` | 17 | 503 |
| 4：显式历史导入、提示标记 | `e90c6c9` | 25 | 528 |
| 合计 | Task 1–4 | **86** | **528** |

基线实际全 workspace 为 **626 passed / 1 failed / 0 ignored，总计 627**，与交接给出的 627 全绿不同。证据：`/private/tmp/s7b-baseline-full.log`、`/private/tmp/s7b-baseline-totals.json`。Task 1 首个提交已记录实际基线。

核对算式：`9 + 35 + 17 + 25 = 86`；`627 + 86 = 713`。Desktop library：`442 + 86 = 528`；Desktop 另有 5 个 integration tests。未删除测试。终态全 workspace 为 **712 passed / 1 failed / 0 ignored，总计 713**；`712 + 1 + 0 = 627 - 0 + 86 = 713`。唯一失败为下述既有 recovery 测试，所有新增测试通过。证据：`/private/tmp/s7b-final-test.log`、`s7b-final-totals.json`，以及同目录提交的 `2026-10-26-conversations-s7b-validation.json`。

## 基线失败与重跑记录

既有失败为 `digger-app-host` integration test `a_restart_after_a_kill_reaps_the_recorded_orphan_and_leaves_unrelated_processes_alone`：预期 `Killed`，实际 `Stale`。当前沙箱禁止执行 `/bin/ps`，导致进程身份查询无法完成，监督逻辑按保守路径返回 Stale。未绕开沙箱，也未弱化断言或修改该测试。

基线原命令 `cargo test --workspace --no-fail-fast` exit **101**。之后对上述测试精确隔离重跑 3 次，exit 均为 **101**，即 **3/3 失败**，另有一次同条件重试仍失败；见 `/private/tmp/s7b-baseline-recovery.json`、`/private/tmp/s7b-baseline-recovery-{1,2,3}.log`、`/private/tmp/s7b-recovery-retry.log`。该失败不能记为已修复，也不能仅因环境原因把全量测试记为通过。

Task 1 的 Desktop 首轮有 5 项既有 hook / startup 超时；对每项精确隔离 3 次，共 **15/15 通过**（每次 exit 0），包括交接点名的 `apps::tests::closing_during_startup_waits_and_releases_the_root`。证据：`/private/tmp/s7b-task1-isolated.json` 与对应日志。Task 2 的 `adapters::agent_hooks::tests::the_notice_text_names_the_config_file_for_protection_refusals` 首轮超时，随后精确隔离 **3/3 通过**，exit 均为 0，见 `/private/tmp/s7b-task2-isolated.json`。未靠重复全量测试掩盖失败。

Task 3 首轮 broad run 的一个新测试失败来自测试夹具漏建 Agent state，修正夹具后再跑；这是确定性夹具错误，不归类为偶发失败。最终 Task 3 Desktop 503 library + 5 integration 全通过；Task 4 Desktop 528 library + 5 integration 全通过。

## 最终干净快照验证

从实现 HEAD `e90c6c9` 用 `git archive HEAD` 导出 `/private/tmp/s7b-clean-20261026`，目录路径不含 `digger`，无 `.git`，使用快照自己的 `target`。环境为独立 `HOME=/private/tmp/s7b-clean-20261026/validation-home`、`DIGGER_DATA_DIR=/private/tmp/s7b-clean-20261026/validation-data`，显式使用现有 `CARGO_HOME` / `RUSTUP_HOME`，取消外部 `CARGO_TARGET_DIR`，测试设 `RUST_TEST_THREADS=1`。Bash 门禁使用 `/opt/homebrew/bin` 的 Bash 5。

最终结果机器记录：`/private/tmp/s7b-final-results.json`；已将命令结果、额外门禁、计数与锁文件核对归档为本报告同目录的 `2026-10-26-conversations-s7b-validation.json`。以下为实际终态退出码。

| 命令 | 最终退出码 / 结论 | 证据 |
|---|---|---|
| `cargo build --workspace` | **0，通过** | `/private/tmp/s7b-final-build.log`；results.json 的 build entry |
| `cargo test --workspace --no-fail-fast` | **101，712 passed / 1 failed / 0 ignored**；唯一既有 recovery 失败，未宣称全绿 | `/private/tmp/s7b-final-test.log` |
| `cargo clippy --workspace --all-targets` | **0，通过** | `/private/tmp/s7b-final-clippy.log` |
| `cargo fmt --check` | **0，通过** | `/private/tmp/s7b-final-fmt.log` |
| `bash scripts/check-host-deps.sh` | **0，通过** | `/private/tmp/s7b-final-host.log` |
| `bash scripts/check-host-deps.sh --self-test` | **0，通过**；bytehost-* / iced / wry 三项变异全部被拦截，恢复后通过 | `/private/tmp/s7b-final-host-self.log` |
| `cargo tree --duplicates` 与 Cargo.lock 共享库名称重复核对 | **0**；26 个共享包名均无重复，第三方多版本依赖见日志 | `/private/tmp/s7b-final-duplicates.log`；`/private/tmp/s7b-final-lock.json` |
| `bash scripts/check-shared-deps.sh` | worktree **0，通过**；archive **128**（无 `.git`） | `/private/tmp/s7b-final-shared-worktree.log`、`s7b-final-shared-archive.log`、`s7b-final-extra-results.json` |
| `bash scripts/check-shell-regions.sh` | **0，通过**，7 个外壳区域逐项一致；环境见下 | `/private/tmp/s7b-final-shell-locked.log`、`s7b-final-shell-locked-result.json` |

已知运行条件：archive 中 `check-shared-deps.sh` 的 git 查询会 exit 128，应在 worktree 执行并注明，不以 archive 缺 `.git` 认定依赖错误。Task 2/3 的 shell-regions 默认调用因 `/private/tmp` 缺默认 `../dozer` exit 2，本次通过显式 `BYTEUI` / `DOZER` 路径及独立 target 完成核对。host self-test 使用只读源仓引用的 `/private/tmp/bytehost` 符号链接解决 sibling 位置要求。

外壳样式检查实际命令：`CARGO_NET_OFFLINE=true BYTEUI=/private/tmp/s7b-byteui-reference DOZER=/Users/chrischiang/Projects/CoralProjects/byteboy/dozer /opt/homebrew/bin/bash scripts/check-shell-regions.sh`，exit 0。ByteUI 参考源码从锁定 v0.4.6 的 `51ca9aa5d3290155cdd0b2e72884601af2603651` 导出到临时目录，使用自己的 target；Dozer 仅只读主题 JSON。参考库没有提交 Cargo.lock，首次默认解析需下载当前不可写 Cargo cache 的 lyon_algorithms 1.0.22（exit 101），无锁离线解析又遇到 yanked clipboard-win（exit 101）。最后仅给临时参考快照复制本次已验证的 Digger Cargo.lock 并离线解析后通过；没有改库源或 Digger 的锁文件。初次日志 `s7b-final-shell-regions.log`、`s7b-final-shell-offline.log` 均保留。

## 分任务验证证据

以下为各任务当时已完成的检查，不能替代上表最终 archive 全量验收。

| 范围 | 命令与结果 | 证据 |
|---|---|---|
| Task 2 最终 focused | `cargo test -p digger-desktop conversation --lib`，exit 0，40 passed | `/private/tmp/s7b-task2-status-green.log` |
| Task 2 lint | `cargo clippy -p digger-desktop --all-targets`，exit 0 | `/private/tmp/s7b-task2-status-clippy.log` |
| Task 3 Desktop | `RUST_TEST_THREADS=1 cargo test -p digger-desktop`，exit 0，503 library + 5 integration passed | `/private/tmp/s7b-task3-desktop-final.log` |
| Task 3 lint | `cargo clippy -p digger-desktop --all-targets`，exit 0 | `/private/tmp/s7b-task3-clippy.log` |
| Task 4 focused | `cargo test -p digger-desktop conversations --lib`，exit 0，79 passed | `/private/tmp/s7b-task4-focused-last.log` |
| Task 4 Desktop | `RUST_TEST_THREADS=1 cargo test -p digger-desktop`，exit 0，528 library + 5 integration passed | `/private/tmp/s7b-task4-desktop-final.log` |
| Task 4 lint / fmt | `cargo clippy --workspace --all-targets`、`cargo fmt --all --check`，各 exit 0 | `/private/tmp/s7b-task4-clippy-final.log`、`/private/tmp/s7b-task4-fmt-final.log` |
| Task 4 依赖门禁 | `bash scripts/check-shared-deps.sh`、`bash scripts/check-host-deps.sh`、`bash scripts/check-host-deps.sh --self-test`，各 exit 0 | `/private/tmp/s7b-task4-shared.log`、`/private/tmp/s7b-task4-host.log`、`/private/tmp/s7b-task4-host-self.log` |

Task 2/3 的完整命令、首次 red 与修复记录另见 `/private/tmp/s7b-task2-validation.md`、`/private/tmp/s7b-task3-validation.md`。Task 4 API 尚不存在时 red exit 101：`/private/tmp/s7b-task4-red.log`、`/private/tmp/s7b-task4-red-full.log`；当前项目限制的真实回归 red exit 101：`/private/tmp/s7b-task4-scope-red.log`。原有 `block 0.1.6` future incompatibility 提示须与源代码 lint 分开记录。

## Review Focus 逐项对应

以下均为仓库中实际测试名，可用 `rg` 定位。`shell` 测试在 `crates/desktop/src/shell.rs`，列表 / 导入测试在 `crates/desktop/src/adapters/conversations_tests.rs`，标记测试在 `adapters/conversations_import.rs`，product 测试在 `product.rs`。

### 1. 项目隔离与路径边界

- `messages_for_one_project_never_touch_another_projects_list`
- `a_message_for_a_project_without_state_is_ignored`
- `late_messages_after_close_project_are_ignored`
- `reopening_a_project_rejects_the_old_owners_tokens_and_errors`
- `the_list_only_contains_sessions_under_the_current_project_path`
- `parent_and_child_projects_pass_their_own_cwd_and_never_each_others`
- `conversations_results_route_to_the_token_project_even_while_it_is_unfocused`
- `conversations_tab_close_reclaims_state_and_drops_late_results`
- `conversations_project_delete_reclaims_state`
- `marker_for_project_a_does_not_affect_project_b_and_paths_are_isolated`
- `import_completion_cannot_cross_projects_or_apply_twice`
- `closing_the_project_during_import_drops_the_result_quietly_even_after_reopen`
- `reopened_project_new_import_rejects_old_owner_completion_while_importing`

覆盖范围：宿主按项目传入各自原始 cwd，token / owner / 关闭状态不串项目；临时导入状态随关闭清理，持久标记仍用于下次打开的提示。**父子目录、末尾斜杠、符号链接、大小写不敏感卷的实际会话匹配由 diggerd 决定，以上测试不能证明后端匹配政策。** 真机仍待验证；若父项目看到子目录会话，按交接报告为 Dozer 同样存在的行为，不在 Digger 另做过滤。

### 2. 导入安全性与响应

- `prompt_appears_only_for_an_empty_loaded_list_without_a_marker_and_with_a_live_daemon`
- `start_while_importing_is_ignored`
- `conversations_import_start_is_limited_to_current_project_and_never_runs_during_exit`
- `importing_during_exit_or_without_eligible_project_never_calls_the_service`
- `activation_refresh_turn_ended_and_passive_visibility_never_import_history`
- `import_runs_off_the_ui_thread_even_when_constructing_the_future_blocks_and_count_is_5000`
- `success_marks_saves_and_refreshes_and_completion_survives_nonempty_refresh_and_restart`
- `zero_imported_still_marks_and_shows_that_nothing_was_found`
- `failure_does_not_mark_and_allows_retry`
- `disclosure_text_is_always_shown_with_the_button_including_importing_and_retry`
- `marker_save_failure_is_visible_and_preserves_the_actual_completed_count`
- `import_without_home_uses_injected_marker_and_actual_product_log_path_in_isolated_child`
- `marker_file_corrupt_missing_or_newer_is_treated_as_empty_and_newer_is_never_written`
- `marker_save_is_atomic_and_failed_replace_preserves_old_complete_bytes_and_removes_temp`
- `import_marker_is_loaded_once_and_never_substitutes_for_list_truth`
- `actual_import_and_retry_buttons_dispatch_start_and_importing_is_disabled`

响应测试在假服务构造 future 前阻塞 200ms、返回 5000 的计数，并断言 update 小于 50ms；证明服务构造与调用脱离 UI update，不代表已测过真实数千文件的磁盘导入。重复导入的底层去重由 diggerd 负责，Digger 不另实现去重；真实文件、受限目录、三份 Claude transcript、重启提示与数据库不重复仍待人工验收。

**用户于 2026-10-26 批准的接口取舍：** 沿用 `backfill_project_transcripts -> Result<u32, ServiceError>`，成功只显示 `Done(n)` 的条数，0 条显示“没有找到可导入的会话”；部分成功同样写提示标记。整体 Err 显示错误、不写标记、允许重试。按钮旁与完成提示分别显示“个别文件读取失败不会在这里列出,详情见 diggerd 日志”，并展示 `digger_paths::logs_dir()` 的真实值，不硬编码，也不解析日志绕开接口。

现有接口不能返回部分失败的条数和原因，所以报告不能声称 UI 展示了每文件错误。要补齐需扩展 dozer-services 的 TranscriptService 返回失败明细，并发布 Dozer 新 libs tag；Dozer 的 Project 面板同受此限制。标记仅表示提示已处理，不表示每个文件成功。

### 3. 刷新风暴与乱序

- `turn_ended_refreshes_only_the_matching_visible_project`
- `turn_ended_while_hidden_marks_dirty_and_the_next_visibility_refreshes_once`
- `ten_turn_ended_events_in_a_row_leave_only_the_latest_result_applied`
- `conversations_state_changed_alone_does_not_refresh`
- `conversations_host_turn_events_respect_readiness_visibility_and_current_project`

使用真实库发行的 RequestToken 和 oneshot 假服务，使最新结果先于旧结果送达，验证旧结果不能覆盖新结果。沿用既有 `drain_host_events`，未另建事件排空器；只有 `TurnEnded` 才触发会话联动。

### 4. 守护进程不可用与恢复

- `daemon_unavailable_shows_the_reason_not_an_empty_list`
- `retry_after_unavailable_refetches`
- `service_failures_surface_as_a_dismissable_notice`
- `unavailable_list_failures_are_visible_even_when_the_library_outbox_is_empty`
- `stale_list_failures_do_not_surface_as_new_notices`
- `conversations_shared_daemon_recovery_fetches_once_without_starting_another_lifecycle`
- `conversations_accepted_unavailable_changes_shared_status_and_retry_refetches`
- `conversations_stale_unavailable_does_not_change_shared_daemon_status`
- `conversations_closed_project_unavailable_does_not_change_shared_daemon_status`
- `conversations_reopened_owner_unavailable_does_not_change_shared_daemon_status`
- `import_prompt_rejects_unavailable_daemon_and_actual_view_explains_shared_retry`

接受的 typed Unavailable 列表结果通过既有 Task 改变共享 AgentShell 状态，重试复用既有 ensure；旧 token、关闭项目与旧 owner 不得改变共享状态。不可用时真实视图显示原因及重试，没有可点击的导入入口。错误沿用 Agent 样式的可关闭红字并写 stderr，不再启一套 daemon 生命周期。

### 5. 布局迁移与窄宽度

- `conversations_panel_id_is_the_persisted_wire_name`
- `the_default_layout_puts_conversations_right_after_agent_and_keeps_agent_selected`
- `an_old_file_without_the_panel_places_it_after_agent_and_keeps_the_users_selection`
- `an_old_file_without_agent_in_the_right_rail_gets_it_at_the_front`
- `migration_is_idempotent`
- `a_newer_schema_file_is_still_never_written`
- `pair_rects_for_conversations_have_nonnegative_sizes_at_the_minimum_window`
- `conversations_pair_uses_the_real_list_width_and_collapse_leaves_only_the_placeholder`
- `the_actual_expanded_conversations_view_has_a_clickable_collapse_control`
- `the_actual_collapsed_conversations_view_has_a_clickable_expand_control`

最后两项在真实 panel view 上布局并点击真实控件，包含右栏与左栏镜像，没有用独立按钮代替真实视图。width 0 退化布局也验证不出现负尺寸。

### 筛选与搜索键盘补充

- `filter_menu_lists_all_plus_present_agents_and_locks_the_current_one`
- `selecting_an_agent_resets_paging_and_closes_the_menu`
- `escape_and_outside_click_close_the_filter_menu_without_changing_the_filter`
- `conversations_filter_request_preserves_anchor_current_and_present_without_an_iced_overlay`
- `conversations_iced_fallback_closes_agent_overlays_and_agent_picker_closes_it`
- `conversations_real_focus_operation_updates_gain_and_loss_and_rejects_closed_owner`
- `conversations_real_search_text_input_captures_characters_before_agent_subscription`
- `conversations_search_focus_blocks_keys_and_late_hidden_focus_is_ignored`
- `conversations_selected_with_agent_hidden_blocks_uncaptured_terminal_keys`
- `conversations_opposite_rail_allows_agent_keys_until_search_really_has_focus`

搜索焦点每 16ms 通过带项目 key 的 Subscription 发起 iced `is_focused` Operation，反馈校验 project / owner / 最新查询 token 后写回库。隐藏、切项目、收起、不可用、关闭时清理焦点并作废旧反馈。订阅读取既有进程级 AtomicBool，不使用 thread_local 回调状态。真实 text_input 的字符捕获与 Agent 订阅阻止路径均有测试；中文候选窗口、拼音提交与位置仍必须人工验证。

## 与原计划不同的实现及原因

1. 原计划要求 sibling worktree，但当前文件系统仅 Dozer 工作区与 `/tmp` 可写；因此从 Digger 基线建立独立临时 clone `/private/tmp/digger-s7b`，仍使用 `feat/conversations-list`，不改用户主工作区。开工观察到原 Dozer 为 `main fa05b398`；收尾只读检查已是 `c76302fc`，仍有 dirty Cargo.lock 与未跟踪 review-trace（属于会话外的工作，本任务未写入 Dozer）。原 Digger 保持 `main 682fc53`，13 份用户删除文档及 `.qoder/` 保留。
2. 依赖此前已升至 `libs-2026-10-26a`，只补 `dozer-panel-conversations` workspace / desktop 声明，不再升级 tag。`Cargo.lock` 只给 desktop 增加两条指向已有包的依赖边：正式的 `dozer-panel-conversations` 和用于 headless 真实 widget 布局 / 点击验证的开发依赖 `iced_tiny_skia`；没有新增包版本、本地 patch 或新来源。
3. 以真实 S7a API 为准：使用 `UpdateCtx`、库的 `on_activate` / refresh、`result_project()`、`take_outbox()` / `take_requests()`；不按原稿虚构 API。显示对象按项目持有以满足库 view 的借用要求，无 self-reference / unsafe。
4. macOS 使用原生 Agent 筛选菜单，不增加 `Overlay::AgentFilter` 或 overlays_open 标记；非 macOS iced 回退在 macOS cfg(test) 中覆盖，但没有完整非 macOS desktop target build。
5. 原计划称搜索字符捕获无需新代码；交接要求核查后实际增加焦点 Operation 与键盘阻止条件，覆盖搜索获得焦点和 Agent 隐藏的情况，对侧可见 Agent 在搜索未聚焦时仍可输入。
6. 库的 unavailable 列表错误可能不进入 outbox；宿主只对已接受 token 的 typed 错误补红字、stderr 与共享 AgentShell 状态，拒绝 stale / closed / old-owner 结果。详情读取错误因库已擦除 ServiceError 类型，只显示通知，不据字符串推断 daemon 状态。
7. “已导入”标记采用计划 D9 独立 JSON，不改 digger.db schema。schema 1，原子写；未来 schema 不回写，缺失或损坏按未导入。启动只加载一次，列表内容始终是真相。
8. notes 按交接使用 `docs/superpowers/notes/2026-10-26-conversations-s7b.md`，而非原计划 Step 3 的旧日期路径。历史导入的部分失败披露采用用户已批准修订，未改共享接口。

## 已知缺口与后续边界

- 删除 Digger 项目不会清理该项目会话，`delete_project_transcripts` 未接，ProjectPanel 的 `NoTranscript` 保持不变；留待用户裁决。
- 搜索框右键 `TextInputMenuOpen` 无宿主设施，保持空操作。
- 列表宽度来自 `BodyArea.pair.first.w`，无 pair 时使用 `area.rect.w`，非负钳制；最小宽度取 `shell::list_min_width()`，具体观感待用户目测。
- `hover_progress` 恒为 0，悬停变化为阶跃，提示控件始终包 Tooltip。
- 详情 webview、本机宿主抽取、总结与关联待办、群聊、产品发布均不在本期。
- 真实路径匹配、文件去重、实际导入 IO 性能、原生菜单位置与取消、中文 IME 尚待真机验收，不由自动假服务测试替代。

详见 `docs/superpowers/notes/2026-10-26-conversations-s7b.md`。

## 给用户的 Digger 目测验收清单（全部待点击）

由 agent 启动独立 `DIGGER_DATA_DIR` 与临时 HOME 的沙箱并截图，**所有交互点击由用户执行**。不碰 `/private/tmp/digger-sandbox`、`/Applications` 的运行实例或其 diggerd。任一项不符即停下记录。

独立沙箱根为 `/private/tmp/s7b-manual-20261026`，HOME 为其 `home` 子目录，`DIGGER_DATA_DIR` 为其 `data` 子目录。通过正常 daemon handshake / open_project 登记 `alpha`（id 1），准备 3 份 JSONL fixture，**未替用户点击导入，也未预先导入**。独立 daemon PID 9093 已 ready；原用户 Digger PID 28169 保持不动。

**当前执行环境未能启动真机 GUI，九项仍全部待验收。** 启动 Desktop PID 9250 后出现 `LSModifyNotification` 与 `hiservices-xpc connection invalid`，Orca `get-app-state` 返回 `app_not_found`，应用列表只见既有用户 Digger。另在临时目录准备了唯一 bundle id `ai.byteboy.s7b.acceptance.20261026` 的 debug `.app`，含 LSEnvironment；`/usr/bin/open -n` 返回 `-10827 kLSNoExecutableErr`，虽已验证实际 174MB 可执行文件为 Mach-O 且有执行权限。没有继续重试 GUI，也没有伪造截图。

用户可在正常桌面终端运行 `/private/tmp/s7b-manual-20261026/launch.sh`，它固定隔离 HOME 与数据根并启动临时 `/private/tmp/s7b-manual-20261026/S7b Acceptance.app/Contents/MacOS/digger`。应用未安装到 Applications。启动记录位于 `processes.json`（记录启动请求 PID，不能据此认定进程仍活跃），诊断位于沙盒根的 `daemon.log` / `desktop.log`；截图目前为**无**。此环境阻挡不证明产品 UI 通过或失败，自动测试不能替代这九项人工验收。

| 项 | 用户检查动作与预期 | 结果 / 截图 |
|---|---|---|
| ① | 全新数据根开项目：会话面板空态、历史读取说明与明确导入按钮正确 | 待用户点击；待截图 |
| ② | Agent 跑一次真实 Claude 会话至回合结束：可见会话面板无需切换自动出现该条；隐藏后重新显示也刷新 | 待用户点击；待截图 |
| ③ | 放入三份隔离 Claude transcript，点导入：历史出现、只显示成功条数与实际 diggerd 日志目录的失败详情提示；0 条显示“没有找到可导入的会话”；标记生效，重启不再提示；核对重复数据与受限文件行为 | 待用户点击；待截图 |
| ④ | 两项目互不串；`/a` 与 `/a/b` 嵌套项目及路径边界表现记录清楚；若后端父目录包含子目录按已知语义报告 | 待用户点击；待截图 |
| ⑤ | 搜索、原生 Agent 筛选位置 / 当前项锁定 / 取消、“更多…”分页、窄宽与折叠 / 展开，左右栏镜像正常；点击行不报错且保持详情占位 | 待用户点击；待截图 |
| ⑥ | 仅在独立沙箱对 diggerd 做可恢复不可用探针：显示原因、导入不可点，恢复后重试重新取数 | 待用户点击；待截图 |
| ⑦ | 旧 shell.json 无该面板：面板排在 Agent 后，原选中面板保留；更高 schema 文件不被回写 | 待用户点击；待截图 |
| ⑧ | 与 Dozer 同机并存：各自仅看到各自数据库中的会话，不干扰运行实例 | 待用户点击；待截图 |
| 中文 IME | 会话搜索框输入拼音、选择中文候选、提交 / 删除：候选框位置与文本正确，字符不进入 Agent 终端 | 待用户点击；待截图 |

## 收尾状态

未修改 Dozer、共享库、守护进程或协议；依赖 tag 保持 `libs-2026-10-26a`。没有合并、创建 tag、推送、安装或修改版本号。停留于 `feat/conversations-list`。Task 5 本次提交仅归档报告与自动验证记录（可通过 `git log -- docs/superpowers/reviews/2026-10-26-conversations-s7b-report.md` 定位）。自动验证结果和基线失败已如实记录；仍待用户在可启动 GUI 的桌面环境完成九项目测验收，再决定后续合并。本期不发布。


最终锁文件核对：worktree 与 archive 的 Cargo.lock SHA-256 相同，为 `49712a59e7089c69262a33bd4f2404f4acd19b09b112bc1360b610181b5dfa8f`；host mutation self-test 恢复了注入。26 个共享包（dozer-* / bytehost-* / byteui）无重复名称，所有 dozer 包源均为 `libs-2026-10-26a#609a18df`。`cargo tree --duplicates` exit 0 只表示成功生成重复树；共享包无重复的结论来自额外 TOML 名称分组检查，不将命令退出码误作无第三方重复。
