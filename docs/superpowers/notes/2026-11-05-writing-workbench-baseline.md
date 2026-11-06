# 写字台真实文稿基线（2026-11-05）

- 日期：2026-11-05
- 状态：W1 核心已落地；T3 的 flush/IME 屏障与 T5–T8 的完整验收仍待完成
- 规划：[W1 plan](../plans/2026-11-05-writing-workbench-completion.md)
- 环境：macOS 26.6.2 / `aarch64-apple-darwin` / `rustc 1.98.1` / `cargo 1.98.1`

## 1. 代码与测试现状

- 执行基线：Digger `8788b56` + 本轮未提交改动；`Cargo.lock` 仍因个人 path patch 处于 dirty，未纳入正式提交。
- `digger-editor`：`Doc` 已拥有正文、块 ID、`next_id`、`runs`、aliases 与 pending Agent history；`State` 已有 `export_state/import_state`，本轮补了 serde 序列化。编辑协议、增量解析、撤销、语法渐显与主题未改。
- `digger-store`：原有 usage ledger / project registry 不变；新增 `writing` 模块。
- `desktop/markdown`：原先是固定 `sample.md` 的内存示例；本轮改为正常模式空态 + 项目内真实文稿，SelfTest 仍保留独立样例。
- 没有发现另一份已合并的文稿持久化实现；未修改 Dozer 产品或共享库 API。

## 2. 权威数据与身份

| 项 | 决定 |
|---|---|
| 项目身份 | 沿用共享 `ProjectInfo` / `projects.db`；不新建文稿注册表 |
| 文稿库 | `<project>/.digger/project.db`（SQLite，WAL，`foreign_keys=ON`） |
| 库实例 | `meta.store_uuid`（UUIDv7）；目录复制/移动时随库文件走 |
| schema | `meta.schema_version=1`；更高版本返回 `UnsupportedSchema`，只读拒绝写入 |
| DocumentId | UUIDv7，创建后不因标题/路径变化复用 |
| durable revision | `documents.durable_revision`，每次 Save 事务 +1 |
| buffer revision | 页面/会话水位，只用于恢复草稿与 dirty 判断，不写 durable revision |
| project revision | `project_revision.value`，每个 Commit 事务 +1 |
| operation ID | Save 的幂等身份；与请求指纹（document、expected revision、快照摘要、文件基线）绑定 |

打开项目**不会**自动建库，也不会自动导入全部 Markdown；首次新建/导入才创建 Writing 库。已有损坏库/未来 schema 返回结构化错误，不静默建空库或覆盖。

## 3. 最小事务 schema

`crates/store/src/writing/store.rs` 落地：

- `documents`：id、title、relative_path（唯一）、durable_revision、newline、bom、mirror_revision、mirror_digest、时间。
- `document_versions`：document_id + revision 主键，正文、`state_json`、schema、内容摘要。
- `commits`：actor、operation_id、base_revision、revision、digest、时间；`(document_id, operation_id)` 唯一。
- `outbox`：document/revision/path、expected_digest、target_digest、state（pending/retry/blocked/synced）、attempts、last_error；`(document_id, revision)` 唯一。
- `idempotency`：operation_id 主键、请求指纹、原 commit/revision/digest。
- `project_revision`：单行计数器。

Save 在 `BEGIN IMMEDIATE` 中完成：幂等检查 → revision CAS → 插入版本 → 更新 durable revision → Commit → project revision → Outbox → 幂等结果。任何一步失败整体回滚；同 operation ID 同指纹返回原回执，不同指纹返回 `OperationConflict`。

## 4. 首次导入 / 重复路径 / 副本 / 替换 / 备份

- 新建或导入使用同一 `create` 事务；`relative_path` 已存在时返回 `Existing(document)`，不覆盖、不复制。
- 导入先读取源文件，记录 BOM/CRLF，库内正文统一 LF；非法 UTF-8、超限、路径逃逸/符号链接拒绝。
- 复制项目目录会连同 `store_uuid` 一起复制：两个副本是**独立库实例**，不做跨库同步；UI 按当前项目路径各自打开，不混用回包。
- 库文件被替换：`store_uuid` 变化即视为新库；旧 Outbox/恢复草稿不会套到新库。
- 备份最小策略：停止写入/排空 Outbox 后复制 `.digger/project.db`、`-wal`/`-shm`（或使用 SQLite backup）与 `.digger/working/`；恢复后重新打开项目，按 `store_uuid` 识别实例。
- 新 schema 不承诺旧二进制可写；旧二进制只读/拒绝。

## 5. 上限与 IO 预算

在该模块固化，测试覆盖超限：

| 项 | 值 | 行为 |
|---|---|---|
| 正文（UTF-8） | 16 MiB | `Limit`，不提交 |
| 块身份状态 JSON | 8 MiB | `Limit`，不提交 |
| 恢复草稿 | 4 MiB | `Limit`，保留原文件 |
| Outbox 单次处理 | 8 条 | 有界；剩余留待下次 |
| 镜像写锁等待 | 5s | `dozer-file-io` 返回超时；Orphan 任务不自动无限重试 |
| 投影文件 | 项目根内常规文件 | 拒绝绝对路径、`..`、`.digger/`、符号链接、硬链接 |

## 6. 共享 file-io 覆盖与边界

复用已发布 `dozer-file-io`：

- `ProjectRoot` 目录句柄 + `O_NOFOLLOW` 逐级打开，路径检查与真正打开是同一对象。
- `FileMetadata` / `Revision` 提供 dev/ino/size/mtime 基线；写入时 `WriteOptions.expected_revision` 做 CAS。
- `write_atomic` 同目录临时文件 + rename + 目录 fsync，保留权限/owner/group/xattr/ACL；区分 `Committed` / `CommittedDurabilityUnknown` / `NotCommitted`。
- 明确不保证非合作同用户恶意进程在最终检查与 rename 之间的写入；W1 以此作为外部冲突边界，不声称绝对无竞争。

## 7. W1 UI 状态

正常模式：

- 无项目：`打开项目后新建或导入文稿`，不展示样例。
- 有项目无文稿：标题/相对路径 + 新建；项目内 `.md` 相对路径 + 导入。
- 打开文稿：顶栏显示项目、文稿标题、保存状态；保存按钮与 Cmd/Ctrl-S 同一条路径。
- 状态：未保存 / 保存中 / 已保存 / 保存失败 / 恢复草稿待处理 / 文稿库错误；镜像另以 `mirror_error` 暴露外部冲突/失败。
- 恢复草稿：显示基于版本，提供“恢复 / 丢弃”；接受后走正常 Save，丢弃不删官方正文。

SelfTest：继续使用独立内存样例与 Agent 替身，不显示会误导为真实文件的样例。

## 8. 11 月 5 日未完成项（历史记录）

当前进展见[11 月 6 日续作](../reviews/2026-11-06-writing-workbench-continuation.md)；以下保留冻结基线时的事实，不作为当前实现状态。

- T3 的页面 sent/queued + IME flush 屏障：当前保存使用 Rust 已确认的 `Doc` 快照，页面未 ACK 的 IME/queued 输入仍可能未进入保存快照。
- 自动恢复草稿目前是编辑后 500ms 一次的简单调度，未实现连续输入 2s 上限、可控时钟测试和恢复预览的完整 UI 流。
- 文件树/预览的“在写字台编辑”显式入口、真实项目/文稿切换的保存/保留/取消三选一协调、创建失败 Retry 的文稿级 UI 尚未接齐。
- T5 的子进程崩溃 failpoint、T6 的 GUI/IME/性能规模、T7 的正式无 patch locked + 备份演练、T8 的 A1–A12 完整报告仍需后续批次。
