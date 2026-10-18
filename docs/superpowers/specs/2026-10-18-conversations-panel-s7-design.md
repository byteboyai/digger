# S7 会话(Conversations)面板:抽成共享库,Digger 接入

- 日期:2026-10-18
- 状态:设计稿,待用户审阅
- 路径:架构级,**分三期交付,每期单独写实施计划**(S7a Dozer 抽库、S7b Digger 接列表、S7c Digger 接审阅详情与发布)
- 相关:`2026-10-18-agent-panel-s6-design.md`(`diggerd` 与 `digger.db`)、`2026-10-18-agent-panel-s6c-design.md`(S6c-2 里 Agent 卡片的 transcript 尾部解析留在 Agent 库)、`2026-10-18-preview-engine-s5b2-design.md`(webview 宿主的分层做法)、`2026-10-18-project-panel-s5a-design.md`(Q2:Digger 不做静默补录)、Dozer `2026-09-15-conversations-extension-pilot-design.md`

## 1. 用户意图

Digger 要有与 Dozer 一模一样的会话面板:当前项目的全部历史会话列表(搜索、按 Agent 筛选、分页、卡片),点一行进入详情审阅(人类发言、AI 回复、思考、工具调用与结果、token 用量的时间线)。库抽在 Dozer 里,两个产品共用一份实现。会话面板在 Dozer 里是右栏第三个面板(Agent、群聊之后)。

## 2. 现状(读代码得到)

- **列表侧已经是扩展式面板。** `extensions/conversations.rs`(1249 行)有自己的 `WorkspaceState` / `Message` / `update` / `view`,数据经 `ConversationService::list_conversations_with_summaries(cwd, agent, limit, offset)`(`Client` 已实现)。耦合点:`PanelHost`(悬停、折叠、语言)、`toast::Outbox`、`homespace::home_panel_head`、`workspace::{agent_dot_color, agent_icon, lh, relative_time_text}`、`TextInputTarget`、macOS 原生菜单(Agent 筛选)、搜索框焦点探测(`CaptureConversationSearchFocus`,为 Dozer 的 winit 键盘路由服务)、待办面板数据(`TodoInfo`,卡片上的「关联待办」)。
- **详情侧是内核基础设施。** 点行后由 `App::conversation_session_open` 写 `ws.review`(`ReviewView`):后台 `Client::get_conversation_turns(conversation_id, after_turn_index, limit)` 取 `TurnRecord`,`transcript::review_entries_from_turns` 折叠成 `ReviewEntry`(`Human` / `AiTurn` / 孤儿 `ToolResult`),序列化后交给一个 wry webview:页面是 `web/review-trace/`(Preact,产物 `assets/review-trace/{host.html,review-trace.js,review-trace.css}`,严格 CSP、无外部引用),经自定义协议 `dozer://review-trace/…` 提供,数据由协议应答注入而非页面自取;页面回报 `DocumentLoaded` 事件前 webview 离屏停放;出错或没有条目时显示 iced 文案。「加载更多」按 200 条一页追加。
- **类型位置:** `TurnRecord`、`ToolCallInfo`、`SummaryJobInfo`、`SummaryJobStatus`、`SummaryTrigger`、`SessionRow` / `ConversationRows` 都已在 `dozer-model`(`dozer-core::protocol` 只是重新导出),库可直接用。缺的是服务接缝:`get_conversation_turns`、`submit_summary_job`、`get_summary_job` 都是 `Client` 的固有方法,不在任何服务 trait 里。(2026-10-18 写 S7a 计划时核实,订正此前「类型要搬家」「总结走批量接口」的说法。)
- **总结生成依赖守护进程的总结流水线。** 详情页「生成总结」经 `Client::submit_summary_job(conversation_id, .., Manual, .., force=true)` 提交、每秒 `get_summary_job` 轮询到终态,再刷新列表并重开详情;`diggerd` 按 S6a 的 D7 默认关闭总结(会调用外部 LLM、产生费用)。
- **数据怎么进库:** `dozerd` 在收到带 `transcript_path` 的 hook 事件与状态转换时增量摄取(`maybe_ingest_*`,不受 `DaemonFeatures` 的启动回填开关影响);历史会话靠启动回填或 `backfill_project_transcripts(cwd)`。Digger 的 `diggerd` 关了启动回填,所以**新装的 Digger 里只有从 hook 装好之后产生的会话,过去的历史默认看不到**。
- Digger 一侧:`ProjectPanel` 里的 `TranscriptService` 是恒 `Unavailable` 的占位(S5a);没有会话面板、没有审阅 webview。

## 3. 目标

- G1 新建 Dozer 共享库 `dozer-panel-conversations`:列表状态/更新/视图、`ReviewView` 与审阅数据折叠(`ReviewEntry`、`review_entries_from_turns`)、`review-trace` 静态资源与纯函数的协议应答、宿主事件解析;Dozer 改引用,**用户可见行为不变**。
- G2 对守护进程的访问全经服务 trait:`ConversationService` 补上 `get_conversation_turns`,新增 `SummaryService { submit_summary_job, get_summary_job }`。
- G3 Digger 右栏出现会话面板(Agent 之后),配对:列表 | 审阅详情;数据来自 `diggerd` / `digger.db`。
- G4 Digger 的审阅详情用**同一份 review-trace 页面**,外观与 Dozer 一致。
- G5 Digger 对「历史会话不在库里」给出明确入口,而不是一个无解释的空列表。

## 4. 非目标

- 不做群聊面板、待办面板的 Digger 接入(卡片上的「关联待办」在 Digger 里不显示)。
- 不做总结生成的 Digger 开通(见 Q2),Digger v1 隐藏「生成总结」,标题回落到会话自带标题。
- 不改 `review-trace` 页面本身、不改摄取与协议。
- 不把审阅 webview 并入预览引擎(S5b-2):它是单槽、非页签的宿主,生命周期不同。

## 5. 反目标

- 库依赖 `dozer-app` / `dozer-client` / `dozer-core` / `wry`,或 Digger 里复制一份 review-trace 页面或折叠逻辑。
- Digger 静默读取并复制用户的全部历史 transcript 到 `digger.db`(S5a Q2 的同一原则)。
- 审阅页加载任何外部资源或自行 `fetch` 本地文件;CSP 被放宽。
- Digger 里出现不工作的「生成总结」按钮,或点了没反应的入口。
- 一个项目看到别的项目的会话(按 `cwd` 过滤的边界:父子目录、符号链接、大小写)。
- 审阅 webview 盖住覆盖层(选择器、菜单、确认框),或在面板不可见时仍留在屏幕上。

## 6. 设计

### 6.1 接缝(S7a 前半,Dozer)

- 类型已在 `dozer-model`,无需搬家;协议编码字节不变(序列化黄金测试覆盖 `GetConversationTurns`、`SubmitSummaryJob`、`GetSummaryJob` 及其应答)。
- `Services` 增加 `summary: Arc<dyn SummaryService>`;`SummaryService` 两个方法对应 `Client` 同名方法。
- `ConversationService` 增加 `get_conversation_turns(conversation_id: String, after_turn_index: i64, limit: u32) -> BoxFuture<Result<Vec<TurnRecord>, ServiceError>>`;`Client` 实现、各 fake 补齐。

### 6.2 库的边界(S7a 后半)

`dozer-panel-conversations` 依赖 `dozer-model`、`dozer-services`、`dozer-i18n`、`dozer-panel-host`、byteui、`serde*`、`tokio`;**不含** `dozer-app` / `dozer-client` / `dozer-core` / `wry`。

- **列表:** `WorkspaceState`(`sessions`、`pages`、`search`、`agent_filter`、总结状态等)、`Message`、`update`、`view`、`filter_sessions`、`conversation_visible_count`、`conversation_agents_present`;i18n owner `dozer.conversations` 随库。产品注入:`PanelDisplay`(悬停进度、面板头、折叠按钮、`agent_icon` / `agent_icon_color`、相对时间文案、`locale`)、`ConversationsCapabilities { summaries: bool, linked_todos: bool }`(Dozer 全开,Digger 全关)、`LinkedTodos`(trait,按 `conversation_id` 取关联待办文本,Digger 给空实现)。搜索框焦点探测与 `CaptureConversationSearchFocus` **不进库**(那是 Dozer 的 winit 键盘路由专用,留在 `dozer-app` 适配层);Agent 筛选下拉改为库发 `ConversationsRequest::ShowAgentFilter { at, current }`,由产品用原生菜单或自己的弹层呈现(同 Agent 面板的 `ShowPicker`)。
- **详情:** `ReviewView`(`source` 只保留 `Conversation`;`Session` 变体经查已无入口时删除,有入口则保留并在 S7a 计划里写明)、`ReviewEntry`、`review_entries_from_turns`(搬自 `transcript.rs`,带测试)、`review_webview_spec(review, locale) -> Option<ReviewWebviewSpec>`(URL 带 `nonce` 与 `locale`、`visible = 已 loaded`、未就绪时 `park_offscreen`)、`review_pane_state`(空 / 加载中 / 就绪)、`serve_review_trace(path: &str, review_json: Option<&str>) -> Option<(mime, Vec<u8>)>`(纯函数,原 `assets.rs` 里服务 `review-trace/` 的那一段,资源 `include_bytes!` 进库)、`parse_review_trace_event`、详情内容的 iced 兜底视图(出错 / 空条目)。`web/review-trace/` 源码目录随库迁入,构建脚本与「产物必须齐全」「CSP 严格」两个测试随走。
- **「加载更多」与加载流程:** `Message::{SessionOpen, DetailLoadMore, ReviewLoaded}` 进库,经 `ConversationService::get_conversation_turns` 取数,200 条一页(`CONVERSATION_DETAIL_PAGE_SIZE`)。
- **总结生成:** 逻辑(提交、每秒轮询、四类终态与错误文案、完成后刷新列表并重开详情)进库,仅在 `caps.summaries` 时可用,经 `SummaryService`;Digger 不开。

### 6.3 Digger 接入(S7b / S7c)

- **登记与布局:** 面板 id `digger.conversations`,标题「会话」,图标 `BotMessageSquare`(与 Dozer 图标栏一致);配对面板,列表是第一块;右栏顺序 `[agent, conversations, apps, usage]`,旧布局文件缺该面板时回默认栏并放在 `agent` 之后,**不改用户当前选中的面板**。
- **数据:** `ConversationService` 与 `TranscriptService` 由 `Client`(连 `diggerd`)提供,取代 `ProjectPanel` 里的占位;列表按当前项目路径过滤。面板可见与每次 `AgentHostEvent::TurnEnded` 后刷新列表(S6d 里被忽略的那个事件在这里接上)。
- **历史导入(G5,Q1):** 列表为空且本项目尚未导入过时,空态文案说明「这里只显示 Digger 装好 hook 之后产生的会话」,并给一个明确的按钮「导入此项目的历史会话」,点击才调 `TranscriptService::backfill_project_transcripts(cwd)`,完成后刷新并显示导入条数;导入过的项目记入 `digger.db` 的一个标记,不再提示。按钮旁一行说明会读取 `~/.claude/` 等位置的会话文件并把文本存进 `digger.db`。
- **审阅 webview(S7c):** 沿用用量图表的做法(`adapters/usage_native.rs` 的原生 webview + `sync_native` 摆位),矩形取配对的第二块减去面板头;协议应答调库的 `serve_review_trace`,数据在 `nonce` 变化时重新导航;未 `DocumentLoaded` 前离屏停放;面板不可见、有覆盖层、窗口关闭时隐藏或销毁。
- **键盘:** 搜索框是 iced `text_input`,按键被它捕获(状态 `Captured`),不会进终端,无需 Dozer 那套焦点探测。

### 6.4 分期

- **S7a(Dozer):** 6.1 接缝 + 黄金测试 → 6.2 抽库(列表、详情逻辑与资源)→ Dozer 改引用 → 目测验收 → 切 tag。**不单独发布**:发布放在 S7c 收尾,Dozer 与 Digger 各一个新版本(项目规定按「面板迁移完成」计,迁移在两个产品里都落地才算完成)。
- **S7b(Digger):** 升依赖、登记与布局迁移、列表、历史导入、`TurnEnded` 联动、空态与不可用态。
- **S7c(Digger):** 审阅 webview 宿主、「加载更多」、覆盖层互斥、与 Dozer 逐状态对照、同机并存回归 → **发布 Dozer 与 Digger 各一个新版本**(项目硬性规定:每个面板迁移完成后各发一个版本)。

## 7. 验证

- Dozer:测试总数守恒;`conversations.rs` 与 `transcript.rs` 里相关测试随代码搬走、不改期望值;**审阅数据黄金测试**(一组覆盖折叠规则的 `TurnRecord` → `ReviewEntry` → 序列化 JSON,字节与改造前一致,在改任何代码之前生成);**协议编码黄金测试**(`GetConversationTurns`、`SubmitSummaryJob`、`GetSummaryJob` 及应答在加服务接缝前后一致);`serve_review_trace` 对 `host.html` / `.js` / `.css` / 注入数据的应答与改造前一致;CSP 严格与产物齐全两个测试;依赖闭包无禁用 crate。
- 用户在 Dozer 上目测:列表搜索、筛选、分页、卡片文案、点行进详情、思考/工具调用折叠、加载更多、生成总结、折叠列表、空态与出错态,与改造前一致。
- Digger 单测:历史导入的三态(未导入 / 导入中 / 已导入)与幂等;失败时按钮可重试;列表按项目过滤;能力开关关闭时没有「生成总结」与「关联待办」。
- Digger 真机(沙箱数据根;点击由人做):装好 hook 后在 Digger 里跑一次 Agent 会话,回合结束列表自动出现该会话;点开能看到时间线;「导入历史」按钮的前后对比;与 Dozer 同机并存,各自只看到自己库里的会话。

## 8. 我的决定与未解问题

我的决定(可以推翻):

- D1 审阅详情在 Digger 里用同一份 review-trace 页面 + 原生 webview,不用 iced 重写(重写会有第二份实现且外观必然不同)。
- D2 `get_conversation_turns` 进 `ConversationService`,总结任务两个方法进新的 `SummaryService`;不为会话面板另起服务 crate。
- D3 搜索框焦点探测、原生菜单留在 `dozer-app` 适配层;库用请求发件箱表达「要弹菜单」。
- D4 库名 `dozer-panel-conversations`;面板 id `digger.conversations`。
- D5 `ReviewSource::Session` 已核实为死代码(全仓库没有任何地方创建 `Session` 来源的 `ReviewView`,只有读它的分支),S7a 删除它和仅为它存在的 `spawn_review_load` 路径。
- D6 审阅 webview 不并入预览引擎。
- D7 webview 自定义协议的 scheme 由产品传入(Digger 用 `digger`),库的 URL 构造函数带 `scheme` 参数。

未解:

- Q1 历史导入:我倾向「按钮触发、说明清楚」(与 S5a Q2 一致,不静默复制用户的历史),代价是新用户首次要点一下。Dozer 是打开项目时静默补录。是否接受?
- Q2 「生成总结」:Digger v1 隐藏,标题回落到首条发言/会话自带标题。若要开通,需要打开 `diggerd` 的总结功能,它会调用外部 LLM/Agent 命令行并产生费用(S6 的 Q1 当时的担忧)。我倾向 v1 不开,单独再决定。是否接受?
- Q3 右栏顺序与默认选中:`[agent, conversations, apps, usage]`,默认选中仍是 Agent。是否接受?

## 9. 留给实现者决定

库内 `Message` 的精确清单(尤其是现在被内核拦截的 `SessionOpen` / `DetailLoadMore` / `SummaryGenerate*` 如何落进库)、`ConversationsCapabilities` 与 `LinkedTodos` 的最终形状、`cwd` 过滤的边界规则(父子目录、符号链接、大小写)与相应测试、导入标记在 `digger.db` 里的表结构、审阅 webview 的摆位度量、`ReviewWebviewSpec` 的字段。
