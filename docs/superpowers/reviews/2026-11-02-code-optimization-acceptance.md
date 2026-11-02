# 代码恢复性 / 性能优化：验收报告

- 日期：2026-11-02
- Spec：[设计](../specs/2026-11-02-code-reliability-and-performance-design.md)
- Plan：[实施计划](../plans/2026-11-02-code-reliability-and-performance.md)
- 基线：[Task 0 记录](../notes/2026-11-02-code-optimization-baseline.md)

本报告只记录本轮实际执行过的检查。标注为“部分 / 未执行”的项不能当作通过。

## 1. 总结

| 阶段 | 状态 | 说明 |
|---|---|---|
| R 恢复性（A1–A5） | 自动回归通过，实机待验收 | 第二轮残留已修复，见 §8；A3 / A5 真实窗口检查未执行 |
| P 性能（A6–A9） | 部分通过 | 坐标 / 缓存、增量回包、SQLite 账本与 worker 已落地；真实 webview 与 90% / 2x 性能门禁未在固定 Release 环境复测 |
| E 工程质量（A10–A12） | 部分通过 | 组合根子模块、CI 分层已落地；GitHub Actions 与无 patch 发行验证未执行 |

## 2. 验收矩阵

| 编号 | 结果 | 证据 / 命令 | 未覆盖 |
|---|---|---|---|
| A1 | 通过 | `cargo test -p digger-store migrate`：新增“提交后归档失败重启只归档”“旧版半完成逐行核对”“内容不同冲突”“已有归档不覆盖”“并发标记”；另加 R01 并发回归：迁移通过 exists 检查后另一 writer 提交 sentinel，本次失败不得删除目标库；`projects::open_in_with_outcome` 把 ArchivePending 作为提示而非不可用 | 无 |
| A2 | 通过 | `cargo test -p digger-editor --lib repeated_tokens`：65535 / 65536 / 70000 英文重复、中文重复；`a_long_character_diff_is_bounded...` 验证字符 diff 预算与诊断计数；`usize` 覆盖词频 / 片段 / unit 计数 | 未在 Release 下重复跑全部属性集（本地 Debug 通过） |
| A3 | 通过（无窗口） | `cargo test -p digger-editor --lib bridge`：协议版本、request ID 幂等、重复同 ID 异内容、旧 ID、只读快照、结构化错误码；`crates/desktop/web npm test`：非 stale 拒绝不再悬挂、conflict 通知前数据已就绪、草稿基于恢复快照提交、stale 仅在收到完整转换证据时重放一次、连续快照超时进入 failed、空闲旧 ack 不倒退版本、关闭结算 waiter、增量结果校验；共享 fixture 双向验证 | 真实 webview harness 未跑 |
| A4 | 通过 | `cargo test -p digger-app-host cleanups`：损坏 / I/O 读失败类型化、不可读台账只读、写失败 dirty + last_error、同内容重试真正 persist、finish/fail 回滚；desktop `apps::store` 覆盖损坏台账可见并拒绝覆盖；设置页 / 产品顶栏显示不可读与未落盘 | 无 |
| A5 | 通过（单测） | `cargo test -p digger-desktop markdown::`：Failed → Retry 推进 generation、旧 generation 迟到结果被丢弃、连续重试、隐藏 Ready 不重建、关闭进入 Closed | 未在真实窗口注入一次创建失败 |
| A6 | 通过 | `cached_lengths_always_match_a_full_recount`（emoji / 空文档 / 文末插入 / 多端点 / import_state）；`ranges16` 单次有序边界；R08 后 `byte_offsets` 对一批端点单次转换，普通 full / incremental 回包都不计算随后丢弃的全文 FNV（`hash_scans` 回归），open / snapshot 保留完整检查 | 真实 webview 下的 UI 延迟未测 |
| A7 | 部分通过 | `an_incremental_block_result_equals_the_full_block_list`；web `incremental` 替换 / 映射 / 越界恢复测试；`after_edit_message` 在小改动时只回替换区间。回包字节：100 块 12.7 KB、1000 块 128.6 KB、5000 块 656 KB 为**完整**回包；增量回包的实际字节与 p95 未单独采样 | 5000 块局部确认“字节下降 ≥90% / p95 改善 ≥2x”未在同一 Release 环境前后对比；真实 webview 未测 |
| A8 | 通过 | `cargo test -p digger-store usage_`：完整字段 round-trip、项目 / 时间过滤与索引计划、run_id 累计值 upsert、u64 越界拒绝、旧 JSON 一次导入 + 提交后归档、冲突不覆盖、损坏源原样、导出快照 | 10 万记录规模未跑（功能测试使用小样本） |
| A9 | 部分通过 | worker 的截止时间、受控准入竞争、排空和确认测试通过；失败事件原样重试，ProductShell 退出核对未确认 / 失败 / 待结算记录，迟到确认可释放退出；最新 RR01 / RR02 集成结果见 §8 | 队列满的 UI 集成、真实慢盘和退出窗口验收仍未执行 |
| A10 | 通过 | `product/routing.rs`（面板路由、几何、预览同步纯函数）、`product/subscriptions.rs`（订阅构造）拆出；`cargo test -p digger-desktop` 723 项通过 | 启动接线仍在 `product.rs`（未强制拆 `boot.rs`，避免行为变更） |
| A11 | 部分通过 | `.github/workflows/consumer-check.yml` 增加核心 Rust 测试、Node 工具链、`npm run check/test`、fixtures、bundle 一致性；新增 `slow-check.yml`（完整属性集 + Release 基准 + artifact） | GitHub Actions 未实际运行；本地等价命令通过；图形 / 进程测试未跑 |
| A12 | 部分通过 | 本报告 + baseline；`cargo fmt --check`、`cargo check --workspace --all-targets`、核心 / desktop 测试、web 测试与 bundle 检查通过 | 无本地 patch 的隔离 checkout 未执行；回退工具为 `UsageDb::export_legacy_json`（代码 + 单测），未做真实数据演练 |

## 3. 前轮本地命令与结果

本节保留前轮执行记录；直接修复 RR01–RR04 后的结果见第 8 节。

环境：macOS 26.6.2 arm64、Rust 1.98.1、Node 24.14.0；`CARGO_TARGET_DIR` 指向临时可写目录。

```text
cargo fmt --all --check                                   ok
cargo check --workspace --all-targets                     ok（1 个既有 dead_code 警告）
cargo test -p digger-editor -p digger-store \
           -p digger-app-host -p digger-paths             ok
  editor lib            46 passed
  editor properties     11 passed（约 358s）
  editor protocol        4 passed
  store                 47 passed
  app-host              37 + 17 + 12 + 7 passed
  paths                 13 passed
cargo test -p digger-desktop                              ok（723 + 2 + 1 + 1 + 3 passed）
crates/desktop/web: npm run check                         ok
crates/desktop/web: npm test                              20 passed
crates/desktop/web: npm run fixtures:check                 12 fixtures match
bash scripts/build-editor-web.sh --check                   ok
cargo run --release -p digger-editor --example sync_bench 见 baseline §4
```

## 4. 复审修复（R01–R08）

| Finding | 最终改动与文件 | 新增回归 / 命令 | 实际结果 | 剩余限制 |
|---|---|---|---|---|
| R01 | `store/migrate.rs` 删除按旧 `exists` 清理目标库及 sidecar 的逻辑；注入 `after_target_check` 测试接缝 | `a_concurrent_writer_that_commits_after_the_exists_check_is_not_deleted`、`a_failed_migration_does_not_delete_the_target_file`；`cargo test -p digger-store migrate` | 16 项通过；并发 sentinel 保留 | 无 |
| R02 | `desktop/apps/mod.rs` 保留完整 `CompletionEvent`；`UsagePersisted` 携带事件；新增 `RetryUsage` / `failed_usage` / 重试 UI | `a_failed_usage_submission_keeps_the_exact_event_for_retry`；apps 测试 | 失败事件字段不变，重试不新增在途运行 | 队列满 / 确认通道关闭的 UI 集成仍有限 |
| R03 | `web/rust-sync.ts` snapshot 分支先写 `conflictInfo` 再切 state | `the first conflict notification already carries the snapshot and draft` | 首次 conflict 通知即可读完整数据 | 真实 banner 未实机看 |
| R04 | `commitDraft` 使用恢复快照的 version / len16 / blocks；stale 只在 `sentTransformed` 且最多一次时重放，否则请求快照 | 新增 stale 无转换、带转换、第二次 stale、快照长度不同的提交测试 | 草稿按快照 base 提交；无证据不重放 | 未接真实 Bridge 闭环 |
| R05 | 恢复最多 2 次快照，超限进入 failed；新增 `failureInfo` / `retrySync`，load 也有超时 | `continuous snapshot timeouts end in a typed terminal state...` | 有限请求、settled 拒绝、草稿保留、可手动重试 | 真实 webview 未跑 |
| R06 | worker 共享关闭门、统一 deadline、Shutdown 返回 checkpoint 结果；超时不 join；产品合并 JoinError | `shutdown_has_one_deadline_even_when_the_database_is_locked` | 锁库时约 0.2s 返回明确错误 | 产品退出集成未做 |
| R07 | blocks / reject / incremental / snapshot 统一校验 request 身份与 `awaitingLoad` | `a late full acknowledgement while idle cannot move the version backwards`、recovering/conflict 重放用例 | 版本、长度、块列表不倒退 | 无 |
| R08 | `doc.rs` 新增 `byte_offsets` 单次转换；拆分 `blocks_data` 与 `blocks_message`，普通回包不再计算 FNV；新增 `hash_scans` 诊断 | `a_batch_of_utf16_endpoints_converts_in_one_pass`、`a_small_document_full_edit_result_does_not_scan_the_text_for_fnv` | 多端点 emoji 与参考一致；普通确认 FNV 扫描为 0 | 完整基准仍见 A7 |

## 5. 性能数据

`sync_bench`（Release，12 次中位数，本机）：100 块 load 0.45 ms / edit 0.042 ms / full blocks+JSON 0.087 ms；1000 块 3.11 / 0.096 / 0.69 ms；5000 块 8.60 / 0.178 / 1.75 ms。完整回包字节 12,678 / 128,640 / 656,116。

这些数据证明增长路径和库层上限，不是 spec §7.3 的验收：缺少同环境修改前 Release 基线、真实 webview 往返与增量回包字节统计。因此 A7 不标记“通过”。

## 6. 数据迁移与回退

- 项目库：`digger_migration_runs` 完成标记与行复制同事务；归档失败返回 `ArchivePending`，重启只补归档；已有 `.migrated` 不覆盖；旧版半完成只做严格逐行核对。
- 用量库：`state/usage.sqlite3`；WAL + busy timeout + `synchronous=FULL`；旧 `config/usage_ledger.json` 只在启动时导入，事务提交后归档；失败保留源文件；完成后 SQLite 是唯一真相。
- 回退：`UsageDb::export_legacy_json` 在停写后导出经序列化的旧格式快照。该工具尚未在真实用户数据上演练；在新库已有记录时，不能直接用旧 JSON 覆盖。

## 7. 未解决与后续

- 真实 webview harness（`scripts/run-editor-selftest.sh`）与生命周期自检需图形会话；本会话未执行。
- A7 的 90% / 2x 门禁需要固定 Release 环境的前后采样，本轮只有库层基准。
- 慢盘 / 锁库 / 队列满、产品退出 drain 还需故障注入测试。
- 没有远端 checkout 与干净 Cargo 配置，未验证“无本地 patch”发行构建；`.cargo/config.toml` 保留为本地联调配置，未提交。
- Rope / 后台文档 actor 未引入；现有增量窗口与缓存已足够支撑本轮，是否继续优化应由真实 webview 数据决定。

## 8. 第二轮残留直接修复（RR01–RR04）

详见[第二轮 review 第 7 节](2026-11-02-code-optimization-review-round2.md)。本节更新最新实现状态，前面的初次测试数量保留为历史记录。

| 编号 | 本次完成 | 自动验证 |
|---|---|---|
| RR01 | 以准入栅栏同步入队与关闭排空，不再用可被并发提交越过的 Shutdown 命令 | 确定性关闭竞争回归；48 项 store 测试；300 轮原压力探针不再复现丢回包 |
| RR02 | 持久化提交前登记所有事件；正常退出核对 pending / failed / 待结算状态；失败明确阻止退出，迟到成功确认后才允许干净退出 | 失败提交后 worker 正常关闭仍不退出；确认迟到后释放退出；待结算操作阻止退出，均有回归 |
| RR03 | 旧 request ID 在状态变化前统一过滤，不消耗恢复预算、不触发重复 incremental 恢复 | ready / sending 与 recovering / conflict / failed / closed 重放矩阵 |
| RR04 | 生产 ready/open 握手有界等待、错误可见、取消定时器并拒绝迟到 open；保留原始类型化错误 | 无首包超时、两种握手拒绝、成功握手与迟到 open 回归 |

最终执行：26 项 web 测试、48 项 store lib 测试、726 项 desktop lib 测试（显式临时产品数据根、串行）通过；全 workspace all-targets 编译、格式、diff、host-deps、12 组 parser fixtures 与嵌入 bundle 一致性检查通过。原工作区 lock / 本地 patch 保留，Rust 验证使用隔离副本；无 patch / --locked 的发行验证仍未完成。

并行桌面回归中一次宿主启动 / 关闭测试失败，单项重跑和最终串行全量均通过，详见 review。真实 webview、A7 固定环境性能门槛、规模与回退演练、GitHub Actions 和发行依赖验证仍未完成，不将整体标记为验收通过。
