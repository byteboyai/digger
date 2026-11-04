# 统一设置共享版本发布记录

- 日期：2026-11-04；用户明确授权“发布共享版本”。
- 结论：**共享版本已发布；Digger 已更新钉版和锁，双产品无 path patch 的发行自动验证通过。整体规格、原生 GUI / IME / 多屏验收尚未完成。**
- 对应：[Spec](../specs/2026-11-03-unified-settings-dialog-design.md)、[Plan](../plans/2026-11-03-unified-settings-dialog.md)、[第二轮修复](2026-11-03-unified-settings-dialog-fixes.md)。

## 已发布版本

| 仓库 | 远端 tag | 提交 |
|---|---|---|
| bytehost | [v0.4.23](https://github.com/byteboyai/bytehost/tree/v0.4.23) | `386b65f4e8f2d976d7fa61e77c57f79aa07f4a7e` |
| Dozer | [libs-2026-11-04a](https://github.com/byteboyai/dozer/tree/libs-2026-11-04a) | `15c61c9b2ef043525c45926e6647429dc9324aa7` |

两个仓库的 main 和新 tag 均通过正常门禁后原子推送；没有 force push、没有移动远端既有 tag。已用 `git ls-remote` 验证 tag 的 peeled commit；[远端引用证据](artifacts/2026-11-04-shared-release/published-refs.json)。bytehost 按仓库约定以 git tag 发布，crate package version 不调整。Dozer 本次是库快照，不发布应用二进制或提升 App 版本。

byteui 继续统一使用已发布 `v0.5.2`，bytegit 继续使用 `v0.9.2`，本次没有需要发布的改动。Digger 所有 10 个 bytehost 声明、23 个 Dozer 库声明及传递依赖均已统一；[锁定来源](artifacts/2026-11-04-shared-release/digger-resolved-sources.json)与[实际 Cargo metadata 来源](artifacts/2026-11-04-shared-release/digger-metadata-sources.json)确认 35 个共享包均来自对应远端 git，未出现重复来源。iced / wgpu 包及版本与发布前锁一致。

## 发布内容与附带修复

本轮将原工作树的统一设置实现与上一轮增量补丁一并纳入发布：共享 dialog controller / geometry、Git 服务及 UI、provider 全提交串行化、部分提交恢复入口、Dozer 的可失败建窗与 GPU 初始化、IME / Esc 派发及可见重试。RR07 / R08 的“远端 tag 缺新 crate”阻碍已解除。凭据命名空间、provider key 和账户 JSON 格式不变。

发布门禁额外发现并修复：

- bytehost 联合构建脚本漏掉 `bytehost-git-accounts`，已加入并增加参数回归测试。
- 新代码的 Clippy 问题已修复，未放宽 `-D warnings`。
- Dozer pre-push 仍调用已于 `7fb7cc43` 退役的 shell-region 比对脚本；共享外壳已直接消费 ByteUI，因此移除过期调用，共享依赖 / 宿主边界门禁保留并通过。没有用 `--no-verify` 绕过。
- Dozer 增补中英文账户恢复文案。

## 自动验证

| 验证 | 结果 |
|---|---|
| bytehost 发布克隆 fmt、全目标 / 全 feature Clippy（warnings denied） | 通过 |
| bytehost 默认 feature 关闭的 workspace check | 通过 |
| bytehost 最新依赖门禁与故障变异测试 | 通过 |
| bytehost 联合构建脚本测试 | 4 通过 |
| bytehost 本地全 feature 测试 | 973 通过；沙箱禁止 `/bin/ps` 的 3 项旧进程测试另在 v0.4.22 复现，未假称通过 |
| bytehost 远端完整 CI | **成功，976 项 Rust 测试通过**，包括上述 3 项；另含默认 feature、fmt、Clippy 与门禁 |
| bytehost 远端双消费者联合构建 | **成功** |
| Dozer 隔离环境 resolve 后 `check --locked --offline --workspace --all-targets` | 通过；仅既有 warnings |
| Dozer 已发布 bytehost 组合的 settings 测试 | 49 通过 |
| Dozer 远端 Consumer check / runtime-tests | **全部成功** |
| Dozer 宿主依赖门禁 / 自检、共享依赖、面板边界、日志来源、i18n 资源、fmt | 通过 |
| Digger 实际工作区，远端 tag 组合 `check --locked --offline --workspace --all-targets` | 通过；仅既有 warnings |
| Digger 已发布依赖组合 desktop 全量串行 lib | **740 通过，46.51s** |
| Digger 共享依赖、宿主边界 / 自检、fmt、diff check | 通过 |

远端运行：

- [bytehost CI](https://github.com/byteboyai/bytehost/actions/runs/41757302708)：成功。
- [bytehost Joint build](https://github.com/byteboyai/bytehost/actions/runs/41757302693)：成功。
- [Dozer Consumer check](https://github.com/byteboyai/dozer/actions/runs/41757801140)：全部成功；最终状态见留档的 `ci-status.json`。

[原始日志目录](artifacts/2026-11-04-shared-release/)保留完整自动输出。bytehost 的远端完整测试不跳过本地受限项；本地失败 / 基线复现日志同时保留。

## 环境、工作树与重跑

原 bytehost / Dozer 工作树只读，未覆盖其 dirty 改动或本地 HEAD。本轮从正常 git 克隆纳入原实现及审核修复，在可写克隆提交和发布。Dozer 以远端 main `648951fc` 为基线；本地未推送的 App `0.5.317` 版本提交不纳入库发布，也未被改动。[原工作树保留检查](artifacts/2026-11-04-shared-release/preservation-check.json)。

发布工作树位于 `/private/tmp/digger-shared-release-2026-11-04/{bytehost,dozer}`；实际 Digger 的 Cargo.toml / Cargo.lock 已按新 tag 更新。历史 dirty 锁文件保留为 [Cargo.lock.before-release](artifacts/2026-11-04-shared-release/Cargo.lock.before-release)，不回滚或丢弃用户既有修改。本轮未提交、推送 Digger 自身的未提交实现。

个人 `.cargo/config.toml` 保留原样，其中仍有本地开发 patch；在原目录直接运行 Cargo 会应用这些覆盖，不能作为发行验证。Digger 的发行测试从外部 cwd 指定实际 manifest，并使用独立 CARGO_HOME（仅复制缓存和构建设置，无 patch）；Dozer 使用无个人配置的发布克隆。重跑示例：

```bash
cd /private/tmp/digger-shared-release-2026-11-04
export CARGO_HOME="$PWD/cargo-home"
export CARGO_TARGET_DIR=/Users/chrischiang/Projects/CoralProjects/byteboy/digger/target
cargo check --manifest-path /Users/chrischiang/Projects/CoralProjects/byteboy/digger/Cargo.toml --locked --offline --workspace --all-targets
```

原增量补丁仍保留为历史交付证据；已经 checkout 新 tag 的代码包含这些修复，不应再次套用旧补丁。真实父子窗口、输入法、焦点、wry 层级、Space / 全屏 / 多屏及计划其他未勾选项仍需验收；本次发布不等于整份统一设置规格完成。
