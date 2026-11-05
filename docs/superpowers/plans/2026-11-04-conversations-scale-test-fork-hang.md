# 会话面板 scale 用例 fork 死锁修复 Implementation Plan

- 日期：2026-11-04
- 状态：已实施
- Spec：[会话面板 scale 用例的 fork 死锁修复设计](../specs/2026-11-04-conversations-scale-test-fork-hang-design.md)
- 目标：把 fork 全量测试二进制的 scale 用例改为 fork 专用最小 probe 二进制，消除全量套件确定性挂起，保留三个 scale 的布局断言。
- 技术栈：Rust 2024、iced、byteui（本地 patch）、dozer-panel-conversations。

本轮勾选表示对应实现 / 命令 / 记录已完成；不把「仅编译通过」或「环境缺失」解释为验收通过。

## 执行约束与顺序

- 所有路径相对于 Digger 根目录。实施前重新确认工作区状态（本仓库当前有「用量面板改接 daemon」的未提交改动，Cargo.lock 因本地 patch 处于 dirty，均不覆盖）。
- 个人 .cargo/config.toml 与全局 Cargo 配置不修改，不提交本地路径 patch。
- 顺序：Task 0 → 1 → 2 → 3 → 4。Task 1 与 2 可合并为一个可审阅变更；Task 4 是验收收尾。
- 每项任务完成后形成可独立审阅的提交 / 变更；纯代码移动不加实现镜像测试。

## Task 0：复现与基线记录

**新增：** docs/superpowers/notes/2026-11-04-scale-test-fork-baseline.md。

- [x] 记录 HEAD、dirty 文件、Rust 版本、目标架构、Cargo patch 与 lock 实际来源；确认本机 diggerd 是否在跑（这是触发前提）。
- [x] 复现全量挂起并留证：cargo test -p digger-desktop --lib 卡在 review_widgets 父线程 read_to_end；sample 抓到的线程栈存档。（沿用 Spec §2 F1–F5 的存档证据，见基线 §2。）
- [x] 单跑子进程命令（CHILD=0.5 BYTEUI_ICON_SCALE=0.5 + --exact … --test-threads=1）记录 13s 通过，证明子进程本身不坏。（沿用 Spec §2 记录；本轮以 probe 三个 scale 2–3.5s 复验。）
- [x] 记录三个既有 #[test]（actual_widget_header…、collapsed_notice…、折叠控件可点）当前实现与断言语义，作为共享断言的对照。

**验证：** 上述命令的退出码与 sample 输出。
**完成条件：** 复现证据齐全，三个断言语义被完整抄录为对照基准。

## Task 1：抽取共享断言模块

**新增：** crates/desktop/src/adapters/scale_widget_checks.rs。
**修改：** crates/desktop/src/adapters/conversations_tests.rs（三个 #[test] 改为复用共享断言）。

- [x] 把三段断言抽成非 cfg(test) 的纯函数，输入为「已注入 stub 服务的 ConversationsPanel」+ scale 期望值，输出 Result<(), String>（失败带差异）。
- [x] 定义最小 stub：恒 Unavailable 的 ConversationService / SummaryService / TranscriptService，不依赖 dozer_client、不碰 daemon socket。
- [x] 三个原 #[test] 改为调用共享断言（默认 scale=1、单进程）并保持原断言语义不变。
- [x] 跑 conversations 相关单测，确认共享断言与原实现逐条等价（A4 / A5 的前置）。

**验证：** cargo test -p digger-desktop --lib adapters::conversations。
**完成条件：** 三段断言只有一份实现，测试路径行为与原实现一致。

## Task 2：新增 probe 二进制并切换 fork 目标

**新增：** crates/desktop/src/bin/icon_scale_widget_probe.rs；在 Cargo.toml 声明 [[bin]]。

- [x] main 读 BYTEUI_ICON_SCALE（非法/缺失 → 退出 2 带原因），用 Task 1 的 stub 与共享断言跑三段检查，全过退出 0，否则非零 + 差异。
- [x] probe 不建多线程 Tokio runtime、不 import dozer_client、不连接 diggerd、不做真实文件 IO（代码审查为据）。
- [x] conversations_tests.rs 的 fork 目标由 current_exe() 改为 probe 二进制；路径优先 env!("CARGO_BIN_EXE_icon_scale_widget_probe")，若单元测试不可见则用 current_exe() 相邻目录解析，实测选定其一。
- [x] 保留 CHILD / BYTEUI_ICON_SCALE 注入与 3 个 scale 的循环语义不变。

**验证：** 三个 scale 分别直接跑 probe（退出 0）；单跑该测试通过；probe 耗时显著低于 13s（秒级）。
**完成条件：** A2 / A3；fork 不再指向全量测试二进制。

## Task 3：全量与隔离验证

- [x] 本机有真实 diggerd 状态下跑 cargo test -p digger-desktop --lib，确认确定跑完、无挂起（A1）。
- [x] 复跑一次以排除偶发；确认 forged_events 不再被连带卡住。
- [x] 无 daemon 状态（停掉 diggerd 或隔离 HOME）下重跑 probe 与全量，确认行为一致、不依赖真实 daemon（A6）。
- [x] 对照 A4 / A5：共享断言在两个路径一致，三个 scale 断言语义与原实现等价。

**验证：** 全量套件 exit 0；probe 三 scale 各 exit 0；无 daemon 下同样通过。
**完成条件：** A1 / A4 / A5 / A6。

## Task 4：CI 与验收收尾

**修改：** 相关 workflow / README 验证说明；新增 docs/superpowers/reviews/2026-11-04-scale-test-fork-acceptance.md。

- [x] 确认 CI 快速集能构建并运行 probe（probe 作为 [[bin]] 默认随包构建）；不引入图形 / daemon 依赖。
- [x] cargo fmt --all --check、cargo check --workspace --all-targets、原有依赖门禁通过。
- [x] 验收报告逐项记录 A1–A6 的命令、环境、退出码与证据；更新本 plan / spec 状态与 README 链接。
- [x] 未采用回退路径（4.5）时明确说明；若采用则标注为过渡并保留完成态目标。

**完成条件：** A1–A6 均有证据；spec 状态改为已实施。

## Spec 覆盖索引

| Spec / 验收 | 任务 |
|---|---|
| §2 现状与证据 | Task 0 |
| §4.3 共享断言 / A4–A5 | Task 1 |
| §4.2 probe 二进制 / A2–A3 | Task 2 |
| §4.4 测试编排 / A1 | Task 3 |
| §4.5 回退 / A6 | Task 3、4 |
| §5 验收矩阵 | Task 3、4 |
