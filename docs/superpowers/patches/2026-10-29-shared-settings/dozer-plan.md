# Dozer 共享设置复审修复与发布 Implementation Plan

- 日期：2026-10-29
- 状态：待实施；所有复选框表示 Dozer / bytehost 原仓库中尚待完成或重新核验的工作。
- 目标存放位置：`dozer/docs/superpowers/plans/2026-10-29-shared-settings-review-fixes.md`。
- 目标：将已验证的审批信息与标题本地化修复应用到原仓库，完成 bytehost 共享设置模块发布和 Dozer 的正式依赖接入，并为 Digger 提供可复现的消费版本。
- 设计依据：`digger/docs/superpowers/specs/2026-10-29-shared-settings-design.md`。
- 上轮修复与验证：`digger/docs/superpowers/reviews/2026-10-29-shared-settings-fixes.md`。

## 1. 已知状态与本期范围

Dozer 已有五个设置页的共享接入代码，但尚有未提交修改，不能把当前工作区视为干净的发行基线。两项共享 UI 修复只在临时副本完成，尚未应用到原 bytehost / Dozer：

1. bytehost 的安装审批卡恢复 `plan_view.title`，展示应用名称与 ID。
2. bytehost 应用管理页增加当前界面 locale；Dozer 将实际 locale 传入，替代共享视图中写死的英文解析。

补丁位于 `digger/docs/superpowers/patches/2026-10-29-shared-settings/{bytehost,dozer}.patch`。读取时两份补丁均通过原工作区的 `git apply --check`；修复副本中共享状态 22 项、共享 UI 8 项、Dozer settings 相关 47 项测试通过。这些结果不代替应用补丁后的验证。

bytehost `v0.4.17` 不包含新增设置 crate。此前核查的远端 main 也没有它们；执行时须重新核实远端状态。本期必须完成真实源码发布与统一 pin，不能只把版本字符串改成一个尚不存在的 tag。

范围包括 bytehost 修复与发布、Dozer 适配及回归、向 Digger 交付兼容版本。不重做已经实现的设置框架，不新增页面、Git 账户后端或 daemon 协议，不顺带迁移其他产品功能。

## 2. 执行约束

- 先读取执行时各仓库适用的工作指令，确认 bytehost、Dozer、Digger 的写入范围与发布权限。源码和检查可先完成；未获发布授权时保留完整可审阅变更并明确阻塞，不能宣称发布完成。
- 记录并保留已有 WIP，包括 `Cargo.lock`、版本号、`Info.plist`、`.qoder/` 和 `web/review-trace/`；不得用 reset / checkout 覆盖用户内容，也不把无关改动混入提交。
- 共享设置 crate 不得依赖 `dozer-*` / `digger-*`。服务、数据根、凭据、权限审批与异步操作仍由产品拥有。
- 保留原主题文件、Git 账户 JSON、`dozer-git` Keychain 命名空间、语言权威快照与冲突语义。
- 不复制 `InstallFlowState` 或执行器；不得为适配 UI 绕过产品信任策略、审批计划绑定或数据清理链路。
- 本地 path patch 仅用于联调，不进入发行清单或发行锁文件。不得提交临时副本的绝对路径。
- 测试数量只作本次证据，不要求永远等于历史数字；新增失败需定位，不把未执行的实机检查标为通过。

## Task 0：核对原工作区与补丁基线

**文件：** 新增 `dozer/docs/superpowers/reviews/2026-10-29-shared-settings-review-fixes.md` 作为实施记录。

- [ ] 记录三个仓库的 HEAD、工作区变更、声明依赖、实际 Cargo patch / lock 来源。
- [ ] 核实 bytehost 的两个新 crate 已存在；若仍未跟踪，发布提交必须包含它们的完整源码与清单。
- [ ] 核实 Dozer 设置容器、五页注册、共享应用视图、语言状态及调用点与补丁前置状态一致。
- [ ] 从 Dozer 根目录检查补丁：

```bash
git -C ../bytehost apply --check ../digger/docs/superpowers/patches/2026-10-29-shared-settings/bytehost.patch
git apply --check ../digger/docs/superpowers/patches/2026-10-29-shared-settings/dozer.patch
```

- [ ] 如检查失败，先判断是否已应用或源码已演进。逐处核对后移植同样的修复，不能强行套补丁或覆盖整文件。
- [ ] 记录当前相关测试基线和已有失败。

**完成条件：** 修复来源、工作区归属和基线明确；没有覆盖已有 WIP。

## Task 1：将共享 UI 修复应用到 bytehost

**文件：**

- `bytehost/crates/bytehost-settings-ui/src/apps/mod.rs`
- `bytehost/crates/bytehost-settings-ui/src/apps/rows.rs`

- [ ] 应用 `bytehost.patch`，或在演进后的代码中实现同等修复。
- [ ] 审批卡标题同时展示安装动作、`plan_view.title` 中的应用名称与 ID；保留原有来源、权限、enforcement、升级提示与批准消息。
- [ ] `AppsView` 接收实际 UI locale；`AppsView::new` 保留默认英文，避免改变 Digger 当前语言行为。
- [ ] 应用标题按传入 locale 调用 `resolve_title`，无投影或无对应 scope 时回退 `app.name`；不重新实现共享库的语言协商规则。
- [ ] 保留并运行中英文标题及无投影回退的测试；确认补丁外的 `AppsView` 字面量调用点也能编译。
- [ ] 在 bytehost 根目录运行：

```bash
cargo test -p bytehost-settings -p bytehost-settings-ui
cargo check -p bytehost-settings-ui --examples
cargo tree -p bytehost-settings-ui -e normal,build
```

**完成条件：** 审批目标身份可见；标题跟随传入语言；共享依赖闭包无产品 crate。

## Task 2：应用 Dozer 的 locale 适配

**文件：** `dozer/crates/dozer-app/src/extensions/settings_apps_view.rs`。

- [ ] 先完成 Task 1，并让本地联调实际使用修复后的 bytehost，避免新旧接口混用。
- [ ] 从 Dozer 根目录应用：

```bash
git apply ../digger/docs/superpowers/patches/2026-10-29-shared-settings/dozer.patch
```

- [ ] 在 `AppsView` 中传入 `locale: locale.to_owned()`。保留现有 `with_locale` 文案解析、消息映射、日志滚动 ID 及唯一状态实例。
- [ ] 核实设置页收到的 locale 来自当前产品界面语言，不以英文 fallback、系统原始语言或另一份缓存替代。
- [ ] 运行 `cargo test -p dozer-app settings` 和 `cargo check -p dozer-app`。

**完成条件：** Dozer 以真实 locale 消费共享视图，设置与服务调用链保持一致。

## Task 3：回归原五页与异步生命周期

**文件：** 按实际缺口补 Dozer / bytehost 现有测试；结果写入实施记录。

- [ ] Theme：选择即时生效、写入失败可见、重启恢复；相关预览刷新与原有配置路径不变。
- [ ] Git：取消连接真正 abort，迟到结果不恢复连接；元数据写入失败时不删除 Keychain token。
- [ ] Advanced：只统计 alive 会话；停止必须确认；停止 / 重启失败可见；重开设置按实际 daemon 状态恢复。
- [ ] Language：保存中禁止重复提交，失败可重试，冲突采纳权威快照，unsupported 不继续写入；关窗后的成功结果仍更新产品语言。
- [ ] Apps：审批卡名称与 ID 正确；在 English / 简体中文之间切换时，有双语投影的应用标题跟随变化；没有投影时保持中立名称。
- [ ] 安装 / 卸载在关窗后完成时仍刷新主窗、发出通知，并仅在成功的 `ProgramAndData` 操作后清理数据；重开不重放操作。
- [ ] 跑 Dozer app 回归及仓库现有相关架构门禁。并行负载导致的超时需单独复现并记录；不能直接忽略真实失败。
- [ ] 实机检查原生窗口层级、Esc、失焦保持、重复打开、中文 IME / 粘贴、小窗口与高 DPI。没有图形会话时保留未验证状态。

**完成条件：** 共享迁移未破坏 Dozer 原五页行为；机器验证与实机验证分别有证据。

## Task 4：发布真正包含修复的 bytehost 版本

**文件：** bytehost 完整共享设置源码、依赖清单、发布记录；Dozer 中所有 bytehost 依赖声明。

- [ ] 检查 bytehost 发布变更包含新增的两个 crate，而不只是本次两份源文件差异。
- [ ] 确认与 iced 0.14、byteui 当前版本兼容，记录验证结果与回退版本。
- [ ] 在获得发布授权后，发布可获取的不可变 tag / rev；发布前完整准备并审阅提交，不能覆盖已有 tag。
- [ ] 验证目标引用确实包含 `crates/bytehost-settings/Cargo.toml`、`crates/bytehost-settings-ui/Cargo.toml` 及本次修复。
- [ ] 将 Dozer 内所有 bytehost 直接依赖统一到同一个真实 tag / rev，包含 panel 等库 crate 的传递依赖声明；不只更新 `dozer-app` 的两个新依赖。
- [ ] 在无本地 path patch 的隔离 checkout 中重新生成并检查 Cargo.lock，运行 Dozer 构建、相关测试与共享依赖门禁。
- [ ] 检查 `cargo tree -d`，确保 byteui、bytehost 核心类型没有因不同 Git 来源或 pin 分裂。

**完成条件：** Dozer 干净消费者能从声明的版本构建；发行锁文件不依赖本机目录。

## Task 5：向 Digger 交付兼容依赖组

**文件：** Dozer 共享库发布记录、Digger Cargo 清单与锁文件。

- [ ] 判断 bytehost pin 更新是否要求发布新的 Dozer 共享库快照，避免 Digger 的直接依赖与面板库传递依赖引用不同 bytehost 版本。
- [ ] 发布前核对 Digger 当前使用的页内查找 API。其本地来源是 `dozer-browser-find`，不能把不含该 API 的旧源码当作可替换快照；缺失时明确列为整合前置，不盲目合并不相关分支。
- [ ] 获得发布授权后生成包含所需 API 与统一 bytehost 依赖的 Dozer 库快照，记录确切 tag / rev。
- [ ] Digger 更新全部同源 bytehost / Dozer pin，在无个人 path patch 的隔离 checkout 中构建并运行设置、应用宿主相关回归与共享依赖门禁。
- [ ] 核实 Digger 在设置与原应用面板之间复用同一状态、服务、审批和清理链；本次共享 UI 变化不改变它的默认语言行为。

**完成条件：** 提供一组 Dozer / Digger 均可复现的兼容版本。若 Digger 尚未脱离 patch，不得标记跨产品发行完成。

## Task 6：收尾与验收

- [ ] 实施记录列出实际修改文件、测试命令与结果、实机证据、确切发布引用和未完成项。
- [ ] 在 Dozer 文档索引中增加本计划与实施记录入口；回填共享 spec / plan 的实际状态。
- [ ] 若修复已由新源码取代补丁，注明补丁对应的基线，避免后续重复应用。
- [ ] 记录整组依赖的回退方式；回退不删除用户配置、凭据或应用数据。
- [ ] 只有原仓库代码已应用、机器回归通过、必需实机项通过且干净消费者可构建时，才标记整体完成。

## 验收映射

| 原审核问题 / 目标 | 本计划覆盖 |
|---|---|
| 审批卡缺少应用名称与 ID | Task 1、3 |
| 共享列表硬编码英文标题 | Task 1、2、3 |
| Dozer 原五页行为与服务边界 | Task 2、3 |
| 新设置 crate 指向不包含它们的旧 tag | Task 4 |
| Digger 兼容版本与无 patch 构建 | Task 5 |
| 真实实施证据、发布与回退 | Task 6 |
