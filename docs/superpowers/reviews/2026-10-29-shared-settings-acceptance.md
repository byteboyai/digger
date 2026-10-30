# 共享设置迁移验收报告（Task 8）

- 日期：2026-10-29
- 计划：[2026-10-29-shared-settings.md](../plans/2026-10-29-shared-settings.md)
- 后续复审修复：[修复记录与剩余发布限制](2026-10-29-shared-settings-fixes.md)；下列结果保留为首次验收记录。
- 基线：[2026-10-29-shared-settings-inventory.md](../notes/2026-10-29-shared-settings-inventory.md)
- 结论：**代码与单元 / 集成回归通过；实机窗口、发布与部分 webview 主题同步未验证。**
  因此 spec 仍保持“设计草案 / 未完成全部验收”，不能据本报告标记整体完成。

## 1. 已实现

### bytehost

- 新增 `bytehost-settings`（仅 serde）：
  `PageId` 校验、`PageKind`、`PageDescriptor`、有序 `PageRegistry`、
  `SettingsState`（打开 / 切页 / 关闭 / 窗口代数 / 注册表回退）、
  `SelectionState<T>`（saving / failed / conflict / unsupported）。
- 新增 `bytehost-settings-ui`：
  - 共享容器 `Shell`（标题、导航、分隔线、关闭、空状态、可选滚动）；
  - 主题页、语言页；
  - 应用管理页（运行时、列表、审批、安装 / 卸载 / 回滚、日志、错误 / 忙碌）；
  - 内置页尾部附加区块、自定义页插槽、产品前置动作 / 说明回调；
  - `bytehost.settings` 文案接缝（英文 fallback 表）。
- 示例 `examples/composition.rs`：注册主题页 + `digger.editor` 自定义页，
  在主题页追加产品区块，并借用共享 `InstallFlowState`。示例可运行：
  `registered 2 pages; current = bytehost.theme`。

### Dozer

- 五页顺序保持 Theme / Git / Advanced / Apps / Language，默认 Theme；
  `dozer.git`、`dozer.advanced` 成为产品自定义页。
- 设置容器改用共享 shell；导航、关闭、空状态由共享层绘制。
- 语言页改用共享 `SelectionState`；权威快照、revision、冲突、广播仍归产品。
- 主题页改用共享视图；选择仍即时 `set_scheme`，新增写入失败可见反馈。
- 应用页的 iced 视图改为共享实现；安装状态机仍是
  `bytehost_panel::install`，`settings_apps.rs` 继续 re-export。
- 保留：PAT 取消真正 abort、迟到结果丢弃、断开先写元数据再删 token、
  daemon alive 统计 / 停止 / 重启、orphan result、`purge_intents`、HostChanged。

### Digger

- 顶栏新增齿轮入口；`Msg::Settings` 与 `settings::{State,window,theme}`。
- 主窗改为 `iced::daemon`；设置窗为第二个原生 iced 窗口：
  - 单实例：重复打开只 `gain_focus`，不递增窗口代数；
  - Esc / 关闭按钮 / 系统关闭 / 主窗退出统一生命周期；
  - 失焦不关闭（没有失焦关闭逻辑）；
  - 设置窗事件按 window ID 与主窗隔离，不触发主窗布局 / Agent 输入。
- 新增 `<digger config>/color-theme.json`（byteui `{"scheme":...}` 格式）：
  - 启动读取并应用；文件缺失沿用 Dark；损坏回退并显示提示；
  - 选择即时应用；保存失败显示在主题页，可重试。
- 注册 Theme / Apps；不注册语言 / Git / 高级。
- 应用面板与设置页共用同一 `AppsState::flow()` 与共享页面；
  面板顶部新增「设置」直达 `bytehost.apps`；设置打开不重建执行器 / 订阅。
- 删除被替代的 `apps/{actions,approval,runtime,logs}.rs`；
  `management.rs` 只保留服务启动、通知、清理进度、中文文案、「打开」动作。

## 2. 命令与结果

| 命令 | 结果 |
|---|---|
| `cargo test -p bytehost-settings` | **22 passed** |
| `cargo test -p bytehost-settings-ui` | **7 passed** |
| `cargo run --example composition -p bytehost-settings-ui` | 输出 `registered 2 pages; current = bytehost.theme` |
| `cargo test -p bytehost-panel -p bytehost-desktop` | 通过（98 / 8 / 3 / 8 / 102 等） |
| `cargo test -p dozer-app` | **1133 passed, 2 ignored** |
| `cargo check -p dozer-app` | 通过（仅有仓库既有警告） |
| `cargo test -p digger-desktop -p digger-paths` | 通过；desktop 主测试集 **709 passed**，新增 settings / theme / paths 测试通过 |
| `cargo check -p digger-desktop` | 通过 |
| `bash scripts/check-host-deps.sh` | `ok（审视了 2 个业务 crate）` |
| `cargo tree -p bytehost-settings -e normal` | 只有 `serde` |
| `cargo tree -p bytehost-settings-ui -e normal,build` | 无 `dozer-*` / `digger-*` |
| `cargo fmt`（三个仓库相关 crate） | 已执行 |

新增 / 更新的关键测试：

- 状态层：非法 / 重复 / 保留命名空间 ID、未知页回退、空注册表、重开代数、
  移除当前页回退、保存中拒绝重复提交、失败保留 pending、冲突采纳服务端值、
  unsupported 禁止提交。
- Dozer：五页注册顺序与默认页、`SettingsTab` ↔ `PageId` 双向映射；
  原 PAT / daemon / 语言 / 应用测试全部保留。
- Digger：首批仅 Theme / Apps、重开代数、未知目标回退、主题文件往返 /
  损坏 / 保存失败、主题路径独立于 Dozer。

## 3. 未验证 / 未完成

以下项目**没有**实机证据，不能当作通过：

1. 原生窗口层级、设置窗在前台 webview / 编辑器 / 应用之上时仍可见并接收输入；
2. 中文 IME、标准粘贴、Esc、失焦保持、重复打开、关闭后重开、退出清理；
3. 小窗口与高 DPI 下关闭按钮可达、滚动体验；
4. Dozer 五页实机对照（Git 取消 / 连接失败 / 断开失败、daemon 停止与重启、
   语言失败 / 冲突 / 重连、关窗后的应用操作完成）；
5. Digger 应用面板与设置页交替操作、安装审批、runtime 缺失、卸载数据清理；
6. 两产品同时启动、临时数据根隔离、主题重启恢复；
7. **webview 主题同步**：Digger 现有预览 / 编辑器 / 群聊 / 待办 web 资源仍写死
   `theme=dark`（或没有主题参数），本次只保证 iced 主窗 / 设置窗与 byteui token
   即时更新。要做到 spec §6.1 的“所有主题敏感 webview”，需要单独改 web 资源与
   URL / 桥接协议，未在本期完成。
8. 发布：新 crate 尚未打 tag；Dozer / Digger 仍用本地 path patch。
   `Cargo.lock` 中的 Dozer 来源仍含本地 `dozer-browser-find` 依赖，发行前必须
   换成包含页内查找的正式库快照。`bash scripts/check-shared-deps.sh --offline`
   因此报告 `lock-not-git`（预期：本地联调 patch），干净 checkout 未验证。
9. 完整 `scripts/run-editor-selftest.sh lifecycle` 未运行：本期没有改动编辑器
   web 包，但计划要求在该项改动影响对应 web 包时才运行。

## 4. 已知取舍

- Dozer 共享文案沿用 `dozer.host` 现有 `settings.*` 词表；共享新增 key
  （少数错误行）没有中文资源时显示英文 fallback。
- Digger 没有 i18n 框架；`settings::shared_text` 提供常用中文映射，未覆盖键落英文。
- 共享应用页的若干内部标签（`observed_label`、`runtime_line`、`plan_lines` 等）
  仍来自 `bytehost-panel` 的英文文本，与迁移前一致。
- 设置窗口是独立普通窗口，不是覆盖在主窗上的无边框浮层；spec 允许的
  “iced 多窗口”方案，未复用 Dozer 的 overlay GPU 管线。
