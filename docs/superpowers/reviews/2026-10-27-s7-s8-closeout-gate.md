# S7/S8 收尾 T1:最终门禁与失败定性

日期:2026-10-27。对象:Digger `main` `669a2fb`(已含 S7b/S7c/S8b/S8c 与 0.1.7)。计划:`dozer/docs/superpowers/plans/2026-10-27-s7-s8-closeout.md` T1。

## 方法

- `git archive HEAD` 导出到 `/private/tmp/claude-501/…/scratchpad/snap`(路径不含 "digger"),独立 `CARGO_TARGET_DIR`,不与其它工作区共享。
- 依赖钉 Dozer `libs-2026-10-26b`;`Cargo.lock` 中 byteui `v0.4.6`。

## 结果

| 检查 | 结果 |
|------|------|
| `cargo build --workspace` | exit 0(仅有既有的 `block v0.1.6` future-incompat 提示) |
| `cargo test --workspace --no-fail-fast` | exit 0,**844 passed / 0 failed / 0 ignored**,与 S8c 报告的核对算式 `808 + 36 = 844` 一致 |
| `cargo clippy --workspace --all-targets` | exit 0 |
| `cargo fmt --check` | exit 0 |
| `check-host-deps.sh` | ok(审视 2 个业务 crate) |
| `check-host-deps.sh --self-test` | ok(注入即失败、恢复即通过) |
| `check-shared-deps.sh` | 0 阻断 / 0 警告;提示本脚本 vendor 自 bytehost v0.4.4,现为 v0.4.12 |
| `check-shell-regions.sh` | ok(byteui 的 7 个外壳区域与 Dozer `workspace.json` 逐项一致) |
| `Cargo.lock` 重复 | byteui / bytegit / iced / dozer-model / dozer-services 各 1 份;`bytehost-*`、`dozer-panel-*` 无重复。其余重复为 windows/objc2/toml 等传递依赖,未处理 |

**脚本在快照里不能直接跑的原因**(不是产品问题):`check-shared-deps.sh` 需要 `.git`,`check-shell-regions.sh` 需要同级 `../dozer`,`--self-test` 是参数不是脚本名。这三项改在真实 Digger 仓库(`main`,无相关改动)里跑,脚本只读。

## 失败测试定性(E2)

三条在干净快照的整套运行里**都没有失败**。再各自单独重跑 10 次,先确认过滤器匹配到了测试(1 / 14 / 1 条),再记结果:

| 测试 | 单独重跑 | 定性 |
|------|----------|------|
| `apps::tests::closing_during_startup_waits_and_releases_the_root` | 10/10 通过 | 仅在并行负载下偶发(S8c 基线一次),本次无法复现。**未定性为竞态或已修复**;保留为「偶发、无复现」 |
| `adapters::agent_hooks::tests`(14 条,含 install-timeout) | 10/10 通过 | 同上,S8b 的 2 次失败本次无法复现 |
| `conversations_shared_daemon_recovery_fetches_once_without_starting_another_lifecycle`(`product.rs:4863`) | 10/10 通过 | S7b/S7c 的失败 3/3 出现在沙盒里(沙盒拒绝 `/bin/ps`,进程身份检查走保守路径);非沙盒环境不失败。**定性为环境相关,不是产品缺陷** |

## 没有做到的

- 前两条没有找到触发条件,因此没有给出修复。要定性只能在并行高负载下反复跑整套,本任务没做。
- 这是一台机器上的一次运行,不等于其它环境。
- Dozer 侧的门禁(`check-i18n.sh`、`check-log-scope.sh`)本任务未涉及,留给 T3b 发 tag 时跑。
