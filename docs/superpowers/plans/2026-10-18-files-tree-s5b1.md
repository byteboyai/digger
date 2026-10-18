# S5b-1 文件树共享库与 Digger 接入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Dozer 的文件树抽成共享库 `dozer-panel-files`,Digger 依赖同一份实现,在左栏 Project 之后挂出文件树面板;树根跟随当前项目,双击文件用系统默认应用打开。

**Architecture:** 两段:(A)Dozer 里抽库,每一步 Dozer 都能编译且测试全绿,最后发新的 `libs-` tag;(C)Digger 升级依赖、接面板。库里原来"直接调原生菜单、直接开预览、直接读 `crate::project`"的三处耦合,分别换成:宿主请求队列(`FilesRequest::ShowContextMenu`)、跨面板命令(`PanelCommand::OpenFile`)、库内自带的文件系统层(`fs` 模块)。不支持的动作由产品注入的 `FilesCapabilities` 过滤,不显示。

**Tech Stack:** Rust、iced 0.14、`dozer-panel-host`、`bytegit`(git 着色、监听、分支)、byteui(主题、图标;菜单渲染由 Digger 的 `adapters/menu_overlay.rs` 承担,见 Task 9)、`dozer-i18n`。

**Spec:** `docs/superpowers/specs/2026-10-18-files-tree-s5b1-design.md`。模板:S5a 计划 `docs/superpowers/plans/2026-10-18-project-panel-s5a.md`(同一套 Dozer worktree、门禁登记方式)。

## Global Constraints

- **库边界:** `dozer-panel-files` 的依赖闭包只含 `dozer-model`、`dozer-services`、`dozer-i18n`、`dozer-panel-host`、byteui、bytehost-core、bytehost-desktop、`bytegit`、iced、`serde*`、`tokio`;**不得**出现 `dozer-app`、`dozer-client`、`dozer-core`。
- **只用 git tag 依赖**,不提交本地 `[patch]`、不提交带本地路径的依赖(Dozer 仓库内部 `path = "../..."` 除外)。
- **Dozer 用户可见行为不变**;被搬走的测试随代码一起走,不删、不改期望值;抽库前后 Rust 测试总数守恒。
- **打 tag、推送是对外动作,执行前向用户确认**;Digger 没有 remote,不推送、不打 tag。
- **Dozer 与 Digger 同日升级依赖**(ADR)。
- **前置:S5a 已合并**(Digger 一侧要用它的按项目布局、`ProjectsState::current_project()` 和左栏 Project 图标)。Part A 不依赖 S5a,可先做。
- **主工作区别人未提交的改动不动**:Dozer 主工作区有未提交的 `Cargo.lock`、`dozer-app/Cargo.toml`、`Info.plist` 改动;所有实现在独立 worktree 里做(Dozer:`../dozer-panel-files`,分支 `feat/panel-files-lib`;Digger:`../digger-s5b1`,分支 `feat/files-tree`)。
- 不引入新的日志依赖。
- **文件操作不得越出当前项目目录**。
- Digger 里 `FilesCapabilities` 五个字段全为 `false`;Digger 的文件树面板是单栏(`shell::rules("digger.files")` 为 `None`)。

## Review Focus

1. **越界操作:** 重命名为含路径分隔符或 `..` 的名字、把目录移动/粘贴到它自己的子目录里、符号链接指向项目外——都被拒绝并给出错误提示,不动磁盘。→ Task 1。
2. **目录里有符号链接环或上万个条目:** 树加载不死循环、不卡住界面线程。→ Task 1。
3. **没有已开项目 / 项目目录已被移走:** 面板空状态或错误提示,监听不 panic,不对旧路径继续操作。→ Task 8。
4. **切换项目时还有在途操作:** 带旧 `project_id` 的完成消息(`DeleteApplied`、`PasteDone`、`FileDropDone`、`StatusesRefreshed`)被丢弃,不会刷到新项目的树上。→ Task 4、Task 8。
5. **能力全关时的菜单:** 不出现文件历史/回滚/搜索/发送到 Agent/预览页签相关条目;菜单不会出现空菜单、首尾分隔线或连续分隔线。→ Task 3。

---

## Part A — Dozer(仓库 `../dozer`,worktree `../dozer-panel-files`,分支 `feat/panel-files-lib`,从 `main` 切出)

### Task 1: 基线、空壳 crate、搬文件系统层与纯逻辑

**Files:**
- Create: `crates/dozer-panel-files/Cargo.toml`、`src/lib.rs`、`src/fs.rs`、`src/git_status.rs`、`src/external_apps.rs`
- Modify: workspace `Cargo.toml`、`crates/dozer-app/Cargo.toml`、`crates/dozer-app/src/project.rs`(改为 `pub use dozer_panel_files::fs::*` 的转调,保持 `crate::project::*` 路径不变)、`crates/dozer-app/src/external_apps.rs`(同)、`extensions/files/git_status.rs`(删,改引用)

**Interfaces:**
- Produces(原样搬自 `dozer-app/src/project.rs` 与 `external_apps.rs`,只改 `use` 路径):`fs::{PathKind, TreeRow, FileTree, ProjectFsError, FsOp, HIDDEN, copy_dir_recursive, paste_item, move_item_to, move_item, is_single_path_component, path_string}`、`git_status::{FileGitStatus, TreeState, ...}`、`external_apps::{ExternalAppsConfig, OpenError, load_from, save_to}`。

- [ ] **Step 1: 记录基线。** 在 worktree 里 `cargo test -p dozer-app 2>&1 | grep "^test result"`,记下总通过数,作为后续每个 Dozer 任务的对比基准。
- [ ] **Step 2: 建 crate。** `Cargo.toml` 照抄 `dozer-panel-todo` 的依赖写法(同样的 byteui / bytehost tag),只留 §Global Constraints 列的依赖,dev-dependencies 加 `tempfile`。`cargo build -p dozer-panel-files` 通过。
- [ ] **Step 2b: 写越界与环路的钉死测试**(放 `fs.rs`;若搬来的原测试已覆盖则只改名保留,缺哪条补哪条):`rename_target_with_separator_or_dotdot_is_not_a_single_component`(`is_single_path_component("a/b") == false`、`("..") == false`、`("") == false`)、`move_item_to_refuses_to_move_a_dir_into_itself`、`paste_item_refuses_to_paste_a_dir_into_its_own_subtree`、`file_tree_does_not_follow_a_symlink_cycle`(临时目录里 `ln -s . loop`,展开全部后行数有界且在 1 秒内返回)、`file_tree_lists_ten_thousand_entries_without_blocking_the_test_for_long`(1 万个空文件,构建 `FileTree` 的耗时断言在合理上限内,上限取实测的 5 倍并写进测试注释)。
- [ ] **Step 3: 运行。** `cargo test -p dozer-panel-files`;已满足的行为直接通过,不满足的先记录为已知缺陷,**按 Dozer 现状保持行为不变**(本计划是抽库,不是修缺陷),并写进 Task 6 的报告,留给用户裁决。
- [ ] **Step 4: 搬代码与测试,`dozer-app` 里留转调。** `git_status.rs` 用到的 `bytegit` 依赖加进新库的 `Cargo.toml`(同 `dozer-app` 现用的 tag)。
- [ ] **Step 5: 验证。** `cargo test -p dozer-panel-files -p dozer-app` 总数 = 基线 + 新增钉死测试数(两库之和)。
- [ ] **Step 6: 提交** `refactor(files): 新建 dozer-panel-files,搬文件系统层与纯逻辑`。

### Task 2: 本地化与门禁登记

**Files:**
- Create: `crates/dozer-panel-files/src/i18n.rs`、`locales/{en,zh-CN}.json`(从 `crates/dozer-app/locales/panels/files/` 搬)
- Modify: `crates/dozer-app/src/i18n.rs`(owner `dozer.files` 的 `include_str!` 改指向新库,仿 `dozer.todo` 的写法)、`scripts/i18n/check_migration.py`、`build_inventory.py`、`scan_sources.py`(按 `dozer-panel-todo` 的条目各加 `dozer-panel-files`)

**Interfaces:**
- Produces: `dozer_panel_files::FILES_OWNER: &str = "dozer.files"`;`i18n::{t, t_args, keyed, keyed_args}`,签名与 `extensions/files/mod.rs:23-30` 的 `t` / `t_args` 一致。

- [ ] **Step 1: 搬 `i18n` 与 `locales/`,改 owner 注册与三个脚本。**
- [ ] **Step 2: 验证。** `bash scripts/check-i18n.sh` 通过;`cargo test -p dozer-panel-files -p dozer-app` 总数守恒。
- [ ] **Step 3: 提交。**

### Task 3: 三处耦合的接缝:`OpenFile` 命令、菜单请求、能力开关

**Files:**
- Modify: `crates/dozer-panel-host/src/lib.rs`(`PanelCommand` 加变体)、`crates/dozer-app/src/app/`(`run_panel_command` 加分支)
- Create: `crates/dozer-panel-files/src/adapter.rs`、`src/menu.rs`
- Test: `crates/dozer-panel-files/src/menu.rs`、`crates/dozer-panel-host/src/lib.rs`

**Interfaces:**
- Produces:
  - `dozer_panel_host::PanelCommand::OpenFile { path: PathBuf }`。
  - `adapter::FilesCapabilities { pub file_history: bool, pub rollback: bool, pub search: bool, pub send_to_agent: bool, pub preview_tabs: bool }`,`impl Default`(全 `false`),`FilesCapabilities::all()`(全 `true`,Dozer 用)。
  - `menu::MenuItem<M> { Entry { icon: Option<IconKind>, label: String, enabled: bool, msg: M }, Separator }`(形状对应 Dozer `menu_spec::MenuSpecItem` 的 `Entry` / `Separator`,不含 `native_menu` 类型)。
  - `menu::context_menu_items(params: FileContextMenuParams, caps: &FilesCapabilities, locale: &str) -> Vec<MenuItem<Message>>`;输出已规整:无首尾分隔线、无连续分隔线、不为空时才返回(空则返回空 `Vec`,由调用方不弹)。
  - `adapter::FilesRequest { ShowContextMenu { items: Vec<MenuItem<Message>>, at: (f32, f32) } }`;`WorkspaceState` 里加 `requests: Vec<FilesRequest>` 与 `take_requests(&mut self) -> Vec<FilesRequest>`。**库的 `update` 不再调用任何原生菜单函数**,右键时只往 `requests` 里推一条。
- Consumes: Task 1 的 `fs`、`git_status`、`external_apps`。

- [ ] **Step 1: 写失败测试**(`menu.rs`):`with_all_capabilities_off_no_gated_entry_appears`(遍历 `{文件、目录、根目录、有/无剪贴板、git 仓库/非仓库}` 的组合,断言条目标签里不含文件历史/回滚/搜索/发送到 Agent 对应的本地化标签)、`menu_never_starts_or_ends_with_a_separator_nor_has_two_in_a_row`(同样遍历,caps 取全关与全开两种)、`with_all_capabilities_on_matches_the_legacy_menu`(对每个组合,`context_menu_items(.., all())` 的条目标签序列与迁移前 `context_menu_spec` 的一致——先在 Dozer 现有代码上抓取这份黄金序列,写进测试)。
- [ ] **Step 2: 运行确认失败。** `cargo test -p dozer-panel-files menu::`。
- [ ] **Step 3: 实现 `menu.rs`** 把 `view.rs` 里的 `context_menu_spec` / `context_menu_items` 逻辑搬来并按 `caps` 过滤、规整分隔线;`PanelCommand::OpenFile` 加进 `dozer-panel-host`,`PanelCommand` 的两处 `match`(`dozer-panel-host` 内与 `dozer-app` 的 `run_panel_command`)补分支:Dozer 把它映射到现有的预览打开逻辑(与树里双击文件原来的行为一致)。
- [ ] **Step 4: 验证。** `cargo test -p dozer-panel-host -p dozer-panel-files -p dozer-app` 总数守恒(加新增条数)。
- [ ] **Step 5: 提交。**

### Task 4: 状态、更新、视图

**Files:**
- Create: `crates/dozer-panel-files/src/{state,update,view,tree}.rs`
- Modify: `crates/dozer-app/src/extensions/files.rs` 与 `files/*`(缩成重新导出)、`app/update.rs`、`app/view.rs`、`app/app.rs`(调用点)
- Test: 随代码搬(`extensions/files/mod.rs` 里的测试约 2000 行)

**Interfaces:**
- Produces(签名沿用现状,只替换归属与耦合类型):
  - `pub struct WorkspaceState`;`outbox` 改为 `dozer_panel_host::NoticeOutbox`,`take_outbox(&mut self) -> Vec<dozer_panel_host::Notice>`。
  - `pub enum Message`(原样搬;`TextInputMenuOpen` 的载荷改为 `adapter::TextInputTarget { id: Id, secure: bool }`;`TabContextMenuOpen` / `TabContextMenuCloseTab` 的 `kind: crate::app::PanelKind` 改为 `adapter::PreviewKind { Files, Project }`,这两条消息只在 `caps.preview_tabs` 为真时产生)。
  - `pub fn update(ws_state: &mut WorkspaceState, app_state: &mut AppState, msg: Message, project_id: i64, handle: &tokio::runtime::Handle, emit: impl Fn(Message) + Send + 'static, external_apps: &ExternalAppsConfig, caps: &FilesCapabilities, locale: &str)`——原来的 `agent_terminal_visible: bool` 并入 `caps.send_to_agent`,不再单独传。
  - `pub fn view<'a>(ws_state: &'a WorkspaceState, width: Length, outer: Border, search_hover_t: f32, dotfiles_hover_t: f32, branch_hover_t: f32, locale: &'a str) -> Element<'a, Message, iced_widget::Theme, iced_renderer::Renderer>`(与原一致)。
  - `adapter::PanelDisplay`:沿用 S5a 的形状,只含树视图用到的方法(`hover_progress`、`panel_head`、`header_padding`、`locale`);`crate::workspace::tree_row_font_size` 与 `chrome::homespace::home_panel_head` 经它或 byteui 取得。
  - 分支选择弹层 `branch_picker_popup` 改成返回 `Element`,由产品决定放在哪;其内部用到的 `chrome::menu::{item_row, shell_frosted}` 库**不得**再依赖 `chrome::menu`。已核实 byteui 没有菜单外框/菜单项组件(`interaction::context_menu` 只是右键触发封装),原先「先停下报告」的条件已触发,处理定为:分支选择改为库只产出 `Vec<MenuItem<Message>>` 并经 `FilesRequest` 交给产品渲染(与右键菜单同一路径),Dozer 继续用原来的弹层,Digger 用 `adapters/menu_overlay.rs`。

- [ ] **Step 1: 写失败测试**(随代码搬来的保持原样,另加):`late_completion_for_a_previous_project_is_dropped`(以 `project_id=1` 发起粘贴/删除,再用 `project_id=2` 调 `update`,送入带旧 id 的 `PasteDone` / `DeleteApplied` / `FileDropDone` / `StatusesRefreshed`,断言树与提示都不变)、`right_click_pushes_a_context_menu_request_instead_of_showing_one`(`RightClickAt` + `ContextMenuOpen` 后 `take_requests()` 有且仅有一条 `ShowContextMenu`,且 `items` 与 `context_menu_items` 同序)、`double_click_on_a_file_emits_open_file_command`(`TreeRowDoubleClick` 对文件行,`ws_state.host` 里出现 `PanelCommand::OpenFile { path }`)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬代码,逐处替换耦合。** 清单:`crate::extensions::toast` → `NoticeOutbox`;`crate::app::{PanelKind, TextInputTarget}` → `adapter` 同名类型;`crate::chrome::native_menu::show_align_no_icon_left`(`update.rs` 约 100、165 行)→ 推 `FilesRequest::ShowContextMenu`;`crate::preview::*` 与 `crate::workspace::*` 的引用逐处列出,每处换成 `PanelCommand` 或适配接缝,**列表写进提交说明**;`crate::project::*` → `crate::fs::*`。
- [ ] **Step 4: Dozer 侧适配。** 新建 `host_adapters/files_panel.rs`:为 `App` 实现 `PanelDisplay`;排空 `take_requests()`:`ShowContextMenu` → 转成 Dozer 的 `MenuSpec` 并调原有原生菜单/iced 弹层,把用户选中的消息再送回 `files::update`(行为与原来同步弹出一致);`app/update.rs`、`app/view.rs`、`app/app.rs` 的调用点改用库的类型,`caps` 传 `FilesCapabilities::all()`。`scripts/audit/check_panel_boundary.py` 登记新库,`extensions/` 下不得出现 `crate::app::App`。
- [ ] **Step 5: 验证。** `cargo test -p dozer-panel-files -p dozer-app` 总数守恒(两库之和 = 基线 + 新增);`cargo tree -p dozer-panel-files | grep -E "dozer-(app|client|core)"` 无输出;`python3 scripts/audit/check_panel_boundary.py` 通过。
- [ ] **Step 6: 提交。**

### Task 5: 清理与全量验证

- [ ] **Step 1:** `grep -rn "fn paste_item\|fn move_item_to\|struct FileTree\|struct FileGitStatus\|enum FileGitStatus" crates` 只命中 `dozer-panel-files`;`extensions/files/` 只剩重新导出;`project.rs`、`external_apps.rs` 只剩转调。
- [ ] **Step 2:** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-i18n.sh`;总数与基线一致(加新增)。
- [ ] **Step 3: 请用户在 Dozer 上目测验收**(文件树外观与交互与抽库前一致:展开/折叠、右键菜单全部条目、新建/重命名/删除/移动/粘贴、拖入文件、git 着色、分支选择、搜索框、隐藏文件开关、双击文件打开预览)。**未验收不继续。**

### Task 6: 发 tag

- [ ] **Step 1: 合并到 Dozer `main`、切 `libs-2026-10-<日期>x` tag、推送——向用户确认后执行。** 该 tag 里 bytehost / byteui / bytegit 的版本必须与 Digger 当时用的一致(逐个 crate 核对)。
- [ ] **Step 2: 报告里写明 Task 1 Step 3 中发现的、按现状保持的行为缺陷**(若有),由用户裁决是否另开修复。

---

## Part C — Digger(worktree `../digger-s5b1`,分支 `feat/files-tree`,从已含 S5a 的 `main` 切出)

### Task 7: 升级依赖并接入面板与左栏图标

**Files:**
- Modify: `Cargo.toml`(dozer 四个库 + `dozer-panel-project` + `dozer-panel-files` → Task 6 的 tag)、`Cargo.lock`、`crates/desktop/Cargo.toml`、`crates/desktop/src/shell.rs`(`files_id()`、`all_panels()`、`meta()`、`defaults()`)、`crates/desktop/src/product.rs`、`crates/desktop/src/adapters/mod.rs`
- Create: `crates/desktop/src/adapters/files.rs`
- Test: `crates/desktop/src/shell.rs`、`crates/desktop/src/adapters/files.rs`

**Interfaces:**
- Produces:
  - `shell::files_id() -> PanelId`(`"digger.files"`);`meta`:`("文件", <与 Dozer 文件树同一个图标>)`;`defaults().rails[0] = [project_id(), files_id(), markdown_id(), info_id()]`,`selected[0]` 仍为 `markdown_id()`。
  - `adapters::files::FilesPanel`:`new(external_apps_path: PathBuf) -> Self`;`set_project(&mut self, project: Option<&ProjectInfo>)`(重建树、重启监听,见 Task 8);`view(&self, locale: &str) -> Element<'_, Msg>`;`update(&mut self, msg: dozer_panel_files::Message, project_id: Option<i64>) -> Task<Msg>`;`take_requests(&mut self) -> Vec<FilesRequest>`;`take_notices(&mut self) -> Vec<dozer_panel_host::Notice>`;`take_commands(&mut self) -> Vec<PanelCommand>`。
  - `Msg::Files(dozer_panel_files::Message)`。
  - 构造时 `FilesCapabilities::default()`(五项全关)。
- Consumes: S5a 的 `ProjectsState::current_project()`、`shell::project_id()`。

- [ ] **Step 1: 升级依赖并过依赖门禁,单独成一个提交。** `Cargo.lock` 重复检查输出 `ok: one version each`;`bash scripts/check-shared-deps.sh`、`bash scripts/check-host-deps.sh` 通过。
- [ ] **Step 2: 写失败测试**(`shell.rs`):`files_follows_project_on_the_left_rail_by_default`(`defaults().rails[0][..2] == [project_id(), files_id()]`)、`legacy_layout_without_files_gets_it_placed_after_project`(不含 `digger.files` 的旧 `shell.json` 经 `load` 后,`model.rail(Side::Left)` 里 `files_id()` 紧跟 `project_id()`,其余相对顺序不变)、`files_panel_is_single_pane`(`rules(&files_id()) == None`)。
- [ ] **Step 3: 运行确认失败。** `cargo test -p digger-desktop shell::`。
- [ ] **Step 4: 实现 `shell.rs` 改动与 `FilesPanel`,`product.rs` 接线。** 面板放进 `body`,按 `usage_view` 的方式;后台结果回流沿用 `UsagePanel` 的 `tx/rx` 做法;`catalog` 登记 `files_id()`。
- [ ] **Step 5: 验证。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`。
- [ ] **Step 6: 提交。**

### Task 8: 树根跟随项目、监听、git 着色

**Files:**
- Modify: `crates/desktop/src/adapters/files.rs`、`crates/desktop/src/product.rs`
- Test: `crates/desktop/src/adapters/files.rs`

**Interfaces:**
- Consumes: `bytegit::watch`(feature `watch`;`dozer-panel-files` 内部已用,Digger 不直接依赖它)、`ProjectsState::current_project_id()`。
- Produces: `FilesPanel::set_project` 在项目变化时:丢弃旧树与旧监听,按新项目路径重建;无项目则回到空状态;**把"当前 project_id"写进库的 `update` 调用,使旧 id 的完成消息被丢弃**(库已按 `project_id` 过滤,见 Task 4)。

- [ ] **Step 1: 写失败测试:** `without_a_project_the_panel_is_empty_and_requests_nothing`(`set_project(None)` 后 `view` 不 panic、`take_requests()` 与 `take_commands()` 为空)、`switching_projects_rebuilds_the_tree_root`(两个临时目录各放不同文件,`set_project(A)` → `set_project(B)`,树的可见行只含 B 的文件)、`a_project_whose_directory_vanished_shows_a_notice_and_does_not_panic`(登记项目后删除目录再 `set_project`,`take_notices()` 有一条错误提示)、`late_paste_done_from_previous_project_is_ignored`(`set_project(A)` 发起粘贴、切到 B 后送入 A 的 `PasteDone`,B 的树不变)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 实现。** 项目页签切换与项目被移除(S5a 的 `Message::Removed`)时调 `set_project`;监听事件回流成 `Message::StatusesRefreshed` / 树刷新消息。
- [ ] **Step 4: 验证与提交。** `cargo test --workspace`。

### Task 9: 点文件、右键菜单与对话框

**Files:**
- Modify: `crates/desktop/src/adapters/files.rs`、`crates/desktop/src/product.rs`
- Test: `crates/desktop/src/adapters/files.rs`

**Interfaces:**
- Consumes: `PanelCommand::OpenFile { path }`、`FilesRequest::ShowContextMenu { items, at }`、Digger 的 `adapters/menu_overlay.rs`(byteui 没有菜单渲染,见该条注)。
- Produces: `adapters::files::open_with_default(path: &Path) -> std::io::Result<()>`(macOS 走 `open <path>`,不经 shell 拼接字符串,路径作为独立参数);产品把 `OpenFile` 映射到它;`ShowContextMenu` 渲染成 `menu_overlay` 弹层(byteui 的 `interaction::context_menu` 只是「右键触发」封装(`MouseArea::on_right_press`),**没有**菜单外框与菜单项渲染——那部分(`shell`、`item`、分隔线、悬停样式)在 Dozer 的 `chrome/menu.rs`。Digger 因此自备一个菜单弹层模块 `crates/desktop/src/adapters/menu_overlay.rs`(外框与行样式对照 Dozer `chrome/menu.rs` 与 `workspace.json` 的 `context_menu` 区域;谁先做谁建,另一方复用),长期应迁入 byteui),点选后把条目的 `msg` 送回 `Msg::Files`,点空白或按 Esc 关闭。

- [ ] **Step 1: 写失败测试:** `open_file_command_uses_the_system_opener_with_the_path_as_a_single_argument`(把"执行 `open`"抽成可注入的函数,测试注入一个记录参数的假实现,路径含空格与引号时参数数组恰为 `[path]`)、`the_context_menu_request_is_not_dropped_when_no_project_is_open`(无项目时右键空白区域不 panic 也不弹空菜单)、`an_empty_item_list_does_not_open_a_popup`。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 实现。** 菜单弹层位置取 `at`,并钳制在窗口内(沿用 byteui `geometry::context_menu_width/height` 度量);删除确认与移动对话框用库里已有的 `delete_confirm_spec` / `files_move_card` 元素,放在面板内;分支选择弹层:若在 Digger 里放不下,降级为只读显示当前分支并把降级写进报告(设计 D6)。
- [ ] **Step 4: 验证与提交。**

### Task 10: 全量验证、真机回归与文档

- [ ] **Step 1:** `cargo build --workspace`、`cargo test --workspace`(记录通过数)、`cargo clippy --workspace --all-targets`(零告警)、`cargo fmt --check`、`bash scripts/check-shared-deps.sh && bash scripts/check-host-deps.sh && bash scripts/check-host-deps.sh --self-test && bash scripts/check-shell-regions.sh`、`Cargo.lock` 重复检查。
- [ ] **Step 2: 真机回归**(隔离数据目录 `DIGGER_DATA_DIR`;只能截图,点击要人来做):左栏顺序 Project → 文件;树根是当前项目;展开/新建/重命名/删除确认/移动/复制粘贴/从 Finder 拖入;双击任意文件在系统默认应用打开;在 Finder 里增删文件后树自动更新、git 着色随之变化;右键菜单里没有文件历史/回滚/搜索/发送到 Agent;切换项目页签时树随之切换。逐项截图记录,任一项不符先停下,不要为了"过"而改验证。
- [ ] **Step 3: 文档。** `2026-10-14-window-shell-design.md` 的 S5 行改为"S5a、S5b-1 已完成,S5b-2 预览引擎待做";`2026-10-18-files-tree-s5b1-design.md` 状态改"已实现"并补"实现结果";在设计 Q1 处记下"Markdown 面板打开指定文件"的排期结论。
- [ ] **Step 4: 合并到 `main`(快进)、清理 worktree 与分支,** 不碰主工作区未提交的改动。
- [ ] **Step 5: 发布新版本(项目硬性要求:每个面板迁移完成后,Dozer 与 Digger 各重新发布一个版本)。** 文件树面板迁移在两个产品里都落地、真机回归通过后:① Dozer 走它自己的发布流程(`scripts/build-macos-app.sh release`:升版本号 → 门禁 → 打包);② Digger 走 `bash scripts/release.sh`(门禁 → 升版本 → 打包 → 签名校验 → 压缩;不提交、不打 tag、不推送)。两个产物都要装到本机启动一次确认版本号与面板可用。**发布(含升版本、提交版本号、安装)是对外动作,先向用户确认再执行**;报告里写明两个新版本号与产物路径。
- [ ] **Step 6: 汇报:** 各仓库测试通过数、依赖图结论、真机截图结论、D1–D6(并附 Step 5 的两个新版本号) 最终落地情况(尤其 D6 分支弹层是否降级)、Task 1 Step 3 发现的行为缺陷、哪些没验。

---

## Self-Review

- **Spec 覆盖:** G1 → Task 1–5;G2 → Task 7;G3 的各项 → Task 4(库内)+ Task 9、10(Digger 实测);G4 / `PanelCommand::OpenFile` → Task 3、9;能力开关 → Task 3、7;树根跟随项目、监听与着色 → Task 8;§6.1 表里 `external_apps` / `project::*` / `menu_spec` / `chrome` / `panel_host` / `preview::*` 各项去向 → Task 1、3、4;D1 → Task 7;D2 → Task 3;D3 → Task 3;D4 → Task 9;D5 → Task 9;D6 → Task 4、9。
- **类型一致:** `FilesCapabilities`、`FilesRequest`、`MenuItem` 在 Task 3 定义,Task 4、7、9 引用同名;`update` 的新签名(`caps` 取代 `agent_terminal_visible`)在 Task 4 定义,Task 7 的 `FilesPanel::update` 按它调用;`PanelCommand::OpenFile` 在 Task 3 定义,Task 4、9 引用。
- **风险点:** Task 4 里 `crate::preview::*` 与 `crate::workspace::*` 的逐处替换是读代码时最不确定的一块,清单要写进提交说明;分支选择弹层依赖的 `chrome::menu` 组件若 byteui 没有等价物,先停下报告,不要改外观;Task 1 Step 3 发现的既有缺陷按现状保持,不在本计划里顺手修;tag 与推送是对外动作;Digger 部分依赖 S5a 已合并。
