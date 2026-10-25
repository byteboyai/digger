# S8c Digger 群聊页 webview、Dozer 对照验收与发布 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 S8b 内容块里的占位换成真实的群聊页:成员条、消息流(流式回复)、输入框、@提及、取消 / 重试、成员增删改、删群,用**同一份 `group-chat-content` 页面**;先把审阅与群聊两个原生 webview 宿主的共同部分抽成 `native_host.rs`;用一个**确定性的假 Agent 命令行**与 Dozer 逐状态对照;压力与容错回归;**完成群聊面板迁移并发布 Dozer 与 Digger 各一个新版本**(项目硬性要求)。

**Architecture:** 全部在 `digger-desktop`。`adapters/native_host.rs`(新,从 S7c 的 `review_native.rs` 抽出):单槽 `wry::WebView` + 代数(epoch)+ 纯函数 `plan(current, desired) -> Vec<Action>`(`Destroy` / `Create` / `Navigate` / `SetBounds` / `Park` / `Show`)+ 导航白名单 + 新窗口 / 下载拒绝 + 缩放 + `window::run` 套路;`review_native` 改用它(行为与测试不变)。`adapters/group_chat_native.rs`:协议名 `digger`、主机名 `group-chat-content`,应答调库的 `serve_group_chat_content`;IPC 里用库的 `parse_group_chat_event` 并核对页面 URL 的来源;每次 `sync_native` 先调库的 `host::take_content_script` 得到要注入的脚本(草稿恢复 + 声明式推送),用 `window.__dozer.dispatch(...)` 包装后 `evaluate_script`;页面事件交库的 `host::handle_webview_event`。10 秒内没有 `ready` 判失败,内容块显示库的失败壳与「重试」。

**Tech Stack:** Rust、iced 0.14、wry、`dozer-panel-group-chat`(S8a)、tokio、byteui。

**Spec:** `docs/superpowers/specs/2026-10-18-group-chat-panel-s8-design.md`(§6.3 聊天内容 webview 与 `native_host.rs`、§6.4 S8c、§7、§8)。前置:S8a、S8b 已合并,S7c 已合并(`review_native`、`review_geometry` 的做法)。

## Global Constraints

- **库不改、页面不改**(页面的唯一改动是 S8a 的 `capabilities` 加法)。严格 CSP 不放宽,页面不得请求任何外部资源:导航处理器只放行 `digger://group-chat-content/`,新窗口与下载一律拒绝;消息里的外链、远程图片不得发出网络请求。
- **页面事件只经库的校验入口:** IPC 收到的文本先 `parse_group_chat_event`,再交 `host::handle_webview_event`;宿主不得自己解释事件、不得放宽限长。来源页面 URL 不是 `digger://group-chat-content/…` 的 IPC 一律丢弃。
- **单槽语义:** 任一时刻最多一个群聊 webview,显示聚焦项目的聊天;切项目页签、关项目页签时必须销毁或换数据,不得短暂显示别的项目的消息或草稿。草稿由库在宿主侧保存、页面重建时一次性恢复。
- **webview 永不盖住覆盖层或停在不该出现的地方:** 面板不可见、任一覆盖层打开(含 S8b 的新建群弹窗、Agent 选择器、溢出菜单、确认框、筛选菜单)、窗口正在关闭 → 隐藏(销毁或离屏停放);`ready` 之前离屏停放;失败后销毁,显示失败壳。
- **键盘归还:** webview 销毁或隐藏后,键盘焦点必须回到 iced 窗口(终端与 `text_editor` 能立即收键),不能被残留的 `WKWebView` 吃掉(真机必验)。
- **群聊会以只读方式拉起成员 Agent 命令行,成员能读项目文件、不能改**(设计 Q1;**2026-10-25 订正,原文写成「在项目目录里工作」**,见 Dozer 仓库 `docs/superpowers/plans/2026-10-25-s7-s8-refresh.md` §1.1):对照与回归一律用**假 Agent 命令行**(Task 4),不消耗真实额度、结果确定;真实 Agent 的端到端只做一次手动抽检并由用户操作。
- 缩放:创建时与 `icon_size::scale()` 变化时 `webview.zoom(scale)`(沿用 S7c)。
- 失败不静默:webview 创建失败、协议异常、IPC 解析失败显示在通知条并 `eprintln!`。
- 对照验收的「已接受差异」只允许 S6d 记过的两条(悬停阶跃、延迟提示精度)与经用户确认记账的;其余必须修。
- **发布要求(项目硬性规定):** 群聊面板迁移在 Dozer 与 Digger 都落地后,Dozer 与 Digger 各重新发布一个版本(Dozer:`scripts/build-macos-app.sh release`;Digger:`scripts/release.sh`),装到本机启动确认。升版本、提交版本号、安装都是对外动作,**先向用户确认再执行**。
- 主工作区别人未提交的改动不动;worktree `../digger-s8c`、分支 `feat/group-chat-chat`(从已含 S8b 的 `main` 切出)。合并前向用户确认。

## Review Focus

1. **就绪握手与推送:** 页面反复重建(切面板、切项目)时,`ready` 之前不推送、之后立刻推全量;同一状态不重复推送;`ready` 超时 → 失败壳,重试后重新计时;失败后迟到的 `ready` 不撤销失败。→ Task 2。
2. **流式更新与竞态:** 发言进行中页面持续收到更新;取消 / 重试 / 删群 / 切群 / 切项目发生在流式中途;轮询 500ms 订阅随进行中发言开关;`diggerd` 中途死亡时消息停在 Running 的表现(与 Dozer 一致,记账)。→ Task 2、5。
3. **键盘与焦点:** 在 webview 输入框打字、输入法(中文)、粘贴多行、⌘A / ⌘C / ⌘V;webview 隐藏后终端与 `text_editor` 立即可输入;打开新建群弹窗时 webview 隐藏且弹窗输入框得到焦点。→ Task 3。
4. **不可信内容:** 消息含 `<script>`、HTML、`javascript:` 链接、远程图片 / CSS、10 万字符无空格行、零宽 / RTL、未闭合围栏——页面不执行、无网络请求、布局不溢出;页面发来的伪造事件(越界 id、他群 id、超长文本、转待办)被库拒绝。→ Task 5。
5. **覆盖层与几何:** 打开任一覆盖层 / 切走面板 / 收起 / 放大 / 缩放窗口 / 拖分隔线 / 折叠列表时位置与可见性始终正确;顶部圆角内缩(群聊详情列没有原生面板头,`chrome_top = 0`)不戳出面板圆角。→ Task 2、3。

---

### Task 1: 抽 `native_host.rs`,`review_native` 改用它

**Files:**
- Create:`crates/desktop/src/adapters/native_host.rs`
- Modify:`crates/desktop/src/adapters/review_native.rs`、`crates/desktop/src/adapters/mod.rs`
- Test:`native_host.rs`;`review_native.rs` 的既有测试**原样通过**

**Interfaces:**
- Produces:
  - `native_host::{Desired { pub visible: bool, pub rect: LogicalRect, pub url: Option<String>, pub park_offscreen: bool, pub owner: Option<i64> }, Live { url: String, owner: Option<i64>, rect: LogicalRect, parked: bool }, Action { Destroy, Create { url }, Navigate { url }, SetBounds(LogicalRect), Park, Show(LogicalRect), Nothing }, plan(current: Option<&Live>, desired: &Desired) -> Vec<Action>}`:语义即 S7c 的 `plan`(无 `url` 或 `!visible` → `Destroy`;无实例 → `Create`;`url` 变 → `Navigate`;`park_offscreen` → `Park`;可见且矩形变 → `SetBounds` / `Show`;`owner` 变 → **先 `Destroy` 再 `Create`**)。
  - `native_host::Allowed { pub scheme: &'static str, pub host: &'static str }` 与 `fn is_allowed_navigation(url: &str, allowed: &Allowed) -> bool`(只放行 `{scheme}://{host}/…`;`file:`、`https:`、`javascript:`、其它主机一律拒绝)。
  - `native_host::Host<E>`:封装 `thread_local` 单槽、epoch、`sync(id, desired, build: impl Fn(&Window, &str) -> WebviewBuilder…)`、`rezoom(scale)`、`destroy(id)`;事件类型 `E` 由调用方给。具体泛型形状留给实现者,**以 `review_native` 现有代码为准抽取**。
- [ ] **Step 1: 写失败测试**(从 S7c 的 `review_native` 测试里把与具体页面无关的那批原样搬过来作为 `native_host` 的测试):`plan_destroys_without_a_url_or_when_not_visible_and_does_nothing_if_nothing_exists`;`plan_creates_once_then_navigates_only_when_the_url_changes`;`plan_parks_offscreen_until_ready_then_shows_at_the_rect`;`plan_destroys_then_creates_when_the_owner_changes`;`plan_sets_bounds_only_when_the_rect_changed`;`is_allowed_navigation_accepts_only_the_exact_origin`(含 `digger://group-chat-content.evil/`、`digger://group-chat-content@evil/`、大小写与端口变体、空串);`late_events_from_an_older_generation_are_ignored`。
- [ ] **Step 2: 运行确认失败,抽取,让 `review_native` 只剩「页面特有」部分(协议应答、IPC 解析、`_r` 校验)。**
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop review_native native_host` 全绿且 `review_native` 的测试条数不减;沙箱真机再点开一次审阅详情确认无回归。
- [ ] **Step 4: 提交** `refactor(digger): 抽出单槽原生 webview 宿主 native_host`。

### Task 2: 群聊页宿主 `adapters/group_chat_native.rs`、几何与事件

**Files:**
- Create:`crates/desktop/src/adapters/group_chat_native.rs`、`crates/desktop/src/adapters/group_chat_geometry.rs`
- Modify:`crates/desktop/src/adapters/group_chat.rs`(`content_view`、`desired_for_chat`、失败壳)、`crates/desktop/src/product.rs`(`Msg::GroupChatNative(Event)`、`boot` 接流、`sync_native` 注入脚本、关闭路径销毁)、`crates/desktop/src/adapters/mod.rs`
- Test:`group_chat_native.rs`、`group_chat_geometry.rs`、`group_chat.rs`、`product.rs`

**Interfaces:**
- Produces:
  - `group_chat_geometry::chat_bounds(block: Rect, zone_radius: f32, zone_padding: f32, scale: f32) -> Rect`:群聊详情列的矩形——左右各内缩区内边距、顶部与底部各内缩「区内边距 + 圆角半径」(对照 Dozer `webview_geometry::pair_content_pane_bounds_for` 的 `chrome_top = 0` 分支),结果宽高钳到至少 1;**不**减面板头(群聊详情列没有原生头)。
  - `group_chat_native::Event { Built(u64, Result<(), String>), Message(u64, String), Done }`;`GroupChatNative::{new, take_receiver, update(event) -> Option<GroupChatWebviewEvent>, sync(id, desired, scripts: Vec<String>, data_scheme) -> Task<Event>}`:协议闭包调 `serve_group_chat_content(path)`(无命中 404 空体);IPC 处理器先校验 `request.uri()` 的来源,再 `parse_group_chat_event(body)`,成功才发 `Event::Message`;`sync` 在每次有新脚本时 `evaluate_script`(脚本由 `host::take_content_script` 产出并已包上 `window.__dozer&&window.__dozer.dispatch&&window.__dozer.dispatch(<json 字符串>)`——包装函数沿用 Digger 用量宿主已有的 `dispatch`,与 Dozer 的 `preview::dispatch_script` 字节相同)。
  - `GroupChatPanel::desired_for_chat(&self, project, block, overlays_open, panel_visible, closing, locale) -> native_host::Desired`:用库的 `group_chat_webview_spec(failed, ready, "digger", locale)`;聚焦项目的 `owner = Some(project_id)`;`failed` 时 `url = None`(销毁)并显示失败壳;`!ready` 时 `park_offscreen`。
  - `GroupChatPanel::on_webview_event(&mut self, project_id, event) -> Task<Msg>`:调库的 `host::handle_webview_event(.., caps, &self.io)`,`HostAction::None` 即可(`ShowTodoPanel` 在 `caps.push_to_todo == false` 下不会出现,收到则忽略并记日志)。
- Consumes:S8a 的 `host::{handle_webview_event, take_content_script, poll_wanted}`、`assets::{serve_group_chat_content, group_chat_webview_spec}`、`protocol::*`;S8b 的 `GroupChatPanel`、`Overlay`、`overlays_open()`;Task 1 的 `native_host`。
- [ ] **Step 1: 写失败测试:** `chat_bounds_matches_dozers_numbers_for_a_known_window`(1200×800、默认分割、右栏:与 Dozer `group_chat_content_pane_bounds_for(..)` 相差不超过 1 像素;期望值由实现者在 Dozer worktree 里用该函数算出后写死,附算式与参数);`chat_bounds_is_never_negative_for_tiny_blocks`;`chat_bounds_follows_the_scale`;`ipc_from_another_origin_is_dropped`;`ipc_with_oversized_or_malformed_json_is_dropped_and_noted`;`ipc_ready_then_the_first_sync_pushes_the_full_payload_once`(假脚本收集器:`ready` 前零脚本;`ready` 后一条全量;同状态再 sync 零脚本);`switching_the_project_destroys_and_recreates_the_webview_and_restores_the_new_projects_drafts`;`failed_state_destroys_the_webview_and_shows_the_failure_shell_and_retry_recreates_it`;`ready_timeout_after_ten_seconds_marks_failed_once`(注入时钟);`late_ready_after_failed_is_ignored`;`forged_push_todo_and_open_todo_events_have_no_effect_with_the_capability_off`;`events_for_a_group_of_another_project_are_rejected`;`protocol_serves_the_three_files_and_404s_everything_else_including_traversal`;`navigation_handler_allows_only_the_group_chat_origin`(用 Task 1 的 `is_allowed_navigation`);`new_window_and_download_requests_are_denied`。
- [ ] **Step 2: 运行确认失败,实现。** `product.rs`:`sync_native` 里对聚焦项目取 `take_content_script`(传入 `webview_available = 实例已创建`)、`Msg::GroupChatNative(Event::Message(_, body))` → 解析成功后交 `on_webview_event`;`boot` 接 `take_receiver` 成 iced 流;关闭路径销毁。
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop group_chat`;沙箱真机用 Task 4 的假 Agent 建群、发言,看到流式回复,截图。
- [ ] **Step 4: 提交。**

### Task 3: 覆盖层、可见性与键盘焦点

**Files:**
- Modify:`crates/desktop/src/product.rs`(`overlays_open()` 已含新建群弹窗,补充可见性输入)、`crates/desktop/src/adapters/group_chat.rs`、`crates/desktop/src/adapters/native_host.rs`(销毁后归还焦点)
- Test:`product.rs`、`native_host.rs`

**Interfaces:**
- Produces:`native_host::Host::destroy` 之后调用 `window::run(id, |w| …)` 把窗口内容视图设为第一响应者(macOS:`makeFirstResponder(contentView)`,复用 Digger 现有的 `bytehost_chrome::platform_macos` 或 `objc2` 调用方式,实现者按现有宿主代码取一种);非 macOS 空操作。`ProductShell::group_chat_visible(&self) -> bool`(= `panel_visible(group_chat_id())`)。
- [ ] **Step 1: 写失败测试:** `opening_the_new_group_dialog_hides_the_chat_webview_and_closing_it_restores_it`;`opening_the_agent_picker_or_overflow_menu_or_confirm_or_agent_filter_hides_it_too`;`switching_to_another_right_panel_destroys_it`;`collapsing_the_right_zone_or_maximizing_the_left_one_destroys_it`;`window_closing_destroys_it_before_shutdown`;`the_first_frame_after_becoming_visible_is_parked_until_ready`;`dragging_the_divider_moves_the_webview_with_the_block`(连续 20 个矩形,每次 `SetBounds` 等于当次 `chat_bounds`);`list_collapse_expands_the_chat_to_the_full_width`;`destroying_the_webview_requests_first_responder_back_to_the_window`(用可注入的「归还焦点」回调断言被调用一次;销毁不存在的实例不调用)。
- [ ] **Step 2: 运行确认失败,实现,验证,提交。** 沙箱真机(人做):在聊天输入框打字 → 切到 Agent 面板 → 终端立即可输入;打开新建群弹窗 → 主题框立即可输入;弹窗关闭后聊天输入框可再聚焦。

### Task 4: 假 Agent 命令行、画廊与 Dozer 逐状态对照

**Files:**
- Create:`crates/desktop/tests/fixtures/fake-agents/{claude,codex}`(可执行脚本)、`crates/desktop/tests/fixtures/fake-agents/README.md`、`scripts/with-fake-agents.sh`、`crates/desktop/examples/group_chat_gallery.rs`(扩展 S8b 版本)、`docs/superpowers/notes/2026-10-18-group-chat-parity.md`
- Test:假脚本自测

**Interfaces:**
- Produces:
  - **假 Agent 命令行:** 与 `dozerd::group_adapter` 的调用约定一致(实现者先读 `group_adapter.rs` 的参数拼装与输出解析,按它的约定输出),按提示词里的指令返回**确定的**内容:普通问题回固定一段 Markdown(含列表、代码块、链接);提示词含 `FAKE:slow` 时分 5 段每段间隔 400ms 输出(用于流式与取消);含 `FAKE:fail` 时非零退出并输出固定错误;含 `FAKE:hostile` 时回第 5 节列的恶意内容(`<script>`、`javascript:` 链接、远程图片、10 万字符无空格行、RTL / 零宽);含 `FAKE:long` 时回约 2 万字。脚本不联网、不写项目文件、不读环境里的任何密钥。
  - `scripts/with-fake-agents.sh <cmd…>`:把 `fake-agents` 目录放到 `PATH` 最前面再执行命令(用来启动 `diggerd` / `dozerd` / 两个产品)。
  - 画廊 `group_chat_gallery --state <name>`:`empty`、`one-group-idle`、`streaming`、`long-history`(500 条)、`failed-message`、`cancelled`、`removed-member`、`webview-failed`、`dialog-open`、`daemon-unavailable`、`hostile`;用假 `GroupChatService` 预置,不连守护进程。
- [ ] **Step 1: 写假脚本与自测。** 自测直接按 `group_adapter` 的方式调用脚本,断言输出能被它的解析函数接受(在 Dozer 一侧的测试里对脚本做一次往返,证明约定没写错)。
- [ ] **Step 2: 画廊。** 每个状态一个函数,构造库状态后摆出面板;用于 Digger 一侧的截图与快速迭代。
- [ ] **Step 3: 逐状态对照。** Dozer 与 Digger 都经 `with-fake-agents.sh` 启动,各用独立的数据根与临时 `HOME`,打开同一个项目路径、做同样的操作序列:建群(两个成员)→ 发言 → 看流式 → 取消 → 重试 → 失败 → 恶意内容 → 长历史 → 增删改成员 → 删群 → 失败壳(临时让页面加载超时)→ 新建群弹窗。对每个状态在两个产品里截图并排比较,差异逐条记进 `group-chat-parity.md`:`状态 | 差异 | 原因 | 处理(修 / 接受并写理由)`。Dozer 一侧由用户操作,我给出摆法清单并截图 Digger 一侧。
- [ ] **Step 4: 修差异,重复 Step 3 直至没有「待修」。** 度量只改 Digger 一侧(`pane_style`、`group_chat_geometry`),不改库、不改页面。
- [ ] **Step 5: 提交(含对照文档与关键截图路径)。**

### Task 5: 压力、容错与同机并存回归

- [ ] **Step 1: 压力**(假 Agent;观察界面是否卡顿、CPU、内存、webview 内存):2 000 条历史消息的群;连续发言 30 次(`FAKE:slow` 与普通混合);单条 2 万字;流式中取消 / 重试 / 切群 / 删群 / 切项目 / 折叠列表 / 缩放窗口 / 最小化再恢复 / 进出全屏。卡顿先判断是页面 / 库(Dozer 同样会卡)还是 Digger 摆位或脚本注入逻辑,**库与页面的问题报告给用户,不在 Digger 里加限流绕过**。
- [ ] **Step 2: 恶意内容:** `FAKE:hostile` 回复显示后,页面不执行脚本、点击 `javascript:` 链接无反应、远程图片 / CSS 不发请求(用 wry 请求回调计数或系统网络日志确认零外部请求)、布局不溢出;页面里手动构造的伪造事件(越界 id、他群 id、超长文本、转待办)被拒绝且有可读日志。
- [ ] **Step 3: 守护进程中途死亡:** 流式回复进行中 `kill diggerd`:页面保持可读、面板顶部出现通知、不崩、不反复重连刷屏;重启后「重试」恢复;停在 Running 的消息的表现与 Dozer 同条件下一致(记账,不在 Digger 里修)。
- [ ] **Step 4: 键盘与输入法:** 聊天输入框里的中文拼音输入、候选框位置(WKWebView 自带)、粘贴多行、⌘A / ⌘C / ⌘V、Esc;webview 隐藏后终端与 `text_editor` 立即可输入(Task 3 的人工项再核一遍)。
- [ ] **Step 5: 同机并存:** Dozer 与 Digger 同时运行并各开同一项目、各用假 Agent,各自只看到自己库里的群与消息;一边发言不影响另一边。
- [ ] **Step 6: 真实 Agent 抽检(用户操作,一次):** 建一个含 Claude 的群,发一句话,确认真实回复流式出现、取消生效。记录是否消耗额度的提示由用户自行判断,计划不替用户决定。
- [ ] **Step 7: 记录** 每项结果、截图路径、发现的问题与处置,写进 `docs/superpowers/notes/2026-10-18-group-chat-regression.md`。任一项不符先停下,不要为了「过」而改验证。

### Task 6: 文档与遗留缺口

- [ ] **Step 1:** `2026-10-18-group-chat-panel-s8-design.md` 状态改「已实现」并补「实现结果」(对照结论、已接受差异、页面 `capabilities` 加法、`native_host.rs` 的抽取边界);`2026-10-14-window-shell-design.md` 的 S8 行同步;`docs/adr/ADR-015-product-owned-daemon.md` 补一句群聊读写各自产品的库、成员 Agent 由各自的 `diggerd` 拉起。
- [ ] **Step 2: 遗留缺口交给用户裁决(不自行决定):** ① 「转待办」随待办面板接入 Digger 时打开(`caps.push_to_todo`);② ~~无头成员 Agent 在项目目录里写文件的权限边界…~~(**2026-10-25 取消**:成员在执行层本来就是只读,Digger 继承同一份 `dozerd` 代码,没有更严可加);③ 页签部件、菜单弹层、`native_host.rs` / wry 样板迁入 byteui 的跨仓库任务(与 S6d、S7 记账合并)。

### Task 7: 门禁、发布与合并

- [ ] **Step 1: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`check-host-deps.sh`(含 `--self-test`)、`check-shared-deps.sh`、`check-shell-regions.sh`、Cargo.lock 重复检查。
- [ ] **Step 2: 发布 Dozer 与 Digger 各一个新版本。** 先向用户确认,再执行:① Dozer:走它自己的发布流程(`scripts/build-macos-app.sh release`:升版本号 → 门禁 → 打包),装到本机启动,确认版本号与群聊面板可用(建群、发言、流式、转待办仍可用);② Digger:`bash scripts/release.sh`(门禁 → 升版本 → 打包 → 签名校验 → 压缩),装到本机启动,确认版本号、`Digger.app/Contents/MacOS` 含 `digger`、`diggerd`、`digger-hook`、群聊面板可建群并显示聊天页、旧布局不丢。记录两个新版本号与产物路径。
- [ ] **Step 3: 合并到 `main`(快进,向用户确认后),清理 worktree 与分支,** 不碰主工作区未提交的改动。
- [ ] **Step 4: 汇报:** 各仓库测试通过数、对照清单结论(已修 / 已接受)、压力与容错结论、两个新版本号、哪些没验、Task 6 Step 2 的待裁决事项。

---

## Self-Review

- **Spec 覆盖:** §6.3 聊天内容 webview(协议、IPC、就绪握手、草稿恢复、声明式推送、失败壳)→ Task 2;`native_host.rs` 抽取(D4)→ Task 1;覆盖层 / 可见性 / 键盘 → Task 3;§7 的 Digger 真机、恶意输入、与 Dozer 对照 → Task 4、5;项目发布规定 → Task 7;文档与遗留 → Task 6。**不在本期:** 「转待办」(待办面板)、byteui 迁移。
- **类型一致:** `native_host::{Desired, Live, Action, plan, Allowed}` 在 Task 1 定义(源自 S7c 的 `review_native`),Task 2 引用;`GroupChatNative` / `Event` / `chat_bounds` 在 Task 2 定义;`desired_for_chat` 用库的 `group_chat_webview_spec`;`Overlay` 与 `overlays_open()` 衔接 S6d-3 / S7b / S8b。
- **风险点:** ① 抽 `native_host.rs` 会动 S7c 已落地的 `review_native`,以「既有测试条数不减且原样通过」为界;② 注入脚本的包装字节要与 Dozer 的 `dispatch_script` 一致(页面依赖 `window.__dozer.dispatch`),Task 2 的测试固定;③ 假 Agent 命令行必须按 `group_adapter` 的真实输出约定写,Step 1 的往返自测是为防止约定写错;④ `makeFirstResponder` 归还焦点的具体调用依赖 Digger 现有 macOS 宿主代码,实现者若找不到可复用的入口先停下报告;⑤ `chat_bounds` 的圆角内缩取值以 Dozer 现状为准,table 测试固定;⑥ 流式中途 `diggerd` 死亡后消息停在 Running 的行为属于库 / 后端,Digger 只记账;⑦ 真实 Agent 抽检消耗额度由用户决定;⑧ 发布、合并都是对外动作,先确认。
