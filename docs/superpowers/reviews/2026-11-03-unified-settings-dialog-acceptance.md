# 统一设置模块与对话框宿主 — 实施 / 验收报告

- 日期：2026-11-03
- 当前状态（2026-11-04）：**共享实现及第二轮审核修复已随 bytehost `v0.4.23` / Dozer `libs-2026-11-04a` 发布，Digger 已更新钉版与锁；实机验收仍未完成。** 最新发行自动验证见[发布记录](2026-11-04-shared-settings-release.md)。下方保留第一轮实施 / 验收证据，不能作为整体规格验收通过的结论。
- 基线：[`2026-11-03-unified-settings-dialog-baseline.md`](../notes/2026-11-03-unified-settings-dialog-baseline.md)
- 计划：[`2026-11-03-unified-settings-dialog.md`](../plans/2026-11-03-unified-settings-dialog.md)
- 设计：[`2026-11-03-unified-settings-dialog-design.md`](../specs/2026-11-03-unified-settings-dialog-design.md)

## 1. 实际落地边界

### bytehost

新增：

- `bytehost-settings::dialog`
  - `DialogController`：Closed / Opening / Open / Closing / Failed；
    `Create` / `Activate` / `Destroy` 类型化 effect；`owner + kind + session + attempt`
    身份过滤;重复打开只激活;关闭中重开等 Closed 后新会话;迟到 `Opened` 返回
    `Destroy(Stale)`;创建中关闭的迟到成功返回 `Destroy(CancelledOpen)`;父窗退出 /
    有界销毁超时都能回到 Closed。
  - `DialogGeometry`：40% 宽 / 75% 高、最小 560×520、四周目标留白 16;
    父窗口过小时按可用区域钳制，保证卡片和关闭操作仍在可见区域。
- `bytehost-settings::git`
  - 三 provider 行状态、敏感输入缓存、请求身份、连接 / 取消 / 断开 / 查询回包、
    部分提交告警与关窗清理。
- `PageRegistry` 别名：`dozer.git` 可映射到内置 `bytehost.git`，不增加导航页。
- 新内置页 `bytehost.git`。

新增 crate `bytehost-git-accounts`：

- provider endpoint / 鉴权 / 用户名 / 仓库列表解析只有一份；
- `CredentialStore` / `MetadataStore` / `HttpTransport` 可注入；
- 默认 feature 不含网络 / Keychain；`http` / `keyring` 显式打开；
- `AccountService` 覆盖查询 / 连接 / 断开 / 仓库列表；凭据与元数据非原子提交时
  返回可见的部分结果；`RequestLedger` 过滤乱序回包；
- `SecretToken` Debug 打码、Drop 清零。

`bytehost-settings-ui`：

- 新增 `git_accounts` 共享视图（三 provider、遮盖输入、连接 / 取消 / 断开、忙碌、
  错误 / 重试、部分提交提示）；
- `composition` 示例注册 `bytehost.git` + `dozer.git` alias 并渲染 Git 页。

`bytehost-desktop::dialog`：

- 复用 iced 公共 `window::run` 取得原生句柄；
- macOS 上把设置窗挂成主窗的 AppKit 子窗口，并把 frame 对齐父窗口；
- `attach_child` / `sync_child` / `detach_child` 不依赖 iced 私有结构，也不创建
  第二条事件循环；其他平台返回显式 `UnsupportedPlatform`。
- `scripts/check-deps.sh` 白名单更新：纯设置层允许默认 feature 的
  `bytehost-git-accounts`，仍禁止网络 / Keychain 实现。

### Digger

- 设置窗改为透明、无标题栏、不可缩放的父所属 overlay：由 iced 创建，再用
  `bytehost-desktop::dialog` 挂成主窗子窗口；窗口 frame 跟随父窗 resize。
- 生命周期改由共享 `DialogController` 驱动；重复打开 / `Activate` / 关闭 /
  绑定失败 / 显式重试 / 父窗退出都走同一协议。
- 原生结果携带完整 `DialogKey`，创建与关闭按 key 定位实例；旧会话的失败 /
  关闭不会误伤新窗口。绑定失败保留可重试的 `Failed` 会话，主窗口同时显示
  错误与重试入口。
- Git 服务接入共享 `Admission`；取消 / 关窗撤销提交准入，迟到但已持久化的
  结果触发权威查询。输入消息使用 `SecretToken`，HTTP 错误文本脱敏。
- 设置遮罩与卡片按共享 `DialogGeometry` 居中；全局 iced 清屏透明，主窗口根容器
  自己铺底色。
- 注册 Theme / Git / Apps；Git 页使用共享 `GitPageState` + `bytehost-settings-ui`
  视图，服务注入 `digger-git` Keychain 与 Digger 自己的 `git_accounts.json`。
- 打开设置 / 切到 Git / 重试都会先查询权威账户事实；关窗清理 Token 输入并使在途
  请求失效。
- 未注册语言 / Advanced 占位页。

### Dozer

- `App` 持有共享 `DialogController`，`SettingsOverlay` 只执行 `Create` /
  `Destroy` / `Activate` 并回传实例身份；重复打开、关闭中重开、迟到回包都走
  同一协议，不再按 `app.settings.is_some()` 维护第二套开关策略。
- 设置 overlay 的输入 / 渲染改调共享 `OverlayGpu::{track_and_convert,
  dispatch,redraw}`，不再在产品内手写第二份 `UserInterface` 生命周期。
- Git 页改用共享 `GitPageState` + `bytehost-settings-ui::git_accounts_page`；
  删除产品内 `ConnectState`、`provider_row` 与本地连接调度，只保留
  `dozer-git` service / JSON 路径 / 文案注入。
- 连接 / 断开接入共享 `Admission`：取消与同 provider 新请求会撤销提交准入，
  迟到但已持久化的结果会保留告警并重新查询权威账户事实。
- `dozer.git` 页面改为内置 `bytehost.git`，旧入口通过 registry alias 继续可用；
  导航仍是原来的五页顺序。
- 卡片几何改调共享 `DialogGeometry`，删除产品内重复的 40% / 75% / 560 / 520 常量。
- `git_accounts.rs` 收缩为产品注入层：`dozer-git` service、既有 JSON 路径、
  兼容函数与本地化错误；provider / 解析 / HTTP / Keychain 全部来自
  `bytehost-git-accounts`，不再保留第二份实现。

## 2. 自动化证据

| 范围 | 命令 | 结果 |
|---|---|---|
| 共享设置状态 / 控制器 / 几何 | `cargo test -p bytehost-settings --all-features` | 48 passed |
| Git 账户契约 / 服务 / 请求身份 | `cargo test -p bytehost-git-accounts --all-features` | 30 passed |
| 共享设置视图 / 示例 | `cargo test -p bytehost-settings-ui --all-targets` | 7 passed；composition 输出 `registered 3 pages`；`settings_product` 示例运行通过 |
| 桌面宿主 | `cargo test -p bytehost-desktop --all-features` | 98 passed |
| 依赖方向 | `bash scripts/check-deps.sh` | ok |
| Digger 产品库 | `cargo test -p digger-desktop --lib` | 738 passed |
| Digger 全 target 编译 | `cargo check -p digger-desktop --all-targets` | 通过 |
| Dozer 设置 | `cargo test -p dozer-app settings` | 47 passed |
| Dozer 项目创建 / 会话 | `cargo test -p dozer-app project_create` | 31 passed |
| Dozer Git 账户 | `cargo test -p dozer-app git_accounts` | 5 passed |
| Dozer 全 target 编译 | `cargo check -p dozer-app --all-targets` | 通过 |

新增的确定性用例包括：

- 重复打开只 `Activate`；关闭幂等；
- 创建中关闭 → 迟到 `Opened` 被销毁；
- 旧 session 的 `Opened` / `Failed` 不影响新会话；
- 失败 → 同 session `attempt + 1` 重试；
- 父窗退出 / 销毁超时强制回到 Closed；
- 关闭中重开等 Closed 后新会话；
- 几何：大窗 / 小窗 / 极小窗 / NaN / Infinity；
- Git：迟到连接结果、取消后回包、关窗后回包、部分提交告警、
  乱序查询、断开幂等、三 provider 解析；
- 旧会话绑定成功 / 失败 / 关闭不会打开、失败或关闭新会话；
- 当前绑定失败保留 `Failed` + key，主窗口可见错误可重试；
- 取消 / 关窗撤销服务层准入；网络阶段取消不落盘；迟到已持久化结果触发查询；
- 两个 provider 并发连接保留全部账户；JSON 元数据原子替换；
- Token 输入消息（含外层产品消息路径）Debug 不出现人工标记。

## 3. 验收矩阵

| 编号 | 结果 | 说明 |
|---|---|---|
| A1 | **自动通过** | `check-deps.sh` 通过；纯设置默认依赖闭包不含 reqwest / keyring；desktop / settings-ui 编译通过。 |
| A2 | **代码完成 / 实机未验证** | Digger（iced）与 Dozer（winit）现在共用同一 `DialogController` / 几何 / Git 状态与视图；Digger 使用 iced 公共 `window::run` + AppKit 子窗口，Dozer 的 winit overlay 执行同一组 `Create` / `Destroy`。没有图形会话，未运行真实双产品窗口对比。 |
| A3 | **自动通过 / 实机未执行** | fake 控制器确定性序列 + Digger 产品消息路径回归通过（含旧会话失败 / 关闭、当前失败重试）；未做真实创建 / 绑定故障注入与句柄日志。 |
| A4 | **代码完成 / 截图未执行** | 共享几何 + 共享 shell；无固定条件截图、大字号 / 多屏 DPI 实机记录。 |
| A5 | **未验证** | IME、Esc 顺序、Tab 焦点、原生粘贴需要实机输入法；未运行。 |
| A6 | **未验证** | 父所属原生子窗口在架构上覆盖主窗 wry；未做编辑器 / 预览 / 应用 webview 打开时的实机层级测试。 |
| A7 | **自动通过（fake）** | 三 provider 共用实现与共享视图；Digger / Dozer 都接共享 `Admission`；取消 / 网络阶段取消 / 乱序 / 部分提交 / 重试有 fake 测试；未做真实账户联网。 |
| A8 | **自动通过（部分）** | Dozer 旧 JSON schema 往返、`dozer.git` alias、provider key 保持；Dozer / Digger 路径与 Keychain service 隔离。未读取真实 Keychain。 |
| A9 | **自动通过 / 实机未执行** | Digger 738、Dozer settings 47 + project_create 31；未做审批 / 文件选择 / 关窗重开的实机回归。 |
| A10 | **示例可运行** | composition 示例注册 Theme / Git / 自定义页；新增独立 `settings_product` 示例：主窗口 owner、fake Git 服务、共享控制器 / Git 页 / 取消准入，不依赖 dozer-app / digger-desktop。 |
| A11 | **发布完成** | 2026-11-04 已发布 bytehost `v0.4.23` 与 Dozer `libs-2026-11-04a`；两个消费方的无 patch / locked 验证见[发布记录](2026-11-04-shared-settings-release.md)。 |
| A12 | **部分完成** | 迁移 / 回退边界见 §4；实机受限项已明确标注。 |

## 4. 回退边界与剩余工作

回退：

- Git 账户 schema、`dozer-git` Keychain service、provider key、`git_accounts.json`
  路径均未改变；Dozer 可回退到上一二进制而不迁移数据。
- Dozer `dozer.git` alias 保留，外部直达入口不失效。
- Digger 的 Git 账户使用独立 `digger-git` 命名空间；回退不会读取 Dozer 凭据。
- 本轮没有数据格式变化，回退只需回退代码 / 依赖 pin。

Task 11 的共享发布（R08）已完成；远端 tag、Digger 钉版 / 锁与隔离发行验证见[发布记录](2026-11-04-shared-settings-release.md)。

剩余工作（不能标为整体完成）：

1. **Task 10 实机验收**：A2–A6 的窗口 / IME / wry 层级 / 多屏 DPI；A9 的审批、
   文件选择、关窗重开；R04 的原生生命周期故障注入 / 内存诊断。
2. **计划其余未勾选项**：清理、新项目装配文档与整体验收仍需逐项核对，不随共享版本发布自动标为完成。

审核 R01–R07 的修复已落地并纳入上方自动化证据；本轮完整输出见
[fix-verification.log](artifacts/2026-11-03-unified-settings-dialog-review/fix-verification.log)。
