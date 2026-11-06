# 写字台异步保存与会话转换：第二轮修复

- 日期：2026-11-05
- 状态：本轮发现的异步结果隔离、缓冲水位、恢复写入顺序和会话转换问题已修复并通过下列自动回归；**W1 整体仍为部分实施**。
- 基线：`8788b56` + 开发方未提交 W1 实现与第一轮修复；本轮保留所有既有修改、个人 Cargo 配置与 dirty 锁文件，没有推送。
- 对照：[Spec](../specs/2026-11-05-writing-workbench-completion-design.md)、[阶段验收](2026-11-05-writing-workbench-acceptance.md)、[第一轮审核](2026-11-05-writing-workbench-review.md)。

## 本轮复核发现及修复

### 1. 旧保存结果会改动新文稿

第一轮异步化后的 `SaveFinished` 没有来源身份，`apply_save` 无条件把回执、snapshot 和 Saved 状态应用到当前文稿；失败时也可能把旧 history 塞入新 Bridge。

修复：后台保存消息携带项目根、store UUID、DocumentId 和会话 epoch；Workbench 与面板双重校验。旧会话结果只保留后台已提交事实，不修改当前 UI/正文/history。文稿重开及接受恢复草稿推进独立会话身份，保存中禁止再次排入保存或切换请求，避免 UI dirty 状态覆盖 Saving 后绕过忙碌判断。

回归：`late_save_completion_cannot_modify_another_document`。

### 2. 旧快照保存成功会清掉保存期间的新输入

原 `apply_save` 无条件 dirty=false，worker 又按文件名清理 recovery；因此旧快照成功可能把较新缓冲标成 Saved，并删除它的恢复副本。

修复：保存计划捕获 buffer revision。回执更新 durable revision，但仅确认捕获的缓冲水位，其后的输入继续 Dirty。恢复清理在文件锁内核对精确的已提交文本/块状态，不能只按 session 文件名删除。

回归：`save_completion_keeps_later_input_dirty_and_preserves_its_recovery`；覆盖保存期间继续输入、拒绝第二次重叠保存和较新草稿保留。

### 3. 后台旧草稿可能覆盖转换时同步写成的新草稿

第一轮后台恢复写入和切换前草稿写入没有共同顺序边界，共用固定临时文件。晚完成的旧任务可能覆盖较新的内容。

修复：RecoveryStore 按目标文件使用跨进程合作写锁，比较 base/buffer revision，拒绝旧水位覆盖新水位；每次文稿会话使用独立 session。转换等待正在进行的恢复写入结束，转换期间不再启动普通恢复 timer 写入。

回归：`delayed_recovery_cannot_overwrite_a_newer_transition_draft`、store `delayed_recovery_write_cannot_replace_a_newer_buffer`。

### 4. 转换只保存 Rust 快照，父项目选择和退出仍可能先行

第一轮 `persist_transition_draft` 未排空页面 sent/queued/IME；Rust dirty=false 也不能证明页面没有输入。产品先改项目再让子面板拒绝切换，会使项目栏显示 B、写字台仍是 A；子面板返回空 Task 也不能阻止产品退出。

修复：

- 文稿切换、新建/导入、恢复候选操作经过同一转换路径；先请求页面 flush，再异步持久化最终草稿，失败留在旧会话。
- 正常项目打开/激活/关闭、列表移除当前项目、删除确认和退出在 ProductShell 改状态之前申请 HostTransition；成功后才重放原消息。失败时项目选择、编辑器和 closing 状态保持一致。
- 页面 flush 等待 IME compositionend 及 sent/queued ACK，转换屏障确认后暂时冻结输入，防止写草稿期间又产生新字符；保存屏障不冻结正常输入。
- 超时或持久化失败解除输入冻结，并取消旧 flush waiter，避免迟到的 compositionend 再把页面锁住。
- 数据库镜像基线查询移入保存 worker，避免在 UI 线程等待后台事务持有的连接 mutex。

回归：

- `transition_flushes_even_a_clean_rust_buffer_before_switching`
- `transition_timeout_cancels_the_action_and_keeps_the_original_document`
- `product::tests::writing_transition_keeps_project_selection_until_flush_and_recovery_complete`
- `product::tests::writing_transition_failure_keeps_both_product_and_editor_in_the_old_project`
- `product::tests::writing_transition_failed_recovery_prevents_normal_exit`
- `product::tests::writing_transition_normal_exit_starts_only_after_the_final_draft_is_durable`
- TS 的 composition/冻结/迟到取消两项回归。

## 验证

使用无个人 patch 的既有 `writing-workbench.d0thZs` 隔离副本，保留正式 git 来源锁；最终源码与工作区核对一致。没有使用当前个人 path 来源锁冒充发行结果。

| 检查 | 结果 |
|---|---|
| `cargo check --locked --offline --workspace --all-targets` | 通过，仅既有 warnings |
| `cargo test --locked --offline -p digger-store -- --test-threads=1` | 75 passed |
| `cargo test --locked --offline -p digger-desktop --lib -- --test-threads=1` | 776 passed，19.95s |
| `cargo test --locked --offline -p digger-editor --lib` | 47 passed，含书籍压力用例 |
| 正式宿主依赖门禁 / 共享依赖门禁 | 通过 / 0 阻断、0 警告 |
| Web `npm test` / `npm run check` | 34 passed / 通过 |
| 生产 parser fixtures | 12 fixtures 一致 |
| 编辑器嵌入页面 | 已根据最终 TS 源码重建 |
| `cargo fmt --all --check`、include 文件独立 rustfmt、`git diff --check` | 通过 |

Rust 命令日志位于 [artifacts](artifacts/2026-11-05-writing-workbench-round2-fixes/)。新增产品转换回归已加入 Consumer check；未推送，未声称远端 CI 执行。

## 剩余范围

项目/文稿载入及新建/导入仍有同步 IO，R06 保留部分完成；W1 的完整有界 IO 池、三选一转换交互、文件树/预览显式编辑入口、Outbox 重启/故障矩阵、真实子进程崩溃、备份恢复演练、真实 GUI/IME、多屏与性能规模、最终远端 CI 仍按原计划待办。本轮 composition 状态机测试不代表真实输入法已验收。

此次修复不将 W1 标为完成，不重写此前原型或第一轮阶段报告的历史证据。
