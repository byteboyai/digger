# S6 Agent 面板与产品自己的守护进程 `diggerd`

- 日期:2026-10-18
- 状态:设计稿,待用户审阅
- 路径:架构级,**分四期交付,每期单独写实施计划**(S6a 守护进程与产品身份、S6b 终端控件库、S6c Agent 面板库、S6d Digger 接入)
- 相关:ADR-014(产品运行时互相独立、只共享代码)、ADR-011(bytehost 进程内应用宿主)、`2026-10-18-project-panel-s5a-design.md`(Project 与项目注册表)、Dozer `2026-07-18-dozer-p1e-agent-integration-design.md`、`2026-08-16-agent-card-workspace-design.md`

## 1. 用户意图

Digger 要有与 Dozer 一模一样的 Agent 面板(迁移计划里排在 project、files 之后)。2026-10-18 的决定:

1. **Agent 运行时走方案 A:Digger 自带独立的守护进程**,而不是把 `dozerd` 嵌进进程。
2. **Digger 用自己名字的守护进程和数据库:`diggerd` 与 `digger.db`**,不借用 `dozerd` / `dozer.db`。两个产品只共享代码,不共享进程、socket、目录、数据库(ADR-014 第 3 条)。

## 2. 现状(读代码得到)

- Agent 面板是配对视图:左 `term/` 终端(`alacritty_terminal` + iced 自绘,约 2000 行),右 Agent 会话卡片列表(`workspace/view.rs` 的 `agent_list_pane`)。面板状态混在 `workspace/state.rs`(3388 行的 `Workspace`)里,没有独立成库。
- 会话与 PTY 由守护进程 `dozerd` 持有(`session.rs` 用 `portable_pty`);界面经 `dozer-client`(约 3000 行,85 个方法)走 Unix socket 连它。会话状态靠推送:Agent 的 hook 事件 → `dozerd` 广播 → `dozer-client` → 面板。
- `dozerd`(约 2 万行)不只管会话:总结流水线、群聊存储、待办、书签、IDE 桥接等都在里面,**共用 `state_dir/dozer.db`**,其中包含项目注册表。`main.rs` 只有 216 行,逻辑大部分已在 `[lib]` 里。
- 数据根、运行态目录、`dozerd.sock` 路径写死在 `dozer-core::paths`(`ProjectDirs("ai","byteboy","dozer")`),没有任何覆盖方式。
- hook 由 Agent 命令行在自己的会话里调用,`dozer-hook` 只有在环境里有 `DOZER_SESSION_ID`(由 `dozerd` 拉起会话时注入)才工作,并连编译期默认的 `socket_path()`。hook 命令被**全局**写进各 Agent 的配置(`~/.claude/settings.json` 等),`ensure_hook_installed` 每次启动都会把它改写成当前产品旁边的 `dozer-hook` 路径(2026-08 出过"堆出重复坏 hook"的事故,所以这段有回归测试)。
- Digger 一侧:项目注册表在进程内的 `<config>/projects.db`(S3p,`dozer-store-project`);没有任何守护进程。

## 3. 目标

- G1 `dozerd` 变成**共享的守护进程库**,Dozer 的 `dozerd` 与 Digger 的 `diggerd` 都是它的薄入口;两者数据根、socket、数据库文件、环境变量名各自独立。
- G2 Digger 打包并拉起自己的 `diggerd`(以及它的 `digger-hook`),数据库是 Digger 数据根下的 **`digger.db`**。
- G3 终端控件与 Agent 面板各抽成 Dozer 里的共享库,Digger 依赖同一份实现。
- G4 Digger 里 Agent 面板可用:在当前项目目录里新建各种 Agent 会话、终端输入输出、会话列表(状态、标题、模型、模式、工作区分支)、切换/结束会话,会话随守护进程存活。
- G5 Dozer 与 Digger 同时安装并同时运行时,两个产品的 hook、会话、事件互不串线。

## 4. 非目标

- 不做 `dozer-mcp` / `digger-mcp`(把面板能力暴露给 Agent 的工具)。留作 S6e,Agent 面板稳定后再做。
- 不做会话(转录)面板、群聊面板(各自单独一轮);但 `diggerd` 里这些后台功能**本期照常运行**(不做功能裁剪),Digger 只是暂时没有对应界面。
- 不把 `dozerd` 嵌进 Digger 进程(方案 B)。
- 不改变 Dozer 的任何用户可见行为、协议、数据位置。

## 5. 反目标(技术上做到了也算失败)

- Dozer 或 Digger 的 hook 事件被对方的守护进程收到,或一个产品把另一个产品的 hook 配置覆盖掉导致对方失效。
- Digger 里出现两份互不知情的项目列表。
- Digger 的 `diggerd` 读写了 Dozer 的目录、socket 或数据库(反过来同理)。
- 库里出现 `dozer-app` 的依赖,或 Digger 里复制一份 Agent 面板 / 终端实现。

## 6. 设计

### 6.1 产品身份(S6a,Dozer 一侧)

在 `dozer-core::paths` 引入 **`ProductId`**(进程启动时由入口设置一次,`OnceLock`):携带产品名(`dozer` / `digger`)、`ProjectDirs` 的 qualifier 三元组、socket 文件名(`dozerd.sock` / `diggerd.sock`)、数据库文件名(`dozer.db` / `digger.db`)、会话环境变量名(`DOZER_SESSION_ID` / `DIGGER_SESSION_ID`)、hook 二进制名(`dozer-hook` / `digger-hook`)。未设置时默认 `dozer`,所以 Dozer 现有行为字节不变。`config_dir()`、`state_dir()`、`socket_path()`、`logs_dir()` 都按它推导。

### 6.2 守护进程成为库

把 `dozerd/src/main.rs` 的启动装配搬进 `dozerd::run(config: DaemonConfig) -> Result<()>`(`DaemonConfig` 含 `ProductId` 与数据库路径)。Dozer 的 `dozerd` 入口变成 3 行;Digger 在自己的仓库里新建 `crates/diggerd`,依赖 `dozerd` 库,入口同样 3 行,设置 `ProductId::digger()` 后调 `run`。

### 6.3 hook:各产品一份,并排共存

- 每个产品安装**自己名字的 hook 命令**(`dozer-hook` / `digger-hook`),写在同一份 Agent 配置的 hook 数组里**并排**,互不覆盖。
- 每个 hook 二进制编译进自己的 `ProductId`:只读自己产品的会话环境变量(`DIGGER_SESSION_ID`),连自己产品的 socket;环境里没有自己的变量就直接返回。于是 Dozer 会话里,`digger-hook` 什么都不做;Digger 会话里,`dozer-hook` 什么都不做。
- 安装逻辑按"命令里含本产品的 hook 名"识别与更新自己的条目,**不碰别人的条目**。这一条要有钉死测试(见 §8),因为 2026-08 的"重复坏 hook"事故正是这里。
- 其余 Agent 的安装器(opencode 插件、goose 插件、aider 启动器、codex 等)里写死的 `dozer` 命名同样要参数化,具体清单由实现者在 S6a 逐个列出。

### 6.4 项目注册表:`digger.db` 单一文件

`diggerd` 的所有表(项目、待办、书签、群聊、会话……)放在 Digger 数据根下的 **`digger.db`**。Digger 进程内的项目存储(S3p 的 `ProjectHost` / `dozer-store-project`)**改为打开同一个 `digger.db`**,不再用 `projects.db`;首次启动时若存在 `projects.db` 就把它的项目与记忆迁入 `digger.db`(幂等,迁入后改名保留,不删除)。两个进程同时读写同一个 SQLite 文件,靠 WAL 和忙等待,**S6a 里要有并发写入的钉死测试**(见 §8)。

### 6.5 终端控件库 `dozer-panel-term`(S6b)

把 `term_model`(`alacritty_terminal` 的 VTE 与网格)与 `term_view`(iced 自绘控件,约 1700 行)抽成库;不含 PTY,PTY 在守护进程里。依赖闭包不含 `dozer-app` / `dozer-client` / `dozer-core`。

### 6.6 Agent 面板库 `dozer-panel-agent`(S6c)

把 Agent 相关状态从 `Workspace` 里拆出来:会话列表、卡片、状态、选中、新建会话入口、与守护进程的订阅。面板对外只依赖一个小 trait(参照已有的 `DaemonService`,按面板实际用到的调用补全:列会话、新建、结束、写入、调整大小、订阅事件),由产品实现——Dozer 用 `dozer-client`,Digger 也用 `dozer-client`(只是连 `diggerd`)。`transcript.rs` 里被卡片用到的字段解析(`permissionMode`、模型 id)随库走。

### 6.7 Digger 接入(S6d)

- 打包:`diggerd`、`digger-hook` 放进 `Digger.app/Contents/MacOS/`(`package-macos.sh` / `release.sh` 同步);拉起逻辑同 Dozer 的 `spawn_dozerd`:在自己的可执行文件旁边找,单实例检查。
- 面板:`digger.agent` 配对视图(左终端、右列表),放进左栏;当前项目的路径作为新会话的工作目录;无已开项目时显示空状态。
- 会话随守护进程存活:关掉 Digger 窗口不杀会话,下次启动能重新连上(与 Dozer 一致)。
- Agent 种类:与 Dozer 一致的全部种类;某个 Agent 命令行没装时,新建时给出可读错误。

## 7. 分期

- **S6a 守护进程与产品身份**:`ProductId`、`dozerd::run`、hook 并排共存与参数化、协议版本字段与兼容检查;Dozer 一侧全做、测试守恒;Digger 一侧新建 `diggerd` 与 `digger.db`、项目存储迁移、打包与拉起。**本期结束时 Digger 有自己的守护进程在跑,但还没有 Agent 界面。**
- **S6b 终端控件库**:抽 `dozer-panel-term`,Dozer 改引用。
- **S6c Agent 面板库**:抽 `dozer-panel-agent`,Dozer 改引用,目测验收,发 tag。
- **S6d Digger 接入**:依赖升级、`digger.agent` 面板、真机回归。

每期都是"Dozer 抽库 → 目测验收 → 发 tag → Digger 接入"的节奏,Dozer 每步编译且测试全绿、测试总数守恒。S6a 要求 S5a 已合并(项目存储与左栏);S6d 还要求 S5b-1(文件树)之后的左栏顺序已定。

## 8. 验证

- Dozer:每期前后测试总数守恒;`ProductId` 默认值下路径、socket、环境变量名与改造前字节一致(黄金测试)。
- **隔离钉死测试(S6a)**:同一台机器上同时起 `dozerd` 与 `diggerd`,各拉起一个会话:① Dozer 会话里触发 `digger-hook`,`diggerd` 收不到事件;② Digger 会话里触发 `dozer-hook`,`dozerd` 收不到事件;③ 各自的 socket、数据库、日志目录互不重叠。
- **hook 配置钉死测试(S6a)**:对一份已含 Dozer hook 条目的 Agent 配置,Digger 安装后两个条目并存;重复安装不产生重复条目;卸载只移除自己的条目;反过来同理。
- **并发写入钉死测试(S6a)**:`diggerd` 与 Digger 进程内的项目存储同时对 `digger.db` 做上千次交错写,不丢写、不出现 `database is locked`。
- **迁移测试(S6a)**:存在旧 `projects.db` 时首次启动迁入 `digger.db`,再次启动不重复迁入,旧文件被改名保留。
- Digger 真机(只能截图,点击要人来做):Dozer 与 Digger 同时运行,各开一个 Claude 会话,状态点各自更新、互不影响;在 Digger 里新建会话、输入输出正常、会话列表卡片信息正确;退出再启动能重连会话;项目列表只有一份。

## 9. 我的决定与未解问题

我的决定(可以推翻):

- D1 `ProductId` 用进程级 `OnceLock`,由入口设置,而不是到处传参。
- D2 hook 并排共存、各认各的环境变量,不用"互换同一个 hook"的方案。
- D3 `diggerd` 本期不做功能裁剪,后台总结/群聊等照常运行。
- D4 `dozer-mcp` 对应的 Digger 版推迟到 S6e。
- D5 旧 `projects.db` 迁入后改名保留,不删除。
- D6 守护进程与进程内存储共用 `digger.db`(而不是让 Digger 也经守护进程读写项目)。

未解:

- Q1 `diggerd` 本期完整运行会占用多少资源(总结流水线是否会在没有界面的情况下自己调用外部 LLM、产生费用)?我倾向在 S6a 里先加一个"后台总结默认关闭"的配置,由 Digger 的入口设置;请你确认这样做是否符合预期。
- Q2 两个进程写同一个 SQLite 文件在 macOS 上的表现需要 S6a 里实测。若实测不稳,退路是 D6 的反面:Digger 的项目存储改为经 `diggerd` 读写。
- Q3 协议版本不一致时(Digger 打包的 `diggerd` 与 `digger-hook` 版本不同步)如何降级,实现者在 S6a 里按"拒绝连接并给出可读错误"处理,是否需要更复杂的兼容,请你定。

## 10. 留给实现者决定

`ProductId` 的具体字段与构造函数、`DaemonConfig` 的形状、各 Agent 安装器里需要参数化的命名清单、迁移的触发点、`diggerd` 单实例检查的实现、S6b / S6c 里状态与视图的逐个字段归属。
