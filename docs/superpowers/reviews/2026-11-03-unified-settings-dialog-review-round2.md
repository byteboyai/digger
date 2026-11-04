# 统一设置模块与对话框宿主：第二轮审核

> 后续状态（2026-11-04）：Digger 已修复，共享实现及修复已发布为 bytehost `v0.4.23` / Dozer `libs-2026-11-04a`，RR07 发布已收口。见[修复记录](2026-11-03-unified-settings-dialog-fixes.md)与[发布记录](2026-11-04-shared-settings-release.md)。原只读工作树未覆盖，实机与整体规格验收仍未完成。以下保留审核时的发现与证据。

- 日期：2026-11-03
- 结论：**仍不通过。** 第一轮问题已有实质修复，但仍发现 / 确认 5 项 P1、2 项 P2；不能宣称整体统一完成。
- 依据：[Spec](../specs/2026-11-03-unified-settings-dialog-design.md)、[Plan](../plans/2026-11-03-unified-settings-dialog.md)、[第一轮审核](2026-11-03-unified-settings-dialog-review.md)、开发方更新的[验收报告](2026-11-03-unified-settings-dialog-acceptance.md)。
- 源码：Digger `e82c048`、bytehost `729fcaf`、Dozer `b701abeb` 上最新未提交工作树；[关键源码 SHA256](artifacts/2026-11-03-unified-settings-dialog-review-round2/source-sha256.json)。行号对应本轮工作树。
- 审核未修改实现、个人配置或原工作区锁文件；探针只追加到临时副本。

## 第一轮修复核对

| 第一轮项 | 本轮判断 |
|---|---|
| R01 Dozer 未迁移 | 已接共享 DialogController、GitPageState 和 Git 视图，删除原三行本地视图；原生错误 / Esc / 驱动示例仍有缺口，不能整体关闭 |
| R02 旧 Bound(Err) 误关新窗口 | 身份过滤与按 key 取窗口已修，产品回归通过 |
| R03 失败状态被清空 | 当前绑定失败保留 Failed，父窗已有错误 / Retry 横幅；但超时强制清理仍有 RR04 |
| R04 裸 NSWindow 地址跨回调 | 已改主线程 Retained 注册表，消除了上一轮无所有权裸地址传递；原生并发 / 关闭场景仍缺实机证据 |
| R05 取消后仍提交 | 网络阶段准入与取消已补，相关测试通过；已准入旧操作仍可覆盖新操作，见 RR02 |
| R06 多 provider JSON 覆盖 | 内置 JSON / Memory store 的事务 update 和原子替换已补；本轮两真实线程重叠探针保留全部账户，确认该场景已修 |
| R07 输入消息 Debug 泄露 | GitToken 改用 SecretToken，产品 Debug 回归通过；HttpRequest 和错误也补脱敏 |
| R08 干净构建失败 | 未修复，仍固定到不含新 crate 的 v0.4.22，见 RR07 |

## Findings

### RR01 · P1：Created 任务在 Resize 后结束，原生绑定从未执行

**位置：** `digger/crates/desktop/src/product.rs:1519–1527`。

`window::resize::<()>(id, size).then(...)` 后接了 `attach_child`，但当前锁定的 iced_runtime 0.14 的 `resize` 是 `task::effect`：只发窗口动作，不产生 Output。`Task::then` 只对 Output 调用闭包，所以 Resize 后的原生绑定根本不会运行。

本轮通过真实 `update_settings(Message::Created(key, child))` 取出 Task 流，模拟回复主窗口 Size，收到且只收到 GetSize 和 Resize，随后流结束；没有 attach_child 动作，没有 Bound 回包，控制器留在 Opening。另一个直接 Task 探针验证 resize 的 then 闭包始终未调用。无需真实 GUI 即可确定此运行时编排错误。

**影响：** 初次打开设置不会沿规定路径完成父子绑定，窗口可能保持占位独立形态，生命周期也不会进入 Open；用户重复打开无法按 Open 规则正常激活。即使后续父窗 resize / DPI 事件偶然触发 sync_child，也不能替代初次打开协议；SyncResult 也不是 Bound 成功。

**需要修复：** 用无 Output effect 的正确串接机制，或原生操作的明确确认事件驱动下一步，保证创建路径确实执行绑定并送出 Bound。不能再只直接调用 on_bound 测试状态机。

**回归：** 正式测试应排演 Created 的整个 Task 流，消费 / 回复 runtime action 后确认存在原生绑定任务和最终成功 / 失败消息；再用真实窗口确认 parentWindow 与 phase 为 Open。

### RR02 · P1：Admission 只做一次检查，旧的已准入提交仍能覆盖新连接

**位置：** `bytehost/crates/bytehost-git-accounts/src/service.rs:211–227`；disconnect_at 同类路径。

服务在 `admission.admit(ticket)` 返回后释放准入锁，随后独立执行凭据写入和元数据事务。新请求可在旧请求的凭据操作尚未完成时注册并成功提交；旧操作之后恢复，仍无条件保存旧 Token 和旧账户。JSON update 锁只能防止不同 provider 丢字段，不能排序同 provider 的凭据 + 元数据提交。

本轮 barrier 探针：A 已准入，暂停在 CredentialStore::set 内 → 注册并完成 B → Admission 已明确拒绝 A 的 ticket → 恢复 A。结果双方均报告完整成功，但最终凭据和账户都回到 A。探针使用共享 Admission 和最新 `connect_with_account_at(..., Some(ticket))`，没有绕过生产准入接口。

**影响：** 网络阶段取消的修复有效，但取消 / 关窗后新连账户仍可能被旧操作覆盖；最终查询只能展示被覆盖后的旧事实，不能恢复 B。

**需要修复：** 在服务 owner / provider 维度建立持久化提交串行边界，使已进入持久化的 A 先完成，再允许 B 提交；或在实际副作用开始时按同一原子协议确认顺序。同步覆盖 connect、disconnect 和不同 service 实例；不要把可能阻塞 Keychain 的锁放进 UI 线程。

**回归：** 暂停在凭据 set / delete 内而不只是 HTTP 内，测试取消 A、B 成功、A 后恢复；最后必须为 B，凭据与元数据一致。允许旧操作完成收尾，不等于允许旧操作成为新请求之后的最终事实。

### RR03 · P2：部分提交后重开查询把已保存凭据当作未连接

**位置：** `bytehost/crates/bytehost-git-accounts/src/service.rs:235–241`；`bytehost-settings/src/git.rs` 的 close / apply_query。

query 只对已有元数据的 provider 查询凭据，元数据没有账户就直接 continue。若连接成功写入 Keychain、写元数据失败，当前会话虽显示 warning，关闭后 warning 被清理，新会话查询只得到空账户；真实凭据仍保留，但 UI 显示未连接，也没有对应断开入口。

本轮 fake metadata.save 失败探针确认：`credential_committed=true`、`metadata_committed=false`、凭据后端实际有值，紧接着所谓权威 query 却返回该 provider 无账户。服务没有把“凭据存在但用户名 / 元数据待恢复”表达出来。

**需要修复：** 查询所有 provider 的凭据事实，显式表达待协调 / 部分提交状态，提供重新校验或明确断开入口；不能自动删除凭据，也不能把未知元数据映射为未连接成功。既有 Dozer Keychain schema 如需变化，须先保证兼容。

**回归：** 保存凭据后使元数据写失败 → 关闭并销毁页面状态 → 新页面查询，仍能看到可操作的真实状态；补断开部分提交、重试失败和进程重启测试，断言凭据、元数据、UI 三者。

### RR04 · P1：强制销毁超时先删除窗口映射，导致清理命令找不到实例

**位置：** `digger/crates/desktop/src/settings/mod.rs:305–308`；`product.rs:1367–1381`。

on_destroy_timed_out 取得控制器的 Destroy effect 后立即清空 window / window_key。随后 apply_settings_effects 用 window_for(key) 查询目标，自然得到 None。原本要求强制释放的命令最终只再排一个 2 秒超时，不会发出 native close；下一次超时时控制器已 Closed，也不会再清理。

本轮状态探针确认 effect 存在而映射已丢；实际处理 Message::DestroyTimedOut 并排空其 Task 的探针确认 **零个窗口动作**，只有另一个超时消息。

**影响：** 原生关闭未完成时，模型假装 Closed、实际窗口可能仍存活，遗留覆盖父窗的透明 / 遮罩窗口；没有实现报告声称的超时强制释放。此结论是确定的命令遗漏，未冒称已在实机看到孤儿窗口。

**需要修复：** 在强制清理完成前保留旧实例映射，或把真实 window id 带进不可丢失的销毁命令。超时后旧资源的所有权不能随状态 reset 消失；新会话与旧资源分开管理。

**回归：** 首次 close 无确认 / 失败 → timeout 必须产生针对原 id 的关闭 / 释放动作；旧 Close / timeout 不误关新窗口。覆盖创建中取消和关闭中重开。

### RR05 · P2：Dozer 仍先消费 Esc，页面 / IME 没有处理机会

**位置：** `dozer/crates/dozer-app/src/platform/settings_overlay.rs:124–134`。

handle_input 一看到 Escape KeyPressed 就直接返回 Settings::Close，后续共享 gpu.track_and_convert / dispatch 根本不会收到事件。因此页面内确认 / 菜单以及可能送达该 KeyPressed 的 IME 组合没有优先消费机会。把普通输入转换搬到共享 GPU 并没有统一 Esc 优先级；Digger 的订阅却按 Captured / Ignored 区分，两个驱动行为仍不同。

这是明确的事件路径问题，本轮没有声称做过真实输入法测试。

**需要修复：** 先将事件交给 IME / 当前页面交互层，只有未消费的 Esc 才向 dialog 请求关闭；共享驱动须暴露事件消费结果或等价策略，避免产品抢先截断。

**回归：** 页面确认打开时 Esc 只取消确认；输入法组合中 Esc 先结束组合；普通空闲状态 Esc 才关闭设置。两个驱动使用同一行为矩阵，并补实机输入法记录。

### RR06 · P1：Dozer 的真实创建失败仍会 panic，进不了 Failed / Retry 协议

**位置：** `dozer/crates/dozer-app/src/platform/window_events.rs:1674` 调 `SettingsOverlay::open`；`bytehost/crates/bytehost-desktop/src/overlay.rs:139`、`overlay_gpu.rs:104–114`。

Dozer 驱动虽然开始消费 Create / Destroy / Activate，但 SettingsOverlay::open 仍返回 Self，原生 create_window、GPU create_surface / format 仍使用 expect。发生真正的创建 / 渲染初始化失败时会沿事件循环 panic，而不是向共享控制器报告 Failed。settings_native_failed 方法存在和 fake 状态单测通过，不代表生产失败路径已经接上。

**需要修复：** 为本次设置迁移提供可返回结构化错误的共享建窗 / GPU 初始化接缝，Dozer 驱动报告失败、清理部分创建资源并暴露可重试入口。不能只在重复创建时调用 settings_native_failed；也不要为此破坏其他尚未迁移弹窗的兼容调用。

**回归：** 在真实 driver 接缝注入 create_window 和 GPU 初始化失败，应用继续运行、无孤儿窗口、父窗输入恢复、Failed 可见并能 Retry。实机故障注入记录与纯状态测试分开。

### RR07 · P1：R08 仍未解决，固定 tag 缺 crate，干净构建仍失败

**位置：** `digger/Cargo.toml:44`，Dozer 对应共享依赖声明。

声明仍指向 bytehost v0.4.22，该版本没有 bytehost-git-accounts。本轮重新复制源码排除 `.cargo`，保留锁文件并执行 `cargo check --locked --offline -p digger-desktop`，再次立即报 `no matching package named bytehost-git-accounts found`，定位到 v0.4.22。开发方也把 A11 标为受阻，现状不能算可交付。

**需要修复 / 验收：** 修完公共 API 与双产品接线后按实施授权发布统一快照，同步全部消费者 pin / lock，在无个人 patch 的隔离配置中用实际 tag SHA 完成 locked 编译、行为与依赖门禁。不能移动现有 tag 或提交机器路径补丁替代。

## 本轮证据与边界

| 检查 | 审核方本轮结果 |
|---|---|
| 共享账户 / 设置原有测试，隔离纯 workspace、默认 feature | 30 + 48 通过 |
| Digger 设置相关测试，隔离源码与本地共享库 patch | 20 通过，含敏感 Debug 回归；另跑 binding_ 4 项通过，其中包括产品旧回包 / 失败可见回归 |
| Created / resize / timeout 故障探针 | 4 通过，断言 RR01 / RR04 当前缺陷存在 |
| 账户服务探针 | 2 个确认 RR02 / RR03 缺陷，1 个两线程重叠验证原 R06 修复 |
| 干净无 patch locked 编译 | 失败，RR07 |
| Dozer 完整测试 / 新版全部 GUI | 本轮未独立复跑；不把开发方的全量结果写成审核方验证 |
| 原生 / IME / 多屏实机 | 未执行；RR05 / RR06 来自生产分支静态核查 |

[原始日志与探针源码](artifacts/2026-11-03-unified-settings-dialog-review-round2/product-probes.log)已保存。探针只用临时目录、fake HTTP、内存凭据和人工字符串；没有调用真实 Keychain、连接外部账户或创建 GUI 窗口。故障探针的 PASS 表示确认缺陷，不是验收成功。转成正式测试时必须断言正确行为。

临时复现目录：`/private/tmp/digger-unified-round2-probes`、`/private/tmp/digger-unified-round2-product`、`/private/tmp/bytehost-unified-round2`；本机依赖环境和局限沿用第一轮，未修改原工作区配置 / Cargo.lock。

## 尚未完成的交付证据

- `settings_product` 目前明确不建窗口，只构造 fake 登录和共享 Git Element，也没有注册 Theme / Git / 自定义页的真实驱动示例。composition 是另一个视图构造示例。Task 6 对“新产品完整装配与驱动接线”的勾选应收回或补齐真实可运行示例，不能让新项目再次自行拼接出 RR01。
- Task 1 双运行时最小真实窗口、A2–A6 的 IME / 焦点 / 原生 wry 层级 / DPI / 父窗退出尚无实机证据，不能跳过可行性门禁后宣称宿主统一已经完成。
- 计划里仍有未勾选的驱动 / 身份收口 / 原生内容阻断任务，与“R01–R07 都完成”的描述不一致；应按代码与实测更新验收报告，不把所有剩余工作归为环境问题。

**建议顺序：** 先修 RR01 并用 Task 流测试与真实窗口证明初次绑定；修 RR02 / RR03 服务提交和恢复；修 RR04–RR06 驱动失败路径与输入；补双驱动最小示例及实机矩阵；最后 RR07 发布与干净消费收口。不得仅凭已有 738 个库测试通过关闭这些项。
