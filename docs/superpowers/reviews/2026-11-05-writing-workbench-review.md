# 写字台 W1 开发审核（2026-11-05）

- 结论：**原审核不通过；修复后 R01–R05、R07 已落实，R06 部分落实（载入/创建/导入仍同步）**。W1 仍为部分实现，未完成项不限于远端 CI 或 GUI 权限。
- 修复记录：见文末「修复状态（2026-11-05）」。
- 后续复核中发现的异步结果/水位与转换问题及其修复，见[第二轮修复记录](2026-11-05-writing-workbench-round2-fixes.md)；W1 整体验收状态仍为部分实施。
- 基线：`8788b56` + 当前未提交实现；未修改生产源码、个人配置或既有锁文件。
- 对照：[Spec](../specs/2026-11-05-writing-workbench-completion-design.md)、[Plan](../plans/2026-11-05-writing-workbench-completion.md)、[开发验收记录](2026-11-05-writing-workbench-acceptance.md)。
- 审核覆盖：保存/文稿切换、Workbench、WritingStore、Recovery、镜像、编辑状态载入及相关产品路由。未进行真实 GUI/IME 或远端 CI 验收。

## Findings

### R01 · P1：切换文稿直接丢弃未保存缓冲；项目切换还吞掉草稿持久化失败

位置：[OpenDocument](../../../crates/desktop/src/markdown/mod.rs:462)、[set_project](../../../crates/desktop/src/markdown/mod.rs:185)、[open_document](../../../crates/desktop/src/markdown/workbench.rs:233)。

点击其它文稿直接调用 `workbench.open_document`，将 current 替换为新文稿并置 dirty=false，随后替换 Bridge。没有保存、写恢复草稿或取消协调。500ms RecoveryTick 尚未执行时，旧正文没有任何持久化副本；旧 tick 后来执行也只会写当前文稿。

项目切换虽尝试写 recovery，却用 `let _ = ...` 忽略失败，随后无条件 close_project、清空错误并替换旧缓冲。草稿超限、磁盘/权限失败会直接失去最后一份内存内容。退出 close 同样没有先 flush/持久化的屏障。

**确定性探针：** A/B 两文稿 → A 内存输入 UNSAVED_A 并置 dirty → 用真实 `Message::OpenDocument(B)` 切换。最终 Bridge 已无 UNSAVED_A，数据库 A 未包含它，A 的 recovery 文件列表为空。已确认实际数据丢失，不是 GUI 未验的推测。

修复要求：所有文稿/项目切换与退出共用转换协调器；先取得最新缓冲并完成“保存”或“保留恢复草稿”的 durable 确认，失败留在旧会话并可取消/重试。用实际 ProductShell 项目和文稿消息覆盖全部路径。

### R02 · P1：首次镜像没有导入基线，能静默覆盖外部新内容

位置：[create Outbox](../../../crates/store/src/writing/store.rs:170)、[write_mirror](../../../crates/store/src/writing/mirror.rs:150)、[Workbench save](../../../crates/desktop/src/markdown/workbench.rs:360)。

导入读取了源文件，却没有将其字节摘要/文件 revision 存为镜像基线；create Outbox 的 expected digest 固定为 None。`write_mirror` 发现目标已经存在时，None 跳过摘要比较，直接把“现在看到的文件”作为 expected revision 再覆盖。

**确定性探针：** 导入 a.md=source → 外部将其改为 EXTERNAL_NEW → 写字台保存 APP_NEW。结果 a.md=APP_NEW，mirror_error=None；外部修改被无提示覆盖。现有外部修改测试仅覆盖成功镜像之后的修改，未覆盖首次镜像窗口。

修复要求：区分“目标必须不存在”和“导入文件的明确基线”；导入事务保存源文件身份/摘要，写入前验证同一基线。新建时目标已存在不得当作无条件覆盖；重放/失败恢复也必须保留基线语义。

### R03 · P1：保存没有页面 flush 屏障，会把不含最后输入的快照标为已保存

位置：[save_current](../../../crates/desktop/src/markdown/mod.rs:245)。

Save 直接 export 当前 Rust Doc，既不等待页面 sent/queued 排空，也不等待 IME 组合完成。`Workbench::save` 成功即 dirty=false/Status::Saved；页面最新输入可能仍未进入该快照。此时退出或切换会留下旧内容，却向用户显示保存成功。

这是开发报告已经承认的未实现项，但生产保存入口已启用，不能视为仅待测试。Rust↔TS 协议目前未新增对应 flush 确认路径。

修复要求：实现带会话/请求/版本身份的 flush 屏障；保存只确认捕获的快照水位，其后的输入继续 dirty。超时保留原页面、拒绝继续关闭/切换；实际队列和中文组字分别测试。

### R04 · P1：恢复旧会话后复用已存在的 operation ID，正常保存失败

位置：[save operation ID](../../../crates/desktop/src/markdown/workbench.rs:357)、[accepted_recovery](../../../crates/desktop/src/markdown/workbench.rs:405)。

operation ID 由 session_id + operation_seq 生成。恢复草稿时把当前 session_id 改成旧草稿的 session_id，却没有恢复旧序号或分配新的保存身份；重启后的 seq 从 0 开始。

**确定性探针：** 旧会话先成功保存一次（旧 session-1）→ 继续输入并写 recovery → 新 Workbench 重开/接受草稿 → 保存。返回 `OperationConflict`，因为再次使用旧 session-1 但内容指纹不同。恢复成功的 UI 随后无法正常保存。

修复要求：恢复文件所属身份和当前保存会话身份分离；新操作用全新稳定 ID，失败重试复用该操作 ID，不能通过修改 session_id 解决恢复文件清理。

### R05 · P1：落后基线的 recovery 被当成当前缓冲保存，绕过版本冲突流程

位置：[accepted_recovery](../../../crates/desktop/src/markdown/workbench.rs:405)、[AcceptRecovery](../../../crates/desktop/src/markdown/mod.rs:481)。

`classify` 虽区分 Behind/Conflict，接受入口并不分支处理；直接载入旧草稿，同时当前 Document 的 expected revision 仍是最新 durable revision。下一次 Save 因此合法通过 CAS，把较新的 Official 正文整体替换为旧草稿。UI 只有“版本落后”的标签和普通“恢复”按钮，没有基线/当前/草稿的审阅或冲突处理；来源基版本在保存请求中丢失。

**确定性探针：** base=1 的 old-draft 留存 → 另一写者提交 NEW_OFFICIAL 至 revision 2 → 重开分类为 Behind → 接受并保存。结果 revision 3 正文为 old-draft，较新正文被覆盖，而没有进入明确冲突候选流程。

修复要求：Behind/Conflict 保留为有来源版本的候选，不能直接进入普通保存；至少提供完整预览/另存和绑定已审阅版本的显式替换动作。继续输入、库再变化时重新校验，未知基线不可直接 Apply。

### R06 · P1：SQLite、文件读取、mirror 和 fsync 都在 UI 更新路径同步执行

位置：[save_current](../../../crates/desktop/src/markdown/mod.rs:258)、[Workbench save](../../../crates/desktop/src/markdown/workbench.rs:361)、[write_recovery_now](../../../crates/desktop/src/markdown/mod.rs:279)。

`Message::Save` 同步调用 SQLite transaction，再同步处理至多 8 个 mirror；单个镜像文件锁最多等待 5s。每个编辑回包还各自排一个 500ms timer，timer 到期后在 update 内同步序列化、写文件、fsync，没有合并或容量限制。项目/文档加载也同步读库。

慢盘、锁竞争、大文稿或持续输入时，事件循环会被阻塞；恢复草稿计时器还会堆积。此处不是“后台 worker 已落地、还没做性能测试”，而是保存和 recovery 调用路径根本没有后台 worker。

修复要求：有界 worker 管理载入、保存、恢复和镜像；每条结果带 store/document/session/request 身份。草稿按可控时钟合并调度，切换/退出显式等待 durable 水位；用暂停 IO 的 fake/barrier 验证 update 不等待、队列满可见和旧结果隔离。

### R07 · P2：块身份状态仅校验 JSON 语法，载入失败静默重建身份

位置：[WritingSnapshot::validate](../../../crates/store/src/writing/types.rs:129)、[WritingStore::load](../../../crates/store/src/writing/store.rs:186)、[load_document_into_bridge](../../../crates/desktop/src/markdown/mod.rs:223)。

validate 允许缺失 text 和全部 State 字段，只要是合法 JSON；没有检查 state schema、块数量/ID 唯一性/next_id/aliases 与正文的语义对应。load 也未验证保存摘要与状态，desktop 对反序列化错误 `.ok()` 后使用纯文本新建 Doc；`Doc::import_state` 的不匹配回退同样未被拦截。

**确定性探针：** snapshot(text="text", state_json="{}", schema=999) 被 create 和 load 正常接受。该数据在 desktop 无法解析为 State，会被按新 Doc 打开；用户看不到损坏/不支持状态，块身份可被重新分配。

修复要求：在纯 editor 提供严格的 State 校验/恢复接口，存储边界执行版本和内容配对校验，载入验证摘要；不支持/损坏数据只读或返回可恢复错误。测试真实完整 State、非法 ID/计数、正文错配和未来 schema；当前 store 测试中的不完整伪 State 不能证明身份保证。

## 验证与证据

使用已有无 patch 隔离目录 `writing-workbench.d0thZs/digger`，审核前确认 workbench/mod/store/mirror 与原工作区字节一致。探针只追加到隔离副本；不触碰用户项目、账号或真实 GUI。

| 本轮检查 | 结果与意义 |
|---|---|
| store `writing::tests` | 15 passed；现有回归通过 |
| desktop `markdown::workbench::tests` | 6 passed；现有回归通过 |
| `review_probe` | 5 passed：断言 R01/R02/R04/R05/R07 缺陷确实存在；**不是修复通过** |
| R03/R06 | 来自真实生产调用路径和缺失协议检查；未冒称进行了 GUI/IME/卡顿实测 |
| 实机、性能、全量套件、远端 CI | 本轮未执行；不复用开发报告数字充作审核方重跑结果 |

[探针源码与命令日志](artifacts/2026-11-05-writing-workbench-review/)已归档。正式修复时要把探针改成断言正确行为的回归；不能将当前用于确认缺陷的 PASS 纳入完成证据。

## 范围与完成状态

开发报告如实声明 W1 部分实施，这一状态应保留。除上述代码问题，文件树/预览显式编辑入口、恢复/外部冲突预览、Outbox 重启协调、真实进程崩溃测试、备份恢复、实机与性能、最终 CI 仍未完成。Plan T4“项目变化先经过草稿/保存协调”的勾选与 R01 不符；T1“严格块状态验证”的勾选与 R07 不符；T7“flush 关键回归已纳入 CI”的勾选也不能代替尚未实现的 flush。

建议先修文稿切换/关闭屏障与有界 IO，然后完成 flush、基线镜像和 recovery 身份/冲突处理，补严格状态校验；再执行剩余 W1 验收。W2–W5 仍维持待实施，不以本轮底层单测通过视为真实写作闭环完成。


## 修复状态（2026-11-05）

修复后新增/转换的回归（原缺陷探针不再作为完成证据，已改成断言正确行为）：

| Finding | 状态 | 修复与回归 |
|---|---|---|
| R01 文稿/项目切换丢缓冲 | 已修复 | `persist_transition_draft` 在 `OpenDocument`/`Create`/`Import`/`set_project`/`close` 前写恢复草稿；失败保留旧会话并显示错误。`switching_documents_persists_a_dirty_buffer_as_recovery`、`a_project_switch_failure_keeps_the_old_session_and_reports_the_error` |
| R02 首次镜像覆盖外部内容 | 已修复 | 导入把源文件摘要写入 Outbox，新建使用 `MirrorBaseline::Absent`；同一文档旧 revision 失败后停止后续 revision。`first_import_mirror_does_not_overwrite_a_file_changed_after_import`、`external_file_change_blocks_the_next_mirror_without_rolling_back_the_save` |
| R03 保存无页面 flush 屏障 | 已修复 | Rust `Flush/Flushed` 协议 + TS `requestFlush` 等待 sent/queued ACK；保存只在确认版本后排后台任务；超时保留输入。`save_waits_for_the_page_flush_and_does_not_commit_before_it`、`a_flush_timeout_keeps_the_document_dirty_and_visible`、TS `a flush barrier reports...` |
| R04 恢复会话复用旧 operation ID | 已修复 | 恢复草稿会话与当前保存会话分离；新保存生成新 operation ID，成功后清理草稿会话。`a_recovered_session_uses_a_new_operation_id_and_can_save` |
| R05 Behind recovery 覆盖新正文 | 已修复 | `accepted_recovery` 只接受 `Recoverable`；Behind/Conflict 无普通恢复入口，改为显式「另存副本」。`behind_recovery_cannot_overwrite_newer_official_state` |
| R06 UI 路径同步 SQLite/镜像/fsync | **部分修复** | 保存、镜像、恢复草稿写入已改为 `spawn_blocking` + 结果消息；恢复计时器合并为单飞。项目/文稿载入、创建/导入仍同步；未加 fake/barrier 卡顿测试 |
| R07 块身份校验过宽/静默重建 | 已修复 | `digger_editor::doc::validate_snapshot` 校验 schema/text/块数/唯一 ID/next_id/aliases/history；store 创建/保存/载入校验，载入比对存储摘要；面板解析失败显示错误而非 `.ok()` 回退。`invalid_or_future_identity_state_is_rejected_not_saved`、`identity_state_with_duplicate_ids_or_bad_next_id_is_rejected`、`load_rejects_a_tampered_stored_digest` |

复验（无 patch 临时副本，正式 git 来源锁；日志见 `artifacts/2026-11-05-writing-workbench/review-fix-gates.log`、`final-review-fix.log`、`store-review-fix.log`、`desktop-review-fix.log`）：

```text
cargo check --locked --workspace --all-targets        # 0
cargo fmt --all --check                               # 0
bash scripts/check-host-deps.sh [--self-test]         # 0 / 15 tests OK
bash scripts/check-shared-deps.sh --offline           # 0 blocks
cargo test --locked -p digger-store -- --test-threads=1    # 74 passed
cargo test --locked -p digger-desktop --lib -- --test-threads=1  # 767 passed
```

本地/Web 复验：`npm test` 32 passed、`npm run check`、fixtures 与嵌入包一致性通过；`run-editor-selftest.sh lifecycle` 11/11。GUI/IME、性能、真实崩溃 failpoint、远端 CI 仍未执行；R06 的载入异步化与卡顿注入测试仍是后续项。
