# S7c Digger 审阅详情 webview、Dozer 对照验收与发布 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 S7b 里「审阅详情即将可用」的占位换成真实的会话详情:点列表一行,右边(按右栏约定是内容块)用**同一份 `review-trace` 页面**显示时间线,支持「加载更多」;与 Dozer 用同一组夹具数据逐状态对照;压力与容错回归;**完成会话面板迁移并发布 Dozer 与 Digger 各一个新版本**(项目硬性要求)。

**Architecture:** 全部在 `digger-desktop`。`adapters/review_native.rs` 仿 `usage_native.rs`:`thread_local` 的单槽 `wry::WebView` + 代数(epoch)防过期、`window::run` 里 `build_as_child`、自定义协议 `digger://review-trace/…` 调库的 `serve_review_trace`、IPC 里用库的 `parse_review_trace_event` 并**核对页面 URL 里的 `_r` 与当前 nonce**。是否创建/导航/摆位/隐藏/销毁由一个纯函数 `plan(...)` 决定(可测);协议应答的数据指向「当前项目」的 `review_snapshot`,切项目时先清再换。内容块里的 iced 部分(面板头、空/加载中/出错兜底、「加载更多」)由库的 `review_pane` 提供。对照验收用同一组 transcript 夹具喂给两个产品(各自独立的 `HOME` / 数据根)。

**Tech Stack:** Rust、iced 0.14、wry、`dozer-panel-conversations`(S7a)、tokio、byteui。

**Spec:** `docs/superpowers/specs/2026-10-18-conversations-panel-s7-design.md`(§6.3 审阅 webview、§6.4 S7c、§7、§8)。前置:S7a、S7b 已合并,S6d 系列已合并(`menu_overlay`、覆盖层互斥、`Overlay` 枚举)。

## Global Constraints

- **库不改、页面不改。** `review-trace` 的 HTML/JS/CSS 用库里提交的产物;严格 CSP 不放宽;页面不得请求任何外部资源(导航处理器只放行 `digger://review-trace/`,新窗口与下载一律拒绝)。
- **数据只经协议应答注入**,页面不 `fetch` 本地文件;`data.json` 没有快照时返回 404(与 Dozer 现状一致,S7a 黄金已记录)。
- **webview 永不盖住覆盖层或停在不该出现的地方:** 以下任一成立就隐藏(离屏停放或销毁,见 Task 1):会话面板不可见(切走、收起、被另一侧放大盖住)、任一覆盖层打开(Agent 筛选、选择器、溢出菜单、确认框)、窗口正在关闭、未 `DocumentLoaded`(离屏停放,避免 hidden 下 rAF 被挂起拖慢首帧)、出错或条目为空(销毁,显示 iced 兜底文案)。
- **单槽语义:** 任一时刻只有一个审阅 webview;它显示的是「当前聚焦项目」的详情;切项目页签、关项目页签、项目的详情被清空时必须同步销毁或换数据,不得短暂显示别的项目的内容。
- 缩放:创建时与 `icon_size::scale()` 变化时调用 `webview.zoom(scale)`(Dozer 同款,保证字号随 UI 缩放)。
- 失败不静默:webview 创建失败、协议异常显示在面板顶部通知条(沿用 S7b),不留空白块。
- 对照验收的「已接受差异」只允许 S6d 记过的两条(悬停阶跃、延迟提示精度)与 Task 4 记账时经用户确认的;其余必须修。
- **发布要求(项目硬性规定):** 会话面板迁移在 Dozer 与 Digger 都落地后,Dozer 与 Digger 各重新发布一个版本(Dozer:`scripts/build-macos-app.sh release`;Digger:`scripts/release.sh`),装到本机启动确认。升版本、提交版本号、安装都是对外动作,**先向用户确认再执行**。
- 主工作区别人未提交的改动不动;worktree `../digger-s7c`、分支 `feat/conversations-review`(从已含 S7b 的 `main` 切出)。合并前向用户确认。

## Review Focus

1. **加载状态机:** 点行 → 加载中 → `DocumentLoaded` → 显示;连点两行(旧页面的 `DocumentLoaded` 晚到)不得让新内容提前可见(核对 `_r`);加载失败/条目为空不得永远停在「加载中」;「加载更多」追加后页面重新导航且滚动位置合理;`nonce` 回绕。→ Task 1、2。
2. **隐藏时机的遗漏:** 打开任一覆盖层、切走面板、最小化、全屏进出、窗口缩放、拖分隔线、收起/展开列表时 webview 的位置与可见性始终正确;面板刚变为可见的第一帧不闪旧内容。→ Task 1、3。
3. **跨项目串内容:** 项目 A 的详情打开时切到 B(B 没有详情 → 销毁或空态)、再切回 A;A 的详情加载中途关闭 A 的页签;两个项目同时有详情状态。→ Task 2、3。
4. **大数据与恶意内容:** 5 000 回合、单回合数 MB、含 `<script>`/HTML/`javascript:` 链接/超长无空格行/零宽与 RTL 字符的会话文本——页面不执行、不溢出布局、界面不卡死;CSP 违规不静默。→ Task 5。
5. **与 Dozer 的外观差异:** 同一份夹具数据在两个产品里逐状态对照,差异逐条记账。→ Task 4。

---

### Task 1: 审阅 webview 宿主 `adapters/review_native.rs`

**Files:**
- Create:`crates/desktop/src/adapters/review_native.rs`
- Modify:`crates/desktop/src/adapters/mod.rs`、`crates/desktop/src/adapters/conversations.rs`(持有 `ReviewNative`)、`crates/desktop/src/product.rs`(`Msg::ReviewNative(Event)`、`boot` 接流、`sync_native` 里调用、关闭路径销毁)
- Test:`review_native.rs`(纯函数 `plan`、`loaded_matches`、数据切换)

**Interfaces:**
- Produces:
  - `review_native::Desired { pub visible: bool, pub rect: LogicalRect, pub spec: Option<ReviewWebviewSpec>, pub project_id: Option<i64> }`(`visible` = 面板可见 && 无覆盖层 && 未在关闭;`spec` 来自库的 `review_webview_spec(rv, locale, "digger")`)。
  - `review_native::Action { Destroy, Create { url: String }, Navigate { url: String }, SetBounds(LogicalRect), Park, Show(LogicalRect), Nothing }` 与纯函数 `plan(current: Option<&Live>, desired: &Desired) -> Vec<Action>`:无 `spec` 或 `!visible` → `Destroy`(已有实例时);有 `spec` 且无实例 → `Create`;`spec.url` 与实例当前 URL 不同 → `Navigate`;`spec.park_offscreen`(未 loaded)→ `Park`(矩形移到可见区外,如 `x = -10_000`);`spec.visible` 且矩形变化 → `SetBounds`/`Show`;`project_id` 与实例所属项目不同 → **先 `Destroy` 再 `Create`**(不复用实例,避免短暂显示别项目内容)。
  - `review_native::loaded_matches(page_url: &str, expected_nonce: u64) -> bool`:解析页面 URL 查询里的 `_r`,等于 `expected_nonce` 才算本次导航的 `DocumentLoaded`;解析失败为 `false`。
  - `Event { Built(u64, Result<(), String>), Loaded { generation: u64, nonce: u64 }, Done }`;`ReviewNative::{new, take_receiver, update(event) -> Option<(i64, ())>, sync(id, desired, data: Arc<Mutex<Option<String>>>) -> Task<Event>}`:`sync` 把 `plan` 的动作翻译成 `window::run` 里的 wry 调用(同 `usage_native.rs` 的 epoch 防过期套路);协议闭包持有 `Arc<Mutex<Arc<Mutex<Option<String>>>>>`(外层可换),切项目先把内层置 `None` 再换成新项目的 `review_snapshot`;IPC 处理器 `parse_review_trace_event(body)` + `loaded_matches(request.uri(), nonce)` 通过才发 `Event::Loaded`。
- Consumes:库的 `review_webview_spec`、`serve_review_trace`、`parse_review_trace_event`、`ReviewWebviewSpec`;S6d 的 `Overlay`。
- [ ] **Step 1: 写失败测试:** `plan_destroys_without_a_spec_or_when_not_visible_and_does_nothing_if_nothing_exists`;`plan_creates_once_then_navigates_only_when_the_url_changes`;`plan_parks_offscreen_until_loaded_then_shows_at_the_rect`;`plan_destroys_then_creates_when_the_project_changes`(不得出现「Navigate」跨项目复用);`plan_sets_bounds_only_when_the_rect_changed`;`plan_hides_when_any_overlay_is_open_or_the_window_is_closing`;`loaded_matches_requires_the_current_nonce_and_rejects_garbage_urls`(`_r=3` 对 3 真、对 4 假;无 `_r`、`_r=abc`、超长 URL、非 UTF-8 百分号编码均为假,不 panic);`late_loaded_from_an_older_generation_is_ignored`;`protocol_serves_404_for_data_json_when_the_snapshot_is_none_and_the_json_when_set`;`switching_the_project_clears_the_served_data_before_swapping`;`navigation_handler_allows_only_the_review_trace_origin`(`digger://review-trace/x` 通过;`https://…`、`file://…`、`digger://other/…`、`javascript:` 拒绝);`new_window_and_download_requests_are_denied`。
- [ ] **Step 2: 运行确认失败,实现。** 协议注册名 `digger`;主机名 `review-trace`;应答头 `Content-Type` 取库返回的 mime;404 用空体。创建后立即 `zoom(icon_size::scale())`,`product.rs` 在缩放变化的消息里调一次 `ReviewNative::rezoom`。
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop review_native`;`cargo build -p digger-desktop`。
- [ ] **Step 4: 提交。**

### Task 2: 内容块、点行打开与「加载更多」

**Files:**
- Modify:`crates/desktop/src/adapters/conversations.rs`(`review_pane`、`desired_for_review`)、`crates/desktop/src/adapters/conversations_display.rs`(`ConversationsPane::Review` 的度量)、`crates/desktop/src/product.rs`(`Msg::ReviewNative`、`sync_native` 里算 `Desired`)
- Create:`crates/desktop/src/adapters/review_geometry.rs`
- Test:`review_geometry.rs`、`conversations.rs`、`product.rs`

**Interfaces:**
- Produces:
  - `review_geometry::review_bounds(block: Rect, scale: f32) -> Rect`:内容块矩形减去面板头与分割线占的高度(与 Dozer `review_content_pane` 的头部同高:`tab_button_size` + 内边距 + 1px 分割线),左右各内缩 8、下缘内缩 8(对照 Dozer `webview_geometry::content_pane_bounds_for` 的 `PanelKind::Conversations` 分支:`y = top_bar + margin.top + preview_chrome_top_px`、`x = 列 x + 8`、`w = 列宽 − 16`);结果宽高钳到至少 1,不为负。
  - `ConversationsPanel::review_view(&self, project, area) -> Element<Msg>`:库的 `review_pane`(头部只有折叠按钮,**没有「生成总结」**;三态:空 → 提示「点击列表中的一条会话开始审阅」文案、加载中 → 曲线动画、就绪 → 为 webview 留空的底板;出错/条目为空 → iced 文案),列表块收起时内容块独占整宽。
  - `ConversationsPanel::desired_for_review(&self, project: Option<&ProjectInfo>, block: Rect, overlays_open: bool, panel_visible: bool, closing: bool, locale: &str) -> review_native::Desired`。
- [ ] **Step 1: 写失败测试:** `review_bounds_matches_dozers_numbers_for_a_known_window`(对 1200×800、默认分割、右栏:Digger 的 `review_bounds` 与 Dozer `content_pane_bounds_for(PanelKind::Conversations, ..)` 的结果相差不超过 1 像素;期望值由实现者在 Dozer worktree 里用该函数算出后写死,附算式与窗口/分割参数);`review_bounds_is_never_negative_for_tiny_blocks`;`review_bounds_follows_the_scale`;`clicking_a_row_opens_the_detail_and_shows_loading_until_loaded`(假 `ConversationService` 返回回合 → 状态 `Loading` → `Loaded` 事件后 `Ready`);`two_quick_clicks_show_only_the_second_rows_detail_even_if_the_first_loaded_event_arrives_last`;`load_more_appends_and_renavigates_with_a_new_nonce`;`load_more_button_is_visible_when_entries_exist`(核对 Dozer 现状:`load_more_button` 在 webview 存在时是否真的可见;**若 Dozer 里它实际被 webview 盖住,按现状保持并写进对照记账,不在 Digger 里改**);`empty_entries_or_error_show_the_fallback_text_and_no_webview`;`no_summary_button_in_the_review_header`;`closing_the_project_that_owns_the_detail_destroys_the_webview`;`switching_to_a_project_without_a_detail_destroys_and_shows_the_empty_state`;`switching_back_shows_the_previous_projects_detail_again`(状态保留、重新创建 webview 并 `Navigate` 到其 nonce)。
- [ ] **Step 2: 运行确认失败,实现。** `Msg::Conversations(pid, SessionOpen(..))` 已由库处理;`ReviewTraceLoaded` 由 `Event::Loaded` 经 `ReviewNative::update` 映射到 `Msg::Conversations(pid, ReviewTraceLoaded)`。
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop`;沙箱真机:用 S7b 的伪造 transcript 导入后点开一条,时间线显示,「加载更多」可用,截图。
- [ ] **Step 4: 提交。**

### Task 3: 覆盖层、面板可见性与窗口事件对 webview 的影响

**Files:**
- Modify:`crates/desktop/src/product.rs`(`sync_native` 的可见性输入、`on_shell` 里的切走处理)、`crates/desktop/src/adapters/conversations.rs`
- Test:`product.rs`

**Interfaces:**
- Produces:`ProductShell::overlays_open(&self) -> bool`(Agent 面板的选择器/溢出菜单/确认框、会话面板的 Agent 筛选菜单、项目删除确认卡中任一为真);`ProductShell::review_visible(&self) -> bool`(= `panel_visible(conversations_id)`)。`sync_native` 每次都把二者喂进 `desired_for_review`。
- [ ] **Step 1: 写失败测试:** `opening_the_agent_filter_menu_hides_the_review_webview_and_closing_it_restores_it`;`opening_the_agent_picker_or_overflow_menu_or_confirm_hides_it_too`(面板虽不同,但覆盖层全局互斥且会盖住右栏);`switching_to_another_right_panel_destroys_it`;`collapsing_the_right_zone_or_maximizing_the_left_one_destroys_it`;`window_closing_destroys_it_before_shutdown`;`the_first_frame_after_becoming_visible_is_parked_not_shown_until_loaded`(Review Focus 2);`dragging_the_divider_moves_the_webview_with_the_block`(连续 20 次不同矩形,每次 `SetBounds` 的矩形等于当次 `review_bounds`);`list_collapse_expands_the_review_to_the_full_width`。
- [ ] **Step 2: 运行确认失败,实现,验证,提交。**

### Task 4: 夹具、画廊与 Dozer 逐状态对照

**Files:**
- Create:`crates/desktop/tests/fixtures/conversations/*.jsonl`(Claude 形状的 transcript 夹具,见下)、`crates/desktop/examples/conversations_gallery.rs`、`scripts/seed-conversations-fixture.sh`、`docs/superpowers/notes/2026-10-18-conversations-parity.md`
- Test:无(人工对照为主)

**Interfaces:**
- Produces:夹具集(全部为自造数据,不含真实用户内容):`short.jsonl`(3 回合)、`tools.jsonl`(含 thinking、多个 tool_use/tool_result、token 用量)、`long.jsonl`(500 回合,用于「加载更多」与滚动)、`huge.jsonl`(5 000 回合,压力用)、`hostile.jsonl`(`<script>`、HTML 片段、`javascript:` 链接、10 万字符无空格行、零宽/RTL 字符、未闭合 Markdown 围栏)、`empty.jsonl`(只有系统行,折叠后无条目)、`broken.jsonl`(含坏 JSON 行)。`seed-conversations-fixture.sh <HOME>` 把它们按 Claude 的目录约定(`<HOME>/.claude/projects/<编码后的项目路径>/*.jsonl`)写进指定的临时 `HOME`,并打印项目路径,**两个产品都用这个临时 `HOME` 与各自独立的数据根**,于是看到的是同一组数据。
- [ ] **Step 1: 写夹具与种子脚本。** 脚本不碰真实 `HOME`(缺参数拒绝运行,目标目录必须位于 `$TMPDIR` 下)。
- [ ] **Step 2: 画廊。** `conversations_gallery --state <name>` 不连守护进程,用假 `ConversationService` 预置会话与回合,摆出:`empty`、`list-only`、`list-and-detail-loading`、`list-and-detail`、`detail-error`、`detail-empty-entries`、`long-detail`、`import-prompt`、`daemon-unavailable`、`agent-filter-open`;用于 Digger 一侧的截图与快速迭代。
- [ ] **Step 3: 逐状态对照。** 用种子脚本准备临时 `HOME`;Dozer 用它启动并打开同一项目路径(Dozer 打开项目时会静默补录),Digger 启动后点「导入历史」;对 Task 4 画廊列出的每个状态,在两个产品里摆出同一状态并截图,并排比较。差异逐条记进 `conversations-parity.md`:`状态 | 差异 | 原因 | 处理(修 / 接受并写理由)`。Dozer 一侧由用户操作,我给出每个状态的摆法清单并截图 Digger 一侧。
- [ ] **Step 4: 修差异,重复 Step 3 直至没有「待修」。** 度量只改 Digger 一侧(`pane_style`、`review_geometry`),不改库。
- [ ] **Step 5: 提交(含对照文档与关键截图路径)。**

### Task 5: 压力、容错与同机并存回归

- [ ] **Step 1: 大数据**(沙箱;观察界面是否卡顿、CPU、内存):打开 `huge.jsonl` 的详情;连续点「加载更多」到底;`hostile.jsonl` 打开后页面不执行脚本、不出现外部请求(用系统代理日志或 wry 导航/请求回调计数确认)、布局不溢出;`broken.jsonl` 与 `empty.jsonl` 显示兜底文案而不是空白或永远「加载中」。卡顿先判断是页面/库(Dozer 同样会卡)还是 Digger 摆位逻辑,**库与页面的问题报告给用户,不在 Digger 里加绕过**。
- [ ] **Step 2: 快速操作:** 连续点 20 次不同行;点行的同时切项目页签;加载中关闭项目页签;加载中打开/关闭 Agent 筛选菜单;反复折叠/展开列表;拖分隔线;缩放窗口;最小化再恢复;进出全屏。全程无崩溃、无残留 webview(用 `lifecycle_selftest` 的探针读真实 NSView 状态,参照 `usage_native::probe_view` 增一个审阅版本,确认关闭面板后实例数为 0)。
- [ ] **Step 3: 守护进程中途死亡:** 详情已打开时 `kill diggerd`:当前详情保持可读(已加载内容在 webview 里)、列表显示不可用原因、点其它行给出可读错误、重启后「重试」恢复。
- [ ] **Step 4: 同机并存:** Dozer 与 Digger 同时运行并各开同一项目,各自只看到自己库里的会话;Dozer 里新增的会话不会出现在 Digger 的列表里(反之亦然)。
- [ ] **Step 5: 记录** 每项结果、截图路径、发现的问题与处置,写进 `docs/superpowers/notes/2026-10-18-conversations-regression.md`。任一项不符先停下,不要为了「过」而改验证。

### Task 6: 文档与遗留缺口

- [ ] **Step 1:** `docs/superpowers/specs/2026-10-18-conversations-panel-s7-design.md` 状态改「已实现」并补「实现结果」(含对照结论、已接受差异、D9 的标记文件取舍);`2026-10-14-window-shell-design.md` 的 S7 行同步;`docs/adr/ADR-015-product-owned-daemon.md` 补一句会话面板读写的是各自产品的库。
- [ ] **Step 2: 遗留缺口交给用户裁决(不自行决定):** ① Digger 删除项目不清理该项目的会话——`TranscriptService::delete_project_transcripts(cwd)` 在 Dozer 里会同时删除用户 home 下的 transcript 文件(`delete_project_transcripts_in(home, cwd)`),把它接进 Digger 的删除流程是**破坏性**动作,需要用户明确批准范围(只清 `digger.db` 里的副本,还是也删原文件)才能做;② 总结生成在 Digger 的开通(费用);③ 页签部件、菜单弹层、`review_native` 的 wry 样板迁入 byteui / bytehost 共用的跨仓库任务。三条写进 `docs/superpowers/notes/2026-10-18-conversations-open-items.md`。

### Task 7: 门禁、发布与合并

- [ ] **Step 1: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`check-host-deps.sh`(含 `--self-test`)、`check-shared-deps.sh`、`check-shell-regions.sh`、Cargo.lock 重复检查。
- [ ] **Step 2: 发布 Dozer 与 Digger 各一个新版本。** 先向用户确认,再执行:① Dozer:走它自己的发布流程(`scripts/build-macos-app.sh release`:升版本号 → 门禁 → 打包),装到本机启动,确认版本号与会话面板可用;② Digger:`bash scripts/release.sh`(门禁 → 升版本 → 打包 → 签名校验 → 压缩),装到本机启动,确认版本号、`Digger.app/Contents/MacOS` 含 `digger`、`diggerd`、`digger-hook`、会话面板可打开详情、旧布局不丢。记录两个新版本号与产物路径。
- [ ] **Step 3: 合并到 `main`(快进,向用户确认后),清理 worktree 与分支,** 不碰主工作区未提交的改动。
- [ ] **Step 4: 汇报:** 各仓库测试通过数、对照清单结论(已修 / 已接受)、压力与容错结论、两个新版本号、哪些没验、Task 6 Step 2 的三条待裁决事项。

---

## Self-Review

- **Spec 覆盖:** §6.3 审阅 webview(摆位、协议应答、nonce 重新导航、离屏停放、面板不可见/覆盖层/关闭时隐藏)→ Task 1–3;「加载更多」→ Task 2;§6.3 键盘无需新代码(搜索框由 `text_input` 捕获,S7b 已覆盖);D1(同一份页面)、D7(scheme 参数,Digger 传 `digger`)→ Task 1;§7 的 Digger 真机与同机并存、与 Dozer 对照 → Task 4、5;项目发布规定 → Task 7;文档与遗留 → Task 6。**不在本期:** 总结生成开通、删除项目清会话(待裁决)、byteui 迁移。
- **类型一致:** `Desired` / `Action` / `Live` / `plan` / `loaded_matches` / `Event` 在 Task 1 定义,Task 2、3 引用;`review_bounds` 在 Task 2 定义;`ReviewWebviewSpec` / `review_webview_spec` / `serve_review_trace` / `parse_review_trace_event` 来自 S7a;`Overlay` 与 `overlays_open` 衔接 S6d-3 / S7b 的覆盖层枚举。
- **风险点:** ① `_r` 校验依赖 wry IPC `Request::uri()` 返回页面 URL,若实现者发现它不是(例如返回空),退路是在导航时把 nonce 记进 `Live` 并在每次 `Loaded` 前核对**当前期望 nonce 与实例上次导航的 nonce 一致**,仍不可靠时停下报告;② `load_more_button` 在 webview 存在时是否可见,Dozer 现状我没有实测,Task 2 已写成「照现状、记账」;③ 用 `HOME` 隔离 Dozer 依赖其数据目录随 `HOME` 变化(macOS 的 `ProjectDirs` 读 `$HOME`),实现者先验证再用;④ wry 的离屏停放坐标与多显示器、Retina 的交互只能真机看;⑤ `zoom` 的 API 在 wry 0.55 上的行为沿用 Dozer 的调用;⑥ 删除项目清会话是破坏性动作,明确留给用户裁决;⑦ 发布、合并都是对外动作,先确认。
