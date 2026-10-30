# 共享设置模块：Dozer 抽取到 bytehost，Digger 接入

- 日期：2026-10-29
- 状态：设计草案（已实施共享 crate、Dozer 回接与 Digger 首批 Theme / Apps；发布与实机验收未完成，见[验收报告](../reviews/2026-10-29-shared-settings-acceptance.md)）
- 配套计划：[Implementation Plan](../plans/2026-10-29-shared-settings.md)
- 范围：bytehost、Dozer、Digger 三个仓库；本文档保存在 Digger。

## 1. 目标与交付边界

把 Dozer 的设置模块拆为 bytehost 提供的共享设置框架、可复用页面，以及各产品自己的适配与扩展。Dozer 改用共享实现，Digger 获得设置入口并复用同一套页面。以后增加产品专属设置时，无需修改 bytehost 的页面枚举，也无需复制整个设置窗口。

本期必须交付：

1. bytehost 中可独立消费的设置状态、页面注册协议和 iced 视图。
2. 共用主题页、语言页的交互状态和视图、应用管理页的视图；应用管理继续使用已有 `bytehost-panel::install` 状态机。
3. Dozer 五个页面全部接回，保留现有行为、配置与凭据位置。
4. Digger 设置窗口、主题页和应用管理页；应用操作使用 Digger 现有的审批、执行器、请求台账与数据根。
5. 两个产品的扩展机制验证，包括完整产品自定义页和内置页的附加区块。

语言页是可选共享页面。Digger 当前没有完整的运行时语言切换能力，本期不注册语言页；其后只有完成文案、状态和 webview 的一致更新，才启用该页。Digger 的 Git 账户管理和守护进程控制也不在本期新增功能内。此边界不影响 Dozer 现有五页的迁移。

## 2. 现状与依据

以下以本地源码为依据：Dozer `08b63c99`、bytehost `21ceaa9`、Digger `192105d`，读取日期为 2026-10-29。工作区可能含本地修改；这些 revision 是定位基线，不代替实施前的工作区核对。历史 spec 中的“待审阅”不能作为当前功能未实现的依据。

| 位置 | 当前职责 / 约束 |
|---|---|
| `dozer/crates/dozer-app/src/extensions/settings.rs` | `SettingsTab` 固定列出 Theme / Git / Advanced / Apps / Language；同时持有状态、异步调度、视图、提示和宿主刷新标记 |
| `dozer/.../extensions/settings_apps.rs` | 已重导出 `bytehost_panel::install`，说明应用安装状态机已共享 |
| `dozer/.../extensions/settings_apps_view.rs` | 应用管理 iced 视图仍在产品内 |
| `dozer/.../platform/settings_overlay.rs` | 独立原生窗口；Esc / 显式关闭；失焦不关闭；支持 IME 与原生编辑菜单 |
| `dozer/.../app/update.rs` | 负责主题刷新、语言权威快照与广播、关窗后的应用结果处理、数据清理意图、daemon 状态 |
| `dozer/.../git_accounts.rs` | 本地账户元数据、网络校验、系统钥匙串；现有命名空间 `dozer-git` |
| `bytehost/crates/bytehost-panel` | 无 UI 框架的应用管理状态机 |
| `bytehost/crates/bytehost-desktop` | 允许 iced / wry；已有窗口、浮层和渲染胶水；`overlay` feature 默认关闭，须保持渲染器兼容 |
| `digger/crates/desktop/src/product.rs` | 产品组合根，无设置模块；主题与多个语言参数仍在产品内处理 |
| `digger/crates/desktop/src/apps/` | 已有应用管理、审批、日志、运行时与执行链，不能再创建第二套安装状态 |
| `digger/crates/paths/src/lib.rs` | Digger 配置、daemon、应用宿主目录与 Dozer 分离 |

Digger 的声明依赖为 bytehost `v0.4.17`、byteui `v0.5.0`、Dozer 库 `libs-2026-10-28g`。本地 `.cargo/config.toml` 有开发 patch；发行验收必须另验证无本地 patch 的依赖图。

## 3. 共享与产品所有权

| 能力 | bytehost 所有 | 产品所有 |
|---|---|---|
| 设置窗口内容 | 标题、导航、关闭按钮、内容区、滚动与空状态 | 打开入口、窗口生命周期、与主窗口的消息映射 |
| 页面注册 | 稳定 ID、排序、重复校验、默认页与路由回退 | 注册哪些页、顺序、标题解析、能力可用性 |
| 主题 | 选项视图与选择事件 | 当前主题、偏好读写、主题应用、所有视图刷新 |
| 语言 | confirmed / pending / saving / failed / conflict / unsupported 状态与选项视图 | 偏好模型适配、系统语言探测、revision、服务调用、词表与广播 |
| 应用管理 | 共用管理视图；复用 `InstallFlowState`、消息和 effect | 状态实例、API、执行器、审批策略、文件选择、刷新与数据清理 |
| Git 账户 | 通用页面布局 / 状态行可供复用 | Dozer 专属页面、PAT、校验、Keychain、取消任务与本地元数据 |
| 高级设置 | 通用确认 / 忙碌 / 错误展示组件 | Dozer 的 dozerd 控制；未来 Digger 的 diggerd 或编辑器设置 |
| 扩展 | 自定义页插槽、内置页附加区块插槽 | 扩展状态、消息、服务、持久化、业务校验 |

“共用”指代码和交互规范共用。配置文件、主题选择、语言偏好、账户、端口、运行时、应用数据均按产品独立，不自动同步。

## 4. 包结构与依赖方向

新增两个 crate，避免把纯设置状态与 GUI 依赖混在一起：

```text
Dozer settings adapter ─┬─> bytehost-settings-ui ─> bytehost-settings
Digger settings adapter┘           │
                                   ├─> bytehost-panel / bytehost-apps（应用管理展示类型）
                                   └─> byteui / iced 0.14

产品窗口适配 ─> bytehost-desktop（已有浮层 / 窗口机制）
产品服务适配 ─> 各自 daemon / AppHostApi / 路径 / 凭据库
```

- `bytehost-settings`：页面标识、注册表、窗口导航状态、通用选择保存状态。仅标准库及必要的 serde；不依赖 iced、wry、byteui 或任何 `dozer-*` / `digger-*`。
- `bytehost-settings-ui`：设置容器、主题 / 语言 / 应用页面及通用区块组件。可依赖 byteui、iced 和 bytehost 应用类型；不依赖产品 crate，不创建窗口、不访问路径、不启动任务、不调用服务。
- 不改 `bytehost-core` / `bytehost-panel` 的现有职责，不给 `HostMessage` 增加任何产品设置变体。
- 共享 UI 不开启额外渲染器后端；两款产品的 `Element` / Renderer 类型必须一致。只有窗口适配确实需要时，才开启 `bytehost-desktop/overlay`。
- bytehost 到 byteui 的依赖沿用消费者当前兼容版本并统一 pin；共享 UI 不能反向依赖 desktop，避免环。
- Digger 业务 crate 仍不得依赖 `bytehost-*` 或 UI。新增设置代码放在 desktop 适配层。

## 5. 注册与扩展协议

以下是拟定的接口语义，具体 Rust 签名在计划 Task 1 的最小消费示例中编译冻结。

### 5.1 页面身份与路由

`PageId` 是经过校验的稳定字符串，不使用显示标题作标识。内置 ID：`bytehost.theme`、`bytehost.language`、`bytehost.apps`；产品 ID：例如 `dozer.git`、`dozer.advanced`、未来的 `digger.editor`。`bytehost.*` 保留给共享模块。

`PageDescriptor` 包含 `id`、`kind`（内置或自定义）和文本 key / 英文 fallback；注册顺序就是导航顺序。`PageRegistry::new(pages)` 校验空 ID、重复 ID、保留命名空间误用；错误返回结构化结果，不静默覆盖。两个产品可以使用不同的顺序。

`SettingsState` 只保存打开状态、当前页、窗口代数和容器交互态。`open(target)`：目标有效时选中目标，否则选主题页，再否则选首个注册页；空列表展示明确空状态。重复打开激活现有窗口；切页不重建业务状态。注册集合更新时，选中页消失则按相同规则回退。

不把 `SettingsTab::{DozerGit,DiggerEditor,...}` 写进 bytehost。应用面板的“去设置”入口统一路由 `bytehost.apps`，产品菜单可以路由自己的 `PageId`。

### 5.2 组合方式

共享容器消费有序导航数据、当前页内容 `Element<M>`、标题和关闭 / 切页回调。产品先把共享页面事件与自己的扩展消息 `.map(...)` 到产品消息 `M`，再传入容器；共享层无需 `Any`、JSON 消息或产品 trait object。

产品用自己的 `match PageId` 渲染页面。内置主题页和语言页接收只读模型，返回类型化页面事件；应用页接收借用的 `InstallFlowState`，返回原有 `install::Message`。未知路由回退由注册层统一完成。

每个内置页提供尾部附加区块 `Vec<Element<M>>`；产品可增加说明或产品专属选项，但不能替换共用确认流程、拦截审批或覆盖内置 ID。附加区块的状态和持久化完全由产品拥有。已有页面差异用只读显示参数表达；不得不断增加 `if product == "dozer"` 分支。

新增一页仅需：定义产品状态 / 消息 / 视图，注册带命名空间的 ID，完成产品消息路由。新增内置页区块仅需构造映射后的 `Element<M>`。两种扩展都须有能运行的示例，证明不修改共享库即可完成。

## 6. 页面行为

### 6.1 主题

首批仍为 byteui 已有 Dark / Light，不新增自动主题。共享页从参数读取当前主题，输出选择事件；不能直接调用 `set_scheme` 或自行找 `color_theme_path`。

Dozer 保留原配置位置和现有即时应用行为。Digger 新增 `<digger config>/color-theme.json`，采用 byteui 现有主题文件格式；文件不存在沿用当前启动默认主题，无法解析则回退并提示。实施前确认 Dozer 现有读取 / 写入 API，复用同一序列化逻辑。

产品处理选择后同步主窗口、设置窗口和主题敏感的预览 / 编辑器 / 图表。后台 webview 保存最新主题，恢复时更新。保存失败必须可见且允许重试，不能显示“已保存”；已经即时生效的主题可保留至当前进程结束。两款产品的默认主题保持各自当前值。

### 6.2 语言

共用泛型选择状态 `SelectionState<T>`，保留已确认值、待保存值和保存结果。Dozer 将 `LocalePreference` 与权威 `LocaleSnapshot` 映射进该模型，revision 与冲突判断继续在产品服务层。

保存期间禁用重复提交；成功更新 confirmed 并清 pending；失败保留待选值供重试；冲突采纳服务端值并提示重选；unsupported 禁止写入。Dozer 的权威快照更新与广播即使窗口已关闭仍要完成。

共享 UI 文案使用 `bytehost.settings` 文本空间及英文 fallback，通过注入的文本解析函数显示；不导入 `dozer-i18n` 或 `dozer-model`。应用管理中已有 bytehost 文案接缝沿用原协议，避免再复制一份词表。Dozer 专属页面继续使用原 owner。

Digger 本期不注册此页，维持当前语言行为，不把历史 i18n plan 当作已落地能力。将来启用需先覆盖产品文案、所有共享面板、已有及新建 webview、应用标题与重连后的快照，并单独补充实施计划。

### 6.3 应用管理

从 Dozer 的 `settings_apps_view.rs` 抽取 UI，移除产品文本、样式 helper、文件选择和宿主访问。样式组合使用 byteui 或共享 UI 内部 helper。安装计划、权限说明、确认、运行时、列表、日志、错误状态全部保留。

共享 UI 借用产品已有的安装状态，不持第二份。Dozer 继续使用现有 AppHostApi 服务适配，Digger 继续使用 `AppsState` / `digger-app-host::Executor`。不得在共享层复制 AppHostApi 或迁移产品的信任策略。

Digger 目前应用管理面板与新设置页共用同一状态：本期保留面板入口，管理内容改为同一共享页视图。两个入口的选择、审批与日志状态一致；打开设置不能再次初始化 `InstallFlowState`、重复订阅或重放操作。原面板的纯启动 / 展示内容可保留产品布局。

确认后的安装 / 卸载等持久任务可以在关窗后完成，仍更新列表、通知和数据清理。仅“准备计划”返回时窗口已关闭的，不得自动批准或安装。`ProgramAndData` 的清理意图放在产品运行态，成功后才调用现有清理链；失败不清理。

## 7. 产品扩展与兼容

Dozer 首批顺序保持 Theme / Git / Advanced / Apps / Language；默认 Theme。Git 与 Advanced 改为产品扩展页，代码可拆至 `settings/git.rs`、`settings/advanced.rs`，原入口可短期保留为薄适配。

- Git：保持原账户 JSON、`dozer-git` Keychain 命名空间与三家 provider；保留取消时真正中止任务、丢弃过时结果，以及断开时先写元数据再删 token 的顺序。PAT 不进入共享状态、通用日志或设置持久化。
- Advanced：继续用 `DaemonService`，确认时只统计 alive 会话；停止 / 重启更新产品的 daemon 状态；重开窗口按真实可达性恢复按钮。bytehost 不识别 `dozerd` 或 `diggerd`。
- Digger：首批 Theme / Apps；不注册无服务支撑的空页。扩展协议通过测试夹具中的 `digger.editor` 和主题页附加区块验证，不为证明框架而新增未经要求的编辑器功能。

Dozer 原数据无需搬迁或全量重写。Digger 只新增自己的主题偏好文件。窗口导航不要求跨重启持久化；重开默认主题页或显式目标页，业务操作状态仍由产品管理。

## 8. 窗口与异步生命周期

窗口内容共享，原生窗口驱动由产品适配。Dozer 继续复用 bytehost 已有 overlay 胶水；Digger 在 iced 多窗口机制下创建独立设置窗口并通过 window ID 路由，不能简单在 wry 上方画一个 iced 内嵌卡片。Task 1 先验证原生层级；若现有 iced 多窗口不能保持层级，使用已有 bytehost overlay 机制完成同一验收，避免新写 GPU 管线。

行为统一：单实例、Esc / 关闭按钮 / 系统关闭关闭设置、失焦保持打开、再次打开激活、输入框支持 IME 和标准粘贴、主窗口退出销毁设置窗口。内容可滚动，小窗口与高 DPI 下关闭按钮始终可达。Dozer 保留现有卡片尺寸策略，容器允许产品传入尺寸约束。

产品每次打开递增 `window_generation`。窗口局部校验 / 草稿异步任务携带窗口代数与请求 ID；过期结果不能更新新窗口。需要取消的任务实际取消。已提交持久操作由产品运行态追踪并处理结果，不能因代数过期丢掉服务端事实。切页不等于关窗，不取消已提交操作；草稿与验证错误在本次窗口内保留。

## 9. 验收标准

| 编号 | 可验证结果 |
|---|---|
| A1 | 两个新增 crate 的 normal / build 依赖闭包无任何 `dozer-*` / `digger-*`；纯状态 crate 无 UI 依赖 |
| A2 | 产品自定义页与内置附加区块示例可编译运行；不用修改共享 enum；重复 / 非法 ID 被拒绝，未知路由有回退 |
| A3 | Dozer 五页保留既有关键行为、配置位置、PAT 取消、daemon 确认、语言冲突与重连语义 |
| A4 | Digger 可从设置入口打开主题 / 应用页，并从应用入口直达应用页；不出现未实现的语言 / Git / 高级页 |
| A5 | 主题立即同步主窗、设置窗及相关 webview，重启恢复；保存失败可见；两产品互不改写偏好 |
| A6 | 应用管理只有一个状态实例；审批策略、结果追踪、关窗后刷新、成功后数据清理全部保留 |
| A7 | 原生窗口层级、单实例、Esc、失焦、IME / 粘贴、高 DPI、退出清理实机通过；过时回包不污染重开窗口 |
| A8 | 共享依赖统一 tag / rev，无双份 byteui / bytehost 核心类型；无 patch 环境可构建，两产品相关回归通过 |

## 10. 不做与后续

不新增动态插件加载、声明式 JSON 表单引擎、跨产品配置同步、Project 级设置系统或新 daemon 协议。不把 Dozer 的所有设置服务搬入 bytehost，也不抽取 Git 账户后端。后续某个能力出现第二个消费者时，再按实际需求抽取服务中立的页面与契约。

## 11. 风险与处理

- 应用管理视图包含产品 helper：逐项改为参数或共享 UI helper，保留原安装流程测试，禁止复制执行器凑编译。
- iced / Renderer feature 合并可能改变渲染类型：先编译两个最小消费例，再迁移真实页面；默认不开 overlay 后端。
- Digger 的应用执行链比纯 UI 更长：设置页只映射既有消息，保留用量记录、请求台账、信任与清理测试。
- 关窗时有异步回包：区分窗口草稿与产品持久操作；分别验证“不能污染新窗口”和“结果不能丢”。
- 多仓库版本相互约束：先完成本地联调，再发布 bytehost、更新 Dozer 库快照、统一更新 Digger；保留前一组 pin 供整组回滚。
