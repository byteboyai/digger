# S7b 会话列表已知缺口与取舍

## 导入结果的失败明细

用户于 2026-10-26 确认沿用现有接口，不绕开，也不改 Dozer。

部分成功拿不到失败条数和原因；需要在 dozer-services 的 TranscriptService 加带失败明细的返回，并走 Dozer 新 libs tag；Dozer 的 Project 面板同样受限。

现有返回是 `Result<u32, ServiceError>`：成功仅显示 `Done(n)` 的条数，0 条显示“没有找到可导入的会话”；部分成功同样写入提示标记。整体失败显示错误、不写标记、允许重试。标记只决定提示，不代表所有文件都成功。

导入按钮旁和完成提示均须显示“个别文件读取失败不会在这里列出,详情见 diggerd 日志”，并展示 `digger_paths::logs_dir()` 的实际值；不硬编码目录，也不解析日志来补造接口结果。计划 Review Focus 2 和人工验收清单同步采用此口径。

## 本期保留的产品缺位

- Digger 删除项目不会清理该项目的会话，`delete_project_transcripts` 未接；Project 面板的 `NoTranscript` 保持不变，后续由用户裁决。
- 搜索框右键菜单没有对应宿主设施，`TextInputMenuOpen` 映射为空操作，不另造菜单系统。
- 搜索焦点每 16ms 用带项目 key 的 iced `is_focused` Operation 查询真实 text_input；反馈带 RequestToken 并校验当前 owner。面板隐藏、项目切换、列表收起、守护进程不可用和关闭时清理焦点并作废旧反馈。Agent 键盘订阅读 AtomicBool 阻止标记，实际搜索框也先捕获字符；对侧 Agent 仍可输入，直到搜索真正获得焦点。库的焦点布尔值只服务展示。
- 列表宽度取配对矩形第一块，最小宽度使用 `shell::list_min_width()`；具体数值仍需目测验收。
- 悬停动画本期不做，`hover_progress` 恒 0，状态变化为阶跃。
- 审阅详情 webview 留给 S7c；点击会话行仍让库正常加载详情状态，内容块本期保持占位。

## Task 4 导入实现与验证

- 显式按钮经 `Msg::ConversationsImport` 发起，只接受当前、仍打开、有已加载空列表且无提示标记的项目，共用 AgentShell Ready 状态；激活、列表刷新、TurnEnded 与 Project 修复均不触发导入。
- 真实 `TranscriptService` 调用和 future 构造都在 tokio 后台任务内执行。返回仍为原来的 `Result<u32, ServiceError>`，不添加失败明细载荷、不解析日志。
- 导入的 RequestToken 归属于该 WorkspaceState；关闭清理瞬态，重开同一项目并发起新导入后，旧 owner 的结果仍会被丢弃。退出中的结果不写标记。
- 提示标记独立存于 `<config_dir>/conversations-imported.json`，schema_version 1，记录项目 id、at_ms、count。启动只加载一次；缺失/损坏视为空，未来 schema 只读，保存采用同目录独占临时文件、完整写入和 sync 后原子重命名，失败清理临时文件并保留旧文件。测试构造注入独立路径。
- Done(7) 与 Done(0) 在标记写入和列表变成非空后仍显示完成文案；重开/重启仅由标记抑制首次导入提示。标记从不代替列表内容。保存失败保留真实成功条数，通过现有红色通知条和 stderr 报错。
- 无法导入时复用 AgentShell 的原因与重试，隐藏导入入口；不另启守护进程。导入/重试/正在导入按钮旁以及完成处均显示批准的日志披露与实际 `digger_paths::logs_dir()`。
- 数千文件的 UI 响应验证用 FakeTranscript 返回 5000 条计数，并在构造 future 前阻塞 200ms；update 断言小于 50ms。重复导入去重和实际文件匹配语义仍由后端负责；Digger 不另做路径匹配或去重。
- HOME 未设置通过独立子进程测试，仍使用隔离 DIGGER_DATA_DIR 和注入 marker 路径，不修改测试进程全局环境。人工三份 Claude transcript 导入与重启验收留给 Task 5，由用户点击。

Task 4 自动验证：首轮缺少 import API 的失败日志 `/private/tmp/s7b-task4-red.log` 和 `/private/tmp/s7b-task4-red-full.log`（exit 101）；当前项目 Start 限制测试在加保护前确实失败 `/private/tmp/s7b-task4-scope-red.log`（exit 101）。最终 `cargo test -p digger-desktop conversations --lib`：79 passed（exit 0）；`RUST_TEST_THREADS=1 cargo test -p digger-desktop`：528 unit + 5 integration passed，0 failed / 0 ignored（exit 0）。本任务新增 25 个测试，Desktop 从 503 unit 增至 528。`cargo clippy --workspace --all-targets`、`cargo fmt --all --check`、`bash scripts/check-shared-deps.sh`、`bash scripts/check-host-deps.sh` 及 `--self-test` 均 exit 0。命令使用独立 HOME=/private/tmp/s7b-home、DIGGER_DATA_DIR=/private/tmp/s7b-data、显式 CARGO_HOME/RUSTUP_HOME；门禁使用 /opt/homebrew/bin 的 Bash 5。最终日志前缀 `/private/tmp/s7b-task4-`；全 workspace 与干净快照验收仍由 Task 5 负责。
