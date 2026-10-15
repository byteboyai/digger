# Project 实体、注册表与共享记忆:提成共享库,Digger 接入

- 日期:2026-10-15
- 状态:已实现(待用户目测验收:文件夹选择器与页签行);实施计划 `docs/superpowers/plans/2026-10-15-project-host.md`
- 路径:架构级(新增共享存储 crate,拆分共享服务 trait,Digger 首次引入 SQLite,新增 ADR)
- 编号:S3b 之前的前置子项目(暂记 S3p)。S3b(todo 面板接入 Digger)排在本项目之后
- 相关:`2026-10-14-window-shell-design.md`(S1)、`2026-10-15-usage-pair-design.md`(S2)、`docs/adr/ADR-003-project-sqlite-consistency-island.md`;Dozer 侧 `../dozer-panel-todo/docs/superpowers/specs/2026-10-15-panel-todo-lib-design.md`(S3a)

## 1. 用户意图

做 S3b 时发现 todo 的每个服务方法都要 `project_id`,而 Digger 没有 Project。用户的决定:

- **Project 整体是 host 的业务层**,不是各应用自有概念。单项目的应用也保留 Project 结构,只是只有一个 id。
- Digger **重用 Dozer 的 Project**,不另起一套。
- **Project 级共享记忆是 Project 不可分割的一部分**,与实体、注册表同在这个子项目里。
- **Dozer 与 Digger 运行时完全独立**:两个进程、各自的数据文件,谁也不依赖谁在不在运行。共享的只有代码(编译期 git `rev` 依赖)。
- **Digger 的 Project 就是磁盘上的一个目录**,`ProjectInfo.path` 原样保留。
- 先单独做 Project,再做 S3b。

## 2. 非目标

- 不做 Project 面板的界面(链接、脚手架、新建项目、删除、转录与总结批次、记忆的界面),那是 S5。
- 不让现有面板按项目取数据:S1 的布局、S2 的用量账本保持全局/应用级。"按 scope id 拆分"留给 S5。
- 不把"已打开页签 + 当前页签"放进 `bytehost-chrome`(Dozer 迁移到共享顶栏时再上提)。
- 不改 Dozer 任何用户可见行为,不改它的表结构与 `dozerd` 协议。
- 不做 S3b(todo 接入),不决定 todo 的语义。
- 不重新设计创作域的一致性岛(见 §7)。

## 3. 现状

| 部分 | 位置 | 说明 |
|---|---|---|
| 类型 | `dozer-model`:`ProjectInfo`、`MemoryInfo`、`MemoryDetail`、`MemoryHistoryEntry` | 已共享 |
| 服务接缝 | `dozer-services`:`ProjectService`、`MemoryService` | 已共享,但 `ProjectService` 把注册表(`open_project`、`list_projects`、`rename_project`、`remove_project`)与 Dozer 的转录/总结批次(`delete_project_transcripts`、`backfill_project_transcripts`、`submit_summary_batch`、`get_summary_batch`)混在一起 |
| 存储实现 | `dozerd/src/projects.rs`(434 行,`ProjectStore`)、`dozerd/src/memory.rs`(393 行,`MemoryStore`) | SQLite(`rusqlite`);`ProjectStore` 用 `bytegit` 取最新 commit 时间。只依赖 `rusqlite`、`bytegit` 与类型,不碰 PTY/终端等守护进程内部 |
| Digger 侧 | `digger-store` 只有用量账本(文件存储);`paths.rs` 声明与 Dozer 数据根完全独立 | 没有 Project,没有 SQLite |
| 顶栏 | `bytehost-chrome` 的顶栏只给一个由产品填充的 `tabs` 槽位,没有项目概念 | S1 里项目页签行内容留空,"＋"置灰 |

## 4. 设计:共享库(Dozer 仓库)

### 4.1 新 crate `dozer-store-project`

- 内容:`ProjectStore`(登记、列表、改名、取消登记、`path_of`)与 `MemoryStore`(list、get、write、delete、history)。表结构与现在逐字一致,只换位置。
- 依赖:`rusqlite`、`bytegit`、`dozer-model`、`dozer-services`、`anyhow`。**依赖闭包不含** `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`。`dozerd` 现在从 `dozer_core::protocol` 取类型,新库直接用 `dozer-model`(`protocol` 本来就是重导出,同一份类型)。
- 提供**进程内服务实现**:`LocalProjectService`、`LocalMemoryService`,分别包住两个 Store、实现对应 trait。Digger 直接使用;`dozerd` 仍走它现有的服务路径,只把 Store 的来源改成新库。`open_project` 先规整路径(去掉末尾分隔符与 `.` 段),避免同一目录因写法不同登记成两个项目;`rename_project` 去首尾空白、拒绝空名字(与 `dozerd` 的 `RenameProject` 同规则)。
- 被否决:塞进 `dozer-services`(它是纯 trait 接缝,塞入 SQLite 会让所有面板库背上 `rusqlite`);两个 Store 各开一个 crate(它们总一起用,拆开无收益)。

### 4.2 拆分 `ProjectService`

- `ProjectService` 只保留注册表四个方法:`open_project`、`list_projects`、`rename_project`、`remove_project`。
- 转录补录/删除、总结批次共四个方法移到新 trait `TranscriptService`(暂名),留给 Dozer。Dozer 的 `Services` 聚合同时持有两个,调用方改为通过对应字段调用,行为不变。
- Digger 只实现 `ProjectService`,不会被迫实现无意义的方法。

## 5. 设计:Digger 一侧

1. **存储位置**:`paths.rs` 新增文件名常量 `PROJECTS_DB_FILE`(`projects.db`)与 `PROJECT_TABS_FILE`(`project_tabs.json`),位于 `<config>/ai.byteboy.digger/` 下,与 Dozer 目录不重叠。两个存储用**同一个** SQLite 文件(各持一个连接,表互不相交),与 `dozerd` 把所有 store 放进 `dozer.db` 同型,表结构逐字相同。`digger-store` 构造 `LocalProjectService`、`LocalMemoryService`,`digger-desktop` 持有。Digger 新增依赖 `dozer-store-project`(带来 `rusqlite`、`bytegit`),这是 Digger 第一次引入 SQLite。
2. **打开项目与页签**:顶栏"＋"(S1 里置灰)改为可用,弹原生文件夹选择器,选中目录后 `open_project(path)`,页签行出现该项目。"已打开页签列表 + 当前页签"是 `digger-desktop` 里的一小块状态,与 `ProjectInfo` 一起驱动页签渲染。关闭页签只关页签、不取消登记;`remove_project` 本子项目只提供服务方法与测试,入口在面板(S5)。启动时恢复上次打开的页签与选中项,存在 Digger 自己的小文件里(同 `layout.json`,字段全部 `#[serde(default)]`)。目录已不存在的项目页签标灰,不自动删除。
3. **与已有面板的关系**:信息、应用、用量、编辑器四个面板暂不按项目取数据。当前选中的 `project_id` 以只读值暴露给面板层,S3b 的 todo 面板第一个使用它。
4. **共享记忆**:只提供服务与存储,Digger 暂无界面使用;用测试覆盖 write、get、list、delete 与历史。
5. **目录语义**:Project = 一个目录。`updated_ms` 保持 git 感知(取最新 commit 时间);目录不是 git 仓库,或 commit 早于最后活跃时间时,回落到最后活跃时间(Dozer 已有该逻辑)。共享模型不改字段。

## 6. 分步与验证

分步(实施计划里细化,每一步 Dozer 与 Digger 都能编译且测试全绿):
1. Dozer 基线:记录 `dozerd`、`dozer-services` 相关测试数量与结果。
2. 建 `dozer-store-project`,搬两个 Store 及其测试,`dozerd` 改引用。
3. 拆 `ProjectService`/`TranscriptService`,Dozer 调用方改字段。
4. 加 `LocalProjectService`、`LocalMemoryService`。
5. Dozer 一侧门禁与脚本登记新 crate(参照 S3a 对 `dozer-panel-todo` 的做法)。
6. Digger:依赖、`paths.rs`、`digger-store` 装配、页签状态与文件夹选择器、页签持久化。
7. 新增 ADR-014,标注 ADR-003 部分被取代,同步 S1 spec 里 S5 一行。

验证:
- **测试数量守恒**:搬走的 Dozer 测试随代码走、期望值不改,基线与终态总数一致或只增不减,全绿。
- **依赖闭包检查**:新库 `cargo tree` 不含 `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`。
- **Digger 侧(临时目录)**:开目录;重复开同一路径取回同一 id;非 git 目录 `updated_ms` 回落;改名;取消登记;记忆写入、读取、删除与历史。
- **独立性测试**:Digger 的两个数据库路径与 Dozer 的目录不重叠。
- **做不到、留给用户**:文件夹选择器与页签行的目测;Dozer 里 Project/记忆相关功能不回归的目测。

## 7. 风险与待办

1. **推送门槛**:Digger 通过 git `rev` 依赖 Dozer,需要 Dozer 上一个已推送的提交。S3a 分支同样未合入、未推送。合入与推送由用户决定,不是本项目的步骤。本项目与 S3a 都基于 Dozer `main` 的 `7cdbffd0`,两条分支会各自动 `dozer-services`,合并顺序要协调。
2. **拆 trait 波及 Dozer 调用方**:`Services` 聚合里加字段,所有用转录/总结方法的地方要改。缓解:机械替换,编译器兜底,测试数量守恒。
3. **Dozer 主线工作树有他人未提交改动**:只在 worktree 里做,不碰主线工作树。
4. **ADR-003 的创作域一致性岛**:本设计让 Project 身份与存储离开 ADR-003,但"创作域状态如何原子提交"的问题仍在。留到出现第一个创作域状态时,单独重新评估,不在本项目里预设。
5. **Digger 首次引入 SQLite 与 `bytegit`**:带来构建时间与跨平台编译(`rusqlite` 用 bundled)的变化,实施第一步先验证 Digger 能编译。
6. **页签状态先放在 Digger 里**,Dozer 迁移共享顶栏时要再上提一次,这是有意接受的重复。
7. **S1 遗留**:S1 的手工验收清单与窗口背景目测仍由用户待做,与本项目无依赖。
