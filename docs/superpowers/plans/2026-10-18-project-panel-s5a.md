# S5a Project 面板共享库与 Digger 接入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Dozer 的 Project 面板抽成共享库 `dozer-panel-project`,Digger 依赖同一份实现,左栏第一个图标是 Project;Digger 的布局按项目各存一份;顶栏项目页签可点且不被原生拖窗吞掉。

**Architecture:** 三个仓库、三段:(A)Dozer 里抽库,每一步 Dozer 都能编译且测试全绿,最后发新的 `libs-` tag;(B)bytehost 里给 `bytehost-chrome` 加顶栏拖窗保护,发新 tag;(C)Digger 同日升级这两个 tag,接面板、拆布局、接拖窗保护。库只依赖 `dozer-model` / `dozer-services` / `dozer-i18n` / `dozer-panel-host` / byteui / bytehost / iced,**不含** `dozer-app` / `dozer-client` / `dozer-core`。

**Tech Stack:** Rust、iced 0.14、`dozer-panel-host`(`ActivationCtx` / `RequestTrackers` / `NoticeOutbox`)、`dozer-store-project`(`LocalProjectService` / `LocalMemoryService`)、`bytehost-chrome`、objc2(仅拖窗保护)。

**Spec:** `docs/superpowers/specs/2026-10-18-project-panel-s5a-design.md`(含 §11 修订)。模板:Dozer `docs/superpowers/specs/2026-10-15-panel-todo-lib-design.md`(S3a)。

## Global Constraints

- **库边界:** `dozer-panel-project` 的依赖闭包里不得出现 `dozer-app`、`dozer-client`、`dozer-core`。
- **只用 git tag 依赖**,不提交本地 `[patch]`、不提交带本地路径的依赖(Dozer 仓库内部的 `path = "../..."` 除外,与 `dozer-panel-todo` 相同)。
- **Dozer 用户可见行为不变**;被搬走的测试随代码一起走,**不删、不改期望值**;抽库前后 Rust 测试总数相同。
- **打 tag、推送是对外动作,每次执行前向用户确认**。Digger 没有 remote,不推送、不打 tag。
- **Dozer 与 Digger 同日升级**(ADR):Digger 在同一个提交里升 bytehost、dozer libs 两组依赖。
- **主工作区别人未提交的改动不动**:Dozer 主工作区(`main`)当前有未提交的 `Cargo.lock`、`dozer-app/Cargo.toml`、`Info.plist` 改动,Digger 主工作区也可能有;所有实现在各自独立 worktree 里做。
- 不引入新的日志依赖;日志沿用各自现有方式。
- **Digger 打开项目时不做静默脚手架**(spec §11.4);只有用户点面板里的"修复项目"才执行 `scaffold_steps()`。
- **Digger 里没有 `TranscriptService`**:依赖它的动作(转录补录、总结批次、删除分级里的转录那一档)不显示;`Services.transcript` 在 Digger 里是 `None` 语义,不是假实现。
- Digger 的 Project 面板是**单栏**(`shell::rules("digger.project")` 为 `None`),配对与预览留给 S5b。

## Review Focus

1. **没有已开项目**(启动后、关掉最后一个页签后):面板显示空状态(`view` 里 `project == None` 的分支),不 panic,布局用全局那份。→ Task 7。
2. **项目目录已被移走/删除**:读描述得 `None`、写描述失败时给错误提示而不是 panic,记忆与改名仍可用。→ Task 2。
3. **改名为空串、纯空白、与已有项目同名、超长**:行为与 Dozer 一致(沿用 `submit_name_edit` 的校验),不会把注册表写坏。→ Task 3。
4. **切换项目时还有在途请求**:旧项目的 `AsyncResult` 被请求代数丢弃,不会把别的项目的记忆列表刷进当前面板。→ Task 3。
5. **移除当前激活的项目**:页签关闭、面板回到空状态或下一个项目;注册表删除但记忆行保留(ADR-014);该项目的布局文件不再被读取。→ Task 8。

---

## Part A — Dozer(仓库 `../dozer`,worktree `../dozer-panel-project`,分支 `feat/panel-project-lib`,从 `main` 切出)

### Task 1: 基线与空壳 crate,搬纯逻辑

**Files:**
- Create: `crates/dozer-panel-project/Cargo.toml`、`src/lib.rs`、`src/links.rs`、`src/delete.rs`、`src/scaffold.rs`、`src/meta.rs`
- Modify: `Cargo.toml`(workspace members)、`crates/dozer-app/Cargo.toml`(加依赖)、`crates/dozer-app/src/extensions/project.rs`(改引用)、删除 `extensions/project/{links,delete,scaffold}.rs` 与 `project_meta.rs` 中已搬走的部分

**Interfaces:**
- Produces: `dozer_panel_project::links::{LinksState, LinkTarget}`(原样搬)、`delete`(分级纯逻辑,原样搬)、`scaffold::{scaffold_steps, run_sync_steps, ScaffoldStep, ScaffoldStepResult}`、`meta::{load_description(&Path) -> Option<String>, write_description(&Path, &str) -> std::io::Result<()>}`、`BlankInfo`(`compute_blank_info` 的返回类型,替代 `crate::preview::BlankPaneInfo`,字段与原结构一一对应)、`dir_size_excluding`、`DISK_USAGE_EXCLUDE`。

- [ ] **Step 1: 记录基线。** 在 worktree 里 `cargo test -p dozer-app 2>&1 | grep "^test result"`,把总通过数写进计划执行记录(后面每个 Dozer 任务都与它比对)。
- [ ] **Step 2: 建 crate。** `Cargo.toml` 照抄 `dozer-panel-todo` 的依赖写法(同样的 byteui / bytehost tag),只留 `dozer-model`、`dozer-services`、`dozer-i18n`、`dozer-panel-host`、byteui、bytehost-core、bytehost-desktop、serde、serde_json、tokio、iced_widget、iced_renderer;dev-dependencies 加 `tempfile`。`cargo build -p dozer-panel-project` 通过。
- [ ] **Step 3: 搬 `links`、`delete`、`scaffold`、`meta`、`dir_size_excluding`、`compute_blank_info`(连同它们的单元测试)。** 搬时只改 `use` 路径;`crate::project_meta::*` 变成 `crate::meta::*`;`BlankPaneInfo` 在库里定义为 `BlankInfo`。
- [ ] **Step 4: `dozer-app` 改引用。** `extensions/project.rs` 里 `pub use dozer_panel_project::{...}` 这几项;`project_meta.rs` 里 `load_description` / `write_description` 改为转调库(保留原路径,免得改所有调用点)。
- [ ] **Step 5: 验证。** `cargo test -p dozer-panel-project` 全绿;`cargo test -p dozer-app` 总通过数 = 基线数 − 搬走的测试数 + 这些测试在新库里的数(两库之和等于基线)。
- [ ] **Step 6: 提交** `refactor(project): 新建 dozer-panel-project,搬纯逻辑`。

### Task 2: 本地化与描述文件的错误路径

**Files:**
- Create: `crates/dozer-panel-project/src/i18n.rs`、`locales/{en,zh-CN}.json`(从 `crates/dozer-app/locales/panels/project/` 搬)
- Modify: `crates/dozer-app/src/i18n.rs`(owner `dozer.project` 的 `include_str!` 改指向新库的 `locales/`,仿第 30 行 `dozer.todo` 的写法)
- Test: `crates/dozer-panel-project/src/meta.rs`

**Interfaces:**
- Produces: `dozer_panel_project::PROJECT_OWNER: &str = "dozer.project"`;`i18n::{t, t_args, keyed, keyed_args}`,签名与原 `extensions/project.rs` 里同名 `pub(crate)` 函数一致,解析走 `dozer-i18n`(同 `dozer-panel-todo::i18n`)。

- [ ] **Step 1: 写失败测试**(`meta.rs`):`load_description_returns_none_when_dir_is_missing`(对不存在的目录 → `None`)、`write_description_fails_cleanly_when_parent_is_a_file`(把 `.dozer` 建成同名文件,`write_description` 返回 `Err`,不 panic)、`load_description_returns_none_for_non_utf8`(写入非 UTF-8 字节 → `None`)。
- [ ] **Step 2: 运行确认失败/通过状态。** `cargo test -p dozer-panel-project meta::`。若 `load_description` 已有行为满足,第三条直接通过则保留为回归测试。
- [ ] **Step 3: 搬 `i18n` 与 `locales/`,改 `dozer-app/src/i18n.rs` 的 owner 注册。** 搬完 `scripts/i18n/check_migration.py` 的"独立面板库"清单里加入 `crates/dozer-panel-project/locales/` → `dozer.project`;`build_inventory.py`、`scan_sources.py` 的路径表里按 `dozer-panel-todo` 的条目各加一行。
- [ ] **Step 4: 验证。** `bash scripts/check-i18n.sh` 通过;`cargo test -p dozer-panel-project -p dozer-app` 总数守恒。
- [ ] **Step 5: 提交。**

### Task 3: 状态与更新(含请求追踪、发件箱、激活)

**Files:**
- Create: `crates/dozer-panel-project/src/state.rs`、`src/update.rs`
- Modify: `crates/dozer-app/src/extensions/project.rs`(缩成重新导出)、`project/update.rs`(删)、`app/update.rs`、`app/app.rs`(5 个调用点改用库的类型)
- Test: `crates/dozer-panel-project/src/update.rs`(原 `project/update.rs` 与 `project.rs` 里的测试随代码搬)

**Interfaces:**
- Produces(签名沿用 Dozer 现状,只是换了归属与发件箱类型):
  - `pub struct WorkspaceState`(字段与原一致;`outbox` 改为 `dozer_panel_host::NoticeOutbox`;`take_outbox(&mut self) -> Vec<dozer_panel_host::Notice>`)。
  - `pub enum Message`(原样搬,含 `AsyncResult`)。
  - `pub fn update(ws_state: &mut WorkspaceState, msg: Message, project_id: i64, current_name: &str, repo_path: &Path, services: &Services, handle: &tokio::runtime::Handle, emit: impl Fn(Message) + Send + 'static)`。
  - `pub fn on_activate(state: &WorkspaceState, ctx: &dozer_panel_host::ActivationCtx<Message>)`、`pub fn request_memories_refresh(...)`、`pub fn spawn_scaffold_run(...)`、`pub fn spawn_repair_run(...)`、`pub fn submit_name_edit(...)`、`pub fn spawn_delete_project(...)`,均沿用现有形参。
  - **转录相关入参改为 `Option<Arc<dyn TranscriptService>>`**(`spawn_scaffold_run`、`spawn_repair_run`、`spawn_delete_project`):`None` 时跳过补录/总结批次/转录删除这几步,其余步骤照常。Dozer 传 `Some`,行为不变。
- Consumes: Task 1 的 `links`、`scaffold`、`meta`;Task 2 的 `i18n`。

- [ ] **Step 1: 写失败测试**(随代码搬来的测试保持原样,另加三条):`rename_to_blank_name_is_rejected_without_touching_the_registry`(`FakeProject` 记录调用次数为 0)、`stale_async_result_from_previous_project_is_dropped`(先以 `project_id=1` 发起 `FetchMemories`,再切到 `project_id=2`,把旧 token 的 `AsyncResult` 送进 `update`,断言 `memories()` 不变)、`scaffold_and_repair_without_transcript_service_skip_backfill_and_still_finish`(传 `None`,断言 `ScaffoldDone` 发出且 `FakeProject` 的 `backfill_*`、`submit_summary_batch` 调用数为 0)。
- [ ] **Step 2: 运行确认失败。** `cargo test -p dozer-panel-project` → 新增三条因 API 不存在而编译失败。
- [ ] **Step 3: 搬 `state` / `update`,换 `dozer-panel-host` 的 `RequestTrackers` / `NoticeOutbox` / `ActivationCtx`,转录入参改 `Option`。** `service_fakes::{FakeMemory, FakeProject, batch}` 里 Project 用到的部分搬进库的 `testing` 模块(`#[cfg(any(test, feature = "testing"))]`,feature 名与 `dozer-panel-todo` 的保持一致;若它没有 feature,就只在库内部测试用,`dozer-app` 保留自己的 fakes)。
- [ ] **Step 4: Dozer 调用点改用库类型。** `app/update.rs` 3 处 `project::update`、1 处 `spawn_scaffold_run`、1 处 `spawn_delete_project`,`app/app.rs` 的 `submit_name_edit`;转录参数传 `Some(services.transcript.clone())`。
- [ ] **Step 5: 验证。** `cargo test -p dozer-panel-project -p dozer-app` 总数守恒(两库之和 = 基线 + 3 条新增)。
- [ ] **Step 6: 提交。**

### Task 4: 视图与展示适配

**Files:**
- Create: `crates/dozer-panel-project/src/view.rs`、`src/adapter.rs`、`crates/dozer-app/src/host_adapters/project_panel.rs`
- Modify: `crates/dozer-app/src/host_adapters/mod.rs`、`extensions/project.rs`、`app/view.rs:707` 一带(`project::view` 调用点)、`scripts/audit/check_panel_boundary.py`(登记新库)
- Test: `crates/dozer-panel-project/src/view.rs`

**Interfaces:**
- Produces:
  - `adapter::PanelDisplay`:沿用 `dozer-panel-todo::adapter::PanelDisplay` 的形状,**只保留 Project 视图实际用到的方法**(`hover_progress`、`panel_head`、`header_padding`、`tab_divider`、`locale`;不要 `list_collapse_button`——单栏不折叠)。
  - `adapter::TextInputTarget { pub id: iced_widget::core::widget::Id, pub secure: bool }`(与 todo 库同形)。
  - `view::view<'a>(ws_state: &'a WorkspaceState, project: Option<&'a ProjectInfo>, width: Length, outer: Border, hover: ProjectPaneHover, locale: &'a str) -> Element<'a, Message, iced_widget::Theme, iced_renderer::Renderer>`,签名与原一致。
  - 文本框右键菜单:库发 `Message::TextInputMenuRequest(adapter::TextInputTarget)`,产品转成自己的菜单请求。
- Consumes: Task 3 的 `WorkspaceState`、`Message`。

- [ ] **Step 1: 写失败测试**(`view.rs`):`view_without_project_renders_empty_container`(`project=None` 不 panic,可用 `iced_test` 或构造 element 后断言无 panic,沿用 `dozer-panel-todo` 视图测试的方法)、`footer_min_width_matches_two_buttons_plus_gaps`(原 `footer_min_width()` 的公式测试,若原来无测试就新增:`>= 2 * 单按钮宽`)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬 `view`,去掉对 `crate::app::TextInputTarget`、`workspace::relative_time_text`、`chrome::homespace`、`ui_text::text` 的依赖。** 相对时间文案若 byteui 已有就用 byteui 的,没有则搬进库的 `view.rs`;`homespace` 外观取 byteui 里对应的公共函数或把用到的两三个度量搬进库;`ui_text::text` 换成 iced 原生 `text`(若原函数只是加字体/着色,保持同样的样式参数)。
- [ ] **Step 4: Dozer 侧在 `host_adapters/project_panel.rs` 为 `App` 实现 `PanelDisplay`**(转调现有 `PanelHost` 与 chrome 函数,行为逐项相同);`TextInputTarget` 映射到 `crate::app::TextInputTarget`。`app/view.rs` 的调用点改用库的 `view`。
- [ ] **Step 5: 验证。** `cargo test -p dozer-panel-project -p dozer-app` 总数守恒(+2);`python3 scripts/audit/check_panel_boundary.py` 通过;`cargo tree -p dozer-panel-project | grep -E "dozer-(app|client|core)"` **无输出**。
- [ ] **Step 6: 提交。**

### Task 5: 清理、目测验收、发 tag

- [ ] **Step 1: 清理。** 确认 `crates/dozer-app/src/extensions/project/` 目录里只剩重新导出或已空(`project_create.rs` 不动),无残留重复实现:`grep -rn "fn scaffold_steps\|fn load_description\|fn dir_size_excluding" crates` 只命中 `dozer-panel-project`(和 `project_meta.rs` 的转调)。
- [ ] **Step 2: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;总数与基线一致(加新增 5 条)。
- [ ] **Step 3: 请用户在 Dozer 上目测验收**(Project 面板外观与交互与抽库前一致:改名、记忆增删、链接、描述编辑、修复项目、删除分级)。**未验收不继续。**
- [ ] **Step 4: 合并到 Dozer `main`、切 `libs-2026-10-<日期>x` tag、推送——向用户确认后执行。** 该 tag 里 bytehost / byteui 的版本必须与 Digger 当时用的一致(逐个核对各 crate 的 `Cargo.toml`)。

---

## Part B — bytehost(仓库 `../bytehost`,独立 worktree)

### Task 6: `bytehost-chrome` 顶栏拖窗保护

**Files:**
- Modify: `crates/bytehost-chrome/src/platform_macos.rs`(更新文件头注释,删掉"没有移植"那段)、`src/lib.rs`(导出)
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces: `pub fn install_topbar_drag_guard(window: &dyn HasWindowHandle)`;`pub fn set_topbar_control_hovered(hovered: bool)`(写一个进程级 `AtomicBool`,`mouseDownCanMoveWindow` 的覆写读它:悬停在可交互控件上返回 `NO`,否则 `YES`)。非 macOS 平台为空实现。
- 参考实现:Dozer `crates/dozer-app/src/platform/window.rs:235-309`(`TOPBAR_CONTROL_HOVERED` + `class_addMethod`);区别只是窗口句柄来自 `HasWindowHandle` 而不是 winit 窗口,与同文件 `install_fullscreen_guard` 一致。

- [ ] **Step 1: 写失败测试:** `hovered_flag_round_trips`(`set_topbar_control_hovered(true)` 后读取为 `true`,再置 `false`)——把原子量读取封装成 `pub(crate) fn topbar_control_hovered() -> bool` 以便测试。真实拖窗行为只能真机验,不写假测试。
- [ ] **Step 2: 运行确认失败。** `cargo test -p bytehost-chrome platform_macos`。
- [ ] **Step 3: 实现两个函数与 `class_addMethod` 覆写;`class_addMethod` 返回 `false`(selector 已存在)时记一条日志,不 panic。**
- [ ] **Step 4: 验证。** `cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --check`、bytehost 自己的共享依赖门禁通过。
- [ ] **Step 5: 提交;切 `v0.4.10`(或当时的下一个 patch)tag 并推送——向用户确认后执行。**

---

## Part C — Digger(worktree `../digger-s5a`,分支 `feat/project-panel`,从 `main` 切出)

### Task 7: 升级依赖并接入 Project 面板与左栏图标

**Files:**
- Modify: `Cargo.toml`(bytehost → Task 6 的 tag,dozer 四个库 → Task 5 的 tag,新增 `dozer-panel-project`)、`Cargo.lock`、`crates/desktop/Cargo.toml`
- Create: `crates/desktop/src/adapters/project.rs`
- Modify: `crates/desktop/src/shell.rs`(`project_id()`、`all_panels()`、`meta()`、`defaults()`)、`crates/desktop/src/product.rs`(目录登记、`project_view`、面板切入时的 `on_activate`)、`crates/desktop/src/adapters/mod.rs`
- Test: `crates/desktop/src/shell.rs`、`crates/desktop/src/adapters/project.rs`

**Interfaces:**
- Produces:
  - `shell::project_id() -> PanelId`(`"digger.project"`);`shell::meta` 里 `"digger.project" => ("项目", IconKind::<与 Dozer Project 面板同一个图标>)`(取 Dozer `PanelKind::Project` 用的图标);`shell::defaults().rails[0] = [project_id(), markdown_id(), info_id()]`,`selected[0]` 仍为 `markdown_id()`。
  - `adapters::project::ProjectPanel`:`new(memory: Arc<dyn MemoryService>, project: Arc<dyn ProjectService>) -> Self`;`view(&self, project: Option<&ProjectInfo>, locale: &str) -> Element<'_, Msg>`;`update(&mut self, msg: dozer_panel_project::Message, project: Option<&ProjectInfo>) -> Task<Msg>`;`on_activate(&mut self, project: Option<&ProjectInfo>)`;`take_notices(&mut self) -> Vec<dozer_panel_host::Notice>`(排空后由 `projects` 的提示条显示)。
  - `ProductShell` 的 `Msg::Project(dozer_panel_project::Message)`。
- Consumes: `ProjectsState::current_project() -> Option<&ProjectInfo>`、`current_project_id()`;`ProjectHost` 里的 `LocalProjectService` / `LocalMemoryService`;Task 5/6 的 tag。

- [ ] **Step 1: 升级依赖并过依赖门禁。** 改 `Cargo.toml` 与 `Cargo.lock`(`cargo update -p` 对应包),`python3 -c` 的 `Cargo.lock` 重复检查输出 `ok: one version each`,`bash scripts/check-shared-deps.sh` 与 `bash scripts/check-host-deps.sh` 通过。**这一步单独成一个提交**(依赖只升一次、不夹带代码)。
- [ ] **Step 2: 写失败测试**(`shell.rs`):`project_is_first_on_the_left_rail_by_default`(`defaults().rails[0][0] == project_id()`)、`legacy_layout_without_project_gets_it_prepended_to_the_left_rail`(用不含 `digger.project` 的旧 `shell.json` 字符串调 `load`,断言 `model.rail(Side::Left)` 以 `project_id()` 开头、其余面板相对顺序不变)、`project_panel_is_single_pane`(`rules(&project_id()) == None`)。
- [ ] **Step 3: 运行确认失败。** `cargo test -p digger-desktop shell::`。
- [ ] **Step 4: 实现。** `shell.rs` 加 `project_id`、更新 `all_panels`/`meta`/`defaults`;旧布局的处理:`persist::load` 对"清单里有而文件里没有的面板"回默认栏(S1 既有行为),在 `load` 之后若 `project_id()` 不在左栏第一位且来源是旧文件,则 `ShellMsg` 移到最前(用现有的移动消息,不新造)。
- [ ] **Step 5: 写适配层测试**(`adapters/project.rs`):`without_a_project_the_view_is_the_empty_state_and_nothing_is_requested`(`on_activate(None)` 不发请求)、`remove_project_closes_nothing_by_itself`(面板只发 `Message`,关页签由 `projects` 处理,见 Task 8)。
- [ ] **Step 6: 实现 `ProjectPanel` 与 `product.rs` 接线。** 面板按 `usage_view` 的方式放进 `body`;切入面板(`on_visible`)与切换项目页签时调 `on_activate`;后台结果回流沿用 `UsagePanel` 的 `tx/rx` 做法;`catalog` 里登记 `project_id()`(`title` "项目"、`surfaces: vec![]`)。**不要**在打开项目时调 `spawn_scaffold_run`。
- [ ] **Step 7: 验证。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`。
- [ ] **Step 8: 提交。**

### Task 8: 面板里的管理动作接通

**Files:**
- Modify: `crates/desktop/src/projects/mod.rs`、`crates/desktop/src/adapters/project.rs`、`crates/desktop/src/product.rs`
- Test: `crates/desktop/src/projects/tests.rs`

**Interfaces:**
- Consumes: `dozer_panel_project::Message` 里的改名/移除/记忆/链接/描述消息;`ProjectsState::update`、`ProjectTabs::close_tab(id)` / `prune`。
- Produces: `projects::Message::Removed(i64)`(面板移除成功后由产品投递;`ProjectsState` 处理:关该页签、刷新注册表、清该项目的布局引用——见 Task 9)。

- [ ] **Step 1: 写失败测试**(`projects/tests.rs`):`removing_the_active_project_closes_its_tab_and_keeps_memory_rows`(用 `LocalProjectService` 建两个项目并各写一条记忆,移除激活的那个后:`tabs.open()` 不含它、`active()` 落到剩下的、`list_memories(removed_id)` 仍能读到那条行)、`renaming_updates_the_tab_title_without_reordering_tabs`、`a_removed_project_cannot_be_reopened_with_the_same_id`(再打开同一目录得到新 id,ADR-014 "id 不复用")。
- [ ] **Step 2: 运行确认失败/通过状态,** 已被 `LocalProjectService` 满足的行为直接通过并保留为回归测试。
- [ ] **Step 3: 实现 `Message::Removed` 与产品里的转投。** 面板移除按钮的二次确认沿用库里的既有交互,不新造。
- [ ] **Step 4: 验证与提交。**

### Task 9: 布局按项目一份

**Files:**
- Modify: `crates/desktop/src/shell.rs`(`load`/保存按 scope)、`crates/desktop/src/product.rs`(切项目页签时换布局、保存时落到当前 scope;项目布局路径 `<shell.json 所在目录>/shell-projects/<id>.json`,生产即 `<config>/digger/shell-projects/<id>.json`)
- Test: `crates/desktop/src/shell.rs`

**Interfaces:**
- Produces: `shell::load_scoped(global: &Path, project: Option<&Path>) -> Loaded`(有项目文件读项目文件,否则读全局;全局都没有用 `defaults()`);保存:有当前项目 → 写项目文件,窗口大小仍写全局 `shell.json`;无当前项目 → 写全局。`Loaded::may_save` 的"更高 schema 版本不覆盖"保护对两类文件各自生效。

- [x] **Step 1: 写失败测试:** `with_no_project_the_global_layout_is_used`、`first_save_for_a_project_writes_its_own_file_and_leaves_the_global_one_untouched`、`switching_between_projects_swaps_the_layout`(A 把左宽改成 500、B 保持默认,切回 A 得到 500,切到 B 得到默认)、`newer_schema_project_file_is_never_overwritten`、`legacy_global_layout_seeds_a_project_without_losing_it`(项目文件不存在时用全局布局作初值,保存后全局文件字节不变)、`removed_projects_layout_file_is_ignored`(项目已不在注册表里,读它得到全局)。
- [x] **Step 2: 运行确认失败。**
- [x] **Step 3: 实现。** 迁移触发时机取"首次保存时",启动不批量迁移(留给实现者的决定里已定:最少写盘)。
- [x] **Step 4: 验证与提交。** 全量 `cargo test --workspace`。

### Task 10: 顶栏拖窗保护接入与真机验证

**Files:**
- Modify: `crates/desktop/src/product.rs`(窗口创建后调 `install_topbar_drag_guard`)、`crates/desktop/src/projects/mod.rs`(`tabs_view` 里页签、"＋"、关闭按钮的 `on_enter` / `on_exit` 驱动 `set_topbar_control_hovered`)
- Test: `crates/desktop/src/projects/tests.rs`

**Interfaces:**
- Consumes: Task 6 的 `bytehost_chrome::platform_macos::{install_topbar_drag_guard, set_topbar_control_hovered}`。

- [ ] **Step 1: 写失败测试:** `hovering_a_tab_marks_a_topbar_control_hovered_and_leaving_clears_it`(对 `ProjectsState::update` 送 `HoverEnter(tab)` / `HoverLeave`,读 `topbar_control_hovered()`)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 实现。** 页签/按钮加悬停消息,`update` 里调 `set_topbar_control_hovered`。
- [ ] **Step 4: 自动化验证。** `cargo test --workspace` 通过。
- [ ] **Step 5: 真机验证(只能截图,点击要人来做;用隔离数据目录 `DIGGER_DATA_DIR`)。** 打开两个项目:按住页签拖动**不**拖走窗口、点页签能切换;点"＋"能弹出文件夹选择器;关闭按钮可点;在顶栏空白处按住拖动**能**拖窗。逐项记录;任一项不符先停下,不要为了"过"而改验证。

### Task 11: 全量验证、文档与收尾

- [ ] **Step 1:** `cargo build --workspace && cargo test --workspace`(记录通过数)、`cargo clippy --workspace --all-targets`(零告警)、`cargo fmt --check`、`bash scripts/check-shared-deps.sh && bash scripts/check-host-deps.sh && bash scripts/check-host-deps.sh --self-test && bash scripts/check-shell-regions.sh`、`Cargo.lock` 重复检查。
- [ ] **Step 2: 真机回归(用户配合点击)。** 左栏第一个图标是 Project 且面板显示;改名、写/删记忆、编辑描述、"修复项目"(四步脚手架只在点击后发生)、移除项目都可用;两个项目的布局互不影响;从旧的全局 `shell.json` 启动布局不丢;转录相关动作不显示。截图留证,逐项写进报告。
- [ ] **Step 3: 文档。** `docs/superpowers/specs/2026-10-14-window-shell-design.md` 的 S5 行改为"S5a 已完成、S5b files 待做";`2026-10-18-project-panel-s5a-design.md` 状态改"已实现",补"实现结果"一节;更新 `docs/adr/ADR-014` 的 Consequences 里"Project 面板界面"一句。
- [ ] **Step 4: 合并到 `main`(快进)、清理 worktree 与分支,** 不碰主工作区未提交的改动。
- [ ] **Step 5: 发布新版本(项目硬性要求:每个面板迁移完成后,Dozer 与 Digger 各重新发布一个版本)。** Project 面板迁移在两个产品里都落地、真机回归通过后:① Dozer 走它自己的发布流程(`scripts/build-macos-app.sh release`:升版本号 → 门禁 → 打包);② Digger 走 `bash scripts/release.sh`(门禁 → 升版本 → 打包 → 签名校验 → 压缩;不提交、不打 tag、不推送)。两个产物都要装到本机启动一次确认版本号与面板可用。**发布(含升版本、提交版本号、安装)是对外动作,先向用户确认再执行**;报告里写明两个新版本号与产物路径。
- [ ] **Step 6: 汇报:** 各仓库测试通过数、依赖图结论、真机截图结论、D1–D7(并附 Step 5 的两个新版本号) 的最终落地情况、哪些没验。

---

## Self-Review

- **Spec 覆盖:** G1 → Task 1–5;G2 → Task 7;G3 → Task 9;G4 → Task 6、10;G5 → Task 8;§6.4 转录动作不显示 → Task 3(`Option<Arc<dyn TranscriptService>>`)+ Task 7;D5/D6 → Task 7、9;§11.1 单栏 → Task 7;§11.2 panel-host → Task 3;§11.3 → Task 6、10;§11.4 无静默脚手架 → Global Constraints + Task 7 Step 6 + Task 11 Step 2;§11.5 门禁登记 → Task 2、4。
- **类型一致:** `WorkspaceState` / `Message` / `update` 的签名在 Task 3 定义,Task 4、7、8 引用同名;`PanelDisplay` 在 Task 4 定义,Dozer 侧 `host_adapters/project_panel.rs` 实现;`set_topbar_control_hovered` / `install_topbar_drag_guard` 在 Task 6 定义,Task 10 引用。`shell::load_scoped` 只在 Task 9 出现。
- **风险点:** Task 3 里转录入参改 `Option` 会触及 Dozer 的 5 个调用点,必须保持 Dozer 行为逐项不变(传 `Some`);Task 5 Step 3 的目测验收是 Part C 的闸门;tag 与推送都是对外动作。Task 4 Step 3 里"`ui_text::text` / `homespace` 的替换"是读代码时最不确定的一处,实现者读到实际函数后若发现替换会改变外观,先停下报告,不要改外观。
