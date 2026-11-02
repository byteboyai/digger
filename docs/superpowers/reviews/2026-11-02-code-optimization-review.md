# 代码优化复审：开发 agent 接续修复清单

- 日期：2026-11-02
- 审核结论：**暂不通过验收，需继续开发**。
- 状态：本文件记录待修复问题与接续要求；不表示以下修复已经完成。
- Spec：[恢复性、性能与工程质量设计](../specs/2026-11-02-code-reliability-and-performance-design.md)
- Plan：[实施计划](../plans/2026-11-02-code-reliability-and-performance.md)
- 被审核报告：[验收报告](2026-11-02-code-optimization-acceptance.md)
- 历史基线：[Task 0 记录](../notes/2026-11-02-code-optimization-baseline.md)
- 审核对象：HEAD `88fcf4e` 加当前未提交实现；行号为审核时定位，实施前按函数名重新核对。

## 1. 开始前必须知道

1. 工作区包含本轮实现的大量未提交修改和未跟踪文件，其中 `Cargo.lock` 的部分修改早于实施。不得 reset、覆盖或删除这些内容，也不要把所有现有修改一并当成本次复审修复提交。
2. 继续沿用现有 crate 所有权和产品数据隔离。只操作临时测试数据根，不在真实用户数据上验证迁移、故障或回退。
3. 按下面的编号修复并补行为回归。原有测试通过不能关闭 finding；必须覆盖本文指出的失败路径。现有个别测试固定了错误行为，需修改预期而非迁就旧实现。
4. 本轮审核没有修复生产代码。下表的“已复现”描述的是审核探针结果；临时探针不作为交付物，开发 agent 要把它们转为仓库中的可重复测试。
5. 本文件是接续任务说明，不要求自动启动其他 agent，也不要求发布、打 tag 或修改兄弟仓库。

## 2. 优先级与执行顺序

| 编号 | 优先级 | 问题 | 对应验收 | 证据 |
|---|---|---|---|---|
| R01 | P1 | 迁移失败可能删除并发写入者已提交的目标库 | A1 | 已复现真实文件被删除 |
| R02 | P1 | 用量持久化失败丢失原完成事件，无法原样重试 | A9 | 代码路径确认 |
| R03 | P1 | conflict 状态通知早于数据赋值，页面不显示恢复入口 | A3 | 已复现通知读取空数据 |
| R04 | P1 | 提交冲突草稿仍用旧版本；stale 无条件重试 | A3 | 已复现旧 base 重发 |
| R05 | P1 | 快照持续超时没有终态，settled 永久等待 | A3 | 已复现连续五次超时仍 pending |
| R06 | P1 | worker 关闭发送 / join 不受统一超时约束 | A9 | 10 秒限时实际等待约 20.7 秒 |
| R07 | P2 | 空闲时接受旧确认，使版本与长度倒退 | A3、A7 | 已复现版本 3 倒退到 2 |
| R08 | P2 | 普通回包仍计算后丢弃 FNV；端点仍重复从头扫描 | A6 | 代码路径确认 |

建议执行顺序：R01 → R02 → R06 → R03 → R04 → R05 → R07 → R08 → 第 5 节剩余交付。R03–R05、R07 可以在同一同步状态机变更中协调实现，但回归测试与完成证据分别对应各编号。

## 3. 逐项修复要求

### R01 — 不能凭调用前 exists 判定数据库文件归属

**位置：** [migrate.rs](../../../crates/store/src/migrate.rs)，`migrate_legacy_projects`，审核时第 351–355 行；`remove_owned_target`。

**当前行为：** 在锁外保存 `target_existed = target.exists()`，随后任何迁移错误都可能根据这个旧布尔量删除目标库及 sidecar。它没有证明目标库只属于本次调用。

**复现：** 使用一个无效 SQLite legacy 文件和一个尚不存在的 target。让迁移线程与另一个 SQLite 写入线程同时开始；后者创建 sentinel 表并成功提交。审核探针第一次迭代即观察到：

```text
concurrent writer = Ok(())
migration = Err("ATTACH legacy: file is not a database")
committed sentinel db deleted = true
```

**修复要求：**

- [ ] 取消依据旧 exists 结果删除目标库的逻辑；事务回滚不能扩展为删除其他 owner 的文件。
- [ ] 若仍需删除本次创建的失败文件，必须采用可证明的独占创建 / 迁移锁和文件身份验证。仅在失败前再检查一次 exists 仍不成立。
- [ ] 保护所有相关 sidecar，并保留现有真实冲突、旧版半完成核对和已提交后只补归档的规则。
- [ ] 不为了维持 `a_failed_migration_leaves_no_new_digger_db` 的表面预期而删除共享目标。如果安全策略改变失败后的空库清理，先明确更新 spec / 测试中的允许结果。

**必须新增的验证：** 用受控 barrier / 故障注入固定“检查不存在 → 其他 writer 提交 → 本次迁移失败”的时序；目标 sentinel 和已提交内容必须仍存在。随机压力探针只能补充，不能成为唯一回归。另测两次迁移并发和失败后正常重启。

**关闭条件：** A1 的并发数据保护有确定性证据；不存在未经证明归属的文件删除。

### R02 — 保留原完成事件，重试记录不能重执行业务操作

**位置：** [apps/mod.rs](../../../crates/desktop/src/apps/mod.rs)，`UsageRuntime::finish`（约第 120 行）、`settle_usage_for`、`Message::UsagePersisted`；[usage_worker.rs](../../../crates/store/src/usage_worker.rs)，`UsageHandle::record`。

**当前行为：** `running.take(key)` 先移除运行态，完成事件随后被移入 record。队列满、worker 关闭或 SQLite 写失败时，只留下 `Result<bool, String>`；回包没有 run_id 或原事件。界面提示“可重试同一操作”，但没有记录重试入口，重新操作还会产生新的运行身份与副作用。

**修复要求：**

- [ ] 在提交前保存完整、不可变的 CompletionEvent，以 run_id 跟踪 pending / persisted / failed。
- [ ] 提交失败和异步结果都携带可关联的运行身份；只有成功确认后才能释放待确认事件。
- [ ] 提供独立的“重试用量记录”消息与界面入口，重试同一个事件，不重新安装 / 停止 / 卸载应用。
- [ ] 保持 run_id、结束时刻和累计值不变；重复确认 / 重试不重复计数，失败事件在退出时可追踪。
- [ ] 修正文案，不以重新执行业务操作代替持久化恢复。

**必须新增的验证：** 分别注入 QueueFull、Closed、数据库提交失败、确认通道关闭。修复后重试应得到与原事件逐字段相同的记录，业务执行器调用次数不能增加；正常关闭与失败关闭都核对尚未确认事件。

**关闭条件：** 同一事件失败后仍可原样提交并持久化，且查询仅返回已提交事实。

### R03 — 先准备 conflict 数据，再通知页面

**位置：** [rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts)，snapshot 分支（约第 338–348 行）；[cm-engine.ts](../../../crates/desktop/web/src/cm-engine.ts)，onState 回调（约第 285 行）。

**当前行为：** 先执行 `state = "conflict"`，setter 立即调用 onState；此时 conflictInfo 还为空，页面执行隐藏 banner。随后赋值 conflictInfo 没有再次通知。因此状态虽然是 conflict，作者却看不到复制 / 选择按钮。

**审核结果：** 回调观察到 `{ state: "conflict", hasConflict: false }`，回调之后 conflictInfo 才有值。

**修复要求：**

- [ ] 原子地准备快照、草稿和冲突状态；onState 第一次观察 conflict 时数据必须完整。
- [ ] 检查恢复、失败和清除冲突时的通知顺序，不允许 UI 读取半更新状态。
- [ ] 保留草稿；恢复入口不得要求刷新或销毁页面。

**必须新增的验证：** 在纯同步测试中注册真正读取 conflictInfo 的 onState，断言第一次 conflict 通知即可构造界面模型；不能只在 receive 返回之后断言字段。具备 GUI 后再检查真实 banner 和三个按钮。

**关闭条件：** 首次冲突即可显示有效恢复操作，选择和关闭后展示状态一致。

### R04 — 草稿提交基于确认过的快照，stale 重试必须有转换证据

**位置：** [rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts)，`commitDraft`（约第 558 行）和 stale_base 分支（约第 488 行）；[rust-sync.test.mjs](../../../crates/desktop/web/tests/rust-sync.test.mjs)。

**复现：** 初始确认 `version=1, len16=4`，恢复返回 `version=7, len16=8`。选择提交草稿后，实际仍发送 `base=1`、替换 `0..4`。随后连续注入 stale_base，观察到新 request ID 不断增加，但所有请求 base 都仍为 1。

**修复要求：**

- [ ] 用户明确提交草稿时，以对应恢复快照的 version、len16 和确认块集合构建新编辑，同时保留本地草稿文本。
- [ ] 快照到提交之间若 Rust 又发生变化，不得猜测坐标或自动整篇覆盖新的 Agent 修改；重新获取并呈现冲突。
- [ ] stale 只有在已完整接收并转换相关 update 链、且错误对应当前请求时才可自动重试；没有证明时请求快照。
- [ ] 对 stale 重试设明确边界，不能靠每次递增 request ID 避开服务端幂等保护而无限重发。
- [ ] 修正现有测试中的错误断言：“快照是版本 7，但提交仍应 base=1”和“没有 update 也可以旧 base 重放”不能继续作为通过标准。

**必须新增的验证：** 快照的版本和长度均不同；有 / 无完整转换链；快照后新 Agent 修改；迟到 stale 拒绝；连续 stale。断言用户明确提交的草稿最多应用一次，版本正确，无无限重试且不丢本地文本。

**关闭条件：** 针对真实 Bridge 的协议闭环可完成草稿提交，不能只断言前端发出一条 edit。

### R05 — 快照超时必须结束等待，不能重新开始同一无限恢复

**位置：** [rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts)，`armTimeout`、`enterRecovery`（约第 295–324 行）。

**当前行为：** sending 超时和 recovering 超时都进入 enterRecovery，后者创建新 snapshot request、再设置同样的定时器。fromTimeout 参数没有改变此行为；快照拒绝同样可能持续重发。

**复现：** 在纯同步探针中依次触发五次当前定时器，得到 `edit + 5 次 snapshot`；状态仍为 recovering，先前的 settled Promise 仍 pending。

**修复要求：**

- [ ] 对一次恢复设置总截止时间或有限重试次数；连续超时 / 拒绝后进入可见失败终态。
- [ ] 所有已登记等待者以类型化错误结束；终态之后调用 settled 也应立即失败。
- [ ] 保留可复制 / 导出的草稿，并提供受控的人工重试；不要通过清空 sent / queued 隐藏失败。
- [ ] 检查首次握手和 SelfTest load 的等待路径，以及非法回包，不留下没有定时边界的开启过程。

**必须新增的验证：** 用 fake clock 触发编辑超时、连续快照超时、快照拒绝、缺失首包和关闭；无需真实睡眠。断言有限请求数、明确终态、全部 waiter 结算和草稿保留。

**关闭条件：** 在 Rust 永不回复的情况下，恢复也能在配置的边界内结束。

### R06 — 关闭的发送、排空和 join 共享一个真正有效的期限

**位置：** [usage_worker.rs](../../../crates/store/src/usage_worker.rs)，`UsageWorker::shutdown`（约第 123–135 行）、Shutdown 命令处理；[product.rs](../../../crates/desktop/src/product.rs)，退出结果合并（约第 3524–3546 行）。

**当前行为：** Shutdown 的阻塞 send 不受 10 秒计时约束；recv_timeout 失败后又无条件 join，仍等待 worker 处理结束。克隆的 UsageHandle 也没有共同的关闭接收门；checkpoint 错误只打印，产品层对部分任务错误兜底成成功。

**复现：** 先确认 worker 已就绪，再由外部 SQLite 连接持有 BEGIN IMMEDIATE，提交三条写入后调用 shutdown。busy timeout 为每条 5 秒，关闭最终报 10 秒超时，但实际等待约 **20.739 秒**。

**修复要求：**

- [ ] 明确关闭状态并对所有 handle 生效，停新提交与排空边界原子化；不能让 Shutdown 后仍返回已接受的新写入。
- [ ] 为投递停止信号、锁等待、排空、checkpoint 和等待线程使用一个总 deadline。
- [ ] 超时后不能再无条件 join；设计可控取消 / 中断和后台线程生命周期，不能使用不安全的强杀线程方案。
- [ ] 向产品返回 checkpoint、worker panic、spawn_blocking JoinError 和未完成事件，不能默认合并成成功。
- [ ] 失败时主窗口能显示错误并提供退出选择；配合 R02 如实报告未确认记录。

**必须新增的验证：** 可控存储 barrier、队列满、锁库、关闭与提交竞争、checkpoint 失败、线程异常及 ProductShell 退出集成。使用可注入的短 deadline / 时钟测试，不要求每次 CI 等待 20 秒。

**关闭条件：** 关闭在统一期限内返回成功或明确失败；确认过的记录可重启读取，未确认记录不被报告成功。

### R07 — 空闲 / 恢复状态也必须验证回包身份

**位置：** [rust-sync.ts](../../../crates/desktop/web/src/rust-sync.ts)，blocks 分支和 `adoptBlocks`（约第 444–529 行）。

**当前行为：** sent 为空就直接 adoptBlocks，绕过 request ID 和版本检查；可能把迟到确认当成 load 回包，覆盖版本、长度和块列表，甚至清掉 queued。

**复现：** 完成请求 1 → 版本 2，完成请求 2 → 版本 3。空闲时重新投递请求 1 的 blocks，实际版本从 3 退回 2，len16 从 6 退回 5。

**修复要求：**

- [ ] 所有确认先校验请求身份和适用状态，再决定是否应用；旧 / 重复确认忽略，不清理当前请求或草稿。
- [ ] 无 request ID 的初始化 / load 回包仅在相应明确等待状态接收，不能用 sent 为空代替状态判断。
- [ ] full、incremental、reject、snapshot 一起核对过期处理，不能修 full 后让旧 incremental 触发不必要恢复。

**必须新增的验证：** 在 ready、sending、recovering、conflict、failed、closed 各状态重放旧 full / incremental / reject；确认版本、当前身份和未确认输入不变。另验证合法 load / open 仍可完成。

**关闭条件：** 确认版本不会因迟到消息倒退，旧消息不破坏恢复状态。

### R08 — A6 的扫描优化需要补齐，而非只改返回字段

**位置：** [doc.rs](../../../crates/editor/src/doc.rs)，`Doc::edit`（约第 228–238 行）、`after_edit_message` 的 full 分支（约第 746–761 行）、`blocks_message`。

**当前行为：**

1. 每个 from / to 仍调用 byte_offset 从全文开头扫描，没有实现有序端点单次转换。
2. full 确认先调用 blocks_message，内部计算全文 FNV，随后才把 hash 替换成空字符串。少于 8 个块的文档会走此路径；仅清空线上字段没有省掉计算。

**修复要求：**

- [ ] 对一批排序端点单次遍历转换，保留 UTF-16 surrogate、重叠 / 乱序与原子失败语义。
- [ ] 分离块构造和全文检查点计算，普通 full 确认也不计算随后丢弃的 FNV；open / snapshot / 显式校验仍保留全文检查。
- [ ] 保持缓存、Agent 回滚、load / restore 和块 ID 行为，不能用减少校验换取耗时改善。
- [ ] 验收报告 A6 在补齐证据前标记部分完成。

**必须新增的验证：** 含 emoji 的多端点编辑与参考实现比对；覆盖小文档、局部增量和 full fallback。使用可测的工作计数 / 基准证明扫描变化，不用读源码字符串的测试充当行为验证。

**关闭条件：** 端点转换和普通回包满足 spec §7.1；A6 的描述与真实执行路径一致。

## 4. 本轮审核的验证范围

| 检查 | 审核结果 / 限制 |
|---|---|
| `npm run check`、`npm test` | 通过；现有 15 项 web 测试没有覆盖上述通知顺序、重复超时和空闲旧确认等完整场景 |
| store lib 测试 | 隔离副本中 45 项通过；另行临时探针复现 R01 和 R06 |
| editor bridge 测试 | 隔离副本中 14 项通过 |
| 同步补充探针 | 复现 R03、R04、R05、R07；使用编译后的真实 RustSync，不是重写状态机 |
| 原工作区 `--locked --offline` | 无法启动：当前本地依赖图要求增加 `dozer-file-io`，现有 lock 不满足 |
| Rust 验证方式 | 复制当前工作区到临时目录，在副本更新 lock 后测试；未改原 lock 和生产代码 |
| GUI / 全 workspace / 完整慢速属性集 | 本轮未重新执行，不能引用历史通过数量代替本轮证明 |

验收报告与 baseline 已注明本地 patch；本轮 lock 失败不直接认定为本次代码引入，但它说明“当前原工作区可按 CI 的 --locked 命令通过”尚无证据。开发 agent 应记录准确依赖来源，不通过清除用户已有 lock 修改或提交个人路径配置来解决。

## 5. Finding 修复后的剩余交付

R01–R08 关闭不等于整个 spec 自动验收通过。以下原报告明确缺失的工作仍要补齐，或保持未验证状态：

- [ ] **A7：完整性能证据。** 基准需真正调用 Bridge + after_edit_message 的生产路径，分别记录 full / incremental 字节和 p50 / p95；在同环境 Release 下与修改前基线比较。5000 块局部回包字节下降至少 90%、Rust p95 改善至少 2 倍，小语料无稳定超过 10% 的回退。现有只测 full blocks 的 sync_bench 不足以证明这些目标。
- [ ] **A3 / A5 / A7：实机验证。** 生产 webview harness、失败创建后重试、冲突 banner、IME / 撤销、迟到消息隔离及真实往返耗时。没有图形能力时保留未验证，不能关闭相关实机验收。
- [ ] **A8 / A9：规模、回退与退出。** 补 10 万记录、停写导出 / 核对及退出 drain 故障集成；只提供导出方法和小样本测试，不等于回退演练完成。
- [ ] **A11：CI 分层。** PR workflow 当前仍执行完整 `cargo test -p digger-editor ...`，没有真正分离慢速属性 / 书籍测试；slow workflow 不能代替快速代表集。核对实际执行命令和注释，并记录 CI 结果。
- [ ] **A12：无 patch 消费。** 使用隔离配置验证声明依赖和锁文件；保留用户本地配置。共享库 / 网络不可用时记明确阻碍，不伪造 tag 或声称已验证。
- [ ] **文档状态纠正。** A1、A3 当前是已发现实现缺陷，不能写通过；A6 应为部分完成；A9 除未测项外还存在 R02 / R06 的实际缺陷。R 阶段暂不通过，spec / plan 整体应标记部分实施、待复审。

## 6. 推荐验证与交付格式

先运行各 finding 的新增定向回归，再按影响范围扩大验证；不需要每修一项就重复完整 6 分钟属性集。Rust 和 TS 协议改动必须同次重建并提交 `crates/desktop/web/dist/cm.html`。

```bash
# CARGO_TARGET_DIR 使用当前环境允许写入的目录。
# 若 --locked 失败，先记录依赖来源并按第 4 节隔离验证，不直接删掉 flag 冒充同等结果。
cargo test --locked -p digger-store --lib
cargo test --locked -p digger-editor --lib bridge
cargo test --locked -p digger-app-host cleanups
cargo test --locked -p digger-desktop

cd crates/desktop/web
npm run check
npm test
npm run fixtures:check
cd ../../..
bash scripts/build-editor-web.sh --check

cargo fmt --all --check
cargo check --locked --workspace --all-targets
bash scripts/check-host-deps.sh
bash scripts/check-shared-deps.sh --offline
git diff --check
```

完整属性、Release 基准和 GUI 自检按 spec 的分层要求另行执行；实际命令与环境必须写入报告。上面的命令清单不代替 R01–R08 的故障回归。

开发完成后，在原验收报告增加“复审修复”章节，逐项填写：

| Finding | 最终改动与文件 | 新增回归 / 命令 | 实际结果 | 剩余限制 |
|---|---|---|---|---|
| R01–R08（分别一行） | 具体实现 | 测试名称、命令和环境 | 退出状态与必要原始数据 | 无 / 明确未验证项 |

本 review 保留原发现与复现事实，修复状态通过该表和复审证据更新。不要删去未通过记录或把本文复现文本改写成“已修复”。修复后申请复审；只有代码、回归和必需实机 / 性能 / 发行证据满足对应验收，才更新整体完成状态。

---

## 7. 开发修复记录（接续本复审）

本节不删除或改写上面的发现与复现事实，只记录 R01–R08 的修复位置与回归证据。逐项结果同步写入[验收报告](2026-11-02-code-optimization-acceptance.md) 的“复审修复”章节。

| Finding | 修复 | 回归 |
|---|---|---|
| R01 | 删除按旧 `exists` 删除 target / sidecar 的逻辑；并发写入者提交后迁移失败不再删库 | `a_concurrent_writer_that_commits_after_the_exists_check_is_not_deleted`、`a_failed_migration_does_not_delete_the_target_file` |
| R02 | 保留完整 `CompletionEvent`；失败按 run_id 留存；新增 `RetryUsage` 重试同一事件，不重执行业务操作 | `a_failed_usage_submission_keeps_the_exact_event_for_retry` |
| R03 | snapshot 分支先准备 `conflictInfo` 再切 conflict 状态 | `the first conflict notification already carries the snapshot and draft` |
| R04 | `commitDraft` 使用恢复快照的 version / len16 / blocks；stale 仅在收到完整 update 转换证据时重放一次，否则请求快照 | stale 无 / 有转换、第二次 stale、快照长度不同的提交测试 |
| R05 | 恢复最多两次快照后进入 failed；load 有超时；`failureInfo` + `retrySync` 提供可见终态与人工重试 | 连续快照超时测试 |
| R06 | worker 共享关闭门；关闭、排空、checkpoint、join 共用一个 deadline；超时不 join；产品传播 JoinError | 锁库 shutdown deadline 测试 |
| R07 | full / incremental / reject / snapshot 统一校验 request 身份；load 仅在接受 `awaitingLoad` 回包时应用 | 空闲旧 ack / 各状态重放测试 |
| R08 | 新增 `byte_offsets` 单次端点转换；普通 full / incremental 回包不再计算随后丢弃的 FNV；`hash_scans` 诊断 | 多端点 emoji 与普通确认 FNV 扫描测试 |

验证结果：核心 crate 测试、desktop 723 项、web 20 项、fixtures 与嵌入 bundle 一致性均通过；R01–R08 的定向回归包含在上面的测试名称中。A5 / A7 / A11 / A12 的实机、完整性能与无 patch 验证仍未执行，不能据此认为整体验收通过。
