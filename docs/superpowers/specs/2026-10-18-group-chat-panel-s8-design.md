# S8 群聊面板:抽成共享库,Digger 接入

- 日期:2026-10-18
- 状态:设计稿,待用户审阅。**2026-10-25:S6 与 S3b 之后的修订见 Dozer 仓库 `docs/superpowers/plans/2026-10-25-s7-s8-refresh.md`,冲突处以它为准**
- 路径:架构级,**分三期交付,每期单独写实施计划**(S8a Dozer 抽库、S8b Digger 接群列表与状态、S8c Digger 接聊天内容 webview 与发布)
- 相关:`2026-10-18-conversations-panel-s7-design.md`(同类的「列表 + webview 内容」面板,S7 的做法是本文的模板)、`2026-10-18-agent-panel-s6-design.md`(`diggerd` 与 `digger.db`)、`2026-10-18-agent-panel-s6d-design.md`(Digger 里已有的原生 webview 宿主做法、覆盖层互斥)、Dozer `2026-10-02-group-chat-panel-design.md`

## 1. 用户意图

Digger 要有与 Dozer 一模一样的群聊面板:在当前项目里建一个「群」,往里加 Agent 成员(各有 @句柄和角色提示),人类发言并 @成员,各成员的 Agent 命令行被后台拉起处理并把回复流回群里;可取消、重试、删群、增删改成员。库抽在 Dozer 里,两个产品共用一份实现。群聊在 Dozer 里是右栏第二个面板(Agent 之后、会话之前)。

## 2. 现状(读代码得到)

- **后端在守护进程里,且已是服务化的。** `dozer-services::GroupChatService`(12 个方法:列群、按 `rev` 增量取消息、建/删群、成员增删改、发言、取消、重试、「转待办」)由 `Client` 实现;`dozerd` 里 `group_store` / `group_service` / `group_adapter`(按成员的 Agent 种类拼命令行、一次性无头调用,发言前移除 `DOZER_SESSION_ID`)/ `group_mentions` / `group_prompt`。群数据在同一个 SQLite 里(Digger 即 `digger.db`)。成员可选种类固定为 `CREATABLE_AGENTS = [Claude, Codex, Codebuddy, Opencode, Qodercn]`。
- **面板是「纯 `update` + 效果」的形状。** `extensions/group_chat/mod.rs`(1978 行)的 `update(state, msg) -> Vec<Effect>` 不碰 `Client`、不 spawn,竞态(乱序、重复、换群后的旧结果、发言占位的 `rev`)在单测里覆盖;`App` 负责执行效果。后端更新靠 `rev` 增量**轮询**:有进行中的发言时每 500ms 一次,出错退避 5 秒,每次最多取 200 条。
- **聊天内容是 webview。** `web/group-chat-content/`(Preact,产物 `assets/group-chat-content/{host.html,group-chat-content.js,group-chat-content.css}`)经 `dozer://group-chat-content/…` 提供;Rust 把**算好的视图模型**(作者显示名、成员颜色槽位、状态映射、提示文案,前端不做业务判断)推给页面(`protocol.rs`,1201 行,已是纯函数:`current_view_payload`、`encode_group_chat_push`、`WebviewPushState` 声明式去重、`parse_group_chat_event` 校验页面发回的事件并限长);页面 10 秒内不发 `ready` 判失败,原生壳显示原因与「重试」。
- **原生部分:** 群列表列(`view.rs`,158 行:头部「新建群聊」按钮、群卡片、新建群弹窗含多行主题输入框 `text_editor` 与可勾选的成员 Agent);草稿(消息输入框)按群存在宿主侧,因为 webview 单固定槽、切面板即销毁重建。
- **耦合点:** `PanelIo` / `RequestTrackers`(请求追踪)、`toast::Outbox`、`homespace::home_panel_head_with_actions`、`crate::theme::region::group_chat_list_pane`、`i18n`(owner `dozer.group_chat`)、`App` 里的 webview 池与几何、`poll_group_chat_if_active`、`push_group_message_to_todo`(需要待办面板)。
- **Digger 一侧:** 没有群聊面板;`UnavailableService` 占位里 `GroupChatService` 恒不可用(S5a 时为让 `Services` 结构完整而留);Digger 已有两套原生 webview 宿主(用量图表、S7 的审阅)和覆盖层互斥规则。

## 3. 目标

- G1 新建 Dozer 共享库 `dozer-panel-group-chat`:状态 / 消息 / `update` + `Effect` / 轮询调度 / 视图模型与推送协议 / 页面事件解析 / 群列表与新建群弹窗视图 / 失败原生壳 / `group-chat-content` 页面资源与纯函数协议应答;Dozer 改引用,**用户可见行为不变**。
- G2 对守护进程的访问全经 `GroupChatService`,库不含 `dozer-client`。
- G3 Digger 右栏出现群聊面板(Agent 之后),配对:内容(聊天)| 群列表(右栏约定,拖到左栏镜像);数据来自 `diggerd` / `digger.db`。
- G4 Digger 的聊天内容用**同一份页面**,外观与行为同 Dozer(发言、@提及、流式回复、取消、重试、成员管理、删群)。
- G5 Digger 里不支持的功能(「转待办」)不显示,页面与原生部分都不留死入口。

## 4. 非目标

- 不做待办面板的 Digger 接入(「转待办」先隐藏)。
- 不改后端:群存储、提示词、@提及解析、各 Agent 的无头适配、成员种类。
- 不改变群聊页面的视觉与交互(除新增一个「隐藏转待办」的能力字段,见 6.2)。
- 不做 MCP 工具(S6e 不暴露群聊)。

## 5. 反目标

- 库依赖 `dozer-app` / `dozer-client` / `dozer-core` / `wry`,或 Digger 里复制一份页面或协议逻辑。
- 页面执行未经校验的事件:任何超长文本、越界成员/群 id、未知事件种类都必须在 Rust 侧拒绝(现有 `parse_group_chat_event` 的限长与校验随库保持,不放宽)。
- 页面加载外部资源;CSP 被放宽。
- 轮询风暴:切走面板后仍高频轮询;多个项目同时轮询;`diggerd` 不在时死循环重试。
- 一个项目的群出现在另一个项目下;删群后残留订阅或草稿;webview 盖住覆盖层。
- 在没有用户明确操作时拉起 Agent 命令行(群聊只在用户发言/重试时调用成员,创建群或加成员本身不调用)。

## 6. 设计

### 6.1 库的边界(S8a,Dozer)

`dozer-panel-group-chat` 依赖 `dozer-model`、`dozer-services`、`dozer-i18n`、`dozer-panel-host`、byteui、`serde*`、`tokio`;**不含** `dozer-app` / `dozer-client` / `dozer-core` / `wry`。

- 库拥有:`WorkspaceState`、`Message`、`Effect`、`update`(原样,纯)、`run_effect`(经 `GroupChatService` 执行效果并 `emit`)、`poll_if_due`、`select_group`、视图模型与推送协议(`protocol.rs` 整体搬入)、`list_pane` / 新建群弹窗 / 失败壳视图(经 `PanelDisplay` 注入面板头、区域度量、折叠按钮、悬停)、页面资源(`include_bytes!`)与 `serve_group_chat_content(path) -> Option<(mime, Vec<u8>)>`(纯函数,路径穿越拒绝)、`GroupChatWebviewSpec`(URL 带 scheme 参数、`visible`、`park_offscreen`)。`web/group-chat-content/` 源码随库迁入,构建脚本与「产物齐全」「CSP 严格」测试随走。
- **服务接缝:** `GroupChatService` 已存在,无需新增 trait。`push_group_message_to_todo` 保留在 trait 里,库在 `caps.push_to_todo == false` 时不发该效果。
- **能力开关:** `GroupChatCapabilities { push_to_todo: bool }`(Dozer 开,Digger 关)。
- **宿主动作:** 无原生菜单依赖;删除群走 webview 内容区菜单(现状);新建群弹窗在库的原生视图里。

### 6.2 页面的一处必要改动(S8a)

页面里有「转待办」按钮与对话框。让 Digger 隐藏它,需要推送载荷里带一个能力字段:`GroupChatViewPayload.capabilities: { push_to_todo: bool }`(**缺省为真**,以兼容旧载荷与旧页面)。Rust 侧按 `caps` 填,页面按它渲染按钮;Dozer 传真,**页面外观与行为不变**。这是对 `protocol.rs` 序列化形状的**加法**(新增字段),`GROUP_CHAT_PROTOCOL_VERSION` 不变;改动需要重新构建页面产物并提交。黄金测试固定「Dozer 传真时,载荷除新增字段外与改造前逐字相同」。

### 6.3 Digger 接入(S8b / S8c)

- **登记与布局:** 面板 id `digger.group_chat`,标题「群聊」,图标 `SquareSparkles`(与 Dozer 图标栏一致);配对面板(第一块是群列表,可拖窄收起);右栏顺序 `[agent, group_chat, conversations, apps, usage]`,旧布局文件缺该面板时回默认栏并放在 `agent` 之后,**不改用户当前选中的面板**。
- **数据:** `GroupChatService` 由 `Client`(连 `diggerd`)提供;每项目一份 `WorkspaceState`,消息 `Msg::GroupChat(i64, Message)` 带项目 id。
- **轮询:** 库的 `poll_if_due`;Digger 现有的 1 秒 `Tick` 太慢,发言进行中需要 500ms 节拍:仅当某个已打开项目的群聊状态 `has_active_turn()` 且面板可见时才订阅 500ms 定时器(同 `shell.animating()` 的开关做法),其余时候不额外订阅;面板不可见时不轮询,切回时做一次补取;每次轮询只针对当前选中的群。
- **聊天内容 webview(S8c):** 沿用 S7c 的 `review_native` 做法(单槽 `thread_local` webview、epoch、纯函数 `plan()`、`_r` 式的导航校验);协议注册名取 `digger`,主机名 `group-chat-content`;`ready` 超时(10 秒)与失败壳、推送去重(`WebviewPushState`)、草稿恢复都在库里。**两个 Digger 原生宿主(审阅、群聊)的 wry 样板此时已重复,S8c 先把 `plan()` / epoch / 导航白名单抽成共用的 `adapters/native_host.rs`,`review_native` 改用它**(行为不变,测试随走)。
- **键盘:** 聊天输入框在 webview 里,按键由网页接收,不进入终端(Agent 面板的键盘订阅只处理 iced 里 `Ignored` 的事件,webview 内的按键不会到达);新建群弹窗的主题框是 iced `text_editor`,同样捕获按键。
- **「转待办」:** `caps.push_to_todo = false`,页面按 6.2 隐藏。

### 6.4 分期

- **S8a(Dozer):** 黄金测试(视图模型载荷、页面事件解析、页面资源应答)→ 抽库 → 6.2 的页面加法 → Dozer 改引用 → 目测验收 → 切 tag。**不单独发布**,发布放 S8c。
- **S8b(Digger):** 升依赖、登记与布局迁移、每项目状态与路由、群列表与新建群弹窗、轮询调度、不可用态;内容区先放占位。
- **S8c(Digger):** 抽 `native_host.rs`、聊天内容 webview 宿主、与 Dozer 逐状态对照、压力与容错回归 → **发布 Dozer 与 Digger 各一个新版本**(项目规定)。

## 7. 验证

- Dozer:测试总数守恒;`mod.rs` 与 `protocol.rs` 的单测随代码搬走、不改期望值;**视图模型载荷黄金测试**(一组覆盖作者种类、状态、已移除成员、颜色槽位回绕、超长主题的状态 → 序列化 JSON,除新增 `capabilities` 字段外与改造前逐字一致,在改代码前生成);页面资源与 `serve_group_chat_content` 应答黄金;事件解析的恶意输入表(超长、越界 id、未知种类、嵌套 JSON、非 UTF-8);依赖闭包无禁用 crate。
- 用户在 Dozer 上目测:建群、加/改/删成员、发言(@单个/多个/不存在的句柄)、流式回复、取消(单条/整群)、重试、删群、新建群弹窗(勾选成员、多行主题)、webview 加载失败壳与「重试」、转待办、折叠列表、中英文切换,与改造前一致。
- Digger 单测:每项目隔离;轮询调度(进行中 500ms、空闲不轮询、出错退避、不可见不轮询、切回补取);能力关闭时载荷 `push_to_todo == false`;项目关闭后状态/草稿清理;`diggerd` 不可用态。
- Digger 真机(沙箱数据根;点击与输入由人做):建群加两个成员、发言并看到流式回复、取消、重试;Agent 命令行未安装时的可读失败;删群;与 Dozer 同机并存各自只看到自己库里的群;重启后群与历史消息仍在。

## 8. 我的决定与未解问题

我的决定(可以推翻):

- D1 库名 `dozer-panel-group-chat`;面板 id `digger.group_chat`;右栏 `[agent, group_chat, conversations, apps, usage]`。
- D2 页面的「隐藏转待办」用载荷里的能力字段实现(缺省为真),而不是 Digger 另做一份页面。
- D3 Digger 的轮询节拍只在有进行中的发言时升到 500ms,其余用现有的 1 秒 `Tick`。
- D4 S8c 把两个 Digger 原生 webview 宿主的共同部分抽成 `native_host.rs`(第二次出现才抽)。
- D5 轮询只针对当前选中群,不为所有群同时轮询。
- D6 webview 自定义协议的 scheme 由产品传入(Digger 用 `digger`),同 S7 的 D7。

未解:

- Q1 **【2026-10-25 订正,原文写反了】** 群聊会在用户发言时以**只读**方式拉起被 @ 的成员 Agent 命令行(Claude:`--allowedTools Read,Grep,Glob` 加禁用 Bash/Edit/Write 等;Codex:`--sandbox read-only`;严禁 `--dangerously-skip-permissions` / `-y`),工作目录是项目目录,所以成员**能读**项目文件作答、**不能修改**任何东西(与 Dozer 完全一致)。Digger v1 照做、不额外加首次使用的确认弹窗,理由是建群与加成员都是用户的明确操作。详见 Dozer 仓库 `docs/superpowers/plans/2026-10-25-s7-s8-refresh.md` §1.1。是否接受?
- Q2 「转待办」在 Digger 里隐藏,待办面板接入 Digger 之后再打开。是否接受?
- Q3 页面产物需要重新构建并提交(6.2 的加法)。构建环境用 Dozer 现有的 `npm`(`web/group-chat-content`),产物字节会变。是否接受这条对 Dozer 的改动?(Dozer 用户可见行为不变,黄金测试固定。)

## 9. 留给实现者决定

`Message` / `Effect` 的最终清单(原样搬,不增不减)、`PanelDisplay` 的精确方法、`GroupChatWebviewSpec` 字段、几何度量(对照 Dozer `group_chat_content_pane_bounds_for`)、`native_host.rs` 的抽取边界、500ms 订阅的开关位置、各 Agent 无头适配在 Digger 里是否需要额外的环境变量处理(`group_adapter` 里 `env_remove("DOZER_SESSION_ID")` 是否已由 S6a 参数化为产品的会话环境变量,S8a 开工时先核对,没有就先停下报告)。
