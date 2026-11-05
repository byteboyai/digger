# 会话面板 scale 用例的 fork 死锁修复设计

- 日期：2026-11-04
- 状态：已实施
- 配套计划：[Implementation Plan](../plans/2026-11-04-conversations-scale-test-fork-hang.md)
- 范围：crates/desktop/src/adapters/conversations_tests.rs、crates/desktop/src/adapters/conversations_display.rs 相关接缝，以及按需新增的 probe 二进制 / CI 组织
- 触发基线：HEAD 3f8a03c 及含「用量面板改接 daemon」未提交改动的本地工作区均已复现；仅在本机有真实 diggerd 守护进程运行时出现

## 1. 目标与交付边界

让 digger-desktop 的全量 cargo test -p digger-desktop --lib 在本机可确定跑完，同时不丢失三个 icon-scale（0.5 / 1 / 2）下的会话面板 widget 布局断言，不把「全局 scale 被并行测试污染」这条原设计约束破坏掉。

只解决这一个测试的挂起；不在此扩到其它 fork / Command::output 用例，不改 byteui 的 scale 语义，不引入新的测试框架依赖（如 serial 插件）。

## 2. 现状与证据

| 编号 | 源码入口 | 发现与证据性质 |
|---|---|---|
| F1 | conversations_tests.rs:2187 | review_widgets_preserve_header_and_notice_clearance_at_supported_scales 用 Command::output() 拉起**当前测试二进制自身**（current_exe()），对 3 个 scale 各一次，父线程同步 read_to_end |
| F2 | 同 F1，:2199-2201 | 子进程参数 --exact …review_widgets… --test-threads=1 --nocapture，环境注入 S7C_REVIEW_SCALE_WIDGET_CHILD 与 BYTEUI_ICON_SCALE |
| F3 | byteui/src/theme/icon_size.rs（本地 patch 工作树） | scale() 读进程级 CURRENT_SCALE / STARTUP_ENV_SCALE / INITIALIZED，init_scale 后不再读环境——这正是必须 fork 子进程的原因 |
| F4 | 全量套件实测 | 约 730 个用例后无进展，卡点是父 review_widgets 线程阻塞在 Command::output → read_output → FileDesc::read_to_end；同刻 product::tests::chat_lifecycle::forged_events_have_no_effect_and_never_spawn_an_agent 卡在自身 fake.calls 锁上（附带现象，非第二死锁点） |
| F5 | 干净 HEAD worktree 复现 | 与本地改动无关；--skip forged_events… 后套件 743 通过（另有 3 个 agent_hooks 高负载超时抖动，单跑即过，属独立既有 flake） |

子进程独立执行（CHILD=0.5 BYTEUI_ICON_SCALE=0.5 + 上述 --exact）能 13s 跑完，说明子进程本身不坏；挂起只发生在**并行测试进程里 fork 全量测试二进制**的场景。子进程输出里的「无法读取会话文件 …/nested-path/…」是 collapsed_notice… 用例人为 push 的 fixture 文案（conversations_tests.rs:2011 的 "nested-path/".repeat(50)），不是真实文件系统递归，也非真实 daemon 调用。

实施补记：修复 fork 后复跑全量发现 `forged_events…` 并非纯连带现象——它的断言在格式化消息时对同一个 `fake.calls` 连续 `lock()` 两次，是测试自身的确定性死锁；另有一次异步选择调用的基线时序问题。已做最小测试修复，详见[验收报告](../reviews/2026-11-04-scale-test-fork-acceptance.md)「偏差」。

结论（证据强度：高）：F1 的「fork 全量测试二进制」是唯一触发者；F3 决定了必须进程隔离；根因是**从多线程并行 harness 里 fork/等待全量测试二进制**这一做法本身，而非任何产品功能。

## 3. 所有权与不可退化约束

| 层 | 负责 | 禁止引入 |
|---|---|---|
| conversations_tests.rs | scale 用例的编排与 fork 目标选择 | 直接改全局 CURRENT_SCALE 后并行泄漏 |
| 新增 probe 二进制 | 三个 scale 下的 widget 布局断言，独立可执行 | dozer-client / daemon socket / app-host / 多线程 Tokio runtime / 真实文件 IO |
| conversations_display.rs / 共享视图 | 保持现有 widget 布局与头/通知语义 | 为测试新增产品可见分支 |

必须保留：三个 scale 的断言语义（头保留、通知可关闭且位于原生审阅区外、折叠控件可点）、scale() 作为布局唯一真相来源、进程隔离不污染共享 runner、CI 快速集与慢速集的分层。

## 4. 设计

### 4.1 选型结论

用**专用最小 probe 二进制**替代「fork 全量测试二进制」。理由：进程隔离是 F3 决定的硬约束（不能回退成改全局 set_scale），但被 fork 的目标不应该是 digger_desktop 全量测试二进制（它初始化 libtest harness、dozer-client 全局阻塞队列、iced/app-host 等，是 F1 挂起的根因）。

### 4.2 probe 二进制

- 新增 crates/desktop/src/bin/icon_scale_widget_probe.rs（[[bin]]），main 只做：
  1. 读 BYTEUI_ICON_SCALE（缺失或非法 → 退出码 2 并打印原因）。
  2. 用**最小 stub** 服务（一个恒 Unavailable 的 ConversationService / SummaryService / TranscriptService，不碰 dozer_client）构造 ConversationsPanel，复用现有 ConversationsDisplay 与 dozer_panel_conversations 视图。
  3. 顺序执行三段断言（对应今天的 actual_widget_header_and_collapse_control_are_above_native_review、collapsed_notice_is_dismissible_and_outside_the_native_review、折叠控件可点），任一失败 → 非零退出 + 打印差异。
  4. 全部通过 → 打印 ok、退出 0。
- 禁止：链接/初始化 dozer-client、连接 diggerd.sock、创建多线程 Tokio runtime（可用 current_thread 或完全不建）、真实文件 IO、app-host 启动。这样 probe 秒级完成、从并行父进程 fork 也安全。

### 4.3 共享断言

把三段断言抽成一个**非 cfg(test)** 的辅助模块（例如 crates/desktop/src/adapters/scale_widget_checks.rs），接收「一个已注入 stub 服务的 ConversationsPanel 构造工厂」或直接返回可布局的 Element，供 conversations_tests.rs（测试路径）与 probe main（二进制路径）共用，避免两处各自维护一份断言导致漂移。

现有三个 #[test] 在 fork 落地后可转为「直接复用共享断言」的单元测试（在单进程、默认 scale=1 下跑），或删除——以「共享断言不再需要 fork 来覆盖 scale 分支」为准。

### 4.4 测试编排

review_widgets_preserve_header_and_notice_clearance_at_supported_scales 保持「3 个 scale 各一次子进程」的语义，但 fork 目标从 current_exe() 改为 probe 二进制，路径经 env!("CARGO_BIN_EXE_icon_scale_widget_probe")（若 cfg(test) 单元测试不可见则用 current_exe() 的相邻 target/debug/ 解析，Task 里验证其一即可）。

### 4.5 回退路径

若 probe 二进制改造量超过预期，阶段性回退为：把该用例从默认并行套件中排除（对齐 README 里 editor 书库压测的 --skip 先例），移入慢速 / 手动 workflow 显式运行。此为**过渡**，不视为完成；完成态仍以 4.2 为准。

## 5. 验收矩阵

| 编号 | 必须可验证的结果 |
|---|---|
| A1 | 本机 cargo test -p digger-desktop --lib（有真实 diggerd）确定跑完，不再挂起；forged_events 不再被连带卡住 |
| A2 | probe 二进制在 0.5 / 1 / 2 三个 scale 下各自输出 ok 退出 0，任一注入失败时非零退出且可读 |
| A3 | probe 不初始化 dozer-client / 不连接 diggerd / 不建多线程 runtime / 不做真实文件 IO（以代码审查或 sample 为据） |
| A4 | 共享断言在测试路径与 probe 路径产出一致结果，不出现两份断言漂移 |
| A5 | 三个 scale 的布局断言语义与原实现等价：头保留、通知可关闭且位于原生审阅区外、折叠控件可点 |
| A6 | CI 快速集可构建 probe 并运行；不依赖图形会话或真实 daemon 也能通过（无 daemon 时行为一致） |
