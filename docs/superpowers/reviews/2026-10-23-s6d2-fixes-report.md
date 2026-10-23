# S6d-2 审核补修报告(1b)

日期:2026-10-23。分支 `feat/agent-sessions`(worktree `../digger-s6d2`),基线 `1ed6129`,在其上追加提交,未改写历史。对应清单:`docs/superpowers/plans/2026-10-23-digger-handoff-1b-s6d2-review-fixes.md`。

## 提交

- `6eeeb38` test(agent): 补 S6d-2 审核缺口的测试;修 paths 测试对环境变量的依赖
- `ecf32d5` fix(agent): 关项目页签时回收在途 create 的会话,不留孤儿

未合并 `main`、未打 tag、未推送。库没有改动。

## A. 补测试

| # | 测试名 | 结果 |
| --- | --- | --- |
| 1 | `switching_away_and_back_during_restore_does_not_double_start` | 已补。恢复在途时再次 `ensure_started` 返回 `Task::none()`,驱动首个任务后 `list_sessions` 只调 1 次,`create` 为空(没有第二个 shell)。 |
| 2 | `list_collapse_button_toggles_the_pane_of_the_agent_panel` | 已补。折叠按钮与测试共用 `agent_display::toggle_pane_message()`,断言发出 `Msg::Shell(ShellMsg::TogglePane(agent_id))`。 |
| 3 | `plain_tab_close_sends_close_for_that_tab` | 已补。页签 × 走 `host.wrap(Message::CloseTab(idx))`;断言包装后仍是 `Msg::Agent(7, CloseTab(2))` / `CloseTab(0)`,项目 id 与页签下标都不被改写。 |
| 4 | `collapsed_list_shows_only_the_terminal_block` | 已补。收起时骨架给的第一块宽 0、第二块是整块内容;`terminal_block` 返回整块,配对收起分支只画第二块。 |
| 5 | `a_new_project_starts_with_the_current_grid_so_its_first_session_is_created_at_the_right_size` | 已补。`ensure_started` 用 100×40,`on_restored` 故意传 80×24,断言 `create` 收到 100×40(`start_grid` 优先于恢复完成时的网格)。 |
| 补 | `a_hundred_resizes_to_the_same_grid_send_once` | 已补。首次同步发 1 次,之后 100 次同网格不再发,`resize` 始终只有 1 条。 |
| 补 | `selecting_a_picker_entry_creates_a_session_in_the_project_directory` | 已加强。断言 `create.cwd == 项目路径`、`create.command == $SHELL`(agent CLI 是 attach 后键入的)、`agent == Claude`、网格 90×30,以及写入 PTY 的内容**逐字节等于** `b"claude\n"`。 |

## B. 测试的环境敏感问题

**来源:** S6a(`feat/diggerd`)遗留。`crates/paths` 的 `config_dir_ends_with_digger` 与 `state_and_logs_dirs_are_under_digger_root` 原本断言路径字符串含 `"digger"`;当 `DIGGER_DATA_DIR` 指向 `/private/tmp/.../s6d2-review.XXXX` 这类不含该词的目录时失败。

**修复:**

- `config_dir_ends_with_digger` 改为:默认根仍要求含 `"digger"`;若 `DIGGER_DATA_DIR` 覆盖,则路径等于覆盖值即通过(并在读取顺序上容忍测试并行时的环境变量增删)。
- `state_and_logs_dirs_are_under_digger_root` 改为断言相对结构:`state == config/state`、`logs == state/logs`,不再依赖根目录名字。

**验证:** 在 `/private/tmp/s6d2-review.VaWHAB`(不含 "digger")设 `DIGGER_DATA_DIR`,临时 `HOME`,跑 `cargo test -p digger-paths` → 退出 0,11 通过。全量复验见下方干净快照(同一套环境变量)。

## C. `close_project` 与在途 create

**结论:确认会留下孤儿会话,已修复(产品侧,库未改)。**

复现测试 `closing_a_project_while_create_is_in_flight_kills_the_session_when_it_finishes`:假服务在收到 `create` 时先记账、500ms 后返回;50ms 时关项目页签。修复前 `close_project` abort `state.pending`,create 已发出、守护进程侧会话已建立,但产品拿不到 `SessionInfo.id`,`kills` 为空。

**修复(`ecf32d5`):**

1. `AgentPanel` 增加每项目的关闭代次 `epochs`;`emitter` 闭包捕获创建时的代次。
2. `close_project` 先 `bump_epoch`,并**不再 abort `state.pending`**——任务随 `state` drop 而 detach,继续跑完 create/attach。
3. 代次对不上时丢弃所有回投;若迟到的是 `Message::Attached { info, .. }`,立刻 `svc.kill(info.id)`,并把 kill 任务计入 `critical`(退出前 `drain_critical` 会等到)。
4. 重新打开项目时新任务捕获新代次,旧一轮的迟到 `Attached` 仍按旧代次被回收。

测试改名并改为断言修复后的行为:create 完成后 `kills == ["created-1"]`,状态里不再有该项目。

**仍存在的相邻缺口(库侧,未修):** `create` 成功但 `attach` 失败时,库只发 `Failed`,产品拿不到会话 id,kill 不到。这需要库在 attach 失败路径返回/关闭会话,不在本清单范围内。

## 验证(最终代码)

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 基线核对 | `git rev-parse --short HEAD`(开工时) | `1ed6129` |
| 桌面 + paths 单包 | `cargo test -p digger-desktop -p digger-paths` | 退出 0;302 + 1 smoke + 11 通过 / 0 失败 |
| clippy | `cargo clippy --workspace --all-targets` | 退出 0;仅基线 `block v0.1.6` 1 条 |
| 格式 | `rustfmt --edition 2024 --check` 对本期改动文件 | 干净 |
| 干净快照 | `git archive ecf32d5` 解到 `/private/tmp/s6d2-fix2-clean.Tsb5n5`(目录名与 `DIGGER_DATA_DIR` 都不含 "digger",临时 `HOME`),自己的 `target` | 见下 |

### 干净快照实测

快照目录 `/private/tmp/s6d2-fix2-clean.Tsb5n5`(不含 "digger"),数据根 `/private/tmp/s6d2-fix2-data.nYFZJg`(不含 "digger"),临时 `HOME` `/private/tmp/s6d2-fix2-home.U2ibdo`,自己的 `target`,归档自 `ecf32d5`:

| 命令 | 退出码 | 结果 |
| --- | --- | --- |
| `cargo metadata --locked --offline` | 0 | 锁文件可复现 |
| `cargo test --workspace --no-fail-fast` | 0 | **465 通过 / 0 失败 / 0 ignored**(39 + 17 + 12 + 7 + 302 + 1 + 35 + 11 + 11 + 30) |
| `cargo clippy --workspace --all-targets` | 0 | 仅基线 `block v0.1.6` 1 条 |

这轮同时满足 B 的验证要求:路径里不含 "digger" 且设置 `DIGGER_DATA_DIR`,全量通过。

测试总数:基线 458 + 新增 7 = **465**;没有基线测试被删除或改名。
