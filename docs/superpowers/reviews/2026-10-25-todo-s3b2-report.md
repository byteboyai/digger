# S3b-2 Digger MCP 待办工具报告(S6e-3b)

日期：2026-10-25
分支：`feat/mcp-todos`（worktree `../digger-mcp-todos`）
基线：Digger `main` `092c4f8`（已核对）

## 结论

Task 1（打开待办工具并补测试）、Task 2（agent 改动在面板一个轮询周期内可见）、Task 3 Step 1–2（验收清单与门禁）完成。真机验收、发布、文档状态行均未执行，停在 `feat/mcp-todos`。

## 提交

| 提交 | 内容 |
|---|---|
| `c7467ab` | `feat(mcp): digger-mcp 开启待办工具(S6e-3b)` |
| `334bde2` | `test(todo): agent 经 MCP 改的任务在面板里 1 个轮询周期内可见` |
| `b926cf7` | `docs: S3b-2 待办 MCP 验收清单` |

## 测试总数核对

- 开工基线：`git archive 092c4f8` 到 `/tmp/mcp-todos-base`，`cargo test --workspace --no-fail-fast` → **622 passed / 0 failed**。
- 完工：干净快照 `cargo test --workspace --no-fail-fast` → **627 passed / 0 failed**。
- 新增 5 条：`todo_roundtrip.rs` 4 条 + `todo_mcp_visible_in_panel.rs` 1 条；`tool_set.rs` 两条测试是改名/改断言，不是新增。
- 未遇到 `apps::tests::closing_during_startup_waits_and_releases_the_root` 偶发；没有重跑凑绿。

## Review Focus 对应

| # | 关注点 | 测试 / 交付 |
|---|---|---|
| 1 | Digger 会话同时配 `dozer` 与 `digger` MCP 时的已知并存失败 | 无法在单测里模拟 agent 的 MCP 状态页；写进验收清单第 6 条，明确 `dozer` 失败与待办工具无关 |
| 2 | 会话不属于任何已登记项目 | `a_session_outside_any_registered_project_gets_a_readable_error` |
| 3 | 空白任务文字 | `blank_todo_text_is_accepted_today_like_dozer`（钉住现状） |
| 4 | 用别的项目任务 id 仍可改 | `todo_ids_are_not_scoped_to_the_session_project`（钉住现状） |
| 5 | 已开着会话看不到新工具 | 验收清单开头与第 8 条要求重开会话 |

主路径测试：`add_then_list_then_toggle_then_edit`（真实 daemon + 独立 `Client` 读回）与 `agent_added_todo_shows_up_within_one_poll_interval`（`TodoPanel::poll` 推时间，不真等 1 秒）。

## 门禁

在干净快照 `/tmp/mcp-todos-clean`（路径不含 `digger`，自己的 `target`）执行。临时快照不带 `.git` 与兄弟仓库，故临时 `git init` 供 shared-deps 取根，并给 `/tmp/{bytehost,byteui,dozer}` 建只读符号链接；源码未改。

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all --check` | 0 | 通过 |
| `cargo metadata --locked --offline` | 0 | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | 通过；仅 `block v0.1.6` 未来兼容性提示 |
| `bash scripts/check-shared-deps.sh` | 0 | 0 阻断 / 0 警告 |
| `bash scripts/check-host-deps.sh` | 0 | 通过 |
| `bash scripts/check-host-deps.sh --self-test` | 0 | 三种注入均被拦下，恢复后通过 |
| `bash scripts/check-shell-regions.sh` | 0 | 7 个外壳区域一致 |
| `cargo test --workspace --no-fail-fast` | 0 | 627 passed / 0 failed |
| `bash scripts/release.sh --dry-run` | 0 | 门禁通过；打印 `0.1.6 -> 0.1.7`，未改文件、未打包 |

## 与计划不一致的地方

1. **孤儿会话错误测试用本地假服务端。** 计划要求“会话 cwd 不属于任何项目”返回可读错误。核对 `dozer-mcp` 后发现错误条件是 `SessionInfo.project_id == None`；真实 `dozerd` 的 `CreateSession` 必须带 `project_id`，构造不出 `None`，而约束禁止改 `dozer-mcp`。因此测试启动一个只应答 Hello/ListSessions 的本地 UDS 假服务端，返回 `SessionInfo { project_id: None }`，钉住 MCP 层的错误路径；没有改库。
2. **假服务端的 Hello 返回 `Reply::Error`。** 按 Client 的旧守护进程兼容策略放行，避免测试里 `paths::product()` 抢先初始化产品身份、与同二进制真实 daemon 的 `install` 冲突。
3. **跨项目 id 测试** 直接用 `Client::add_todo` 在项目 B 造任务，再用项目 A 的 MCP 会话 `toggle_todo`；未改任何项目校验。
4. **`tool_set`** 按计划只改分组名称/期望顺序，测试数量不变。
5. **`memory_roundtrip.rs` 的 `McpFeatures` 副本** 同步 `todos: true`，与入口一致。

## 已知行为（不算本次 bug）

- `add_todo("")` 成功并产生空任务；与 Dozer 现状一致，收紧需改 `dozer-mcp` / `dozerd` 并走 Dozer 新 libs tag。
- `toggle_todo` / `edit_todo_text` 不校验任务 id 是否属于会话项目；与 Dozer 现状一致，收紧另议。
- Digger 会话里若同时配置 `dozer` MCP，`dozer-mcp` 缺 `DOZER_SESSION_ID` 可能显示启动失败；与待办工具无关。
- 升级前已开着的 Agent 会话看不到新增的 4 个工具，必须重开会话。
- 详情弹窗、文本框右键菜单、写失败 Toast 仍按 S3b-1 的已知缺位处理。

## 真机

清单：`docs/superpowers/reviews/2026-10-25-todo-s3b2-acceptance.md`。
按交接要求由用户在新安装版本上执行；本任务未发布、未安装、未碰沙箱 `/private/tmp/digger-sandbox`。

## 停点

停在 `feat/mcp-todos`。未合并 `main`、未打 tag、未推送、未发布、未安装、未改版本号。
