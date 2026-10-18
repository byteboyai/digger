# S8a 群聊面板抽成共享库 `dozer-panel-group-chat` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Dozer 的群聊面板——纯 `update` + `Effect` 的状态机、`rev` 增量轮询调度、视图模型与推送协议、页面事件校验、群列表 / 新建群弹窗卡片 / 失败壳视图、`group-chat-content` 页面资源与纯函数协议应答、webview 事件与推送的宿主逻辑——抽成共享库;给页面载荷加一个**加法**的 `capabilities.push_to_todo` 字段;Dozer 改为薄调用,**用户可见行为不变**;合并后切 `libs-*` tag,Digger 同日升级钉版。**不发布产品版本**(发布在 S8c 收尾)。

**Architecture:** 先在现状上写黄金测试,再搬代码。库按 `dozer-panel-todo` / S7a 的形状拥有 `WorkspaceState` / `Message`(第一个字段是项目 id,沿用现状)/ `Effect` / `update` / `run_effect` / `spawn_command` / `poll_if_due` / `select_group`(这些原本就只依赖已在共享库里的 `PanelIo` 与 `RequestTrackers`),外加 `host.rs`:把 `App` 里的 `group_chat_content_event`、`take_group_chat_content_script`、`group_chat_poll_wanted` 的逻辑改成接收显式参数的函数,返回「要做什么」(脚本、宿主动作),由产品执行。产品注入 `PanelDisplay`(面板头、区域度量、折叠按钮、悬停、`agent_icon` / `agent_icon_color`、`locale`)与 `GroupChatCapabilities { push_to_todo }`。webview 的创建、摆位、几何与新建群弹窗的独立窗口宿主仍在 `dozer-app`(库不含 `wry` / `winit`)。

**Tech Stack:** Rust、iced 0.14、byteui、`dozer-model` / `dozer-services` / `dozer-i18n` / `dozer-panel-host`、tokio、serde;`web/group-chat-content` 的 Preact 前端(npm 构建,产物提交)。

**Spec:** `docs/superpowers/specs/2026-10-18-group-chat-panel-s8-design.md`(§2、§3 G1–G2、§6.1、§6.2、§6.4 S8a、§7、§8)。模板:S7a 的 Dozer 段。

## Global Constraints

- **依赖闭包:** `cargo tree -p dozer-panel-group-chat` 不得出现 `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`、`wry`、`winit`。
- **Dozer 用户可见行为不变;** 搬走的测试随代码走、不改期望值、抽库前后 Rust 测试总数守恒(`mod.rs` 45 条、`protocol.rs` 24 条、`assets.rs` 里 `group_chat_content_*` 5 条、`webview_geometry.rs` 里与群聊相关的几何测试留在原处)。
- **推送载荷的唯一变化是新增字段:** `GroupChatViewPayload.capabilities: { push_to_todo: bool }`,**缺省为真**(反序列化缺字段时、旧页面读新载荷时都按真处理);`GROUP_CHAT_PROTOCOL_VERSION` 不变。除该字段外,载荷、事件解析结果、页面 HTML / CSS 与改造前**逐字一致**(黄金测试);页面 JS 产物允许变化,但变化只限「按 `capabilities.push_to_todo` 渲染/隐藏转待办入口」,其余渲染行为靠前端的 `npm test` 与 `render-smoke` 固定。**严格 CSP 不放宽**,页面无外部引用。
- **页面事件的校验与限长不放宽:** `MAX_TOPIC_CHARS = 200`、`MAX_ROLE_CHARS = 2_000`、`MAX_POST_CHARS = 20_000`、`MAX_TODO_CHARS = 10_000`、`MAX_HANDLE_CHARS = 32`、`READY_TIMEOUT = 10s`、`POLL_INTERVAL = 500ms`、`POLL_BACKOFF = 5s`、`POLL_LIMIT = 200` 原值搬入。
- **不改后端:** 不碰 `dozerd` 的群存储、提示词、@提及、无头适配(唯一例外见 Task 1 Step 1 的环境变量核对,仅在发现硬编码时做最小参数化)。
- 只用 git tag 依赖;Dozer 内部 `path = "../.."` 除外。**合并、切 tag、推送是对外动作,向用户确认后执行。** 不发布产品版本。
- **主工作区别人未提交的改动不动;** 实现在 worktree `../dozer-panel-group-chat`、分支 `feat/panel-group-chat-lib`(从 `main` 切出,须已含 S7a)。
- 不引入新的日志依赖;库内日志用 `tracing` 薄封装(同 `dozer-panel-project`)。

## Review Focus

1. **状态机竞态:** 乱序到达的旧 `Polled`、`rev` 回退、轮询期间换群 / 删群、`Posted` 的占位不得推进 `latest_rev`、同一消息 `upsert` 的 `rev` 比较、`AsyncResult` 令牌过期。(现有测试随走;新增 `rev` 回绕与空群场景。)→ Task 3。
2. **载荷体量与内容:** 数千条消息、单条 2 万字、成员颜色槽位回绕、已移除成员、RTL 与零宽字符、空主题;序列化稳定(同一状态两次序列化字节相同,去重靠它)。→ Task 1、2。
3. **不可信页面事件:** 超长文本、越界 / 他群 / 他项目的 id、未知种类、嵌套与重复 JSON 键、`SetDraft` 指向不存在的群、非 UTF-8;一律拒绝或忽略,不 panic,不污染状态。→ Task 1、3。
4. **加载超时与失败:** 10 秒内没有 `ready` → 失败;失败后「重试」清除并重新计时;已加载后收到 `Failed`;失败之后迟到的 `ready` 不得撤销失败;`observe_availability` 在 webview 反复出现 / 消失时的计时。→ Task 3。
5. **能力开关:** `push_to_todo = false` 时,页面事件里的转待办 / `OpenTodo` 即使被伪造发来也**不产生任何效果**(`route_event` 返回 `None`),载荷里 `push_to_todo == false`,页面不渲染入口(前端测试)。→ Task 4。

---

## Dozer(仓库 `../dozer`,worktree `../dozer-panel-group-chat`,分支 `feat/panel-group-chat-lib`)

### Task 1: 基线、黄金测试与前置核对

**Files:**
- Create:`crates/dozer-app/tests/fixtures/group_chat_payload_golden.json`、`crates/dozer-app/tests/fixtures/group_chat_events_golden.txt`、`crates/dozer-app/tests/fixtures/group_chat_serve_golden.txt`
- Test:`crates/dozer-app/src/extensions/group_chat/protocol.rs`、`crates/dozer-app/src/assets.rs`

- [ ] **Step 1: 前置核对(只读,必要时最小修)。** `grep -rn 'DOZER_SESSION_ID' crates/dozerd/src/group_adapter.rs crates/dozerd/src/headless_agent.rs`:若 S6a 已把 `env_remove("DOZER_SESSION_ID")` 参数化为 `paths::product().session_env`,记录结论即可;**若仍硬编码**,做最小参数化(只把字面量换成产品的会话环境变量名,默认产品下字节不变)并加测试 `group_adapter_removes_the_products_session_env`(默认产品移除 `DOZER_SESSION_ID`;`ProductId` 为 digger 时移除 `DIGGER_SESSION_ID`;构造出的 `Command` 的 env 里对应键为移除),写进提交说明。
- [ ] **Step 2: 记录基线。** `cargo test --workspace 2>&1 | grep "^test result"`;记要搬走的测试清单与数量(`group_chat/mod.rs` 45、`protocol.rs` 24、`assets.rs` 的 `group_chat_content_*` 5),写进提交说明。
- [ ] **Step 3: 在改任何代码之前写三份黄金测试(现状上生成一次并提交,之后字节不得变;`UPDATE_GOLDEN=1` 仅此一次)。**
  - `group_chat_payload_matches_the_golden`:一组覆盖面的 `WorkspaceState`(空、一个群无消息、多成员含颜色槽位回绕、人类 / 成员 / 已移除成员 / 系统消息、五种状态各一条、含 Queued 占位、超长主题、RTL 与零宽字符、`hint` 非空、`ever_loaded` 真假)→ `current_view_payload` → `encode_group_chat_push` → 与 `group_chat_payload_golden.json` 逐字比较。
  - `group_chat_events_match_the_golden`:一张表(合法事件各一、超长文本各限长 ±1、越界 / 他群 id、未知种类、缺字段、嵌套对象、数字字符串混淆、非 UTF-8 字节)→ `parse_group_chat_event` 与 `route_event` 的结果(`Ok(Command…)` / `Err(原因)` / `None`)序列化后与 `group_chat_events_golden.txt` 逐行比较。
  - `group_chat_serve_matches_the_golden`:对 `host.html`、`group-chat-content.js`、`group-chat-content.css` 调现有的服务函数,记录 `(mime, 字节数, sha256)`,另含 `../` 穿越、未知子路径的应答,和 `group_chat_serve_golden.txt` 比较。
- [ ] **Step 4: 验证并提交** `test(group-chat): 抽库前的载荷、事件、资源黄金测试`。

### Task 2: 建库,搬协议、资源、常量与 i18n

**Files:**
- Create:`crates/dozer-panel-group-chat/{Cargo.toml,src/{lib.rs,protocol.rs,assets.rs,i18n.rs,consts.rs},locales/{en,zh-CN}.json}`;`web/group-chat-content/` 整体 `git mv` 进 `crates/dozer-panel-group-chat/web/group-chat-content/`,`assets/group-chat-content/` 产物 `git mv` 进 `crates/dozer-panel-group-chat/assets/group-chat-content/`
- Modify:workspace `Cargo.toml`、`crates/dozer-app/Cargo.toml`、`crates/dozer-app/src/extensions/group_chat/protocol.rs`(整体搬走,留 `pub use`)、`crates/dozer-app/src/assets.rs`(`group-chat-content/` 段改调库)、`crates/dozer-app/build.rs` / 打包脚本里引用 `web/group-chat-content` 与 `assets/group-chat-content` 的路径、`scripts/audit/check_panel_boundary.py`、`scripts/i18n/*.py`
- Test:库内各模块(随代码搬)

**Interfaces:**
- Produces(原样搬自现状,只改 `use` 与可见性):
  - `protocol::*`:视图模型(`StatusKey`、`AuthorKind`、`AuthorVm`、`MemberVm`、`GroupVm`、消息 VM、`GroupChatViewPayload` 等)、`current_view_payload`、`encode_group_chat_push`、`WebviewPushState`(含 `observe_availability`、`pending_push`、`mark_sent`、`set_ready`、`set_failed`、`clear_failed`、`failed`、`take_needs_draft_restore`)、`GroupChatWebviewEvent`、`parse_group_chat_event`、`Command`、`route_event`、`READY_TIMEOUT` 与各 `MAX_*` 常量。
  - `assets::{serve_group_chat_content(path: &str) -> Option<(&'static str, Vec<u8>)>, GroupChatWebviewSpec { pub url: String, pub visible: bool, pub park_offscreen: bool }, group_chat_webview_spec(failed: bool, loaded_or_ready: bool, scheme: &str, locale: &str) -> Option<GroupChatWebviewSpec>}`:三个静态文件 `include_bytes!`;路径穿越(`../`、`%2e%2e`、绝对路径、空串)与未知子路径返回 `None`;URL 为 `"{scheme}://group-chat-content/host.html?…"`(Dozer 传 `"dozer"`,字节与现状相同,查询参数与现状一致,实现者从 `app.rs` 约 4061–4090 行抄)。
  - `consts::{CREATABLE_AGENTS, POLL_INTERVAL, POLL_BACKOFF, POLL_LIMIT, GROUP_CHAT_OWNER = "dozer.group_chat"}`。
- [ ] **Step 1: 写失败测试**(库内新增,其余随代码搬、期望值不改):`payload_through_the_library_matches_the_golden`、`events_through_the_library_match_the_golden`、`serve_through_the_library_matches_the_golden`(三条都复用 Task 1 的黄金文件,**字节不变**是本任务最重要的守恒依据);`serve_rejects_traversal_and_unknown_paths_without_panicking`;`webview_spec_url_uses_the_given_scheme_and_keeps_the_dozer_bytes`;`webview_spec_is_none_when_failed_and_parked_until_ready`;`creatable_agents_are_exactly_claude_codex_codebuddy_opencode_qodercn`(固定顺序,与后端 `group_store::agent_to_str` 一致——对后端的 `agent_to_str` 加一条交叉断言测试,若后端在 dozerd 里不便引用则在 `dozer-app` 集成测试里写)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬代码与测试。** `web/group-chat-content` 的 `build.mjs`、`package.json` 的输出路径、`render-smoke.mjs` 里引用的产物路径一并改;「产物必须齐全」「CSP 严格」测试随走(`assets.rs` 的 `group_chat_content_bundle_assets_are_present` / `…strict_csp…` 等 5 条改到库里)。
- [ ] **Step 4: 验证。** `cargo test -p dozer-panel-group-chat -p dozer-app` 总数 = 基线 + 新增;`bash scripts/check-i18n.sh`;`python3 scripts/audit/check_panel_boundary.py`;`cargo tree -p dozer-panel-group-chat | grep -E "dozer-(app|client|core)|dozerd|wry|winit"` 无输出;`web/group-chat-content` 里 `npm test`、`npm run build`,`git diff --exit-code` 确认产物与提交一致。
- [ ] **Step 5: 提交。**

### Task 3: 状态、更新、效果、宿主逻辑与视图

**Files:**
- Create:`crates/dozer-panel-group-chat/src/{state.rs,update.rs,effects.rs,host.rs,adapter.rs,view.rs,dialog.rs}`
- Modify:`crates/dozer-app/src/extensions/group_chat/{mod.rs,view.rs}`(缩成转调或删除)、`crates/dozer-app/src/workspace/state.rs`(`group_chat` 字段保留,类型换成库的)、`crates/dozer-app/src/app/{app.rs,update.rs,message.rs,view.rs}`、`crates/dozer-app/src/runtime.rs`(约 820–823 行)、`crates/dozer-app/src/platform/{group_create_overlay.rs,window_events.rs}`、`crates/dozer-app/src/host_adapters/group_chat_panel.rs`(新建)
- Test:库内 `update.rs`、`effects.rs`、`host.rs`、`view.rs`、`dialog.rs`;`crates/dozer-app/src/host_adapters/group_chat_panel.rs`

**Interfaces:**
- Produces:
  - `state::WorkspaceState`(字段与方法原样:`groups()`、`selected()`、`messages()`、`latest_rev()`、`loaded()`、`load_pending()`、`hint()`、`has_active_turn()`、`composer_drafts_restore_json()`、`set_composer_draft(group_id, text)`、`select(group_id) -> Vec<Effect>`、`load_due()`、`mark_stale()`、`poll_due(now)`、`take_outbox()`)、`update::{Message, Effect, update(state, msg) -> Vec<Effect>}`(**变体名与载荷原样搬,不增不减**,`project_id()` 保留)。
  - `effects::{run_effect, spawn_command, spawn_load_groups, spawn_fetch_messages, select_group, poll_if_due, on_activate}`:签名保持,只把 `crate::panel_host::PanelIo` / `RequestTrackers` 换成 `dozer_panel_host` 的同名类型;`toast::Outbox` → `dozer_panel_host::NoticeOutbox`。
  - `host::{HostAction { None, ShowTodoPanel }, handle_webview_event(state: &mut WorkspaceState, push: &mut WebviewPushState, project_id: i64, event: GroupChatWebviewEvent, caps: GroupChatCapabilities, io: &PanelIo<Message>) -> HostAction, take_content_script(state: &WorkspaceState, push: &mut WebviewPushState, project_id: i64, webview_available: bool, now: Instant, stamp: LocaleStamp, locale: &str, caps: GroupChatCapabilities) -> Vec<String>, poll_wanted(state: &WorkspaceState, panel_visible: bool) -> bool}`:逻辑照搬 `group_chat_content_event`(`Ready` / `Failed{reason}` / `SetDraft` 直接处理,其余经 `route_event` → `select_group` 或 `spawn_command`,`OpenTodo` 返回 `ShowTodoPanel`)、`take_group_chat_content_script`(先 `observe_availability`;不可用或已失败返回空;草稿恢复脚本 `window.__gcDrafts=…;window.__gcApplyDrafts&&window.__gcApplyDrafts()` 一次性下发;再声明式 `pending_push`)、`group_chat_poll_wanted`。返回的是**脚本字符串**,webview id 与 `dispatch_script` 包装由产品负责。
  - `adapter::{GroupChatCapabilities { pub push_to_todo: bool } (Default 全 false, ::all() 全 true), trait PanelDisplay<M> { wrap(&self, Message) -> M; locale(&self) -> &str; panel_head(icon, title, actions) -> Element<M>; pane_style(GroupChatPane) -> PaneStyle; list_collapse_button(tooltip_collapse, tooltip_expand) -> Element<M>; agent_icon(AgentKind, Color) -> Element<M>; agent_icon_color(AgentKind) -> Color }, GroupChatPane { List, Content }}`。
  - `view::{list_pane<'a, M>(state, host, project_id, width, outer) -> Element<M>, content_pane<'a, M>(failed: Option<&LocalizedText>, host, width, outer) -> Element<M>, ShellMessage::Retry}`;`dialog::new_group_dialog_card<'a, M>(state, host, project_id) -> Option<Element<M>>`(= 原 `GroupChatCreateOverlay::card`,`agent_dot_color` / `agent_icon` 改走 `host`;`state.new_group_dialog_open()` 为假时返回 `None`)。
- Consumes:Task 2 的 `protocol::*`、`assets::*`、`consts::*`;`dozer_services::GroupChatService`。
- [ ] **Step 1: 写失败测试**(新增;原有 `mod.rs` 45 条随代码走、期望值不改):
  - `a_stale_poll_after_switching_group_is_dropped`、`rev_never_goes_backwards_on_out_of_order_polls`、`deleting_the_selected_group_while_a_poll_is_in_flight_clears_selection_without_panicking`、`posted_placeholders_do_not_advance_latest_rev`(沿用语义,补空群场景);
  - `host_ready_failed_and_set_draft_are_handled_without_touching_commands`、`host_set_draft_for_a_missing_group_is_ignored`、`host_events_for_a_group_of_another_project_are_rejected`;
  - `content_script_is_empty_when_the_webview_is_unavailable_or_failed`、`content_script_sends_the_drafts_restore_once_then_only_changed_payloads`(连续两次相同状态第二次为空)、`content_script_after_retry_resends_the_full_payload`;
  - `ready_timeout_marks_failed_after_ten_seconds_and_retry_restarts_the_clock`、`late_ready_after_failed_does_not_clear_the_failure`、`failed_after_loaded_falls_back_to_the_native_shell`(Review Focus 4);
  - `poll_wanted_only_when_visible_and_loading_or_active_turn`;
  - `list_pane_builds_for_no_groups_one_group_and_five_hundred_groups`、`content_pane_shows_the_failure_reason_and_a_retry_that_emits_retry`、`dialog_card_is_none_when_closed_and_lists_exactly_the_creatable_agents_when_open`、`degenerate_states_build_without_panicking`(无项目 id、超长主题、空 `hint`)。
- [ ] **Step 2: 运行确认失败。** `cargo test -p dozer-panel-group-chat`。
- [ ] **Step 3: 搬代码,逐处替换耦合。** 清单:`crate::extensions::toast::*` → `NoticeOutbox`;`crate::panel_host::*` → `dozer_panel_host::*`;`crate::i18n::panel_text*` / `panel_keyed*` → 库 `i18n`(owner `dozer.group_chat`,`locales/panels/group_chat/` 内容原文搬入,中英文文案逐字不变);`crate::theme::region::group_chat_list_pane` / `chrome::homespace::home_panel_head_with_actions` / `App::list_collapse_button` / `workspace::{agent_dot_color, agent_icon}` → `PanelDisplay`;`App::group_chat_content_event` / `take_group_chat_content_script` / `poll_group_chat_if_active` 的函数体 → `host`;`Message::GroupChatContentWebviewEvent` / `GroupChatShell` 保留在 `dozer-app` 的顶层消息里(包库类型),`group_create_overlay.rs` 的 `card` 改为调 `dialog::new_group_dialog_card`(winit 窗口、`sync_action`、IME 处理仍留原处)。逐处替换清单写进提交说明。
- [ ] **Step 4: Dozer 侧适配。** `host_adapters/group_chat_panel.rs`:为 `App` 实现 `PanelDisplay`(委托现有 `home_panel_head_with_actions`、`list_collapse_button`、`agent_icon`、`agent_dot_color`,`pane_style` 读 `theme::region::group_chat_list_pane` 与内容侧度量);`caps` 传 `all()`;`app/update.rs` 的 `Message::GroupChat` / `GroupChatContentWebviewEvent`(→ `host::handle_webview_event`,`ShowTodoPanel` → `self.show_panel(PanelKind::Todo)`)/ `GroupChatShell::Retry` 分支改转库;`app.rs` 的 `take_group_chat_content_script` 改为调 `host::take_content_script` 后用 `GROUP_CHAT_CONTENT_ID_OFFSET` 与 `dispatch_script` 包装;`app.rs` 约 4061–4090 行的 webview spec 改读 `group_chat_webview_spec(.., "dozer", locale)`;`webview_geometry` 与 `dims.group_chat_*` 不动。
- [ ] **Step 5: 验证。** `cargo test --workspace` 总数 = 基线 + 新增;Task 1 的三份黄金字节未变;S6c / S7a 的黄金字节未变;`cargo tree` 与边界脚本同 Task 2;`cargo clippy --workspace --all-targets`、`cargo fmt --check`、`bash scripts/check-i18n.sh`。
- [ ] **Step 6: 提交**(说明里列出 `App` / `Workspace` / `platform` 被替换或删除的字段与函数清单)。

### Task 4: 载荷能力字段与页面的加法

**Files:**
- Modify:`crates/dozer-panel-group-chat/src/protocol.rs`(`capabilities` 字段、`route_event` 门禁、`current_view_payload` 新参数)、`crates/dozer-panel-group-chat/web/group-chat-content/src/{protocol.ts,main.tsx,components/*,fixtures.ts,protocol.test.ts,render-smoke.tsx}`、`crates/dozer-panel-group-chat/assets/group-chat-content/*`(重新构建的产物)、`crates/dozer-panel-group-chat/src/host.rs`、Task 1 的载荷黄金文件(**仅**新增字段处允许变化,见 Step 4)
- Test:`protocol.rs`、前端 `npm test`、`render-smoke`

**Interfaces:**
- Produces:`protocol::Capabilities { pub push_to_todo: bool }`(`Serialize`,`Deserialize` 缺省 `true`);`GroupChatViewPayload.capabilities: Capabilities`;`current_view_payload(state, project_id, locale, caps: GroupChatCapabilities)`(签名多一个参数,`host::take_content_script` 已带 `caps`);`route_event(state, event, caps)`:`caps.push_to_todo == false` 时,`PushToTodo` / `OpenTodo` 对应的事件返回 `None`。前端:载荷类型多 `capabilities?: { push_to_todo?: boolean }`,**缺省按真**;`push_to_todo === false` 时不渲染「转待办」按钮与对话框入口。
- [ ] **Step 1: 写失败测试**(Rust):`payload_capabilities_default_to_true_when_absent_in_json`(反序列化 `{}` 缺字段);`payload_serializes_push_to_todo_false_for_digger_caps`;`route_event_drops_push_to_todo_and_open_todo_when_the_capability_is_off`(含伪造事件:合法形状但 `caps` 关闭,返回 `None`、状态不变、不产生 `Command`);`route_event_still_routes_them_when_on`(与 Task 1 事件黄金一致)。前端:`protocol.test.ts` 加「缺 `capabilities` 按真」「`push_to_todo:false` 不渲染入口」两条;`render-smoke` 加一个关闭态的快照断言。
- [ ] **Step 2: 运行确认失败,实现。** 页面改动限于条件渲染,不动样式与其它组件。
- [ ] **Step 3: 重新构建页面。** `npm test`、`npm run build`;提交新产物;`assets` 里的「产物齐全」「CSP 严格」测试仍通过(host.html 不得多出外部引用)。
- [ ] **Step 4: 更新黄金。** 载荷黄金文件**只允许**因新增 `capabilities` 字段而变化:写一个测试 `payload_golden_differs_from_the_pre_extraction_golden_only_by_the_capabilities_field`(把新黄金去掉 `capabilities` 键后与 Task 1 提交的原始黄金逐字比较,原始文件保留为 `group_chat_payload_golden_v0.json` 不再改动,新黄金另存);`serve` 黄金里 `host.html` / `.css` 的 sha256 不变,`.js` 的 sha256 允许变化并在提交说明里标注。
- [ ] **Step 5: 验证与提交。** `cargo test --workspace`;前端测试全绿。

### Task 5: 清理、验收、合并与 tag

- [ ] **Step 1: 清理。** `grep -rn "fn parse_group_chat_event\|fn route_event\|struct WebviewPushState\|fn current_view_payload\|fn take_group_chat_content_script\|fn group_chat_content_event" crates` 只命中 `dozer-panel-group-chat`;`extensions/group_chat/` 只剩转调与重新导出(或整个删除);`web/group-chat-content` 不再位于 `dozer-app` 下。
- [ ] **Step 2: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-i18n.sh`;`python3 scripts/audit/check_panel_boundary.py`;`web/group-chat-content` 的 `npm test`;打包自检(`scripts/build-macos-app.sh` 的 dry-run 或 `cargo build -p dozer-app --release` 后确认页面资源已编进二进制,不依赖磁盘路径)。
- [ ] **Step 3: 请用户在 Dozer 上目测验收**(与改造前并排):群列表(空态、多群、选中高亮)、新建群弹窗(独立窗口、多行主题、勾选成员、取消 / 创建、Esc)、聊天页(成员条、消息流、状态点、流式回复)、发言与 @提及(单个 / 多个 / 不存在的句柄)、取消(单条 / 整群)、重试、增删改成员、删群、转待办(仍可用,打开待办面板)、webview 加载失败壳与「重试」(可临时拆掉页面文件复现)、输入框草稿在切面板后的恢复、折叠列表、窗口缩放、中英文切换。**未验收不继续。**
- [ ] **Step 4: 合并到 Dozer `main`、切 `libs-2026-10-<日期><字母>` tag、推送——向用户确认后执行。** tag 里 bytehost / byteui / bytegit 的版本必须与 Digger 当时用的逐个核对一致。**不发布产品版本。**

## Digger(本仓库)

### Task 6: 同日升级钉版

**Files:**
- Modify:`Cargo.toml`(各 `dozer-*` 库的 `tag`)、`Cargo.lock`

- [ ] **Step 1: 改钉版。** 所有 dozer libs 的 `tag` 改为 Task 5 的新 tag(全部一起);本期 Digger 不依赖 `dozer-panel-group-chat`。
- [ ] **Step 2: 验证。** `cargo update --workspace` 后跑 Cargo.lock 重复检查与 `scripts/check-shared-deps.sh`;`cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`。
- [ ] **Step 3: 提交** `chore(deps): align digger with dozer libs <tag>`。

---

## Self-Review

- **Spec 覆盖:** G1(库)→ Task 2、3;G2(只经 `GroupChatService`)→ Task 3(`run_effect` 等沿用已服务化的调用);§6.1 库边界、宿主逻辑下沉 → Task 3;§6.2 页面的 `capabilities` 加法与黄金约束 → Task 4;§7 黄金测试 / 依赖闭包 / 恶意事件表 → Task 1、2;§8 Q3(页面产物重建)→ Task 4;同日升级 → Task 6。**不在本期:** Digger 接入(S8b / S8c)、`native_host.rs`、产品发布。
- **类型一致:** `Message` / `Effect` / `WorkspaceState` 在 Task 3 定义(原样搬);`protocol::*` 与 `assets::*` 在 Task 2 定义,Task 3、4 引用;`GroupChatCapabilities` 在 Task 3 定义、Task 4 用于 `route_event` 与 `current_view_payload`;`HostAction` / `host::*` 在 Task 3 一次定义;Task 3 的 `take_content_script` 已带 `caps` 参数,Task 4 只补载荷字段。
- **风险点:** ① `Message` / `Effect` 变体我读了名字与主要载荷,搬时逐个对应、不增不减;② `app.rs` 里 webview 的 URL 查询参数(`theme` 等)我未逐字核对,Task 2 要求从现状抄并由黄金测试固定;③ 新建群弹窗是独立 winit 窗口,库只给卡片元素,窗口生命周期、IME、`sync_action` 全留 `dozer-app`;④ 页面 JS 产物字节会变(Task 4),这是计划允许的唯一产物变化,要在对照时重点看页面渲染;⑤ `group_adapter` 的环境变量核对(Task 1 Step 1)若发现硬编码,会让本计划多一处对 `dozerd` 的最小改动,须向用户说明;⑥ 合并、切 tag、推送是对外动作;⑦ 本期不发布产品版本,发布在 S8c 收尾(项目规定)。
