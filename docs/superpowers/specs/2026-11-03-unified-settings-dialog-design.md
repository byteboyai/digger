# 统一设置模块与对话框宿主

- 日期：2026-11-03
- 状态（2026-11-04）：共享实现与审核修复已随 bytehost `v0.4.23`、Dozer `libs-2026-11-04a` 发布；自动发行验证见[发布记录](../reviews/2026-11-04-shared-settings-release.md)。实机与整体规格验收仍未完成。
- 配套计划：[Implementation Plan](../plans/2026-11-03-unified-settings-dialog.md)
- 前置设计：[共享设置模块](2026-10-29-shared-settings-design.md)。本文扩展其窗口宿主与 Git 账户边界；未变更的页面注册、应用执行与产品数据隔离约束继续有效。
- 范围：bytehost、Dozer、Digger；必要时修改 byteui 的公共主题 token。本次共享发布由用户于 2026-11-04 明确授权。

## 1. 问题与目标

两款产品已消费 `bytehost-settings` 与 `bytehost-settings-ui`，但设置内容共享没有带来完整的宿主共享。Dozer 用覆盖主窗口区域的原生子窗口显示遮罩与居中卡片，自行连接绘制和输入；Digger 用 iced 多窗口运行时打开普通设置窗口。Git 账户连接流程也仍留在 Dozer 内。新项目尚不能通过统一入口获得同样的设置体验。

本次交付应使两款产品使用同一套设置内容契约、对话框生命周期、几何与输入策略，并共享 Git 账户页面和连接逻辑。默认呈现以用户提供的 Dozer 截图为参照：覆盖所属主窗口的遮罩、居中设置卡片、左侧导航、右侧页面、右上关闭和底部关闭；保留产品主题和已注册页面差异。

统一不能仅表现为外观相似或两个产品都调用 `settings_shell`。对话框的打开 / 重开 / 创建失败 / 关闭、主窗口关系、焦点、IME、原生 webview 层级与后台结果处理必须通过同一行为契约和共享测试。

## 2. 已核对的现状

2026-11-03 源码基线：Digger `e82c048`、Dozer `b701abeb`、bytehost `729fcaf`、byteui `e338245`。实际实施前重新记录 HEAD、dirty 文件与依赖图；本文中的版本不是实施时可跳过核对的依据。

| 位置 | 当前行为 |
|---|---|
| `bytehost/crates/bytehost-settings` | PageId / PageRegistry、SettingsState、SelectionState；不依赖 GUI |
| `bytehost/crates/bytehost-settings-ui` | settings_shell、主题 / 语言 / 应用视图与文案解析；不创建窗口、不读写文件、不调用服务 |
| `bytehost/crates/bytehost-desktop/src/overlay.rs` | 原生子窗口、几何 / 重定位等已有共享机制 |
| `bytehost/.../overlay_gpu.rs` | 可选 overlay feature 提供 OverlayGpu、遮罩卡片、IME / 光标辅助；默认不强制打开渲染器 |
| `dozer/.../extensions/settings.rs` | 注册 Theme / Git / Advanced / Apps / Language；Git、Advanced 为产品自定义页；已使用共享 shell |
| `dozer/.../platform/settings_overlay.rs` | SettingsOverlay 手动连接 winit 事件、UserInterface、GPU、IME、尺寸 / 位置 |
| `dozer/.../platform/overlay_window.rs` | 对 bytehost overlay / overlay_gpu 的薄封装，注入 Dozer 遮罩主题色；不是从零自写所有底层机制 |
| `dozer/.../git_accounts.rs` | provider、账户元数据、PAT 校验、Keychain、远端仓库查询；Keychain service 为 `dozer-git` |
| `digger/crates/desktop/src/settings/` | 仅注册 Theme / Apps；独立普通窗口默认 720×640；主题和路径归产品 |
| `digger/.../product.rs` | iced::window::open / close / gain_focus、多窗口消息路由、设置视图与应用服务接线 |

当前声明组合为 bytehost `v0.4.22`、byteui `v0.5.2`、bytegit `v0.9.2`、Dozer 库 `libs-2026-11-03a`。实施必须验证实际已发布的依赖闭包，不沿用本地 patch 的成功作为发布证据。

## 3. 范围与完成边界

本期包含：

1. bytehost 提供通用的父窗口所属对话框控制器、统一遮罩 / 卡片视图与两种运行时驱动。
2. Dozer 和 Digger 的设置迁到该宿主；两边产品代码只保留业务事件、窗口运行时调用和页面装配适配。
3. 抽取 Git 账户的公共状态、视图、provider 行为与服务实现；Dozer 回接，Digger 接入可实际连接的 Git 账户页。
4. 给新产品提供可编译、可运行的最小设置示例，覆盖 Theme / Git 与自定义页，服务可使用明确标注的 fake。
5. 自动契约、跨运行时实机验收、干净消费者构建、依赖发布及回退记录。

本期不把所有弹窗一次性迁移：新建项目、文件移动、Toast 等继续使用原有调用点。新宿主以这些需求为参考，但只用设置完成首个生产验证，不搭建动态插件或任意嵌套模态框框架。

页面集合由能力决定：Dozer 保留现有五页和顺序；Digger 为 Theme / Git / Apps。语言共享组件继续存在，但 Digger 尚无完整运行时语言能力，不显示占位语言页。Advanced 保持产品扩展，不能把 Dozer 的重启 dozerd 等操作直接接到 Digger。页面数不同不视为宿主不统一。

## 4. 所有权与依赖

| 层 | 负责 | 不负责 |
|---|---|---|
| byteui | 颜色、间距、边框、字体、交互组件；必要的 dialog token | 建窗、设置业务、凭据、服务任务 |
| bytehost-settings | 导航 / 注册、选择保存状态、Git 页公共状态与类型化意图 | iced / wry、网络、文件、Keychain |
| bytehost-settings-ui | 共享 shell 与主题 / 语言 / 应用 / Git 页的 Element | 建窗、调度、保存令牌、直接调用产品服务 |
| bytehost-desktop::dialog（新增模块或等价边界） | 对话框控制、几何、遮罩、输入规则、父子窗口绑定与驱动；复用已有 overlay / overlay_gpu | 产品页面枚举、账户或安装状态、产品路径 |
| bytehost-git-accounts（拟新增 crate） | provider / 账户 DTO、异步账户服务接口、通用校验 / 连接 / 断开实现及可注入凭据 / 元数据存储 | UI / wry、产品名、硬编码产品路径、产品 Git 操作调度 |
| 产品适配 | 注册页、提供服务和配置命名空间、主题 / 语言应用、安装执行与原生视图可见性接线、应用退出 | 复制共享生命周期、Git 状态机或设置页面 |

Git 领域 DTO / 服务接口放在 `bytehost-git-accounts`；纯设置状态可以依赖其无 GUI 的契约，但不能因此引入网络或平台实现。按 feature 或模块分离服务实现，默认契约消费不加载 Keychain / HTTP；`bytehost-settings` 对该 crate 必须关闭实现侧 default features，并由依赖门禁验证。Task 1 用最小消费者冻结具体拆法，保证 `bytehost-settings` 保持纯状态依赖闭包。

bytegit 继续负责本地 Git 能力。本期不将账户设置、HTTP provider API 或 Keychain 塞入 bytegit，也不产生第二套 Git 调度器。Dozer 既有远端仓库查询消费者须跟随公共账户 DTO / provider 后端迁移，不能保留另一份 Token 校验和存储实现。

## 5. 统一对话框宿主

### 5.1 呈现与几何

默认 `ParentOverlay`：原生辅助窗口覆盖所属主窗口客户区域，承载共享遮罩与居中卡片；不是主窗口内简单叠一个 iced 容器，也不是可脱离父窗自由拖动的普通设置窗口。

卡片采用同一套策略：目标宽度为父客户区 40%，目标高度 75%，正常空间下最小 560×520；宽大文档仍按父可用区域钳制，四周目标留白至少 16，空间不足时优先保证卡片和关闭操作在可见区域。小窗口 / 大字号下内容滚动，标题与关闭操作保持可达；不能让下限尺寸把按钮挤出父窗口。尺寸单位为逻辑像素，不能混用截图物理像素。

主窗口移动、缩放、跨显示器 / DPI 变化时共享几何策略重算。最小化、恢复、全屏 / Space 切换、主窗口退出的跟随行为需在实机证明。样式来自公共 token；两产品主题不同时颜色可不同，但同主题下几何、描边、导航和交互反馈必须一致。

### 5.2 生命周期与身份

共享控制器表达 Closed / Opening / Open / Closing / Failed，产生有限的类型化命令和事件；驱动负责执行运行时操作。接口名称可在可行性验证后冻结，行为不能删减。

- 每个父窗口同一时刻最多一个设置实例；重复打开只激活已有实例或切页。
- 每次新会话具有 owner + kind + session 身份；重试创建再附 attempt 身份。旧 Built / Closed / Page / Focus 回包不能复活已关闭或新会话。迟到的创建成功结果必须销毁其旧实例资源，不能仅丢弃事件而留下已创建窗口。
- `SettingsState.window_generation` 与新宿主身份必须收口为一个权威来源，不能在产品和宿主各维护一个互不关联的 generation。迁移兼容 API 要委托同一来源。
- 创建 / 绑定 / 渲染失败可见，解除模态输入限制，保留导航和非敏感业务状态，可显式重试；失败时不能留下透明挡鼠标的孤儿窗口。
- 关闭按钮、Esc、系统关闭和父窗口退出走统一关闭协议；先停敏感输入 / 解绑定 / 释放句柄，再恢复父窗口交互。重复关闭幂等。
- 父窗退出不得只依赖驱动最终回包，须有有界销毁与明确错误；对话框不能让整个应用永远等在 Closing。

### 5.3 输入、焦点和原生视图

- 点遮罩不关闭，也不触发后面的主窗口；切到其他应用不关闭设置。回到产品时维持正确父子层级。
- Esc 优先交给活动 IME 组合或页面内菜单 / 确认，随后才关闭设置；Return 按当前页面焦点触发，不默认提交整个窗口。共享策略需定义事件消费顺序。
- 打开前记录可恢复的父窗焦点；打开后进入卡片合理位置；Tab / Shift-Tab 留在模态内容内。关闭后目标仍有效才恢复；父窗口切换 / 销毁后不聚焦过时句柄。
- PAT 输入支持遮盖、粘贴、选择、原生编辑菜单与 IME；候选条随光标且缩放正确。所有渲染器驱动都要满足同一用例。
- 主窗内 wry 编辑器 / 预览 / 应用页面不能盖住遮罩、收键盘或吞点击。宿主发出输入阻断 / 可见性生命周期信号，产品仅把它接到已有原生视图管理器。关闭后按最新面板和项目状态重新同步，不能盲目恢复旧快照。
- 设置内部的安装批准、卸载确认与文件选择沿用既有执行链；页面内确认优先消费 Esc。系统文件选择返回后不自动关闭设置，不再创建第二套 AppHost 状态。

### 5.4 两种驱动与可行性门禁

Dozer 当前手工驱动 winit / iced 渲染，Digger 由 iced 多窗口运行时驱动。两者不必重写整个主事件循环；允许 bytehost 内存在 WinitDialogDriver 与 IcedDialogDriver 两个薄驱动，共用控制器、配置、几何和呈现规则。产品侧不得各复制一份对话框策略。

Winit 驱动复用已有 overlay / OverlayGpu，集中通用输入、IME、光标、重定位与渲染适配；Iced 驱动使用 iced 支持的建窗 / 运行时命令及 bytehost 原生父子绑定辅助。不能假定 Digger 能直接取得 `ActiveEventLoop`，也不能引入第二条主事件循环或强转私有 iced 类型。

实施先做两个真实最小窗口：证明父子绑定、透明遮罩、wry 层级、失焦保留和创建失败销毁。若 iced 公共 API 无法满足，不允许退回“Digger 保持普通窗口但套相同皮肤”后宣称统一；应记录阻碍，修订公共宿主接缝或选择双方可用的同一驱动方案，再进入产品迁移。

feature 继续显式选择：纯设置 / 业务消费者不因新增 dialog 引入 GUI；默认 bytehost-desktop 不能意外开启 tiny-skia 或另一份 Renderer。两个驱动分别有消费者编译门禁。

## 6. 共享 Git 账户能力

GitHub / GitLab / Gitee 的状态、输入 / 校验 / 连接 / 断开、错误与重试视图统一；注册内置 `bytehost.git`。Dozer 的历史 `dozer.git` 跳转通过产品 alias 映射到新 ID，保留外部入口，不创建两页。Advanced 仍为自定义 ID。Digger 没有账户历史，不读取 Dozer 的账户或 Keychain。

服务接缝至少能查询账户、连接、断开、取消未提交请求；凭据与元数据后端可替换。provider 的 API endpoint、用户名解析、身份校验和结构化错误只有一份实现，保留现有 provider 功能，不顺便增加 OAuth、自托管服务器支持或权限范围扩张。

每次连接具有产品 / provider / request 身份；同 provider 只接受当前请求结果，不同 provider 可独立进行。关闭 / 重开窗口不让旧结果修改新输入。窗口是展示者，账户已提交事实由长寿命服务 owner 接收和重新查询确认。

网络校验前可取消；进入凭据 / 元数据持久化后按事实完成收尾，不因关窗丢弃已发生的保存 / 删除。Keychain 与元数据不是一个原子事务：任一失败须报告可诊断的部分结果，重新查询恢复，不假装全部连接 / 断开成功。允许的重试必须幂等。

Dozer 保留现有 `dozer-git` Keychain service、provider key 和 git_accounts.json 路径 / schema；Digger 注入独立 `digger-git` 和自己的 config 路径。共享默认实现不得读取 Dozer 全局路径。PAT 不落明文配置、不进入 Debug / 日志 / 普通事件 / 截图测试产物；关闭后清理输入缓存。测试用 fake credential / network / metadata，不触碰真实账户。账户显示状态与 Git push / pull 使用凭据的接线是否相同，要逐消费者核查，不能只迁移“连接”按钮。

## 7. 产品行为与兼容

- Dozer 保留所有页面、已安装应用、审批、语言持久化和 daemon 操作；关闭再打开时公共 / 产品状态的保留策略不能变化。主题与语言异步结果继续回到产品 owner。
- Digger 切换至统一 ParentOverlay，保留主题保存错误、应用管理和清理台账提示 / 重试；加入 Git 页与独立账户服务。设置关闭不终止安装事实、应用清理或 usage 排空。
- 两产品数据根、账户和端口仍相互隔离；仅代码共用，不自动同步设置。
- 旧的普通设置窗口路径和 SettingsOverlay 的重复策略在迁移完成后删除；过渡期可分阶段接入，但不得长期维持两个生产开关掩盖缺口。
- 公共示例提供明确 ProductSettings 装配位置；新项目只需页注册、服务配置、主题 / 文案解析和驱动接线，不复制 Dozer / Digger 的 settings 文件树。

## 8. 验收矩阵

| 编号 | 必须证明的结果 | 证据 |
|---|---|---|
| A1 | 状态、UI、服务、宿主依赖方向正确，无产品反向依赖和意外渲染后端 | 最小消费者编译、cargo tree / metadata、依赖门禁 |
| A2 | 两驱动真实满足同一 ParentOverlay 行为 | 两个最小实机程序和同一契约用例，不接受截图相似代替 |
| A3 | 重复打开、关闭重开、迟到 Built、失败重试、父窗退出没有孤儿 / 复活 / 永久阻塞 | fake driver 确定性序列 + 实机故障注入，记录 session / attempt / 句柄 |
| A4 | 同主题下布局 / 关闭 / 滚动一致，小窗口、大字号、多屏 DPI 可操作 | 两产品固定条件截图与交互记录 |
| A5 | IME / Esc 优先级 / Tab / 原生粘贴 / 焦点恢复正确 | 两驱动实机输入用例，包括真正输入法组合输入 |
| A6 | wry 不盖遮罩、不接收底层输入，关闭按最新状态恢复 | 编辑器 / 预览 / 应用 webview 打开时的实机生命周期用例 |
| A7 | 三 provider 共用实现，乱序 / 取消 / 保存失败 / 部分提交均可恢复 | 服务 fake 与状态机测试；最小示例；不要求真实账户联网登录 |
| A8 | Dozer 老账户 / Keychain / 页面入口兼容；Digger 和新产品数据隔离 | 旧格式 fixture、alias 测试、两临时产品根、凭据后端调用断言 |
| A9 | 两产品既有设置业务回归通过，Digger Git 可操作，没有占位假功能 | 现有测试 + 产品消息集成 + 实机审批 / 文件选择 / 关闭再打开 |
| A10 | 新项目可由示例组装，无产品仓库源码依赖 | bytehost 示例编译运行、假服务说明与公共 API 文档 |
| A11 | 声明的已发布统一版本在无 path patch 环境 locked 编译 / 测试通过 | bytehost / Dozer / Digger 全部消费者日志和实际 tag SHA |
| A12 | 迁移、删除旧路径、回退边界和未验证项记录完整 | 验收报告、分阶段变更清单；实机受限项明确未通过 |

只跑无窗口单测、只编译、或仅完成其中一个产品都不能标记整体完成。平台首批为 macOS；其他平台不支持时返回显式能力 / 错误，不能成功打开行为不同的普通窗口冒充验收。

## 9. 交付与回退

按公共契约 / 驱动、Git 能力、Dozer 回接、Digger 回接、依赖发布分阶段交付，每阶段可独立审阅。共享 API 在两个消费者和最小示例编译后冻结；已有 API 的删除需跟随所有消费者迁移，不留下半升级组合。

产品迁移前保存原有设置 / 账户 fixture 和最后可用版本。账户和配置 schema 不变时回退主要是代码与统一 pin 回退；若出现必要 schema 变化，必须先另写向后兼容 / 导出方案，不能修改来源后只提供旧二进制回退。不要通过跨产品复制凭据恢复。

提交 / 推送 / tag / 发布按实施时的授权执行；文档不会因为列出候选版本就证明版本已存在。最终验收报告分别标明自动通过、实机通过、失败、未执行与环境受限。
