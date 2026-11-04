# 统一设置模块与对话框宿主 — 基线与消费者清单

- 日期：2026-11-03
- 范围：bytehost / Dozer / Digger / byteui（只读核对）
- 结论：本轮记录的是实施前快照；实施后的自动证据见
  [`2026-11-03-unified-settings-dialog-acceptance.md`](../reviews/2026-11-03-unified-settings-dialog-acceptance.md)。
- 本文件不包含真实 PAT、Keychain 内容或产品数据。

## 1. 仓库与构建基线

| 仓库 | HEAD | 实施前 dirty | 锁文件包数 | 备注 |
|---|---|---|---|---|
| bytehost | `729fcaf` | 干净 | 673 | 发布 tag `v0.4.22` |
| Dozer | `b701abeb` | 干净 | 1037 | 库 tag `libs-2026-11-03a` |
| Digger | `e82c048` | `README.md`（既有产品改动）+ 本计划 / spec 未跟踪 | 781 | 2026-11-03 的工作树 |
| byteui | `e338245` | `.dozer/` 未跟踪 | 428 | 发布 tag `v0.5.2` |

环境：

- macOS 26.6.2（25G83），arm64。
- rustc / cargo `1.98.1`（2026-09-01 / 2026-08-05）。
- 本轮没有可交互图形会话的自动化能力，也没有多显示器 / Retina 切换环境；
  因此 Task 0 要求的设置截图、焦点 / IME / wry 层级观察标记为**未执行**。
- 本地联调依赖各仓库**未提交**的 `.cargo/config.toml`：

  - Digger / Dozer 把 `bytehost-*`、`byteui`、`bytegit`、`dozer-*` patch 到兄弟工作树。
  - 这是开发用配置，不进入提交；发布验证必须在无 patch 的隔离 `CARGO_HOME` 重跑（Task 11）。

依赖声明（实施前）：

- Digger：bytehost `v0.4.22`、byteui `v0.5.2`、bytegit `v0.9.2`、
  Dozer 库 `libs-2026-11-03a`。
- Dozer：bytehost `v0.4.22`、byteui `v0.5.2`、bytegit `v0.9.2`。
- bytehost：byteui `v0.5.2`。

## 2. 打开入口、页面与状态 owner

### Digger

| 项目 | 基线 |
|---|---|
| 打开入口 | `Msg::Settings(settings::Message::Open(target))`；顶栏设置按钮在 `product/routing.rs`；应用面板 `Apps::OpenSettings` 直达 `bytehost.apps` |
| 页面 | `bytehost.theme`、`bytehost.apps`（顺序固定） |
| 页面 / 导航 owner | `settings::State.core: SettingsState` + `PageRegistry` |
| 原生窗口 owner | `settings::State.window: Option<iced::window::Id>`；`Msg::Settings(Opened / Escape)` |
| 窗口创建 | `iced::window::open(settings::window::settings_window())`（普通独立窗口 720×640） |
| 关闭 | 产品 `close_settings` / `begin_exit` 手动 `window::close` |
| 背景任务 owner | 应用服务仍由 `crate::apps::AppsState` 持有；设置只触发 `Apps::Opened` |
| 原生视图 | markdown / preview / app webview 由产品适配器管理；设置打开时没有显式阻断信号 |

### Dozer

| 项目 | 基线 |
|---|---|
| 打开入口 | `App::open_settings()`（`Message::SettingsOpen`、项目创建「去设置」等） |
| 页面 | `bytehost.theme`、`dozer.git`、`dozer.advanced`、`bytehost.apps`、`bytehost.language` |
| 页面 / 导航 owner | `extensions::settings::State.core` + `product_registry()` |
| 原生窗口 owner | `platform::window_events::Ready.settings_overlay: Option<SettingsOverlay>`；`app.settings.is_some()` 为唯一开关 |
| 创建 / 关闭 | `sync_settings_overlay` 调 `settings_overlay::sync_action`；`SettingsOverlay::open` 用共享 overlay 建子窗口 |
| 尺寸策略 | `project_create::card_logical_size`：40% / 75%，最小 560×520（产品内重复实现） |
| Git 账户 | `git_accounts.rs`：provider URL / 解析 / `reqwest` / `keyring`（service `dozer-git`）/ `git_accounts.json` |
| 背景任务 owner | `settings::State.connect_tasks`、应用 `settings_apps`、语言 / daemon 操作仍在产品 |
| 原生视图 | 设置 overlay 是主窗子窗口，天然盖住 wry；未统一发出阻断信号 |

### bytehost

| 组件 | 基线 |
|---|---|
| `bytehost-settings` | `PageId` / `PageRegistry` / `SettingsState` / `SelectionState`；无 GUI / 服务依赖 |
| `bytehost-settings-ui` | `settings_shell`、主题 / 语言 / 应用页；不建窗 |
| `bytehost-desktop::overlay` | 建窗 / 定位 / 遮罩几何 / Toast 堆叠 |
| `bytehost-desktop::overlay_gpu` | `overlay` feature 下的 wgpu 管线、`backdrop_card`、IME / 光标辅助 |

## 3. 基线测试

实施前执行过的基线（与本轮改动无关的失败不得静默跳过）：

| 仓库 / 范围 | 命令 | 结果 |
|---|---|---|
| bytehost settings / desktop | `cargo test -p bytehost-settings -p bytehost-desktop` | 通过 |
| Dozer 设置 / 账户 | `cargo test -p dozer-app settings`、`cargo test -p dozer-app git_accounts` | 通过 |
| Digger 桌面库 | `cargo test -p digger-desktop --lib` | 728 passed |
| bytehost 依赖门禁 | `bash scripts/check-deps.sh` | 实施前通过；本轮更新白名单后仍通过 |

## 4. 账户 fixture 与 Keychain 映射

- Dozer 旧元数据文件：`<config_dir>/git_accounts.json`，schema
  `{ "github": {"username": "..."}, "gitlab": ..., "gitee": ... }`；无 Token。
- Dozer Keychain service：`dozer-git`，账户名 = provider key
  (`github` / `gitlab` / `gitee`)。
- Digger 新账户命名空间：`digger-git`，元数据文件
  `<digger config_dir>/git_accounts.json`，与 Dozer 完全隔离。
- 测试只使用 fake credential / metadata / HTTP；不读取真实 Keychain。
- 未导出真实账户数量或 Token；仓库中没有真实凭据 fixture。

## 5. 已知缺口（实施前）

- Digger 设置是普通独立窗口，不是父所属 overlay。
- Dozer 的 Git 账户实现是产品内唯一版本，Digger 没有 Git 页。
- 两产品各自维护打开 / 重开 / 关闭与几何策略；`window_generation` 只存在于
  `SettingsState`，没有和原生窗口身份统一。
- 没有共享的对话框生命周期、失败重试或有界销毁协议。
