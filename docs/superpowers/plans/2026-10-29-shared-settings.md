# 共享设置模块 Implementation Plan

- 日期：2026-10-29
- 状态：已实施（代码与回归见[验收报告](../reviews/2026-10-29-shared-settings-acceptance.md)）；发布与实机项未完成。本计划的复选框保留为原始工作清单。
- Spec：[共享设置设计](../specs/2026-10-29-shared-settings-design.md)
- 目标：将 Dozer 设置的共享部分放入 bytehost，Dozer 回接全部五页，Digger 接入主题与应用管理；两产品通过注册协议扩展。
- 技术栈：Rust 2024、iced 0.14、byteui、bytehost；三仓库联调。

## 执行约束

- 本次文档工作不执行下面的代码修改、发布或迁移。实施时按任务顺序完成，每个任务形成可独立审阅的变更。
- 路径以下以共同父目录为基准：`bytehost/`、`dozer/`、`digger/`。执行前读取各仓库当时适用的工作指令。
- 不覆盖已有未提交内容。本次读取时 Digger 的 `Cargo.lock` 已修改，`.cargo/config.toml` 指向本地开发源码；实施时重新记录，不能直接重置或拿来当发行基线。
- 共享库不依赖产品 crate；产品状态与服务留在产品层。共享组件只产生消息 / effect，不执行服务、读写文件或访问 Keychain。
- 保留 Dozer 配置与凭据位置。Digger 使用自己的目录、安装审批和执行链。不得把两产品的状态接到同一份配置或 AppService。
- 本地 patch 只用于开发；不把路径 patch 提交为最终依赖。不预先填写尚未发布的版本号。
- 测试围绕契约、状态所有权与真实风险；不为简单搬文件编写逐行镜像测试。不依赖旧文档里的测试数量作为通过标准。

## Task 0：基线与迁移清单

**文件：** 新建 `digger/docs/superpowers/notes/2026-10-29-shared-settings-inventory.md`。

- [ ] 记录三仓库的 HEAD、dirty 文件、Cargo 声明依赖、实际 patch / lock 来源、构建环境。
- [ ] 对照 spec §2 核实源文件，列出 settings 的所有调用点、主题读写 API、语言广播、应用 orphan result、PAT 取消、daemon 控制与各页面测试位置。
- [ ] 列出 Dozer `settings_apps_view.rs` 和 Digger `apps/{management,approval,runtime,logs,actions}.rs` 的视图差异，分别标注共享部分、产品策略和不能删除的产品行为。
- [ ] 运行并记录相关测试与构建基线：bytehost panel / desktop，Dozer app 的 settings / settings_apps，Digger desktop / app-host 与 host-deps。已有失败单独列出原因和复现方式。
- [ ] 记录两款产品的设置 / 应用管理截图或可复现状态：主题、语言冲突、安装确认、运行时缺失、日志、错误；没有显示环境时明确标注未取得。

**完成条件：** 每个源状态、消息、effect、视图和测试都有迁移归属，避免抽取时遗漏产品副作用。

## Task 1：先验证扩展 API、Renderer 与窗口可行性

**新增：** `bytehost/crates/bytehost-settings/{Cargo.toml,src/lib.rs}`、`bytehost/crates/bytehost-settings-ui/{Cargo.toml,src/lib.rs,examples/composition.rs}`。修改 bytehost workspace 清单。

- [ ] 建立不依赖产品的最小公共 API：`PageId`、`PageDescriptor`、`PageKind`、`PageRegistry`、`SettingsState`、`SelectionState<T>`；共享 UI 建立容器和主题页的最小骨架。
- [ ] 编写消费示例：同一框架注册主题页、`dozer.git` 或 `digger.editor` 自定义页，并在主题页尾部插入产品附加区块。使用类型化 `.map` 路由，不引入字符串消息总线。
- [ ] 验证纯状态 crate 依赖闭包仅标准库及必要 serde；UI 默认 feature 不引入额外 Renderer 后端，不依赖 desktop。
- [ ] 分别用 Dozer 与 Digger 实际的 iced / Renderer 类型编译最小消费代码，冻结公共签名，特别是 Element 生命周期、消息 Clone 约束和文本解析回调。
- [ ] 在 Digger 实际窗口循环中做最小独立设置窗口验证：wry 浏览器 / 编辑器 / 应用在前台时仍可见并接收输入，失焦不关闭，IME 与粘贴可用，关闭后主窗正常交互。优先 iced 多窗口；若不满足层级，改用已有 bytehost overlay，不新写 GPU 管线。
- [ ] 把选定窗口接法、Renderer feature 与确切接口写入 inventory 和 spec；此时才继续大量迁移。

**完成条件：** 扩展页和附加区块两种方式均可编译；两个真实产品均能消费；Digger 的原生层级方案已验证。对应 A1、A2、A7。

## Task 2：共享注册、导航和保存状态

**新增：** `bytehost-settings/src/{page,registry,state,selection}.rs` 及相应测试。

- [ ] 实现 ID 校验、保留命名空间、重复检测、有序页面集合。内置页 ID 与种类必须一致，产品不能冒用 `bytehost.*`。
- [ ] 实现 open / select / close / registry 更新；默认主题页、首个可用页、空集合状态；重复打开不创建新窗口，窗口代数只在真正新开时增长。
- [ ] 实现语言等选择的通用保存状态：saving 时忽略重复提交，成功确认，失败保留 pending，冲突采纳服务端值，unsupported 禁止提交。通用层不解析产品 revision。
- [ ] 测试重复 / 非法 ID、未知目标回退、选中页移除、空集合、重开代数、失败重试与冲突。明确产品适配层负责丢弃过时请求。

**验证：** `cargo test -p bytehost-settings`。对应 A1、A2 与 A3 的共享状态部分。

## Task 3：共享容器、主题与语言页

**新增：** `bytehost-settings-ui/src/{shell,theme,language,section,text}.rs`。
**来源：** Dozer `extensions/settings.rs` 的 settings_card、scheme_row、language_page、导航与样式 helper。

- [ ] 抽取标题、导航、分隔、右侧内容、关闭操作和可滚动内容。所有尺寸使用 byteui token 或调用方约束，避免引用 Dozer project_create helper。
- [ ] 主题页读取当前值并发选择事件，移除直接 `set_scheme` / 文件写入。语言页使用通用模型，移除 dozer-model / service 引用。
- [ ] 内置页支持映射到产品消息后的附加区块，独立自定义页面可直接作为容器内容。
- [ ] 增加 `bytehost.settings` 文本 key / fallback 的解析接缝；Dozer 资源迁移相应通用文案，产品词表保留 Git / Advanced。无翻译时必须显示英文 fallback。
- [ ] 给示例补 Theme、Language saving / failed / conflict / unsupported、空注册表、长文本、小窗口与暗 / 亮主题状态。

**验证：** 示例编译、共享 UI 测试及可视检查；共享层依赖闭包无产品类型。对应 A1、A2、A3。

## Task 4：共享应用管理视图

**新增：** `bytehost-settings-ui/src/apps/`，按实际体量拆 management / approval / runtime / logs 等视图文件。
**来源：** Dozer `settings_apps_view.rs`，对照 Digger `apps/` 视图。

- [ ] 使用借用的 `InstallFlowState` 与原 `install::Message`，不新增安装状态机或执行器。
- [ ] 将产品格式化、文案与必要的只读能力差异变为参数 / 回调；文件选择只发消息。不可把权限审批隐藏在默认回调里。
- [ ] 迁移应用列表、安装来源、计划 / 权限说明、确认、运行时、日志、忙碌与错误状态。状态机和 effect 继续在 bytehost-panel。
- [ ] 为原 Dozer 视图保留短期 re-export 或薄包装，逐个接回调用点，避免双份 UI 长期存在。
- [ ] 用现有或最小 fake 状态验证安装确认发原始消息、卸载模式不变、busy 禁用、权限 / 风险文本仍可见。审批与网络等行为测试留在原服务层。

**验证：** `cargo test -p bytehost-panel -p bytehost-settings-ui`，对照 Task 0 场景检查差异。对应 A1、A6。

## Task 5：Dozer 回接与产品扩展拆分

**修改：** Dozer Cargo 清单、`extensions/settings.rs`、`settings_apps*.rs`、`platform/settings_overlay.rs`、`app/update.rs`、设置相关 i18n 资源。
**新增：** `dozer/crates/dozer-app/src/extensions/settings/{git,advanced}.rs`（以实际模块布局为准）。

- [ ] 用注册表替代固定 SettingsTab 的共享依赖，保持原五页顺序、默认页与所有直达入口。
- [ ] Git、Advanced 转为产品自定义页，状态与业务服务留在 Dozer。移走的代码不要继续留第二份实现。
- [ ] 主题事件交回产品，保留原主题路径、即时应用和预览刷新；写入错误转为用户可见反馈。
- [ ] 将 Dozer 语言状态映射到共享模型，保留权威快照、冲突、unsupported、重连、缓存和广播链。
- [ ] 应用共享页接现有 AppHostApi 适配；保留 orphan result、通知、HostChanged、purge_intents 的所有权与顺序。
- [ ] 原生 SettingsOverlay 只负责窗口和事件映射，复用已有 bytehost 机制，删除对旧 settings_card 的直接产品耦合。
- [ ] 迁移或保留关键测试：PAT 取消实际 abort、迟到结果丢弃、断开元数据失败不删 token、daemon alive 数量 / 停止 / 重启 / 重开、语言失败与冲突、关窗后的应用完成和清理。
- [ ] 增加窗口代数 / 请求 ID 验证；持久操作回包更新产品事实，不被窗口关闭吞掉。

**验证：** `cargo test -p dozer-app`、`cargo check -p dozer-app`，按仓库现有规则跑格式与相关门禁；实机五页对照。对应 A3、A5、A6、A7。

## Task 6：Digger 设置入口、窗口和主题

**新增：** `digger/crates/desktop/src/settings/{mod,window,theme}.rs`。
**修改：** workspace / desktop Cargo 清单、`crates/desktop/src/{lib,product,shell}.rs` 中实际入口与消息路由、`crates/paths/src/lib.rs`。如模块声明位置不同，按实际结构更新。

- [ ] 增加产品设置状态与消息，注册 Theme / Apps。产品扩展示例留在测试或 examples，不新增虚构的正式功能页。
- [ ] 在顶栏增加设置入口，并统一支持按 PageId 打开；窗口驱动采用 Task 1 验证方案，按窗口 ID 区分主窗和设置窗事件。
- [ ] 创建、激活、Esc / 按钮 / 系统关闭、失焦、主窗退出都走同一生命周期；消息关闭不会退出主应用。
- [ ] 在 paths 增加 Digger 专属主题文件路径，使用 byteui 格式读取与保存；首次启动保持当前默认值，损坏文件回退且提示。
- [ ] 主题生效后同步主窗、设置窗、编辑器、预览和其他主题敏感 webview；隐藏页面恢复应用最新主题。不得仅改变设置页选中项。
- [ ] 验证重复打开单实例、保存失败可重试、重启恢复、过时回包不污染重开窗口；测试使用临时数据根。

**验证：** `cargo test -p digger-desktop -p digger-paths`、`cargo check -p digger-desktop`；运行窗口场景。对应 A4、A5、A7。

## Task 7：Digger 应用页接现有唯一执行链

**修改：** `digger/crates/desktop/src/settings/mod.rs`、`apps/{mod,management,approval,runtime,logs,actions}.rs`、`product.rs`。

- [ ] 设置应用页借用现有 `AppsState::flow()`，所有共享消息映射到现有 `apps::Message::Install`；不在设置状态中再加 InstallFlowState。
- [ ] 应用面板管理部分也使用共享视图，原入口保留；“去设置”直达 `bytehost.apps`。设置打开不重复初始化服务、订阅、执行器或轮询。
- [ ] 保留 `digger-app-host` 的信任审核、prepare / approve 绑定、请求身份、用量追踪和 runtime 管理；新 UI 不绕过这些链路。
- [ ] 安装、停止、回滚、卸载与日志沿用原结果通道。关闭设置后的完成更新运行态和通知；重新打开读取现状，不能重发操作。
- [ ] 保留并扩展已有 ProgramAndData 成功清理 / 失败不清理、日志结果、用量事件与请求隔离测试；补两个入口共享同一 flow 和不重复提交的集成测试。
- [ ] 删除被替代的重复视图，保留必要的产品布局薄包装和展示差异适配。

**验证：** `cargo test -p digger-desktop -p digger-app-host`，确认新增入口经过同一审批与结果追踪。对应 A4、A6。

## Task 8：跨产品回归与实机验收

**新增：** `digger/docs/superpowers/reviews/2026-10-29-shared-settings-acceptance.md`，逐项记录命令、结果、截图与未完成项。

- [ ] bytehost：新增 crate 测试、`bytehost-panel` / `bytehost-desktop` 回归、composition 示例编译、`cargo fmt --check`。用 `cargo tree -p bytehost-settings` 和 `cargo tree -p bytehost-settings-ui -e normal,build` 检查 A1。
- [ ] Dozer：settings 与 app 回归、构建、现有相关架构门禁；确认新 crate 不引入产品反向依赖。
- [ ] Digger：`cargo test -p digger-desktop -p digger-app-host -p digger-paths`、`bash scripts/check-host-deps.sh`、`bash scripts/run-editor-selftest.sh lifecycle`；只在改动影响对应 web 包时运行网页包检查。
- [ ] 扩展契约：产品自定义页、主题页附加区块、非法 / 重复 ID、不可用直达页回退；示例无需修改共享库代码。
- [ ] 实机：两款产品分别检查亮 / 暗、小窗口 / 高 DPI、中文 IME / 粘贴、Esc、失焦、重复打开、关闭后重开、退出清理；编辑器 / 浏览器 / 应用 webview 在前台时设置仍正常覆盖和交互。
- [ ] Dozer：Git 取消、连接失败、断开失败，daemon 停止确认与重启，语言失败 / 冲突 / 重连，关窗后的应用操作完成。
- [ ] Digger：应用面板与设置页交替操作、安装审批、runtime 缺失、日志、卸载数据清理；验证没有重复服务或状态。
- [ ] 同时启动两产品，用临时产品数据根验证主题、账户 / 应用目录、端口与 daemon 不串扰；记录主题重启恢复与故障写入场景。

**完成条件：** spec A1–A8 各有证据；未执行的实机项必须明确标为未验证，不能据此标记整体完成。

## Task 9：统一依赖、发布顺序与文档收尾

**修改：** 三仓库 Cargo 清单 / lock、相关 README 与 spec 状态。发布操作按实施时获得的授权执行；未授权时先准备可审阅的完整发布变更。

- [ ] 先完成三仓库本地联调；确定 bytehost 与 byteui 兼容 pin。若需要 Dozer panel 依赖更新，纳入同一发布组。
- [ ] 发布包含两个新 crate 的 bytehost tag；Dozer 全部 bytehost 依赖统一到该 tag，验证后生成新的 Dozer 库快照；Digger 再统一更新 bytehost 与所有同源 Dozer 库 pin。
- [ ] 使用隔离 checkout / Cargo 配置验证无本地路径 patch 的构建。保留个人开发配置，不通过删除用户配置完成验收。
- [ ] 两消费方检查重复依赖：`cargo tree -d`，确认 byteui、bytehost-core 及相关 iced 核心类型仅一份兼容来源；运行各仓库 `scripts/check-shared-deps.sh`（如有）。
- [ ] 记录发布组的确切 tag / rev 与回滚 pin；若需要回滚，将三仓库按兼容发布组回退，不删除新写入的用户文件、不恢复旧数据快照覆盖新数据。
- [ ] README 链接 spec / plan / 验收报告；spec 状态只在全部必需交付和验收完成后改为已实施。Digger 语言、Git、daemon 页继续列为后续能力，不能写成已接入。

**完成条件：** 干净消费者可构建，版本可复现，文档与实际页面集合一致。对应 A8。

## Spec 覆盖索引

| Spec | 实施与验证 |
|---|---|
| §3–4 所有权 / 依赖 | Task 0、1、4、8、9 |
| §5 注册、路由、两种扩展 | Task 1、2、3、8 |
| §6.1 主题与持久化 | Task 3、5、6、8 |
| §6.2 语言兼容与启用边界 | Task 2、3、5、9 |
| §6.3 应用状态与服务复用 | Task 4、5、7、8 |
| §7 Dozer 产品扩展 | Task 5、8 |
| §8 原生窗口与异步生命周期 | Task 1、5、6、7、8 |
| §9 A1–A8、发布与回滚 | Task 8、9 |
