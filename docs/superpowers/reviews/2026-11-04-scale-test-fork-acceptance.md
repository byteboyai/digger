# 会话面板 scale 用例 fork 死锁修复验收

- 日期：2026-11-04
- 状态：已实施
- Spec：[2026-11-04-conversations-scale-test-fork-hang-design.md](../specs/2026-11-04-conversations-scale-test-fork-hang-design.md)
- 计划：[2026-11-04-conversations-scale-test-fork-hang.md](../plans/2026-11-04-conversations-scale-test-fork-hang.md)
- 基线：[2026-11-04-scale-test-fork-baseline.md](../notes/2026-11-04-scale-test-fork-baseline.md)

## 环境

| 项 | 值 |
|---|---|
| HEAD | `3f8a03ce85daffea44c1fa5d9311beb370084e8c` + 本轮未提交改动 |
| Rust | `rustc 1.98.1 (48a229cea 2026-09-01)` / `cargo 1.98.1` |
| 目标架构 | `aarch64-apple-darwin`（macOS 26.6.2） |
| `diggerd` | 运行中（`/Applications/Digger.app/Contents/MacOS/diggerd`） |
| Cargo patch | 本地 `.cargo/config.toml` 指向 byteboy 工作树；未修改、未提交 |
| 测试数据根 | `cfg(test)` 下 `digger_paths::config_dir()` 固定到 `std::env::temp_dir()/digger-tests-<pid>`，lib 套件不连真实 daemon socket |

## 交付物

| 文件 | 变更 |
|---|---|
| `crates/desktop/src/adapters/scale_widget_checks.rs` | 新增。共享断言、最小 `Unavailable` stub、无窗口 layout/click harness、每线程复用软件 renderer |
| `crates/desktop/src/bin/icon_scale_widget_probe.rs` | 新增。读 `BYTEUI_ICON_SCALE`，跑三段共享断言 |
| `crates/desktop/Cargo.toml` | 新增 `[[bin]]`；`iced_tiny_skia` / `iced_renderer` 的 `tiny-skia` 移到常规依赖（软件渲染，无 GPU / 图形会话） |
| `crates/desktop/src/adapters/conversations.rs` | 新增 `ConversationsPanel::new_headless`（marker 留在内存，不读文件） |
| `crates/desktop/src/adapters/conversations_tests.rs` | 三个断言改为调用共享实现；fork 目标改为 probe；`HOME` 隔离；移除全量测试二进制的 fork |
| `crates/desktop/src/adapters/mod.rs` | 注册共享模块 |
| `crates/desktop/src/product_chat_tests.rs` | 修复 `forged_events` 断言的自锁与异步基线（见「偏差」） |
| `.github/workflows/consumer-check.yml` | PR 快速集构建并运行 probe 三个 scale |
| `README.md` | 文档链接与 probe 命令 |

## 验收矩阵

### A1 — 全量 lib 套件确定跑完，不再挂起

前置：scale 用例在 `current_exe()` 相邻目录调用 probe，因此需先构建 probe
（`cargo build -p digger-desktop --bin icon_scale_widget_probe`；Task 2 验证即此步，
CI 的探针步骤也会构建）。未构建时用例会以可读错误失败，不会挂起。

命令：

```bash
cargo test -p digger-desktop --lib
```

结果（第二轮，默认并行）：

```text
test result: FAILED. 733 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.55s
```

- 套件**确定跑完**，没有再出现 `review_widgets` 父线程 `read_to_end` 挂起。
- `forged_events_have_no_effect_and_never_spawn_an_agent` 通过。
- 3 个失败全部是既有的 `adapters::agent_hooks` 高负载超时抖动（Spec §2 F5 已记录），与本次改动无关；单线程跑该模块 14/14 通过（3.52s）。

单线程全绿对照：

```bash
cargo test -p digger-desktop --lib -- --test-threads=1
```

```text
test result: ok. 736 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 26.15s
real 26.51
```

对话模块定向验证：

```bash
cargo test -p digger-desktop --lib adapters::conversations -- --test-threads=1
```

```text
test result: ok. 74 passed; 0 failed; 0 ignored; 0 measured; 662 filtered out; finished in 13.90s
```

scale 用例单跑：

```bash
cargo test -p digger-desktop --lib \
  adapters::conversations::tests::review_widgets_preserve_header_and_notice_clearance_at_supported_scales \
  -- --exact --test-threads=1
```

```text
test result: ok. 1 passed; ... finished in 5.75s
```

### A2 — probe 三个 scale 各自 exit 0；注入失败可读

```bash
PROBE=target/aarch64-apple-darwin/debug/icon_scale_widget_probe
for scale in 0.5 1 2; do HOME=<isolated> BYTEUI_ICON_SCALE=$scale "$PROBE"; done
```

| scale | 输出 | 退出码 | 耗时 |
|---|---|---|---|
| 0.5 | `icon_scale_widget_probe: ok at scale 0.5` | 0 | 3.47s |
| 1 | `icon_scale_widget_probe: ok at scale 1` | 0 | 2.12s |
| 2 | `icon_scale_widget_probe: ok at scale 2` | 0 | 1.96s |

失败注入：

```text
$ env -u BYTEUI_ICON_SCALE "$PROBE"
icon_scale_widget_probe: BYTEUI_ICON_SCALE is required (e.g. 0.5, 1, 2)
exit=2

$ BYTEUI_ICON_SCALE=nan "$PROBE"
icon_scale_widget_probe: invalid BYTEUI_ICON_SCALE "nan"; expected a positive finite number
exit=2
```

### A3 — probe 不初始化 dozer-client / 不连 daemon / 不建多线程 runtime / 不做真实文件 IO

代码审查：

```bash
rg -n "dozer_client|tokio::runtime|std::fs|File::|read_to_string|socket|daemon" \
  crates/desktop/src/bin/icon_scale_widget_probe.rs \
  crates/desktop/src/adapters/scale_widget_checks.rs
```

输出只有模块注释中的「no dozer-client / no daemon socket / no filesystem」说明，没有 import、`Runtime`、`std::fs` 或 socket 调用。

- probe `main` 只读环境变量、`set_scale`、构造 stub panel、跑同步断言；不调用 `init_scale`（它会读落盘文件）。
- `ConversationsPanel::new_headless` 使用内存 `ImportMarker::default()`，没有 marker 文件读取。
- stub 的 `ConversationService` / `SummaryService` / `TranscriptService` 恒返回 `Unavailable`。
- 唯一的隐式文件访问是 `iced_tiny_skia`/`resvg` 测量 SVG 时加载系统字体（与既有 layout 单测相同的软件 renderer），不是产品 / daemon 数据。
- 父测试用隔离 `HOME` 启动 probe，避免扫描用户 `~/Library/Fonts`；probe 仍不读 Digger 配置或 daemon socket。

### A4 — 共享断言在测试路径与 probe 路径一致

只有一份实现：

- `crates/desktop/src/adapters/scale_widget_checks.rs`：
  - `check_header_and_divider`
  - `check_collapse_control_above_review`
  - `check_collapsed_notice`
  - `check_all`（probe 按顺序组合以上三段）
- `conversations_tests.rs` 的两个既有断言测试调用同一组函数；`pane_toggle_clickable` 也由原点击测试共用。
- probe 的 `main` 调用 `stub_panel()` + `check_all()`，与测试路径共用同一个 stub / layout harness。

### A5 — 三个 scale 的断言语义与原实现等价

| 语义 | 实现 |
|---|---|
| 头保留 | `check_header_and_divider`：panel head 分隔线高度 > 0；padding + divider 底边 ≤ `preview_chrome_top_px()`；`"对话"` 底边 ≤ `review_bounds().y`；实际分隔线也在原生区上方 |
| 通知可关闭且在原生审阅区外 | `check_collapsed_notice`：超长错误完整渲染；`"×"` 在初始通知视口内且底边 ≤ 原生区顶边；所有 dismiss 命中矩形完整在视口内；中心可点并发出 `ConversationsNoticeDismiss` |
| 折叠控件可点 | `check_collapse_control_above_review`：展开/折叠两种布局中，所有发出 `TogglePane` 的命中矩形底边 ≤ 原生区顶边；`pane_toggle_clickable` 覆盖 mirrored 两侧 |

### A6 — CI 快速集 / 无 daemon 一致

- `consumer-check.yml` 新增步骤：

```yaml
- name: 会话面板 icon-scale 探针
  run: |
    for scale in 0.5 1 2; do
      BYTEUI_ICON_SCALE="$scale" cargo run --locked -q -p digger-desktop --bin icon_scale_widget_probe
    done
```

- probe 不依赖图形会话、GPU、真实 daemon；CI 只构建并运行 `[[bin]]`。
- lib 测试的 `digger_paths::config_dir()` 在 `cfg(test)` 下固定到临时目录，因此全量套件本身不读真实 `~/Library/Application Support/...` 或真实 `diggerd.sock`。
- 无 daemon 复跑：probe 在隔离 `HOME` 下三个 scale 均 exit 0；lib 套件在隔离 `HOME`（`HOME=<empty>`，保留 `RUSTUP_HOME` / `CARGO_HOME`）下单线程 736/736 通过，22.86s。

```bash
HOME=<empty> RUSTUP_HOME=/Users/chrischiang/.rustup CARGO_HOME=/Users/chrischiang/.cargo \
  cargo test -p digger-desktop --lib -- --test-threads=1
# test result: ok. 736 passed; 0 failed; ... finished in 22.86s
```

## 命令门禁

| 命令 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `bash scripts/check-host-deps.sh --self-test` | `ok (审视了 3 个业务 crate)`；15 tests OK |
| `cargo check --workspace --all-targets` | 通过（1m33s；仅既有 dead_code 警告） |

## 偏差与说明

1. **`forged_events` 不是纯连带现象。** Spec §2 F4 认为它只是卡在 `fake.calls` 锁上；实际断言在第 335 行对同一个 `Mutex` 连续 `lock()` 两次（`assert_eq!` 的消息参数里再锁一次），是测试自身的确定性死锁。同时，选择会话触发的一次异步 `list_groups` / `list_group_messages` 可能在 `before` 之后落地。已做最小测试修复：先等异步选择调用稳定，再只取一次锁。该修复是 A1「forged_events 不再被卡住」的必要条件。
2. **renderer 复用。** 每次 `iced_tiny_skia::Renderer::new` 都会在首次测量 SVG 时扫描系统字体；原实现每个 layout 新建一个 renderer，在本机单次扫描可达数十秒。共享 harness 改为每线程复用一个软件 renderer，probe 从 >120s 降到 2–3.5s。
3. **`tiny-skia` 依赖位置。** 为了让非 `cfg(test)` 的 probe 能编译同一份 layout harness，`iced_tiny_skia` 与 `iced_renderer/tiny-skia` 从 dev-dependencies 移到常规依赖。它是纯软件 renderer，不需要窗口 / GPU；主程序仍由 `iced` 的 wgpu feature 主导。
4. **未采用 4.5 回退路径。** scale 用例仍在 lib 套件内，只是 fork 目标改为 probe；没有把用例移出默认套件。
5. **本机耗时受字体库影响。** probe 首次运行可能触发系统字体扫描；未隔离 `HOME` 且未复用 renderer 的早期运行超过 120s。本轮隔离 `HOME` + 复用 renderer 后，首次 3.5s、热运行约 2s。CI 的 macOS runner 字体集不同，绝对耗时可能更低。

## 结论

A1–A6 均有可复现证据。全量 lib 套件不再挂起，probe 三个 scale 通过且秒级完成，共享断言只有一份实现；剩余 3 个并行失败是 Spec 已记录的既有 `agent_hooks` 高负载 flake，单线程全绿。
