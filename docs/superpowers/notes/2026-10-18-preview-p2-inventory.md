# S5b-2 P2 施工图:预览相关函数与消息的归属清单

- 日期:2026-10-18
- 对应计划:`docs/superpowers/plans/2026-10-18-preview-engine-p2-panel.md` Task 1 Step 4
- 状态:**用户已确认(2026-10-18)**:§5 的三件事按我的倾向——① 持久化经 `UpdateCtx` 注入 `PreviewPersistence` trait;② 降级页按钮样式经展示适配注入;③ 三个空函数与相关消息按现状搬(消息保留为库的 no-op 变体,停用的测试原样不动)。Task 2 起等 P1 合并后继续。
- 取自:Dozer `main` @ `ed041788`(worktree `../dozer-panel-preview`,分支 `feat/panel-preview-lib`)。清单由脚本从 `workspace/state.rs`、`app/update.rs`、`app/app.rs` 提取名字含 `preview` / `find_` / `conflict` / `blank_info` / `recovery` 的函数(80 个)并逐个读过函数体判定;`app/update.rs` 的消息分支单列。

判定符号:**A** 进库(只碰 `PreviewPane` / 核心类型 / 本地文件 / `tokio`);**B** 留在 `dozer-app`(碰 wry、几何、协议、Agent 命令、MCP 预览上下文、Dozer 键盘 / 焦点、跨面板);**C** 拆开(给出怎么拆)。

## 0. 前置与基线

- **P1 尚未合并:** Dozer `main` 上没有 `dozer-preview-core`,也没有 `ViewerPanel`。Task 1 只动现有代码,不依赖 P1;**Task 2 起必须先让 P1 合并并把本分支 rebase 上去**,否则 `PanelKind → ViewerPanel`、`capabilities` 显式传参、`router` 等都还在 `dozer-app` 里,库建不起来。清单里凡写「核心」的都以 P1 计划的落点为准。
- **基线(worktree 切自 `main`,未改任何文件时):** `cargo test --workspace` 共 **3181** 通过、0 失败;其中 `dozer-app` 的 bin 目标 **1921**(+2 ignored)。
- **本任务新增:** `workspace/preview_models.rs` 14 条视图模型测试 + `preview/webview_spec_golden.rs` 2 条黄金测试 ⇒ `dozer-app` bin 现为 **1937** 通过(1921 + 16)。
- **要随代码走的测试(`#[test]` 个数):** `preview/view.rs` **163**、`preview/state.rs` 2、`tabular/mod.rs` 21、`workspace/view.rs` 7(含 `loading_view_*`)、`workspace/tests.rs` 里与预览相关的约 20 条(其余约 60 条是终端 / Agent 的)、`preview/webview.rs` 7、`preview/backend.rs` 16(P1 已搬)。`preview/webview_protocol.rs` 43、`preview/code_host.rs` 13 留 `dozer-app`(P3)。

## 1. 跨函数的结构性发现(影响 Task 3–5 的设计)

1. **`allowed_files` 白名单。** `Workspace.allowed_files: Arc<Mutex<HashSet<PathBuf>>>` 是协议「文件字节端点只服务用户显式打开过的路径」的安全白名单,由 `restore_preview_state`、`preview_open_path_at`、各打开路径往里 `insert`,由 wry 协议闭包读。库的 `update` 不能直接碰它(那是 P3 协议的东西):**库输出 `Effect::AllowFile(PathBuf)`**,`dozer-app` 在执行效果时写白名单;P3 再把白名单的所有权收进协议处理器。
2. **持久化路径是产品相关的。** `preview_state::{load, save}`(按项目 id 分文件)与 `recovery_dir()` 的落盘位置来自 Dozer 的目录布局。库不应知道它们:**库输出 `Effect::PersistTabs { project_id, snapshot }` / `Effect::LoadPersistedTabs`**,或在 `UpdateCtx` 里注入一个 `PreviewPersistence` trait(`load(project_id) -> PreviewState` / `save(..)` / `recovery_dir()`)。我倾向后者(同步读、异步写,与现状一致),请在 Task 4 定。
3. **`ShellIo` 的三样东西要换成 `UpdateCtx` 字段:** `io.arm_load_timeout(...)` → `Effect::ScheduleLoadTimeout`;`io.safe_startup` → `ctx.safe_startup: bool`;`io.proxy.send_event(Message::…)` → `emit`。
4. **`Workspace` 里 Files / Project 各有一组并列字段:** `preview` / `preview_error` / `preview_tab_first`(+ Project 的 `project_preview*`、各自的溢出锚点)。库里应有一个把这组字段打包的 `PreviewPanelState { pane, error, tab_first, overflow_anchor }`,`Workspace` 持两份;`preview_pane(kind)` 等 4 个访问器随之变成对 `PreviewPanelState` 的选择。
5. **页签右键菜单是跨扩展消息:** `preview_pane_for` 里页签的 `on_right_press` 直接发 `Message::Files(files::Message::TabContextMenuOpen { kind, path })`。库里改为发 `Message::TabContextMenu { path }`(带面板种类),由适配层转成 Files 扩展的消息。
6. **品牌字样进了共享文案:** 降级页的 `preview.fallback_title`(`Cannot preview this file inside Dozer`)与 `preview.fallback_security_note`(`For safety, Dozer will not execute…`)里写死了 `Dozer`。Digger 里会显示成 Dozer。**搬进库时加 `{product}` 占位**,Dozer 传 `"Dozer"`(文案逐字不变),Digger 传 `"Digger"`;中英文资源都要改,`check-i18n` 的键集合校验要同步。
7. **降级页的按钮样式依赖 Dozer 的另一个模块:** `crate::extensions::project_create::action_button_hover_style`(Dozer「新建项目」弹窗的按钮样式)。库不能依赖它:要么经 `PanelDisplay` 注入一个 `action_button_style(color)`,要么改用 byteui 的 `feedback::dialog::action_button_style`(**外观是否逐像素相同我没有核实,默认走注入,保证 Dozer 不变**)。
8. **三个空函数是已退役 iced 编辑器的遗骸:** `preview_pane_active_editor_event`、`preview_pane_undo_active`、`preview_pane_redo_active` 函数体为空,但仍被 `Message::PreviewTabInsertTab / UndoActive / RedoActive` 分支调用;`workspace/tests.rs` 里有三条相关的历史测试(`preview_pane_undo_active_reverts_edit_and_marks_dirty`、`…redo_active_reapplies…`、`…undo_active_noop_without_native_tab`),但都已被 `#[cfg(any())]` 停用,**从不参与编译,也不在测试计数里**。**是否删除这三个函数、三条消息和这三段停用的测试,是你的决定**(我默认「按现状搬」:消息保留为库的 no-op 变体,停用的测试原样留在原文件,不动)。
9. **计划里「同名文件消歧」不存在。** `preview_tab_display_title` 对文件 tab 就是 `tab.title`(文件名),没有同名消歧逻辑;P2 计划 Review Focus 3 的这个说法是我写错了,已订正。`preview_models` 的测试固定的是实际行为(`Blank` → 项目根目录名 / 本地化 `Blank`;脏页签加 ` *`)。
10. **设计稿提到的 `CodeView` 不存在。** 当前代码里只有 `native_editor.rs`(211 行,原生编辑器占位与 `wry_toggle_eligible` 等判定)和 `code_host.rs`(Phase B 代码宿主契约:URL 构造、`codemirror_enabled()` 等恒真开关)。`native_editor.rs` 的判定函数(`wry_toggle_eligible`、`extension_to_syntax`、`is_strict_json_extension`)被工具栏与路由用到,P1 已把路由搬走;这几个判定函数跟随它们的调用方进库。`code_host.rs` 的 URL 构造被 `desired_editor_webviews` 调用,**属于 P3 的协议契约,但库里的 `PreviewPane::desired_*` 需要它**——所以 P2 要把 `code_host.rs` 里的**纯 URL / 绑定 / 常开开关**一并带进库,协议应答仍留 `dozer-app`。
11. **`WebviewSpec` 黄金已生成:** `tests/fixtures/preview_webview_specs_golden.txt`(1340 行、72 个规格、Files 与 Project 面板 × en / zh-CN × 8 个场景),跨进程稳定。注意 URL 里的 `theme=dark` 来自全局配色:如果将来某个测试把全局配色改成浅色且并行运行,这条黄金会抖;目前全量测试里没有这种情况(基线全绿)。

## 2. `workspace/state.rs`

| 行 | 函数 | 判定 | 说明 |
|---|---|---|---|
| 35 | `spawn_preview_command_poller` | B | Agent 命令轮询(`client` + `PreviewCommand`),S6e-3 |
| 1236 | `spawn_preview_state_save` | C | 快照构造(遍历页签得 `PersistedPreviewTab`)进库为 `PreviewPane::persisted_snapshot()`(纯);落盘走 §1.2 的持久化注入 |
| 1317 / 1325 / 1328 | `spawn_/flush_/enqueue_preview_context_push` | B | MCP 预览上下文推送(`client.update_preview_context`),S6e-3;读取的 `web_selection` 等字段是 `PreviewTab` 的 pub 字段,搬后仍可读 |
| 1419 | `restore_preview_state` | C | `preview_state::load` → §1.2;`allowed_files.insert` → `Effect::AllowFile`;`push_shell_tab` / `set_pending_view` / `set_pending_tabular`(A);`safe_startup` → `ctx`;末尾触发 `load_preview_tab` |
| 1477 | `load_preview_tab` | C | 「路由种类 → 起始加载阶段」映射是纯策略(进库为 `PreviewPane::begin_load_for_route(tab_id) -> Option<u64>`,逐分支搬);`io.arm_load_timeout` → `Effect::ScheduleLoadTimeout`;recovery 读取块 → `Effect::ReadRecovery`;`spawn_pending_tabular_loads` → `Effect::LoadTabular`(`tabular-loader` 特性) |
| 2264 / 2279 | `active_preview_webview_id` / `active_preview_panel_kind` | B | wry 池寻址 / `PanelKind` |
| 2295 / 2301 / 2310 / 2321 | `preview_pane(_mut)` / `preview_error_mut` / `preview_tab_first_mut` | B→A | 按 `PanelKind` 选 Files / Project 字段;随 §1.4 的 `PreviewPanelState` 变成两份的选择器,留 `dozer-app` |
| 2336 | `active_preview_tab_has_native_editor` | A | 一行判定 |
| 2350 / 2355 / 2358 | `preview_pane_active_editor_event` / `undo_active` / `redo_active` | 待定 | 空函数,见 §1.8 |
| 2361 / 2667 | `preview_pane_save_active` / `save_at` | A | 保存到绑定路径(`Effect::SaveFile`),校验目标路径 = 绑定路径 |
| 2369 / 2382 | `preview_retry` / `preview_plain_text_open` | A | 纯 `PreviewPane` 操作 |
| 2400 / 2406 / 2413 | `preview_conflict_active` / `keep` / `reload` | A | `reload` 里的读文件 → `Effect` |
| 2441 / 2468 | `apply_preview_command` / `resolve_preview_command_target` | B | Agent 命令,S6e-3;它们调用的 `PreviewPane::apply_preview_command` 是 pane 方法,随 pane 进库 |
| 2506 | `preview_spawn_window_index` | A | `spawn_blocking` 建窗口化索引 → `Effect::StartWindowIndex`,结果回 `Message::WindowIndex` |
| 2611 | `preview_pane_tabular_action` | C | pane 状态与 `pending_tabular_commands` 入队(A);spawn 的部分 → `Effect` |
| 2681 / 2724 | `preview_pane_toggle_render_mode` / `toggle_json_mode` | A | pane 操作(可能向 pane 的待发命令队列入队,队列字段随 pane) |
| 2752–2845 | `preview_find_*` ×15、`set_find_query_focused` | A | 对 pane 的薄封装 |
| 2846 / 2855 / 2863 | `take_preview_webview_find(_clear)` / `preview_find_set_webview_state` | A | 取走 pane 的待发查找命令 / 写查找结果态;webview 脚本推送本身在 P3 |
| 2979 / 3002 | `blur_inputs` / `blur_preview_editors` | B | Dozer 的 iced 焦点操作 |

## 3. `app/update.rs`

| 行 | 函数 / 消息分支 | 判定 | 说明 |
|---|---|---|---|
| 5766 | `spawn_preview_profile` | A | `Effect::ProbeProfile` |
| 5819 | `preview_open_path` | A | 转调 |
| 5999 | `preview_open_path_at` | C | pane 操作与 profile 派发(A);`allowed_files.insert` → `Effect::AllowFile`;上下文推送 / webview 跳转行 → B |
| 6160 / 6282 | `preview_select_tab` / `project_preview_select_tab` | C | `select`(A);`tab_drag` 武装(Dozer `App` 级,B);上下文推送 flush(B)。**两份合成一份,按 `ViewerPanel` 参数化** |
| 6212 | `project_preview_open_path` | C | 与 `preview_open_path_at` 同款,合并 |
| 1262 | `PreviewProfiled` | A | 世代校验 + `apply_profile` + 后续加载 |
| 1422 | `PreviewLoadTimeout` | A | 世代校验后 `fail_load` |
| 1189 | `PreviewWindowIndex` | A | |
| 1452 / 1460 | `PreviewRecoveryWritten` / `PreviewRecoveryRead` | A | |
| 2400 | `PreviewBlankInfoLoaded` | A | 丢弃与项目不符的旧结果 |
| 2422–2540 | `PreviewLargeFileSearch{Close,Submit,Results,Progress,Go}` | A | `Submit` 的搜索任务 → `Effect` |
| 2322 / 2358 / 2927 / 2970 | `Preview/ProjectPreviewTabOverflow{Toggle,Dismiss}` | A | 溢出锚点进 `PreviewPanelState`;锚点坐标来自 `App.last_cursor`(适配层传入) |
| 2396 / 2884 | `PreviewOpenPath` / `ProjectPreviewOpenPath` | A | |
| 2569 / 2885 | `PreviewSelectTab` / `ProjectPreviewSelectTab` | C | 见 6160 |
| 2570 | `PreviewOpenExternal` | A | `Effect::OpenExternal` |
| 2617–2651 | `PreviewRetry` / `PlainTextOpen` / `ConflictKeep` / `ConflictReload` / `TabularTextModeToggle` / `JsonModeToggle` | A | |
| 2667 / 2886 | `PreviewCloseTab` / `ProjectPreviewCloseTab` | C | 脏页签先保存再关(A);webview 池销毁由清单差异表达(B 执行) |
| 2698 / 2910 | `PreviewToggleRenderMode` / `ProjectPreviewToggleRenderMode` | A | |
| 2715 | `PreviewSaveActive` | A | |
| 2718 / 2728 / 2731 | `PreviewTabInsertTab` / `UndoActive` / `RedoActive` | 待定 | 见 §1.8 |
| 2734–2804 | `PreviewFind*` ×11、`PreviewFindWebviewState` | A | |
| 2813 / 2852 | `TabularLoaded` / `TabularSheetLoaded` | A | `tabular-loader` 特性下的结果落地 |
| 3965 / 4048 | `PreviewSelectionMenuOpen/Close` | B | 「发送选中内容到 Agent」菜单(`agent_context`) |
| 1176 | `PreviewCommandsFetched` | B | Agent 命令 |
| 335 / 857 / 920 / 1038 / 1125 / 1131 / 1157 | `EditorWebviewEvent` / `JsonEditorEvent` / `TabularHostEvent` / `FlyfishEvent` / `ImageAnnotateEvent` / `PlantUmlEvent` / `PlantUmlLoaded` | B | webview 页面事件(P3);它们内部调用的 `PreviewPane::apply_*` 随 pane 进库 |
| 1032 / 1534 | `ReviewTraceWebviewEvent` / `ReviewLoaded` | — | 会话面板(S7a),不属于预览 |

## 4. `app/app.rs`

| 行 | 函数 | 判定 | 说明 |
|---|---|---|---|
| 1090 / 1141 / 1187 | `take_preview_{editor,tabular,plantuml}_scripts` | B | 向各 webview 推脚本(P3) |
| 1472 / 1495 / 1520 | `preview_editor_reveal` / `select` / `replace` | B | Agent 编辑工具入口(`#[allow(dead_code)]`),S6e-3 |
| 1674 | `apply_preview_pool_evictions` | B | wry 池淘汰结果 → pane `suspend_tab`,P3 |
| 2833 / 2847 | `active_preview_webview_id` / `active_preview_panel_kind` | B | |
| 2904 | `preview_active_tab_is_native` | A | 一行判定 |
| 2919 | `blur_preview_editors` | B | |
| 3088 | `cancel_all_preview_background` | A | 抽成 `PreviewPane::cancel_all_background()`,`App` 调用 |
| 3619 / 3631 / 3651 | `active_preview_tab_has_native_editor` / `preview_find_bar_open` / `preview_large_file_search_bar_open` | A | pane 判定 |
| 3642 / 3660 | `preview_find_bar_over_webview` / `preview_conflict_bar_over_webview` | C | 「条存在且激活页签托管 webview」是 pane 判定(A);用它去隐藏 / 腾位 webview 的是 `App` 几何(B) |
| 3668 / 3677 / 3684 / 3697 | `take_preview_webview_find(_clear)` / `preview_find_set_webview_state` / `set_find_query_focused` | A | 转调 |
| 3713 | `take_pending_preview_zoom` | B | wry 缩放 |
| 3814 | `preview_tab_bar_avail_px` | B | `App` 几何,改为 `PanelDisplay::tab_bar_avail_px` 的 Dozer 实现 |
| 3873 | `preview_desired`(361 行) | B | 为所有面板构造 `(WebviewSpec, bounds)` 清单,调用 `pane.desired_*`;P3 |
| 4542 / 4555 | `webview_hidden_by_panel_popup_*` 两条测试 | B | |

## 5. 汇总

- **进库(A)约 45 个函数 / 约 40 个消息分支;拆开(C)约 10 个;留 `dozer-app`(B)约 25 个函数 / 约 12 个消息分支;待你定 3 个函数 + 3 条消息 + 3 段停用测试(§1.8)。**
- **需要你拍板的:** ① §1.2 持久化走注入 trait 还是 `Effect`(我倾向 trait);② §1.7 降级页按钮样式走注入还是改用 byteui(我倾向注入);③ §1.8 三个空函数与相关消息 / 测试删不删(我倾向按现状搬、不删)。
- **不需要你拍板、已定的:** §1.1 白名单经 `Effect::AllowFile`;§1.4 `PreviewPanelState`;§1.5 页签右键菜单消息;§1.6 `{product}` 占位;§1.10 `code_host.rs` 的纯部分随 P2 进库。
