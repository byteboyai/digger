# 写字台 W1 实现验收（阶段报告，2026-11-05）

- 日期：2026-11-05
- 状态：**W1 部分实施**。存储事务、恢复草稿、镜像原语与项目内新建/导入/保存入口已落地；T3 的 flush/IME 屏障、T5–T8 的完整跨层/实机/性能/CI 验收未完成。
- Spec：[写字台真实文稿与后续写作能力](../specs/2026-11-05-writing-workbench-completion-design.md)
- 计划：[W1 implementation plan](../plans/2026-11-05-writing-workbench-completion.md)
- 基线：[writing-workbench-baseline](../notes/2026-11-05-writing-workbench-baseline.md)

最新记录（2026-11-06）：[未完成项续作与验证](2026-11-06-writing-workbench-continuation.md)。以下为 11 月 5 日历史阶段报告，当前状态以续作记录为准。

历史补记：[第二轮修复](2026-11-05-writing-workbench-round2-fixes.md)完成异步保存身份/水位隔离、恢复写入顺序和项目/退出转换屏障；最终 store 75、desktop 776、Web 34 项自动测试通过。下文保留初次实施与第一轮修复的阶段快照，W1 整体仍为部分实施。

## 交付物

| 文件 | 变更 |
|---|---|
| `crates/store/src/writing/` | 新增 `WritingStore`、版本/CAS/幂等事务、Outbox、恢复草稿、镜像与源文件读取 |
| `crates/store/src/lib.rs` | 导出 `writing` 模块 |
| `crates/editor/src/doc.rs` | `State` 增加 serde，作为保存快照的块身份载体 |
| `crates/editor/src/bridge.rs` | `export_snapshot/restore_history/replace_document` |
| `crates/desktop/src/markdown/workbench.rs` | 项目/文稿/保存/恢复状态机 |
| `crates/desktop/src/markdown/mod.rs` | 正常模式空态、新建/导入/保存/恢复 UI、项目切换、500ms 恢复草稿调度、Cmd-S |
| `crates/desktop/src/product.rs` | 项目变化驱动写字台 `set_project`；Cmd/Ctrl-S 路由 |
| `.github/workflows/consumer-check.yml` | 新增 writing/workbench/面板回归过滤 |
| `docs/superpowers/notes/2026-11-05-writing-workbench-baseline.md` | 存储/身份/上限/UI/未完成基线 |

## A1–A12 结果

| 编号 | 状态 | 证据 / 说明 |
|---|---|---|
| A1 | 部分 | 无项目空态；项目内新建/导入；重复路径返回已有文稿；普通预览未改。文件树/预览的“在写字台编辑”入口尚未接线 |
| A2 | 部分 | 临时库 create→save→重开：正文、DocumentId、revision、state JSON 保留；`State` serde round-trip。未做完整 GUI 重开 |
| A3 | 部分 | 事务回滚、幂等重放、同/不同指纹、双连接 CAS、严格块身份 schema/摘要校验均有测试；真实子进程崩溃 failpoint 未做 |
| A4 | 部分满足 | Rust `Flush/Flushed` + TS `requestFlush` 已实现 sent/queued ACK 屏障；保存只在确认版本后排队。真实中文 IME/GUI 未验证 |
| A5 | 部分 | 项目/文稿切换前写恢复草稿并在失败时留在旧会话；ProductShell 项目消息测试覆盖 A→B→A；旧 load/save 乱序与保存后继续输入的完整路径仍未覆盖 |
| A6 | 部分 | 恢复草稿原子写/读、摘要/损坏保留、base 分类有测试；编辑后 500ms 简单调度；崩溃/重放/恢复预览 UI 未完成 |
| A7 | 部分 | Outbox 状态、镜像原子写、首次导入基线与外部修改阻断有测试；进程重启按 Outbox 重放与旧任务乱序未完成 |
| A8 | 部分 | 路径逃逸/`.digger`/符号链接、大小上限、BOM/CRLF、非法 UTF-8 测试；只读目录/磁盘满/文件增长未覆盖 |
| A9 | 未执行 | 需要正常 GUI/IME 会话；本会话 Accessibility/Screen Recording 被系统拒绝 |
| A10 | 部分通过 | 无 patch 临时副本 `cargo check --locked --workspace --all-targets`、fmt、host/shared 门禁通过；正式锁差异仅为 `digger-store` 新增依赖，共享来源唯一；远端 CI 未推送 |
| A11 | 未执行 | 没有 100/1000/5000 块输入/flush/保存规模测量 |
| A12 | 部分 | baseline 记录备份/只读/上限策略；未实际演练备份→恢复→重开 |

## 命令与结果

证据日志：[`artifacts/2026-11-05-writing-workbench/`](artifacts/2026-11-05-writing-workbench/)（`final-gates.log`、`final-gates-2.log`、`check-final.log`、`desktop-final.log`、`store-final.log`、`gates.log`、`desktop-lib2.log`、`store-writing.log`、`probe-build.log`、`formal-lock.diff`）。

本地（开发 patch）：

```text
cargo test -p digger-store -- --test-threads=1          # 74 passed
cargo test -p digger-desktop --lib -- --test-threads=1  # 767 passed
cargo test -p digger-editor --lib                       # 47 passed
cargo fmt --all --check                                 # 0
cargo check --workspace --all-targets                   # 0
```

无 patch 临时副本（删除 `.cargo/`，HEAD 正式锁 + 显式 resolve，独立 target）：

```text
cargo metadata --locked --format-version 1
# 35 个共享包，全部 git tag 来源，BAD_PATH []
# 正式锁差异：digger-store dependencies 增加 dozer-file-io / sha2 0.10.9 / uuid

cargo check --locked --workspace --all-targets          # 0
cargo fmt --all --check                                 # 0
bash scripts/check-host-deps.sh                         # 0
bash scripts/check-host-deps.sh --self-test             # 15 tests OK
bash scripts/check-shared-deps.sh --offline             # 0 blocks, 0 warnings
cargo build --locked -p digger-desktop --bin icon_scale_widget_probe
cargo test --locked -p digger-desktop --lib -- --test-threads=1  # 767 passed
```

## 关键设计落地

- 权威正文只在 `<project>/.digger/project.db` 的 `document_versions.text`；块身份 `state_json` 与正文同事务。
- Save = expected revision CAS + operation ID 幂等 + Commit + project revision + Outbox + 幂等结果同事务；任一失败整体回滚。
- 文稿身份 UUIDv7；库实例 UUID 存 `meta.store_uuid`；打开项目不建库，首次新建/导入才初始化。
- 镜像用已发布 `dozer-file-io` 的 `ProjectRoot` / `write_atomic`；写入前比对 `documents.mirror_digest`，外部变化标记 blocked，不回滚成功 Commit。
- 恢复草稿落 `.digger/working/<document>/<session>.json`，临时文件 + fsync + rename；损坏保留原件并报告。
- 正常模式不再显示 `sample.md`；SelfTest 保留独立样例。

## 未完成 / 下一步

1. T3：页面 flush 屏障（sent/queued/IME）、严格 500ms/2s 调度、切换/关闭三选一协调。
2. T4：文件树/预览显式编辑入口、创建失败重试、文稿级冲突/恢复预览 UI。
3. T5/T6：真实子进程 failpoint、跨项目/跨文稿乱序、GUI/IME、规模与性能。
4. T7/T8：远端 CI、正式锁提交、备份恢复演练、A1–A12 完整收口。


## 审核修复补记（2026-11-05）

针对 [writing-workbench-review](2026-11-05-writing-workbench-review.md)：

- R01：文稿/项目切换与关闭前统一写恢复草稿；失败保留旧会话并报错。
- R02：导入保存源文件摘要、新建使用 `Absent` 基线；旧 revision 冲突后停止同文档后续镜像。
- R03：新增 Rust `Flush/Flushed` 与 TS `requestFlush` 保存屏障；超时保留输入，不标已保存。
- R04：恢复草稿会话与保存会话分离，新保存使用新 operation ID。
- R05：Behind/Conflict 草稿不再直接覆盖，提供「另存副本」。
- R07：编辑器严格校验块身份 schema/text/块数/ID/next_id/aliases/history；store 载入校验摘要。
- R06：保存、镜像、恢复草稿写入已移到后台阻塞线程；载入与创建/导入仍同步。

复验（无 patch 临时副本，日志 `artifacts/2026-11-05-writing-workbench/review-fix-gates.log`、`final-review-fix.log`、`store-review-fix.log`、`desktop-review-fix.log`）：

```text
cargo check --locked --workspace --all-targets        # 0
cargo fmt --all --check                               # 0
host/shared-deps gates                                # 0
cargo test --locked -p digger-store -- --test-threads=1          # 74 passed
cargo test --locked -p digger-desktop --lib -- --test-threads=1  # 767 passed
npm test                                              # 32 passed
run-editor-selftest.sh lifecycle                      # 11/11
```

仍未完成：真实 GUI/IME、性能规模、真实进程崩溃 failpoint、载入异步化与卡顿注入测试、远端 CI。
