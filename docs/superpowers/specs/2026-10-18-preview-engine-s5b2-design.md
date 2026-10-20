# S5b-2 预览引擎:从 Dozer 抽成共享库,Digger 接入(第一批:代码、Markdown/HTML/图片)

- 日期:2026-10-18
- 状态:P3 已交付库侧与 Digger 宿主(tag `libs-2026-10-19e`);P3 遗留项与验收记录见 `2026-10-19-preview-engine-p3-host.md` 末尾「执行记录」
- 路径:架构级,**分四期交付,每期单独写实施计划**
- 相关:`2026-10-18-files-tree-s5b1-design.md`(S5b-1,树里的 `PanelCommand::OpenFile`)、`2026-10-18-project-panel-s5a-design.md`(S5a)、ADR-012 / ADR-013(章节编辑器)、ADR-011(bytehost 宿主)、Dozer `2026-10-15-panel-todo-lib-design.md`(S3a)

## 1. 用户意图

Digger 要重用 Dozer 的面板。Dozer 的 Files 面板与 Project 面板的右半边,是**同一套预览引擎的两个实例**(`dozer-app/src/preview/`,约 1.75 万行,外加 `assets/` 里的一批 web 宿主)。S5b-1 把文件树做完之后,文件树里的"打开文件"在 Digger 里先交给系统默认应用;本轮要让它真正在 Digger 里预览。

用户在 2026-10-18 的决定:

1. 文件树里的文件(包括 `.md`)在 Digger 里用**预览引擎**打开;**章节编辑器(Markdown 面板)保持独立**,不承担"打开磁盘上任意文件"的职责。两者产品定位不同:章节编辑器编辑的是 Creator 领域的文档(Markdown 文本是权威内容、Rust 维护块 ID);预览引擎是通用文件查看器。
2. 预览引擎分期,**第一批类型**:代码/纯文本(含编辑与保存)、Markdown / HTML / 图片的渲染。JSON、JSON Lines、表格、PlantUML、PDF / 媒体、图片标注放在后面几批,不在本文范围。
3. 分期顺序按我提出的四期。

## 2. 引擎的现状(读代码得到)

四层叠在一起:

1. **纯逻辑核心**:`router`(唯一的"该用哪个后端"决策点)、`file_profile`(内容类型、编码)、`file_policy`、`trust`、`loading`、`large_text`、`text_save`、`recovery`、`startup`。基本不碰界面。
2. **预览面板状态与 iced 界面**:`PreviewPane`(页签、当前页签、查找会话、一次性聚焦标记、能力快照)与 `view.rs`(约 7500 行:页签条、工具栏、查找条、原生视图)。
3. **webview 宿主与协议**:代码走 CodeMirror(`dozer://editor/`,绑定 `(project_id, panel, tab_id, path)`,**JS 不能自报路径,所有读写由 Rust 侧按绑定校验**);渲染型走 flyfish(`dozer://flyfish/`,文件端点只服务用户显式打开过的路径白名单);另有 JSON / 表格 / PlantUML / 图片标注各自的宿主。`assets.rs` 与 `webview_protocol.rs`(约 2000 行)是协议应答。
4. **平台层**:真正的 `wry::WebView` 由 `dozer-app` 的平台池持有(`main.rs` 每帧按"期望清单 `WebviewSpec`"同步),摆放位置由 `webview_geometry.rs` 按配对列宽公式算出,放大态要用零尺寸矩形把原生 webview 藏起来(webview 不听 iced 的绘制顺序)。

对 `dozer-app` 内核的耦合比文件树重:`crate::app` 约 84 处(`PanelKind`、外壳状态、每帧聚焦操作)、`crate::capabilities` 19 处、`crate::tabular` 20 处。

Digger 一侧已有的:`MarkdownPanel` 自己用 wry 建原生子 webview(`markdown/host.rs`,带 nonce 的 `window.__DIGGER__`),`sync_native` 每帧按矩形同步;bytehost 的 `build_child` 不提供 IPC 与初始化脚本,所以这一套是 Digger 自己写的(ADR-013)。

## 3. 目标

- G1 把预览引擎的**第一批后端**抽成 Dozer 里的共享库,Dozer 改为依赖它,**用户可见行为不变**。
- G2 Digger 依赖同一份库,在 Project 和 Files 面板的右半边显示预览;文件树的 `OpenFile` 改映射到预览(取代 S5b-1 里的"系统默认应用打开")。
- G3 Digger 里第一批可用:代码/纯文本的查看与编辑与保存;Markdown、HTML、图片(含 SVG)的渲染;多页签;文件内查找;未知二进制与不支持的类型给出可解释的降级,并保留"用系统应用打开"。
- G4 引擎不直接持有 `wry` 句柄:库只产出"期望的 webview 清单"和处理来自 webview 的消息,由产品用自己的平台代码创建/摆放(见 §5.2)。

## 4. 非目标与反目标

非目标:JSON / JSON Lines / 表格 / PlantUML / PDF / 媒体 / 图片标注宿主;大文件之外的预览资源预算调优;章节编辑器的任何改动;文件历史、回滚、搜索弹窗(S5b-1 已排除)。

反目标(技术上做到了也算失败):
- Dozer 预览的外观或行为有任何变化。
- 库里出现 `dozer-app`、`dozer-client`、`dozer-core` 的依赖,或 Digger 里复制一份预览实现。
- webview 里的页面能读写它没被绑定的路径,或读到用户没显式打开过的文件。
- 把磁盘上任意 `.md` 文件塞进章节编辑器。

## 5. 设计

### 5.1 库的切分

分两个 crate,与四期对应:

- `dozer-preview-core`(P1):第 1 层的纯逻辑。依赖只含 `serde*`、`tokio`、`dozer-i18n`、标准库与少量编码探测库;**不含 iced、不含 wry、不含 `dozer-app`**。
- `dozer-panel-preview`(P2、P3、P4):第 2、3 层(面板状态、iced 界面、宿主页与协议应答的纯函数部分、web 资源 `assets/`),依赖 `dozer-preview-core`、byteui、iced、`dozer-panel-host`;**不含 `wry`**。

### 5.2 webview 的端口

库里的状态机不创建 webview,只对外产出与接收:

- `WebviewSpec { id, url, visible, bounds, kind }`:当前期望存在的 webview 清单(沿用 Dozer 现有的 `WebviewSpec`);
- `WebviewEvent`:来自页面的消息(已按绑定校验过的类型,不是原始 JSON);
- `ProtocolHandler::reply(&self, uri: &str) -> ProtocolReply`:`dozer://` 协议的应答,纯函数,持有"显式打开过的路径白名单"。

产品负责:按清单创建/销毁/摆放 `wry` webview、把 `ProtocolHandler` 接到 wry 的自定义协议、把页面消息转成 `WebviewEvent` 喂回库。Dozer 的平台池本来就是这么做的,抽库只是把边界画清楚;Digger 在 `markdown/host.rs` 旁边新增自己的预览宿主,**与章节编辑器的 webview 互不干扰**(不同的 scheme 或不同的 nonce 空间,具体由实现者定,见 §8)。

### 5.3 路由与第一批后端

`router::classify_preview` 整体搬进 P1,**路由表不删减**(其余类型的后端在库里暂未启用,但路由仍然产出 `PreviewKind::Json` 等,产品对"本产品尚未启用的类型"统一走降级)。引入 `PreviewCapabilities { code: bool, rendered: bool, json: bool, streamed: bool, tabular: bool, plantuml: bool, ... }`,由产品注入;Dozer 全开,Digger 第一批只开 `code`、`rendered`(其中 PDF/媒体子类型关闭)。未启用类型的页签显示"本版本暂不支持预览,可用系统应用打开"(沿用现有 `Unsupported` / `External` 的降级界面)。

### 5.4 点文件与配对

- S5b-1 的 `PanelCommand::OpenFile { path }`:Digger 的产品层改映射为"在当前面板的预览里打开该路径"(取代 `open <path>`);系统应用打开保留为预览工具栏与右键里的显式动作。
- Project 和 Files 在 Digger 里从单栏变成**配对视图**(左:信息/文件树;右:预览),沿用 S1 的 `PaneRules` / `pair_view`,两个面板各自一份 `PreviewPane`(与 Dozer 一致)。`shell::rules` 随之返回 `Some(..)`;**这会改变 S5a、S5b-1 里"单栏"的布局约定,P4 要同步改它们的测试**。

### 5.5 安全

沿用 Dozer 的两条:① webview 与 `(project_id, panel, tab_id, path)` 绑定,路径来自 Rust 不来自 JS;② 文件字节端点只服务"用户显式打开过的路径"白名单。保存时校验目标路径仍等于绑定路径,且在项目目录内。

## 6. 分期

- **P1 纯逻辑核心**(只动 Dozer):建 `dozer-preview-core`,搬第 1 层,Dozer 改引用,测试守恒。
- **P2 面板状态与 iced 界面**:建 `dozer-panel-preview`,搬 `PreviewPane` 与页签条、工具栏、查找条、`CodeView` 等原生部分;Dozer 改引用;Digger 接入但先不挂宿主 webview(此时 Digger 能显示页签与降级页)。
- **P3 webview 宿主(第一批)**:搬代码 / 渲染型两类宿主的清单、协议应答、web 资源;Digger 写自己的预览宿主;能在 Digger 里真正看到代码与 Markdown/HTML/图片。
  - **实现进度(2026-10-19):** 协议应答、web 资源、注入脚本、IPC 分类、`desired_webviews` 黄金均已进库并发布 `libs-2026-10-19e`;Dozer 改为薄调用,行为不变。Digger 的 `adapters/preview_native` + `examples/preview_smoke` 在真机上让 Markdown/HTML/源码三类 host 全部 `Ready`。**D2 收窄落地:** P3 只在示例里真机可见,不进应用主界面(与 P2 的 D2 同理)。
  - **P3 未完成项:** 搬迁前的协议/注入脚本黄金、页面事件作用到 `PreviewPane` 的库侧迁移(仍留 `dozer-app`)、Digger 打包态资源交付、示例的 `--selftest` 与逐项目测验收。均记录在 P3 计划末尾,不影响 D2 收窄后的本期目标。
- **P4 配对与点文件**:Project / Files 改成配对视图,`OpenFile` 改映射到预览;更新 S5a、S5b-1 里与单栏相关的测试;真机回归与文档。

每期都是"Dozer 抽库 → 目测验收 → 发 tag → Digger 接入"的节奏,Dozer 每步编译且测试全绿、测试总数守恒。每期单独写实施计划。

## 7. 验证

- Dozer:抽库前后 Rust 与前端测试总数守恒;依赖闭包里没有 `dozer-app` / `dozer-client` / `dozer-core`,P1 的闭包里还没有 iced 与 wry;用户目测验收(代码编辑保存、Markdown/HTML/图片渲染、多页签、查找)。
- 安全:新增钉死测试——协议应答对"未显式打开过的路径"返回 404、保存到绑定路径之外被拒绝、页面消息里带别的路径被丢弃。
- Digger 真机(只能截图,点击要人来做):树里双击 `.py` / `.rs` 在右半边打开并可编辑保存;`.md` 渲染;`.html`、`.png`、`.svg` 渲染;`.json` / `.csv` / `.pdf` 显示降级并能用系统应用打开;切换项目页签时预览随之切换;放大一侧时原生 webview 被正确藏起;章节编辑器不受影响。
- `Cargo.lock` 每个共享库一份;`check-shared-deps.sh`、`check-host-deps.sh`、`clippy`、`fmt --check` 通过。

## 8. 我的决定与未解问题

我的决定(可以推翻):

- D1 分两个 crate(`dozer-preview-core` 与 `dozer-panel-preview`),库里不依赖 `wry`。
- D2 webview 端口用"期望清单 + 事件 + 协议应答"三件套,不引入"创建 webview"的 trait。
- D3 路由表整体搬走不删减,用 `PreviewCapabilities` 在产品层关闭未启用的类型。
- D4 Project 与 Files 的右半边各一份 `PreviewPane`,与 Dozer 一致。**Digger 一期:Project 面板的页签不落盘,切项目即丢弃(回来为空);只有 Files 面板的页签恢复。** Dozer 按项目常驻 workspace,Digger 不做(G-I3)。
- D5 P4 改配对时同步修改 S5a、S5b-1 的单栏约定与测试。

未解:

- Q1 Digger 的预览宿主与章节编辑器共用同一个 `wry` 子 webview 管理代码,还是各写各的?我倾向各写各的、互不干扰(因为章节编辑器的块同步协议很特殊),代价是有一点重复的 wry 胶水。请你在审阅时看这一条。
- Q2 `dozer://` scheme 在 Digger 里是否沿用?沿用的好处是库与 web 资源不用改;风险是与 Dozer 同名但不同进程,无冲突。我倾向沿用,由实现者最终确认 wry 在 macOS 上对自定义 scheme 的同源行为。
- Q3 `ClientCapabilities`(预览资源预算,Dozer 启动时探测一次,引擎里 19 处引用):Digger 取一份固定的保守默认值,还是也做探测?我倾向固定默认值,探测留给以后。

## 9. 留给实现者决定

`dozer-preview-core` 与 `dozer-panel-preview` 之间具体哪些类型落在哪边、`PreviewPane` 里每个字段的归属、`WebviewEvent` 的具体枚举、`ProtocolHandler` 的构造形式、Digger 预览宿主的文件组织、各期内部的任务切分。
