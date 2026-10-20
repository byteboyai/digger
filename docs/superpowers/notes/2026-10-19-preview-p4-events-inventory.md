# S5b-2 P4 施工图:页面事件进库的归属清单

- 日期:2026-10-19
- 对应计划:`docs/superpowers/plans/2026-10-19-preview-engine-p4-pairing.md` Task 1 Step 2
- 状态:**等用户过目。** §5 有 4 件我不确定、按计划要求「拿不准的停下报告」的事。
- 取自:Dozer `2eaba8cf`(worktree `../dozer-p4`,分支 `feat/preview-events-lib`)。
- 判定符号:**A** 进库(只碰 `PreviewPane` / `dozer-preview-core` / `Effect` / 注入的 `UpdateCtx`);**B** 留 `dozer-app`(碰 wry、几何、`dozerd` / MCP、Toast、原生菜单、Dozer 焦点);**C** 拆开(给出怎么拆)。

## 0. 基线(Task 1 Step 1)

- `cargo test --workspace`:**3292 通过、0 失败、4 ignored**(`--no-fail-fast`,第二次跑;第一次跑时 `dozer-store-project` 掉 1 条,单跑与复跑均通过 —— 见 §6.1)。
- 分项:`dozer-app` bin **1332**、`dozer-panel-preview` lib **356**、`dozer-app` lib(含集成测试目标)**1330**。
- 三份黄金的 sha1:
  - `crates/dozer-app/tests/fixtures/preview_webview_specs_golden.txt` `7cd8d5d95d2f147f2746f8c03a7c2fce7797f3cc`
  - `crates/dozer-panel-preview/tests/fixtures/preview_webview_specs_golden.txt` `e498ea01be2ff18e5fad46dc083b5533dc140da2`
  - `crates/dozer-panel-preview/tests/fixtures/preview_host_script_golden.txt` `ef27018f7b1acd9cbf37ce999ece1ccfcdccd872`
  - (协议黄金 `preview_protocol_golden.txt` `7abcba078806758b7c27bc54c912ee464fd72f8a`,Task 3 涉及)

## 1. 跨函数的结构性发现(决定 Task 2 的形状)

1. **`PreviewPanelState` 的所有权是「临时打包」。** `Workspace` 里 Files / Project 各有一组并列字段,`preview_run` 用 `std::mem::take` 打包、跑完放回。所以库的 `update(state, msg, ctx)` 可以在**不持有状态**的前提下处理页面事件 —— **A 判定不需要新机制**,Task 2 只往里加分支。
2. **`UpdateCtx` 已够用,唯缺两样:** `safe_startup`(P2 施工图 §1.3 的欠账,与 P4 无关)与 `emitted` 的「产品回调」。页面事件需要的 `project_root` / `caps` / `locale` / `panel` / `project_id` / `active_webview_pool_id` / `behavior` 都在。
3. **保存所需的一切已在库或核心:** `PreviewTab::save_gate()`(只读 / 冲突 / 允许)、`save_text_atomic`、`disk_mtime`、`clear_snapshot` 全在库或 `dozer-preview-core`。`SaveGate` 的两条错误文案目前用 `crate::i18n::host_text`(dozer-app)拼字符串 —— **搬进库时改用 `LocalizedText::keyed` 存进 `tab.web_error`,与库现状一致**(库的 `error: Option<LocalizedText>` 已经是这个形状)。
4. **脏快照写入用的是库的 `PreviewPersistence`**,`preview_run` 已经把它传进 `run_effect`;`EditorEvent::Snapshot` 分支搬进库后照样能拿到。
5. **`context_changed` 只产出 `Effect::ContextChanged { immediate }`**,而 Dozer 的执行器(`run_preview_effects`)已经处理它 → **A**。MCP 推送本身(`spawn_preview_context_push`)留 `dozer-app`。
6. **workspace 级钩子都是 `ws` 方法,没有 `App` 级状态:** `arm_load_timeout` → 已有 `Effect::ScheduleLoadTimeout`;`queue_editor_command` / `open_large_file_search` / `finish_load` / `close_by_id` / `spawn_preview_state_save` / `flush_preview_context_push` 全是 `PreviewPane` 或 `Workspace` 上的方法,前者库里已有,后者可继续经 `Effect` 通知。
7. **`EditorEvent::Failed` 的降级文案要读一个全局计数器文件**(`failures_path()`),那是 Dozer 的配置目录。库有 `PreviewPersistence::recovery_dir()`,但**没有**失败计数路径。见 §5 Q1。
8. **`PreviewSelectionMenuOpen` 必须在 `with_project` 闭包外弹**(要 `self.shell_state()` 算窗口坐标)。库只能产出「请产品弹菜单」的通知。见 §5 Q2。
9. **右键菜单里的「发送给 Agent」走 `dozerd` + Agent 输入区**,是 Dozer 专属;Digger 第一批没有 Agent,菜单项应当不出现。见 §5 Q2。
10. **`FlyfishEvent::Title` 分支会**覆盖页签标题,但注释说「文件 tab 恒用文件名」——这条逻辑搬进库后与 `TabKind` 无关的判断仍在库里可做(**A**),Dozer 行为不变。
11. **`TabularHostEvent` 需要 `tabular-loader` 特性**,Digger 不开。搬进库时按 `#[cfg(feature = "tabular-loader")]` 或运行时 `caps.tabular` 守卫;Dozer 开特性 → 行为不变。
12. **`GitLogDiffWebviewEvent` / `FileHistoryDiffWebviewEvent` / `EditHistoryDiffWebviewEvent` 绑的是面板外单槽**(`self.git_log`、`self.file_history`、`self.edit_history`),不是 `PreviewPane`;**B**,留 `dozer-app`。
13. **`ReviewTraceWebviewEvent` 绑 `ws.review`**(会话审阅的 trace 视图),同样是面板外单槽;**B**。
14. **`Message::Host` 的透传已经存在**(P3 加的 `App::host_event` → `Message::*WebviewEvent`),Task 2 只需把 7 个分支的**处理体**换掉,消息枚举与 IPC 入口不动。

## 2. `app/update.rs` 逐分支判定

### 2.1 `EditorWebviewEvent`(行 341–866,`dozer-app` 里最大的一段,约 525 行)

先按 `EditorEvent` 变体拆:

| 事件 | 行 | 判定 | 说明 |
|---|---|---|---|
| `Ready` | 388 | **A** | 只改 `tab.web_revision` / `load_state.advance` / 失败计数清零 / 决定是否建索引。`io.arm_load_timeout` → `Effect::ScheduleLoadTimeout`(已有);`reset_failure_to(failures_path())` → §5 Q1;建索引 → `Effect::StartWindowIndex`(已有) |
| `DocumentLoaded` | 444 | **A** | 终态判定、`backend_state`、`load_observe` 日志(`tracing`,库已用)、`pending_restore` → `Effect::PushHostScript`(新)、`pending_view` → `queue_editor_command`(库里已有)。`pending_jump_line` → `Effect::RevealPosition`(新,或复用 `queue_editor_command`) |
| `SelectionChanged` | 525 | **A** | 纯字段 + `context_changed` |
| `ViewportChanged` | 541 | **A** | 同上 |
| `DocumentChanged` | 546 | **A** | 纯字段(脏标记) |
| `SaveRequested` | 552 | **C** | `save_gate` 判定 + revision 校验进库;写盘出 `Effect::SaveFile`,清快照出 `Effect::ClearRecovery`(已有);**「保存后关闭」需要 `want_close` 预探测**,由库在 `take_pending_close` 之外读页签状态即可(库里已有 `pending_close`) |
| `Snapshot` | 630 | **A** | 防抖脏快照 → 现有 `spawn_blocking` + `PreviewPersistence`;回投 `Message::RecoveryWritten` |
| `ViewState` | 686 | **A** | 纯字段 |
| `WindowRequest` | 703 | **A** | `Effect::StartWindowIndex` 或现有 `queue_windowed_view`(库里有) |
| `WindowApplied` | 707 | **A** | 纯 `load_state.finish` |
| `FindRequest` | 716 | **A** | `open_large_file_search`(库里有) |
| `Failed` | 719 | **C** | 失败计数 → §5 Q1;`backend_state` / `web_error` 进库 |
| `FocusChanged` | 754 | **A** | `{}` |
| `ContextMenuRequested` | 757 | **C** | 闭包外弹菜单 → 新增 `Effect::SelectionContextMenu { .. }` 原样交给产品;库只做转发(见 §5 Q2) |

段末的五个「借用结束后」动作(`queue_editor_command` ×2、`queue_windowed_view`、`finish_load`、`open_large_file_search`)全在库里,**A**;`spawn_preview_state_save` / `flush_preview_context_push` 属 `Workspace` → 改为 `Effect::TabClosed` + `Effect::ContextChanged`(已有)。

### 2.2 `JsonEditorEvent`(行 867–929)

| 判定 | 说明 |
|---|---|
| **A** | 与 `EditorWebviewEvent` 同形(ready / document_loaded),只是没有编辑相关字段。Digger `caps.json == false`,不会走到,但搬进库不增加成本。 |

### 2.3 `TabularHostEvent`(行 930–1046)

| 判定 | 说明 |
|---|---|
| **A + 特性守卫** | `WindowRequest` / `SheetSelected` / `Failed` 全是对 `PreviewPane` 的调用;`Ready` 触发 `push_tabular_window`。用 `caps.tabular` 做**运行时**守卫(不必动 feature 依赖),Dozer `caps::all()` 不变。 |

### 2.4 `FlyfishEvent`(行 1048–1133)

| 事件 | 判定 | 说明 |
|---|---|---|
| `SearchState` | **C** | 前半段要读 `pane.find_state()`(库里有),但写回走 `ws.preview_find_set_webview_state`(也是 `PreviewPane` 方法)。整体可进库;**Digger 没有查找条的话不会触发** |
| `Ready` | **A** | 清 `web_error` |
| `DocumentLoaded` | **A** | 终态 + `load_state.finish` |
| `Title` | **A** | 按 `TabKind` 覆盖标题 |
| `Failed` | **A** | `runtime = None`(Digger 无此字段 → 见 §5 Q3)+ `backend_state` |

**结论:整段 A**(§5 Q3 的 `runtime` 字段除外)。

### 2.5 `ImageAnnotateEvent`(行 1135–1139)

| 判定 | 说明 |
|---|---|
| **A** | 一行 `pane.apply_image_annotate_event(tab_id, payload)`。`apply_image_annotate_event`(`pane.rs:322`)与 `ImageAnnotateEvent`(`webview_protocol.rs:637`)都**已经在库里**,`dozer-app` 只是 re-export。**没有类型搬运工作。** |

### 2.6 `PlantUmlEvent`(行 1141–1168)

| 判定 | 说明 |
|---|---|
| **A** | 同 2.5:`apply_plantuml_event` 已在库;`enter_code_mode` / `queue_editor_command` 也在库。 |

### 2.7 `GitLogDiffWebviewEvent` / `FileHistoryDiffWebviewEvent` / `EditHistoryDiffWebviewEvent`

**B**。三个都是面板外单槽(`self.git_log` 等)的 `Ready` 握手,与 `PreviewPane` 无关。留 `dozer-app`,库里 `HostEvent::GitLogDiff` 继续透传。

### 2.8 `ReviewTraceWebviewEvent`

**B**。绑 `ws.review`。

## 3. 碰 Dozer 全局的部分(必须留 `dozer-app` 的清单)

| 东西 | 位置 | 为什么留 |
|---|---|---|
| MCP 预览上下文推送 | `ws.spawn_preview_context_push` / `flush_preview_context_push` | `dozerd` 客户端 + `App` 级防抖 |
| `dozerd` 预览命令轮询 | `spawn_preview_command_poller` | `dozerd` |
| 失败计数文件 | `crate::preview::failures_path()` | 配置目录;见 §5 Q1 |
| Toast | `Message::Toast` + `external_apps::open_failure_message` | 产品 UI |
| 原生 / iced 右键菜单 | `PreviewSelectionMenuOpen` | 几何 + 原生菜单 + Agent 终端可见性 |
| 「发送给 Agent」 | `SendSelectionToAgent` | `dozerd` + Agent 输入区 |
| 键盘焦点 | `sync_terminal_grid` / `terminal_visible` | Dozer 终端 |
| 窗口几何 | `webview_geometry` / `shell_state` | wry |
| `runtime.rs` IPC 闭包 | `classify_ipc` 调用点 | wry |

## 4. Task 2 的落点(施工)

按 §2,第一批 Digger 需要的 **Editor / Flyfish / ImageAnnotate 全部 A**,`Json` / `Tabular` / `PlantUml` 也 A,`GitLogDiff` / `FileHistoryDiff` / `EditHistoryDiff` / `ReviewTrace` 留 B。

具体新增(与计划 Interfaces 一致):

- `Message::Host(HostEvent)` —— 加进 `dozer-panel-preview::Message`;`update` 里按 §2 分发。
- `Effect::SaveFile { tab_id, path, text }` → `run_effect` 用 `save_text_atomic` 写、回投 `Message::SaveFinished`。
- `Effect::PushHostScript { webview_id, script }` + `PreviewPane::take_host_scripts(panel)` —— 给 recovery / view-state 恢复用(现在这是 `queue_editor_command`,wry 侧要从这里取脚本)。
- `Message::SaveFinished { tab_id, result }`。
- `Message::SelectionContextMenu { .. }`(若 §5 Q2 选「通知产品」)。

`dozer-app` 侧:§2 判定为 A 的分支处理体删除,换成一行 `self.preview_ui(kind, Message::Host(event))`,并执行返回的 `Effect`(`run_preview_effects` 已有);B 分支原样保留。

## 5. 需要你拍板(按计划「拿不准的停下报告」)—— 已拍板(2026-10-19)

- **Q1 → 加进 `PreviewPersistence`:** 新增 `fn failures_path(&self) -> PathBuf`。
- **Q2 → `Effect` 原样转发:** 新增 `Effect::SelectionContextMenu`,Digger 忽略。
- **Q3 → 保留在库里:** `PreviewTab::runtime` 不动。
- **Q4 → 按计划原文执行:** `Arc<Mutex<AllowedFiles>>`。

以下为当时的问题原文,保留备查。

### Q1:失败计数文件怎么给库?

`EditorEvent::Failed` 与 `Ready` 都读写 `<config_dir>/preview_failures.json`(连续失败 3 次提示降级)。库拿不到 `config_dir`。

- **(a) 加进 `PreviewPersistence`:** 新增 `fn failures_path(&self) -> PathBuf`。三行改动,Dozer / Digger 各自实现。**我的倾向。**
- (b) 加进 `UpdateCtx`。会让 `UpdateCtx` 多一个字段,且每次构造都要给。
- (c) 留在 `dozer-app`,库只负责起 `Effect::FailureCount`。Dozer 侧的处理体没法删干净,与计划「处理体搬进库」冲突。

### Q2:预览区右键菜单(「复制 / 发送给 Agent」)怎么办?

- **(a) 新增 `Effect::SelectionContextMenu { panel, tab_id, path, x, y, range, selected_text }`,原样转发。** Dozer 现有 `Message::PreviewSelectionMenuOpen` 的处理体不变;Digger 第一批忽略该效果(Global Constraints 允许「通知类效果忽略」)。**我的倾向。**
- (b) 菜单进库(需要 Agent 可见性 / 窗口坐标两个产品输入,Digger 没有 Agent)。

### Q3:`PreviewTab::runtime` 字段(Dozer 专属)在库里怎么处理?

`FlyfishEvent::DocumentLoaded/Failed` 会写 `tab.runtime = PreviewRuntime::None`(Doze r 的 flyfish 渲染器枚举)。`TabKind` / `web_error` / `backend_state` 都在库里,**`runtime` 也在库里**(P2 已搬)。请确认:搬进库后 Digger 写 `runtime = None` 是无害的(它只被 Dozer 的 `desired_*` 读)。若你希望库里彻底不出现这个概念,我会把它降级为 `tab.web_error` 的副作用并只在 Dozer 侧另存 —— 但那会让 Dozer 的代码改动变大。

### Q4:`AllowFile` → `AllowedFiles` 的过渡

Task 3 要求把白名单参数从 `&...HashSet` 换成 `&AllowedFiles`。`dozer-app` 的 `Workspace.allowed_files` 是 `Arc<Mutex<HashSet<PathBuf>>>`,被 `runtime.rs` 的 wry 协议闭包读。Task 3 会把它换成 `Arc<Mutex<AllowedFiles>>`。**这是计划原文,我按原文执行**;只想让你确认「Dozer 可见行为不变」这一点我按「同一个 `insert` / `contains` 语义 + 规范化」理解,不额外加日志。

## 6. 其它

### 6.1 基线的不稳定

第一次跑全量时 `dozer-store-project` 的 1 条测试失败、第二次全绿;`dozer-app` 的 `preview_webview_specs_match_the_golden` 在**并行跑多个测试二进制**时也失败过(macOS 上两个二进制同时写同一个临时目录),单跑通过。两者都与本次改动无关,记在这里以免把后续全量跑结果误判成回归。

### 6.2 与 P3 施工图的差异

P3 计划里 Task 4(页面事件进库)顺延到 P4,当时没有产出施工图。本文件就是它的替代。

### 6.3 搬走后 `dozer-app` 的行数

`EditorWebviewEvent`(525 行)+ `FlyfishEvent`(85)+ `PlantUmlEvent`(27)+ `ImageAnnotateEvent`(6)+ `JsonEditorEvent`(62)+ `TabularHostEvent`(116)≈ **820 行**会从 `app/update.rs` 消失,换成约 30 行分发。`app/update.rs` 现在 **6050 行**。

### 6.4 给 Digger Task 7 的补充发现

`project_delete_confirm`(`app/update.rs:3518`)只关项目页签 + 发删除请求,**不清 `<config_dir>/preview_state/<id>.json`** —— Dozer 现状就是这样(删项目后重启、id 万一复用会恢复旧页签)。计划 Task 7 要求 `forget_project` 删文件,Digger 侧做,不必回头改 Dozer。
