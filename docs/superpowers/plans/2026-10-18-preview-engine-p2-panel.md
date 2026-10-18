# S5b-2 P2 预览面板库 `dozer-panel-preview`:状态、界面与非 webview 编排 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Dozer 里新建共享库 `dozer-panel-preview`,把预览引擎的第 2 层搬进去:`PreviewPane` / `PreviewTab` / 查找与大文件搜索状态(`state.rs` 868 行、`view.rs` 里 `impl PreviewPane` 约 2 900 行及其约 4 300 行测试)、`tabular` 数据模型、由 `workspace/view.rs` 承载的 iced 界面(页签条、工具栏与模式切换按钮、查找条、大文件搜索条、溢出菜单、加载中视图、降级页、冲突条、空白信息卡),以及**不碰 webview / 不碰 Agent 命令轮询**的那部分打开 / 加载 / 恢复 / 关闭编排;引入 `PreviewCapabilities`(由产品注入哪些后端启用)。Dozer 改为薄调用,**用户可见行为不变**。Digger 本期**只同日升级钉版**(见 D2)。不发布产品版本。

**Architecture:** 库依赖 P1 的 `dozer-preview-core`(路由、画像、策略、加载、资源、恢复、保存)。`crate::app::PanelKind`(`view.rs` 里 68 处)换成核心的 `ViewerPanel`;`crate::capabilities::current()` 全局换成显式传入的 `&ClientCapabilities`(P1 已铺好);`crate::tabular` 整个模块搬进库(数据模型 + 展示转换 + 列宽估算,约 770 行),加载器 `load` / `load_sheet` 依赖的 `calamine` / `csv` 放在可选特性 `tabular-loader` 后面(Dozer 开,Digger 关,数据类型始终编译)。库拥有 `Message` / `Effect` / `update`(第一个字段带面板种类 `ViewerPanel`,取代今天 Files / Project 各一套 `Preview*` / `ProjectPreview*` 顶层消息)、`run_effect`(只做本地文件 IO 与 `tokio` 任务)、`view::*`(经 `PanelDisplay<M>` 注入页签部件、面板头、区域度量、悬停、`locale`、可用宽度)。**webview 的清单产出(`desired_*_webviews`)留在库里(纯数据),真正的 `wry` 池、`webview_geometry`、协议应答(`assets.rs` / `webview_protocol.rs` 的编解码与应答)、编辑器 / flyfish / JSON / 表格 / PlantUML 宿主脚本推送都留在 `dozer-app`,等 P3。**

**Tech Stack:** Rust、iced 0.14、byteui、`dozer-preview-core`(P1)、`dozer-i18n`、`dozer-panel-host`、`dozer-model`、tokio、serde;可选 `calamine` / `csv`。

**Spec:** `docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md`(§2 现状四层、§5.1、§5.3 能力开关、§6 P2、§7、§8)。前置:P1 已合并(`dozer-preview-core` 存在);S6c-2 / S7a 的「`PanelDisplay` 注入页签四件」做法是本期界面注入的模板。

## Global Constraints

- **依赖闭包:** `cargo tree -p dozer-panel-preview`(默认特性)不得出现 `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`、`wry`、`winit`、`calamine`、`csv`;开启 `tabular-loader` 后才允许出现后两者。
- **Dozer 用户可见行为不变;** 搬走的测试随代码走、不删、不改期望值,抽库前后 Rust 测试总数守恒(`preview/view.rs` 的约 4 300 行测试、`preview/state.rs`、`tabular` 的测试、`workspace/view.rs` 里 `review_*` 以外的预览测试);库测试在默认特性下**必须能跑**,依赖 `tabular::load` 的几十条测试用 `--features tabular-loader` 运行(Dozer 的工作区测试开启该特性,所以总数照算)。
- **路由表不删减、`classify_preview` 输出不变**(P1 已固定);`PreviewCapabilities` 只改变「产品对某类型是否启用后端」,不改变路由。
- **安全约束原样保留(P3 才会真正落地宿主,但数据类型本期搬迁):** webview 绑定 `(project_id, panel, tab_id, path)`;路径只来自 Rust;保存目标必须等于绑定路径。本期搬迁不得放宽任何校验函数。
- **Files 与 Project 两个面板各一份 `PreviewPane`,状态互不串**;库的 `Message` 带 `ViewerPanel`,Dozer 适配层映射回今天的两套顶层消息语义(行为等价)。
- 只用 git tag 依赖;Dozer 内部 `path = "../.."` 除外。**合并、切 tag、推送是对外动作,向用户确认后执行。** 不发布产品版本(发布在 P4 收尾)。
- **主工作区别人未提交的改动不动;** worktree `../dozer-panel-preview`、分支 `feat/panel-preview-lib`(从已含 P1 的 `main` 切出)。
- 不引入新的日志依赖;库内日志用 `tracing` 薄封装。
- **与设计的取舍(我的决定):**
  - **D1 切线规则。** 凡函数体只碰 `PreviewPane` / 核心类型 / 本地文件系统 / `tokio` 任务 → 进库;凡碰 `wry`、`webview_geometry`、协议应答、Agent 命令轮询(`spawn_preview_command_poller`、`apply_preview_command`、`resolve_preview_command_target`)、预览上下文推送(`spawn_preview_context_push` 系列,服务于 MCP)、`dozerd` 通知 → 留在 `dozer-app`,P3 / S6e-3 再处理。Task 1 Step 3 的函数清单按此规则逐个判定,写进提交说明;**拿不准的先停下报告,不要猜**。
  - **D2 Digger 本期只钉版。** 设计 §6 写「P2 Digger 接入但先不挂宿主 webview(能显示页签与降级页)」,但 Project / Files 在 Digger 里要到 P4 才有配对布局,P2 时 Digger 没有任何地方可以显示预览页签;现在接入只会留下不被渲染的死代码。所以 Digger 的状态持有、适配层、布局变化与宿主合并到 P3 / P4 做,P2 只同日升级钉版。请你审阅这一条偏离。
  - **D3 `tabular` 数据模型进库、加载器放特性后面**,而不是留在 `dozer-app` 里用 trait 对象隔离(那会让 `TabularState` 在库里失去具体类型,pane 的几十条表格测试无法随走)。
  - **D4 `PreviewTab` 里和 Agent / MCP 有关的字段**(预览上下文推送所需的光标 / 选区缓存等,若存在)随 `PreviewTab` 一起搬,只是字段;推送逻辑留在 `dozer-app`(D1)。

## Review Focus

1. **加载状态机与代数防过期:** 过期的 `PreviewProfiled` / 超时 / `WindowIndex` / 恢复读写结果被丢弃;`reserve` 被拒绝后的降级;同一路径连续打开两次只留一个页签;`begin_load` / `advance_load` / `finish_load` / `fail_load` / `cancel_load` 的所有迁移;页签在加载中被关闭。(随代码搬走的测试覆盖,另补乱序组合。)→ Task 3、4。
2. **脏页签与数据保护:** 关闭脏页签前先保存、冲突时「保留我的修改 / 丢弃并重载」、外部修改检测(`disk_mtime`)、保存失败不丢缓冲;搬迁不得改变任何一条的语义。→ Task 3、4。
3. **视图模型与格式化:** 页签标题(`Blank` 占位用项目根目录名、脏页签加 ` *`;现状没有同名消歧)、页签宽度估算、空白信息卡的大小 / 时间 / 千分位格式(中英文、0 字节、超大值、未来时间、纪元时间)、降级页文案按 `PreviewKind` × `PreviewCapabilities` 的全组合。→ Task 1、5。
4. **能力开关穷举:** 对 `PreviewKind` 的每个变体 × `PreviewCapabilities` 各特性开关组合:启用 → 原后端;关闭 → `Unsupported` / `External` 降级页与「用系统应用打开」动作;不 panic;路由结果不变。→ Task 4。
5. **两个面板的隔离与消息映射:** Files 与 Project 同时有多个页签、同时加载、同时查找时互不影响;Dozer 适配层把库消息映射回原顶层消息后,每一条消息的目标面板正确(用全组合表测试固定)。→ Task 6。

---

## Dozer(仓库 `../dozer`,worktree `../dozer-panel-preview`,分支 `feat/panel-preview-lib`)

### Task 1: 基线、清单与现状上的视图模型测试

**Files:**
- Modify:`crates/dozer-app/src/workspace/view.rs`(把可测的纯函数从视图函数里抽出,先在原地)
- Test:`crates/dozer-app/src/workspace/view.rs`、`crates/dozer-app/src/preview/view.rs`

- [ ] **Step 1: 记录基线。** worktree 里 `cargo test --workspace 2>&1 | grep "^test result"`;再单独记 `cargo test -p dozer-app preview::` 与 `workspace::view` 的通过数;记 `grep -c "#\[test\]"` 对 `preview/state.rs`、`preview/view.rs`、`tabular/mod.rs`、`workspace/view.rs` 的值。
- [ ] **Step 2: 抽出视图模型(单独一个提交,不搬位置)。** 在 `workspace/view.rs` 内把下列逻辑从 iced 构造函数里抽成纯函数 / 小结构体并让原函数改用它们:`preview_tab_display_title`(已纯)与 `preview_tab_display_width`;`BlankInfoModel`(= `preview_blank_info_card` 里的文件名、类型、大小、修改时间、路径等行的文案,含 `format_human_size`、`with_thousands_separator`、`format_time`、`month_name_en`、`civil_from_days`);`FallbackModel`(= `preview_fallback_page` 里按 `PreviewKind` / 错误 / 能力选择的标题、说明、按钮集合);`ConflictBarModel`;`ToolbarModel`(哪些模式切换按钮出现:渲染 / 代码、表格网格 / 原文、JSON 树 / 原文,取决于 `wry_toggle_eligible` 等)。
- [ ] **Step 3: 写表驱动测试固定现状**(期望值按现状手推并写死,抽取前后都要过):`tab_label_and_title_rules_are_stable_for_blank_dirty_and_cjk`;`blank_info_formats_sizes_times_and_thousands_for_zh_and_en`(0 字节、1023 / 1024、数 GB、`u64::MAX`、纪元时间、未来时间);`fallback_model_for_every_preview_kind`(遍历 `PreviewKind` 的**每个**变体 × 能力全开 / 全关);`toolbar_model_shows_mode_buttons_only_for_eligible_tabs`;`conflict_bar_model_for_dirty_and_clean_tabs`。
- [ ] **Step 3b: 在改任何代码之前写 `WebviewSpec` 黄金。** `preview_webview_specs_match_the_golden`:构造若干组 `PreviewPane` 状态(Files / Project 各自:空、一个代码页签、一个渲染型页签、一个 JSON 页签、一个表格页签、多页签含挂起 / 加载中 / 冲突态),对 `desired_webviews` / `desired_editor_webviews` / `desired_json_webviews` / `desired_tabular_webviews` 的输出(`id`、`url`、`visible`、`park_offscreen`、绑定)序列化后与 `crates/dozer-app/tests/fixtures/preview_webview_specs_golden.txt` 逐字比较(`UPDATE_GOLDEN=1` 仅此一次,之后字节不得变)。
- [ ] **Step 4: 函数清单(D1 的判定表)。** 输出 `docs/superpowers/notes/2026-10-18-preview-p2-inventory.md`:逐个列出 `workspace/state.rs`、`app/update.rs`、`app/app.rs` 里名字含 `preview` 的函数(约 60 个,起点见 `grep -n "fn .*preview"`),每个写 `进库 / 留 dozer-app(原因)/ 拆开(怎么拆)`。**这份清单是 Task 3–5 的施工图,交给用户过目后再继续。**
- [ ] **Step 5: 验证并提交。**

### Task 2: 建库、搬 `tabular`、能力类型

**Files:**
- Create:`crates/dozer-panel-preview/{Cargo.toml,src/{lib.rs,capabilities.rs,i18n.rs,log.rs},locales/{en,zh-CN}.json}`、`crates/dozer-panel-preview/src/tabular/`(从 `crates/dozer-app/src/tabular/mod.rs` `git mv`)
- Modify:workspace `Cargo.toml`、`crates/dozer-app/Cargo.toml`(依赖库并开启 `tabular-loader`;`calamine` / `csv` 改由库带入)、`crates/dozer-app/src/tabular/mod.rs`(`pub use`)、`scripts/audit/check_panel_boundary.py`、`scripts/i18n/*.py`
- Test:`tabular` 随代码走

**Interfaces:**
- Produces:`capabilities::PreviewCapabilities { pub code: bool, pub rendered: bool, pub json: bool, pub streamed: bool, pub tabular: bool, pub plantuml: bool, pub pdf: bool, pub media: bool }`(`Default` 全 false,`::all()` 全 true;字段与 P1 路由产出的 `PreviewKind` / 子类型一一对应,以 `router.rs` 为准);`capabilities::backend_enabled(kind: &PreviewKind, caps: &PreviewCapabilities) -> bool`;库 feature `tabular-loader`(默认关)。
- [ ] **Step 1: 写失败测试:** `capabilities_default_is_all_off_and_all_is_all_on`;`backend_enabled_covers_every_preview_kind_variant`(用穷举 `match` 写成测试辅助,新增变体编译即报错);`tabular_data_model_compiles_and_works_without_the_loader_feature`(默认特性下 `TabularView::new` / 列宽估算 / 展示转换的既有测试仍可运行;仅依赖 `load*` 的测试加 `#[cfg(feature = "tabular-loader")]`)。
- [ ] **Step 2: 建 crate,搬 `tabular`。** `dozer-app` 里 `crate::tabular::*` 路径经 `pub use dozer_panel_preview::tabular::*` 保持不变;`load` / `load_sheet` / `SheetLoadRequest` 的可见性按原样。
- [ ] **Step 3: 验证。** `cargo test -p dozer-panel-preview`(默认)与 `--features tabular-loader` 的通过数之和与搬走的 `tabular` 测试数一致;`cargo tree` 约束;`cargo build --workspace`。
- [ ] **Step 4: 提交。**

### Task 3: 搬 `PreviewPane` 状态与全部窗格操作

**Files:**
- Create:`crates/dozer-panel-preview/src/{state.rs,pane.rs,find.rs,webview_spec.rs,tests/*}`
- Modify:`crates/dozer-app/src/preview/{state.rs,view.rs,webview.rs,mod.rs}`(缩成 `pub(crate) use` 或删除)、`crates/dozer-app/src/workspace/state.rs`(`preview_pane(kind)` 等转调)
- Test:随代码走的 `state.rs` / `view.rs` 测试(约 4 300 行)

**Interfaces:**
- Produces(原样搬,只改 `use`、可见性与 `PanelKind` → `ViewerPanel`):`PreviewTab`、`SaveGate`、`TabularState`、`PreviewRuntime`、`WindowedRuntime`、`ViewStateRestore`、`TabKind`、`WebviewFindAction`、`FindState`、`LargeFileSearch`、`PreviewPane` 及 `impl PreviewPane` 的全部公开方法(`open_path` / `open_path_provisional` / `push_shell_tab` / `select` / `close` / `close_by_id` / `reorder` / `begin_load` … `cancel_load` / `apply_profile` / `apply_recovery_restore` / `grant_reserve` / `mark_reserve_denied` / `suspend_tab` / `desired_webviews` / `desired_editor_webviews` / `desired_json_webviews` / `desired_tabular_webviews` / 查找系列 / 大文件搜索系列 / 冲突系列 / 表格与 PlantUML 事件应用等,**以清单为准,不增不减**);`webview_spec::{WebviewSpec, EditorHostBinding, …}` 中**纯数据**的部分(绑定、规格、事件枚举),编解码与应答函数留在 `dozer-app`(D1)。`find_field_id(panel: ViewerPanel)`、`take_find_focused`、`CaptureFindFocus`(iced 焦点探测,Dozer 键盘路由用)随库,`ViewerPanel` 由核心提供。
- [ ] **Step 1: 写失败测试**(库内新增,其余随代码搬、期望值不改):`opening_the_same_path_twice_keeps_one_tab_and_selects_it`;`close_clamps_the_active_index_and_never_panics_on_the_last_tab`;`reorder_out_of_range_is_a_noop`;`stale_generation_results_are_dropped_for_every_load_transition`(对 `finish_load` / `fail_load` / `advance_load` / `apply_profile` / `apply_recovery_restore` 各给旧代数);`closing_a_tab_while_loading_cancels_it_and_drops_its_late_results`;`two_panes_do_not_share_state`(Files 与 Project 各开页签、各查找,互不影响);`find_state_survives_tab_switches_and_resets_on_close`;`panelkind_to_viewerpanel_mapping_is_total`(在 `dozer-app` 的适配里,穷举 `PanelKind` 里和预览相关的变体)。
- [ ] **Step 2: 运行确认失败,搬代码与测试。** 搬迁用 `git mv` 保历史;每搬一个文件先让 `cargo build -p dozer-app` 通过再继续。`crate::capabilities::current()` 的 9 处调用改为参数传入的 `&ClientCapabilities`(由 `dozer-app` 在边界处取全局值传入);`crate::extensions::search::SearchHit`(大文件搜索结果)若只被预览用,类型搬进库,否则库里用泛型或最小等价类型并在适配层转换,**不得让库依赖 `dozer-app`**。
- [ ] **Step 3: 验证。** `cargo test -p dozer-panel-preview -p dozer-app` 总数守恒(加新增);`cargo tree` 约束;`python3 scripts/audit/check_panel_boundary.py`。
- [ ] **Step 4: 提交**(说明里列出 `dozer-app` 里被替换 / 删除的类型与函数清单)。

### Task 4: `Message` / `Effect` / `update`、能力开关与非 webview 编排

**Files:**
- Create:`crates/dozer-panel-preview/src/{message.rs,update.rs,effects.rs,adapter.rs}`
- Modify:`crates/dozer-app/src/workspace/state.rs`、`crates/dozer-app/src/app/{update.rs,app.rs,message.rs}`(按 Task 1 清单里标「进库」的函数转调)
- Test:库内 `update.rs`、`effects.rs`

**Interfaces:**
- Produces:
  - `message::Message`:第一类字段是 `ViewerPanel`;变体对应今天顶层的 `Preview*` / `ProjectPreview*`(`OpenPath`、`SelectTab`、`CloseTab`、`ToggleRenderMode`、`TabularTextModeToggle`、`JsonModeToggle`、`SaveActive`、`Retry`、`PlainTextOpen`、`OpenExternal`、`ConflictKeep`、`ConflictReload`、`Find{Open,OpenWithReplace,ReplaceToggle,Close,Text,Go,Case,Replacement,ReplaceCurrent,ReplaceAll,WebviewState}`、`LargeFileSearch{Close,Submit,Results,Progress,Go}`、`TabOverflow{Toggle,Dismiss}`、`BlankInfoLoaded`、`Profiled`、`LoadTimeout`、`WindowIndex`、`RecoveryWritten`、`RecoveryRead` 等)。**变体清单以 Task 1 的清单为准,不增不减;** 属于 webview 脚本推送 / Agent 命令的(`PreviewCommandsFetched`、各 `*WebviewEvent`)不进库。
  - `update::update(state: &mut PreviewPane, msg: Message, ctx: &UpdateCtx<'_>) -> Vec<Effect>`,`UpdateCtx<'a> { pub panel: ViewerPanel, pub project_root: &'a Path, pub caps: &'a PreviewCapabilities, pub client: &'a ClientCapabilities, pub now: Instant, pub locale: &'a str }`;`Effect { ProbeProfile { tab_id, generation, path }, ReadRecovery { .. }, WriteRecovery { .. }, SaveFile { tab_id, path, bytes }, LoadBlankInfo { path }, OpenExternal { path }, StartWindowIndex { .. }, ScheduleLoadTimeout { tab_id, generation, after: Duration }, … }`(**纯数据,仅含不涉及 webview 的效果**);`effects::run_effect(effect, emit: impl Fn(Message) + Send + 'static, handle: &Handle)` 做本地 IO 并回投结果消息。需要 webview 的动作(创建 / 销毁 / 推送脚本)由 `PreviewPane::desired_*` 的清单差异表达,不是 `Effect`。
  - 能力接入:`update` 在得到路由结果后,若 `!backend_enabled(kind, caps)`,把该页签置为 `Unsupported` / `External` 降级态(沿用现有降级界面的状态值),并**不**产生加载效果。
- [ ] **Step 1: 写失败测试**(假时钟、临时目录,每条独立):
  - `open_path_probes_the_profile_then_routes_by_the_profile`(`OpenPath` → `ProbeProfile` 效果;`Profiled` 到达 → 页签进入对应后端态);
  - `disabled_backends_degrade_to_unsupported_and_never_load`(对 `PreviewKind` 每个变体 × 每个开关,启用 → 进入原后端态,关闭 → 降级态且无加载效果);
  - `external_open_action_is_available_on_every_degraded_tab`;
  - `save_active_sends_the_buffer_to_the_bound_path_only`(目标路径不等于绑定路径 → 拒绝,无 `SaveFile` 效果);`save_failure_keeps_the_buffer_and_the_dirty_flag`;
  - `closing_a_dirty_tab_requests_save_first_and_close_follows_the_save_result`;`conflict_keep_and_reload_preserve_or_discard_exactly_as_before`;
  - `load_timeout_for_an_older_generation_is_ignored`、`load_timeout_marks_failed_once`;
  - `recovery_roundtrip_restores_tabs_once`、`recovery_read_for_a_closed_tab_is_ignored`;
  - `blank_info_loaded_for_a_closed_tab_is_ignored`;
  - `large_file_search_progress_and_results_for_an_old_query_are_dropped`;
  - `update_never_panics_on_messages_for_missing_tab_ids`(全部带 `tab_id` 的变体各发一次不存在的 id)。
- [ ] **Step 2: 运行确认失败,实现。** 把 Task 1 清单里「进库」的编排函数逐个落成 `update` 的分支 + `Effect`;留在 `dozer-app` 的函数改为调用库的 `update` 并执行效果(效果执行用库的 `run_effect`,webview 相关效果不存在)。每落一个先跑相关旧测试。
- [ ] **Step 3: 验证。** `cargo test -p dozer-panel-preview -p dozer-app` 总数守恒(加新增);`cargo clippy -p dozer-panel-preview --all-targets`。
- [ ] **Step 4: 提交。**

### Task 5: iced 界面(页签条、工具栏、查找条等)进库

**Files:**
- Create:`crates/dozer-panel-preview/src/view/{mod.rs,pane.rs,tabs.rs,toolbar.rs,find_bar.rs,large_search_bar.rs,overflow.rs,loading.rs,fallback.rs,conflict.rs,blank_info.rs,display.rs}`
- Modify:`crates/dozer-app/src/workspace/view.rs`(`preview_pane_for`、`preview_find_bar_widget`、`preview_large_file_search_bar_widget`、`preview_tab_overflow_popup`、`preview_loading_view`、`preview_fallback_page`、`preview_conflict_bar_widget`、`preview_blank_info_card` 删除,留转调)、`crates/dozer-app/src/app/view.rs`
- Test:库内 `view/*`

**Interfaces:**
- Produces:`display::PanelDisplay<M>`(产品注入,形状同 S6c-2 / S7a):`wrap(&self, ViewerPanel, Message) -> M`、`locale`、`hover_progress(PreviewHover) -> f32`、`hover_tooltip_ready(PreviewHover) -> bool`、`pane_style() -> PaneStyle`、页签四件(`panel_tab(TabArgs<M>)`、`overflow_button(count, on_press)`、`overflow_menu(entries, anchor, ...)`、`drag_surface(content, index, on_move)`)、`tab_bar_avail_px() -> f32`、`list_collapse_button(...)`(Files / Project 各自的收起按钮,若适用)、`tab_divider()`;`PreviewHover { TabItem(usize), TabClose(usize), TabOverflow, RenderMode, TabularMode, JsonMode, … }`。
  `view::pane<'a, M>(state: &'a PreviewPane, panel: ViewerPanel, tab_first: usize, error: Option<&'a str>, host: &'a impl PanelDisplay<M>, ctx: &ViewCtx<'a>, width: Length, outer: Border) -> Element<'a, M>` 及各子视图;`ViewCtx { pub locale, pub project_root, pub caps: &PreviewCapabilities, pub conflict_active: bool, pub find_over_webview: bool }`。
  **webview 的占位**:`view::pane` 在内容区只画 iced 兜底(加载中 / 降级 / 冲突 / 空白信息 / 原生编辑器占位)与 webview 要覆盖的**空白底板**;webview 本身由产品按 `desired_*_webviews` 摆放(P3)。
- [ ] **Step 1: 写失败测试:** 视图模型相关沿用 Task 1;`pane_builds_for_zero_one_and_fifty_tabs_without_panicking`;`tab_titles_use_the_disambiguated_form_and_close_buttons_send_close_for_the_right_index`;`render_mode_button_only_for_eligible_tabs_and_sends_toggle`;`find_bar_shows_replace_row_only_when_replace_is_open_and_sends_the_documented_messages`;`large_file_search_bar_builds_in_every_state`(空闲 / 进行中 / 有结果 / 无结果);`overflow_menu_has_one_entry_per_tab_and_marks_the_active_one`;`fallback_page_offers_the_system_open_action_for_every_degraded_kind`;`loading_view_present_for_every_active_stage_absent_at_idle`(沿用现有 `loading_view_*` 两条);`conflict_bar_actions_send_keep_and_reload`;`degenerate_states_build_without_panicking`(空标题、10 000 字标题、`active` 越界、`tab_first` 越界、窄到 0 的宽度)。
- [ ] **Step 2: 运行确认失败,搬视图。** 逻辑与度量逐字搬,只做耦合替换:`HoverId::*` → `PreviewHover` + `host.hover_progress`;`Message::Preview*` → 库 `Message` 经 `host.wrap`;`tab_widget::*` → 页签四件;`theme::region::preview_pane()` → `host.pane_style()`;`app.preview_tab_bar_avail_px(..)` → `host.tab_bar_avail_px()`;`crate::i18n::host_text` → 库 `i18n`(owner `dozer.preview` 的现有键原文搬入,中英文文案逐字不变)。
- [ ] **Step 3: 验证。** `cargo test -p dozer-panel-preview -p dozer-app` 总数守恒(加新增);`bash scripts/check-i18n.sh`;`cargo tree` 约束。
- [ ] **Step 4: 提交。**

### Task 6: Dozer 适配与消息映射

**Files:**
- Create:`crates/dozer-app/src/host_adapters/preview_panel.rs`
- Modify:`crates/dozer-app/src/app/{message.rs,update.rs,view.rs,app.rs}`
- Test:`host_adapters/preview_panel.rs`

**Interfaces:**
- Produces:`PreviewDisplay<'a> { app: &'a App, panel: ViewerPanel }` 实现 `PanelDisplay<Message>`(委托现有 `tab_widget::{panel_tab, tab_overflow_button, tab_overflow_menu}`、`terminal::tab_drag_surface`、`app.hover_progress` / `hover_tooltip_ready`(`PreviewHover` → 原 `HoverId::tab_item(PanelKind::Files|Project, idx)` 等)、`theme::region::preview_pane()`、`app.preview_tab_bar_avail_px(kind)`);`wrap(panel, msg)` → 返回今天的顶层消息:`Files` → `Message::Preview*`、`Project` → `Message::ProjectPreview*`(没有对应 Project 专属变体的,走原有的带 `PanelKind` 的变体);`fn panel_of(kind: PanelKind) -> Option<ViewerPanel>` 与反向映射。
- [ ] **Step 1: 写失败测试:** `every_library_message_maps_to_the_legacy_top_level_message_of_the_same_panel`(穷举库 `Message` 变体 × 两个面板,断言映射到的顶层变体与面板种类、载荷);`hover_slots_map_one_to_one_to_the_legacy_hover_ids`(含 `render_mode` / `tabular_mode` / `json_mode` 三个具名槽,Files 与 Project 不碰撞);`panel_kind_mapping_is_total_for_preview_panels_and_none_for_others`;`wry_pool_still_receives_the_same_webview_specs`(对同一组页签状态,`preview_desired` 产出的 `WebviewSpec` 列表与 Task 3 搬迁前的黄金一致——此黄金在 Task 3 Step 1 之前于现状上生成并提交)。
- [ ] **Step 2: 实现并替换调用点;** 删除 `dozer-app` 里已搬走的函数与类型,只留适配。
- [ ] **Step 3: 验证。** `cargo test --workspace` 总数守恒;clippy、fmt。
- [ ] **Step 4: 提交。**

### Task 7: 清理、门禁、验收、合并与 tag

- [ ] **Step 1: 清理。** `grep -rn "pub struct PreviewPane\|pub struct PreviewTab\|fn preview_pane_for\|mod tabular" crates` 只命中 `dozer-panel-preview` 与转调处;`crates/dozer-app/src/preview/` 只剩 P3 的内容(协议、宿主脚本、`trust.rs`、`plantuml.rs`、`code_host.rs`、`webview_protocol.rs`、`native_editor.rs` 等);`workspace/view.rs` 里不再有预览 iced 函数。
- [ ] **Step 2: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-i18n.sh`;`python3 scripts/audit/check_panel_boundary.py`;前端 `npm test`(预览相关的 web 工程未动,仍跑一遍);总数 =(基线 − 无删除 + 新增)。
- [ ] **Step 3: 请用户在 Dozer 上目测验收**(与改造前并排,Files 与 Project 两个面板都做):打开多种文件(代码、Markdown、图片、JSON、CSV、PDF、二进制、超大文本)、页签条(状态、关闭、窄窗口翻页、溢出下拉、拖拽换位)、渲染 / 代码模式切换、表格网格 / 原文、JSON 树 / 原文、查找(代码内、webview 内、替换)、大文件搜索、保存、脏页签关闭提示、外部修改冲突条、加载中视图、降级页、空白信息卡、重启后页签恢复、切项目页签、放大 / 收起、中英文切换。**未验收不继续。**
- [ ] **Step 4: 合并到 Dozer `main`、切 `libs-2026-10-<日期><字母>` tag、推送——向用户确认后执行。** tag 里 bytehost / byteui / bytegit 的版本必须与 Digger 当时用的逐个核对一致。**不发布产品版本。**

## Digger(本仓库)

### Task 8: 同日升级钉版

**Files:**
- Modify:`Cargo.toml`(各 `dozer-*` 库的 `tag`)、`Cargo.lock`

- [ ] **Step 1: 改钉版。** 所有 dozer libs 的 `tag` 改为 Task 7 的新 tag(全部一起);本期 Digger 不依赖 `dozer-panel-preview`(D2)。
- [ ] **Step 2: 验证。** `cargo update --workspace` 后跑 Cargo.lock 重复检查与 `scripts/check-shared-deps.sh`;`cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`。
- [ ] **Step 3: 提交** `chore(deps): align digger with dozer libs <tag>`。

---

## Self-Review

- **Spec 覆盖:** §6 P2 的「搬 `PreviewPane` 与页签条、工具栏、查找条、`CodeView` 等原生部分」→ Task 3、5;`PreviewCapabilities`(§5.3)→ Task 2、4;测试守恒 / 依赖闭包 / 用户目测(§7)→ Task 7;「Digger 接入但先不挂 webview」→ 订正为只钉版(D2,待你审阅);webview 端口的纯数据类型 → Task 3(清单产出留库,池与应答留 `dozer-app` 等 P3)。`CodeView`:设计提到的这个名字我在 Dozer 当前代码里没有找到同名类型(`native_editor.rs` 是 211 行的原生编辑器占位,`code_host.rs` 是 Phase B 的代码宿主契约),按 D1 的切线规则处理,清单里逐个判定。
- **类型一致:** `ViewerPanel`(P1)贯穿 Task 3–6;`PreviewCapabilities` 与 `backend_enabled` 在 Task 2 定义,Task 4、5 引用;`Message` / `Effect` / `UpdateCtx` 在 Task 4 一次定义;`PanelDisplay` / `PreviewHover` / `ViewCtx` 在 Task 5 定义,Task 6 实现;D1 的清单在 Task 1 Step 4 产出,是 Task 3–5 的施工依据。
- **风险点:** ① 这是体量最大的一期(状态 + 约 4 300 行测试 + 约 1 300 行界面 + 编排),Task 1 的清单评审是最重要的控制点,清单不被确认不要动手搬;② `Message` 变体我从 `app/message.rs` 读到约 45 个与预览相关,有的带 `PanelKind` 有的按面板拆成两套,库统一后 Task 6 的全组合映射测试是守住行为的关键;③ `tabular` 带进库后默认特性下的测试数与 Dozer 工作区测试数要按「默认 + `tabular-loader`」两套之和对账;④ `SearchHit` 与 `crate::extensions::search` 的耦合在 Task 3 才能最终判定,可能需要把该类型挪到 `dozer-model`(那会是对一个共享库的加法,先报告);⑤ 预览上下文推送(服务 MCP)与 Agent 命令轮询都留在 `dozer-app`,但它们读写 `PreviewPane` 的字段,搬迁后要经库的公开访问器,若发现缺口先停下报告;⑥ P3 之前 Dozer 的 `desired_webviews` 路径必须字节不变(Task 6 的 `WebviewSpec` 黄金);⑦ 合并、切 tag、推送是对外动作,先确认;⑧ 不发布产品版本(发布在 P4)。
