# 未完成项补齐与复验（2026-11-03）

对应 [计划](../plans/2026-11-02-code-reliability-and-performance.md) 和 [原验收报告](2026-11-02-code-optimization-acceptance.md)。本轮代码和可执行自动化已补齐；整体仍有实机与发行依赖阻碍，不能标为全部验收完成。以下结论覆盖原报告中的同名未完成项。

## 本轮实现

- 同步回包改为 TypeScript 判别联合；生产 JSON 入口统一捕获解析、字段、整数范围、块顺序 / ID、changes 等错误，保留草稿并进入有界恢复。旧 request 在校验载荷前过滤，避免重放消耗新请求恢复预算。Rust / TS 共用增量 fixture。
- 清理台账逐阶段注入 write / sync / rename 错误，验证原文件逐字节不变、临时文件清理、相同意图重试。重复 request 保留已有终态失败，只有明确 retry 改回 Pending。未落盘意图阻止正常退出，关闭页面有“重试保存清理台账”和明确强退入口。
- 项目迁移与 JSON 导入均增加真实子进程 failpoint：提交前 / 后直接退出（86），父进程检查事务事实再重启；导入结果经旧格式导出逐字段还原。增加两个迁移并发竞争测试。
- 归档改用同目录 hard-link + unlink，原子占据归档名称，消除 exists()+rename 覆盖竞争；Unix 可恢复发布归档链接后中止的状态。不支持硬链接的文件系统保持 ArchivePending，不覆盖或删除备份。
- 8 个固定种子 × 100 次随机编辑，逐版本、逐字段比较“应用增量”与“完整结果”。由此修复 `how` 不一致：保留块为本次提交的 Exact，替换区间采用回包值。包括 ID、种类、标题、范围和诊断字段。
- 后台写入用 barrier 验证慢写时提交不等待、QueueFull 立即返回、失败不冒充已提交事实、同一事件重试。另用 SQLite DEFERRABLE 外键制造真实 COMMIT 失败，验证完整回滚。
- 新增生产 Bridge + JSON、Doc load/edit、前端应用和 1k / 10k / 100k SQLite 原始样本入口；真实页面 IPC / paint 基准已接入 harness T25。仅 SelfTest 模式允许注入首次 webview 创建失败，供真实 Retry 按钮验收。
- PR 快速任务排除书籍压力单测和完整属性集，保留单测与代表属性；slow workflow 跑完整 Release 单测、全种子属性集、同机前后性能判定和 SQLite 规模，并上传日志。宿主门禁兼容 macOS Bash 3.2。
- 修正会话并发测试的调度前提：确认十个请求已经开始后倒序完成，继续检验过时结果过滤，不要求执行器启动已被取代的请求。

组合根所有权保持：ProductShell 拥有窗口 / Project / 面板状态；product/routing 负责类型化路由与展示协调，product/subscriptions 负责订阅组合与 identity；原生页面句柄仍由各适配器拥有。拆分模块没有引入第二份状态或动态消息总线。桌面回归覆盖项目归属、过时结果、焦点快照和后台完成消息；实机接缝见末节。

## 自动验证

原始日志和逐样本数据在 [artifacts](artifacts/2026-11-03-code-optimization/environment.json)。所有测试使用临时产品根；没有迁移真实用户数据、修改个人 Cargo 配置或修改兄弟仓库。

| 检查 | 结果 |
|---|---|
| editor Debug 单测 | 47 通过；含计数边界、块身份与 800 次随机增量对照 |
| editor Release | 47 单测、11 完整属性测试、4 协议测试通过；新增增量 fixture 再跑协议测试通过 |
| store + paths 最终全量 | 56 + 13 通过，含崩溃、竞争、COMMIT 失败、索引与回退 |
| app-host | 39 单测、17 embedded、12 install_flow 通过；recovery 6 通过、1 环境失败 |
| desktop 串行全量 | 726 通过（46.83s） |
| TypeScript / Node | tsc 通过，30 测试通过 |
| parser / bundle | 12 fixtures 一致；生产 dist 重建一致；临时破坏隔离副本 dist 后门禁按预期失败（已恢复） |
| 编译 / 格式 | 本地 patch 下 workspace all-targets 通过；cargo fmt、git diff --check、shell 语法检查通过 |
| 性能门禁自检 | 真实样本通过；临时人为放大 5000 块 p95 后按预期失败 |
| 无 patch locked 编译 | 已执行，失败，详见后文；不作为发行通过 |

早期桌面测试在并发编译期间出现两个 hook 启动超时；最终串行全量通过。会话并发测试原先假设过时任务必定启动，已修正并经全量复验。app-host recovery 的失败保留原始结果：沙箱拒绝进程枚举，预期 Killed 得到 Stale；没有跳过或降低该断言。

## 性能证据

同一 arm64 macOS、同一 Release 工具链，基线为优化前 `88fcf4e` 的 editor 源码，当前为本轮工作树。固定混合语料 100 / 1000 / 5000 块，三个位置局部插入；每组 3 次预热、41 个原始样本。初始化 / Doc 测量在 Bridge 计时外，Bridge 计时包含解析入站 JSON、编辑、构造生产回包和序列化。每次局部增量返回 3 块。原始样本包含独立 Doc load/edit 耗时。重建诊断中的 rust-objcopy/libLLVM 警告未阻止程序运行。

| 块数 | 基线 p50 / p95 (ms) | 当前 p50 / p95 (ms) | 最大回包 前 → 后 | p95 改善 |
|---:|---:|---:|---:|---:|
| 100 | 0.0531 / 0.1271 | 0.0201 / 0.0236 | 12,578 → 574 B | 5.39× |
| 1000 | 0.3979 / 0.4222 | 0.0949 / 0.1026 | 128,423 → 590 B | 4.11× |
| 5000 | 1.9688 / 2.1395 | 0.4249 / 0.4566 | 654,765 → 606 B | 4.69× |

5000 块回包减少 99.9074%，Rust p95 ≥ 2×，100 块没有超过 10% 的回退。两轮采样均满足门槛。此结论只覆盖 Rust 生产桥路径；真实 webview 未通过验收。

前端测量使用同一当前状态机处理两种生产回包，包含 JSON 解码、校验和块集合应用，不含 DOM、IPC 或绘制；这是完整 / 增量算法比较，不是旧版页面与新版页面的整页比较。5000 块 p95 见 frontend.jsonl。

SQLite 使用 WAL + FULL，逐条生产事务建立历史，再对分布于历史中的既有 ID upsert；索引查询只取某项目末尾 100ms 范围。每档 41 样本：

| 历史条数 | upsert p50 / p95 (µs) | 查询 p50 / p95 (µs) |
|---:|---:|---:|
| 1000 | 53.79 / 70.92 | 6.46 / 16.17 |
| 10000 | 63.96 / 79.17 | 7.00 / 16.50 |
| 100000 | 56.25 / 64.12 | 6.58 / 16.00 |

该范围内未随历史量线性增长；这不意味着返回全部历史的查询为常数时间。剩余瓶颈是完整文本存储与范围生成仍可随文档长度增长，以及大范围修改的完整回退；当前无需扩大到 Rope / 通用 actor。

## 可重复命令与回退

```bash
# 生产源与产物（本机已有 path patch，使用可写 target / 临时数据根）
CARGO_TARGET_DIR="$PWD/target" cargo fmt --all --check
DIGGER_DATA_DIR="$(mktemp -d)" CARGO_TARGET_DIR="$PWD/target" cargo test -p digger-desktop --lib -- --test-threads=1
CARGO_TARGET_DIR="$PWD/target" cargo test --release -p digger-editor
CARGO_TARGET_DIR="$PWD/target" cargo test -p digger-store -p digger-app-host -p digger-paths
(cd crates/desktop/web && npm ci --no-audit --no-fund && npm run check && npm test)
CARGO_TARGET_DIR="$PWD/target" bash scripts/build-editor-web.sh --check
# 同机前后比较，含性能门槛；先跑 npm test 可额外生成 Node 前端样本
CARGO_TARGET_DIR="$PWD/target" bash scripts/bench-editor-sync.sh /private/tmp/digger-perf
# SQLite 规模（工具自建并清理临时库）
CARGO_TARGET_DIR="$PWD/target" cargo run --release -p digger-store --example usage_bench
```

实际验证以 artifacts 的命令日志为准。本机为保护既有 dirty lock，测试使用隔离源码副本；个人配置里的本地 patch 仍参与开发构建。干净构建另复制源码，排除 `.cargo`，恢复 HEAD 声明的锁，在独立 CARGO_HOME 中只配置 registry 镜像与缓存，执行 `cargo check --locked --offline --workspace --all-targets`。该干净检查没有 path patch，失败原因如下。

回退时先退出 Digger / 停止写入，完整备份 `state/usage.sqlite3` 与可能存在的 `-wal` / `-shm`、旧 JSON `.migrated`、项目新旧库和清理台账；不要用过期来源覆盖新库。可运行：

```bash
cargo run --release -p digger-store --example usage_export -- /backup/usage.sqlite3 /backup/usage-rollback-new.json
```

工具要求现有源库和新的输出路径，经旧 reader 重新校验，保留 SQLite。将导出的 JSON 放回旧版本预期位置前，保留旧文件副本并确认没有新版本进程在写。项目迁移只向前复制和补归档；涉及新版本产生的项目 / 记忆时不支持直接回拷旧 projects.db。损坏清理台账先备份、明确修复后重启，不能以删除台账推断清理已完成；待落盘意图使用产品“重试保存”处理。

## 仍需外部条件的验收

1. **真实窗口 / 进程权限。** 当前沙箱启动测试二进制时出现 hiservices / 系统通知服务连接错误，测试进程没有可访问窗口；`pgrep` 返回 Cannot get process list。Computer Use 权限存在，但无法访问未注册窗口，未操作已运行的用户 Digger。需在正常 macOS 图形终端跑 `scripts/run-editor-selftest.sh`、`lifecycle`、`usage`；首次创建失败重试可用 `DIGGER_EDITOR_FAIL_FIRST_BUILD=1 bash scripts/run-editor-selftest.sh`，点真实“重试”按钮并观察新 generation。完成 IME / undo / 焦点 / 主题 / 切换检查；T25 会打印逐样本 IPC 和 paint 数据。真实输入法仍需人工组合输入验证。app-host recovery 也须在可查询进程的环境重跑。
2. **无 patch 发行依赖。** 干净锁与声明依赖已验证到源码编译，但 byteui `v0.5.1` 没有 `NotebookTabs`；Dozer `libs-2026-11-01e` 没有当前预览测试使用的 `SaveOutcome` 和 `SaveFile.project_root`。本机兄弟仓库已有新 API，但当前钉版没有。需要共享仓库发布兼容的统一快照，再更新所有相关消费者与 lock 并重跑干净 `--locked` 检查和共享依赖门禁。没有跨仓库修改、发布 tag、伪造钉版或移除现有界面功能。原有本地 path patch 导致共享门禁 `lock-not-git`，也不能视为发行通过。
3. **远端 CI。** 当前仓库没有 remote；workflow 已修改，本地等价自动检查如上，没有声称 GitHub Actions 已执行。共享依赖收口后配置 remote 再运行 PR 与 slow workflow。

计划中的“纯移动与行为变化分开提交”是历史实施约束，原实现已经在既有提交中，不能用本轮测试证明，也未重写历史。当前补齐属于可审阅的未提交变更；工作区原有 Cargo.lock 的 source 删除保持原样。
