# 统一设置模块与对话框宿主：代码审核

- 日期：2026-11-03（修复复核：2026-11-03）
- 结论：**R01–R07 的代码修复完成并有自动化回归；R01 / R04 的实机部分待验收。R08 仍受阻：包含新 crate 的版本尚未发布，干净消费者仍不能构建，因此整体仍不能标记发布通过。**
- 依据：[Spec](../specs/2026-11-03-unified-settings-dialog-design.md)、[Plan](../plans/2026-11-03-unified-settings-dialog.md)、[开发报告](2026-11-03-unified-settings-dialog-acceptance.md)。
- 审核源码：Digger `e82c048`、bytehost `729fcaf`、Dozer `b701abeb` 上的当前未提交实现。行号对应审核时工作树。
- 原始审核只增加审核文档和复现证据；修复阶段在三个仓库的工作树内修改源码并补充正式回归测试，没有改真实账户数据。

## 修复结果（复审后）

| 编号 | 状态 | 修复要点 |
|---|---|---|
| R01 | **已修复（实机待验收）** | Dozer 的 `App` 持有共享 `DialogController`，`SettingsOverlay` 只执行 `Create` / `Destroy` / `Activate`；Git 页改用共享 `GitPageState` + `bytehost-settings-ui::git_accounts_page`，删除产品内 `ConnectState` / `provider_row` / 本地连接调度。输入 / 渲染改调共享 `OverlayGpu::{track_and_convert,dispatch,redraw}`。新增独立 `settings_product` 示例。 |
| R02 | **已修复** | 绑定结果按 `DialogKey` 定位实例；旧 session / 旧 attempt 的失败只丢弃。新增 `stale_binding_failure_cannot_close_the_current_dialog` 等产品路径回归。 |
| R03 | **已修复** | 当前 attempt 失败保留 `Failed` + key，只销毁失败实例；主窗口新增可见错误条 + 重试 / 关闭入口。正常关闭等 iced `Closed` 事件确认后才回报控制器，2s 超时兜底。新增 `current_binding_failure_keeps_a_retryable_visible_state` / `destroy_without_a_reply_is_bounded_by_timeout`。 |
| R04 | **已修复（内存诊断待实机）** | 父窗口改为主线程注册表持有 `Retained<NSWindow>`，只在单个回调有效期内使用；校验主线程、父窗仍可见；失败返回 `ParentClosed` / `NotMainThread`，不再跨回调传裸地址。 |
| R05 | **已修复** | 服务层 `Admission` 与 `connect_with_account_at` / `disconnect_at` 接入 Digger 与 Dozer；取消 / 同 provider 新请求撤销准入，网络后不落盘。迟到但已持久化的结果触发权威查询并保留告警。新增网络阶段取消 barrier 测试。 |
| R06 | **已修复** | `JsonFileMetadataStore` 按路径共享事务锁，`update` 在锁内 load-modify-save，磁盘发布用同目录临时文件 + `sync_all` + 原子替换；两 provider 并发回归保留双账户。 |
| R07 | **P2 已修复** | 产品输入消息使用 `SecretToken`（Debug 打码、Drop 清零）；HTTP 错误文本经 `redact_secrets`；新增外层消息 / Token 输入回归。 |
| R08 | **仍受阻** | 尚未发布包含 `bytehost-git-accounts` 的 tag；Digger / Dozer 仍钉 `v0.4.22`，干净 checkout 仍会失败。本轮没有伪造版本号或提交个人 path patch。 |

修复后验证见 [`fix-verification.log`](artifacts/2026-11-03-unified-settings-dialog-review/fix-verification.log)；原 `product-probes.rs` / `account-probes.rs` 是“缺陷存在性”探针，已由正式回归测试取代，保留为历史证据。

## Findings

### R01 · P1：Dozer 仍使用旧生命周期和 Git 页面，核心统一目标未交付

**位置：** `dozer/crates/dozer-app/src/platform/window_events.rs:1638`、`platform/settings_overlay.rs:47`、`extensions/settings.rs:1100`。

Dozer 仍按 `app.settings.is_some()` / 自己的 `sync_action` 开关 SettingsOverlay，没有消费共享 DialogController；Git 页仍调用三次本地 `provider_row`，连接调度也保留在 `extensions/settings.rs`。当前共享状态 / Git 视图只被 Digger 和 composition 示例消费。公共 desktop 模块也只是 iced 的绑定辅助，不是已完成的两驱动宿主。因此共享几何和 PageId 并不能证明两个产品已统一。

开发报告末尾承认这些剩余工作，但顶部写“代码实施完成”，A2 又写“两驱动共用控制器 / 几何 / 视图”，与代码不符。最小双驱动实机验证、独立产品装配示例、输入 / 焦点 / wry 生命周期接线也缺乏完成证据。

**继续工作：** 完成 Task 1 / 3 / 6 / 7 的生产接线和示例，让 Dozer 使用同一控制器与 GitPageState / Git 页；产品只保留业务和运行时适配，公共输入 / 关闭策略不能继续双份。修正验收状态，分别写实现缺口和环境未验证项。

**回归：** 两产品重复打开、旧结果、失败重试、关闭中重开与 Git 三 provider 运行同一契约；两个运行时的真实最小窗口按 A2–A6 验收。缺实机条件可以记录受限，但不能将尚未实现的 Dozer 迁移标为通过。

### R02 · P1：旧会话绑定失败会关闭当前新会话

**位置：** `digger/crates/desktop/src/product.rs:1511–1516`。

`on_bind_failed(key, detail)` 自身会拒绝旧 key，但产品随后无条件 `self.settings.window.take()` 并关闭它。只要旧 session / attempt 的 Bound(Err) 在新窗口打开后送达，新窗口就被销毁。共享控制器的身份过滤被产品清理分支绕过。

隔离产品探针实际构造：旧窗口关闭 → 新 session 打开并绑定 → 投递旧 key 的 Bound(Err)。结果是新句柄变为 None、当前 phase 变为 Closed。现有 `stale_bind_results_cannot_open_or_fail_a_new_session` 只测 State 方法，没有覆盖真正的 ProductShell 分支。

**继续工作：** 在任何清理前校验完整 DialogKey 与对应原生实例；销毁旧实例时按 effect 的 key / window 定位，不能用“当前 window”代替。成功、失败、Closed 回包都应携带并校验实际实例身份。

**回归：** 将探针改成正确性断言：旧 session 和旧 attempt 的成功 / 失败 / 关闭结果不得影响新句柄、状态或焦点；若旧成功带来了资源，必须只销毁旧资源。

### R03 · P1：当前绑定失败被清成 Closed，错误与显式重试入口一起消失

**位置：** `digger/crates/desktop/src/product.rs:1513–1521`；`settings/mod.rs:229–237`。

当前 key 绑定失败先进入 Failed，但产品紧接着调用 `on_native_closed`，后者向共享控制器发送当前 key 的 Closed，把身份和 Failed 状态清空。随后设置窗口被关闭，而错误和“重试”只画在设置窗口里的 `settings_view`，主窗口没有该失败入口。用户实际只能看到设置消失，无法使用报告声称的失败后显式重试。

隔离产品探针确认 Bound(current, Err) 后 `bind_error` 虽有值，但 `window == None`、phase 为 Closed、key 为 None。现有 State 单测只直接调用 `on_bind_failed`，因此误以为产品路径仍保留 Failed。

此外 Destroy 分支（product.rs:1378）在执行原生 close 之前就主动回报 Closed，不能证明句柄已释放；旧 / 当前实例分离和有界销毁需要在真实驱动接缝完成。

**继续工作：** 分离“失败资源已清理”与“会话正常关闭”语义；保留可重试的 Failed 会话及有效的可见错误入口。Closed 应来自对应实例的实际销毁确认；丢回包走有界超时。创建失败后即使没有设置窗，也必须在父窗可见并可重试。

**回归：** 通过 ProductShell 实际消息路径注入创建、绑定失败，断言错误可见、模态解除、Retry 推进同 session 的 attempt、旧结果无效，成功后清除错误；验证清理失败 / 超时。

### R04 · P1：NSWindow 裸指针跨两个运行时回调，没有生命周期保障

**位置：** `bytehost/crates/bytehost-desktop/src/dialog.rs:83–87`、`:131–145`；detach 路径同样存在。

第一个 `window::run(parent)` 把借用得到的 NSWindow 地址转成 usize，返回后再调第二个 `window::run(child)`。第二个回调只保证 child 的生命周期，parent 可能已在两个回调之间关闭 / 释放。`attach_natively` 却无条件把旧地址转回 `&NSWindow` 并发消息，存在 use-after-free / UB 风险。主线程执行不能延长已释放对象的生命周期；注释“两个指针都由 window::run 保证存活”不成立。

这是静态安全性问题，本轮没有故意触发原生崩溃，也没有将其描述为已复现 crash。

**继续工作：** 在共享驱动中建立明确的原生对象所有权 / 主线程注册表，或在保证父子同时有效的操作范围完成绑定；不能把无 retain 的地址经异步 Task 传递。执行时还需验证 parent / session 仍有效，已退出的父窗不能重新挂回。

**回归：** 两个回调之间关闭父窗、关闭 child、切换会话；操作安全结束且不绑定失效对象。实机故障注入或内存诊断结果独立记录，不只测 `supported()`。

### R05 · P1：取消仅丢弃 UI 回包，请求仍会提交凭据

**位置：** `digger/crates/desktop/src/product.rs:1628–1630`；`bytehost-git-accounts/src/service.rs:118–120`。

Cancel 只调用 `GitPageState::cancel`，没有停止正在执行的 Task / HTTP，也没有给 AccountService 传取消或提交准入身份。服务在网络返回后无条件写 Keychain / 元数据。关窗也只让页面 ticket 失效，提交后的结果仍可能被 UI 丢弃，没有独立的长寿命事实 owner。

barrier 探针在网络等待时取消，然后释放响应：凭据仍保存，`connect_succeeded` 因 ticket 已失效而返回 false，页面仍显示未连接。更危险的同 provider 交错是：取消 A → 提交 B → B 完成 → A 后完成，旧凭据会覆盖 B，UI 却只接收 B 的结果。

**继续工作：** 实现服务层的取消 / 提交准入边界与同 provider 请求顺序；网络阶段取消后不得开始持久化。已经进入持久化的请求由长期 owner 收尾、保存部分提交诊断并触发权威查询，不能简单丢弃事实。Digger 当前每次创建新 service 实例，也需收口所有权。

**回归：** 使用可控 HTTP barrier 覆盖取消 / 关窗 / 同 provider 新旧请求逆序，以及持久化开始后的关窗；同时断言凭据、元数据、UI 和重开查询，不只断言旧回包被忽略。

### R06 · P1：多 provider 并发保存会丢账户，成功结果不可靠

**位置：** `bytehost/crates/bytehost-git-accounts/src/service.rs:128–138`；`stores.rs:106–118`。

连接使用独立的 metadata.load → 修改单 provider → metadata.save 全文件覆盖。各 provider 可以并发，Digger 也为每个请求创建独立 service；没有共享的事务 / 原子更新边界。GitHub、Gitee 都读到空快照后各写一次，后写覆盖先写，两个结果却都为 `metadata_committed=true`。disconnect 与 connect 同样可互相覆盖。

确定性探针在两个 load 后设置 barrier，甚至把 save 单独串行化，仍稳定得到：两次成功、两份凭据、仅一条账户元数据。说明只给 save 加 Mutex 或只改原子 rename 都不够。真实 JsonFileMetadataStore 直接 `std::fs::write`，并发还可能交错截断；进程中止也可能损坏原文件。

**继续工作：** 引入按产品存储范围共享的事务更新 / 单 owner，完整串行化 load-modify-save 或提供 provider 原子 mutation；磁盘发布采用同目录唯一临时文件、同步、原子替换与错误恢复。不要给每次新建的 service 各加一把互不共享的锁。

**回归：** 两 provider 同连、不同 provider 连 / 断、同 provider 累计变更、写 / 同步 / 替换故障、提交前后中止。两次成功后所有账户均在重开查询中保留，失败不损坏原文件。

### R07 · P2：PAT 绕过 SecretToken，普通消息 Debug 会明文暴露

**位置：** `digger/crates/desktop/src/settings/mod.rs:32`、`:51–52`。

Message 派生 Debug + Clone，`GitToken(GitProvider, String)` 直接包含输入。字段注释“不进入 Debug / 日志”与实际行为相反；外层产品消息也能递归打印它。SecretToken 的打码只保护后续状态和服务参数，保护不了输入事件。

隔离产品探针用人工标记字符串验证 `format!("{message:?}")` 包含完整输入。这里证明的是可被诊断明文输出，未声称生产已有真实 Token 泄漏。

**继续工作：** 在输入转换处进入敏感类型，并为必要消息实现安全 Debug；检查克隆 / 关闭缓存清理、HttpRequest 和网络错误中的 token / query，确保诊断、截图产物无凭据。禁止用“当前没有打印该消息”替代约束。

**回归：** 对输入消息、外层消息、服务请求、错误与取消 / 关闭路径做人工 marker 不出现的测试，不使用真实 PAT。

### R08 · P1：声明仍固定到不含新 crate 的 tag，干净消费者无法构建

**位置：** `digger/Cargo.toml:44`；Dozer 新增依赖同样仍指向 bytehost v0.4.22。

新增 bytehost-git-accounts 仍声明为 `tag = "v0.4.22"`，而这个已发布版本没有新 crate。开发环境只因未提交的个人 path patch 能编译。隔离源码排除 `.cargo`，保留原锁，执行 `cargo check --locked --offline -p digger-desktop` 立即报 `no matching package named bytehost-git-accounts found`，定位到 v0.4.22。

这不是 A11 的通过状态，也不只是缺一份截图：当前变更不能被普通干净 checkout 消费。开发报告虽记录受阻，但“代码实施完成”的顶部结论仍不能覆盖它。

**继续工作：** 先完成并验证公共 API 和两消费者；按实施授权发布包含新能力的统一快照，再同步 Dozer / Digger 及传递库的 pin 和 Cargo.lock。不能移动已有 tag、提交机器路径 patch 或只依赖工作区 Cargo feature 合并。

**回归：** 两产品在隔离配置中用实际发布 tag / SHA 跑 locked all-targets、相关行为与依赖门禁；记录真实结果。发布未完成则保持未完成状态。

## 审核验证与局限

| 验证 | 本轮结果 |
|---|---|
| bytehost-git-accounts 原有测试（隔离纯 workspace，默认 feature） | 23 通过 |
| bytehost-settings 原有测试（同上） | 48 通过 |
| Digger settings::tests（隔离副本，本地共享库 patch） | 7 通过 |
| 新账户故障探针 | 2 通过，分别确认 R05 / R06 缺陷存在 |
| 新 ProductShell 故障探针 | 3 通过，分别确认 R02 / R03 / R07 缺陷存在 |
| 干净无 patch locked 编译 | 失败，R08 |
| 真实 GUI / IME / 原生生命周期 | 本轮未执行；R04 为静态安全性结论，不冒称实机复现 |
| 修复后回归（R01–R07） | bytehost-git-accounts 30、bytehost-settings 48、settings-ui 7、bytehost-desktop 98、digger-desktop 738、dozer settings 47 / git_accounts 5 / project_create 31；`settings_product` 示例运行通过。详见 `fix-verification.log` |
| 修复后 dozer 全量 | 1135 通过、1 失败：`on_activate_refreshes_the_session_list_of_that_project` 期望离线错误，但本机有 `dozerd` 在运行；属环境前提不满足，不是本轮改动。 |

原始日志与探针源码在 [审核证据目录](artifacts/2026-11-03-unified-settings-dialog-review/account-probes.log)。探针只运行人工字符串、内存凭据和临时 JSON；没有访问真实 Keychain、授权第三方账户或修改生产源码。

复现副本位于 `/private/tmp/digger-unified-review`、`/private/tmp/digger-unified-product-review`、`/private/tmp/bytehost-unified-review`。Digger 验证使用可写 target、临时 DIGGER_DATA_DIR；只在副本更新依赖锁。账户 / 状态测试不包含 GUI / HTTP / Keychain 实现 feature 的完整验证，不重复声称开发报告中的全部测试已由审核方执行。

注意：新增探针断言的是“当前缺陷存在”，因此其 PASS 不是验收通过。修复时应把它们改写为期望正确行为的正式回归测试，补在共享服务与真正产品消息路径，不仅修改页面 / 控制器单测。

## 继续开发顺序与复审入口

1. ✅ 已修 R04 原生生命周期安全、R02 / R03 的实例身份和清理协议。
2. ✅ 已收口账户长期 owner，修 R05 / R06，并完成 R07 敏感输入边界。
3. ✅ 已完成 R01 的 Dozer 控制器 / Git 页面 / 双驱动示例；公共模态与输入接线沿用既有驱动，实机部分仍需验证。
4. ⏳ 实机矩阵未执行；R08 发布 / 干净消费验证仍受阻。
5. ✅ 已更新验收报告与计划勾选，保留未执行项和原始证据；本轮回归见 `fix-verification.log`。

原始审核未直接修复；复审阶段已按同一顺序完成 R01–R07 的代码修复与回归，R08 保持发布受阻状态。无需为了本报告重写现有提交或改变产品账户数据。
