# 会话面板 scale 用例 fork 死锁基线

- 日期：2026-11-04
- 对应 Spec：[2026-11-04-conversations-scale-test-fork-hang-design.md](../specs/2026-11-04-conversations-scale-test-fork-hang-design.md)
- 对应计划：[2026-11-04-conversations-scale-test-fork-hang.md](../plans/2026-11-04-conversations-scale-test-fork-hang.md)

## 1. 环境

| 项 | 值 |
|---|---|
| HEAD | `3f8a03ce85daffea44c1fa5d9311beb370084e8c`（`feat(settings): add interface language preference`） |
| 分支 | 本地工作区，含「用量面板改接 daemon」未提交改动 |
| Rust | `rustc 1.98.1 (48a229cea 2026-09-01)` / `cargo 1.98.1 (797e8a9bc 2026-08-05)` |
| 目标架构 | `aarch64-apple-darwin`（macOS 26.6.2，Build 25G83） |
| Cargo patch 来源 | `.cargo/config.toml` 将 `byteui` / `bytehost-*` / `dozer-*` / `bytegit` 指到本地 `byteboy` 工作树；未提交、未修改 |
| Cargo.lock | dirty：本地 path patch 使共享库条目去掉 `source`；未覆盖 |
| `diggerd` | 运行中：`/Applications/Digger.app/Contents/MacOS/diggerd`（PID 88113） |
| 其他 dirty 文件 | `crates/desktop/src/adapters/usage.rs`、`crates/desktop/src/apps/management.rs`、`crates/desktop/src/apps/mod.rs`、`crates/desktop/src/product.rs` |

命令：

```bash
git rev-parse HEAD
rustc --version
cargo --version
uname -m
pgrep -fl diggerd
git status --short
```

## 2. 挂起基线与证据

改动前的全量套件证据记录在 Spec §2（F1–F5），要点：

- 约 730 个用例后停止推进；父 `review_widgets_preserve_header_and_notice_clearance_at_supported_scales` 线程阻塞在
  `Command::output()` → `read_output` → `FileDesc::read_to_end`。
- 同一时刻 `product::tests::chat_lifecycle::forged_events_have_no_effect_and_never_spawn_an_agent`
  卡在自身 fake 的 `calls` 锁上，是连带现象，不是第二个死锁点。
- 干净 HEAD worktree 同样复现；`--skip forged_events…` 后 743 通过（另有 3 个
  `agent_hooks` 高负载超时抖动，单跑即过）。
- 触发前提：本机有真实 `diggerd` 在跑。

**本轮执行说明：** 本节环境与挂起描述沿用 Spec 已记录的基线；实施完成后在
[验收报告](../reviews/2026-11-04-scale-test-fork-acceptance.md) 中记录修复后的全量运行结果。
未把「仅编译通过」当作基线复现。

## 3. 子进程单独执行

改动前的子进程命令（用 `S7C_REVIEW_SCALE_WIDGET_CHILD` 走进程内分支）：

```bash
CHILD=0.5 BYTEUI_ICON_SCALE=0.5 \
  <target>/debug/deps/digger_desktop-<hash> \
  --exact adapters::conversations::tests::review_widgets_preserve_header_and_notice_clearance_at_supported_scales \
  --test-threads=1 --nocapture
```

Spec 记录：该命令约 13s 退出 0，说明子进程本身不坏；挂起只发生在
「并行测试进程里 fork 全量测试二进制」的场景。子进程输出中的
「无法读取会话文件 …/nested-path/…」是 `collapsed_notice…` 用例人为 push 的
fixture（`"nested-path/".repeat(50)`），不是真实文件递归或 daemon 调用。

## 4. 三个既有断言作为对照基准

以下语义在修复后必须逐条保持（共享实现见
`crates/desktop/src/adapters/scale_widget_checks.rs`）。

### 4.1 `actual_widget_header_and_collapse_control_are_above_native_review`

1. `ConversationsDisplay::panel_head` 实际布局出的最后一个子节点是分隔线，高度 > 0。
2. `review_pane_style().padding.top + divider.y + divider.height` ≤
   `preview_chrome_top_px()`：头部（含分隔线）留在原生审阅区之上。
3. `head_node.size().height` > `tab_button_size()`：头部容纳既有分隔线。
4. 对 `collapsed = false / true`：
   - 文本 `"对话"` 的底边 ≤ `review_bounds(...).y`；
   - 存在高度为 1、宽度超过审阅区一半、与审阅区水平重叠的分隔线，且其底边 ≤ 原生区顶边；
   - 存在点击后发出 `ShellMsg::TogglePane(_)` 的矩形，且其底边 ≤ 原生区顶边；
   - 折叠控件必须可命中。

### 4.2 `collapsed_notice_is_dismissible_and_outside_the_native_review`

1. 注入超长错误（`"nested-path/".repeat(50)`）后，折叠布局仍渲染完整错误文本。
2. `"×"` 关闭控件位于初始通知视口内（`0 ≤ y` 且底边 ≤ `review_notice_height(true)`）。
3. 关闭控件底边 ≤ 原生审阅区顶边。
4. 头部 `"对话"` 底边 ≤ 原生审阅区顶边。
5. 所有能发出 `ConversationsNoticeDismiss` 的命中矩形都完整位于初始通知视口内。
6. 关闭按钮中心可点击并发出 `ConversationsNoticeDismiss`。

### 4.3 折叠控件可点

`the_actual_expanded_conversations_view_has_a_clickable_collapse_control` /
`the_actual_collapsed_conversations_view_has_a_clickable_expand_control` 在
`mirrored = false / true` 两种列表方向下，遍历实际布局树的中心点并点击，
必须至少有一个点发出 `ShellMsg::TogglePane(conversations_id())`。

## 5. 结论

唯一触发者是全量测试二进制内的 `Command::output()` fork/等待；进程隔离是
`icon_size::scale()` 的进程级全局决定的硬约束，不能改回在共享 runner 里
`set_scale`。修复方向为专用最小 probe 二进制 + 共享断言，见计划 Task 1–4。

**实施后补记：** 修复 fork 后复跑发现 `forged_events…` 并非纯连带现象——其断言在
格式化消息时对同一个 `fake.calls` 连续 `lock()` 两次，是测试自身的确定性死锁；
另有一次异步选择调用的基线时序问题。最小测试修复与证据见
[验收报告](../reviews/2026-11-04-scale-test-fork-acceptance.md)「偏差」。
