# S5b-2 P4 配对视图与点文件:Project / Files 右半边显示预览,页面事件进库,Digger 接线 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户在 Digger 主界面里真正用上预览引擎:Project 和 Files 两个面板从单栏改成"左:信息 / 文件树,右:预览"的配对视图;文件树双击文件、Project 文档链接都在右半边预览里打开;代码页签可编辑并保存,Markdown / HTML / 图片 / SVG 渲染;切项目、放大、收起、关项目时 webview 随之正确显示、隐藏、销毁。同时补完 P3 顺延的两块:页面事件作用到 `PreviewPane`(进库),以及白名单规范化与资源交付。收尾做 Dozer + Digger 联合发布(向用户确认后执行)。

**Architecture:** Dozer 侧只做库:`Message::Host(HostEvent)` 进 `update`,产出 `Effect`;`AllowedFiles` 收敛白名单并规范化。Digger 侧新增 `adapters/preview.rs`(按 `(project_id, ViewerPanel)` 持有库的 `PreviewPanelState`、执行 `Effect`、路由消息)、`adapters/preview_display.rs`(库 `PanelDisplay` 的 Digger 实现)、`adapters/tab_chrome.rs`(页签 / 溢出菜单控件,以后 Agent 面板复用),并把 P3 的 `preview_native::Host` 接进 `sync_native`。布局沿用 S1 的 `PaneRules` / `pair_view` / `pair_rects`(用量面板已是模板),预览 webview 的矩形 = 配对第二块 − `byteui::theme::geometry::preview_chrome_top_px()`(与 Dozer `webview_geometry` 同一常量)。

**Tech Stack:** Rust、iced 0.14、wry(仅 Digger 产品侧)、`dozer-panel-preview` / `dozer-panel-files` / `dozer-panel-project`(`libs-2026-10-19f` 起)、bytehost-chrome。

**Spec:** `docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md`(§5.4 点文件与配对、§5.5 安全、§6 P4、§7 验证、D4 / D5);前置计划 `2026-10-19-preview-engine-p3-host.md` 的"未完成项"。前置状态:Dozer `main` 与 Digger `main` 已含 P1–P3 与文件树(`libs-2026-10-19f`)。

## Global Constraints

- **Dozer 用户可见行为不变;** 搬走的测试随代码走、不删、不改期望值,总数守恒;`preview_webview_specs_golden.txt`、协议黄金、注入脚本黄金字节不变;P3 评审时恢复的 14 个资源 / CSP 守卫测试不得再减少。
- **依赖闭包:** `dozer-panel-preview` 不含 `wry`/`winit`/`dozer-app`/`dozer-client`/`dozer-core`;`wry` 只在 `digger-desktop`。
- **安全约束原样保留(spec §5.5):** webview 与 `(project_id, panel, tab_id, path)` 绑定,路径只来自 Rust;白名单只含用户显式打开过的路径(规范化后比较);保存目标必须等于绑定路径且在项目目录内;来自旧项目 / 已关页签的页面事件一律丢弃。
- **Digger 能力集:** `PreviewCapabilities { code: true, rendered: true, pdf: false, media: false, plantuml: false, json: false, streamed: false, tabular: false }`;关闭的类型走降级页并保留"用系统应用打开"。
- **章节编辑器零改动,** 预览 webview 与它各一张句柄表(P3 已定)。
- **没有 Agent / MCP:** `Effect::ContextChanged`、`Effect::TabSelected` 之外的"通知类"效果在 Digger 里忽略,不报错。
- **每个项目一份状态:** 预览页签随项目保存,磁盘格式与 Dozer 相同(`<config_dir>/preview_state/<project_id>.json`);只给**当前项目且面板可见**的页签创建 webview,其余项目只留状态。
- 只用 git tag 依赖;Dozer 内部 `path = "../.."` 除外。**合并、切 tag、推送、发布版本是对外动作,每一项向用户确认后执行。** 主工作区别人未提交的改动不动;两个仓库各用一个 worktree(`../dozer-p4`、`../digger-p4`),完成后合并并清理。
- **我的决定(可推翻):**
  - **D1 Project 与 Files 都用 `PaneRules`,** 列表(信息 / 文件树)是第一块,预览是第二块,最小列宽沿用 `shell::list_min_width()`;默认分割比沿用 `default_split_ratio()`。已保存的旧布局没有配对状态时用默认分割。
  - **D2 页签控件自己写在 Digger(`tab_chrome.rs`),** 没有悬停动画(`hover_progress` 恒为 0,`hover_tooltip_ready` 恒为 false);byteui 目前没有 `panel_tab` / 溢出菜单。以后迁进 byteui 是单独任务。
  - **D3 行为改动(请审阅):** 文件树的 `PanelCommand::OpenFile` 从"系统默认应用"改为"在预览里打开";Project 的文档链接 `OpenLink` 同样改为在 Project 的预览里打开(S5a 现在是系统应用打开)。系统应用打开保留在预览降级页和页签右键里。
  - **D4 项目切换时销毁非当前项目的 webview,** 回来时由库按页签状态重新加载(`Suspended` → `Profiling`),不做 webview 常驻。
    - **G-I3 取舍(已确认):** Project 面板的页签**不落盘**(`behavior_of` 里 `persist_tabs: panel != ViewerPanel::Project`),切项目时 Project 的 `PreviewPane` 整份丢弃,回来是空的;只有 Files 面板的页签跨重启 / 切项目恢复。脏缓冲仍由库的恢复快照兜底(D5)。Dozer 是按项目常驻 workspace,Digger 一期不做。
  - **D5 脏页签保护沿用库的恢复机制** (`ReadRecovery` / `ClearRecovery`);Digger 退出和关项目时对脏页签不弹确认,依赖恢复文件(与 Dozer 现状一致)。

## Review Focus

1. **项目串台:** A 项目的预览 webview 绝不出现在 B 项目上;A 的迟到 IPC / 迟到加载结果在切到 B 后被丢弃;删除项目后它的页签状态、白名单、`preview_state` 文件被清理。→ Task 7、8。
2. **webview 可见性与布局:** 放大对侧、列表收起、预览收起、面板不是所选面板、窗口缩到最小、配对拖到最窄,webview 必须藏起或精确贴合预览区,不遮住页签条 / 查找条 / 其他面板。→ Task 8、10。
3. **脏数据:** 编辑后切页签 / 切项目 / 关项目页签 / 退出,未保存内容可恢复;保存冲突(磁盘被外部修改)出现冲突条并不丢缓冲;保存目标被篡改被拒绝。→ Task 2、10。
4. **旧布局迁移:** 已有 `shell.json` / 项目布局里 Project、Files 是单栏、没有配对状态,升级后不丢面板位置,不出现空白或崩溃。→ Task 5。
5. **页签压力:** 50 个页签、CJK 与超长标题、同名文件、快速开关,页签条翻页 / 溢出菜单正确,`live_count()` 随关闭回落,无句柄泄漏。→ Task 6、10。
6. **与章节编辑器共存:** 同窗口同时有 Markdown 面板与预览时,键盘焦点、复制粘贴、IPC 各走各的通道。→ Task 10。

---

## Dozer(仓库 `../dozer`,worktree `../dozer-p4`,分支 `feat/preview-events-lib`,从当前 `main` 切出)

### Task 1: 基线与页面事件施工图

**Files:**
- Create(digger 仓库):`docs/superpowers/notes/2026-10-19-preview-p4-events-inventory.md`

- [ ] **Step 1: 记录基线。** `cargo test --workspace` 总数与 `dozer-app` / `dozer-panel-preview` 分项;三份黄金的哈希写进提交说明。
- [ ] **Step 2: 写施工图。** 列出 `dozer-app/src/app/update.rs` 里 `EditorWebviewEvent`、`JsonEditorEvent`、`FlyfishEvent`、`ImageAnnotateEvent`、`PlantUmlEvent`、`Tabular*`、`GitLogDiffWebviewEvent` 的处理函数,逐个判 **进库 / 留 dozer-app(原因)/ 拆开(怎么拆)**,标出碰 Dozer 全局的部分(MCP 预览上下文、`dozerd` 通知、Toast、弹窗、键盘焦点)。第一批 Digger 只需要 Editor、Flyfish、ImageAnnotate 三类,其余可留 `dozer-app` 并在库里留 `Message::Host` 的透传。**交用户过目后再继续;拿不准的停下报告。**

### Task 2: 页面事件进库

**Files:**
- Modify:`crates/dozer-panel-preview/src/{message.rs,effects.rs,update.rs,pane.rs}`、`crates/dozer-app/src/app/update.rs`
- Test:`crates/dozer-panel-preview/src/update_tests.rs`

**Interfaces:**
- Consumes:P3 的 `HostEvent`(`host_ipc`);Task 1 施工图的"进库"清单。
- Produces:`Message::Host(HostEvent)`;`Effect::SaveFile { tab_id: usize, path: PathBuf, text: String }`、`Effect::PushHostScript { webview_id: usize, script: String }`;`PreviewPane::take_host_scripts(&mut self, panel: ViewerPanel) -> Vec<(usize, String)>`;保存回执 `Message::SaveFinished { tab_id: usize, result: Result<(), String> }`。

- [ ] **Step 1: 失败的库测试。** 每个"进库"事件至少一条:编辑器改动置脏并更新行列 / 选区;保存请求产出 `SaveFile` 且 `path` 等于页签绑定路径;`SaveFinished(Ok)` 清脏、`Err` 保留缓冲并置错误;Flyfish / ImageAnnotate 的 ready / rendered / failed 驱动页签状态。**安全钉死:** `host_event_for_a_closed_tab_is_ignored`、`host_event_from_a_stale_generation_is_ignored`、`save_is_rejected_when_the_event_path_differs_from_the_bound_path`、`save_is_rejected_outside_the_project_root`、`oversized_text_change_is_rejected_without_changing_the_tab`。
- [ ] **Step 2: 实现。** 把施工图判为"进库"的处理体搬进 `update`;`run_effect` 执行 `SaveFile`(原子写,沿用核心 `text_save`),回投 `SaveFinished`。`dozer-app` 的各分支改成转调 `self.preview_ui(panel, Message::Host(event))` 并执行 `PushHostScript`。
- [ ] **Step 3: 验证并提交。** 总数守恒 + 新增;三份黄金不变。

### Task 3: `AllowedFiles` 与资源根(P3 遗留)

**Files:**
- Modify:`crates/dozer-panel-preview/src/assets.rs`、`crates/dozer-app/src/{runtime.rs,workspace/state.rs}`

**Interfaces:**
- Produces:`#[derive(Clone, Default)] pub struct AllowedFiles(..)`,方法 `insert(&self, path: impl AsRef<Path>)`(入表时规范化,失败则存原路径)、`contains(&self, path: &Path) -> bool`(查表时也规范化请求路径,拒绝目录)、`clear(&self)`、`len(&self) -> usize`;`handle_protocol` 的白名单参数改为 `&AllowedFiles`;`pub fn from_lib_source() -> PathBuf`(开发态资源根,`env!("CARGO_MANIFEST_DIR")/assets/flyfish`)。

- [ ] **Step 1: 失败的库测试(Review Focus 与计划 P3 Task 2 原定三条):** `allowlist_rejects_traversal_encoded_slash_nul_and_symlink_escape`、`allowlist_compares_canonical_paths_and_rejects_directories`(含 `/tmp` → `/private/tmp` 的符号链接用例)、`file_url_roundtrips_names_with_space_cjk_hash_question_percent_ampersand`。
- [ ] **Step 2: 实现并替换 `Workspace.allowed_files` 与 `runtime.rs` 里的 `Arc<Mutex<HashSet<PathBuf>>>`。** 原有 `%2e%2e` 与精确匹配的测试不得改期望。
- [ ] **Step 3: 验证并提交。**

### Task 4: Dozer 收尾与验收

- [ ] **Step 1: 全量门禁:** `cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets`(警告数不高于基线)、边界脚本、`check-shared-deps`、`check-i18n`、依赖闭包检查。
- [ ] **Step 2: 用户目测验收清单(用户做,我给截图):** Files 与 Project 各自打开 `.py`(编辑、保存、脏标、外部修改冲突条)、`.md`、`.html`、`.png`、`.svg`、`.pdf`、`.json`、`.csv`;⌘F;⌘C / ⌘V / ⌘X / ⌘A;缩放;放大一侧;多页签。**验收通过前不合并。**
- [ ] **Step 3: 向用户确认后合并、切 `libs-<date><letter>` tag、推送。**

---

## Digger(仓库 `../digger`,worktree `../digger-p4`,分支 `feat/preview-pairing`;在 Dozer tag 之后开始)

### Task 5: 钉版与配对规则

**Files:**
- Modify:`Cargo.toml`、`Cargo.lock`、`crates/desktop/src/shell.rs`
- Test:`crates/desktop/src/shell.rs`

- [ ] **Step 1: 失败的测试(改写 S5a / S5b-1 的单栏测试):** `project_and_files_are_pairs_with_the_list_first`(`rules(&project_id())` 与 `rules(&files_id())` 都是 `Some`,`pane.spec == PaneSpec::new(false, true)`,`min_first_px == list_min_width()`);`legacy_layout_without_pair_state_loads_with_the_default_split`(不含配对字段的旧 `shell.json` 经 `load` 后,Project / Files 仍在原栏、原顺序,`pair_rects` 用默认分割);删除 `files_panel_is_single_pane` 及 S5a 里同义断言。
- [ ] **Step 2: 升级依赖到 Dozer 新 tag;** 实现 `rules()` 的两个分支(`default_side: Side::Left`)。
- [ ] **Step 3: 验证并提交** (`cargo test -p digger-desktop shell::`)。

### Task 6: 页签控件与 `PanelDisplay` 实现

**Files:**
- Create:`crates/desktop/src/adapters/tab_chrome.rs`、`crates/desktop/src/adapters/preview_display.rs`
- Test:两文件内

**Interfaces:**
- Consumes:库 `PanelDisplay<M>`、`TabArgs`、`OverflowMenuArgs`、`PaneStyle`、`PreviewHover`。
- Produces:`pub struct PreviewDisplay<'a> { pub locale: &'a str, pub scale: f32, pub tab_bar_px: f32 }` 实现 `PanelDisplay<Msg>`,`product_name() == "Digger"`,`wrap(panel, m)` 产出 `Msg::Preview(panel, m)`;`tab_chrome::{panel_tab, overflow_button, overflow_menu, mode_button}`;`tab_display_width(title)` 按字符宽度估算且对 CJK 取全宽。

- [ ] **Step 1: 失败的测试。** `tab_display_width_counts_cjk_as_full_width`、`overflow_entries_mark_the_blank_tab_as_not_closable`、`hover_progress_is_always_zero_and_tooltip_never_ready`、`wrap_routes_each_panel_to_its_own_message`。
- [ ] **Step 2: 实现。** 页签样式取 Dozer `chrome/tab_widget.rs` 的外观(读代码对照,不得改库的布局数学);按钮用 `byteui::interaction::icons`。D2 的无动画约定写进模块头注释。
- [ ] **Step 3: 验证并提交。**

### Task 7: 预览状态与消息路由 `adapters/preview.rs`

**Files:**
- Create:`crates/desktop/src/adapters/preview.rs`
- Modify:`crates/desktop/src/adapters/mod.rs`、`crates/desktop/src/paths.rs`(`preview_state_dir()`)

**Interfaces:**
- Consumes:库 `PreviewPanelState`、`update`、`UpdateCtx`、`run_effect`、`PreviewPersistence`、`Effect`;P3 `preview_native::Host`。
- Produces:`pub struct PreviewHub`:`new(host: preview_native::Host) -> Self`;`set_current_project(&mut self, project: Option<&ProjectInfo>)`;`open(&mut self, panel: ViewerPanel, project: &ProjectInfo, path: PathBuf, line: Option<usize>) -> Vec<Effect>`;`update(&mut self, panel: ViewerPanel, project_id: i64, message: Message) -> iced::Task<Msg>`(未知项目 / 已删除项目的消息丢弃);`state(&self, panel, project_id) -> Option<&PreviewPanelState>`;`desired(&self, panel: ViewerPanel, project: &ProjectInfo, locale: &str, rect: impl Fn(&PreviewWebviewSpec) -> Option<LogicalRect>) -> Vec<(PreviewWebviewSpec, LogicalRect)>`;`forget_project(&mut self, project_id: i64)`(删项目:清状态、白名单、`preview_state` 文件);`struct DiggerPersistence` 实现 `PreviewPersistence`。

- [ ] **Step 1: 失败的测试(Review Focus 1)。** `a_late_message_for_a_forgotten_project_is_dropped`、`two_projects_keep_independent_tabs_per_panel`、`files_and_project_panels_do_not_share_tabs`、`forget_project_removes_state_allowlist_and_the_persisted_file`、`effects_without_a_digger_meaning_are_ignored_not_errors`(`ContextChanged`、`TabSelected`)、`allow_file_effect_populates_the_host_allowlist_with_canonical_paths`。
- [ ] **Step 2: 实现。** `UpdateCtx.caps` 取 Global Constraints 的能力集;`PersistTabs` 经 `DiggerPersistence` 写盘;`AllowFile` → `Host::allow_file`;`OpenExternal` → 系统打开器(沿用 `adapters/files.rs` 的 `open_with_default`);启动时 `restore_tabs` 恢复当前项目的页签。
- [ ] **Step 3: 验证并提交。**

### Task 8: 接进产品壳:配对视图、点文件、webview 同步

**Files:**
- Modify:`crates/desktop/src/product.rs`、`crates/desktop/src/adapters/{files.rs,project.rs}`
- Test:`crates/desktop/src/product.rs`

**Interfaces:**
- Produces:`Msg::Preview(ViewerPanel, dozer_panel_preview::Message)`、`Msg::PreviewHost(preview_native::Event)`;`fn preview_native_rect(pair: PairRects, layout: PairLayout) -> Option<Rect>`(纯函数:配对第二块下移 `preview_chrome_top_px()`,上下各留与 Dozer 一致的边距 8px);`project_view` / `files_view` 改为 `pair_view(area, list, preview, ...)`,列表收起 / 放大时退化为单块并隐藏 webview。

- [ ] **Step 1: 失败的测试。** `preview_rect_is_the_second_block_below_the_chrome`、`preview_rect_is_none_when_the_pair_is_collapsed_or_the_panel_is_not_selected`(Review Focus 2);`open_file_command_opens_the_preview_and_not_the_system_opener`;`project_open_link_opens_the_project_preview`;`switching_the_project_tab_destroys_the_other_projects_webviews`;`closing_a_project_tab_and_deleting_a_project_drop_its_preview_webviews`;`maximizing_the_other_side_hides_every_preview_webview`。
- [ ] **Step 2: 实现。** `body` 里 Project / Files 分支改用 `pair_view`(预览块用库 `view::pane` + `PreviewDisplay`);`sync_native` 追加预览 webview:对当前项目、被选中且可见的 Files / Project 各取 `PreviewHub::desired`,经 `Host::sync`;任一条件不满足即传空清单(销毁);`Msg::Projects` 换当前项目与项目删除两条路径调用 `PreviewHub::set_current_project` / `forget_project`;`PanelCommand::OpenFile` 与 `OpenLink` 改映射为 `PreviewHub::open`(D3)。
- [ ] **Step 3: 验证并提交。** `cargo test -p digger-desktop`、`clippy`、`fmt`。

### Task 9: P3 遗留:资源根、夹具与 selftest

**Files:**
- Create:`crates/desktop/src/preview_assets.rs`、`crates/desktop/examples/fixtures/preview/{hello.py,notes.md,page.html,pic.png,logo.svg,data.json,sheet.csv,doc.pdf,带 空格 #1.txt}`
- Modify:`crates/desktop/examples/preview_smoke.rs`、`scripts/package-macos.sh`

**Interfaces:**
- Produces:`pub fn roots() -> PathBuf`(打包态 `Contents/Resources/flyfish` 的父目录规则沿用库 `bundled_assets_root`,开发态用库 `from_lib_source()`);selftest 在夹具缺失时**失败**而不是跳过。

- [ ] **Step 1: 失败的测试。** `roots_in_dev_resolve_to_the_pinned_checkout`、`bundled_layout_has_the_three_first_batch_dirs`(对 `package-macos.sh debug` 的产物目录断言 `editor/ flyfish/ image-annotate/` 存在、`flyfish/vendor/pdf` 不存在)、`selftest_fails_when_a_fixture_is_missing`。
- [ ] **Step 2: 实现,** 示例改用 `roots()` 与夹具目录(不再依赖 `../dozer`);selftest 断言 `.py .md .html .png .svg` 各 1 个 webview、`.json .csv .pdf` 为 0 且是降级页、关闭全部后 `live_count() == 0`。
- [ ] **Step 3: 验证并提交。** 打一个调试包,从包里启动 selftest 通过。

### Task 10: 全量验证、真机回归与文档

- [ ] **Step 1: 门禁:** `cargo build --workspace`、`cargo test --workspace`(记录各分项数)、`clippy`(零新增告警)、`fmt --check`、`check-shared-deps.sh`、`check-host-deps.sh`(含 `--self-test`)、`check-shell-regions.sh`、`Cargo.lock` 重复检查;`bash scripts/run-editor-selftest.sh` 通过(章节编辑器不受影响)。
- [ ] **Step 2: 自动化生命周期检查(并入 selftest 或单独示例):** 连续开关预览页签 50 次、切项目页签 20 次、放大 / 还原 20 次后 `live_count()` 回到期望值;50 个页签打开不崩溃。
- [ ] **Step 3: 真机回归(隔离数据目录 `DIGGER_DATA_DIR`;用户点击,我给截图):** 左栏顺序 Project → 文件;Project 与 Files 都是"左列表 | 右预览";文件树双击 `.py` 在右边打开并可编辑保存(磁盘内容变化)、脏标、外部修改冲突条;`.md` 渲染;`.html` 渲染且隔离;`.png` `.svg` 渲染;`.json` `.csv` `.pdf` 降级并能用系统应用打开;Project 文档链接在右边预览;切项目页签预览随之切换且不串;放大一侧 webview 藏起;拖配对分割线 webview 跟随;列表收起;含空格 / 中文 / `#` 的文件名;Markdown 面板与预览同时使用;重启后页签恢复。逐项截图记录,任一项不符先停下,不要为了"过"而改验证。
- [ ] **Step 3b: 对照 Dozer 目测** 同一份清单里 Dozer 与 Digger 的外观 / 行为差异,记入 spec 的"有意偏离"。
- [ ] **Step 4: 文档:** S5b-2 spec 状态改"已实现"并写实现结果与 D3 的行为变化;window-shell 的 S5 行改"S5 已完成";S5a / S5b-1 spec 中"单栏"的约定标注已由 P4 取代。
- [ ] **Step 5: 合并到 `main`(快进)、清理 worktree 与分支。**
- [ ] **Step 6: 联合发布(项目硬性要求:每个面板迁移完成后两个产品各发一版)。** Files / Project 两个面板在两个产品里都落地、真机回归通过后:Dozer 走 `scripts/build-macos-app.sh release`,Digger 走 `bash scripts/release.sh`(不提交版本号以外的东西、不打 tag、不推送);两个产物装到本机启动一次确认版本号与面板可用。**发布含升版本、提交版本号、安装,是对外动作,先向用户确认。** 并包含 S5a 评审修复与 P3。
- [ ] **Step 7: 汇报:** 各仓库测试数、依赖图结论、真机截图结论、D1–D5 最终落地情况、没验的项。

## 自检记录

- 覆盖:spec §5.4 的"配对 + OpenFile 映射"落在 Task 5–8;§6 P4 的"更新 S5a、S5b-1 单栏测试"落在 Task 5 Step 1;§7 的真机清单落在 Task 10;P3 的遗留(页面事件、白名单规范化与测试、`preview_assets.rs`、夹具与 selftest)分别落在 Task 2、3、9。
- 风险:Task 6 的页签控件与库 `PanelDisplay` 的 20 个方法是最大的一块新代码,若超过预期可拆成两个提交;Task 2 的范围由 Task 1 施工图决定,Digger 只需要 Editor / Flyfish / ImageAnnotate 三类。
- 需用户审阅:D3(`OpenFile` 与 `OpenLink` 的行为变化)、D4(切项目销毁 webview)、D5(脏页签不弹确认)。
