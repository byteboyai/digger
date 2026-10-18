# S7a 会话面板抽成共享库 `dozer-panel-conversations` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Dozer 里新增服务接缝(`ConversationService::get_conversation_turns`、`SummaryService`),把会话面板的**列表、详情审阅状态与逻辑、总结生成、`review-trace` 页面资源与纯函数协议应答**抽成共享库 `dozer-panel-conversations`;Dozer 改为薄调用,**用户可见行为不变**;合并后切 `libs-*` tag,Digger 同日升级钉版。**本期只动 Dozer(加 Digger 的钉版一步),不发布产品版本**(发布在 S7c 收尾)。

**Architecture:** 先在现状上写三份黄金测试(审阅数据 JSON、协议编码、`review-trace` 应答),再加服务接缝,再搬代码。库按扩展式面板的形状(`WorkspaceState` / `Message` / `update` / `view`,同 `dozer-panel-todo`)拥有:列表状态、`ReviewView` + `review_nonce` + 给协议用的 `review_snapshot`(`Arc<Mutex<Option<String>>>`)、`SessionOpen` / `DetailLoadMore` / `ReviewLoaded` / 总结生成的全部更新逻辑(原先被 `App::update` 拦截的那几条落进库)、详情的 iced 兜底视图、`serve_review_trace` / `parse_review_trace_event`。产品注入:`PanelDisplay`、`ConversationsCapabilities { summaries, linked_todos }`、`LinkedTodos`;宿主动作(Agent 筛选下拉)经 `ConversationsRequest` 发件箱交给产品。webview 的创建与摆位仍在 `dozer-app`(库不含 `wry`)。

**Tech Stack:** Rust、iced 0.14、byteui、`dozer-model` / `dozer-services` / `dozer-i18n` / `dozer-panel-host`、tokio、serde;`web/review-trace` 的 Preact 前端(npm 构建,产物提交)。

**Spec:** `docs/superpowers/specs/2026-10-18-conversations-panel-s7-design.md`(§2、§3 G1–G2、§6.1、§6.2、§6.4 S7a、§7)。模板:S5a / S6c-1 的 Dozer 段(worktree、黄金测试、门禁做法)。

## Global Constraints

- **依赖闭包:** `cargo tree -p dozer-panel-conversations` 不得出现 `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`、`wry`、`winit`。
- **Dozer 用户可见行为不变;** 搬走的测试随代码走、不改期望值、抽库前后 Rust 测试总数守恒。**唯一例外**:专为已删除的 `ReviewSource::Session` 死分支存在的测试(`workspace/tests.rs` 里对 `Session(..)` 的断言约 4 条,以 `grep` 实测为准)可以删除,须在提交说明里逐条列出,总数核对按「基线 − 已删 + 新增」算。
- **审阅数据 JSON 字节不变:** `ReviewSnapshot` 序列化结果、`ReviewEntry` 的外部标签形状、`host.html` / `review-trace.js` / `review-trace.css` 应答内容与改造前逐字节一致(黄金测试)。**严格 CSP 不放宽**,页面不得出现外部引用。
- **协议编码不变:** `Request::{GetConversationTurns, SubmitSummaryJob, GetSummaryJob}` 与对应应答的 JSON 与改造前逐字一致(黄金测试)。
- **「加载更多」语义不变:** `append == true` 必须累加不能替换(2026-08-27 的真实 bug,已有测试,随代码走);过期的 `ReviewLoaded`(`source` 已变)被丢弃、不污染快照。
- 只用 git tag 依赖;Dozer 内部 `path = "../.."` 除外。**合并、切 tag、推送是对外动作,向用户确认后执行。** 不发布产品版本。
- **主工作区别人未提交的改动不动;** 实现在 worktree `../dozer-panel-conversations`、分支 `feat/panel-conversations-lib`(从 `main` 切出,须已含 S6c 全部与 S5 系列)。
- 不引入新的日志依赖;库内日志用 `tracing` 薄封装(同 `dozer-panel-project` 的 `log` 模块)。
- 与设计的订正(我的决定,标 D8):设计初稿写「`TurnRecord` 要搬家」「总结走 `submit_summary_batch`」,读代码后核实**类型已在 `dozer-model`、总结用的是 `submit_summary_job` / `get_summary_job`**,所以接缝是 `ConversationService::get_conversation_turns` 加 `SummaryService`(设计稿已订正)。

## Review Focus

1. **审阅数据的折叠规则:** 连续只有 tool_use 的 `ai` 行折叠进同一个 `AiTurn`、孤儿 `ToolResult`、空回合列表、巨大回合(上万条、单条数 MB)、含非 UTF-8 替换符与零宽字符的文本;JSON 序列化后字节与改造前一致。→ Task 1、2。
2. **加载竞态:** 连续点两行(先点 A 再点 B,A 的结果后到)只显示 B;「加载更多」进行中又切行;列表刷新期间点行(行已不在列表)不 panic;`nonce` 回绕。→ Task 3。
3. **总结生成的终态:** 提交失败、`Failed`(有/无 `error_detail`)、`Cancelled`、任务不存在、轮询出错、成功后刷新失败;生成中重复点击不重复提交;生成期间项目关闭/切走;结果到达时详情已换成别的会话。→ Task 3。
4. **webview 状态机:** 未 `DocumentLoaded` 前离屏停放;出错或条目为空时销毁而不是永远「加载中」;`nonce` 变化触发重新导航;`loaded_nonce` 不等于当前 `nonce` 时回到未加载。→ Task 2、3。
5. **列表边界:** 搜索词含大小写与 Unicode、空列表、500 条上限、分页越界、agent 筛选后当前页回 0;「● 当前」标记只会有一个。→ Task 2。

---

## Dozer(仓库 `../dozer`,worktree `../dozer-panel-conversations`,分支 `feat/panel-conversations-lib`)

### Task 1: 基线、黄金测试与服务接缝

**Files:**
- Modify:`crates/dozer-services/src/lib.rs`(`ConversationService::get_conversation_turns`、新 `SummaryService`、`Services.summary`)、`crates/dozer-client/src/lib.rs`(两个 trait 的 `impl for Client`,`map_err(into_service_error)`)、`crates/dozer-app/src/service_fakes.rs`、`crates/dozer-app/src/host_adapters/services.rs`(构造处补 `summary`)、`crates/dozer-panel-project/src/fakes.rs`(及其它实现 `ConversationService` 的 fake)
- Create:`crates/dozer-app/tests/fixtures/review_snapshot_golden.json`、`crates/dozer-app/tests/fixtures/review_trace_serve_golden.txt`
- Test:`crates/dozer-app/src/transcript.rs`、`crates/dozer-app/src/assets.rs`、`crates/dozer-client/src/lib.rs`、`crates/dozer-services/src/lib.rs`

**Interfaces:**
- Produces:
  - `ConversationService::get_conversation_turns(&self, conversation_id: String, after_turn_index: i64, limit: u32) -> BoxFuture<'_, Result<Vec<TurnRecord>, ServiceError>>`。
  - `dozer_services::SummaryService: Send + Sync`:`submit_summary_job(&self, conversation_id: String, force: bool) -> BoxFuture<'_, Result<i64, ServiceError>>`(内部固定 `source_session_id = None`、`trigger = Manual`、`provider = None`、`model = None`,与现有调用逐项一致)、`get_summary_job(&self, job_id: i64) -> BoxFuture<'_, Result<Option<SummaryJobInfo>, ServiceError>>`;`Services` 增加 `pub summary: Arc<dyn SummaryService>`。
  - 测试替身 `FakeSummary`(可预置任务状态序列与各方法失败)与 `FakeConversation` 的 `turns` 预置,放 `service_fakes.rs`,后面各任务复用。
- [ ] **Step 1: 记录基线。** worktree 里 `cargo test --workspace 2>&1 | grep "^test result"`;再记要搬走的测试清单与数量:`extensions/conversations.rs` 的 `#[cfg(test)]` 全部、`transcript.rs` 里 `review_*` / `merge_*` 相关、`app/update.rs` 里 `refresh_open_conversation_summary` / `mark_review_loaded` 相关、`workspace/tests.rs` 里 `review_*`、`workspace/view.rs` 的 `review_pane_state_tests`、`assets.rs` 里 `review_trace_*`、`preview/webview_protocol.rs` 里 `parse_review_trace_event_*`。写进提交说明。
- [ ] **Step 2: 在改任何代码之前写三份黄金测试(现状上生成一次并提交,之后字节不得变)。**
  - `review_snapshot_matches_the_golden`:一组覆盖折叠规则的 `TurnRecord`(人类发言、只有 tool_use 的连续 `ai` 行后跟带正文的 `ai` 行、孤儿 `tool_result`、带 thinking 与 token 用量、含中文/emoji/换行)→ `review_entries_from_turns` → 用 `ReviewSnapshot { entries, agent_label, summary_* }` 序列化 → 与 `review_snapshot_golden.json` 逐字比较(`UPDATE_GOLDEN=1` 仅此一次)。
  - `review_trace_serve_matches_the_golden`:对 `host.html`、`review-trace.js`、`review-trace.css`、带数据的 `data.json` 调现有的服务函数,记录 `(mime, 字节数, sha256)` 与 `data.json` 全文,和 `review_trace_serve_golden.txt` 比较。
  - `conversation_protocol_encoding_golden`(`dozer-client`):`Request::{GetConversationTurns, SubmitSummaryJob, GetSummaryJob}` 与 `Reply::{ConversationTurns, SummaryJob}` 各取一个样例,`encode_line` 的输出与写死的字符串逐字比较。
- [ ] **Step 3: 写失败测试:** `client_implements_get_conversation_turns_end_to_end`、`client_implements_summary_service_end_to_end`(假 Unix socket 服务端,断言收到的 `Request` 字段:`SubmitSummaryJob` 的 `trigger == Manual`、`force` 透传、`provider`/`model` 为 `None`;错误经 `into_service_error`)、`mem_conversation_returns_turns_in_order_and_respects_after_turn_index_and_limit`。
- [ ] **Step 4: 运行确认失败,实现,验证。** `cargo test --workspace` 总数 = 基线 + 新增;三份黄金通过。
- [ ] **Step 5: 提交** `feat(services): get_conversation_turns 与 SummaryService 接缝;加审阅与协议黄金测试`。

### Task 2: 建库,搬纯逻辑、资源与列表纯函数

**Files:**
- Create:`crates/dozer-panel-conversations/{Cargo.toml,src/{lib.rs,review.rs,snapshot.rs,assets.rs,filter.rs,i18n.rs},locales/{en,zh-CN}.json}`;`web/review-trace/`(源码目录整体 `git mv` 进 `crates/dozer-panel-conversations/web/review-trace/`)与 `assets/review-trace/`(产物 `git mv`)
- Modify:workspace `Cargo.toml`、`crates/dozer-app/Cargo.toml`、`crates/dozer-app/src/transcript.rs`(`ReviewEntry`、`ToolResultEntry`、`review_entries_from_turns` 与测试搬走;若文件只剩转调则 `pub use`)、`crates/dozer-app/src/assets.rs`(服务 `review-trace/` 的段改调库)、`crates/dozer-app/src/preview/webview_protocol.rs`(`ReviewTraceEvent`、`parse_review_trace_event` 搬走并 `pub use`)、`crates/dozer-app/src/extensions/conversations.rs`(`filter_sessions`、`conversation_visible_count`、`conversation_agents_present` 搬走)、`crates/dozer-app/src/app/{app,update}.rs`(`ReviewSnapshot`、`merge_review_entries`、`mark_review_loaded`、`refresh_open_conversation_summary` 搬走并转调)、`crates/dozer-app/build.rs` 或打包脚本里引用 `web/review-trace` 的路径、`scripts/audit/check_panel_boundary.py`、`scripts/i18n/*.py`
- Test:库内各模块(随代码搬)

**Interfaces:**
- Produces(原样搬自现状,只改 `use` 与可见性):
  - `review::{ReviewEntry, ToolResultEntry, review_entries_from_turns(turns: &[TurnRecord]) -> Vec<ReviewEntry>, merge_review_entries(existing: &mut Vec<ReviewEntry>, new: Vec<ReviewEntry>, append: bool)}`。
  - `snapshot::{ReviewSnapshot<'a> { entries, agent_label, summary_title, summary_text, summary_time }, ReviewView { source: ReviewSource, entries, error: Option<String>, agent: AgentKind, nonce: u64, summary_title/text/time, loaded_nonce: Option<u64> }, ReviewSource { Conversation(String) }`(**只保留 `Conversation`**,D5),`mark_review_loaded(&mut Option<ReviewView>)`、`refresh_open_conversation_summary(review: &mut Option<ReviewView>, nonce: &mut u64, row: &SessionRow) -> Option<String>`、`review_pane_state(rv: Option<&ReviewView>) -> ReviewPaneState`、`review_webview_spec(rv: Option<&ReviewView>, locale: &str, scheme: &str) -> Option<ReviewWebviewSpec>`}`。`ReviewWebviewSpec { pub url: String, pub visible: bool, pub park_offscreen: bool }`;`url` 为 `"{scheme}://review-trace/host.html?_r={nonce}&locale={locale}"`(Dozer 传 `"dozer"`,字节与现状相同)。
  - `assets::{serve_review_trace(path: &str, review_json: Option<&str>) -> Option<(&'static str, Vec<u8>)>, ReviewTraceEvent { DocumentLoaded }, parse_review_trace_event(raw: &str) -> Result<WebviewEnvelope<ReviewTraceEvent>, ProtocolError>}`(应答逻辑照搬 `assets.rs` 约 275–301 行;三个静态文件 `include_bytes!`)。`WebviewEnvelope` / `ProtocolError` 若仍在 `dozer-app`,先在库里放一份最小等价类型并让 `dozer-app` 复用库的,**不得**让库依赖 `dozer-app`。
  - `filter::{filter_sessions, conversation_visible_count, conversation_agents_present, CONVERSATION_PAGE_SIZE: usize = 20, CONVERSATION_DETAIL_PAGE_SIZE: u32 = 200}`。
  - `CONVERSATIONS_OWNER: &str = "dozer.conversations"`。
- [ ] **Step 1: 写失败测试**(库里新增,其余随代码搬、期望值不改):`review_snapshot_through_the_library_matches_the_golden`(用 Task 1 的黄金文件,库内重跑同一输入,字节一致——这是本任务最重要的守恒依据);`serve_review_trace_through_the_library_matches_the_golden`;`review_webview_spec_url_uses_the_given_scheme_and_keeps_the_dozer_bytes`(`scheme = "dozer"` 时与现状字符串相同,`scheme = "digger"` 时只有前缀不同);`webview_spec_is_none_for_error_or_empty_entries_and_parked_until_loaded`(Review Focus 4);`serve_review_trace_rejects_path_traversal_and_unknown_paths`(`../`、`%2e%2e`、绝对路径、空串返回 `None`,不 panic);`serve_review_trace_data_json_is_404_without_a_snapshot_exactly_as_before`(现状:没有快照时 `data.json` 返回 404,不是空对象;黄金测试里也记下这一条);`filter_handles_unicode_case_and_empty_inputs`、`visible_count_never_overflows_for_large_page_numbers`;`only_one_row_is_marked_current`(随 `current_review_conversation_id` 的简化——`Session` 分支删除后只剩 `Conversation`)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬代码与测试。** 删除 `ReviewSource::Session`、`review_should_refresh_on_turn`、`spawn_review_load`(路径版)及 `agent_state_changed` 里那段「Session 审阅回合结束重解析」分支(全仓库没有创建 `Session` 来源 `ReviewView` 的地方,已核实);对应只测这些的测试删除并列入提交说明。`web/review-trace` 的构建脚本与「产物必须齐全」「CSP 严格」两个测试随走;`include_bytes!` 路径与打包脚本、`scripts` 里引用 `assets/review-trace` 的地方一并改。
- [ ] **Step 4: 验证。** `cargo test -p dozer-panel-conversations -p dozer-app` 总数 =(基线 − 已删 + 新增);`bash scripts/check-i18n.sh`;`python3 scripts/audit/check_panel_boundary.py`;`cargo tree -p dozer-panel-conversations | grep -E "dozer-(app|client|core)|dozerd|wry|winit"` 无输出;在 `web/review-trace` 里 `npm test` 与 `npm run build`,产物字节与提交的一致(`git diff --exit-code assets`)。
- [ ] **Step 5: 提交。**

### Task 3: 状态、消息、更新、总结生成与视图

**Files:**
- Create:`crates/dozer-panel-conversations/src/{state.rs,update.rs,summary.rs,adapter.rs,view.rs,detail.rs}`
- Modify:`crates/dozer-app/src/extensions/conversations.rs`(缩成转调或删除)、`crates/dozer-app/src/workspace/state.rs`(`review`、`review_nonce`、`review_snapshot`、`spawn_review_load_conversation`、`spawn_conversations_refresh`、`current_review_conversation_id` 缩成转调或删除)、`crates/dozer-app/src/workspace/view.rs`(`review_content_pane`、`review_content`、`load_more_button`、`review_webview_spec`、`review_pane_state` 删除或转调)、`crates/dozer-app/src/app/{message.rs,update.rs,app.rs,view.rs}`、`crates/dozer-app/src/runtime.rs`(约 320、529、830–838、960 行)、`crates/dozer-app/src/host_adapters/conversations_panel.rs`(新建)
- Test:库内 `update.rs`、`summary.rs`、`view.rs`;`crates/dozer-app/src/host_adapters/conversations_panel.rs`

**Interfaces:**
- Produces:
  - `state::WorkspaceState`:原列表字段 + `review: Option<ReviewView>`、`review_nonce: u64`、`review_snapshot: Arc<Mutex<Option<String>>>`、`requests: RequestTrackers`、`outbox: NoticeOutbox`、`host_requests: Vec<ConversationsRequest>`;方法 `take_outbox() -> Vec<Notice>`、`take_requests() -> Vec<ConversationsRequest>`、`review_snapshot(&self) -> Arc<Mutex<Option<String>>>`、`current_conversation_id(&self) -> Option<&str>`、`sessions()`、`summary_generating(&str)`、`summary_error(&str)`。
  - `update::Message`:列表现有变体(`AsyncResult`、`SessionsRefreshed`、`ListMore`、`SearchInput`、`SearchSubmit`、`AgentFilterSelect`、`AgentFilterClose`、`Hover(ConvHover, bool)`)+ 原先被内核拦截、现在落进库的变体 `SessionOpen(String, AgentKind)`、`DetailLoadMore(String, i64)`、`ReviewLoaded { source: ReviewSource, append: bool, result: Result<Vec<ReviewEntry>, String> }`、`ReviewTraceLoaded`、`SummaryGenerate(String, AgentKind)`、`SummaryGenerateFinished(String, Result<(), LocalizedText>)`、`AgentFilterOpen { at: (f32, f32) }`(替代原 `AgentPickerOpen`,触发请求而不是自己画菜单);**变体名与载荷以搬代码时逐个对应现状为准,不增不减**,`TextInputMenuOpen` 保持库发出不透明目标(同 `dozer-panel-project` 的 `TextInputTarget`)。
  - `update::update(state: &mut WorkspaceState, msg: Message, ctx: &UpdateCtx<'_>, emit: impl Fn(Message) + Clone + Send + 'static)`,`UpdateCtx<'a> { pub project_id: i64, pub project_path: &'a Path, pub conversation: &'a Arc<dyn ConversationService>, pub summary: &'a Arc<dyn SummaryService>, pub handle: &'a tokio::runtime::Handle, pub caps: ConversationsCapabilities, pub locale: &'a str, pub now_ms: u64 }`;`update::on_activate(state, ctx, emit)` 刷新列表(原 `on_activate`)。
  - `summary::run_summary_job(...)`:原 `conversation_summary_generate` 的库版——每秒轮询 `get_summary_job`,终态与文案(`summary_submit_failed`、`summary_failed`、`summary_failed_unknown`、`summary_cancelled`、`summary_job_missing`、`summary_poll_failed`、`summary_refresh_failed`)逐字不变;成功后刷新列表并回送 `SummaryGenerateFinished`。仅 `caps.summaries` 时可触发;轮询间隔作为可注入参数(测试用 1ms)。
  - `adapter::{ConversationsCapabilities { pub summaries: bool, pub linked_todos: bool } (Default 全 false, ::all() 全 true), trait LinkedTodos { fn linked_text(&self, conversation_id: &str) -> Option<String>; }, ConversationsRequest { ShowAgentFilter { at: (f32, f32), current: Option<AgentKind>, present: Vec<AgentKind> } }, trait PanelDisplay<M>}`;`PanelDisplay<M>`:`wrap(&self, Message) -> M`、`locale`、`hover_progress(ConvHover) -> f32`、`panel_head(icon, title, actions)`、`list_collapse_button(tooltip_collapse, tooltip_expand)`、`tab_divider`、`pane_style(ConversationsPane) -> PaneStyle`、`agent_icon(AgentKind, Color)`、`agent_icon_color(AgentKind)`、`relative_time(last_ts_ms, now_ms) -> String`。
  - `view::{conversations_list<'a, M>(state, host, todos: &dyn LinkedTodos, width, outer) -> Element, review_pane<'a, M>(state, host, caps, width, outer) -> Element}`;`detail::{load_more_button, review_fallback}`。
- Consumes:Task 1 的 `ConversationService` / `SummaryService` / `FakeSummary`,Task 2 的 `review::*`、`snapshot::*`、`assets::*`、`filter::*`。
- [ ] **Step 1: 写失败测试**(用 Task 1 的 fake,每条独立,轮询间隔注入 1ms):
  - `opening_a_row_loads_turns_and_publishes_a_snapshot_with_summary_fields`、`load_more_appends_instead_of_replacing`(沿用 2026-08-27 的断言)、`stale_review_results_for_another_source_are_dropped_and_do_not_touch_the_snapshot`、`opening_a_row_that_vanished_from_the_list_does_nothing`、`two_quick_opens_show_only_the_last_one_even_if_the_first_result_arrives_last`(Review Focus 2)、`review_load_failure_sets_the_error_and_the_pane_state_is_ready_not_loading`;
  - `summary_generate_is_ignored_without_the_capability`、`summary_generate_twice_submits_once`、`summary_terminal_states_map_to_the_legacy_texts`(`Failed` 带/不带 `error_detail`、`Cancelled`、任务不存在、轮询出错、提交失败、成功后刷新失败 → 七条文案与现状逐字相同)、`summary_success_refreshes_the_list_and_reopens_the_open_detail`、`summary_finishing_after_the_detail_changed_does_not_clobber_the_new_detail`;
  - `agent_filter_open_pushes_a_request_instead_of_opening_a_menu`、`agent_filter_select_resets_paging_and_closes`、`search_submit_resets_paging`;
  - `document_loaded_marks_only_the_current_nonce`;`sessions_refresh_failure_keeps_the_old_list_and_notifies_once_per_cooldown`(沿用现有冷却语义);
  - `list_view_hides_linked_todos_without_the_capability`、`review_pane_hides_the_generate_button_without_the_capability`、`degenerate_states_build_without_panicking`(无会话、无项目路径、`review` 为空/出错/条目为空、超长标题)。
- [ ] **Step 2: 运行确认失败。** `cargo test -p dozer-panel-conversations`。
- [ ] **Step 3: 搬代码,逐处替换耦合。** 清单:`PanelHost` / `HoverId` / `PanelKind` → `PanelDisplay` / `ConvHover`;`toast::Outbox` → `NoticeOutbox`;`home_panel_head` → `host.panel_head`;`workspace::{agent_dot_color, agent_icon, lh, relative_time_text}` → `PanelDisplay` 或库内私有小函数(`lh` 是行高包装,库内自备);`CaptureConversationSearchFocus` / `take_conversation_search_focus*` **留在 `dozer-app`**(Dozer 键盘路由专用);`native_menu` 的 Agent 筛选 → `ConversationsRequest::ShowAgentFilter`;`TodoInfo` → `LinkedTodos`;`client.submit_summary_job` / `get_summary_job` / `get_conversation_turns` → `ctx.summary` / `ctx.conversation`;`proxy.send_event` → `emit`;`ReviewView` / `review_nonce` / `review_snapshot` 从 `Workspace` 搬进库状态,`Workspace` 里留 `conversations: conversations::WorkspaceState`。逐处替换清单写进提交说明。
- [ ] **Step 4: Dozer 侧适配。** `host_adapters/conversations_panel.rs`:为 `App` 实现 `PanelDisplay`(委托现有 `agent_icon`、`relative_time_text`、`home_panel_head_with_actions`、`list_collapse_button`,`pane_style` 读 `theme::region::{agent_list_pane|review_content_pane}`);实现 `LinkedTodos`(转给 `ws.todo`);`caps` 传 `all()`;排空 `take_outbox()`(→ Toast)与 `take_requests()`(`ShowAgentFilter` → 原生菜单 / iced 兜底,选中回送 `AgentFilterSelect`);`app/update.rs` 里被拦截的 `SessionOpen` / `DetailLoadMore` / `SummaryGenerate*` / `ReviewLoaded` / `ReviewTraceWebviewEvent` 分支改为转库;`app.rs` 的 `preview_desired` 改读 `review_webview_spec(.., "dozer")`;`runtime.rs` 的 `review_snapshot` 与协议应答改调 `serve_review_trace`;`CONVERSATION_REVIEW_ID_OFFSET`、事件路由不变。`Hover` 与 `TextInputMenuOpen` 由适配层转成顶层消息。
- [ ] **Step 5: 验证。** `cargo test --workspace` 总数 =(基线 − 已删 + 新增);三份黄金与 S6c 的两份黄金字节未变;`cargo tree` 与边界脚本同 Task 2;`cargo clippy --workspace --all-targets`、`cargo fmt --check`、`bash scripts/check-i18n.sh`。
- [ ] **Step 6: 提交**(说明里列出 `Workspace` / `App` 被替换或删除的字段与函数清单、被删的 `Session` 死代码与测试)。

### Task 4: 清理、验收、合并与 tag

- [ ] **Step 1: 清理。** `grep -rn "ReviewSource::Session\|fn review_entries_from_turns\|fn serve_review\|struct ReviewView\|fn merge_review_entries\|fn conversation_summary_generate" crates` 只命中 `dozer-panel-conversations` 与转调处;`workspace/state.rs` 不再有 `review*` 字段;`app/update.rs` 里没有 `conversation_session_open`。
- [ ] **Step 2: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-i18n.sh`;`python3 scripts/audit/check_panel_boundary.py`;`web/review-trace` 的 `npm test`;总数与基线一致(按 Global Constraints 的算式)。
- [ ] **Step 3: 请用户在 Dozer 上目测验收**(与改造前并排):会话列表(卡片文案、相对时间、agent 图标与颜色、「● 当前」、关联待办)、搜索与大小写、Agent 筛选(macOS 原生菜单)、「更多…」分页、点行进详情(webview 出现前的「加载中」、时间线、思考与工具调用折叠、token 用量)、「加载更多」、「生成总结」(成功、失败、取消的提示)、折叠/展开列表、空态与出错态、切项目页签后各自的列表与详情、中英文切换。**未验收不继续。**
- [ ] **Step 4: 合并到 Dozer `main`、切 `libs-2026-10-<日期><字母>` tag、推送——向用户确认后执行。** tag 里 bytehost / byteui / bytegit 的版本必须与 Digger 当时用的逐个核对一致。**不发布产品版本。**

## Digger(本仓库)

### Task 5: 同日升级钉版

**Files:**
- Modify:`Cargo.toml`(各 `dozer-*` 库的 `tag`)、`Cargo.lock`

- [ ] **Step 1: 改钉版。** 所有 dozer libs 的 `tag` 改为 Task 4 的新 tag(全部一起);本期 Digger 不依赖 `dozer-panel-conversations`。
- [ ] **Step 2: 验证。** `cargo update --workspace` 后跑 Cargo.lock 重复检查与 `scripts/check-shared-deps.sh`;`cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`。
- [ ] **Step 3: 提交** `chore(deps): align digger with dozer libs <tag>`。

---

## Self-Review

- **Spec 覆盖:** 设计 G1(库)→ Task 2、3;G2(服务接缝)→ Task 1;§6.1 → Task 1;§6.2 列表/详情/总结/资源/协议应答 → Task 2、3;D3(原生菜单与焦点探测留适配层、请求发件箱)→ Task 3;D5(删 `Session` 死代码)→ Task 2;D7(scheme 参数)→ Task 2 `review_webview_spec`;§7 黄金测试与依赖闭包 → Task 1、2、4;同日升级 → Task 5。**不在本期:** Digger 接入(S7b / S7c)、产品发布、总结在 Digger 的开通。
- **类型一致:** `ReviewView` / `ReviewSource` / `ReviewSnapshot` / `ReviewWebviewSpec` 在 Task 2 定义,Task 3 引用;`UpdateCtx` / `Message` / `ConversationsCapabilities` / `ConversationsRequest` / `PanelDisplay` 在 Task 3 一次定义;`FakeSummary` 与 `SummaryService` 在 Task 1 定义;`CONVERSATION_DETAIL_PAGE_SIZE` 与 `CONVERSATION_PAGE_SIZE` 在 Task 2 的 `filter.rs`。
- **风险点:** ① `Message` 变体清单我核对了名字与几处分支,载荷(尤其 `ReviewLoaded` 现在带项目 id、库内不带)需实现者逐个对应;② `WebviewEnvelope` / `ProtocolError` 的归属未核实,Task 2 Step 3 已写处理原则;③ 搜索框焦点探测留在 `dozer-app` 是对 Dozer 键盘路由的保留,若发现它还牵着库内别的状态,先停下报告;④ 删除 `Session` 死代码依据「全仓库没有创建点」的实测,实现者动手前先再 `grep` 一遍确认;⑤ `web/review-trace` 的目录迁移会动打包脚本与 `build.rs` 路径,是最容易漏的一处,Task 4 的全量与打包自检要覆盖;⑥ 总结轮询间隔注入是为了测试,生产值保持 1 秒;⑦ 合并、切 tag、推送是对外动作;⑧ 本期不发布产品版本,发布在 S7c 收尾(项目规定)。
