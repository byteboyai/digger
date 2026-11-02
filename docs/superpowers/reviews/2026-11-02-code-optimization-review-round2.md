# 代码优化第二轮复审：仍需修复的边界

- 日期：2026-11-02
- 审核时结论：**暂不通过整体验收**。发现 R02 / R05 / R06 / R07 的残留路径；下文保留当时证据，后续直接修复与验证见第 7 节。
- 第一轮：[Review 与开发修复记录](2026-11-02-code-optimization-review.md)
- 对照：[验收报告](2026-11-02-code-optimization-acceptance.md)、[Spec](../specs/2026-11-02-code-reliability-and-performance-design.md)、[Plan](../plans/2026-11-02-code-reliability-and-performance.md)
- 对象：HEAD `88fcf4e` 加当前未提交实现；行号为本轮审核时的位置，接续开发前重新定位函数。
- 审核阶段只新增本文；后续已按用户要求直接修复生产代码，不代表尚缺实机 / 性能 / 发行证据的整体验收通过。

## 1. 本轮已确认的改善

| 原 Finding | 本轮核查 |
|---|---|
| R01 | 删除按调用前 exists 清理目标库的逻辑；受控并发 sentinel 回归通过，原删库路径已移除 |
| R02 | 完成事件现在可按 run_id 保留并手动重试；但正常退出仍未核对失败 / 未确认事件，见 RR02 |
| R03 | conflictInfo 在通知之前赋值，原通知顺序问题已修复，web 回归通过 |
| R04 | commitDraft 已使用快照版本与长度；无转换证据会请求快照。原旧 base 提交问题已修复，真实 Bridge 闭环尚未验收 |
| R05 | 连续快照超时已有有限重试和失败界面；正常 ready 握手仍未覆盖，见 RR04 |
| R06 | 锁库时限时返回的回归通过；提交准入与 Shutdown 入队仍存在竞态，见 RR01 |
| R07 | 空闲时旧 full ack 不再倒退版本；旧 reject / incremental 的处理尚不完整，见 RR03 |
| R08 | 批量 UTF-16 端点和小文档普通回包零 FNV 扫描的定向回归通过，原问题已修复 |

以上是本轮实际核查范围，不等于 spec 所有性能、GUI 和发行要求均已通过。

## 2. RR01 [P1] — 关闭门与入队不原子，已接受写入可落在 Shutdown 后面

**关联：** 原 R06；A9。

**位置：** [usage_worker.rs](../../../crates/store/src/usage_worker.rs)，`UsageHandle::record` 第 66–75 行、query / import 的同类路径，以及 `shutdown_with_deadline` 第 147–152 行。

**原因：** AtomicBool 只保证标志本身的可见性，不把“检查未关闭”和 try_send 合成一个原子准入过程。当前仍允许：

1. 提交线程读取 closed=false，然后被切走。
2. 关闭线程设置 closed=true，将 Shutdown 放入队列。
3. 提交线程恢复，把 Record 放在 Shutdown 后，向调用方返回 Ok(receiver)。
4. worker 处理 Shutdown 后退出，丢掉后面的命令与回包通道。

**已复现：** 对真实 UsageWorker 进行 32 个线程提交与关闭的竞争，保持全部 receiver 存活，关闭后收取结果：

```text
写入探针：round=60, shutdown=Ok(()), accepted=236, accepted_but_no_reply=3
查询探针：round=5,  shutdown=Ok(()), accepted=248, accepted_but_no_reply=5
```

这是已返回“接受”的命令没有收到执行结果；不是 QueueFull / Closed 被正确拒绝。探针是概率压力复现，仓库回归应另加受控时序。

**接续修复：**

- [ ] 为“检查关闭状态 + 非阻塞入队”建立与关闭过程共用的准入同步；任何成功入队的命令都必须排在最终 Shutdown 之前。
- [ ] 对 record / query / import 一致应用；不能仅再读一次 AtomicBool，或在返回 Ok 后才发现命令会被丢弃。
- [ ] 保留 UI 提交不等待磁盘、有界队列与统一关闭期限。
- [ ] 使用 barrier / 可注入准入接缝，固定“检查 false 后暂停 → 关闭 → 恢复提交”的时序。允许结果只能是明确拒绝，或已接受且收到结果；关闭成功不能遗留取消的 receiver。
- [ ] 写入回归使用唯一 run_id，同时核对确认结果与数据库记录；保留并发压力测试作为补充。

**关闭条件：** 提交与关闭竞争下不存在 Shutdown 后仍返回接受、随后无结果的命令。

## 3. RR02 [P1] — 正常退出仍可丢弃已保留的失败完成事件

**关联：** 原 R02 / R06；A9。

**位置：** [apps/mod.rs](../../../crates/desktop/src/apps/mod.rs)，`failed`、`handle_usage_finish`、`retry_usage_for`、`shutdown`；[product.rs](../../../crates/desktop/src/product.rs)，第 3536–3559 行的退出合并，以及第 3306–3309 行的 ShutdownFinished 成功分支。

**代码路径确认：**

- 写入失败后，事件保存在 UsageRuntime.failed，可通过 RetryUsage 重试，这是有效修复。
- AppsState::shutdown 只等待执行器，不读取 failed，也没有跟踪所有待确认持久化的事件。
- ProductShell 只根据 apps.shutdown 与 worker.shutdown 的结果决定正常退出。worker 可以已报告某次写入失败，但之后 checkpoint / shutdown 成功。
- 此时产品收到 ShutdownFinished(Ok(())) 直接退出，failed 中尚未补写的事件随进程消失。待确认事件若仍只被 iced Task 持有，还存在错误回包晚于关闭结果到达的情况。

因此，报告中“退出流程合并未确认事件”的描述没有对应的产品检查。worker 排空成功不代表所有业务完成事件都持久化成功。

**接续修复：**

- [ ] 在提交之前登记所有未确认完成事件，直到提交成功确认才移除；状态需涵盖 pending / failed，不能只维护失败后的列表。
- [ ] 正常退出在停止新操作、处理已接受结果之后核对这份状态。存在未记录事件时不得直接走干净退出。
- [ ] 提供明确的补写 / 取消退出 / 强制退出选择，按实际可恢复能力设计；强制退出如实说明未确认事件可能丢失。
- [ ] 重试使用原 run_id、结束时刻和累计值，不重执行业务操作。
- [ ] 补产品集成测试：失败写入后 worker 能成功 shutdown、仍有 pending 的写入、错误回包迟到、QueueFull；断言既有失败事件不能因 worker 正常关闭而消失。

**关闭条件：** 所有未确认记录被确认或进入明确的用户可操作退出失败状态，普通关闭不静默丢弃。

## 4. RR03 [P2] — 迟到 reject / incremental 仍会消耗恢复次数或启动错误恢复

**关联：** 原 R07；A3 / A7。

**位置：** [rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts)，incremental 分支第 452–460 行、reject 身份检查第 542–559 行。

**原因：** reject 的过期过滤要求 sentRequest 非空，但进入 recovering 后它已经被清为 null。旧请求的拒绝因此绕过过滤，被当成当前恢复失败；incremental 在 ready 时也先判断无在途请求，再判断是否旧确认。

**已复现的两个序列：**

```text
edit #1 → internal reject #1 → snapshot #2（recovering）
重复投递旧 reject #1 → snapshot #3
再次投递旧 reject #1 → failed（连续恢复失败）
```

即使没有任何新的快照失败，旧拒绝也能耗尽恢复预算。

```text
正常接受 incremental #1 → ready / version 2
重复投递同一 incremental #1 → recovering / 请求 snapshot #2
```

**接续修复：**

- [ ] 先按消息种类、请求身份和当前等待状态判断是否适用，再改变状态。旧请求不能在 sentRequest=null 时获得默认适用资格。
- [ ] 只有对应当前 snapshotRequest 的拒绝能计入该恢复轮次；旧 edit 拒绝不能耗尽快照重试预算。
- [ ] ready 时重复 / 旧 incremental 应忽略，不触发恢复；未来未知确认与旧确认分别处理。
- [ ] 对 ready / sending / recovering / conflict / failed / closed 的 full、incremental、reject、snapshot 重放补完整测试，不再只测旧 full ack。

**关闭条件：** 过期消息不改变版本、当前请求、草稿、恢复次数或恢复状态。

## 5. RR04 [P2] — 正常 ready 握手没有超时，协议拒绝也不会进入失败状态

**关联：** 原 R05；A3。

**位置：** [app.ts](../../../crates/desktop/web/src/app.ts) 第 15–19 行；[rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts) 构造过程、load 定时器，以及第 557–560 行的拒绝处理。

**原因：** 新增超时只在 SelfTest load、edit、snapshot 路径设置。正常应用由 startApp 直接 post ready，RustSync 未登记该握手等待或设置定时器。收到 unsupported_protocol / unauthorized 时，如果尚无 sent / queued，也直接返回。

**已复现：** 新建同步实例，按真实握手失败形式投递 `reject { code: unsupported_protocol }`，结果仍为 `state=opening, failureInfo=null`，没有任何已设置的定时器。Rust 不回复时同样没有超时事件。现有“协议不匹配”测试直接调用 adopt，不覆盖真实 reject 路径。

**接续修复：**

- [ ] 让正常 ready 握手进入同步状态机并设置有界等待，握手成功后取消；关闭 / 重试不能遗留旧定时器。
- [ ] 握手的 unsupported_protocol / unauthorized 等拒绝直接形成适当的可见失败状态，不能以没有在途编辑为由忽略。
- [ ] 对超时、握手拒绝和迟到 open 分别测试，避免恢复中的草稿被无条件重新 adopt。
- [ ] 使用真实 ready → Bridge reject / open 的消息闭环验证；单独调用 adopt 只能作为局部测试。

**关闭条件：** 正常应用无首包或握手被拒绝时，能在边界内结束 opening，显示错误并结算等待。

## 6. 本轮验证与剩余验收

本轮通过：TypeScript 检查、20 项 web 测试、47 项 store lib 测试、14 项 bridge 测试、批量 UTF-16 与小文档 FNV 的 2 项定向测试。补充探针在这些测试之外复现 RR01 / RR03 / RR04；RR02 是对实际产品退出链的代码检查结论。

原工作区 `cargo test --locked --offline -p digger-store --lib` 仍因本地依赖需要增加 `dozer-file-io` 而无法启动。Rust 验证使用当前源码的隔离副本，在副本更新 lock 后完成；没有修改原 lock 或个人 Cargo 配置。这不是无 patch / 声明依赖发行验证。

以下要求仍未完成，不能因为原 finding 有修复代码就将整体标记通过：

- A7：同环境 Release 前后对比、生产增量回包字节 / p95、真实 webview 往返与既定性能门槛。
- A3 / A5：真实握手、冲突 banner、IME / 撤销及创建失败后的窗口重试。
- A8 / A9：规模与回退演练、QueueFull / 慢盘 / 关闭竞争，以及包含失败记录的 ProductShell 退出集成。
- A11：实际 CI；PR 当前仍运行完整 editor 测试，快速代表集与慢速属性测试没有真正分离。
- A12：无本地 patch 的依赖与锁文件验证。

开发 agent 接续时，先修 RR01 / RR02，再修 RR03 / RR04；为各项追加回归与结果到原验收报告，保留此前复现事实。阶段 R / A3 不能仅凭现有 20 项 web 测试标为完全通过；A9 需明确记录本轮仍存在的实现缺陷。实机、性能和发行环境不足的项继续标未验证。

## 7. 用户授权后的直接修复记录

2026-11-02：RR01–RR04 已直接实现并完成下面的自动回归。第 2–5 节描述修复前的问题，不删除原发现。

| 编号 | 最终实现 | 验证 |
|---|---|---|
| RR01 | record / query / import 共用非阻塞准入栅栏；关闭设置停止标志后，worker 先跨过所有正在入队提交的栅栏，再排空队列；移除可被后续提交越过的 Shutdown 命令。连接关闭、checkpoint 和线程结束仍受总 deadline 约束 | `an_admitted_write_finishes_before_shutdown_even_if_enqueued_after_close` 用受控通道暂停已准入写入，关闭后再放行，确认结果与数据库均保留；三类新提交被拒绝。原并发压力探针在 300 轮复测中不再复现接受后无结果 |
| RR02 | submit 前登记不可变完成事件；成功确认后才清除。产品完成后台关闭后仍核对未确认、失败与待结算事件；存在任何一项则显示退出错误与明确的强制退出选择。迟到成功确认可自动释放原本干净的退出 | `a_clean_worker_shutdown_does_not_discard_failed_usage`、`a_late_usage_acknowledgement_releases_an_otherwise_clean_exit`、`an_operation_waiting_for_its_completion_message_also_blocks_clean_exit` |
| RR03 | 收到回包时先统一过滤已经过期的 request ID；恢复中的旧 edit 拒绝不能消耗 snapshot 预算，旧 incremental 不能触发无在途请求恢复；failed / closed 为接收终态 | 新增旧 reject、ready / sending 重复 incremental，以及 recovering / conflict / failed / closed 全类型旧回包重放矩阵 |
| RR04 | 正常页面通过 RustSync.start 发送 ready，登记 opening 等待和超时；拒绝与协议不兼容直接进入可见失败并保留原始错误类型；只在当前握手等待中接收 open，成功或关闭取消定时器 | 新增 ready 无首包超时、unsupported_protocol / unauthorized 拒绝、成功取消超时与迟到 open 不污染编辑的测试；同步更新 cm-engine / RustApi / app 接线和 dist |

验证结果：

- `npm run check`、`npm test`：**26 项通过**。修正了旧协议 fixture 测试中无转换证据仍应重发 edit 的错误预期。
- `cargo test --offline -p digger-store --lib`：**48 项通过**。
- 使用独立临时 `DIGGER_DATA_DIR`，`cargo test --offline -q -p digger-desktop --lib -- --test-threads=1`：**726 项通过**，约 47 秒，包含上述产品退出集成回归。
- 全 workspace / all targets 编译检查、格式检查、diff 检查通过；host-deps 门禁使用支持 mapfile 的 Homebrew Bash 执行并通过。
- 重建 `web/dist/cm.html`；`scripts/build-editor-web.sh --check` 通过，包含 **12 组生产 parser fixtures** 和源码 / 嵌入 bundle 一致性。
- 并行桌面回归曾出现 `closing_during_startup_waits_and_releases_the_root` 失败，单项重跑通过；最终采用上面的串行全量结果。未显式指定临时产品数据根的早期尝试也有启动提示断言失败，不计作通过。
- Rust / bundle 验证在当前源码的隔离副本完成，副本中补齐本地依赖需要的 lock 条目；原工作区 Cargo.lock 和个人路径 patch 未改。此结果不替代无 patch / --locked 发行验证。

本次关闭的是四项代码残留及其自动回归。第 6 节的真实窗口、完整性能、规模 / 回退演练、CI 和无 patch 发行要求仍需后续验收；本次没有在真实用户数据上运行迁移，也没有发布。
