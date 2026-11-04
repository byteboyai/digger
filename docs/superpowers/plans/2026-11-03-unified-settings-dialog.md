# 统一设置模块与对话框宿主 Implementation Plan

- 日期：2026-11-03
- 状态（2026-11-04）：共享实现与审核修复已发布为 bytehost `v0.4.23`、Dozer `libs-2026-11-04a`；发行自动验证见[发布记录](../reviews/2026-11-04-shared-settings-release.md)。实机验收及其余未勾选项仍待完成，不能将整份计划标为完成。
- Spec：[统一设置模块与对话框宿主](../specs/2026-11-03-unified-settings-dialog-design.md)
- 范围：bytehost、Dozer、Digger；byteui 仅在需要公共 token 时修改。
- 交付目标：两产品同一 ParentOverlay 行为和共享设置能力；允许不同事件循环的薄驱动，不允许继续维护两份宿主策略。

## 执行约束

- 本计划是新工作，不把上轮共享页面的完成等同于本轮宿主统一完成。实施前读取各仓库适用指令；本计划不要求多代理。
- 当前文档在 Digger；兄弟仓库当前仅可读。实施时使用各仓库可写工作区，未取得写入能力前不声称已跨仓落地。不改个人 Cargo 配置或真实产品数据。
- 修改前记录 HEAD、dirty 和锁文件，保留现有修改。以下路径带仓库前缀；模块名称是建议，Task 1 / 2 编译冻结后在文档更新实际位置。
- 先证明 Digger 的 iced 驱动能承载真实父窗口 overlay，再迁移生产；不能把独立普通窗口保留为默认回退并标记统一成功。
- 不创建第二条 GUI 主事件循环，不依赖 iced 私有结构，不把 `bytehost-settings-ui` 变成建窗 / 服务调度模块。
- 账户测试全部使用临时产品根、fake 网络和凭据。实机输入使用无效测试字符串，不把真实 PAT / Keychain 内容写入报告。
- 每阶段提交公共 API 与消费者接线的可审阅变更；纯移动和行为变化尽量分开。任何必要偏离写明原因与影响，不改验收条件来掩盖失败。
- Cargo target / HOME / 缓存用可写隔离目录；完整实机验证需要图形会话、进程查询能力和多屏条件。能力不足写“未验证”，不删断言或静默跳过。

## Task 0：冻结基线、交互与消费者清单

**读取：**

- bytehost：`crates/bytehost-settings/src/`、`bytehost-settings-ui/src/`、`bytehost-desktop/src/{overlay,overlay_gpu}.rs`、Cargo feature。
- Dozer：`extensions/settings*.rs`、`platform/{settings_overlay,overlay_window,overlay_gpu}.rs`、`git_accounts.rs`，及 app update / 窗口事件分发 / Git 账户消费者。
- Digger：`settings/{mod,theme,window}.rs`、`product.rs`、原生页面同步、订阅和设置 Tests。

**新增：** Digger `docs/superpowers/notes/2026-11-03-unified-settings-dialog-baseline.md`。

- [ ] 记录各仓库 HEAD、dirty、已发布 tag SHA、实际锁、path patch、Rust / Node / macOS / 显示器 / GUI 权限。
- [ ] 列出打开入口、页面 ID / alias、状态 owner、window / native handle、背景任务 owner 和所有 Git 凭据消费者。
- [ ] 记录两产品主题 / 应用 / 语言 / Git / Advanced 的已有测试与基线命令，保留失败和未执行项。
- [ ] 保存同主题、同父客户区尺寸的设置截图，记录焦点、Esc / 失焦、滚动、原生 webview、文件选择和关闭再打开行为。
- [ ] 保存 Dozer 非敏感账户元数据 fixture 与旧 Keychain key 映射；不导出真实 Token。

**验证 / 完成条件：** 能区分共享实现、产品接线、已存在的底层 overlay 能力和当前重复策略。现状与 spec 不同先修正文档，不按旧假设直接搬代码。

## Task 1：两种运行时的最小 ParentOverlay 可行性验证

**新增建议：** bytehost `crates/bytehost-desktop/examples/dialog_winit.rs`、`dialog_iced.rs` 与验证记录。先复用当前 overlay，不先搬生产设置。

- [ ] Winit 示例证明父所属辅助窗口 + 共用遮罩 / 卡片内容；不改变已有主渲染器的 feature。
- [ ] Iced 示例用公共建窗 / runtime API 与原生父子绑定，证明 Digger 无需取得 ActiveEventLoop 或创建第二条事件循环。
- [ ] 两示例跑同一组打开、重复打开、移动、缩放、跨 DPI、切应用、最小化 / 恢复、关闭、父窗退出场景。
- [ ] 给示例放入真正 wry 子视图，证明遮罩层级与输入阻断；单独验证透明覆盖区域不会点击穿透。
- [ ] 注入一次创建 / 绑定失败，观察无透明挡输入窗口、可重试、可恢复父窗焦点。
- [ ] 冻结 DialogHost / Driver 的最小命令事件接口、原生句柄借用生命周期、Renderer feature 与支持平台；记录不支持能力的错误。

**验证：** 两示例分别编译与实机运行，附句柄 / session / DPI 日志与截图；不以 compile success 代替窗口行为。

**门禁：** A1 / A2 可行后进入 Task 2。若 iced 驱动无法满足，先提供具体失败证据与兼容公共接缝方案，更新 spec；不开始 Digger 的生产迁移，不把原普通窗口算作最终实现。可继续独立的 Task 5 账户抽取。

## Task 2：共享对话框控制器、身份和几何

**新增建议：** bytehost `crates/bytehost-desktop/src/dialog/{mod,model,geometry}.rs`；必要时调整 `bytehost-settings/src/state.rs`。

- [x] 实现 Closed / Opening / Open / Closing / Failed 与类型化 effect；owner / kind / session / attempt 过滤覆盖所有驱动回包。（`bytehost-settings::dialog`，含迟到 / 重开 / 失败重试 / 超时测试）
- [ ] 与 SettingsState 既有 window_generation 收口成单一权威身份，处理兼容入口与消费者迁移；明确导航与原生窗口状态的各自 owner。
- [x] 实现一个父窗口单设置实例、重开激活 / 切页、幂等关闭、失败显式重试和有界销毁。
- [x] 实现统一逻辑几何：40% 宽 / 75% 高，正常空间最小 560×520；父可用区域钳制、留白、大字号 / 小窗口滚动。
- [x] 把几何 / 透明 / 输入阻断策略建立在现有 overlay 机制上；删除本次替代的重复计算，不另建同义常量。（Dozer 卡片尺寸已改调共享几何）
- [x] 确定哪些 API 是对所有 dialog 公共、哪些是 settings 配置；不为尚未迁移的其他弹窗加入业务枚举。

**验证：** fake driver 的确定性状态序列；覆盖开关交错、旧 Built 成功 / 失败、旧 Close / Focus、销毁失败 / 超时、父窗关闭。几何覆盖不同尺寸、scale、窄区域和父窗位置。无产品 / GUI 服务反向依赖。

**完成条件：** A1、A3 的无窗口部分；两个驱动消费同一模型，不再各维护重开 / 失败 / 关闭策略。

## Task 3：共享驱动与输入生命周期

**修改建议：** bytehost `desktop::dialog::{winit,iced}`、`overlay.rs`、`overlay_gpu.rs` 及相关 examples。

- [ ] Winit 驱动集中通用 UserInterface / redraw / cursor / IME / clipboard / event conversion 与重定位接线。
- [ ] Iced 驱动委托 iced 渲染与 runtime，只保留必要的运行时适配；使用同一 controller、几何和配置，禁止产品直接散落原生父子绑定代码。
- [ ] 统一父子层级、Space / 全屏 / 最小化跟随、父句柄失效、原生关闭与退出销毁；所有 native 操作在正确线程执行。
- [ ] 实现 IME / 子菜单 / 页面确认 / settings 的 Esc 顺序，Tab 焦点范围与有效目标恢复。
- [ ] 发出原生内容阻断 / 恢复信号，提供产品 hook；恢复必须按最新视图状态重新同步。
- [ ] 驱动 feature 可分别编译；默认 desktop 消费者不会增加 Renderer 后端。避免两份 iced / wgpu 类型和 default feature 意外合并。

**验证：** Task 1 两示例重跑；native fault 注入；检查 Renderer feature 图。真实组合输入、粘贴、原生菜单不能由模拟 ChangeSet 或截图代替。

**完成条件：** A2、A3、A5 / A6 的宿主部分；形成可审阅的公共 API 与示例。

## Task 4：设置内容、页身份与公共视觉规范

**修改：** bytehost `settings-ui::{shell,section,theme,language,apps}.rs`、`settings::page`；必要时 byteui token。

- [x] 明确共享 shell 只渲染卡片内内容；dialog host 只包一次遮罩 / 卡片，防止重复边框、重复滚动和双关闭行为。
- [x] 统一导航、标题、右上关闭、底部关闭、留白、焦点 / hover、主题和小窗口布局；将必要尺寸 / 样式归公共 token。
- [x] 应用页保留自身滚动策略，其余内容通过统一滚动容器；标题和关闭按钮一直可达。
- [x] 增加内置 bytehost.git；Dozer 侧保留 dozer.git alias，避免旧直达入口失效；页面集合仍按能力注册。
- [x] 确认文本 key / fallback 一致、产品可解析 locale，没有硬编码 Dozer / Digger 名称。

**验证：** 同主题 / 同尺寸视图示例、小窗口 / 大字号检查；注册 / alias / 扩展页回归。视觉改变与 token 变化分别说明。

**完成条件：** A4 共享视图部分；不会为了统一布局移除应用审批或错误展示。

## Task 5：抽取共享 Git 账户服务和状态

**新增建议：** bytehost `crates/bytehost-git-accounts`，模块 `model / service / providers / credentials / metadata`；settings 的 Git 状态模块。

- [x] 分离纯 DTO / 服务契约与 HTTP / Keychain 实现 feature，保持 settings 默认依赖闭包不含网络 / 平台服务。
- [x] 从 Dozer 提取 GitHub / GitLab / Gitee provider endpoint、Token 校验、用户名解析、账户查询 / 连接 / 断开；核对远端仓库查询消费者，避免保留重复后端。
- [x] 接口注入产品 config 路径、Keychain service / provider key、HTTP 和持久化后端；默认不能落入 Dozer 目录。
- [x] 实现 provider 独立 request 身份、取消 / 乱序过滤、关闭后的 owner 收尾；UI 重开先查权威账户事实。
- [x] 注入 Keychain 成功 / 元数据失败、删除凭据失败、Token 无效、超时、确认通道关闭等结果；部分提交错误可见、可重新查询和幂等重试。
- [x] Token 通过专用敏感输入命令传递，不经普通可 Debug 的产品消息；清理输入缓存，测试输出与日志脱敏。
- [x] 旧账户 schema、dozer-git service、provider key 和旧路径无迁移；若发现不能兼容，先补数据迁移 spec 再改格式。

**验证：** fake credential / metadata / HTTP 服务测试；旧格式全字段 fixture、三 provider 错误解析、取消前与持久化后关闭、两产品数据根隔离。禁止使用真实 Keychain 或真实账号请求做自动测试。

**完成条件：** A7、A8 服务层；bytegit 职责和现有产品 Git 调度没有改变。

## Task 6：共享 Git 账户页与新产品装配示例

**新增建议：** bytehost `settings-ui/src/git_accounts.rs`、`settings-ui/examples/settings_product.rs`，以及两驱动加载共享设置内容的运行示例。

- [x] 三 provider 行、遮盖输入、连接 / 取消 / 断开、忙碌、失败 / 部分提交提示和重试只维护一份视图与状态规则。
- [x] settings 页面产生类型化意图，调用服务由产品或共享应用适配器执行，UI 不访问 Keychain / 文件。
- [x] 为新项目提供 Theme / Git / 一个自定义页的装配示例，明确 fake 与真实服务配置；Apps / Language 通过可选注册示范。（`examples/settings_product.rs` + composition）
- [x] 示例展示 main window owner、驱动事件接线、文案解析和关闭结果处理，不依赖 dozer-app / digger-desktop。

**验证：** 最小消费者编译、fake 登录 / 失败 / 取消交互；两驱动显示同一 Git 页。PAT 遮盖和真实编辑操作另在 Task 10 验收。

**完成条件：** A7 UI 部分、A10；“引入库即可使用”有实际示例支撑。

## Task 7：Dozer 回接共享宿主和 Git 能力

**修改：** Dozer `extensions/settings*.rs`、`platform/settings_overlay.rs`、窗口事件分发、`git_accounts.rs` 和账户消费者。

- [x] 以共享 DialogHost 替换 SettingsOverlay 内重复的控制、输入与重定位策略，保留运行时薄接线和公共设置内容装配。（窗口生命周期由 `DialogController` 驱动；winit 输入 / 重定位仍是产品薄适配）
- [x] Git / Advanced / Apps / Language / Theme 业务保持；Git 页切到 bytehost.git 并映射旧入口。
- [x] git_accounts.rs 收缩为产品服务配置或兼容重导出；Dozer 账户、Keychain 和远端仓库调用统一经过共享实现。
- [x] 主题 / 语言权威结果、安装批准 / 卸载确认、清理持久化、daemon 重启和关闭后结果继续由原业务 owner 处理。
- [x] 删除迁移后不再使用的旧状态、view、身份与窗口策略，不删除其他尚未迁移的弹窗能力。

**验证：** Dozer 既有设置 / 账户 / 应用 / 语言回归，实际窗口重开、切页、旧账户显示、fake 服务错误与后台完成测试。

**完成条件：** A8、A9 的 Dozer 部分；主窗口其他 dialog 与 Toast 无回归。

## Task 8：Digger 切换统一宿主并接入 Git 页

**修改：** Digger `settings/{mod,window}.rs`、`product.rs` / `product/subscriptions.rs`、Git 服务适配、路径与原生页面同步接缝。

- [x] 移除普通设置窗口作为生产路径，接入共享 `DialogController` + `bytehost-desktop::dialog` 原生父子绑定；未保留普通独立窗口回退。
- [x] 注册 Theme / Git / Apps，实例化独立账户服务和 digger-git 命名空间；不出现未接服务的“连接”按钮。
- [x] 保留主题 / 应用原状态与执行器，不创建第二个 AppHost、清理台账或 usage worker。
- [ ] 设置模态可见性纳入 markdown / preview / app webview 的输入与可见性管理；关闭按当前 Project / 面板 / 布局同步。
- [x] 重复打开、父窗退出、创建失败、晚到原生结果、后台安装 / 清理结果均接共享协议。
- [x] 不注册语言 / Advanced 占位页；保留将来能力扩展入口。

**验证：** settings、product、markdown、preview、apps、退出相关回归；Fake Git 服务的产品消息集成；所有 examples 编译。

**完成条件：** A3 / A8 / A9 的 Digger 自动化部分；产品只剩页面、服务和 runtime 薄适配。

## Task 9：契约测试、跨消费者依赖与 CI

**修改：** bytehost / Dozer / Digger 各自的门禁、workflow 和文档。

- [ ] 共享宿主测试覆盖两个驱动适配；验证 Open / Focus / Built / Close 的乱序和失败序列，不只测 enum / 文件名。
- [ ] 公共 API 的最小消费者分别关闭和开启 driver feature，检查 default feature / Renderer / iced / wry 版本一致性。
- [ ] 快速 CI 跑纯状态、fake 服务、产品消息集成、最小消费者与必要编译；图形 job 独立、明确环境条件。
- [ ] 继承现有安装 / 关闭 / 清理 / 用量测试，不因为设置迁移删掉质量阈值或把失败全部 ignore。
- [ ] 保存故障场景、当前版本、实机日志与失败截图；账户测试输出不能包含凭据。

**完成条件：** A1、A3、A7、A8 的可重复自动证据。CI 配置已提交和 CI 已实际运行分别记录。

## Task 10：双产品实机验收

**新增：** Digger `docs/superpowers/reviews/2026-11-03-unified-settings-dialog-acceptance.md`；跨仓可链接同一报告或同步结果。

- [ ] 同主题、同客户区尺寸核对布局；小窗口、大字号、Retina / 跨屏缩放、全屏 / Space、最小化 / 恢复与父窗退出。
- [ ] 两产品各跑重复打开 / 切页 / 关闭重开；注入创建 / 绑定失败再显式重试，记录 session / attempt 与句柄，检查无孤儿窗口。
- [ ] 真正中文输入法组合输入、Esc 先取消组合、Tab / Shift-Tab、原生编辑菜单 / 粘贴、外部浏览器打开再返回、失焦不关闭。
- [ ] 打开 markdown / preview / app webview 后打开设置，验证遮罩层级和输入阻断；设置期间后台切换 Project / 面板，关闭后恢复最新视图。
- [ ] 用 fake / 测试服务运行 Git 连接成功 / 失败 / 取消 / 断开、应用审批 / 卸载确认 / 文件选择；完成结果关窗后仍准确。
- [ ] A1–A12 分别填写通过 / 失败 / 未执行 / 环境受限，缺 GUI 或多屏不能写实机完成。

**完成条件：** A2–A9 的实机部分。没有真实账号测试不影响 fake 服务契约验收，但不能声称已验证生产账户授权。

## Task 11：统一依赖发布、无 patch 验证与清理

**修改：** 各仓库 Cargo.toml / Cargo.lock / 共享依赖声明；更新 README、spec 状态与验收报告。

- [ ] 若修改 byteui token，先验证兼容候选，再按授权发布 byteui；bytehost 跟随消费，Dozer / Digger 与传递库统一来源。
- [x] bytehost 公共 API 经两消费者和最小示例通过后，按授权发布共享版本；Dozer 库快照若传递依赖变化也同步收口，不能只更新产品直接依赖。2026-11-04 已发布上述两个 tag，byteui / bytegit 无新增改动，沿用已发布版本。
- [x] 候选开发 patch 验证与已发布 tag 验证分开；禁止先填尚不存在的版本号，再把本地构建当发行通过。远端 tag 与解析提交已留档。
- [x] 从无个人 `.cargo` patch 的隔离 checkout / CARGO_HOME，使用声明的已发布 tag 与锁执行 fmt、`cargo check --locked --workspace --all-targets`、相关行为测试、共享依赖门禁与最小示例；两个产品都验证。2026-11-04：Digger 740 / Dozer settings 49 通过；示例经 bytehost 全目标 Clippy 与 CI 编译，详见发布记录。
- [ ] 检查锁中没有本地路径来源、重复共享库或意外后端；用户自己的开发配置与历史 dirty lock 不覆盖。
- [ ] 删除生产设置旧路径 / 过渡开关，核查无重复 Git 实现；旧 dozer.git alias 可保留作为兼容入口。
- [ ] 整理回退：旧路径 / schema / credential namespace 不变，依赖组合整体回退；若有数据格式变化必须另有向后兼容方案。
- [ ] 新项目 README 明确驱动选择、最小装配、扩展页、账户隔离与支持平台；实机缺口未解决前不把整体标为完成。

**完成条件：** A10–A12；已发布统一依赖的干净消费者与双产品实机均有证据。发布受权限 / 环境限制时保留明确阻碍，不能只交付 Digger 内的补丁就标跨仓完成。

## 顺序与交付单元

Task 0 → Task 1 → Task 2 → Task 3 → Task 4 是宿主路线；Task 5 → Task 6 是账户路线，可在宿主验证受限时独立推进。Task 7 / 8 均依赖共享宿主、公共内容和账户能力已冻结，随后 Task 9 → Task 10 → Task 11 收口。这里的依赖允许一个开发者分阶段执行，不要求跨代理并行。

建议审阅单元：基线与可行性 → 共享控制器 / 驱动 → 公共设置与账户服务 → Dozer 迁移 → Digger 迁移 → 依赖与验收。每个单元附自动 / 实机证据和剩余风险，最终提交说明以完成后的实际边界为准。

## Spec 覆盖

| Spec / 验收 | 任务 |
|---|---|
| §2 基线 | Task 0 |
| §4 依赖边界 / A1 | Task 1、2、5、9、11 |
| §5 宿主 / A2–A6 | Task 1–4、7、8、10 |
| §6 账户 / A7–A8 | Task 4–8、9、10 |
| §7 产品兼容 / A9 | Task 7、8、10 |
| §7 新产品 / A10 | Task 6、9、11 |
| §9 发布与回退 / A11–A12 | Task 0、9–11 |
