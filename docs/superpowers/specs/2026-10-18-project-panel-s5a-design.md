# S5a Project 面板:从 Dozer 抽成共享库,Digger 接入

- 日期:2026-10-18
- 状态:设计稿,待用户审阅
- 路径:架构级(Dozer 新增共享 crate;Digger 新增一个面板、一个左栏图标、按项目拆分布局持久化)
- 相关:`2026-10-14-window-shell-design.md`(S1,S5 一行)、`2026-10-15-project-host-design.md`(S3p)、`docs/adr/ADR-014-project-as-host-business.md`;Dozer 侧 `2026-10-15-panel-todo-lib-design.md`(S3a,抽库的模板)、`2026-10-10-project-services-design.md`(Project 面板已服务化)

## 1. 用户意图

Digger 要重用 Dozer 的 project、files、agent、群聊、会话、todo、用量面板,并且 Dozer 里改了面板,Digger 要同步。这是 2026-10-14 起既定的迁移计划,中间为了完成 bytehost 暂停了。Project 是其中的地基:其余面板都按 `project_id` 取数。

用户在 2026-10-18 的决定:

1. Digger 的 Project 面板与 Dozer **一致**,Project 管理也一致。
2. Digger 左栏第一个图标是 Project。
3. 原计划的 S5 "Project 面板 + files" **拆开**:先 S5a(本文,Project),再 S5b(files)。agent、会话、群聊面板各自单独一轮,排在 files 之后。
4. "新建/克隆项目"对话框**推迟**,不在本文范围。

## 2. 目标

- G1 在 Dozer 里新建共享库 `dozer-panel-project`,把 `extensions/project.rs` 与 `extensions/project/**`(约 4500 行:信息、共享记忆、改名、移除、删除分级、链接、脚手架修复)搬进去。Dozer 改为依赖该库,**用户可见行为不变**。
- G2 Digger 依赖该库,在窗口骨架里挂出同一个 Project 面板,左栏第一个图标是 Project。
- G3 Digger 的窗口布局持久化从"全局一份"拆成"按项目一份"(S1 把接口按 scope id 留好了,明确留给 S5)。
- G4 移植顶栏拖窗保护(Dozer 覆写 `mouseDownCanMoveWindow` 的那段 unsafe 代码),并加真实窗口验证。S1 明确说等顶栏有了项目页签再做,现在项目页签已经有了。
- G5 `remove_project` 在面板里有入口(S3p 只提供了服务方法与测试)。

## 3. 非目标

- 不做"新建/克隆项目"对话框(`extensions/project_create.rs`,约 1900 行)。它依赖 Git 账号、密钥、浮层窗口、远程仓库选择,Digger 都还没有。Digger 继续用顶栏"＋"的原生文件夹选择器打开已有目录。
- 不做 files、agent、会话、群聊面板。
- 不改任何服务 trait 的签名,不改 `dozerd` 的存储与线上协议。
- 不让 Digger 实现 `TranscriptService`(那是会话/Agent 那几轮的事,见 §6)。
- 不升级任何依赖。

## 4. 反目标(技术上做到了也算失败)

- Dozer 的 Project 面板外观或行为有任何变化。
- 库里出现 `dozer-app`、`dozer-client`、`dozer-core` 的依赖,或出现与 Dozer 重复的一份面板实现(Digger 里复制粘贴一份也算)。
- Digger 与 Dozer 因此共享运行时状态:仍然是各自进程、各自数据库文件,只共享代码(ADR-014 第 3 条)。

## 5. 约束

- **库边界沿用 S3a:** 依赖闭包只含 `dozer-model`、`dozer-services`、byteui、`bytehost-core` / `bytehost-desktop`、iced、`serde*`。
- **依赖只用 git tag**,不提交本地 `[patch]`、不提交带本地路径的依赖。Dozer 发新的 `libs-` tag,Digger 同日升级到它(ADR 要求)。
- **Digger 没有 remote,Dozer 的 tag 与推送是对外动作,需要用户批准。**
- 每一步 Dozer 都能编译且测试全绿;被搬走的测试随代码一起走,不删、不改期望值。
- 日志用各自现有方式,不引入新日志依赖。
- 不动 Digger 主工作区别人未提交的改动,实现在独立 worktree 里做。

## 6. 设计

### 6.1 库 `dozer-panel-project`

与 `dozer-panel-todo` 并列,结构沿用其 §4.1:`state`、`update`、`view`、`links`、`scaffold`、`delete`、`adapter`、`i18n`,`locales/` 随库。

面板对 `dozer-app` 内核的现有依赖,各给一个接缝(同 todo 的 §4.2):

| 现有依赖 | 去向 |
|---|---|
| `panel_host::{ActivationCtx, RequestTrackers, scope_of, project_of, RequestTokenWrapExt}` | 库自带激活上下文与请求代数管理(todo 已做过,复用同一套形状) |
| `extensions::toast` | 库的 `ProjectOutbox` / `ProjectNotice`,产品排空后转成自己的 toast |
| `crate::i18n` | 库自带 `i18n` 与 `locales/` |
| `project_meta::{load,write}_description` | 搬进库(读写 `<项目>/.dozer/description.md`) |
| `app::TextInputTarget`、`workspace::relative_time_text`、`chrome::homespace`、`preview::BlankPaneInfo` | 文本菜单目标改成库发 `Message::TextInputMenuRequest(..)` 由产品响应;时间文案与外观用 byteui 已有的,没有的搬进库;`BlankPaneInfo` 改成库自己的小结构 |
| `service_fakes::{FakeMemory, FakeProject}`(测试) | 随测试搬进库的 `testing` 模块 |
| `PanelDisplay<M>` | 同用量/todo 的同名 trait,只补 Project 确实需要的方法 |

### 6.2 Dozer 一侧

`extensions/project*` 缩成"重新导出 + 注册包装":把库的 `Message` 纳入顶层 `Message`、实现 `PanelDisplay`、在选中 Project 面板时构造库的激活上下文、处理文本菜单消息。`project_create.rs` 原样留在 `dozer-app`,不动。

### 6.3 Digger 一侧

- 新增面板 id `digger.project`,进 `all_panels()`。**新布局的左栏顺序是 `[project, markdown, info]`**。
- 服务接线:`MemoryService`、`ProjectService` 由 `dozer-store-project` 的 `LocalMemoryService` / `LocalProjectService` 提供(ADR-014 已有,`ProjectHost` 已经在用)。
- 取数的 `project_id` 来自顶栏当前页签(`ProjectTabs`,S3p 已有)。无已开项目时,面板显示与 Dozer 一致的空状态。
- **布局按项目一份(G3):** `shell.json` 拆成"全局部分(窗口大小)+ 每项目一份(rail 布局、左宽、每侧选中面板、`PaneStore`)"。迁移:旧的全局 `shell.json` 作为所有项目的初始值,首次保存时按项目写出,不丢用户布局;没有已开项目时(启动后尚未打开任何项目)使用全局那份,切到某个项目时才换成它自己的(这一条是我的决定,标为 D6);更高 schema 版本仍沿用现有的"不覆盖原文件"保护。
- **拖窗保护(G4):** 移植那段 unsafe 代码,验证项目页签、"＋"、关闭按钮可点击且不触发拖窗,空白处可拖窗。

### 6.4 Digger 里没有 `TranscriptService`

Digger 没有对话转录。面板里依赖它的动作(打开项目时的转录补录、"修复项目"里的补录与总结批次、删除分级里的转录那一档)在 Digger 里**不显示**,其余布局不变;会话/Agent 面板接入、`TranscriptService` 有了实现之后再打开。这是我的决定,不是用户说过的(见 §9)。

## 7. 步骤(每一步 Dozer 都能编译且测试全绿)

沿用 S3a §5 的节奏:

1. **基线:** 在 Dozer worktree 记录 `dozer-app`、前端的测试数量与结果,作为抽库前后对比的基准。
2. **建空壳 crate,搬纯逻辑:** `links`、`scaffold` 的纯函数、`delete` 分级、描述文件读写。Dozer 改引用新库。
3. **搬状态与更新:** 换上库自带的请求追踪、发件箱、激活上下文。
4. **加 `PanelDisplay`,搬视图与 `locales/`。**
5. **清理 `dozer-app` 里无调用者的旧路径。** Dozer 发新的 `libs-` tag(对外动作,需批准)。
6. **Digger:** 升级到新 tag;加 `digger.project` 面板与左栏第一个图标;接 `MemoryService` / `ProjectService`。
7. **Digger:** 布局按项目拆分与迁移。
8. **Digger:** 拖窗保护。
9. **Digger:** `remove_project` 入口接通。

## 8. 验证

- **测试数量守恒:** Dozer 抽库前后 Rust 与前端测试总数相同、全绿。
- **依赖闭包:** `cargo tree -p dozer-panel-project` 里没有 `dozer-app`、`dozer-client`、`dozer-core`;`Cargo.lock` 里每个共享库只有一份;`scripts/check-shared-deps.sh` 与 `check-host-deps.sh` 通过。
- **Dozer 目测:** Project 面板外观与交互与抽库前一致(用户验收)。
- **Digger 真机(只能截图,点击要人来做):**
  - 左栏第一个图标是 Project,面板能显示信息、增删记忆、改名、移除项目。
  - 两个项目之间切页签,各自的布局互不影响;从旧的全局 `shell.json` 升级后布局不丢。
  - 拖窗保护按 §6.3 的描述逐项验证。
- **Digger 自动化:** 旧布局迁移、按项目读写、无已开项目空状态都有测试;`cargo test --workspace`、`clippy`、`fmt --check` 通过。

## 9. 我的决定与未解问题(请审阅时重点看)

我的决定(用户没说过,可以推翻):

- D1 库名叫 `dozer-panel-project`(按 `dozer-panel-todo` 类推)。
- D2 Digger 里依赖转录的动作先不显示,而不是置灰(§6.4)。
- D3 按项目拆分布局放在 S5a 里(原计划写在 S5),放在 Project 面板接入之后。
- D4 旧的全局 `shell.json` 作为所有项目的初始布局。
- D6 没有已开项目时用全局布局(§6.3)。
- D5 已有用户的旧布局里没有 `digger.project`:按 S1 的既有规则"清单里有而文件里没有的面板回到默认栏",再把它放在左栏最前面。

未解:

- Q1 项目描述写在 `<项目>/.dozer/description.md`,脚手架也会在项目里建 `.dozer/`。Digger 打开项目后也会往用户的目录里写这个文件夹,与 Dozer 一致,但这是 Digger 第一次往用户目录写东西。是否接受?
- Q2 Digger 的 Project 面板在"打开项目时的静默脚手架"这一步做不做(Dozer 里是静默建 `.dozer/` 并补录转录)?我倾向于 Digger 里只建 `.dozer/`、不补录。

## 10. 留给实现者决定

库内部的模块切分细节、`ProjectOutbox` 的具体字段、`shell.json` 新 schema 的版本号与字段名、迁移的触发时机(启动时一次性,还是首次保存时)、拖窗保护里 unsafe 代码的封装位置。

## 11. 修订(2026-10-18,写实施计划时读代码发现)

1. **Dozer 的 Project 面板是配对视图**:左半 `project::view`(信息),右半 `project_preview_pane`(项目文件的预览页签,默认空白页显示 `BlankPaneInfo`)。右半属于 files(S5b)。**所以 S5a 里 Digger 的 Project 面板是单栏的信息面板**(`rules()` 返回 `None`,不配对),与 Dozer 的左半逐项一致;配对与预览随 S5b 一起到。这是 D7,是对"一模一样"的一处临时偏离。
2. **共享宿主件已经存在**:`dozer-panel-host` 已提供 `ActivationCtx`、`RequestTrackers`、`scope_of` / `project_of`、`NoticeOutbox`、`HostOutbox`。§6.1 表里"库自带激活上下文 / `ProjectOutbox`"改为**直接用 `dozer-panel-host`**,不新造;`dozer-panel-todo` 就是这么做的。
3. **拖窗保护的落点是 `bytehost-chrome`**(`platform_macos.rs`,它的文件头注释明确写着等顶栏有项目页签时再移植),不是 Digger 自己。这需要 bytehost 发新 tag,Digger 再升级。产品侧每帧要告诉它"鼠标是否悬停在顶栏的可交互控件上"。
4. **脚手架比设计里写的更动磁盘**:`scaffold_steps()` 是四步——建 `.dozer/`、写 README、`git init`、写项目文档。设计 Q1/Q2 把它说成"只建 `.dozer/`"是不准确的。**计划里的默认改为:Digger 打开项目时不做任何静默脚手架**,只有用户在面板里点"修复项目"才执行这四步(行为与 Dozer 一致,但由用户触发)。项目描述仍写 `<项目>/.dozer/description.md`,那是用户在面板里主动编辑才写。
5. **Dozer 一侧还要登记新 crate 的门禁**:`scripts/i18n/*.py` 的扫描路径、`scripts/audit/check_panel_boundary.py`(`extensions/` 下不得出现 `crate::app::App`,所以 `PanelDisplay` 的 `App` 实现要放 `host_adapters/`,同 todo)、`dozer-app/src/i18n.rs` 的 owner 注册。
