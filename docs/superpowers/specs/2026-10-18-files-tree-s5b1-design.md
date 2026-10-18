# S5b-1 文件树:从 Dozer 抽成共享库,Digger 接入

- 日期:2026-10-18
- 状态:设计稿,待用户审阅
- 路径:架构级(Dozer 新增共享 crate;Digger 新增一个面板与左栏图标;`dozer-panel-host` 增加一个跨面板命令)
- 相关:`2026-10-18-project-panel-s5a-design.md`(S5a,抽库模板与 Dozer 侧门禁登记方式)、`2026-10-14-window-shell-design.md`(S1,S5 一行)、Dozer `2026-10-15-panel-todo-lib-design.md`(S3a)

## 1. 用户意图

Digger 要重用 Dozer 的 project、files、agent、群聊、会话等面板,Dozer 里改了面板 Digger 要同步。原计划 S5 是"Project 面板 + files"。用户在 2026-10-18 的决定:

1. S5 拆成 S5a(Project,已有设计与计划,由其他 agent 实施)和 S5b(files)。
2. S5b 再拆:**S5b-1 文件树**(本文),**S5b-2 预览引擎**(后做,单独设计)。原因:Dozer 的 Files 与 Project 两个面板的右半边是**同一套预览引擎的两个实例**(`preview/` 约 1.8 万行),它是共同底座,而且与 Digger 自己的 Markdown 编辑器面板如何分工需要单独设计。
3. S5b-1 的范围:文件树加"不依赖新宿主能力"的右键动作;文件历史、回滚、搜索弹窗、预览页签菜单、与预览相关的拖拽**暂缓**。
4. Digger 里点文件:最初的设想是 Markdown 文件交给现有的 Markdown 编辑器面板、其余用系统默认应用打开。**写设计时核实发现 Digger 的 `MarkdownPanel` 没有任何"打开指定文件"的入口**(只有 `new()` 载入内置示例文档,没有路径相关的方法),所以本轮**所有文件一律用系统默认应用打开**;"在 Markdown 面板打开文件"记为后续事项,见 §9 Q1。

## 2. 目标

- G1 Dozer 里新建共享库 `dozer-panel-files`,把 `extensions/files/**`(约 7500 行:树、状态、更新、视图、git 着色、树内拖拽)搬进去;Dozer 改为依赖它,**用户可见行为不变**。
- G2 Digger 依赖同一个库,在窗口骨架里挂出文件树面板 `digger.files`,放在左栏 Project 之后。
- G3 Digger 里可用的功能:展开/折叠/选中/双击、git 状态着色与文件变化监听、搜索框与"显示隐藏文件"开关、新建文件/文件夹、重命名、删除(带确认)、复制/粘贴、树内拖拽移动、从 Finder 拖入文件、复制路径、用系统默认应用/按扩展名配置的外部软件打开、分支选择与 `git init`。
- G4 点文件的行为经一个新的跨面板命令交给产品(见 §6.2),Digger 一侧本轮是系统默认应用打开。

## 3. 非目标

- 不做预览面板与预览引擎(S5b-2)。文件树与预览之间的拖拽随它一起到。
- 不做:查看文件历史、回滚到上一版本、从树发起搜索弹窗、预览页签右键菜单、"发送到 Agent 上下文/终端"。这些在 Digger 里**不显示**(见 §6.3)。
- 不做 macOS 原生 `NSMenu`:Digger 的右键菜单用 byteui 已有的 `interaction::context_menu` 渲染(iced 弹层)。
- 不改 `bytegit`、不改任何服务 trait 签名。
- 不升级任何依赖。

## 4. 反目标

- Dozer 文件树的外观或行为有任何变化。
- 库里出现 `dozer-app`、`dozer-client`、`dozer-core` 的依赖,或 Digger 里复制一份文件树实现。
- Digger 里出现"点了没反应"的菜单项:不支持的动作要么不显示,要么明确提示,不能静默无效。
- 文件操作(重命名、删除、移动、粘贴)越出当前项目目录。

## 5. 约束

- 库边界沿用 S5a / S3a:依赖闭包只含 `dozer-model`、`dozer-services`、`dozer-i18n`、`dozer-panel-host`、byteui、bytehost-core、bytehost-desktop、`bytegit`、iced、`serde*`、`tokio`。
- 只用 git tag 依赖;打 tag、推送是对外动作,执行前向用户确认;Digger 没有 remote。
- Dozer 与 Digger 同日升级依赖(ADR)。
- 被搬走的测试随代码一起走,不删、不改期望值;抽库前后 Rust 测试总数守恒。
- 实现在独立 worktree 里做,不动主工作区别人未提交的改动。
- **依赖 S5a 已合并**:Digger 一侧要用 S5a 的"按项目一份"布局、`ProjectsState::current_project()` 和左栏 Project 图标。

## 6. 设计

### 6.1 库 `dozer-panel-files`

与 `dozer-panel-project`、`dozer-panel-todo` 并列。内部模块沿用现有划分:`state`、`update`、`view`、`tree`、`git_status`、`i18n`(+ `locales/`,owner `dozer.files`)、`adapter`。

文件树对 `dozer-app` 内核的现有依赖与去向:

| 现有依赖 | 去向 |
|---|---|
| `external_apps`(扩展名 → 外部 App 的配置表,82 处引用) | 配置表的类型与 `load_from` / `save_to` 搬进库;读写哪个文件由产品注入(Dozer 仍是 `config_dir()/external_apps.json`) |
| `project::ProjectFsError` 及项目内文件操作(粘贴、移动) | 搬进库,保持"不越出项目目录"的检查 |
| `menu_spec::MenuSpec` | 库自己产出菜单项列表(`FilesMenuItem`),产品用各自的后端渲染:Dozer 转 `MenuSpec`(原生/iced 两条路),Digger 转 byteui `context_menu` |
| `chrome`、`theme`、`app::TextInputTarget` | `PanelDisplay` 适配(同 S5a),取 byteui 已有的 |
| `panel_host::*` | 用 `dozer-panel-host` 的请求追踪、`NoticeOutbox`、`HostOutbox`、`PanelCommand` |
| `preview::*`(10 处) | 见 §6.2,树只发命令,不再直接碰预览 |
| `workspace::*`、`extensions::*` | 逐处替换为 `PanelCommand` 或适配接缝,清单由实现者在搬代码时列出 |

### 6.2 点文件:新增跨面板命令 `PanelCommand::OpenFile`

在 `dozer-panel-host` 的 `PanelCommand` 里加 `OpenFile { path: PathBuf }`。树里双击/回车文件时库发出它,不再直接开预览。
- **Dozer** 的 `run_panel_command` 把它映射到现有的预览打开逻辑,行为不变。
- **Digger** 的映射:一律走 `open <path>`(系统默认应用)。等 Markdown 面板有了"打开指定文件"的入口之后,再把 `md` / `markdown` 改映射到它(那是 Markdown 面板自己的改动,不在本轮)。

### 6.3 能力开关:不支持的动作不显示

库的 `adapter` 里定义 `FilesCapabilities { file_history: bool, rollback: bool, search: bool, send_to_agent: bool, preview_tabs: bool }`,由产品在构造面板时注入。菜单生成函数按它过滤。Dozer 全开;Digger 全关。
`PanelCommand::SearchIn`、`ShowFileHistory` 保持不变,只是 Digger 的产品层不会收到它们。

### 6.4 Digger 一侧

- 面板 id `digger.files`;左栏默认顺序 `[project, files, markdown, info]`(我的决定 D1)。
- 文件树的根目录 = 当前项目的路径;无已开项目时显示空状态(与 Dozer 一致)。切换项目页签时重建树、重启监听。
- 文件变化监听与 git 着色走 `bytegit`(Digger 的依赖图里已经有它)。
- 右键菜单与删除确认、移动对话框用 byteui 的组件在面板内/弹层里呈现。
- 面板单栏(`shell::rules` 为 `None`),预览的配对留给 S5b-2。

## 7. 步骤(每一步 Dozer 都能编译且测试全绿)

1. 基线;建空壳 crate;搬纯逻辑(`git_status`、`tree` 的纯函数、`external_apps` 配置表、项目内文件操作)。
2. 搬 `i18n` / `locales/`;登记 Dozer 门禁(`scripts/i18n/*.py`、`scripts/audit/check_panel_boundary.py`、`dozer-app/src/i18n.rs`)。
3. 加 `PanelCommand::OpenFile`;把树对预览的直接调用换成命令;加 `FilesCapabilities` 与菜单项过滤。
4. 搬状态、更新、视图;换用 `dozer-panel-host` 的请求追踪与发件箱。
5. Dozer 侧适配层(`host_adapters/files_panel.rs`);清理;目测验收;发 tag。
6. Digger:升级依赖;`digger.files` 面板与左栏图标;树根跟随当前项目。
7. Digger:点文件的映射(Markdown / 系统打开);右键菜单与对话框的呈现。
8. Digger:文件监听与 git 着色;真机验证与文档。

## 8. 验证

- 测试数量守恒(Dozer);依赖闭包里没有 `dozer-app` / `dozer-client` / `dozer-core`;`Cargo.lock` 每个共享库一份;`check-shared-deps.sh`、`check-host-deps.sh` 通过。
- Dozer 目测(用户验收):文件树外观与交互与抽库前一致。
- Digger 真机(只能截图,点击要人来做):树根是当前项目;展开、新建、重命名、删除确认、移动、复制粘贴、从 Finder 拖入文件;双击任何文件都在系统默认应用打开;外部在 Finder 里增删文件后树自动更新、git 着色随之变化;不支持的菜单项确实不出现;切换项目时树随之切换。
- Digger 自动化:`cargo test --workspace`、`clippy`、`fmt --check` 通过。

## 9. 我的决定与未解问题

我的决定(可以推翻):

- D1 左栏顺序 `[project, files, markdown, info]`。
- D2 新增 `PanelCommand::OpenFile`(而不是让树直接依赖一个"打开器"trait),与已有的 `SearchIn` / `ShowFileHistory` 同一个机制。
- D3 能力开关用一个布尔结构体 `FilesCapabilities`,由产品注入。
- D4 右键菜单在 Digger 用 byteui 的 `interaction::context_menu`,不做原生菜单。
- D5 本轮所有文件都走系统默认应用打开(Markdown 面板还没有打开文件的入口),不做内容嗅探。
- D6 分支选择与 `git init` 留在范围内(它们只依赖 `bytegit`);若实现时发现分支弹层在 Digger 里放不下,降级为只读显示当前分支,并在报告里说明。

未解:

- Q1 要不要把"Markdown 面板打开指定文件"作为单独一轮补上(让文件树里的 `.md` 在 Digger 自己的编辑器里打开)?我倾向于排在 S5b-1 之后、S5b-2 之前,因为它也会决定预览引擎与 Markdown 面板怎么分工。

## 10. 留给实现者决定

`FilesMenuItem` 的具体形状、菜单项过滤的实现方式、`external_apps` 配置路径的注入形式、树监听在项目切换时的重建细节、搬代码时对 `workspace::*` / `extensions::*` 引用的逐处替换方案。
