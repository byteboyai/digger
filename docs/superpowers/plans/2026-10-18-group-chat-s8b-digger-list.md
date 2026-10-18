# S8b Digger 接入群聊面板:群列表、新建群、每项目状态与轮询调度 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Digger 右栏出现群聊面板(Agent 之后、会话之前),配对布局(右栏约定「内容 | 群列表」):群列表可建群、选群、加载;新建群弹窗可勾选成员 Agent;内容块先放一个原生的「聊天内容即将可用」占位,并显示当前选中群的主题与成员句柄,用来验证选群与状态(真正的聊天页是 S8c)。数据来自 `diggerd` / `digger.db`;每个项目一份状态;轮询只在需要时、面板可见时运行。本期**不发布产品版本**。

**Architecture:** 全部在 `digger-desktop`。`adapters/group_chat.rs` 的 `GroupChatPanel` 持有 `HashMap<i64, WorkspaceState>`(库的状态)、一个 `PanelIo<Message>`(由新的 `Daemon::services()` 提供的完整 `Services` 构造,`group_chat` 等是 `diggerd` 的真实服务,其余沿用 `UnavailableService` 占位)、`GroupChatCapabilities::default()`(`push_to_todo` 关)。消息 `Msg::GroupChat(dozer_panel_group_chat::Message)`——库的 `Message` 第一个字段本来就是项目 id,据此路由,不再另包一层。轮询由一个仅在 `poll_wanted` 时才存在的 500ms 订阅驱动;面板切入时 `on_activate` 标记过期并立即补一次。新建群弹窗是 `stack!` 叠放的模态卡片(库的 `dialog::new_group_dialog_card`),Esc 或「取消」关闭,点背景**不**关闭(保留草稿,与 Dozer 一致)。三个面板(Agent、会话、群聊)现在都各有一条通知条,本期把它抽成共用的 `adapters/notice.rs`。

**Tech Stack:** Rust、iced 0.14、`dozer-panel-group-chat`(S8a 的 `libs-*` tag)、`dozer-panel-host`、`dozer-services`、`dozer-client`、byteui、tokio。

**Spec:** `docs/superpowers/specs/2026-10-18-group-chat-panel-s8-design.md`(§6.3 登记与布局、数据、轮询、键盘;§6.4 S8b;§7 Digger 单测;§8 D1、D3、D5、Q1–Q2)。前置:S8a 已合并并切 tag;S6d 系列、S7 系列已合并(`AgentPanel`、`menu_overlay`、`Overlay` 枚举、`AgentShell` 状态、`ConversationsPanel`)。已批准的取舍:Q1 成员 Agent 在项目目录里工作、不加额外首次确认;Q2 「转待办」隐藏;右栏 `[agent, group_chat, conversations, apps, usage]`。

## Global Constraints

- **库不改。** 缺口只能报告;依赖方向、`check-host-deps.sh`、`check-shared-deps.sh`、Cargo.lock 重复检查同前。
- **建群、加成员、选群、删群本身绝不拉起任何 Agent 命令行。** 只有 `post_group_message` / `retry_group_message` 会让后台调用成员 Agent;本期没有发言入口,所以 S8b 期间 Digger 不应出现任何由群聊产生的 Agent 进程(真机用进程列表核对)。
- **能力:** `GroupChatCapabilities::default()`(全关);内容块与群列表里没有「转待办」相关文字或入口。
- **轮询纪律:** 面板不可见不轮询;没有 `load_pending` 且没有进行中的发言时不订阅 500ms 定时器;每次只轮询当前选中群;同一项目同时最多一个在途请求;`diggerd` 出错按库的 5 秒退避;不得因多个项目同时打开而成倍轮询(只轮询聚焦项目)。
- 项目隔离:一个项目的群不出现在另一个项目下;项目页签关闭后其状态、草稿、定时器、在途请求结果全部丢弃。
- 面板 id `digger.group_chat` 是落盘线上名,一经发布不得改;已有用户的 `shell.json` 不被重置;旧文件缺该面板时回默认栏并放在 `digger.agent` 之后(因此在 `digger.conversations` 之前),**用户当前选中的面板不变**。
- 失败不静默:守护进程调用失败显示在面板顶部通知条(可关),同时 `eprintln!`。
- 主工作区别人未提交的改动不动;worktree `../digger-s8b`、分支 `feat/group-chat-list`(从已含前置的 `main` 切出)。合并前向用户确认。**不发布产品版本**(发布在 S8c 收尾)。

## Review Focus

1. **项目隔离与迟到结果:** 项目 A 的 `Polled` / `GroupsLoaded` / `GroupCreated` 不改项目 B;状态不存在或项目已关闭时的结果被忽略;切项目页签时旧项目的定时器不再对新项目发请求。→ Task 2。
2. **轮询的开关与节流:** 隐藏时不订阅、隐藏时到达的 tick 被丢弃、可见后立即补一次、在途期间 tick 不重复发请求、`diggerd` 恢复后退避结束再继续、`has_active_turn` 为假后订阅关闭。→ Task 3。
3. **新建群弹窗:** 多行主题(含中文输入法)、空主题忽略、连点「创建」只建一个群、失败时弹窗与草稿保留、成功后关闭并自动选中、取消清已勾选成员、Esc 关闭、弹窗打开时键盘不进终端、点背景不关闭。→ Task 4。
4. **「不拉起 Agent」:** 建群 + 勾选 3 个成员 + 选群 + 反复切换项目,假服务记录里没有 `post_group_message` / `retry_group_message`;真机无 `claude` / `codex` 等进程由此产生。→ Task 4、5。
5. **布局迁移与窄宽度:** 旧布局无该面板、右栏为空、已含(幂等)、更高 schema 版本(不回写);只有 `agent` 没有 `conversations` 的旧布局;配对窄到 `min_first_px` 以下列表收起。→ Task 1。

---

### Task 1: 登记、配对规则与旧布局迁移

**Files:**
- Modify:workspace `Cargo.toml` 与 `Cargo.lock`(所有 dozer libs 升到 S8a 的 tag,加 `dozer-panel-group-chat`)、`crates/desktop/Cargo.toml`、`crates/desktop/src/shell.rs`、`crates/desktop/src/product.rs`(catalog 登记、`body` 分支、`on_visible`)、`crates/desktop/src/adapters/mod.rs`
- Test:`shell.rs`、`product.rs`

**Interfaces:**
- Produces:`shell::group_chat_id() -> PanelId`(`"digger.group_chat"`);`meta`:标题「群聊」、`IconKind::SquareSparkles`;`rules`:`default_side: Right`、`pane: Some(PaneRules { spec: PaneSpec::new(false, true), default_split: byteui::theme::geometry::default_split_ratio(), min_first_px: list_min_width() })`;`defaults()` 右栏 `[agent, group_chat, conversations, apps, usage]`,`selected` 右侧仍是 `agent`;`shell::migrate_missing_group_chat(model: &mut ShellModel, had_it_in_file: bool)`:旧文件没有该面板时把它移到 `digger.agent` 之后(`agent` 不在右栏时移到右栏最前),**不改任何一侧的选中面板**,已含则什么都不做;用现有的移动消息。
- [ ] **Step 1: 升依赖。** 所有 dozer libs 的 `tag` 改为 S8a 的新 tag,加 `dozer-panel-group-chat`;跑 Cargo.lock 重复检查与 `check-shared-deps.sh`。
- [ ] **Step 2: 写失败测试:** `group_chat_panel_id_is_the_persisted_wire_name`;`the_default_layout_orders_agent_group_chat_conversations_apps_usage_and_keeps_agent_selected`;`an_old_file_without_the_panel_places_it_right_after_agent_and_keeps_the_users_selection`(文件右栏 `["digger.agent","digger.conversations","digger.apps","digger.usage"]`、选中 `conversations` → 迁移后 `[agent, group_chat, conversations, apps, usage]`、选中仍是 `conversations`);`an_old_file_with_only_agent_and_apps_works`;`migration_is_idempotent`;`an_old_file_with_an_empty_right_rail_gets_it_at_the_front`;`a_newer_schema_file_is_still_never_written`;`the_pair_panels_are_agent_group_chat_conversations_and_usage`(更新既有断言);`pair_rects_for_group_chat_have_nonnegative_sizes_at_the_minimum_window`。
- [ ] **Step 3: 运行确认失败,实现,验证,提交。** `body` 的 `"digger.group_chat"` 分支先返回 Task 2 的视图(此时用空壳占位以便编译)。

### Task 2: `GroupChatPanel`:服务、每项目状态、路由与展示适配

**Files:**
- Create:`crates/desktop/src/adapters/group_chat.rs`、`crates/desktop/src/adapters/group_chat_display.rs`、`crates/desktop/src/adapters/notice.rs`
- Modify:`crates/desktop/src/daemon.rs`(`Daemon::services()`)、`crates/desktop/src/adapters/{agent,conversations}.rs`(改用共用的 `NoticeBar`)、`crates/desktop/src/product.rs`(`Msg::GroupChat`、`boot` 接流、`on_visible`、`Msg::Projects` 之后同步、`body`)
- Test:`group_chat.rs`、`group_chat_display.rs`、`notice.rs`、`daemon.rs`

**Interfaces:**
- Produces:
  - `Daemon::services(&self) -> dozer_services::Services`:同一个 `Client` 适配出的完整服务束——`session`、`conversation`、`summary`、`transcript`、`group_chat` 为真实服务,其余(`usage`、`todo`、`project`、`memory`、`codehealth`、`context`)用 `UnavailableService` 占位并写明理由。
  - `notice::NoticeBar { text: Option<String> }`:`push(&mut self, text)`(同时 `eprintln!`)、`dismiss()`、`view<M: Clone>(&self, on_dismiss: M) -> Option<Element<M>>`(红字一行,可点关闭);`AgentPanel`、`ConversationsPanel`、`GroupChatPanel` 各持有一个(行为与之前的各自实现逐项相同,既有测试原样通过)。
  - `GroupChatPanel`:`states: HashMap<i64, WorkspaceState>`、`io: PanelIo<Message>`(`emit` 写入 `UnboundedSender<Message>`,`boot` 接成 iced 流回投 `Msg::GroupChat`)、`caps`、`notice: NoticeBar`、`focus_project: Option<i64>`。方法 `ensure_state(&mut self, project_id) -> &mut WorkspaceState`;`activate(&mut self, project: &ProjectInfo, now: Instant) -> Task<Msg>`(= 建状态、库 `on_activate`(标过期)、立即 `poll_if_due` 一次);`update(&mut self, msg: Message, project_open: impl Fn(i64) -> bool) -> Task<Msg>`(`msg.project_id()` 不存在状态或项目已关闭 → 忽略;库 `update` 返回的每个 `Effect` 经 `run_effect` 执行;随后排空 `take_outbox()` 到 `notice`);`close_project(&mut self, pid)`;`poll_wanted(&self, panel_visible: bool) -> bool`(聚焦项目的库 `host::poll_wanted`);`tick(&mut self, now: Instant, panel_visible: bool)`;`view(&self, project: Option<&ProjectInfo>, area: &BodyArea) -> Element<Msg>`。
  - `GroupChatDisplay<'a>` 实现 `PanelDisplay<Msg>`:`wrap` → `Msg::GroupChat`;`panel_head` / `list_collapse_button`(发 `Msg::Shell(ShellMsg::TogglePane(group_chat_id))`)/ `pane_style`(取与 Dozer `group_chat_list_pane` 区域等价的度量)仿 `ProjectPanel`;`agent_icon` / `agent_icon_color` 取与 Agent 面板同一份 byteui 映射;悬停无动画(设计 D3)。
  - 内容块占位:`选中群 → 主题(粗体)+ 成员句柄列表(每行 "@handle · 品牌名")`;未选中 → 「选择一个群聊,或点击＋新建」;并固定一行 dim 文案「聊天内容即将可用」(S8c 删除)。
- Consumes:S8a 的库;S6d 的 `Daemon`、`AgentShell` 状态、`BodyArea` / `pair_view`;S7b 的 `ConversationsPanel`。
- [ ] **Step 1: 写失败测试**(假 `GroupChatService`,每条独立):
  - `messages_for_one_project_never_touch_another_projects_groups`;`a_message_for_a_project_without_state_is_ignored`;`late_messages_after_close_project_are_ignored`;`the_services_bundle_routes_group_chat_to_the_daemon_and_the_rest_to_placeholders`;
  - `notice_bar_behaves_like_the_two_previous_implementations`(推入、替换、关闭、`eprintln!` 调用一次;三个面板共用同一类型);
  - `first_visibility_creates_the_state_marks_stale_and_loads_the_groups_once`;`second_visibility_reloads_without_flicker`(已有群列表保持显示,不闪成空态);
  - `daemon_unavailable_shows_the_reason_and_retry_reloads`;`service_failures_surface_as_a_dismissable_notice`;
  - `selecting_a_group_fetches_its_messages_from_rev_zero_and_shows_the_placeholder_summary`;
  - `no_todo_text_anywhere_in_the_view`(遍历视图树文本);
  - `hover_is_static_and_collapse_button_toggles_the_group_chat_pane`;
  - `degenerate_states_build_without_panicking`(无项目、0 个群、500 个群、超长主题、窄到 0 的块宽)。
- [ ] **Step 2: 运行确认失败,实现,验证,提交。**

### Task 3: 轮询订阅与节流

**Files:**
- Modify:`crates/desktop/src/product.rs`(`subscription`、`Msg::GroupChatTick`、可见性与项目切换的同步)、`crates/desktop/src/adapters/group_chat.rs`
- Test:`group_chat.rs`、`product.rs`

**Interfaces:**
- Produces:`Msg::GroupChatTick`;`product::subscription` 在 `shell.group_chat.poll_wanted(shell.panel_visible(&group_chat_id())) && !shell.closing` 时追加 `iced::time::every(Duration::from_millis(500)).map(|_| Msg::GroupChatTick)`,否则不订阅;`GroupChatPanel::tick(now, panel_visible)` 对聚焦项目调库的 `poll_if_due`(限速、在途、退避都在库的状态里)。聚焦项目 = 当前页签项目;项目切换后旧项目不再被轮询,新项目的 `activate` 立即补一次。
- [ ] **Step 1: 写失败测试:** `no_subscription_when_the_panel_is_hidden_even_with_an_active_turn`;`no_subscription_when_nothing_is_loading_and_no_turn_is_active`;`subscription_exists_while_loading_or_a_turn_is_active_and_visible`;`a_tick_while_hidden_does_nothing_and_does_not_panic`;`becoming_visible_polls_once_immediately`;`ticks_do_not_stack_requests_while_one_is_in_flight`(假服务延迟返回,期间连发 10 个 tick,只见 1 次 `list_group_messages`);`two_projects_with_active_turns_only_the_focused_one_is_polled`;`switching_projects_stops_the_old_and_activates_the_new`;`daemon_errors_back_off_for_five_seconds_then_resume`(注入时钟);`subscription_stops_after_the_last_active_message_finishes`;`closing_the_window_drops_the_subscription`。
- [ ] **Step 2: 运行确认失败,实现,验证,提交。** 沙箱真机观察:发言进行中(用假数据或 S8c 之后)订阅开启,空闲时 CPU 与帧率回落;本期用假服务的画廊式自检代替(见 Step 3)。
- [ ] **Step 3: 自检夹具。** `crates/desktop/examples/group_chat_gallery.rs`:不连守护进程,用假 `GroupChatService` 预置 2 个群、含 Running 消息(模拟进行中的发言),启动一个最小窗口摆出面板,用来肉眼确认订阅开关与列表/占位渲染(S8c 复用并扩展)。

### Task 4: 新建群弹窗(模态)与覆盖层互斥

**Files:**
- Modify:`crates/desktop/src/adapters/group_chat.rs`、`crates/desktop/src/adapters/agent.rs`(`Overlay` 枚举加 `GroupChatCreate`、`on_key` 门禁)、`crates/desktop/src/product.rs`(叠放、Esc、`overlays_open`)
- Test:`group_chat.rs`、`agent.rs`、`product.rs`

**Interfaces:**
- Produces:`ProductShell::group_create_overlay(&self) -> Option<Element<Msg>>`:当聚焦项目的库状态 `new_group_dialog_open()` 为真时,`stack!` 叠放「半透明背景 + 居中卡片(宽 420,同 Dozer `CARD_WIDTH`)」,卡片内容取库的 `dialog::new_group_dialog_card`,消息经 `GroupChatDisplay::wrap`;背景只吞点击、**不**触发关闭;Esc(订阅里 `Ignored` 的 `KeyPressed(Escape)`)发 `NewGroupDialogCancel(project_id)`。`Overlay::GroupChatCreate` 加入 S6d-3 / S7b 的覆盖层互斥枚举(打开它先关掉其它覆盖层;其它覆盖层打开时点「＋」无效或先关其它,以枚举规则为准);`overlays_open()` 把它算作打开(S7c / S8c 的 webview 据此隐藏)。弹窗打开时 `AgentPanel::on_key` 丢弃按键(模态)。
- [ ] **Step 1: 写失败测试:** `plus_button_opens_the_dialog_and_cancel_closes_and_clears_selected_agents`;`escape_cancels_but_clicking_the_backdrop_does_not`;`empty_topic_is_ignored_on_submit_and_the_dialog_stays_open`;`submit_creates_one_group_even_if_clicked_five_times_quickly`;`success_closes_the_dialog_selects_the_new_group_and_adds_the_checked_members`(假服务记录 `create_group` 一次、`add_group_member` 按勾选各一次,句柄默认取品牌名、角色提示为空);`failure_keeps_the_dialog_and_the_topic_and_shows_a_notice`;`dialog_lists_exactly_the_creatable_agents`(Claude / Codex / Codebuddy / Opencode / Qodercn);`creating_a_group_and_adding_members_never_calls_post_or_retry`(Review Focus 4);`dialog_is_modal_for_the_terminal_keyboard`;`opening_the_dialog_closes_the_agent_picker_and_vice_versa`;`switching_project_with_the_dialog_open_closes_it_for_the_old_project_only`(旧项目草稿保留,回到旧项目时按库的语义恢复或清空,以库的行为为准并写成测试);`multi_line_topic_accepts_newlines`。
- [ ] **Step 2: 运行确认失败,实现。**
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop`;沙箱真机:建群、勾选成员、用**拼音输入法**输入中文主题(iced `text_editor` 的输入法由控件自身处理,这是人工验证项),截图。
- [ ] **Step 4: 提交。**

### Task 5: 门禁、真机回归与交付

- [ ] **Step 1: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`check-host-deps.sh`(含 `--self-test`)、`check-shared-deps.sh`、`check-shell-regions.sh`、Cargo.lock 重复检查。
- [ ] **Step 2: 真机回归**(沙箱 `DIGGER_DATA_DIR` 与临时 `HOME`;点击与输入法由用户做,我启动并截图):① 全新数据根开项目,群聊面板空态与「＋」;② 建群(中文主题)并勾选 2–3 个成员,群出现在列表且自动选中,内容块占位显示主题与成员句柄;③ 再建一个群并来回选择;④ 开第二个项目,群互不串;⑤ 重启 Digger,群与成员仍在(来自 `digger.db`);⑥ `diggerd` 改名后启动:显示原因,恢复后「重试」可用;⑦ 旧 `shell.json`(无该面板、有 `conversations`)启动后顺序为 `agent, group_chat, conversations…` 且选中面板不变;⑧ 与 Dozer 同机并存,各自只看到自己库里的群;⑨ **全程用进程列表确认没有任何 `claude` / `codex` / `codebuddy` / `opencode` / `qodercn` 进程因群聊而出现**;⑩ 面板隐藏时 CPU 空闲,可见且空闲时无 500ms 定时器(用 Instruments 或日志计数)。逐项记录,任一项不符先停下。
- [ ] **Step 3: 记录已知缺口** 到 `docs/superpowers/notes/2026-10-18-group-chat-s8b.md`:内容块是临时占位;删除群的入口在 S8c 的聊天页内;「转待办」隐藏;悬停为阶跃;列表最小宽度同 S6d / S7 的待调项。
- [ ] **Step 4: 报告** 写明:各项真机结果与截图路径、依赖升级到的 tag、哪些依赖人工、与计划不符之处。合并到 `main` 前向用户确认。**不发布。**

---

## Self-Review

- **Spec 覆盖:** §6.3 登记与布局、数据来源 → Task 1、2;轮询(500ms 仅在需要时、只轮询选中群、隐藏不轮询、切回补取)→ Task 3;「转待办」隐藏 → Task 2 测试;键盘(新建群主题框被 `text_editor` 捕获,模态时终端不收键)→ Task 4;§7 Digger 单测与真机 → Task 2–5;D1、D3、D5 → Task 1、3。**不在本期:** 聊天内容 webview、发言 / 取消 / 重试 / 成员管理 / 删群(都在聊天页里,S8c)、`native_host.rs`、与 Dozer 对照、产品发布。
- **类型一致:** `GroupChatPanel` / `GroupChatDisplay` 在 Task 2 定义,Task 3、4 增方法(`tick`、`group_create_overlay`);`NoticeBar` 在 Task 2 定义并替换前两个面板的实现;`Overlay::GroupChatCreate` 在 Task 4 追加到 S6d-3 / S7b 的枚举;`Daemon::services` 在 Task 2 定义,S8c 的聊天页复用。
- **风险点:** ① 库 `Message` / `Effect` / `host::*` 的最终名字以 S8a 合并结果为准(尤其 `NewGroupDialogCancel`、`poll_wanted`、`dialog::new_group_dialog_card`);② `Services` 结构的字段集合以 S6c / S7a 合并后的实际为准,`Daemon::services` 里漏掉新字段会编译失败,正好提醒补齐;③ 中文输入法在 `text_editor` 里是否可用只能真机验证(与终端的 `ImeSink` 是两条路径);④ 抽 `NoticeBar` 会动 S6d / S7b 已写好的代码,以「既有测试原样通过」为界;⑤ 弹窗 Esc 依赖 `text_editor` 不捕获 Escape,若实际被捕获,退路是在弹窗卡片外层包一个只吞 Esc 的键盘监听,先停下报告;⑥ 本期没有发言入口,订阅开关只能用假服务夹具验证,真正的流式轮询要到 S8c 才能端到端验证;⑦ 合并是对外动作,先确认;⑧ 本期不发布产品版本。
