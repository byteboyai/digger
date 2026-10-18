# S7b Digger 接入会话面板:列表、历史导入与联动 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Digger 右栏出现会话面板(Agent 之后),配对布局:会话列表(搜索、Agent 筛选、分页、卡片)是配对的第一块,按右栏的约定渲染在内容块之后(即「内容 | 列表」,拖到左栏时镜像,与用量和 Dozer 的会话面板一致);内容块先放一个「审阅详情即将可用」的占位(真实详情是 S7c)。数据来自 `diggerd` / `digger.db`;Agent 回合结束时列表自动刷新;历史会话经一个**明确的按钮**导入,不静默复制。本期**不发布产品版本**。

**Architecture:** 全部在 `digger-desktop`。`adapters/conversations.rs` 的 `ConversationsPanel` 持有 `HashMap<i64, WorkspaceState>`(每项目一份,库的状态)、从 `daemon::Daemon` 取的 `ConversationService` / `SummaryService` / `TranscriptService`(都是 `dozer_client::Client` 的实现,连 `diggerd`)、`LinkedTodos` 空实现、`ConversationsCapabilities::default()`(全关);消息 `Msg::Conversations(i64, dozer_panel_conversations::Message)` **带项目 id**(列表结果在项目不聚焦时照样到达)。`adapters/conversations_import.rs` 管历史导入的状态机与「已导入」标记。Agent 筛选下拉复用 S6d-2 的 `adapters/menu_overlay.rs`。S6d-2 里被忽略的 `AgentHostEvent::TurnEnded` 在这里接成列表刷新。

**Tech Stack:** Rust、iced 0.14、`dozer-panel-conversations`(S7a 的 `libs-*` tag)、`dozer-services`、`dozer-client`、byteui、tokio。

**Spec:** `docs/superpowers/specs/2026-10-18-conversations-panel-s7-design.md`(§6.3 登记与布局、数据、历史导入、键盘;§6.4 S7b;§7 Digger 单测;§8 Q1–Q3)。前置:S7a 已合并并切 tag;S6d-1、S6d-2、S6d-3 已合并(提供 `AgentPanel`、`menu_overlay`、`Daemon`、`AgentShell` 状态、覆盖层互斥)。已批准的取舍:Q1 历史导入走明确按钮;Q2 「生成总结」在 Digger v1 隐藏;Q3 右栏 `[agent, conversations, apps, usage]`、默认选中仍是 Agent。

## Global Constraints

- **库不改。** 缺口只能报告,不在 Digger 里绕开库;依赖方向、`check-host-deps.sh`、`check-shared-deps.sh`、Cargo.lock 重复检查同 S6d。
- **不静默读取或复制历史。** 只有用户点「导入此项目的历史会话」才调 `TranscriptService::backfill_project_transcripts(cwd)`;按钮旁必须有一行说明:会读取 `~/.claude/` 等位置的会话文件、把文本存进 `digger.db`。导入只针对当前项目。
- **`ProjectPanel` 里的 `TranscriptService` 占位保持不变**(Project 面板的「修复项目」仍不触发补录);会话面板用真实的服务,两者不混。
- **能力开关全关:** 没有「生成总结」按钮、没有「关联待办」文字;不能出现点了没反应的入口。
- 项目隔离:列表只含 `cwd` 属于当前项目路径的会话;一个项目的列表不得出现在另一个项目下。
- 面板 id `digger.conversations` 是落盘线上名,一经发布不得改。已有用户的 `shell.json` 不被重置;旧文件缺该面板时回默认栏并放在 `digger.agent` 之后,**用户当前选中的面板不变**。
- 失败不静默:守护进程调用失败显示在面板顶部一行红字(可关),同时 `eprintln!`;沿用 Agent 面板的通知条实现,不另造一套。
- 主工作区别人未提交的改动不动;worktree `../digger-s7b`、分支 `feat/conversations-list`(从已含前置的 `main` 切出)。合并前向用户确认。**不发布产品版本**(发布在 S7c 收尾)。
- 与设计的取舍(我的决定,标 D9):设计 §6.3 提到把「已导入」标记记进 `digger.db`。`digger.db` 的表结构由 `diggerd` 的迁移体系管理,Digger 侧再往里加表会与它的版本管理冲突,所以标记改存**独立的小文件** `<config_dir>/conversations-imported.json`(原子写、`schema_version` 字段、读不出来按「未导入」处理)。它只是提示用的提示位,真相永远是「列表是否为空」。

## Review Focus

1. **项目隔离与路径边界:** 父子目录项目(`/a` 与 `/a/b` 同时打开)、末尾斜杠、符号链接、大小写不敏感的卷;A 项目导入不影响 B 项目的列表与标记;项目页签关闭后标记与状态的清理。→ Task 2、4。
2. **导入的安全性与响应:** 连点按钮只触发一次;导入中关闭项目页签或退出;数千个会话文件时界面不卡(在后台线程);读取受限目录失败时部分成功也要显示条数与错误;重复导入不产生重复会话;`HOME` 未设置。→ Task 4。
3. **刷新风暴:** 同一项目短时间内多次 `TurnEnded`、面板不可见时的 `TurnEnded`(不起刷新,首次可见时补一次)、刷新结果乱序到达(旧结果不得覆盖新结果,库的 `RequestTrackers` 负责,这里用测试固定)。→ Task 3。
4. **守护进程不可用与恢复:** 不可用时显示原因而不是「没有会话」;恢复后「重试」重新取数;不可用期间导入按钮不可点并说明原因。→ Task 2、4。
5. **布局迁移与窄宽度:** 旧布局无该面板、右栏为空、已含该面板(幂等)、更高 schema 版本(不回写);配对窄到 `min_first_px` 以下时列表收起,占位块宽度不为负。→ Task 1。

---

### Task 1: 登记、配对规则与旧布局迁移

**Files:**
- Modify:workspace `Cargo.toml` 与 `Cargo.lock`(所有 dozer libs 升到 S7a 的 tag,加 `dozer-panel-conversations`)、`crates/desktop/Cargo.toml`、`crates/desktop/src/shell.rs`、`crates/desktop/src/product.rs`(catalog 登记、`body` 分支、`on_visible`)、`crates/desktop/src/adapters/mod.rs`
- Test:`shell.rs`、`product.rs`

**Interfaces:**
- Produces:`shell::conversations_id() -> PanelId`(`"digger.conversations"`);`meta`:标题「会话」、`IconKind::BotMessageSquare`;`rules`:`default_side: Right`,`pane: Some(PaneRules { spec: PaneSpec::new(false, true), default_split: byteui::theme::geometry::default_split_ratio(), min_first_px: list_min_width() })`(列表是第一块、可拖窄收起;宽度真机再调);`defaults()` 右栏 `[agent, conversations, apps, usage]`,`selected` 右侧仍是 `agent`;`shell::migrate_missing_conversations(model: &mut ShellModel, had_it_in_file: bool)`:旧文件没有该面板时把它移到 `digger.agent` 之后(`agent` 不在右栏时移到右栏最前),**不改任何一侧的选中面板**,已含则什么都不做;用现有的移动消息,不新造。
- [ ] **Step 1: 升依赖。** 把所有 dozer libs 的 `tag` 改为 S7a 的新 tag,加 `dozer-panel-conversations`;跑 Cargo.lock 重复检查与 `check-shared-deps.sh`。
- [ ] **Step 2: 写失败测试:** `conversations_panel_id_is_the_persisted_wire_name`;`the_default_layout_puts_conversations_right_after_agent_and_keeps_agent_selected`;`an_old_file_without_the_panel_places_it_after_agent_and_keeps_the_users_selection`(文件右栏 `["digger.agent","digger.apps","digger.usage"]`、选中 `usage` → `[agent, conversations, apps, usage]`、选中仍是 `usage`);`migration_is_idempotent`;`an_old_file_without_agent_in_the_right_rail_gets_it_at_the_front`;`a_newer_schema_file_is_still_never_written`;`agent_usage_and_conversations_are_the_pair_panels`(更新既有断言);`pair_rects_for_conversations_have_nonnegative_sizes_at_the_minimum_window`。
- [ ] **Step 3: 运行确认失败,实现,验证,提交。** `body` 的 `"digger.conversations"` 分支先返回 Task 2 的视图(此时用空壳占位以便编译)。

### Task 2: `ConversationsPanel`:每项目状态、服务、路由与展示适配

**Files:**
- Create:`crates/desktop/src/adapters/conversations.rs`、`crates/desktop/src/adapters/conversations_display.rs`
- Modify:`crates/desktop/src/product.rs`(`Msg::Conversations`、`boot` 接流、`on_visible`、`Msg::Projects` 之后同步、`body`)、`crates/desktop/src/daemon.rs`(`Daemon::conversation_services()`)
- Test:`conversations.rs`、`conversations_display.rs`、`daemon.rs`

**Interfaces:**
- Produces:
  - `Daemon::conversation_services(&self) -> (Arc<dyn ConversationService>, Arc<dyn SummaryService>, Arc<dyn TranscriptService>)`(同一个 `Client`)。
  - `ConversationsPanel`:`states: HashMap<i64, WorkspaceState>`、`svc`(上面三项)、`handle`、`tx/rx`(`UnboundedSender<(i64, Message)>`)、`notice: Option<String>`、`import: HashMap<i64, ImportState>`(Task 4)、`dirty: HashSet<i64>`(Task 3)。方法 `ensure_state(project) -> &mut WorkspaceState`;`activate(&mut self, project: &ProjectInfo) -> Task<Msg>`(= 库 `on_activate`:首次可见建状态并刷新列表,再次可见也刷新,与 Dozer 一致);`update(&mut self, pid: i64, msg: Message, project: Option<&ProjectInfo>) -> Task<Msg>`(没有 `states[pid]` 的消息忽略;库的 `update` 之后排空 `take_outbox()` 写 `notice` 与 `eprintln!`、`take_requests()` 交 Task 3);`close_project(&mut self, pid: i64)`(清状态、导入态、`dirty`);`view(&self, project: Option<&ProjectInfo>, area: &BodyArea) -> Element<Msg>`。
  - `ConversationsDisplay<'a> { panel: &'a ConversationsPanel, pid: i64, locale: &'a str }` 实现 `PanelDisplay<Msg>`:`wrap` → `Msg::Conversations(pid, m)`;`hover_progress` 恒 0(设计 D3);`panel_head` / `list_collapse_button`(发 `Msg::Shell(ShellMsg::TogglePane(conversations_id))`)/ `tab_divider` / `pane_style`(取与 Dozer `agent_list_pane` / `review_content_pane` 区域等价的度量)仿 `ProjectPanel`;`agent_icon` / `agent_icon_color` 取 byteui 的 `IconKind` 与颜色映射(与 Agent 面板同一份,不另写);`relative_time` 用库的文案键。
  - `NoTodos`:`LinkedTodos` 空实现。
  - 视图:有项目且 `states[pid]` 存在 → `pair_view(area, 列表, 占位, DividerPress(Pair(side)), bg, panel_bg)`;列表块收起时只画占位;占位文案「审阅详情即将可用」(S7c 替换);无项目沿用 Project 面板的空态;守护进程不可用沿用 `AgentShell` 的不可用态与「重试」(共用同一个守护进程状态,不各自再 `ensure`)。
- Consumes:S7a 的库;S6d 的 `Daemon`、`AgentShell` 状态、`BodyArea`/`pair_view`。
- [ ] **Step 1: 写失败测试**(假 `ConversationService`,每条独立):
  - `messages_for_one_project_never_touch_another_projects_list`;`a_message_for_a_project_without_state_is_ignored`;`late_messages_after_close_project_are_ignored`;
  - `the_list_only_contains_sessions_under_the_current_project_path`(假服务按 `cwd` 过滤的调用参数断言:`list_conversations_with_summaries` 收到的 `cwd` 恰是项目路径,`agent == None`、`limit == 500`、`offset == 0`);
  - `parent_and_child_projects_pass_their_own_cwd_and_never_each_others`(`/a` 与 `/a/b`);
  - `first_visibility_creates_the_state_and_refreshes_and_second_visibility_refreshes_again`;
  - `daemon_unavailable_shows_the_reason_not_an_empty_list`、`retry_after_unavailable_refetches`;
  - `service_failures_surface_as_a_dismissable_notice`;
  - `no_summary_button_and_no_linked_todo_text_in_the_view`(能力全关,遍历视图树里的文本);
  - `hover_is_static_and_collapse_button_toggles_the_conversations_pane`;
  - `degenerate_states_build_without_panicking`(无项目、空列表、500 条、超长标题、窄到 0 的块宽)。
- [ ] **Step 2: 运行确认失败,实现。** 后台结果流:`tx/rx` 在 `boot` 接成 iced 流回投 `Msg::Conversations`(`receiver_stream` 同款)。库的 `AsyncResult` 令牌里带项目 id,`resolve_project` 逻辑沿用 `ProjectPanel` 的做法作为兜底。
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop`;沙箱真机起一次,截图:空态、不可用态。
- [ ] **Step 4: 提交。**

### Task 3: Agent 筛选菜单与回合结束联动

**Files:**
- Modify:`crates/desktop/src/adapters/conversations.rs`、`crates/desktop/src/adapters/agent.rs`(`drain_host_events`)、`crates/desktop/src/product.rs`(`Msg::Agent` 之后排空事件)
- Test:`conversations.rs`、`agent.rs`、`product.rs`

**Interfaces:**
- Produces:
  - `ConversationsRequest::ShowAgentFilter { at, current, present }` → `ConversationsPanel.filter_menu = Some(at)`,`stack!` 叠 `menu_overlay`:条目「全部」+ `present` 里的各 Agent(图标与颜色同 Agent 面板;当前选中项用 `gold` 且不可点,与 Dozer 一致);点选发 `AgentFilterSelect(Option<AgentKind>)`,点空白或 Esc 发 `AgentFilterClose`;与选择器/溢出菜单**互斥**(复用 S6d-3 的覆盖层互斥规则,把 `Overlay` 枚举扩一个 `AgentFilter`)。
  - `AgentPanel::drain_host_events(&mut self) -> Vec<(i64, AgentHostEvent)>`(S6d-2 里被忽略的事件改为暂存,由产品排空)。
  - `ConversationsPanel::on_turn_ended(&mut self, pid: i64, project: Option<&ProjectInfo>, visible: bool) -> Task<Msg>`:该项目已有状态且面板可见 → 刷新;有状态但不可见 → `dirty.insert(pid)`(首次可见时刷新并清除);没有状态 → 什么都不做(首次可见本来就会刷新)。`StateChanged` 事件不触发刷新(Dozer 只在回合结束刷新)。
- [ ] **Step 1: 写失败测试:** `filter_menu_lists_all_plus_present_agents_and_locks_the_current_one`;`selecting_an_agent_resets_paging_and_closes_the_menu`;`escape_and_outside_click_close_the_filter_menu_without_changing_the_filter`;`overlays_stay_mutually_exclusive_with_the_agent_picker_and_overflow_menu`;`turn_ended_refreshes_only_the_matching_visible_project`;`turn_ended_while_hidden_marks_dirty_and_the_next_visibility_refreshes_once`;`ten_turn_ended_events_in_a_row_leave_only_the_latest_result_applied`(假服务乱序返回,旧结果不覆盖新结果);`state_changed_alone_does_not_refresh`。
- [ ] **Step 2: 运行确认失败,实现,验证,提交。**

### Task 4: 历史导入

**Files:**
- Create:`crates/desktop/src/adapters/conversations_import.rs`
- Modify:`crates/desktop/src/adapters/conversations.rs`、`crates/desktop/src/product.rs`
- Test:`conversations_import.rs`、`conversations.rs`

**Interfaces:**
- Produces:
  - `ImportState { Idle, Importing, Done(u32), Failed(String) }`;`Msg::ConversationsImport(i64, ImportEvent)`,`ImportEvent { Start, Finished(Result<u32, String>) }`。
  - `ImportMarker`:文件 `<config_dir>/conversations-imported.json`,内容 `{ "schema_version": 1, "projects": { "<project_id>": { "at_ms": u64, "count": u32 } } }`;`ImportMarker::load(path) -> Self`(缺失、损坏、更高版本一律当空,**更高版本不回写**)、`is_marked(project_id) -> bool`、`mark(project_id, count, now_ms)`、`save(path) -> io::Result<()>`(临时文件 + 重命名)。
  - `ConversationsPanel::import_prompt(&self, pid) -> Option<ImportPrompt>`:仅当该项目状态存在、列表已加载且为空、标记未设、守护进程可用时为 `Some`;视图在列表为空时显示说明文案与按钮(`Idle`)、「正在导入…」(`Importing`,按钮不可点)、`Done(n)` 的条数提示、`Failed(msg)` 的错误与「重试」。说明文案固定包含:只显示 Digger 装好 hook 之后产生的会话;导入会读取 `~/.claude/` 等位置的会话文件并把文本存进 `digger.db`。
  - 导入流程:`Start`(仅 `Idle`/`Failed` 时接受,置 `Importing`)→ 后台 `TranscriptService::backfill_project_transcripts(project.path)` → `Finished(Ok(n))`:置 `Done(n)`、`ImportMarker::mark` + `save`、触发列表刷新;`Finished(Err)`:置 `Failed`,**不**写标记。`close_project` 清掉该项目的导入态(后台任务结果到达时找不到状态即丢弃,不 panic)。
- [ ] **Step 1: 写失败测试:** `prompt_appears_only_for_an_empty_loaded_list_without_a_marker_and_with_a_live_daemon`(逐条件取反各一条);`start_while_importing_is_ignored`(连点只调一次 `backfill`);`success_marks_saves_and_refreshes`(假服务返回 7 → 标记文件含该项目、`count == 7`、列表刷新被触发、提示显示 7);`failure_does_not_mark_and_allows_retry`;`zero_imported_still_marks_and_shows_that_nothing_was_found`;`closing_the_project_during_import_drops_the_result_quietly`;`marker_for_project_a_does_not_affect_project_b`;`marker_file_corrupt_missing_or_newer_is_treated_as_empty_and_newer_is_never_written`;`marker_save_is_atomic`(写入中途失败不留下半个文件);`non_empty_list_never_shows_the_prompt_even_without_a_marker`;`disclosure_text_is_always_shown_with_the_button`;`import_runs_off_the_ui_thread`(用一个阻塞 200ms 的假服务,`update` 本身在 50ms 内返回)。
- [ ] **Step 2: 运行确认失败,实现。**
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop`;沙箱真机:临时 `HOME` 下放 3 个伪造的 Claude transcript(位于该项目对应的 `~/.claude/projects/<编码后的路径>/`),点按钮前列表为空,点后出现 3 条,第二次打开面板不再提示;截图留证。
- [ ] **Step 4: 提交。**

### Task 5: 门禁、真机回归与交付

- [ ] **Step 1: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`check-host-deps.sh`(含 `--self-test`)、`check-shared-deps.sh`、`check-shell-regions.sh`、Cargo.lock 重复检查。
- [ ] **Step 2: 真机回归**(沙箱 `DIGGER_DATA_DIR` 与临时 `HOME`;点击由用户做,我启动并截图):① 全新数据根开项目,会话面板空态说明正确、按钮在;② 在 Agent 面板里跑一次真实 Claude 会话到回合结束,会话面板**无需切换**自动出现这条会话;③ 点导入,历史出现,标记生效,重启后不再提示;④ 开两个项目,列表互不串;`/a` 与 `/a/b` 嵌套项目各自正确;⑤ 搜索、Agent 筛选(菜单位置、当前项锁定)、「更多…」分页、折叠列表;⑥ `diggerd` 改名后启动:显示原因,恢复后「重试」可用,导入按钮此时不可点;⑦ 旧 `shell.json`(无该面板)启动后顺序与选中面板符合预期;⑧ 与 Dozer 同机并存,各自只看到自己库里的会话。逐项记录,任一项不符先停下。
- [ ] **Step 3: 记录已知缺口** 到 `docs/superpowers/notes/2026-10-18-conversations-s7b.md`:Digger 里删除项目不会清理该项目的会话(`TranscriptService::delete_project_transcripts` 未接,Project 面板仍用占位),留给用户裁决是否在 S7c 或单独处理;列表宽度取值;悬停为阶跃。
- [ ] **Step 4: 报告** 写明:各项真机结果与截图路径、依赖升级到的 tag、哪些依赖人工、与计划不符之处。合并到 `main` 前向用户确认。**不发布。**

---

## Self-Review

- **Spec 覆盖:** §6.3 登记与布局、数据来源、Agent 筛选、键盘(搜索框被 `text_input` 捕获,无需新代码,Task 2 测试覆盖「按键不进终端」由 Agent 面板的既有测试保证)→ Task 1–3;`TurnEnded` 联动 → Task 3;历史导入(Q1)→ Task 4;能力全关(Q2)→ Task 2 测试;右栏顺序(Q3)→ Task 1;§7 Digger 单测与真机 → Task 2–5。**不在本期:** 审阅详情 webview、「加载更多」、详情与列表联动、与 Dozer 的逐状态对照、产品发布(S7c)。
- **类型一致:** `ConversationsPanel`、`ConversationsDisplay`、`NoTodos` 在 Task 2 定义,Task 3、4 增方法/字段(`filter_menu`、`import`、`dirty` 已在 Task 2 的字段表里预留);`ImportState`、`ImportEvent`、`ImportMarker` 在 Task 4 定义;`Overlay::AgentFilter` 在 Task 3 追加到 S6d-3 的枚举;`drain_host_events` 在 Task 3 追加到 S6d-2 的 `AgentPanel`。
- **风险点:** ① 库 `Message` / `ConversationsRequest` 的最终变体名以 S7a 合并结果为准(`AgentFilterSelect`、`AgentFilterClose`、`ShowAgentFilter`);② 项目 `cwd` 过滤的边界(父子目录、符号链接、大小写)由 `diggerd` 一侧的查询决定,Digger 只传项目路径,Task 2 的测试只能固定「传了什么」,真正的匹配语义要在真机上验证,若发现父目录项目会看到子目录项目的会话,作为 Dozer 同样存在的行为报告,不在 Digger 里过滤;③ 标记文件独立于 `digger.db` 是我的取舍(D9),代价是两处状态(标记与库内容)可能不一致,所以标记只做提示;④ Digger 删除项目不清会话是已知缺口,先记账;⑤ 悬停阶跃、列表最小宽度同 S6d 的已知差异;⑥ 合并是对外动作,先确认;⑦ 本期不发布产品版本。
