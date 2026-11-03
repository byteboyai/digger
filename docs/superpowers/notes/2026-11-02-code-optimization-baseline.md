# 代码恢复性 / 性能优化：实施基线

- 日期：2026-11-02
- Spec：[2026-11-02-code-reliability-and-performance-design.md](../specs/2026-11-02-code-reliability-and-performance-design.md)
- Plan：[2026-11-02-code-reliability-and-performance.md](../plans/2026-11-02-code-reliability-and-performance.md)
- 对应 Task 0。

## 1. 仓库与环境

| 项 | 值 |
|---|---|
| HEAD | `88fcf4e0cdd3edf34c967971e62e62190828e01a` |
| 实施开始前 dirty | `Cargo.lock`（24 行 source 删除，来自 `.cargo/config.toml` 本地 path patch）、`README.md`（新增本轮 spec/plan 链接）；spec / plan 未跟踪 |
| 本机 | macOS 26.6.2 (25G83), arm64 |
| Rust | 1.98.1 (48a229cea 2026-09-01) |
| Cargo | 1.98.1 |
| Node / npm | v24.14.0 / 11.11.0 |
| 编辑器 web 工具链 | TypeScript 5.6+、Vite 6、CodeMirror 6（`crates/desktop/web/package-lock.json`） |
| Cargo patch | `.cargo/config.toml` 把 dozer / byteui / bytegit 指向兄弟目录；**未提交**，发布验证必须使用干净 checkout |
| Cargo target | 默认 target 不可写，测试统一用 `CARGO_TARGET_DIR` 指向临时可写目录 |

`Cargo.lock` 的既有未提交修改保留；本轮的测试运行曾让 Cargo 临时加入 `dozer-file-io`（本地 patch 引入），已还原，最终 diff 仍是开始前的 24 行 source 删除。

## 2. F1–F8 复现与证据分类

| 编号 | 结论 | 本轮落地 |
|---|---|---|
| F1 迁移提交与 rename 分离 | 已复现：改名失败后重试主键冲突 | Task 1：`digger_migration_runs` 同事务标记 + 归档待完成 outcome |
| F2 词频 / 片段计数 `u16` | 已复现：`"a ".repeat(65536)` Debug 溢出 | Task 2：统一 `usize`，新增 65535/65536/70000 与中文重复测试 |
| F3 非 stale 拒绝清 sent 留 queued | 已复现：`settled()` 悬挂 | Task 3：请求身份 + 恢复状态机 + 有界超时 + conflict |
| F4 清理台账读失败当空表 | 已复现：损坏后待办消失 | Task 4：类型化 load + dirty/last_error + 原子唯一临时文件 |
| F5 局部解析后仍全量扫描 | 行为路径结论；性能假设由基准验证 | Task 6：`len16` / 字符数缓存、单次有序边界换算；普通确认不再全文 FNV |
| F6 完成消息同步克隆 / 重写全部账本 | 行为路径结论 | Task 8/9：SQLite 独立库 + 后台 worker + 有界队列 |
| F7 创建失败后无法真实重试 | 代码路径结论；未做实机注入 | Task 5：`Idle/Building/Ready/Failed/Closed` + Retry / 重新显示 |
| F8 组合根与 CI | 结构结论 | Task 10/11：routing / subscriptions / native helpers 子模块；PR + slow workflow |

## 3. 基线命令与退出状态

均使用 `CARGO_TARGET_DIR=<临时目录>`、`--offline`（本地 patch 已存在）：

```bash
cargo test -p digger-editor -p digger-store -p digger-app-host -p digger-paths
cargo test -p digger-desktop
cargo check --workspace --all-targets
cd crates/desktop/web && npm ci && npm run check && npm test && npm run fixtures:check
bash scripts/build-editor-web.sh --check
```

实施前基线：editor 35 单测 + 11 属性测试、store 30、app-host 33+17+12+7、paths 12、desktop 722 通过；属性测试约 362s。实施后对应数字见验收报告。

## 4. 固定语料与基准

`crates/editor/examples/sync_bench.rs` 固定 100 / 1000 / 5000 块，中英文 + emoji + 列表 / 引用 / 代码块混合，中位数为 12 次样本。2026-11-02 本机 Release 结果：

| 块数 | 文本字节 | UTF-16 长度 | Doc load | 一次局部编辑 | 完整 blocks + JSON | 回包字节 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 4,962 | 3,762 | 0.45 ms | 0.042 ms | 0.087 ms | 12,678 |
| 1000 | 51,402 | 39,402 | 3.11 ms | 0.096 ms | 0.69 ms | 128,640 |
| 5000 | 265,002 | 205,002 | 8.60 ms | 0.178 ms | 1.75 ms | 656,116 |

这些是库层 Release 数据，不是真实 webview 往返，也不是 CI 门禁。Spec §2 的 Debug 探针（13.4 / 133.8 / 680.5 ms）保留为历史参照，不与本表直接比较。

## 5. 前轮未完成 / 环境受限（历史）

- 图形会话下的 `bash scripts/run-editor-selftest.sh`、真实 webview 性能、macOS 进程查询（`ps`）测试未在本会话执行。
- 发布级“无本地 patch”构建未执行：本机 `.cargo/config.toml` 指向兄弟目录；没有远端 checkout 可用。
- GitHub Actions 未实际运行；本地等价命令全部通过。


## 6. 2026-11-03 完整路径基线补齐

[补齐复验](../reviews/2026-11-03-code-optimization-completion.md) 已使用优化前 `88fcf4e` 源码与当前源码，同机 Release 固定 100 / 1000 / 5000 块，分别采集 Doc load/edit、生产 Bridge + JSON 的逐样本数据与 p50/p95，并另测 Node 前端回包应用。基线源码从本仓库历史提取到临时独立 editor crate，复用当前依赖锁；不把历史 Debug 探针与 Release 直接比较。脚本：`scripts/bench-editor-sync.sh`，门槛：`scripts/check-editor-performance.py`。原始数据、工具链、当前 HEAD、环境失败与限定见复验产物。
