# 共享库升级(bytehost v0.4.8 / byteui v0.4.5 / dozer libs-2026-10-18a)Implementation Plan

> **状态(2026-10-18):依赖升级与 `PanelEffect` 处理已落地**——提交 `062d722`(分支 `chore/align-deps-2026-10`,`chore/upgrade-2026-10-17` 指向同一提交)。`Cargo.lock` 已核对:每个共享库只有一份,tag 为 `v0.4.8` / `v0.4.5` / `libs-2026-10-18a` / bytegit `v0.8.1`。**尚未完成的只有:Task 3 的全量测试/clippy/门禁记录、Task 4 的真机回归(WebSocket `ws: hello`、窗口外壳)、Task 5 的收尾汇报。** 下文 Task 0–2 保留作为执行记录/依据,不要重新执行(分支与提交已存在)。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Digger 同时升到 bytehost `v0.4.8`、byteui `v0.4.5`、dozer 库快照 `libs-2026-10-18a`(带 bytegit `v0.8.1`),并让 `PanelEffect` 新增的两个变体编译通过且行为明确;依赖图里每个共享库只剩一个版本。

**Architecture:** 一次提交里同时改三组依赖(ADR 要求 dozer 与 digger 同日升级 bytehost/byteui;`dozer-*` 库 crate 自己依赖 bytehost/byteui,版本不一致会让图里出现两份 bytehost、跨 crate 传的类型对不上)。代码侧只在 `crates/app-host` 的 effects 适配层处理新增变体,按 Digger 嵌入式(进程内)宿主的实际情况落成"由组合根处理的空动作 + 日志"。

**Tech Stack:** Rust、bytehost(`bytehost-{core,chrome,desktop,webview,apps,client,panel}`)、byteui、dozer 库 crate(`dozer-model`、`dozer-services`、`dozer-panel-usage`、`dozer-store-project`)、cargo。

**Spec:** `docs/adr/ADR-011-bytehost-app-host-embedding.md`(嵌入方式与"同日升级"约定);`docs/superpowers/specs/2026-10-16-shared-deps-guard-design.md`(依赖守卫);上游变更记录:bytehost `v0.4.5`(网关 101 响应头修复、字符徽标无关)、`v0.4.6`(`PanelEffect::ConnectionLost/ConnectionRestored`)、`v0.4.7`(重订阅退避、压缩包 `./`)、`v0.4.8`(仅 `bytehost-chrome/Cargo.toml` 把 byteui 钉到 `v0.4.5`,`5626db1`,保证图里只有一份 byteui;其余是文档);byteui `v0.4.5`(新增 `teal` 颜色 token、字符徽标入口)。

## Global Constraints

- **同一个提交里改三组依赖:** `bytehost-*` → `v0.4.8`,`byteui` → `v0.4.5`,`dozer-*` → `libs-2026-10-18a`。不得出现只升其中一组的中间提交(那样依赖图里会有两份 bytehost,构建失败或类型错配)。
- **依赖一律用 git tag;不提交本地 `[patch]`、不提交带本地路径的依赖。**
- **不动工作区里别人未提交的改动:** 主工作区(分支 `chore/align-deps-2026-10`)里 `crates/desktop/src/{main.rs,lifecycle_selftest.rs,markdown/*}` 等有未提交修改,升级一律在**独立 worktree** 里做。
- **前置条件(不在 digger 仓库内):** dozer 已切出并推送 `libs-2026-10-18a`,且该 tag 里各 dozer crate 的依赖是 bytehost `v0.4.8` / byteui `v0.4.5`。没有这个 tag,本计划不能开始(见 Task 0)。
- **Digger 没有 remote:** 本计划不推送、不打 tag;结果只合并到本地分支。
- `PanelEffect` **不是** `#[non_exhaustive]`:新增变体对所有穷尽 `match` 是编译错误,不能指望自动兼容。
- 日志用 Digger 现有的日志方式(不要引入新的日志依赖)。

## Review Focus

1. **依赖图里每个共享库只有一个版本:** `Cargo.lock` 中 `bytehost-*`、`byteui`、`bytegit`、各 `dozer-*` 各只出现一份(按 Task 3 Step 3 的 `Cargo.lock` 计数检查,不用 `cargo tree -d`)。这是同日升级约定想防的事,最容易被"能编译"掩盖。
2. **`libs-2026-10-18a` 真的带着 bytehost `v0.4.8` / byteui `v0.4.5`:** 否则 Task 1 之后图里会有两份,而不是一份。
3. **新增两个变体在组合根被明确处理**,既不 panic、不被静默吞掉成"未处理",也不凭空编造 UI(见 Task 2 的取舍)。
4. **进程内嵌入的 WebSocket 握手:** 网关修复(`v0.4.5`)对 Digger 同样生效——Digger 的 webview 加载的是进程内网关,用 WebSocket 的应用在升级前同样会握手失败。升级后要实测一次。
5. **`Cargo.lock` 里 bytegit 的版本:** `dozer-store-project` 带进来的是 `v0.8.1`(旧本地分支 `chore/bytegit-0.8.0` 针对的是 `libs-2026-10-16c` + bytegit `v0.8.0`);升级后只能有 `v0.8.1` 一份。

---

## 现状与差异(核对于 2026-10-17)

| 依赖 | 现在(`Cargo.toml` 行) | 升级后 |
|---|---|---|
| `bytehost-core/chrome/desktop/webview/apps/client/panel` | `v0.4.1`(29–39 行) | `v0.4.8` |
| `byteui` | `v0.4.4`(42 行) | `v0.4.5` |
| `dozer-model/services/panel-usage/store-project` | `libs-2026-10-16b`(49–53 行) | `libs-2026-10-18a` |

分支关系:主工作区在 `chore/align-deps-2026-10`(= `main` + 1 个对齐提交 `54f9749`);另有 `chore/bytegit-0.8.0`(`eb858d0`,指向 `libs-2026-10-16c`,**未合并**)和 `feat/shared-deps-guard`(`73659ed`,依赖守卫脚本,**未合并**)。本计划以 `chore/align-deps-2026-10` 为基线,**取代** `chore/bytegit-0.8.0`(新 libs tag 已包含 bytegit `v0.8.1`)。

**bytehost 对 Digger 消费的 crate 的实际改动**(`v0.4.1..v0.4.8`,2026-10-17 在 bytehost 仓库核对过):
- **`bytehost-core`、`bytehost-desktop`、`bytehost-webview`、`bytehost-client`:没有任何改动**(`git log v0.4.1..v0.4.8 -- <这些 crate>` 为空)。
- **`bytehost-chrome`:** 只有 `v0.4.8` 的 `5626db1`——`Cargo.toml` 里 byteui 依赖钉到 `v0.4.5`,无源码改动。这正是 Digger 必须用 `v0.4.8` 而不是 `v0.4.7` 的原因(`v0.4.7` 的 chrome 仍依赖旧 byteui,图里会有两份)。
- **`bytehost-apps`:** 网关 101 响应头修复(`14763a7`,`v0.4.5`)、`Logs` 请求的 debug 日志(`1c6ce71`)、压缩包接受开头的 `./`(`378341c`,`v0.4.7`)。
- **`bytehost-panel`:** 审批卡渠道标签改为"来源渠道 本机"(`c097aa2`)、`PanelEffect` 新增 `ConnectionLost`/`ConnectionRestored`(`e8cacf9`,`v0.4.6`)、重订阅退避真正逐档拉长(`b3baed4`,`v0.4.7`)。
- 其余提交是 shared-deps-guard 工具、README 和测试/样例,不进 Digger 消费的 crate。

**Digger 侧受影响的代码(已核对):** 只有 `crates/app-host/src/effects.rs` 的 `Executor::run`(约 223–285 行)对 `PanelEffect<AppKeyId>` 做穷尽 `match`,它把"由组合根处理"的变体归到返回 `false` 的一组。`crates/desktop` 没有对 `PanelEffect` 的穷尽匹配;没有用结构体字面量构造 `ColorTokens`(所以 byteui 新增 `teal` 字段不影响 Digger)。

---

## Task 0(前置,在 dozer 仓库):切出 `libs-2026-10-18a`

本任务不在 digger 里做,只是 Task 1 的入口条件;列在这里是为了让执行者先核对。

- [ ] **Step 1: 确认 tag 已存在于远程。** Run: `git ls-remote --tags https://github.com/byteboyai/dozer libs-2026-10-18a` → Expected: 有一行输出。没有就停下,请 dozer 一侧在 `origin/main`(应是 `7131e1b1` 或更新)上切 tag 并推送(对外动作,需要用户批准)。
- [ ] **Step 2: 核对 tag 内容。** 在一个临时目录里 `git clone --depth 1 --branch libs-2026-10-18a https://github.com/byteboyai/dozer`,检查 `crates/{dozer-model,dozer-services,dozer-panel-usage,dozer-store-project}/Cargo.toml` 及它们依赖的 `dozer-core`/`dozer-panel-host` 等:所有 `byteboyai/bytehost` 的 tag 都是 `v0.4.8`、`byteboyai/byteui` 都是 `v0.4.5`、`byteboyai/bytegit` 是 `v0.8.1`。Expected: 无一处旧版本。任何一处不符合就停下报告。

---

## Task 1: 在独立 worktree 里升依赖,让构建失败暴露出需要改的代码

**Files:**
- Modify: `Cargo.toml`(上表三组共 12 行)、`Cargo.lock`
- 可能 Modify: `docs/**` 里写着旧版本号的地方(见 Step 5)

**Interfaces:** 无新接口。产出:工作区依赖指向新 tag、`Cargo.lock` 里每个共享库一份。

- [ ] **Step 1: 开 worktree。** `git worktree add ../digger-upgrade -b chore/upgrade-2026-10-17 chore/align-deps-2026-10`。Expected: 干净的 worktree,不含主目录里的未提交改动。
- [ ] **Step 2: 复核"这几个 crate 没改"。** 在 bytehost 仓库:`git log --oneline v0.4.1..v0.4.8 -- crates/bytehost-core crates/bytehost-desktop crates/bytehost-webview crates/bytehost-client` → Expected: **无输出**;`-- crates/bytehost-chrome` 只应有 `5626db1`(byteui 钉到 `v0.4.5`)。再 `git diff v0.4.1 v0.4.8 -- crates/bytehost-apps/src/proto.rs` → Expected: 无输出(wire 类型没动)。任何一条不符就停下,先弄清改了什么。把结论写成提交说明里的一句话。
- [ ] **Step 3: 改 `Cargo.toml`。** `bytehost-*` 7 行 `v0.4.1 → v0.4.8`;`byteui` `v0.4.4 → v0.4.5`;`dozer-*` 4 行 `libs-2026-10-16b → libs-2026-10-18a`;顺手把 `Cargo.toml` 里那段说明注释里的 `libs-2026-10-16` 例子更新成新 tag 名。
- [ ] **Step 3b: 更新锁文件。** `cargo update -p bytehost-core -p bytehost-chrome -p bytehost-desktop -p bytehost-webview -p bytehost-apps -p bytehost-client -p bytehost-panel -p byteui -p dozer-model -p dozer-services -p dozer-panel-usage -p dozer-store-project`。审阅 `git diff Cargo.lock`:只应有这些包及其已有依赖边的变化。
- [ ] **Step 4: 构建,确认失败点正是预期的。** Run: `cargo build --workspace`。Expected: **失败**,错误是 `crates/app-host/src/effects.rs` 的 `match effect` "non-exhaustive patterns: `PanelEffect::ConnectionLost` and `PanelEffect::ConnectionRestored` not covered";**没有**别的编译错误。如果还有别的错误,先停下判断——那说明 Step 2 的"没改"结论有误,或 dozer libs tag 带进了别的变化。
- [ ] **Step 5: 查找文档里的旧版本号。** `grep -rn "v0.4.1\|v0.4.4\|libs-2026-10-16" --include=*.md --include=*.toml . | grep -v target`,把"当前依赖版本"的描述更新到新版本(历史说明保持原样,不要改写过去)。

(这一任务不单独提交——Task 2 通过后与它合成同一个提交,见 Global Constraints。)

---

## Task 2: 明确处理 `PanelEffect::ConnectionLost` / `ConnectionRestored`

**背景与取舍:** bytehost 这两个效果表示"已建立过的变更订阅连续失败 / 恢复"。dozer 把它映射成带 key 的 Toast。Digger 是**进程内嵌入**(ADR-011,`InProcess`,没有守护进程),订阅流只会在接收端被丢弃时以 `Disconnected` 收尾(见 `crates/app-host/src/subscription.rs`),运行中真正"断线"的路径几乎不存在。所以这里不凭空设计 UI:归入"由组合根处理"的一组(返回 `false`),并在组合根里**只记一条日志**——语义明确、不静默丢弃、以后真要给用户看再换映射(状态机只发中立效果,换映射只改这一处)。

**Files:**
- Modify: `crates/app-host/src/effects.rs`(`Executor::run` 的 `match`,约 280–285 行;同函数的文档注释约 216 行里"返回 `false` = 由组合根处理(…)"的列表)
- Modify: 组合根里处理 `run` 返回 `false` 的那一处(核对结果:`crates/desktop/src/apps/mod.rs` 的 `self.exec.run(…)` 调用处都忽略返回值,所以只在 effects 适配层写日志(`eprintln!`,带 `// cli-output` 标记,与仓库现有写法一致),不动 desktop)
- Test: `crates/app-host/tests/embedded.rs`(沿用约 433 行 `exec.run(SCOPE, PANEL, 0, PanelEffect::Subscribe)` 那条的写法)

**Interfaces:**
- Consumes: `bytehost_panel::host::PanelEffect::{ConnectionLost, ConnectionRestored}`(`v0.4.6` 起)。
- Produces: `Executor::run` 对这两个变体返回 `false`,并留下一条日志(含"连接中断"/"已恢复"语义与 `scope`/`panel` 标识,不含任何令牌或 URL)。

- [ ] **Step 1: 写失败测试**(`embedded.rs`):`connection_lost_and_restored_are_left_to_the_composition_root`——构造与 433 行同样的 `exec`,断言 `!exec.run(SCOPE, PANEL, 0, PanelEffect::ConnectionLost)` 与 `!exec.run(SCOPE, PANEL, 0, PanelEffect::ConnectionRestored)`;并断言调用它们**不会**向 `requests` 派发任何请求(沿用文件里检查"没有发出请求"的方式)。
- [ ] **Step 2: 运行确认失败。** Run: `cargo test -p digger-app-host --test embedded connection_lost` → Expected: 编译失败(non-exhaustive,即 Task 1 Step 4 的错误)。
- [ ] **Step 3: 实现。** 在 `run` 的 `match` 里把两个变体加进返回 `false` 的那一组;为它们各写一条日志(同文件已有日志用法为准;文案:"应用变更订阅连续失败,宿主侧未处理" / "应用变更订阅已恢复");更新 `run` 的文档注释里"返回 `false`"的变体清单,补上这两个。
- [ ] **Step 4: 运行。** Run: `cargo build --workspace && cargo test -p digger-app-host` → Expected: 全部通过,含新测试。
- [ ] **Step 5: 提交(含 Task 1 的依赖改动,一个提交)。** `git add Cargo.toml Cargo.lock crates docs && git commit -m "chore(deps): bytehost v0.4.1→v0.4.8, byteui v0.4.4→v0.4.5, dozer libs-2026-10-16b→libs-2026-10-18a; handle PanelEffect::ConnectionLost/Restored"`,提交说明里写上 Task 1 Step 2 的审阅结论。

---

## Task 3: 全量验证

- [ ] **Step 1:** `cargo build --workspace && cargo test --workspace`。Expected: 全部通过。记录通过数;任何失败按名字写进报告(含非本次引起的),不要静默略过。
- [ ] **Step 2:** `cargo clippy --workspace --all-targets` 与 `cargo fmt --check`。Expected: 触及的文件无新告警。
- [ ] **Step 3: 依赖图里每个共享库只有一份。** 不要用 `cargo tree -d | grep`——重复的第三方包(如 `windows-sys`)的父节点行里会带着 bytehost 的名字,会误报。直接数 `Cargo.lock`:
  ```bash
  python3 -c "import re,collections;n=collections.Counter(re.findall(r'^name = \"(bytehost-[a-z]+|byteui|bytegit|dozer-[a-z-]+)\"',open('Cargo.lock').read(),re.M));print({k:v for k,v in n.items() if v>1} or 'ok: one version each')"
  ```
  Expected: `ok: one version each`。再确认各自 `source` 的 tag:`grep -A2 'name = "bytehost-apps"' Cargo.lock` 应指向 `tag=v0.4.8`,`byteui` 指向 `v0.4.5`,`bytegit` 只有 `v0.8.1`。
- [ ] **Step 4: 现有门禁。** `bash scripts/check-host-deps.sh && bash scripts/check-host-deps.sh --self-test`。Expected: 通过。`feat/shared-deps-guard` 在独立 worktree `../digger-shared-deps-guard`,未合并;若基线分支上已有 `scripts/check-shared-deps.sh`(来自 `feat/shared-deps-guard`,当前**不在** `chore/align-deps-2026-10` 上),在合并该分支之后也跑一遍;没有就跳过并在报告里写明。

---

## Task 4: 进程内嵌入的真实回归(WebSocket 与窗口外壳)

- [ ] **Step 1:** 在 worktree 里构建并运行 Digger(用隔离的数据目录,不碰真实数据)。装一个会用 WebSocket 的样例应用(bytehost 仓库的 `scripts/samples/py-notes`,页面会连 `/ws` 并显示 `ws: hello`),打开它的面板。Expected: 页面出现 **`ws: hello`**(`v0.4.5` 之前同样的环境是 `ws error`/close 1006)。用 `orca computer --app pid:<GUI pid> --window-id <id>` 截图留证;iced 界面不在辅助功能树里,只能截图,点击要人来做。
- [ ] **Step 2:** 窗口外壳回归:打开 Digger 主窗口,顶栏、左右图标栏、状态栏与升级前(`chore/align-deps-2026-10` 的构建)肉眼一致(byteui `v0.4.5` 只是给 `ColorTokens` 加了 `teal` 字段、新增字符徽标入口并做了纯格式整理;`bytehost-chrome` 只改了 `Cargo.toml` 里的 byteui 版本,无源码改动)。若 `scripts/check-shell-regions.sh` 存在则跑它(它比对 byteui 导出的外壳区域规格与 Dozer 的 `workspace.json`,基线上能跑就跑)。
- [ ] **Step 3:** 记录结果;任何一项不符,先停下,不要为了"过"而改验证。

---

## Task 5: 合并与收尾

- [ ] **Step 1:** 把 `chore/upgrade-2026-10-17` 合并回 `chore/align-deps-2026-10`(快进),**不要**碰主工作区里的未提交改动;若主目录当前有未提交改动阻止了切换分支,留给用户处理,在报告里说明。
- [ ] **Step 2: 处理被取代的分支。** `chore/bytegit-0.8.0`(`eb858d0`)的内容已被新 libs tag(bytegit `v0.8.1`)覆盖:**不要自行删除**,在报告里说明"可删",由用户决定(2026-10-18 核对:该分支与 worktree `../digger-bytegit080` 在本地已不存在,此步只需在报告里确认即可)。
- [ ] **Step 3: 清理** worktree `../digger-upgrade`(合并之后),验证用的临时数据目录与进程。
- [ ] **Step 4: 汇报:** 通过数、依赖图结论、Task 4 的截图结论、`PanelEffect` 两个变体的处理方式(及"以后要给用户看只改这一处")、被取代的分支、哪些没验(如 `check-shared-deps.sh` 不在基线上)。

---

## Self-Review

- **Spec 覆盖:** 依赖三组(Task 1)、新增变体(Task 2)、"每个库一份"(Task 3 Step 3 + Review Focus 1、2、5)、网关修复在嵌入式场景生效(Task 4 + Review Focus 4)、前置 tag(Task 0)、不碰未提交改动(Global Constraints + Task 1 Step 1)。
- **类型一致:** 只涉及 `PanelEffect` 两个变体与 `Executor::run` 的返回约定(`false` = 组合根处理),两处引用一致。
- **风险点:** Task 1 Step 4 要求"失败点恰好是预期的一处";若还有别的编译错误,说明 Step 2 的"没改"结论有误或 dozer libs tag 带进了别的变化,必须先停下处理。`libs-2026-10-18a` 若不存在或内容不对(Task 0),整个计划作废重来。Digger 没有 remote:本计划所有产物只在本地,不存在"推送"步骤。
