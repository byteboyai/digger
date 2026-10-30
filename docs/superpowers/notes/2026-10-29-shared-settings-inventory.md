# 共享设置迁移基线（Task 0）

- 记录时间：2026-10-29
- 计划：[2026-10-29-shared-settings.md](../plans/2026-10-29-shared-settings.md)
- 说明：本文件只记录**实施前**的基线与本机来源；实施后的差异见
  [验收报告](../reviews/2026-10-29-shared-settings-acceptance.md)。

## 1. 三仓库基线

| 仓库 | HEAD | 读取时已有未提交内容 | 本次新增 / 修改（摘要） |
|---|---|---|---|
| `bytehost` | `21ceaa9`（`v0.4.17-3-g21ceaa9`） | 干净 | 新增 `crates/bytehost-settings`、`crates/bytehost-settings-ui`；`Cargo.lock` |
| `dozer` | `08b63c99` | `Cargo.lock`、`crates/dozer-app/Cargo.toml`（版本号 `0.5.308`）、`packaging/macos/Info.plist`、未跟踪 `.qoder/` 与 `web/review-trace/` | 设置页接共享容器；应用页复用共享视图；`dozer-app` 依赖与 `Cargo.lock` |
| `digger` | `192105d` | `Cargo.lock`、`README.md`，以及本次计划 / spec 文档 | 设置窗口、主题偏好、共享应用视图；删除被替代的四个 apps 视图文件 |

构建环境（本机实测）：

```text
rustc 1.98.1 (48a229cea 2026-09-01)
cargo 1.98.1 (797e8a9bc 2026-08-05)
target: aarch64-apple-darwin
共享 target-dir: /Users/chrischiang/Projects/CoralProjects/byteboy/target
rustc-wrapper: sccache-wrapper
```

## 2. 依赖与本地 patch 来源

### Digger

- 声明：bytehost `v0.4.17`、byteui `v0.5.0`、Dozer 库 `libs-2026-10-28g`。
- `.cargo/config.toml`（本地、不提交）：
  - bytehost 全部 crate patch 到 `../bytehost/crates/*`（含本次新增的两个 crate）；
  - Dozer 全部 crate patch 到 `../dozer-browser-find`。原记录里的
    `scratchpad/dozer-find` 已不存在；`dozer-browser-find` 含 Digger 当前需要的
    浏览器页内查找 API（`FindRequest` / `find_open` / `find_field_id` 等），
    而 `libs-2026-10-28g` 与 `dozer` HEAD 都不含。

### Dozer

- 声明：bytehost `v0.4.17`、byteui `v0.5.0`、bytegit `v0.8.1`。
- `.cargo/config.toml`（本地、不提交）：
  - byteui → `../byteui`，bytegit → `../bytegit`；
  - bytehost 全部 crate → `../bytehost/crates/*`（共享设置 crate 尚无 tag）。

### bytehost

- workspace 内 path 依赖；新增 UI crate 通过 `byteui` git tag `v0.5.0` 取色板。
- `bytehost-settings` 的 normal 闭包只有 `serde`（`cargo tree` 实测）。
- `bytehost-settings-ui` 的 normal/build 闭包不含任何 `dozer-*` / `digger-*`。

## 2.5 冻结的公共签名（Task 1 结论）

- `Element` 使用 iced 默认 `iced_renderer::Renderer` 与 `iced_widget::Theme`；
  Dozer / Digger 当前都启用 wgpu，feature 合并后是同一个类型。
- 所有消息回调都要求 `M: Clone`（iced 按钮 / `MouseArea` 的约束）。
- 文本解析回调固定为 `&dyn Fn(&str, &str) -> String`（key, English fallback）；
  产品可注入线程局部 locale 包装，共享层不知道产品 i18n。
- 页面内容 `Element<'a, M>` 直接由产品构造；共享层不用 `Any`、JSON 或 trait object。
- 窗口驱动：Dozer 沿用 `bytehost-desktop` + winit overlay；Digger 使用
  `iced::daemon` 的第二原生窗口。共享 UI 不创建窗口、不依赖 `bytehost-desktop`。

## 3. 迁移清单（源 → 归属）

### 3.1 设置状态与入口

| 源 | 内容 | 归属 |
|---|---|---|
| Dozer `extensions/settings.rs` | `SettingsTab` 固定枚举、页序、默认页 | 产品注册表 `product_registry()`；共享 `PageId` / `PageRegistry` |
| Dozer `settings.rs` `State::selected` | 当前 tab | 共享 `SettingsState.current()` |
| Dozer `settings.rs` `Message::TabSelected/TabHover` | 产品级 tab 点击 | 新增 `PageSelected(PageId)`；保留 `TabSelected(SettingsTab)` 供应用面板直达 |
| Dozer `platform/settings_overlay.rs` | 原生窗口、Esc、失焦 | 仍归产品；只调用共享 `settings_card` |
| Digger `product.rs` | 无设置入口 | 新增顶栏齿轮、`Msg::Settings`、`settings::{State,window,theme}` |
| Digger `main.rs` | 单窗 `iced::application` | `iced::daemon` + `boot` 显式打开主窗 + 第二设置窗 |

### 3.2 主题

| 源 | 内容 | 归属 |
|---|---|---|
| Dozer `settings.rs::scheme_row` | Dark / Light 选项 | 共享 `theme_page` |
| Dozer `apply_sync_message(ThemeSelected)` | `set_scheme` + `color_theme_path` 落盘 | 仍归 Dozer；新增落盘失败可见字段 `theme_error` |
| Digger `boot` | 固定 Dark | `settings::theme::init_notice()` 读取 `<config>/color-theme.json` |
| Digger 设置事件 | 无 | `theme::select` 即时应用 + 落盘，失败保留在设置页 |

### 3.3 语言

| 源 | 内容 | 归属 |
|---|---|---|
| Dozer `LanguageState` / `LanguageStatus` | confirmed / pending / Saving / Failed / Conflict / Unsupported | 共享 `SelectionState<T>` / `SaveStatus`（类型别名保留旧名） |
| Dozer `language_page` | 三选项 + 状态行 | 共享 `language_page` |
| Dozer `app/update.rs` / `app/app.rs` | 权威快照、revision、广播、`set_locale` | 仍归 Dozer；只调用共享状态方法 |
| Digger | 无运行时语言能力 | 不注册语言页 |

### 3.4 应用管理

| 源 | 内容 | 归属 |
|---|---|---|
| Dozer `settings_apps_view.rs` | 运行时、应用行、审批、安装 / 卸载 / 回滚确认、日志、忙碌 / 错误 | 共享 `apps::{view,rows,flow}` |
| Dozer `settings_apps.rs` | `InstallFlowState` re-export | 不变；视图改为薄包装 |
| Digger `apps/management.rs` | 启动状态、通知、清理进度、安装流程 | 顶部产品块保留，主体改用共享 `apps::view` |
| Digger `apps/actions.rs` | 应用行「打开 / 停止 / 回滚 / 卸载」 | 共享行动作 + 产品 `app_leading_actions`（「打开」）；文件删除 |
| Digger `apps/approval.rs` | 审批卡 | 共享 `flow::review_view`；文件删除 |
| Digger `apps/runtime.rs` | 运行时区块 | 共享 runtime rows；文件删除 |
| Digger `apps/logs.rs` | 日志查看器 | 共享 `apps::log_viewer`；文件删除 |
| Digger `apps/store.rs` | 浏览器数据清理 | 仍归产品；设置页展示进度 / 失败 / 重试 |
| `digger-app-host` | 信任审核、prepare/approve、请求台账、用量 | 未改；共享视图只发原始 `install::Message` |

### 3.5 视图差异（Dozer ↔ Digger）

共享部分：

- 运行时探测行、受管版本安装 / 卸载；
- 应用列表版本 / 观测状态、停止 / 回滚 / 日志 / 卸载；
- 安装来源表单（目录 / 压缩包 / URL）、计划审批、运行时安装审批；
- 卸载 / 回滚二次确认、日志查看器、失败与忙碌状态。

产品差异（通过参数 / 回调保留）：

- Dozer：文案走 `dozer.host` 现有 `settings.apps.*` 词表；日志滚动 ID 为
  `app-log-view:{id}`；无产品前置动作。
- Digger：中文文案表在 `settings::shared_text`；日志滚动 ID 为
  `digger.apps.logs.{id}`；应用行有「打开」；`AppSummary.issue` 的宿主权威文本；
  服务不可用 / 通知 / 浏览器数据清理在 `extra_top`。
- 两产品都**不**在共享层保存 `InstallFlowState`：视图借用产品已有的那一份。

## 4. 实施前测试 / 构建基线

| 命令 | 结果 |
|---|---|
| `cargo test -p bytehost-panel -p bytehost-desktop` | 通过（98 + 8 + 3 + 8 + 102 等） |
| `cargo test -p dozer-app` | 通过（1131 passed，2 ignored；现有警告不阻断） |
| `cargo test -p digger-desktop -p digger-paths` | 通过（709 + 集成测试） |
| `bash scripts/check-host-deps.sh` | `ok（审视了 2 个业务 crate）` |

已有失败：无。基线时 `digger/.cargo/config.toml` 的 Dozer patch 指向不存在的
scratchpad，导致 `cargo check` 无法解析依赖；实施时改为 `../dozer-browser-find`。

## 5. 截图 / 可复现状态

本机没有可用的显示器捕获会话，**未取得**截图。实机项（原生层级、IME、粘贴、
高 DPI、失焦保持）在验收报告中明确标为未验证，不以单元测试替代。
