# 面板开发前最小稳定化收口基线（2026-11-05）

- 日期：2026-11-05
- 状态：基线已记录；本批实现见同目录 plan / review
- 采集环境：macOS 26.6.2、`aarch64-apple-darwin`
- 工具链：`rustc 1.98.1 (48a229cea 2026-09-01)`、`cargo 1.98.1`、`rustup default stable-aarch64-apple-darwin`

## 1. 五仓库快照

采集命令：每个仓库执行 `git rev-parse HEAD`、`git branch --show-current`、`git status --short`、`git rev-parse origin/main`、`git tag --sort=-creatordate`。

| 仓库 | 本地 HEAD | 分支 | origin/main | 说明 |
|---|---|---|---|---|
| bytehost | `386b65f4e8f2d976d7fa61e77c57f79aa07f4a7e` | main | `386b65f` | 与 v0.4.23 tag 同提交；工作区干净 |
| byteui | `e33824578d0f6fa10f168657fe08d882ad6fadc4` | main | `e338245` | 与 v0.5.2 tag 同提交；仅 `?? .dozer/` 工具目录 |
| bytegit | `5472fa553ab34d97d9a1e4055c89d03b48eb4848` | main | `5472fa5` | 与 v0.9.2 tag 同提交；仅 `?? .dozer/` 工具目录 |
| Dozer | `cda41ebf7ca7c24531af23ba36a9224f227d0bad` | main | `648951fc` | 本地比远端多 `4733c629`、`cda41ebf` 两个提交（会话列表修复、0.5.319 / 0.5.320 版本）；工作区干净 |
| Digger | `7d7e16539c1f2349d233bb69d3435639841f7fee` | main | `e82c048` | 本地比远端多 5 个提交（共享设置、语言、用量接 daemon、scale 死锁修复） |

远端 tag 与 SHA：

| 发布依赖 | tag | 提交 |
|---|---|---|
| bytehost | `v0.4.23` | `386b65f4` |
| byteui | `v0.5.2` | `e3382457` |
| bytegit | `v0.9.2` | `5472fa55` |
| Dozer libs | `libs-2026-11-04a` | tag object `6e7e1f8e`，peeled `15c61c9b` |

Dozer 本地 HEAD（`cda41ebf`）晚于 `libs-2026-11-04a`（`15c61c9b`）；Digger 仍按已发布 `libs-2026-11-04a` 消费 Dozer 库，符合“产品源码可晚于共享库 tag”的组合方式。

## 2. 工作区 dirty 清单

采集时（本批实现前）：

- Dozer：干净。
- Digger：
  - `Cargo.lock`：仅因本地 `.cargo/config.toml` path patch 删除共享包的 `source = ...` 行；属于个人联调锁，不纳入本批正式提交。
  - `README.md`：已加入本 spec / plan 链接。
  - `docs/superpowers/plans/2026-11-05-panel-development-readiness.md`、`docs/superpowers/specs/2026-11-05-panel-development-readiness-design.md`：本批文档，未跟踪。
- byteui / bytegit：`?? .dozer/` 为工具目录，不属于源码。

本批实现新增的候选文件（与用户既有修改分开提交）：

- `crates/desktop/src/adapters/usage.rs`
- `crates/desktop/src/adapters/usage_native.rs`
- `crates/desktop/src/product.rs`
- `crates/desktop/src/lifecycle_selftest.rs`
- `crates/desktop/src/adapters/usage.rs` 内置确定性测试（不新增独立测试文件）
- `.github/workflows/consumer-check.yml`
- 本 notes / review 文档

## 3. 共享依赖与 API 核对

Digger `Cargo.toml` 的已发布来源：

- `bytehost-* = v0.4.23`
- `byteui = v0.5.2`
- `bytegit = v0.9.2`
- `dozer-* / dozerd / dozer-client / dozer-mcp = libs-2026-11-04a`

`dozer-panel-usage` 的公开面足够在 Digger 适配层完成本批修复：

- `WorkspaceState` 提供 `rows/loading/agent_filter/begin_refresh`；
- `activation::ActivationCtx` 允许宿主包一层带 scope / epoch 的消息信封；
- `Message::Loaded/LoadFailed` 仍由宿主 `update` 消费，不需要新增 daemon 协议或修改 `UsageService`。

结论：本批不需要共享库发版或修改 `UsageService` / daemon 协议。

## 4. 有效 Cargo 配置与隔离验证设计

个人配置：

- 仓库 `/Users/chrischiang/Projects/CoralProjects/byteboy/digger/.cargo/config.toml`：把 dozer / byteui / bytegit / bytehost 的依赖 patch 到兄弟工作树。
- `~/.cargo/config.toml`：rsproxy registry、`[build] target = "aarch64-apple-darwin"`、`rustc-wrapper = sccache-wrapper`、共享 `target-dir = /Users/chrischiang/Projects/CoralProjects/byteboy/target`。**不含 path patch**。

无 patch 临时验证环境：

1. `rsync` 复制 Digger 与 Dozer 工作树到临时目录，排除 `.git` / `target`。
2. 删除副本里的 `.cargo/`，避免仓库 patch。
3. 保留用户 `CARGO_HOME`（仅 registry 镜像与 sccache，无 patch），用独立 `CARGO_TARGET_DIR` 防止复用开发 patch 产物。
4. 从正式锁出发执行 `--locked`；不隐式改写开发者锁。
5. 记录 `cargo metadata --locked` 的 source 摘要，确认共享包只有 git 来源、无 sibling path。

## 5. 构建方式与数据根隔离

| 产品 | GUI / daemon 构建 | 数据根隔离入口 |
|---|---|---|
| Digger | `scripts/package-macos.sh release`（`digger` + `diggerd` + `digger-hook` + `digger-mcp`，ad-hoc 签名 bundle）；GUI 依赖共享 bytehost host | `DIGGER_DATA_DIR=<临时根>`；测试态 `config_dir()` 固定到 `temp_dir/digger-tests-<pid>` |
| Dozer | `scripts/build-macos-app.sh debug`（release 会自动 bump 版本并跑 i18n 门禁，冒烟用 debug 避免改版本） | 隔离临时 `HOME`（`ProjectDirs` 决定 `~/Library/Application Support/ai.byteboy.dozer`）；不碰真实 `dozerd` |

会话冒烟优先使用 shell；真实 agent 需要的 Token / MCP 配置不写入本仓库，另列范围并避免留档。

## 6. Task 3 隔离验证发现

无 patch 临时副本（删除 `.cargo/`、独立 `CARGO_TARGET_DIR`、使用 HEAD 正式锁）首次验证结果：

1. **Dozer 正式锁滞后于版本号。** `Cargo.lock` 的 `dozer-app` 仍是 `0.5.318`，而 `Cargo.toml` 已是 `0.5.320`；`cargo metadata --locked` 无法通过。已在临时副本显式 resolve，差异只有这一行；正式候选锁应同步为 `0.5.320`。
2. **Dozer settings 测试缺少 bytehost v0.4.23 新字段。** `crates/dozer-app/src/extensions/settings.rs:1705` 构造 `AccountSnapshot` 时缺 `unreconciled_providers`，`cargo check --locked --workspace --all-targets` 在测试目标上失败。该字段由 bytehost v0.4.23 提供，属于真实基线断裂；已补 `unreconciled_providers: Vec::new()`。
3. 修复后 Dozer 无 patch `cargo check --locked --workspace --all-targets` 通过；共享包 metadata 只有 git tag 来源，无 sibling path。

两处都记录为 Dozer 侧候选变更；Digger 本批不依赖共享库发版。

## 7. 本批候选内容边界

- 行为修复：UsagePanel scope epoch / 后台结果信封 / 原生载荷清空 / 产品路由拒绝旧回包。
- 回归：关闭重开、A→B 逆序成功、旧失败、隐藏切换、同域乱序、幂等同步、epoch 耗尽、裸回包旁路。
- CI：Digger 快速集新增用量回归过滤。
- 不包含：共享库发版、daemon 协议变更、用量数据迁移、面板框架重构。
