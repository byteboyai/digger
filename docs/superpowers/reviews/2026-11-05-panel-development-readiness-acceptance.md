# 面板开发前最小稳定化收口验收报告（2026-11-05）

- 日期：2026-11-05
- 状态：**部分实施**。A1–A3 已满足；A4 仅本地提交、未推送 / 未跑远端 CI；A5 实机冒烟被环境权限阻断；A6 按实际结果分列。
- Spec：[2026-11-05-panel-development-readiness-design.md](../specs/2026-11-05-panel-development-readiness-design.md)
- 计划：[2026-11-05-panel-development-readiness.md](../plans/2026-11-05-panel-development-readiness.md)
- 基线：[2026-11-05-panel-development-readiness-baseline.md](../notes/2026-11-05-panel-development-readiness-baseline.md)

## 环境与身份

| 项 | 值 |
|---|---|
| 主机 | macOS 26.6.2 / `aarch64-apple-darwin` |
| 工具链 | `rustc 1.98.1 (48a229cea 2026-09-01)` / `cargo 1.98.1` |
| Digger 行为修复 | `1097ab9 fix(usage): isolate background results by scope epoch` |
| Digger CI | `2ad08ba ci: run usage scope lifecycle regressions` |
| Dozer 基线修复 | `685b4386 fix(settings): match bytehost v0.4.23 AccountSnapshot` |
| 共享依赖 | bytehost `v0.4.23` / byteui `v0.5.2` / bytegit `v0.9.2` / Dozer `libs-2026-11-04a`（peeled `15c61c9b`） |
| 隔离验证目录 | `<TMP>/panel-readiness.QLFHub`（删除 `.cargo/`，独立 `CARGO_TARGET_DIR`） |

## 证据归档

命令日志保存在 [`artifacts/2026-11-05-panel-readiness/`](artifacts/2026-11-05-panel-readiness/)：

- Digger：`digger-check.log`、`digger-gates.log`、`digger-probe-*.log`、`digger-lib-serial.log`、`digger-lib-parallel.log`、`digger-consumer-rust.log`、`digger-web.log`、`digger-embedded.log`、`digger-usage-ci-filters.log`、`digger-metadata-summary.txt`、`digger-lifecycle-selftest.log`、`digger-usage-selftest-{new,old}.log`。
- Dozer：`dozer-gates.log`（首次失败证据）、`dozer-check-rerun.log`、`dozer-gates-rerun.log`、`dozer-runtime-tests.log`、`dozer-settings-tests.log`、`dozer-product-isolation.log`、`dozer-metadata-summary.txt`。

## A1 — 基线与既有修改保留

- 五仓库 HEAD、分支、origin/main、tag SHA、dirty 清单见 baseline。
- Digger 原有 `Cargo.lock`（本地 path patch 来源变化）、README、plan/spec 均保留；本批没有 reset / clean，也没有把个人 patch 锁纳入提交。
- 最终 Digger 工作区仍只有 `Cargo.lock` 未提交（预期）。

## A2 — 异步作用域与展示隔离

实现：

- `UsagePanel` 增加独立 `scope_epoch`，`set_scope` 实际变化时先 checked 递增失效旧生命周期，再清空 rows / loading / filter / outbox；相同 scope 幂等。
- 后台结果改为 `UsageResult { scope, epoch, message }`；`apply_result` 在进入共享 `update` 前同时校验 scope 与 epoch。
- 产品消息拆分：`Msg::Usage` 只走本地交互，`Msg::UsageResult` 走带身份的异步入口；`UsagePanel::update` 显式拒绝裸 `Loaded/LoadFailed`。
- 作用域变化同时调用 `Native::clear_payload`，把空载荷写入 Surface，避免 webview 继续显示旧项目图表。
- epoch 耗尽进入不可恢复拒绝态，`content_view` 显示重启提示，`activate` 不再发查询。

新增确定性回归（oneshot + started 信号，5s 超时保护，无固定 sleep 同步）：

| 场景 | 结果 |
|---|---|
| 无项目不查询；A 查询后关闭全部，A 成功迟到仍为空 | 通过 |
| A 关闭再打开同一路径，旧 A 成功不能用新 epoch | 通过 |
| A→B，B 先成功、A 后成功，最终只有 B | 通过 |
| A→B，旧 A 失败不产生 B 通知、不结束 B loading | 通过 |
| 隐藏时 A→B，旧 A 回包被拒绝；再次显示查询 B | 通过 |
| 同域两次刷新乱序保留最新；相同 scope 同步不清数据 | 通过 |
| 切换 scope 清空 rows / filter / loading / outbox | 通过 |
| epoch checked 耗尽拒绝新查询、不回绕 | 通过 |
| 裸 `Loaded` 走交互入口被拒绝 | 通过 |
| 原生 Surface 旧载荷被空载荷替换 | 通过 |
| ProductShell 关闭重开 / 另一 scope / 裸回包路由 | 3 项通过 |

命令与结果：

```text
cargo test -p digger-desktop --lib adapters::usage::tests -- --test-threads=1
# 12 passed

cargo test -p digger-desktop --lib product::tests::usage_route -- --test-threads=1
# 3 passed

cargo test -p digger-desktop --lib -- --test-threads=1
# 749 passed; 0 failed

cargo test -p digger-desktop --lib
# 749 passed; 0 failed
```

## A3 — 无 patch 构建、依赖来源与门禁

隔离环境：复制 Digger / Dozer 工作树，删除副本 `.cargo/`，使用 HEAD 正式锁与独立 `CARGO_TARGET_DIR`；保留用户 `CARGO_HOME`（仅 registry / sccache，无 patch）。

### Digger

```text
cargo metadata --locked --format-version 1
# 35 个 bytehost/byteui/bytegit/dozer 包，全部 git tag 来源，BAD_PATH_SOURCES []

cargo check --locked --workspace --all-targets        # exit 0 / 48s
cargo fmt --all --check                              # exit 0
bash scripts/check-host-deps.sh                      # ok, 3 business crates
bash scripts/check-host-deps.sh --self-test          # 15 tests OK
bash scripts/check-shared-deps.sh --offline          # 0 blocks, 0 warnings
cargo build --locked -p digger-desktop --bin icon_scale_widget_probe
BYTEUI_ICON_SCALE=0.5|1|2 <probe>                    # 三个 scale 均 exit 0
cargo test --locked -p digger-desktop --lib -- --test-threads=1  # 749 passed
cargo test --locked -p digger-desktop --lib                       # 749 passed
```

Consumer 快速集同样通过：

- editor lib（跳过书库压测）、properties deterministic / split-merge。
- `digger-store` / `digger-app-host` / `digger-paths`。
- `python3 scripts/tests/test_editor_selftest.py` 4 tests OK。
- `cargo test --locked -p digger-desktop --lib adapters::usage::tests` 已加入 `.github/workflows/consumer-check.yml`，并执行 `product::tests::usage_route`。
- Web：`npm run check`、`npm test` 30/30、`npm run fixtures:check`、`bash scripts/build-editor-web.sh --check` 全部通过。

### Dozer

首次无 patch 验证发现两处真实基线断裂（详见 baseline §6）：

1. `Cargo.lock` 的 `dozer-app` 仍是 `0.5.318`，`Cargo.toml` 已是 `0.5.320`；`--locked` 无法通过。
2. `crates/dozer-app/src/extensions/settings.rs` 的 `AccountSnapshot` 测试字面量缺少 bytehost v0.4.23 的 `unreconciled_providers`，`cargo check --all-targets` 失败。

已提交 Dozer 最小修复 `685b4386`（补字段 + 正式锁版本同步）。修复后：

```text
cargo metadata --locked --format-version 1
# 11 个 bytehost/byteui/bytegit 包，全部 git tag 来源，BAD_PATH_SOURCES []

cargo check --locked --workspace --all-targets        # exit 0
cargo fmt --all --check                              # exit 0
bash scripts/check-host-deps.sh                      # ok, 8 business crates
bash scripts/check-host-deps.sh --self-test          # 13 tests OK
bash scripts/check-shared-deps.sh --offline          # 0 blocks, 0 warnings
python3 scripts/audit/check_panel_boundary.py        # ok, 205 refs / 5 files
bash scripts/check-i18n.sh                           # exit 0
bash scripts/check-log-scope.sh                      # exit 0
```

Dozer runtime 对应集：

| 范围 | 结果 |
|---|---|
| `dozer-file-io` + `dozer-preview-core` | 29 + 166 passed |
| `dozerd --lib --test runtime_regressions` | 555 + 6 passed |
| `dozerd --test product_isolation`（含 helper 构建） | 5 passed |
| `dozer-client` + `dozer-core --lib` | 70 + 118 passed |
| preview/todo/agent/conversations/services/store-project | 83 / 86 / 410 / 69 / 29 / 33 passed |
| `dozer-app extensions::settings` | 43 passed |
| log scope | 通过 |

## A4 — 最终提交与远端 CI

- Digger 行为修复与 CI 拆分成本地提交：`1097ab9`、`2ad08ba`。
- Dozer 基线修复本地提交：`685b4386`。
- **未推送、未触发远端 CI**：本轮没有获得推送授权，plan 也明确“不授权立即推送 / 发版”。因此没有 CI URL 与 job 结论，不引用旧 SHA 绿灯。
- Digger 快速集已在 workflow 中加入新回归；远端是否实际执行仍需推送后确认。

## A5 — 最小实机冒烟

| 编号 | 结果 | 说明 |
|---|---|---|
| M1 设置打开/关闭/重开、resize、前后台切换 | **未执行** | `agent-desktop permissions` 显示 Accessibility 与 Screen Recording 均 `denied`，本会话无法读取或操作 GUI |
| M2 中文组字、Esc、复制粘贴 | **未执行** | 同上；没有输入法注入与屏幕读取权限 |
| M3 shell 会话输出、输入、resize、重开、显式结束 | **未执行** | 同上 |
| M4 Digger 中文编辑、undo/redo、保存、退出重开 | **未执行** | 同上 |
| M5 A/B 项目用量切换、关闭全部、重开 | **未执行** | 自动回归已覆盖慢回包身份；实机图表未验 |
| M6 正常退出与测试实例清理 | **未执行** | 未启动最终候选 GUI 实例 |

已完成的构建 / 真实窗口侧准备：

```text
bash scripts/package-macos.sh debug
# Bundled: target/aarch64-apple-darwin/bundle/macos/Digger.app

SELFTEST_TIMEOUT=120 bash scripts/run-editor-selftest.sh lifecycle
# SUMMARY shell lifecycle selftest: 11 passed, 0 failed

SELFTEST_TIMEOUT=120 bash scripts/run-editor-selftest.sh usage
# FAIL: the page never became ready
```

`usage` 场景在同一台机器上用改动前已安装的 `/Applications/Digger.app`（0.1.7）复跑也得到相同失败，说明是既有环境 / 用量 webview 就绪问题，不是本轮 scope epoch 改动引入。日志见 `digger-usage-selftest-new.log` / `digger-usage-selftest-old.log`。因此 M2–M5 仍保留未验，不把 shell lifecycle 的 11 项通过当成 M1–M6 通过。

没有用无窗口测试替代上述实机结论，也没有为了造故障扩写注入框架。多屏 / Space / 全屏与历史兼容仍在后续发行清单。

## A6 — 结论与后续范围

已完成：

- 旧回包跨生命周期隔离的确定性实现与回归。
- Digger / Dozer 无 patch、`--locked`、单一共享来源的构建与门禁。
- Dozer 两处真实基线断裂的最小修复。
- Digger 快速集接入新回归。

未完成 / 受限：

- 远端 CI（未推送）。
- M1–M6 实机冒烟（GUI 权限被系统拒绝）。
- 因此本批**不宣布“面板开发基线收口完成”**，Spec 状态保持部分实施；面板功能主线可以在已完成的代码基线上继续，但发布前仍需补 A4 远端结果与 A5 实机结果。

后续保留范围：多屏 / Space / 全屏、完整历史兼容矩阵、性能工程；不作为本批默认门槛。
