# 面板功能开发前的最小稳定化收口 Implementation Plan

- 日期：2026-11-05
- 状态：部分实施。Task 0–3 已完成；Task 4 仅本地提交（未推送 / 未跑远端 CI）；Task 5 实机冒烟被 GUI 权限阻断；Task 6 已按实际结果分列。
- Spec：[最小稳定化收口设计](../specs/2026-11-05-panel-development-readiness-design.md)
- 顺序：Task 0 → 1 → 2 → 3 → 4 → 5 → 6
- 目标：修复项目用量异步隔离，验证最新发布依赖组合，完成最小实机冒烟后转入面板功能开发。

审核修复补记：已补齐 usage 自检场景启动选择与真实项目消息路径回归；无 patch 全目标编译及 751 项串行 lib 测试通过。真实 usage 自检仍未完成（120s 无 SUMMARY，含 macOS 系统服务连接错误），不改变本计划“部分实施”状态；详见[验收报告审核修复补记](../reviews/2026-11-05-panel-development-readiness-acceptance.md)。

## 执行约束

- 本文件及 Digger 路径相对本仓库；`../dozer` 等为兄弟仓库。执行前读取适用工作指令与实际可写范围；原仓库只读时在临时副本完成可审阅结果并明确尚未落地。
- 保留用户既有改动、个人 Cargo 配置和工作区锁，不 reset / clean，不直接用隔离副本覆盖 dirty 文件。
- 不重复应用历史补丁，不重做已发布共享设置与门禁修复，不预设新 tag 或 App 版本。
- 本轮任务是规划；本文不授权立即执行后续推送、发版或修改真实账户配置。实施时按当次用户授权推进，提交 / CI / 发布状态分别记录。
- 每个检查保存命令、cwd、退出码、日志和源码身份；超时、失败、未执行分别记录。时间盒到期复盘，不靠降低断言完成。

## Task 0：冻结待验证内容并建立基线

**新增：** `docs/superpowers/notes/2026-11-05-panel-development-readiness-baseline.md`。

- [x] 重新记录五仓库 HEAD、分支、status、remote main / tag SHA，核对 Spec §2 快照后的变化。
- [x] 保存 Dozer / Digger dirty 文件清单与必要的内容摘要，区分用户修改、版本元数据和 path 来源锁变化；不把本批之外的内容自动纳入提交。
- [x] 核对已发布 tag 与实际 API，确认本批可在 Digger 适配层完成，不需要共享库发版。
- [x] 记录有效 Cargo 配置来源、工具链和目标架构，设计无 path patch 的可写临时验证环境。
- [x] 明确最终候选的内容清单；记录两产品 GUI / daemon 的构建方式和现有数据根隔离入口，不猜测环境变量。

**完成条件：** Spec A1；可区分开发 patch 构建与正式依赖构建，保留全部既有修改。

## Task 1：确定性复现用量旧回包

**文件：** `crates/desktop/src/adapters/usage.rs` 的测试模块，按需拆出同目录测试文件；ProductShell 相关测试。

- [x] 构造可控制每次查询完成顺序的 fake UsageService，用唯一会话 / 项目标记区分结果。
- [x] 先写 A 查询 → 关闭全部 → A 成功迟到的回归，断言最终空态；记录修复前的预期失败。
- [x] 补 A 关闭再开、A/B 逆序成功、旧失败、隐藏面板切换、同域逆序刷新和同 scope 幂等测试。
- [x] 使用 oneshot / barrier 与有限超时，确保查询已经开始再切换；禁止用固定 sleep 充当同步。
- [x] 如果预期失败未出现，检查产品路由是否已有其它过滤并更新根因；保留行为需求，避免按原假设硬改实现。

**验证：** `cargo test --locked -p digger-desktop --lib adapters::usage::tests -- --test-threads=1`；若调整测试模块，记录最终过滤器与实际执行数，0 tests 不算通过。

**完成条件：** 静态风险已转成可重复失败或有证据排除的场景，根因与失败日志留档。

## Task 2：修复作用域生命周期与完整路由

**修改：** `crates/desktop/src/adapters/usage.rs`、`crates/desktop/src/product.rs`；按需修改 `usage_native.rs`、`lifecycle_selftest.rs` 和相关 fixture。

- [x] 引入独立 scope epoch 与带 scope / epoch 的后台消息信封，标识不随共享状态 reset 复用。
- [x] 所有实际作用域变化先失效旧 epoch、清理展示状态，再按可见性查询；相同 scope 重复同步幂等。
- [x] 后台成功 / 失败在进入共享 update 前检查身份；与本地交互消息分开，更新 receiver、顶层 Msg 和测试入口，避免裸回包旁路。
- [x] 保留同域共享 refresh_generation 语义；不修改 UsageService 或 daemon 协议。
- [x] 项目切换 / 关闭同步原生视图清空，确保隐藏后重现不展示旧图表。
- [x] 通过真实 ProductShell 更新路径覆盖关闭、重开和旧消息；确认旧错误不产生 notice、不影响新 loading。
- [x] Task 1 全部回归通过；epoch checked 递增及耗尽处理有清晰代码路径。

**验证：** 用量模块与新增产品路由测试，`cargo fmt --all --check`，`cargo check --locked -p digger-desktop --all-targets`。这些开发环境结果不替代 Task 3 的正式来源验证。

**完成条件：** Spec A2；实现局限于实际问题，现有共享库 API 和数据口径保持兼容。

## Task 3：建立两产品最终无 patch 验证组合

**涉及：** 两产品 `Cargo.toml` / `Cargo.lock`，已有 scripts 和 Digger `.github/workflows/consumer-check.yml`。

- [x] 将选定源码放入可写临时副本，建立独立 Cargo 配置与缓存；从该副本 / 外部 cwd 执行，核对无本地 patch。
- [x] 从现有正式锁出发；确需更新时在副本显式 resolve，审阅差异后形成候选正式锁。此后所有验证使用 `--locked`。
- [x] 保存 `cargo metadata --locked --format-version 1` 的共享来源摘要，核对 tag / SHA 与唯一来源；检查没有 sibling path 来源。
- [x] 两产品执行 `cargo check --locked --workspace --all-targets`、fmt，以及现有共享依赖、宿主边界门禁和自检；Dozer 另执行面板边界、日志来源与 i18n 门禁。
- [x] Digger 构建 `icon_scale_widget_probe`，执行三个 scale；执行 desktop 全量 lib 串行验证及一次有限时长默认并行运行。
- [x] Digger 执行 Consumer check 中现有核心 Rust、Web 同步 / fixture / 嵌入产物检查；新用量回归加入快速集，并确认匹配到用例。
- [x] Dozer 执行现有 Consumer check / runtime-tests 对应检查，覆盖 settings 及本地会话列表 / transcript 修复相关测试；测试名从最终源码取得。
- [x] 既有 agent_hooks 并行超时若出现，保留原失败日志并定向 / 串行确认；若定向仍失败或出现新失败，修复后重跑受影响范围，不标全绿。
- [x] 对所有本批实质修改重跑相关检查，保留最终版本结果；无关共享库全量测试不重复跑作额外门槛。

Digger 代表命令（在上述隔离环境内；offline 仅在缓存齐备时附加）：

```bash
cargo check --locked --workspace --all-targets
cargo build --locked -p digger-desktop --bin icon_scale_widget_probe
cargo test --locked -p digger-desktop --lib -- --test-threads=1
cargo test --locked -p digger-desktop --lib
bash scripts/check-host-deps.sh
bash scripts/check-host-deps.sh --self-test
bash scripts/check-shared-deps.sh
```

命令由执行器设置有限超时并保存日志；探针三个 scale 的命令沿用现有 workflow，避免新增另一份路径推断。

**完成条件：** Spec A3；代码和正式锁形成可审阅差异。工作区个人 patch 保留，不将其锁文件直接推送。

## Task 4：最终提交与远端 CI 覆盖

- [x] 核对最终差异仅包含已选范围，拆清行为修复、正式锁与必要 CI 改动；本地版本元数据不自动夹带。
- [ ] 在实施授权范围内提交 / 推送候选分支，按仓库现有流程触发 CI；未执行时明确记录原因，不将写入本计划视作发布授权。
- [ ] 保存 Dozer / Digger 最终 SHA、CI URL、job 结论与新用量回归的实际执行证据。
- [x] 若 CI 未覆盖相关默认并行套件，保留 Task 3 的实际结果，明确其与远端快速集的差别；不宣称远端跑过未配置的套件。
- [ ] CI 失败按代码 / 环境分类处理；修复后跟踪新 SHA，不能引用旧 SHA 的绿灯结束任务。

**完成条件：** Spec A4；最终两个产品的远端验证有可核对结果，不要求额外发共享 tag 或 App 二进制。

## Task 5：最终候选的最小实机冒烟

**新增 / 更新：** `docs/superpowers/reviews/2026-11-05-panel-development-readiness-acceptance.md` 的 M1–M6 矩阵。

- [x] 构建与 Task 4 SHA 对应的 Digger GUI / daemon：`scripts/package-macos.sh debug` → `target/aarch64-apple-darwin/bundle/macos/Digger.app`；临时数据根与两产品 GUI 冒烟未执行（权限阻断）。
- [ ] 执行 Spec M1 / M2：两产品设置开关、resize、焦点、中文 IME、Esc 和复制粘贴。
- [ ] 执行 M3 / M6：shell 持续输出、输入和 resize；GUI 重开恢复；显式结束会话与本次实例清理。按产品设计判断 daemon 是否应继续运行。
- [ ] 执行 M4：Digger 中文、多行、undo/redo、保存、退出重开；比较保存前后实际内容。
- [ ] 执行 M5：A/B 项目用量、关闭全部、同项目重开，观察原生图表空态；慢回包保证引用 Task 1/2 的确定性证据。
- [ ] 有现成注入入口时补创建失败 / Retry；无入口则列后续项，不编造结果。
- [ ] 实机发现阻断时补最小行为回归并修复，重走受影响的 Task 3 / 4 / 5，更新最终候选 SHA。

**完成条件：** Spec A5；M1–M6 均有结果，失败有处置。图形环境或输入法不可用时保留未验，不用无窗口测试代替。

## Task 6：形成收口结论与下一阶段边界

- [x] 报告逐项填写 A1–A6、M1–M6，链接基线、日志、最终依赖组合与 CI；记录工具链、数据根、源码和二进制身份。
- [x] 分列未解决缺陷、已知测试噪声、环境受限、后续发行验收；不照搬已被后续发布修复的历史缺口。
- [ ] A1–A6 满足后将 spec / plan 状态更新为已完成，并明确可以转入面板功能主线。（A4/A5 未满足，暂保持部分实施。）
- [x] 记录后续保留的多屏 / Space / 全屏、完整历史兼容与性能测量范围，不把它们追加为本批默认门槛。
- [x] 首次时间盒到期仍未达标时给出已完成项、实际阻断与下一步，不重开无退出条件的框架优化计划。

**完成条件：** Spec A6；结论区分“面板开发基线收口”与“完整发行验收”。

## 验收覆盖索引

| Spec 验收 | 任务 |
|---|---|
| A1 基线与修改保留 | Task 0 |
| A2 异步作用域与展示隔离 | Task 1、2 |
| A3 正式来源与本地验证 | Task 3 |
| A4 最终提交 CI | Task 4 |
| A5 最小实机路径 | Task 5 |
| A6 结论及后续范围 | Task 6 |
