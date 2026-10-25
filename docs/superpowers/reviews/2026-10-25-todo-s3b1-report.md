# S3b-1 Digger 待办面板接入报告

日期：2026-10-25
分支：`feat/todo-panel`（worktree `../digger-todo-s3b1`）
基线：Digger `main` `a9ff9ef`（已核对）
依赖：`libs-2026-10-24e`（`eb702ec4d115259c0b7100867fd8e3baab1ca1f6`）

## 结论

计划的 6 个任务全部完成，每个任务一个提交。自动化门禁全绿；真机验收清单已交付用户，未执行 GUI 点击项。

## 提交

| 提交 | 内容 |
|---|---|
| `195fcf2` | 引入 `dozer-panel-todo` 并嵌入其 web 资源 |
| `1fa9d4f` | `TodoPanel` 适配器（按项目状态、服务、消息分派） |
| `9e033ed` | `todo_native` 宿主（wry + `digger-todo` 协议 + 推送去重） |
| `ebe9831` | 注册待办面板、配对布局与原生视图几何 |
| `a782b62` | 页面事件路由与指派 agent |
| `c18c2b0` | 对真实 `diggerd` 的集成测试与验收清单 |
| 本报告 | 报告提交 |

## 与计划的实现对照

- **依赖/资源**：`dozer-panel-todo` 与其它 `dozer-panel-*` 同为 `libs-2026-10-24e`；`build.rs` 将待办资源拷为 `todo-host.html` / `todo-content.js` / `todo-content.css`（只给 `host.html` 加前缀，另两个本来就不重名），避免与用量的 `host.html` 互相覆盖。
- **TodoPanel**：每项目一份 `WorkspaceState`；`set_project` 切换时作废旧项目 `todos` / `categories` 请求；`update` 对 `Message::AsyncResult` 的 `result_project()` 与当前项目比对，旧项目结果整条丢弃；`Unavailable` 读失败静默保留列表，其余失败进 outbox；`poll` 1 秒自限速。
- **TodoNative**：`digger-todo://` 只服务三个文件，导航白名单只放行本协议；`epoch` 防旧 webview 回传；`Ready` 触发 `WebviewPushState::set_ready(true)` 强制重发；推送脚本双重 JSON 编码并额外转义 `</`、U+2028、U+2029。
- **面板/布局**：`digger.todo` 默认左区，顺序 `project, files, todo, markdown, info`；配对规则与 Project/Files 相同；旧布局缺 todo 时由骨架追加到默认侧末尾，原有顺序不变。
- **几何**：`todo_native_rect` 使用库的 `top_row_inner_h_px()` 和 `WorkspaceState::add_input_height()`，不写像素常数；列表收起时内容区独占整块。
- **事件/指派**：`route_event` 的 `Message` 进 `update`，`AssignAgent` 返回 `TodoEffect`，`OpenDetail` 只写一行 stderr；指派经 `Client::assign_todo_agent` 后台执行，先作废旧 `todos` 请求，再以 `Message::AsyncResult(token, Mutated(..))` 回流触发刷新。
- **轮询**：只有 `panel_visible(todo)` 且未退出时订阅 1 秒 tick；`TodoPanel::poll` 再自限速，避免重复订阅或不可见时耗电。
- **集成测试**：使用进程内真实 `dozerd`（独立自定义产品数据根），不是 `examples/product_daemon`（当前仓库没有该示例）。三个场景共用一个 daemon，重启场景先 `shutdown_daemon` 再拉起。

## Review Focus 对应测试

| # | 测试 |
|---|---|
| 1 守护进程不可用/恢复 | `unavailable_reads_keep_the_last_list_and_stay_silent` |
| 2 切项目迟到结果 | `results_for_a_previous_project_are_dropped` |
| 3 webview 与 iced 控件重叠/不可见销毁 | `todo_native_rect_never_overlaps_the_add_input`、`todo_native_is_destroyed_when_the_panel_is_not_visible` |
| 4 特殊字符推送 | `dispatch_script_survives_hostile_task_text` |
| 5 旧布局没有 todo | `a_saved_layout_without_todo_still_loads_and_appends_it` |
| 6 页面回传不存在 id | `an_event_for_a_task_that_no_longer_exists_is_ignored` |

其它新增测试：4 条资源、7 条 TodoPanel 基础、7 条页面事件/指派、7 条 Native（含 epoch、Ready 重发、locale 变化、导航白名单、agent SVG、切项目 payload），以及 3 条真实 daemon 集成测试。

## 门禁

工作区与干净快照均通过：

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all --check` | 0 | 通过 |
| `cargo metadata --locked --offline` | 0 | 通过 |
| `cargo test --workspace --no-fail-fast` | 0 | **622 passed / 0 failed**（基线 588，新增 34） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | 通过；仅 `block v0.1.6` 未来兼容性提示 |
| `bash scripts/check-shared-deps.sh` | 0 | 0 阻断 / 0 警告 |
| `bash scripts/check-host-deps.sh` | 0 | 通过 |
| `bash scripts/check-host-deps.sh --self-test` | 0 | 三种注入均被拦下，恢复后通过 |
| `bash scripts/check-shell-regions.sh` | 0 | 7 个外壳区域一致 |

干净快照：`git archive HEAD` → `/tmp/todo-s3b1-clean`（目录名不含 `digger`），在自己的 `target` 下执行。归档不带 `.git` 与兄弟仓库，因此临时 `git init`（仅为 shared-deps 取根）并给 `/tmp/{bytehost,byteui,dozer}` 建只读符号链接后，`check-host-deps --self-test` / `check-shell-regions` 通过；源码与产物未因此改变。跑完已删除快照。

## 与计划不一致的地方

1. **TodoNative 的项目 id**：计划签名 `sync(window, visible, rect, state)` 没有项目 id，但库的 `current_view_payload` 需要它。`Native` 增加 `set_project(Option<i64>)` 字段，由 `TodoPanel::set_project` 转发；其余签名一致。
2. **集成测试启动方式**：计划提到 `examples/product_daemon`，该示例不存在；改用 `digger-mcp/tests/memory_roundtrip.rs` 的进程内 `dozerd` 模式，并为 desktop 增加 `dozerd` dev-dependency。
3. **轮询订阅测试**：`iced::Subscription` 不暴露内省接口，测试直接验证其判据函数 `todo_poll_active(shell)`（可见且未退出），订阅点只调用同一函数。
4. **指派 agent 测试**：用本地 fake UDS 服务端验证 `assign_todo_agent` 真实调用与 `Mutated(Ok)` 后的刷新，不引入产品可注入闭包。
5. **分类右键 / 行拖拽**：Digger 没有原生分类右键菜单设施，`CategoryContextMenuOpen` / `CategoryRowPress` 在消息处置表里明确为 `Ignored`；库的 `CategoryDragOver` / `Release` 仍透传，但没有产品侧武装入口。详情与文本框右键按计划忽略。
6. **`Msg::ContentRetry`**：库消息仍透传（库内当前为 no-op），未额外做原生失败页重建；计划未要求。

## 已知缺位（按设计默认）

- 不做详情弹窗；点“打开详情”只在 stderr 写 `todo detail not available in Digger yet`。
- 不开 `task_poller` / `summaries`；指派 agent 只写归属字段。
- 写失败没有 Toast，落 stderr 并靠刷新纠正。
- MCP 待办工具（S3b-2）未开。

## 真机

验收清单：`docs/superpowers/reviews/2026-10-25-todo-s3b1-acceptance.md`。
按交接要求，真机点击与沙箱 `/private/tmp/digger-sandbox` 均由用户执行；本任务未启动、停止或改动该沙箱。

## 停点

停在 `feat/todo-panel`。未合并 `main`、未推送、未发版、未安装。
