# Project 实体、注册表与共享记忆(S3p)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Dozer 的 Project 注册表与 Project 级共享记忆提成 Dozer/Digger 共用的 `dozer-store-project`,拆开 `ProjectService` 里的转录/总结方法,并让 Digger 用它打开项目文件夹、在顶栏显示和恢复项目页签,给之后的 S3b(todo 面板)一个真实的 `project_id`。

**Architecture:** 在 Dozer 仓库新建 `dozer-store-project`(搬走 `dozerd` 里的 `ProjectStore` 与 `MemoryStore`,加上进程内的 `LocalProjectService` / `LocalMemoryService`);把 `ProjectService` 拆成"注册表"和 `TranscriptService`(转录与总结批次,留给 Dozer)两个 trait。Digger 通过 `digger-store::ProjectHost` 装配它,在自己的数据根里开 `projects.db`,页签状态放在 `digger-desktop` 的 `projects` 模块里,顶栏页签行由它绘制。两个产品运行时互不相识,共享的只有代码。

**Tech Stack:** Rust 2024、rusqlite 0.32(bundled)、bytegit v0.7.0、iced 0.14、rfd 0.15、`dozer-model` / `dozer-services`(Dozer 仓库,git 依赖)、`bytehost-chrome`(S1)。

**Spec:** `docs/superpowers/specs/2026-10-15-project-host-design.md`(Digger 仓库)。执行者先读它;背景在 `docs/superpowers/specs/2026-10-14-window-shell-design.md`(S1)与 `docs/adr/ADR-003-project-sqlite-consistency-island.md`。

**代码验证说明:** 本计划里的 Rust 代码与补丁都已在一次性临时副本里实际编译并跑过测试(Dozer `main` `3666f2bc` 的副本:`cargo check --workspace --tests`、`dozerd` / `dozer-services` / `dozer-client` / `dozer-store-project` 全部测试、`dozer-app` 的 `extensions::project` 95 条与 `panel_host` 22 条、三个门禁;Digger `feat/usage-pair` 的副本,其开发期 `[patch]` 指向打了 Task 3 补丁的 Dozer `main` 副本:`digger-desktop` 编译通过、`digger-desktop` + `digger-store` 全部测试,以及 `scripts/check-host-deps.sh`)。临时副本已丢弃,没有改动任何真实仓库。

## Global Constraints

- **目录约定**:`ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy`;Digger 在 `$ROOT/digger`;Dozer 的新 worktree 在 `$ROOT/dozer-store-project`(分支 `feat/store-project`,从 Dozer `main` 切出,计划写成时是 `3666f2bc`)。
- **不碰这些目录**:`$ROOT/dozer`(主工作树,有他人未提交改动,只许从它 `git worktree add`)、`$ROOT/dozer-panel-todo`(S3a 的 worktree)、`$ROOT/bytehost*`、`$ROOT/byteui*`。
- **不推送、不打 tag、不合并**:Dozer 与 Digger 都一样。合入与推送由用户决定(见 Task 12 末尾的"用户门槛")。
- **Dozer 用户可见行为不变**:搬走的测试期望值不改、不删;`dozerd` 协议、表结构不变。
- **表结构逐字不变**:`projects`(含 `created_ms` 迁移)、`memories`、`memory_history` 的建表语句随文件原样搬,不重写。
- **`dozer-store-project` 的依赖闭包**只含 `dozer-model`、`dozer-services`、`anyhow`、`bytegit`、`rusqlite`;`cargo tree -p dozer-store-project` 里不得出现 `dozer-app` / `dozer-client` / `dozer-core` / `dozerd`。
- **依赖图里只许有一份 `dozer-model`**(Digger 一侧所有 `dozer-*` 依赖必须指向同一个 revision;不同 revision 会让 `ProjectInfo` 类型互不兼容)。
- **Digger 数据文件**:`<config>/projects.db`、`<config>/project_tabs.json`,与 Dozer 的任何目录、端口、数据库都没有关系。
- **Digger 的 Project 就是磁盘上的一个目录**:`ProjectInfo` 的字段一个都不改。
- 注释与现有代码风格一致(中文注释,说"为什么");不留 TODO / 占位。
- 提交信息末尾加:`Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`
- 开工前在每个要改的仓库先 `git status` 记录一次,不要碰与本计划无关的未提交文件。

## Review Focus

以下是 spec 隐含、但没有被某个任务的"主线"测试覆盖的输入或故障;每一条在对应任务里已有一个测试(括号里是任务号):

1. **同一个目录用不同写法打开**(末尾 `/`、`/.`)必须还是同一个项目,不能登记成第二个(Task 4)。
2. **取消登记后重新登记同一目录**拿到的是新 id;旧 id 复用的话,残留在旧 id 下的共享记忆会挂到"新"项目上(Task 4)。
3. **项目目录在页签开着时被删/被移走**:页签标灰保留,不自动删除,也不崩(Task 9)。
4. **文件夹选择器被取消**:状态和提示都不变(Task 9)。
5. **页签文件损坏 / 顶层是数组 / 更高 schema 版本**:用空状态显示,损坏文件下一次变化时可被覆盖,更高版本文件绝不被覆盖(Task 8、9)。
6. **项目数据库打不开**:项目功能整体禁用并说明原因,"＋"不可点,绝不静默退回一个下次启动就不再引用的临时库(Task 7、9)。
7. **注册表读失败 ≠ 没有项目**:读失败时保留现有页签,只提示(Task 9)。

已知且接受的限制(不测试、不在本计划里修):页签很多时顶栏页签行会溢出;`remove_project` 与 Dozer 一样不删该项目下的记忆行(靠 id 不复用保证它们不会串到别的项目)。

---

## File Structure

**Dozer(worktree `$ROOT/dozer-store-project`)**

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/dozer-store-project/Cargo.toml` | 新建 | 依赖闭包受限的存储 crate |
| `crates/dozer-store-project/src/lib.rs` | 新建 | 重导出 |
| `crates/dozer-store-project/src/projects.rs` | 从 `dozerd/src/projects.rs` `git mv` | `ProjectStore`(含原有测试) |
| `crates/dozer-store-project/src/memory.rs` | 从 `dozerd/src/memory.rs` `git mv` | `MemoryStore`(含原有测试) |
| `crates/dozer-store-project/src/service.rs` | 新建 | `LocalProjectService` / `LocalMemoryService` |
| `crates/dozerd/src/projects.rs`、`memory.rs` | 改成一行重导出 | 让 `dozerd` 与它的集成测试路径不变 |
| `crates/dozerd/Cargo.toml` | 改 | 加依赖 |
| `crates/dozer-services/src/lib.rs` | 改 | 拆 `ProjectService` / `TranscriptService`,`Services` 加字段 |
| `crates/dozer-client/src/lib.rs` | 改 | `Client` 分别实现两个 trait |
| `crates/dozer-app/src/**`(5 个文件) | 改 | 调用点与测试替身跟随拆分 |

**Digger(分支 `feat/project-host`)**

| 文件 | 动作 | 职责 |
|---|---|---|
| `Cargo.toml` | 改 | 加 `dozer-store-project` 依赖与开发期 `[patch]` |
| `crates/store/Cargo.toml`、`src/lib.rs` | 改 | 依赖与导出 |
| `crates/store/src/projects.rs` | 新建 | `ProjectHost`:把注册表与记忆装配到一个 SQLite 文件 |
| `crates/desktop/src/paths.rs` | 改 | 两个文件名常量 |
| `crates/desktop/src/projects/tabs.rs` | 新建 | 页签列表 + 当前页签 + 落盘(纯逻辑) |
| `crates/desktop/src/projects/mod.rs`、`tests.rs` | 新建 | `ProjectsState`:消息、更新、顶栏页签行 |
| `crates/desktop/src/product.rs`、`main.rs` | 改 | 接线 |
| `docs/adr/ADR-014-*.md` | 新建 | 决策记录 |
| `docs/adr/ADR-003-*.md`、S1 spec、S3p spec | 改 | 标注与勘误 |

---

# Part A:Dozer 一侧

### Task 1: Worktree 与基线

**Files:** 无代码改动。

- [ ] **Step 1: 记录主仓库状态并建 worktree**

```bash
ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy
git -C $ROOT/dozer status --short | head -20      # 记录一次,别人的改动不要碰
git -C $ROOT/dozer log --oneline -1               # 计划写成时是 3666f2bc
git -C $ROOT/dozer worktree add ../dozer-store-project -b feat/store-project main
cd $ROOT/dozer-store-project && git log --oneline -1
```

Expected: 新目录存在,`git log` 的提交与 `main` 一致,主工作树没有任何变化。

- [ ] **Step 2: 记录基线(写进你的执行记录,后面对账用)**

```bash
cd $ROOT/dozer-store-project
cargo test -p dozerd --lib 2>&1 | grep "test result"
cargo test -p dozerd 2>&1 | grep -E "test result|FAILED"           # 含集成测试
cargo test -p dozer-services -p dozer-client 2>&1 | grep "test result"
cargo test -p dozer-app extensions::project 2>&1 | grep "test result"
cargo test -p dozer-app panel_host 2>&1 | grep "test result"
bash scripts/check-i18n.sh | tail -2
python3 scripts/audit/check_panel_boundary.py | tail -2
bash scripts/check-log-scope.sh | tail -3
```

Expected(计划写成时 `3666f2bc` 上的实测值,只作参照,以你的实测为准):`dozerd` 单元测试 482 条,`dozer-services` 24 条,`extensions::project` 95 条,`panel_host` 22 条;`check-i18n.sh` ok;面板边界检查 ok;`check-log-scope.sh` 有且仅有 1 处已知基线 `crates/dozer-app/src/platform/window_events.rs:2556`。若你的实测与此不同,先弄清原因再继续。

---

### Task 2: 新 crate `dozer-store-project`,搬两个存储

**Files:**
- Create: `crates/dozer-store-project/Cargo.toml`、`src/lib.rs`
- Move: `crates/dozerd/src/projects.rs` → `crates/dozer-store-project/src/projects.rs`;`crates/dozerd/src/memory.rs` → `crates/dozer-store-project/src/memory.rs`
- Modify: `crates/dozerd/src/projects.rs`、`crates/dozerd/src/memory.rs`(改成重导出)、`crates/dozerd/Cargo.toml`

**Interfaces:**
- Produces: `dozer_store_project::{ProjectStore, MemoryStore}`,方法签名与搬走前逐字相同(`ProjectStore::{new,open,list,rename,remove,path_of}`、`MemoryStore::{new,list,get,write,delete,history}`)。`dozerd::projects::ProjectStore` / `dozerd::memory::MemoryStore` 路径继续可用。

- [ ] **Step 1: 搬文件并建 crate**

```bash
cd $ROOT/dozer-store-project
mkdir -p crates/dozer-store-project/src
git mv crates/dozerd/src/projects.rs crates/dozer-store-project/src/projects.rs
git mv crates/dozerd/src/memory.rs   crates/dozer-store-project/src/memory.rs
```

`crates/dozer-store-project/Cargo.toml`:

```toml
[package]
name = "dozer-store-project"
version = "0.1.0"
edition.workspace = true

# Project 注册表与共享记忆的 SQLite 存储 + 进程内服务实现。Dozer(dozerd)与 Digger 共用这份代码,
# 各自打开自己的数据库文件。依赖闭包不得含 dozer-app / dozer-client / dozer-core / dozerd。
[dependencies]
dozer-model = { path = "../dozer-model" }
dozer-services = { path = "../dozer-services" }
anyhow.workspace = true
bytegit = { git = "https://github.com/byteboyai/bytegit", tag = "v0.7.0" }
rusqlite = { version = "0.32", features = ["bundled"] }

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true }
```

`crates/dozer-store-project/src/lib.rs`(本任务只导出两个存储,`service` 在 Task 4 加):

```rust
//! Project 注册表与 Project 级共享记忆的存储。
//!
//! 同一份代码被 `dozerd`(经 UDS 对外服务)与 Digger(进程内直接使用)共用;两个产品各自打开自己的
//! SQLite 文件,运行时互不相识。表结构与搬走前 `dozerd` 里的 `projects` / `memories` / `memory_history` 逐字一致。

mod memory;
mod projects;

pub use memory::MemoryStore;
pub use projects::ProjectStore;
```

- [ ] **Step 2: 两个存储改用 `dozer-model` 的类型**

`dozer_core::protocol` 里的这几个类型本来就是 `dozer_model` 的重导出(同一份类型),只改 `use`:

```bash
cd $ROOT/dozer-store-project/crates/dozer-store-project/src
sed -i '' 's/^use dozer_core::protocol::ProjectInfo;$/use dozer_model::ProjectInfo;/' projects.rs
sed -i '' 's/^use dozer_core::protocol::{MemoryDetail, MemoryHistoryEntry, MemoryInfo};$/use dozer_model::{MemoryDetail, MemoryHistoryEntry, MemoryInfo};/' memory.rs
grep -n "dozer_core" projects.rs memory.rs || echo "no dozer_core left"
```

Expected: `no dozer_core left`。

- [ ] **Step 3: `dozerd` 改成重导出**

`crates/dozerd/src/projects.rs` 全文:

```rust
pub use dozer_store_project::ProjectStore;
```

`crates/dozerd/src/memory.rs` 全文:

```rust
pub use dozer_store_project::MemoryStore;
```

`crates/dozerd/Cargo.toml` 的 `[dependencies]` 里,紧跟 `dozer-i18n` 那一行加:

```toml
dozer-store-project = { path = "../dozer-store-project" }
```

- [ ] **Step 4: 编译并对账测试数量**

```bash
cd $ROOT/dozer-store-project
cargo check --workspace --tests 2>&1 | grep -E "^error" -A8 | head -30; echo checked
cargo test -p dozer-store-project 2>&1 | grep "test result"
cargo test -p dozerd --lib 2>&1 | grep "test result"
cargo test -p dozerd 2>&1 | grep -E "test result|FAILED"
```

Expected: `cargo check` 无 error;`dozer-store-project` **23** 条通过;`dozerd` 单元测试 = Task 1 基线 **减 23**;`dozerd` 的集成测试(`tests/*.rs`)条数与基线相同且全绿。若搬走的条数不是 23,停下来查原因(说明有测试遗漏或 `mod tests` 依赖了 `dozerd` 内部项)。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: move ProjectStore and MemoryStore into dozer-store-project

The two stores only need rusqlite, bytegit and dozer-model, so they leave
the daemon crate verbatim (types switch from the dozer_core::protocol
re-export to dozer_model). dozerd re-exports them under the old paths.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 拆 `ProjectService` 与 `TranscriptService`

**Files:** `crates/dozer-services/src/lib.rs`、`crates/dozer-client/src/lib.rs`、`crates/dozer-app/src/host_adapters/services.rs`、`crates/dozer-app/src/service_fakes.rs`、`crates/dozer-app/src/extensions/project/update.rs`、`crates/dozer-app/src/extensions/project/delete.rs`、`crates/dozer-app/src/app/update.rs`、`crates/dozer-app/src/extensions/project.rs`

**Interfaces:**
- Produces:
  - `dozer_services::ProjectService`:只有 `rename_project(&self, id: i64, name: String)`、`remove_project(&self, id: i64)`、`open_project(&self, path: String)`、`list_projects(&self)`。
  - `dozer_services::TranscriptService`:`delete_project_transcripts(&self, cwd: String)`、`backfill_project_transcripts(&self, cwd: String)`、`submit_summary_batch(&self, cwd: String, provider: Option<AgentKind>, model: Option<String>)`、`get_summary_batch(&self, batch_id: i64)`,签名与拆分前逐字相同。
  - `Services.transcript: Arc<dyn TranscriptService>`(紧跟 `project`)。
  - `Client` 同时实现两个 trait;`FakeProject` 同时实现两个 trait(共享同一份状态)。
  - `spawn_scaffold_run(repo_path, transcript, handle, emit, skip_git_init)`、`spawn_repair_run(repo_path, project_id, transcript, handle, emit)`、`spawn_delete_project(project_id, repo_path, scope, project, transcript, handle, on_done)`。

这是一次纯机械的拆分,编译器会兜底;下面的补丁就是全部改动。

- [ ] **Step 1: 保存并应用补丁**

把下面整段 diff 存成仓库之外的文件,例如 `$ROOT/split.patch`,然后:

```bash
cd $ROOT/dozer-store-project
git apply --check $ROOT/split.patch && git apply $ROOT/split.patch
git diff --stat | tail -10
```

Expected: 8 个文件被修改。

```diff
--- a/crates/dozer-services/src/lib.rs
+++ b/crates/dozer-services/src/lib.rs
@@ -141,8 +141,8 @@
     ) -> BoxFuture<'_, Result<(), ServiceError>>;
 }
 
-/// 项目维护(Project 面板):改名、取消登记、转录补录/删除、总结批次。
-/// 返回值保持 `Client` 原样——面板用到补录条数、批次 id/进度、改名结果。
+/// 项目注册表(host 层共享业务):登记、列表、改名、取消登记。Project 就是磁盘上的一个目录。
+/// Dozer 经 `dozerd` 实现它,Digger 用 `dozer-store-project` 的进程内实现。
 pub trait ProjectService: Send + Sync {
     fn rename_project(
         &self,
@@ -150,6 +150,18 @@
         name: String,
     ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>>;
     fn remove_project(&self, id: i64) -> BoxFuture<'_, Result<(), ServiceError>>;
+    /// 登记(或取回)一个项目;路径不存在等情形服务端返回 `Ok(None)` 或 `Err`,面板按原样展示。
+    fn open_project(
+        &self,
+        path: String,
+    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>>;
+    /// 项目列表(最近优先),供创建后刷新最近项目。
+    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>>;
+}
+
+/// Dozer 的 agent 转录补录/删除与总结批次。Digger 没有 agent 转录,不实现它。
+/// 返回值保持 `Client` 原样——面板用到补录条数、批次 id/进度。
+pub trait TranscriptService: Send + Sync {
     fn delete_project_transcripts(&self, cwd: String) -> BoxFuture<'_, Result<u32, ServiceError>>;
     fn backfill_project_transcripts(&self, cwd: String)
     -> BoxFuture<'_, Result<u32, ServiceError>>;
@@ -163,13 +175,6 @@
         &self,
         batch_id: i64,
     ) -> BoxFuture<'_, Result<Option<SummaryBatchInfo>, ServiceError>>;
-    /// 登记(或取回)一个项目;路径不存在等情形服务端返回 `Ok(None)` 或 `Err`,面板按原样展示。
-    fn open_project(
-        &self,
-        path: String,
-    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>>;
-    /// 项目列表(最近优先),供创建后刷新最近项目。
-    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>>;
 }
 
 /// `post_group_message` 的结果:(人类消息, 排队中的 agent 占位消息, 未识别的 @ 句柄)。
@@ -353,6 +358,7 @@
     pub usage: Arc<dyn UsageService>,
     pub todo: Arc<dyn TodoService>,
     pub project: Arc<dyn ProjectService>,
+    pub transcript: Arc<dyn TranscriptService>,
     pub memory: Arc<dyn MemoryService>,
     pub group_chat: Arc<dyn GroupChatService>,
     pub conversation: Arc<dyn ConversationService>,
@@ -418,6 +424,7 @@
             usage: Arc::new(Mem(Ok(vec![]))),
             todo: Arc::new(MemTodo(Ok(vec![]))),
             project: Arc::new(MemProject),
+            transcript: Arc::new(MemTranscript),
             memory: Arc::new(MemMemory(Ok(vec![]))),
             group_chat: Arc::new(MemGroupChat(Ok(vec![]))),
             conversation: Arc::new(MemConversation(Ok(vec![]))),
@@ -436,6 +443,7 @@
         assert!(Arc::ptr_eq(&s.usage, &t.usage));
         assert!(Arc::ptr_eq(&s.todo, &t.todo));
         assert!(Arc::ptr_eq(&s.project, &t.project));
+        assert!(Arc::ptr_eq(&s.transcript, &t.transcript));
         assert!(Arc::ptr_eq(&s.memory, &t.memory));
         assert!(Arc::ptr_eq(&s.group_chat, &t.group_chat));
         assert!(Arc::ptr_eq(&s.conversation, &t.conversation));
@@ -622,6 +630,20 @@
         fn remove_project(&self, _i: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
             unit()
         }
+        fn open_project(
+            &self,
+            _p: String,
+        ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
+            Box::pin(async { Ok(None) })
+        }
+        fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>> {
+            Box::pin(async { Ok(vec![]) })
+        }
+    }
+
+    struct MemTranscript;
+
+    impl TranscriptService for MemTranscript {
         fn delete_project_transcripts(
             &self,
             _c: String,
@@ -648,31 +670,21 @@
         ) -> BoxFuture<'_, Result<Option<SummaryBatchInfo>, ServiceError>> {
             Box::pin(async { Ok(None) })
         }
-
-        fn open_project(
-            &self,
-            _p: String,
-        ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
-            Box::pin(async { Ok(None) })
-        }
-        fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>> {
-            Box::pin(async { Ok(vec![]) })
-        }
     }
 
     /// `handle.spawn` 要 `'static + Send`:持有 `Arc<dyn …>` 的 async 块必须满足。
     #[test]
-    fn dyn_project_and_memory_futures_run_inside_tokio_spawn() {
+    fn dyn_transcript_and_memory_futures_run_inside_tokio_spawn() {
         let memory: Arc<dyn MemoryService> = Arc::new(MemMemory(Ok(vec![])));
-        let project: Arc<dyn ProjectService> = Arc::new(MemProject);
+        let transcript: Arc<dyn TranscriptService> = Arc::new(MemTranscript);
         let rt = tokio::runtime::Builder::new_current_thread()
             .build()
             .unwrap();
         let (list, backfilled, batch) = rt.block_on(async move {
             tokio::spawn(async move {
                 let list = memory.list_memories(7).await;
-                let backfilled = project.backfill_project_transcripts("/p".into()).await;
-                let batch = project.submit_summary_batch("/p".into(), None, None).await;
+                let backfilled = transcript.backfill_project_transcripts("/p".into()).await;
+                let batch = transcript.submit_summary_batch("/p".into(), None, None).await;
                 (list, backfilled, batch)
             })
             .await
--- a/crates/dozer-client/src/lib.rs
+++ b/crates/dozer-client/src/lib.rs
@@ -12,7 +12,8 @@
 use dozer_services::{
     BookmarkService, BoxFuture, CodeHealthService, ContextService, ConversationRows,
     ConversationService, EditHistoryService, GroupChatService, LocaleService, MemoryService,
-    PreciseEdit, ProjectService, ServiceError, ServiceErrorKind, TodoService, UsageRows,
+    PreciseEdit, ProjectService, ServiceError, ServiceErrorKind, TodoService, TranscriptService,
+    UsageRows,
     UsageService,
 };
 use std::path::PathBuf;
@@ -1871,6 +1872,28 @@
                 .map_err(into_service_error)
         })
     }
+    fn open_project(
+        &self,
+        path: String,
+    ) -> BoxFuture<'_, Result<Option<dozer_core::protocol::ProjectInfo>, ServiceError>> {
+        Box::pin(async move {
+            Client::open_project(self, &path)
+                .await
+                .map_err(into_service_error)
+        })
+    }
+    fn list_projects(
+        &self,
+    ) -> BoxFuture<'_, Result<Vec<dozer_core::protocol::ProjectInfo>, ServiceError>> {
+        Box::pin(async move {
+            Client::list_projects(self)
+                .await
+                .map_err(into_service_error)
+        })
+    }
+}
+
+impl TranscriptService for Client {
     fn delete_project_transcripts(&self, cwd: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
         Box::pin(async move {
             Client::delete_project_transcripts(self, &cwd)
@@ -1910,25 +1933,6 @@
                 .map_err(into_service_error)
         })
     }
-    fn open_project(
-        &self,
-        path: String,
-    ) -> BoxFuture<'_, Result<Option<dozer_core::protocol::ProjectInfo>, ServiceError>> {
-        Box::pin(async move {
-            Client::open_project(self, &path)
-                .await
-                .map_err(into_service_error)
-        })
-    }
-    fn list_projects(
-        &self,
-    ) -> BoxFuture<'_, Result<Vec<dozer_core::protocol::ProjectInfo>, ServiceError>> {
-        Box::pin(async move {
-            Client::list_projects(self)
-                .await
-                .map_err(into_service_error)
-        })
-    }
 }
 
 impl GroupChatService for Client {
@@ -2460,7 +2464,7 @@
 mod project_service_tests {
     use super::todo_service_tests::serve_once;
     use super::*;
-    use dozer_services::{MemoryService, ProjectService, ServiceErrorKind};
+    use dozer_services::{MemoryService, ProjectService, ServiceErrorKind, TranscriptService};
     #[tokio::test]
     async fn list_memories_comes_back_as_ok() {
         let sock = serve_once("mem-list", Some(Reply::Memories { memories: vec![] }));
@@ -2484,7 +2488,7 @@
             Some(Reply::BackfillDone { imported_files: 3 }),
         );
         let got =
-            ProjectService::backfill_project_transcripts(&Client::new(sock.clone()), "/p".into())
+            TranscriptService::backfill_project_transcripts(&Client::new(sock.clone()), "/p".into())
                 .await;
         assert_eq!(got, Ok(3));
         let _ = std::fs::remove_file(&sock);
--- a/crates/dozer-app/src/host_adapters/services.rs
+++ b/crates/dozer-app/src/host_adapters/services.rs
@@ -15,6 +15,7 @@
         usage: Arc::new(client.clone()),
         todo: Arc::new(client.clone()),
         project: Arc::new(client.clone()),
+        transcript: Arc::new(client.clone()),
         memory: Arc::new(client.clone()),
         group_chat: Arc::new(client.clone()),
         conversation: Arc::new(client.clone()),
--- a/crates/dozer-app/src/service_fakes.rs
+++ b/crates/dozer-app/src/service_fakes.rs
@@ -10,6 +10,7 @@
     BookmarkService, BoxFuture, CodeHealthService, ContextService, ConversationRows,
     ConversationService, DaemonService, EditHistoryService, GroupChatService, LocaleService,
     MemoryService, PostedMessages, PreciseEdit, ProjectService, ServiceError, ServiceErrorKind,
+    TranscriptService,
     Services, TodoService,
 };
 use std::sync::{Arc, Mutex};
@@ -187,6 +188,7 @@
         usage: Arc::new(crate::usage_probe::fake_usage::FakeUsage::ok(vec![])),
         todo: Arc::new(FakeTodo::default()),
         project: Arc::new(FakeProject::default()),
+        transcript: Arc::new(FakeProject::default()),
         memory: Arc::new(FakeMemory::default()),
         group_chat: Arc::new(FakeGroupChat::default()),
         conversation: Arc::new(FakeConversation::default()),
@@ -429,7 +431,8 @@
     }
     pub(crate) fn into_services(self) -> Services {
         Services {
-            project: Arc::new(self),
+            project: Arc::new(self.clone()),
+            transcript: Arc::new(self),
             ..base_services()
         }
     }
@@ -452,6 +455,24 @@
         let r = s.remove.clone();
         Box::pin(async move { r })
     }
+    fn open_project(
+        &self,
+        path: String,
+    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
+        let mut s = self.state.lock().unwrap();
+        s.calls.push(format!("open_project({path})"));
+        let r = s.open.clone();
+        Box::pin(async move { r })
+    }
+    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>> {
+        let mut s = self.state.lock().unwrap();
+        s.calls.push("list_projects()".to_string());
+        let r = s.projects.clone();
+        Box::pin(async move { r })
+    }
+}
+
+impl TranscriptService for FakeProject {
     fn delete_project_transcripts(&self, cwd: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
         let mut s = self.state.lock().unwrap();
         s.calls.push(format!("delete_project_transcripts({cwd})"));
@@ -491,21 +512,6 @@
             s.batches[i].clone()
         };
         s.batch_calls += 1;
-        Box::pin(async move { r })
-    }
-    fn open_project(
-        &self,
-        path: String,
-    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
-        let mut s = self.state.lock().unwrap();
-        s.calls.push(format!("open_project({path})"));
-        let r = s.open.clone();
-        Box::pin(async move { r })
-    }
-    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>> {
-        let mut s = self.state.lock().unwrap();
-        s.calls.push("list_projects()".to_string());
-        let r = s.projects.clone();
         Box::pin(async move { r })
     }
 }
--- a/crates/dozer-app/src/extensions/project/update.rs
+++ b/crates/dozer-app/src/extensions/project/update.rs
@@ -2,7 +2,7 @@
 
 use super::*;
 use crate::panel_host::RequestTokenWrapExt;
-use dozer_services::{MemoryService, ProjectService, ServiceErrorKind, Services};
+use dozer_services::{MemoryService, ProjectService, ServiceErrorKind, Services, TranscriptService};
 use std::sync::Arc;
 
 dozer_core::scope!(LOG, panel, "project");
@@ -180,7 +180,7 @@
             spawn_repair_run(
                 repo_path.to_path_buf(),
                 project_id,
-                services.project.clone(),
+                services.transcript.clone(),
                 handle,
                 emit,
             );
@@ -481,7 +481,7 @@
 /// 2026-08-28,避免每次静默打开项目都真实拉起 agent 进程)。
 pub fn spawn_scaffold_run(
     repo_path: std::path::PathBuf,
-    project: Arc<dyn ProjectService>,
+    transcript: Arc<dyn TranscriptService>,
     handle: &tokio::runtime::Handle,
     emit: impl Fn(Message) + Send + 'static,
     skip_git_init: bool,
@@ -493,7 +493,7 @@
             scaffold::run_sync_steps(&repo_path2, skip_git_init)
         })
         .await;
-        if let Err(e) = project.backfill_project_transcripts(cwd.clone()).await {
+        if let Err(e) = transcript.backfill_project_transcripts(cwd.clone()).await {
             dozer_core::log_warn!(LOG, error = %e, "补录转录失败");
         }
         emit(Message::ScaffoldDone);
@@ -548,7 +548,7 @@
 pub fn spawn_repair_run(
     repo_path: std::path::PathBuf,
     project_id: i64,
-    project: Arc<dyn ProjectService>,
+    transcript: Arc<dyn TranscriptService>,
     handle: &tokio::runtime::Handle,
     emit: impl Fn(Message) + Send + 'static,
 ) {
@@ -571,7 +571,7 @@
         }
 
         emit(Message::TranscriptBackfillStarted(project_id));
-        let backfill_result = match project.backfill_project_transcripts(cwd.clone()).await {
+        let backfill_result = match transcript.backfill_project_transcripts(cwd.clone()).await {
             Ok(0) => scaffold::ScaffoldStepResult::AlreadyOk,
             Ok(n) => scaffold::ScaffoldStepResult::Created(keyed_args(
                 "scaffold.result.imported",
@@ -587,7 +587,7 @@
 
         // 总结步骤改用 V2 批次协议:提交批次 → 轮询批次状态,展示成功/失败/
         // 跳过计数,不再用旧 0/0 进度掩盖失败(spec 2026-09-26 A7)。
-        let (batch_id, _total) = match project.submit_summary_batch(cwd.clone(), None, None).await {
+        let (batch_id, _total) = match transcript.submit_summary_batch(cwd.clone(), None, None).await {
             Ok(pair) => pair,
             Err(e) => {
                 dozer_core::log_warn!(LOG, error = %e, "提交总结批次失败");
@@ -604,7 +604,7 @@
         };
         loop {
             tokio::time::sleep(SUMMARY_BACKFILL_POLL_INTERVAL).await;
-            match project.get_summary_batch(batch_id).await {
+            match transcript.get_summary_batch(batch_id).await {
                 Ok(Some(batch)) => {
                     let completed = batch
                         .total
--- a/crates/dozer-app/src/extensions/project/delete.rs
+++ b/crates/dozer-app/src/extensions/project/delete.rs
@@ -31,6 +31,7 @@
     repo_path: PathBuf,
     scope: DeleteScope,
     project: std::sync::Arc<dyn dozer_services::ProjectService>,
+    transcript: std::sync::Arc<dyn dozer_services::TranscriptService>,
     handle: &tokio::runtime::Handle,
     on_done: impl Fn(i64, bool, Vec<String>) + Send + 'static,
 ) {
@@ -41,7 +42,7 @@
             return;
         }
         if scope != DeleteScope::DozerOnly
-            && let Err(e) = project.delete_project_transcripts(cwd.clone()).await
+            && let Err(e) = transcript.delete_project_transcripts(cwd.clone()).await
         {
             on_done(project_id, true, vec![format!("Delete agent history: {e}")]);
             return;
--- a/crates/dozer-app/src/app/update.rs
+++ b/crates/dozer-app/src/app/update.rs
@@ -4297,13 +4297,13 @@
         // 新开的项目 tab(区别于"已开着、只是前台化"那条 `focus_project_tab`
         // 早退分支):静默跑一次 ensure(README/.dozer/git/agent 历史),不
         // 展示结果(见 project_scaffold 设计"打开即 ensure"一节)。
-        let project = crate::host_adapters::services::services_for(&self.client).project;
+        let transcript = crate::host_adapters::services::services_for(&self.client).transcript;
         let handle = self.handle.clone();
         let proxy = self.proxy.clone();
         let emit = move |m| {
             let _ = proxy.send_event(Message::Project(m));
         };
-        project::spawn_scaffold_run(repo_path, project, &handle, emit, skip_git_init);
+        project::spawn_scaffold_run(repo_path, transcript, &handle, emit, skip_git_init);
     }
 
     pub(crate) fn project_tab_switch(&mut self, id: i64) {
@@ -4418,7 +4418,8 @@
         // 关 tab 必须在发起删除请求之前——删除一旦成功,这个项目在
         // dozerd/磁盘上都可能已经不存在了,`Workspace` 不该继续留着。
         self.project_tab_close(project_id);
-        let project = crate::host_adapters::services::services_for(&self.client).project;
+        let services = crate::host_adapters::services::services_for(&self.client);
+        let (project, transcript) = (services.project, services.transcript);
         let handle = self.handle.clone();
         let proxy = self.proxy.clone();
         let on_done = move |project_id, deregistered, errors: Vec<String>| {
@@ -4429,7 +4430,7 @@
             });
         };
         project::delete::spawn_delete_project(
-            project_id, repo_path, scope, project, &handle, on_done,
+            project_id, repo_path, scope, project, transcript, &handle, on_done,
         );
     }
 
--- a/crates/dozer-app/src/extensions/project.rs
+++ b/crates/dozer-app/src/extensions/project.rs
@@ -1647,7 +1647,7 @@
         spawn_repair_run(
             dir.path().to_path_buf(),
             1,
-            project.into_services().project,
+            project.into_services().transcript,
             rt.handle(),
             emit,
         );
@@ -1726,7 +1726,7 @@
         let (emit, rx) = collect();
         spawn_scaffold_run(
             dir.path().to_path_buf(),
-            fake.clone().into_services().project,
+            fake.clone().into_services().transcript,
             rt.handle(),
             emit,
             true,
@@ -1751,6 +1751,7 @@
             dir.path().to_path_buf(),
             delete::DeleteScope::WithAgentCache,
             fake.clone().into_services().project,
+            fake.clone().into_services().transcript,
             rt.handle(),
             move |id, dereg, errors| {
                 let _ = tx.lock().unwrap().send((id, dereg, errors));
```

- [ ] **Step 2: 编译整个工作区(含测试)**

```bash
cargo check --workspace --tests 2>&1 | grep -E "^error" -A8 | head -40; echo checked
```

Expected: 无 error。(`dozer-app` 会有 4 条已有的 `dead_code` 警告:`text_stable`、`reload_all_webviews_for_locale`、`FakeLocale`,不是本改动引入的,不要为它们改代码。)

- [ ] **Step 3: 跑相关测试并对账**

```bash
cargo test -p dozer-services -p dozer-client 2>&1 | grep -E "test result|FAILED"
cargo test -p dozer-app extensions::project 2>&1 | grep -E "test result|FAILED"
cargo test -p dozer-app panel_host 2>&1 | grep -E "test result|FAILED"
```

Expected: 条数与 Task 1 基线完全相同,全绿(这是一次不增不减测试的重构)。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(services): split ProjectService into a registry and TranscriptService

The registry (open/list/rename/remove) is host-level business any product
can implement; transcript backfill/delete and summary batches are Dozer's
agent concerns. Services gains a transcript field; Client and FakeProject
implement both traits and the call sites take the one they use.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `LocalProjectService` 与 `LocalMemoryService`

**Files:**
- Create: `crates/dozer-store-project/src/service.rs`
- Modify: `crates/dozer-store-project/src/lib.rs`

**Interfaces:**
- Consumes: `ProjectStore` / `MemoryStore`(Task 2)、`ProjectService` / `MemoryService`(Task 3)。
- Produces: `LocalProjectService::new(Arc<ProjectStore>)`、`LocalMemoryService::new(Arc<MemoryStore>)`,分别实现对应 trait,`Clone`。`open_project`:路径先规整(去掉末尾分隔符与 `.` 段),不是目录返回 `Ok(None)`;`rename_project`:去首尾空白,空名字返回 `Rejected` 的 `"名称不能为空"`;其余错误一律 `Rejected`。

- [ ] **Step 1: 先写测试(此时没有实现)**

`crates/dozer-store-project/src/service.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn services() -> (tempfile::TempDir, LocalProjectService, LocalMemoryService) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projects.db");
        let p = LocalProjectService::new(Arc::new(ProjectStore::new(&db).unwrap()));
        let m = LocalMemoryService::new(Arc::new(MemoryStore::new(&db).unwrap()));
        (dir, p, m)
    }

    #[test]
    fn open_registers_a_directory_and_a_second_open_returns_the_same_id() {
        let (dir, p, _) = services();
        let proj = dir.path().join("book");
        std::fs::create_dir(&proj).unwrap();
        let path = proj.to_string_lossy().into_owned();
        let a = block_on(p.open_project(path.clone())).unwrap().unwrap();
        let b = block_on(p.open_project(path)).unwrap().unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, "book");
        assert!(b.last_active_ms > a.last_active_ms, "再次打开只刷新活跃戳");
        assert_eq!(a.created_ms, b.created_ms);
    }

    #[test]
    fn a_trailing_separator_or_dot_segment_is_the_same_project() {
        let (dir, p, _) = services();
        let proj = dir.path().join("book");
        std::fs::create_dir(&proj).unwrap();
        let plain = proj.to_string_lossy().into_owned();
        let a = block_on(p.open_project(plain.clone())).unwrap().unwrap();
        let b = block_on(p.open_project(format!("{plain}/"))).unwrap().unwrap();
        let c = block_on(p.open_project(format!("{plain}/."))).unwrap().unwrap();
        assert_eq!((a.id, a.id), (b.id, c.id));
        assert_eq!(block_on(p.list_projects()).unwrap().len(), 1);
    }

    #[test]
    fn open_of_a_missing_path_or_a_file_is_none_and_registers_nothing() {
        let (dir, p, _) = services();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "x").unwrap();
        for path in [dir.path().join("nope"), file] {
            let got = block_on(p.open_project(path.to_string_lossy().into_owned())).unwrap();
            assert_eq!(got, None);
        }
        assert!(block_on(p.list_projects()).unwrap().is_empty());
    }

    #[test]
    fn a_non_git_directory_updates_at_its_last_active_time() {
        let (dir, p, _) = services();
        let proj = dir.path().join("plain");
        std::fs::create_dir(&proj).unwrap();
        let info = block_on(p.open_project(proj.to_string_lossy().into_owned()))
            .unwrap()
            .unwrap();
        assert_eq!(info.updated_ms, info.last_active_ms);
    }

    #[test]
    fn rename_trims_and_rejects_blank_names() {
        let (dir, p, _) = services();
        let proj = dir.path().join("a");
        std::fs::create_dir(&proj).unwrap();
        let info = block_on(p.open_project(proj.to_string_lossy().into_owned()))
            .unwrap()
            .unwrap();
        let renamed = block_on(p.rename_project(info.id, "  新名字 ".into()))
            .unwrap()
            .unwrap();
        assert_eq!(renamed.name, "新名字");
        let e = block_on(p.rename_project(info.id, "   ".into())).unwrap_err();
        assert_eq!(e.kind, ServiceErrorKind::Rejected);
        let e = block_on(p.rename_project(9999, "x".into())).unwrap_err();
        assert_eq!(e.kind, ServiceErrorKind::Rejected, "不存在的 id 是明确的拒绝,不是静默成功");
    }

    #[test]
    fn remove_deregisters_but_leaves_the_directory_alone() {
        let (dir, p, _) = services();
        let proj = dir.path().join("keep");
        std::fs::create_dir(&proj).unwrap();
        let info = block_on(p.open_project(proj.to_string_lossy().into_owned()))
            .unwrap()
            .unwrap();
        block_on(p.remove_project(info.id)).unwrap();
        assert!(block_on(p.list_projects()).unwrap().is_empty());
        assert!(proj.is_dir());
        assert!(block_on(p.remove_project(info.id)).is_err());
        // 重新登记同一目录拿到的是新 id:旧 id 复用的话,残留在旧 id 下的共享记忆会挂到"新"项目上。
        let again = block_on(p.open_project(proj.to_string_lossy().into_owned()))
            .unwrap()
            .unwrap();
        assert!(again.id > info.id);
    }

    #[test]
    fn list_is_most_recently_opened_first() {
        let (dir, p, _) = services();
        let mut ids = vec![];
        for n in ["a", "b"] {
            let d = dir.path().join(n);
            std::fs::create_dir(&d).unwrap();
            ids.push(
                block_on(p.open_project(d.to_string_lossy().into_owned()))
                    .unwrap()
                    .unwrap()
                    .id,
            );
        }
        let listed: Vec<i64> = block_on(p.list_projects())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(listed, vec![ids[1], ids[0]]);
    }

    #[test]
    fn memory_write_is_an_upsert_by_title_and_keeps_history() {
        let (_dir, _, m) = services();
        let first = block_on(m.write_memory(
            1, "标题".into(), "project".into(), "d".into(), "正文一".into(), "user".into(),
        ))
        .unwrap();
        let second = block_on(m.write_memory(
            1, "标题".into(), "project".into(), "d".into(), "正文二".into(), "user".into(),
        ))
        .unwrap();
        assert_eq!(first.id, second.id);
        let listed = block_on(m.list_memories(1)).unwrap();
        assert_eq!(listed.len(), 1);
        let got = block_on(m.get_memory(1, first.id)).unwrap();
        assert_eq!(got.body, "正文二");
        assert!(got.history.len() >= 2, "created + updated 两条审计");
        block_on(m.delete_memory(1, first.id, "user".into())).unwrap();
        assert!(block_on(m.list_memories(1)).unwrap().is_empty());
    }

    #[test]
    fn memories_of_different_projects_do_not_mix() {
        let (_dir, _, m) = services();
        block_on(m.write_memory(1, "t".into(), "user".into(), "".into(), "a".into(), "u".into())).unwrap();
        block_on(m.write_memory(2, "t".into(), "user".into(), "".into(), "b".into(), "u".into())).unwrap();
        assert_eq!(block_on(m.list_memories(1)).unwrap().len(), 1);
        assert_eq!(block_on(m.list_memories(2)).unwrap().len(), 1);
    }
}
```

`crates/dozer-store-project/src/lib.rs` 改成完整版:

```rust
//! Project 注册表与 Project 级共享记忆的存储与进程内服务实现。
//!
//! 同一份代码被 `dozerd`(经 UDS 对外服务)与 Digger(进程内直接使用)共用;两个产品各自打开自己的
//! SQLite 文件,运行时互不相识。表结构与 `dozerd` 的 `projects` / `memories` / `memory_history` 逐字一致。

mod memory;
mod projects;
mod service;

pub use memory::MemoryStore;
pub use projects::ProjectStore;
pub use service::{LocalMemoryService, LocalProjectService};
```

- [ ] **Step 2: 运行,确认因缺少实现而编译失败**

```bash
cargo test -p dozer-store-project 2>&1 | grep -E "^error" -A4 | head -20
```

Expected: `cannot find type LocalProjectService` / `LocalMemoryService`(或 `unresolved import`)。

- [ ] **Step 3: 在测试模块之上加入实现**

把下面这段放在 `service.rs` 最前面(测试模块 `#[cfg(test)] mod tests` 保持在其后):

```rust
//! 进程内服务实现:把 SQLite 存储包成 `dozer-services` 的 trait,供不经 `dozerd` 的产品(Digger)直接使用。
//!
//! SQLite 调用是同步且很快的;future 在构造时就已算完(`Box::pin(async move { r })`),与 `dozer-app` 的
//! 服务假实现同型,因此在任何执行器上都不会阻塞调度。

use crate::{MemoryStore, ProjectStore};
use dozer_model::{MemoryDetail, MemoryInfo, ProjectInfo};
use dozer_services::{BoxFuture, MemoryService, ProjectService, ServiceError, ServiceErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn rejected(e: anyhow::Error) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Rejected, format!("{e:#}"))
}

/// `ProjectService` 的进程内实现。Project 就是磁盘上的一个目录。
#[derive(Clone)]
pub struct LocalProjectService {
    store: Arc<ProjectStore>,
}

impl LocalProjectService {
    pub fn new(store: Arc<ProjectStore>) -> Self {
        Self { store }
    }
}

impl ProjectService for LocalProjectService {
    fn rename_project(
        &self,
        id: i64,
        name: String,
    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
        let trimmed = name.trim();
        // 与 dozerd 的 RenameProject 同规则:空名字不是合法名字。
        let r = if trimmed.is_empty() {
            Err(ServiceError::new(ServiceErrorKind::Rejected, "名称不能为空"))
        } else {
            self.store.rename(id, trimmed).map(Some).map_err(rejected)
        };
        Box::pin(async move { r })
    }

    fn remove_project(&self, id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        let r = self.store.remove(id).map_err(rejected);
        Box::pin(async move { r })
    }

    fn open_project(
        &self,
        path: String,
    ) -> BoxFuture<'_, Result<Option<ProjectInfo>, ServiceError>> {
        // 契约允许"路径不存在"返回 `Ok(None)`:不是目录就不登记。
        // 末尾分隔符与 `.` 段不应造出第二个项目:`projects.path` 是 UNIQUE 的原样文本,先规整成同一种写法。
        let normalized: PathBuf = Path::new(&path).components().collect();
        let r = if normalized.is_dir() {
            self.store.open(&normalized.to_string_lossy()).map(Some).map_err(rejected)
        } else {
            Ok(None)
        };
        Box::pin(async move { r })
    }

    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<ProjectInfo>, ServiceError>> {
        let r = self.store.list().map_err(rejected);
        Box::pin(async move { r })
    }
}

/// `MemoryService` 的进程内实现。
#[derive(Clone)]
pub struct LocalMemoryService {
    store: Arc<MemoryStore>,
}

impl LocalMemoryService {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }
}

impl MemoryService for LocalMemoryService {
    fn list_memories(
        &self,
        project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<MemoryInfo>, ServiceError>> {
        let r = self.store.list(project_id).map_err(rejected);
        Box::pin(async move { r })
    }

    fn get_memory(
        &self,
        project_id: i64,
        id: i64,
    ) -> BoxFuture<'_, Result<MemoryDetail, ServiceError>> {
        let r = self.store.get(project_id, id).map_err(rejected);
        Box::pin(async move { r })
    }

    fn write_memory(
        &self,
        project_id: i64,
        title: String,
        kind: String,
        description: String,
        body: String,
        actor: String,
    ) -> BoxFuture<'_, Result<MemoryDetail, ServiceError>> {
        let r = self
            .store
            .write(project_id, &title, &kind, &description, &body, &actor)
            .map_err(rejected);
        Box::pin(async move { r })
    }

    fn delete_memory(
        &self,
        project_id: i64,
        id: i64,
        actor: String,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        let r = self.store.delete(project_id, id, &actor).map_err(rejected);
        Box::pin(async move { r })
    }
}
```

- [ ] **Step 4: 运行,确认全部通过**

```bash
cargo test -p dozer-store-project 2>&1 | grep -E "test result|FAILED|panicked"
```

Expected: **32** 条通过(搬来的 23 条 + 本任务新增 9 条)。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(store-project): in-process ProjectService and MemoryService

Products that do not run dozerd (Digger) use the stores directly. open_project
normalises the path and returns None for non-directories; rename trims and
rejects blank names, matching dozerd's RenameProject.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Dozer 门禁、依赖闭包与整体对账

**Files:** 通常无改动(只在门禁报出新 crate 时才登记)。

- [ ] **Step 1: 依赖闭包**

```bash
cd $ROOT/dozer-store-project
cargo tree -p dozer-store-project --prefix none | grep -E "dozer-(app|client|core)|dozerd" || echo "closure clean"
```

Expected: `closure clean`。

- [ ] **Step 2: 三个门禁**

```bash
bash scripts/check-i18n.sh | tail -2
python3 scripts/audit/check_panel_boundary.py | tail -2
bash scripts/check-log-scope.sh | tail -3
```

Expected: 与 Task 1 基线相同——`check-i18n: ok`、面板边界 ok、`check-log-scope.sh` 只有基线里那 1 处。新 crate 没有界面、没有 i18n 文案、没有日志宏,按计划写成时的实测**不需要**在任何门禁里登记;若某个门禁因新 crate 报错,按报错内容参照 `dozer-panel-usage` 在同一脚本里的登记方式补上,并把理由写进提交信息。

- [ ] **Step 3: 全量对账**

```bash
cargo check --workspace --tests 2>&1 | grep -E "^error" | head; echo checked
cargo test -p dozerd 2>&1 | grep -E "test result|FAILED"
cargo test -p dozer-app 2>&1 | grep -E "test result|FAILED|failed"
```

Expected: `dozerd` 单元测试 = 基线 − 23,集成测试不变;`dozer-app` 全绿,但允许出现 Task 1 基线里**就已经红**的测试(例如依赖仓库历史的 `git_log::tests::build_*`,需要 HEAD 包含全部近期提交);任何**新增**的失败都要查清。

- [ ] **Step 4: 记录并收尾**

```bash
git status --short            # 应当干净
git log --oneline main..HEAD  # 应是 Task 2/3/4 的三个提交
```

不推送、不合并。把分支名与提交 hash 记下来,Part B 的开发期 `[patch]` 要指向这个 worktree。

---

# Part B:Digger 一侧

### Task 6: Digger 分支、依赖与开发期 `[patch]`

**Files:** `Cargo.toml`(Digger 仓库根)

**前置条件(开工前先核对,不满足就停下来问用户):** `main` 必须已经包含 S1(`bytehost-chrome` 窗口骨架)和 S2(用量配对),即 `git log` 里有合并提交 `1a3b66d`("Merge branch 'feat/usage-pair'"),并且 `product.rs` 里有 `top_bar_slots` 与 `TopBarSlots`。`feat/usage-pair` 与 `feat/window-shell` 已合并并删除,本计划的 Task 7-11 补丁对应的就是合并后 `main` 上的这些文件。如果在本计划开工前,`docs/superpowers/plans/2026-10-15-digger-i18n-adoption.md` 已被执行(bytehost 换成 `v0.4.0`、byteui 换成 `v0.4.3`、去掉对 `../bytehost-chrome`、`../byteui-shell` 的 `[patch]`),以那之后的 `Cargo.toml` 为准;补丁若因此上下文对不上,按补丁意图手工合并。

- [ ] **Step 1: 记录状态,切分支**

```bash
cd $ROOT/digger
git status --short                     # 记录一次
git switch -c feat/project-host main
git log --oneline -3
```

- [ ] **Step 2: 记录基线**

```bash
cargo test -p digger-desktop -p digger-store 2>&1 | grep "test result"
```

Expected(计划写成时在合并前的 `feat/usage-pair` 上的实测,内容与合并后的 `main` 相同):`digger-desktop` 90 条、`digger-store` 16 条。已知偶发:`apps::tests::closing_during_startup_waits_and_releases_the_root` 在整套并行时有时失败、单独跑不会,遇到就单独重跑那一条。

- [ ] **Step 3: 加依赖和开发期 `[patch]`**

`Cargo.toml` 的 `[workspace.dependencies]` 里,紧跟 `dozer-panel-usage` 那一行加一行(`rev` 抄旁边 `dozer-services` 那一行的值,**三个 dozer 依赖必须同一个 rev**):

```toml
# Project 注册表与 Project 级共享记忆的存储和进程内服务(Dozer 与 Digger 共用同一份代码,各开各的数据库文件)。
dozer-store-project = { git = "https://github.com/byteboyai/dozer", rev = "<与 dozer-services 同一个 rev>" }
```

文件末尾(现有两个 `[patch]` 之后)追加开发期补丁。它指向 Dozer 的新 worktree;`dozer-store-project` 在上面固定的 rev 里还不存在,`[patch]` 提供它,已验证这样可行:

```toml
# ---- 开发期临时配置(S3p 的 Task 13 移除)----
# 新 crate 还没推送:把 dozer 的三个 crate 指向本地 worktree。三个必须一起指,
# 否则依赖图里会有两份 dozer-model,`ProjectInfo` 类型互不兼容。
[patch."https://github.com/byteboyai/dozer"]
dozer-model = { path = "../dozer-store-project/crates/dozer-model" }
dozer-services = { path = "../dozer-store-project/crates/dozer-services" }
dozer-store-project = { path = "../dozer-store-project/crates/dozer-store-project" }
```

- [ ] **Step 4: 编译并检查只有一份 `dozer-model`**

```bash
cargo build --workspace 2>&1 | grep -E "^error" -A6 | head -20; echo built
cargo tree -d --prefix none 2>/dev/null | grep -E "^(dozer-model|dozer-services|byteui|bytehost-core) " || echo "no duplicates"
```

Expected: 编译成功(此时 `dozer-store-project` 还没人用,只是解析通过);`no duplicates`。

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): depend on dozer-store-project, patched to the local worktree for now

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `digger-store::ProjectHost`

**Files:** `crates/store/Cargo.toml`、`crates/store/src/lib.rs`、`crates/store/src/projects.rs`(新建)

**Interfaces:**
- Consumes: `LocalProjectService`、`LocalMemoryService`、`ProjectStore`、`MemoryStore`(Task 4)。
- Produces: `digger_store::ProjectHost { pub projects: Arc<dyn ProjectService>, pub memory: Arc<dyn MemoryService> }`(`Clone`),`ProjectHost::open(db_path: &Path) -> Result<ProjectHost, ServiceError>`;打不开返回 `ServiceErrorKind::Unavailable`。

- [ ] **Step 1: 依赖与导出补丁,以及测试**

先应用下面的补丁(`crates/store` 的依赖和 `lib.rs` 导出;`dozer-store-project.workspace = true` 靠 Task 6 加的 workspace 依赖解析),再新建测试:

`crates/store/src/projects.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
    }

    #[test]
    fn projects_and_memories_survive_reopening_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projects.db");
        let proj = dir.path().join("book");
        std::fs::create_dir(&proj).unwrap();
        let id = {
            let host = ProjectHost::open(&db).unwrap();
            let info = block_on(host.projects.open_project(proj.to_string_lossy().into_owned())).unwrap().unwrap();
            block_on(host.memory.write_memory(info.id, "风格".into(), "feedback".into(), "".into(), "短句".into(), "user".into())).unwrap();
            info.id
        };
        let host = ProjectHost::open(&db).unwrap();
        let listed = block_on(host.projects.list_projects()).unwrap();
        assert_eq!(listed.iter().map(|p| p.id).collect::<Vec<_>>(), vec![id]);
        assert_eq!(block_on(host.memory.list_memories(id)).unwrap().len(), 1);
    }

    #[test]
    fn an_unopenable_database_is_unavailable_not_an_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        // 把"文件"当目录用,建库目录必然失败。
        let e = ProjectHost::open(&blocker.join("projects.db")).err().expect("must fail");
        assert_eq!(e.kind, ServiceErrorKind::Unavailable);
    }
}
```

补丁(存成文件后 `git apply --check` 再 `git apply`):

```diff
--- a/crates/store/Cargo.toml
+++ b/crates/store/Cargo.toml
@@ -10,6 +10,8 @@
 # 领域类型(纯 serde)与服务 trait 接缝(无 bytehost),业务层可安全依赖。
 dozer-model.workspace = true
 dozer-services.workspace = true
+# Project 注册表与共享记忆的存储与进程内服务(Dozer 与 Digger 共用)。
+dozer-store-project.workspace = true
 
 serde.workspace = true
 serde_json.workspace = true
@@ -17,3 +19,4 @@
 
 [dev-dependencies]
 tokio = { workspace = true }
+tempfile = "3"
--- a/crates/store/src/lib.rs
+++ b/crates/store/src/lib.rs
@@ -14,9 +14,11 @@
 //! - **读失败 ≠ 空**:账本损坏是 `Err`,与"确实没有记录"(`Ok(vec![])`)区分开,不静默吞。
 
 mod ledger;
+mod projects;
 mod runner;
 mod service;
 
+pub use projects::ProjectHost;
 pub use ledger::{CompletionEvent, TaskUsageRecord, UsageLedger, UsageLedgerError};
 pub use runner::{Clock, RunningTask, RunningTasks, SystemClock, TaskOutcome, TaskRunner};
 pub use service::{LedgerUsageService, UnavailableUsageService};
```

- [ ] **Step 2: 运行,确认失败**

```bash
cargo test -p digger-store projects 2>&1 | grep -E "^error" -A4 | head -12
```

Expected: `cannot find type ProjectHost`(`use super::*` 里没有它)。

- [ ] **Step 3: 在测试模块之上加入实现**

```rust
//! Project 注册表与 Project 级共享记忆的进程内装配。
//!
//! 实现来自 Dozer 与 Digger 共用的 `dozer-store-project`;这里只决定 Digger 把它放在哪个数据库文件里。
//! 两个存储用同一个 SQLite 文件(各持一个连接,表互不相交),与 `dozerd` 把所有 store 放进 `dozer.db` 同型。
//! 文件属于 Digger 自己的数据根,与 Dozer 的数据库没有任何关系。

use dozer_services::{MemoryService, ProjectService, ServiceError, ServiceErrorKind};
use dozer_store_project::{LocalMemoryService, LocalProjectService, MemoryStore, ProjectStore};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProjectHost {
    pub projects: Arc<dyn ProjectService>,
    pub memory: Arc<dyn MemoryService>,
}

/// `anyhow::Error` 的 `{:#}` 带出整条原因链;用泛型 `Display` 接,Digger 不必直接依赖 anyhow。
fn unavailable<E: std::fmt::Display>(e: E) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, format!("{e:#}"))
}

impl ProjectHost {
    /// 打开(必要时创建)`db_path`。打不开是 `Unavailable`,调用方据此显示"项目功能不可用",不静默退回临时库。
    pub fn open(db_path: &Path) -> Result<Self, ServiceError> {
        let projects = ProjectStore::new(db_path).map_err(unavailable)?;
        let memories = MemoryStore::new(db_path).map_err(unavailable)?;
        Ok(Self {
            projects: Arc::new(LocalProjectService::new(Arc::new(projects))),
            memory: Arc::new(LocalMemoryService::new(Arc::new(memories))),
        })
    }
}
```

- [ ] **Step 4: 运行,确认通过**

```bash
cargo test -p digger-store 2>&1 | grep -E "test result|FAILED"
```

Expected: 比基线多 **2** 条(`projects_and_memories_survive_reopening_the_same_file`、`an_unopenable_database_is_unavailable_not_an_empty_registry`),全绿。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(store): ProjectHost wires the shared project registry and memory into one SQLite file

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 页签状态 `ProjectTabs` 与文件名常量

**Files:** `crates/desktop/src/paths.rs`、`crates/desktop/src/main.rs`、`crates/desktop/Cargo.toml`、`crates/desktop/src/projects/mod.rs`(新建,本任务只有一行)、`crates/desktop/src/projects/tabs.rs`(新建)

**Interfaces:**
- Produces:
  - `paths::PROJECTS_DB_FILE = "projects.db"`、`paths::PROJECT_TABS_FILE = "project_tabs.json"`。
  - `ProjectTabs::{open() -> &[i64], active() -> Option<i64>, open_tab(i64), close_tab(i64), activate(i64) -> bool, prune(&HashSet<i64>), load(&Path) -> Loaded, save(&self, &Path) -> io::Result<()>}`;`Loaded { tabs: ProjectTabs, save_blocked: bool }`。

注意:本任务到 Task 9 之间,`projects::tabs` 里的项还没有调用方,bin 目标会有 `dead_code` 警告,这是预期的,Task 9 之后消失。

- [ ] **Step 1: 补丁(常量、`mod projects;`、dev-dependency)与测试**

存成补丁后 `git apply`:

```diff
--- a/crates/desktop/Cargo.toml
+++ b/crates/desktop/Cargo.toml
@@ -52,3 +52,6 @@
 
 [build-dependencies]
 dozer-panel-usage.workspace = true
+
+[dev-dependencies]
+tempfile = "3"
--- a/crates/desktop/src/main.rs
+++ b/crates/desktop/src/main.rs
@@ -8,6 +8,7 @@
 mod lifecycle_selftest;
 mod markdown;
 mod paths;
+mod projects;
 mod product;
 mod shell;
 
--- a/crates/desktop/src/paths.rs
+++ b/crates/desktop/src/paths.rs
@@ -42,6 +42,11 @@
     config_dir().join("shell.json")
 }
 
+/// 项目注册表与共享记忆所在的 SQLite 文件名(ADR-014)。属于 Digger 自己的数据根,与 Dozer 的数据库无关。
+pub const PROJECTS_DB_FILE: &str = "projects.db";
+/// 已打开的项目页签与当前页签的落盘文件名。
+pub const PROJECT_TABS_FILE: &str = "project_tabs.json";
+
 /// 应用宿主平台数据根:`<config>/digger/bytehost/`(ADR-011)。
 pub fn bytehost_root() -> PathBuf {
     config_dir().join("bytehost")
@@ -86,6 +91,14 @@
         let r = bytehost_root();
         assert_eq!(r.file_name().unwrap(), "bytehost");
         assert!(r.starts_with(config_dir()));
+    }
+
+    #[test]
+    fn project_data_is_under_digger_config_and_never_dozers() {
+        let db = config_dir().join(PROJECTS_DB_FILE);
+        assert_eq!(db.file_name().unwrap(), PROJECTS_DB_FILE);
+        assert!(db.starts_with(config_dir()));
+        assert!(!db.to_string_lossy().contains("dozer"));
     }
 
     #[test]
```

`crates/desktop/src/projects/mod.rs`(本任务只有这一行):

```rust
mod tabs;
```

`crates/desktop/src/projects/tabs.rs`——先只放测试模块:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(open: &[i64], active: Option<i64>) -> ProjectTabs {
        ProjectTabs { open: open.to_vec(), active }
    }

    #[test]
    fn opening_an_open_tab_only_activates_it() {
        let mut t = tabs(&[1, 2], Some(1));
        t.open_tab(2);
        assert_eq!((t.open(), t.active()), (&[1, 2][..], Some(2)));
        t.open_tab(3);
        assert_eq!((t.open(), t.active()), (&[1, 2, 3][..], Some(3)));
    }

    #[test]
    fn closing_the_active_tab_picks_the_left_neighbour_then_the_first_then_none() {
        let mut t = tabs(&[1, 2, 3], Some(2));
        t.close_tab(2);
        assert_eq!((t.open(), t.active()), (&[1, 3][..], Some(1)));
        t.close_tab(1);
        assert_eq!((t.open(), t.active()), (&[3][..], Some(3)));
        t.close_tab(3);
        assert_eq!((t.open(), t.active()), (&[][..], None));
    }

    #[test]
    fn closing_a_background_tab_keeps_the_active_one() {
        let mut t = tabs(&[1, 2, 3], Some(3));
        t.close_tab(1);
        assert_eq!(t.active(), Some(3));
        t.close_tab(99);
        assert_eq!(t.open(), &[2, 3]);
    }

    #[test]
    fn activating_a_tab_that_is_not_open_changes_nothing() {
        let mut t = tabs(&[1], Some(1));
        assert!(!t.activate(5));
        assert_eq!(t.active(), Some(1));
    }

    #[test]
    fn prune_drops_unregistered_projects_and_repairs_the_active_tab() {
        let mut t = tabs(&[1, 2, 3], Some(2));
        t.prune(&HashSet::from([1, 3]));
        assert_eq!((t.open(), t.active()), (&[1, 3][..], Some(1)));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        let t = tabs(&[4, 7], Some(7));
        t.save(&path).unwrap();
        assert_eq!(ProjectTabs::load(&path), Loaded { tabs: t, save_blocked: false });
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn a_missing_or_garbage_file_loads_as_empty_and_may_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        assert_eq!(ProjectTabs::load(&path), Loaded { tabs: ProjectTabs::default(), save_blocked: false });
        for junk in ["not json", "[1,2]", "42", "null", "\"x\""] {
            std::fs::write(&path, junk).unwrap();
            let loaded = ProjectTabs::load(&path);
            assert_eq!(loaded, Loaded { tabs: ProjectTabs::default(), save_blocked: false }, "{junk}");
        }
    }

    #[test]
    fn missing_fields_take_defaults_and_duplicates_and_a_dangling_active_are_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        std::fs::write(&path, r#"{"open":[3,3,5],"active":9}"#).unwrap();
        let loaded = ProjectTabs::load(&path);
        assert_eq!((loaded.tabs.open(), loaded.tabs.active()), (&[3, 5][..], Some(3)));
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(ProjectTabs::load(&path).tabs, ProjectTabs::default());
    }

    #[test]
    fn a_newer_schema_file_is_shown_empty_and_flagged_unsaveable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project_tabs.json");
        std::fs::write(&path, r#"{"schema":99,"open":[1],"active":1}"#).unwrap();
        assert_eq!(ProjectTabs::load(&path), Loaded { tabs: ProjectTabs::default(), save_blocked: true });
    }
}
```

- [ ] **Step 2: 运行,确认失败**

```bash
cargo test -p digger-desktop projects::tabs 2>&1 | grep -E "^error" -A4 | head -12
```

Expected: `cannot find type ProjectTabs`。

- [ ] **Step 3: 在测试模块之上加入实现**

```rust
//! 已打开的项目页签:有序 id 列表 + 当前页签,落盘为 `project_tabs.json`。
//!
//! 只存 id,不存路径与名字(那些以注册表为准)。读盘宽容、写盘原子;磁盘上是更高 schema 版本时用空状态显示且
//! **不回写**(与 `shell.json` 同一条规矩)。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FileShape {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    open: Vec<i64>,
    #[serde(default)]
    active: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectTabs {
    open: Vec<i64>,
    active: Option<i64>,
}

/// 读盘结果:`save_blocked` 为真表示磁盘上是不支持的高版本文件,调用方不得回写。
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub tabs: ProjectTabs,
    pub save_blocked: bool,
}

impl ProjectTabs {
    pub fn open(&self) -> &[i64] {
        &self.open
    }

    pub fn active(&self) -> Option<i64> {
        self.active
    }

    /// 打开(或前台化)一个页签:已开着就不重复,只切为当前。
    pub fn open_tab(&mut self, id: i64) {
        if !self.open.contains(&id) {
            self.open.push(id);
        }
        self.active = Some(id);
    }

    /// 关页签。关的是当前页签时,切到左邻;没有左邻则切到新的第一个;一个都不剩则无当前页签。
    pub fn close_tab(&mut self, id: i64) {
        let Some(at) = self.open.iter().position(|t| *t == id) else {
            return;
        };
        self.open.remove(at);
        if self.active == Some(id) {
            self.active = if at > 0 { self.open.get(at - 1).copied() } else { self.open.first().copied() };
        }
    }

    /// 切换当前页签;`id` 不在已开页签里返回 `false`,状态不变。
    pub fn activate(&mut self, id: i64) -> bool {
        if self.open.contains(&id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    /// 丢掉注册表里已不存在的项目(例如被取消登记),保持"当前页签必在已开页签里"。
    pub fn prune(&mut self, registered: &HashSet<i64>) {
        self.open.retain(|id| registered.contains(id));
        self.fix_active();
    }

    fn fix_active(&mut self) {
        if self.active.is_none_or(|a| !self.open.contains(&a)) {
            self.active = self.open.first().copied();
        }
    }

    fn sanitized(open: Vec<i64>, active: Option<i64>) -> Self {
        let mut seen = HashSet::new();
        let open: Vec<i64> = open.into_iter().filter(|id| seen.insert(*id)).collect();
        let mut tabs = Self { open, active };
        tabs.fix_active();
        tabs
    }

    /// 读盘。缺文件、读不出、不是 JSON 对象(含顶层数组:serde 会把 `[1,2]` 当位置序列塞进结构体)都回落为空状态。
    pub fn load(path: &Path) -> Loaded {
        let empty = Loaded { tabs: Self::default(), save_blocked: false };
        let Ok(text) = std::fs::read_to_string(path) else {
            return empty;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return empty;
        };
        if !value.is_object() {
            return empty;
        }
        let Ok(file) = serde_json::from_value::<FileShape>(value) else {
            return empty;
        };
        if file.schema > SCHEMA_VERSION {
            return Loaded { tabs: Self::default(), save_blocked: true };
        }
        Loaded { tabs: Self::sanitized(file.open, file.active), save_blocked: false }
    }

    /// 原子写盘:先写临时文件再改名,半截 JSON 不会被当成有效状态。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let shape = FileShape { schema: SCHEMA_VERSION, open: self.open.clone(), active: self.active };
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&shape).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
}
```

- [ ] **Step 4: 运行,确认通过**

```bash
cargo test -p digger-desktop projects::tabs 2>&1 | grep -E "test result|FAILED"
cargo test -p digger-desktop paths 2>&1 | grep -E "test result|FAILED"
```

Expected: `projects::tabs` **9** 条通过;`paths` 比基线多 1 条(`project_data_is_under_digger_config_and_never_dozers`)。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): ProjectTabs, the open project tabs and the active one

Persisted as project_tabs.json: tolerant load (a top-level array or garbage
loads as empty, a newer schema is shown empty and never overwritten), atomic
save. Ids only; names and paths come from the registry.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: `ProjectsState`:消息、更新与顶栏页签行

**Files:** `crates/desktop/src/projects/mod.rs`(覆盖 Task 8 的一行)、`crates/desktop/src/projects/tests.rs`(新建)

**Interfaces:**
- Consumes: `ProjectHost`(Task 7)、`ProjectTabs`(Task 8)、`paths::{PROJECTS_DB_FILE, PROJECT_TABS_FILE}`。
- Produces: `projects::Message { PickFolder, FolderPicked(Option<PathBuf>), Opened(Result<Option<ProjectInfo>, String>), Listed(Result<Vec<ProjectInfo>, String>), Activate(i64), Close(i64) }`;`ProjectsState::{open_in(&Path), refresh() -> Task<Message>, update(Message) -> Task<Message>, tabs_view() -> Element<Message>, current_project() -> Option<&ProjectInfo>, current_project_id() -> Option<i64>}`(`notice()` 仅测试用)。

- [ ] **Step 1: 先写测试,并把 `mod.rs` 临时写成只有声明**

`crates/desktop/src/projects/mod.rs`:

```rust
mod tabs;

#[cfg(test)]
mod tests;
```

`crates/desktop/src/projects/tests.rs`:

```rust
use super::*;
use crate::paths;

fn info(id: i64, path: &Path, name: &str) -> ProjectInfo {
    ProjectInfo { id, path: path.to_string_lossy().into_owned(), name: name.into(), last_active_ms: id as u64, created_ms: 1, updated_ms: id as u64 }
}

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn opened(state: &mut ProjectsState, info: ProjectInfo) {
    let _ = state.update(Message::Opened(Ok(Some(info))));
}

#[test]
fn a_fresh_data_dir_has_no_tabs_and_creates_the_registry_file() {
    let d = dir();
    let state = ProjectsState::open_in(d.path());
    assert_eq!(state.current_project_id(), None);
    assert!(state.notice().is_none());
    assert!(d.path().join(paths::PROJECTS_DB_FILE).exists());
}

#[test]
fn opening_a_project_adds_and_activates_its_tab_and_survives_a_restart() {
    let d = dir();
    let proj = d.path().join("book");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "book"));
    assert_eq!(state.current_project_id(), Some(1));

    let mut again = ProjectsState::open_in(d.path());
    assert_eq!(again.current_project_id(), None, "快照还没读回来,页签先不画");
    let _ = again.update(Message::Listed(Ok(vec![info(1, &proj, "book")])));
    assert_eq!(again.current_project_id(), Some(1), "页签与当前项目随重启恢复");
}

#[test]
fn a_registry_read_returning_without_a_tabbed_project_drops_that_tab() {
    let d = dir();
    let proj = d.path().join("gone");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "gone"));
    let _ = state.update(Message::Listed(Ok(vec![])));
    assert_eq!(state.current_project_id(), None);
    let reloaded = ProjectsState::open_in(d.path());
    assert!(reloaded.tabs.open().is_empty(), "取消登记后的页签也从落盘里消失");
}

#[test]
fn a_failed_registry_read_keeps_the_tabs_and_only_warns() {
    let d = dir();
    let proj = d.path().join("keep");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "keep"));
    let _ = state.update(Message::Listed(Err("boom".into())));
    assert_eq!(state.current_project_id(), Some(1));
    assert!(state.notice().unwrap().contains("boom"));
}

#[test]
fn a_project_whose_directory_is_gone_stays_as_a_greyed_tab() {
    let d = dir();
    let proj = d.path().join("vanished");
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "vanished"));
    let _ = state.update(Message::Listed(Ok(vec![info(1, &proj, "vanished")])));
    assert_eq!(state.current_project_id(), Some(1), "不自动删除");
    assert!(state.missing.contains(&1));
}

#[test]
fn closing_a_tab_keeps_the_project_registered_and_picks_the_left_neighbour() {
    let d = dir();
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &a, "a"));
    opened(&mut state, info(2, &b, "b"));
    let _ = state.update(Message::Close(2));
    assert_eq!(state.current_project_id(), Some(1));
    assert_eq!(state.registry.len(), 2, "关页签不取消登记");
}

#[test]
fn opening_a_non_directory_only_shows_a_notice() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::Opened(Ok(None)));
    assert!(state.notice().unwrap().contains("文件夹"));
    assert_eq!(state.current_project_id(), None);
}

#[test]
fn an_unopenable_registry_disables_projects_and_never_falls_back_to_a_temp_database() {
    let d = dir();
    // 把数据库路径占成一个目录:SQLite 打不开。
    std::fs::create_dir(d.path().join(paths::PROJECTS_DB_FILE)).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    assert!(state.notice().unwrap().contains("不可用"));
    let _ = state.update(Message::FolderPicked(Some(d.path().to_path_buf())));
    assert_eq!(state.current_project_id(), None);
}

#[test]
fn a_newer_schema_tabs_file_is_never_overwritten() {
    let d = dir();
    let file = d.path().join(paths::PROJECT_TABS_FILE);
    std::fs::write(&file, r#"{"schema":99,"open":[1],"active":1}"#).unwrap();
    let proj = d.path().join("p");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(5, &proj, "p"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"{"schema":99,"open":[1],"active":1}"#);
}

#[test]
fn cancelling_the_folder_picker_changes_nothing() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::FolderPicked(None));
    assert_eq!(state.current_project_id(), None);
    assert!(state.notice().is_none());
}
```

- [ ] **Step 2: 运行,确认失败**

```bash
cargo test -p digger-desktop projects::tests 2>&1 | grep -E "^error" -A4 | head -12
```

Expected: `cannot find type ProjectsState` / `Message`。

- [ ] **Step 3: 写完整的 `mod.rs`**

覆盖 `crates/desktop/src/projects/mod.rs`:

```rust
//! 项目注册表与项目页签在产品壳里的状态与视图。
//!
//! Project 是 host 层的共享业务(ADR-014):身份、注册表、Project 级共享记忆的存储来自 Dozer 与 Digger 共用的
//! `dozer-store-project`,由 `digger-store` 装配;这里只管 Digger 壳自己的事——哪些项目页签开着、哪个是当前、
//! 顶栏页签行怎么画、"＋"怎么弹文件夹选择器。页签状态**不进**共享骨架(Dozer 迁到共享顶栏时再一起上提)。

mod tabs;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};
use digger_store::ProjectHost;
use dozer_model::ProjectInfo;
use iced::widget::{button, row, text, tooltip};
use iced::{Alignment, Element, Task};

use crate::paths;
use tabs::ProjectTabs;

#[derive(Debug, Clone)]
pub enum Message {
    /// 顶栏"＋":弹原生文件夹选择器。
    PickFolder,
    /// 选择器返回(取消为 `None`)。
    FolderPicked(Option<PathBuf>),
    /// `open_project` 的结果:`Ok(None)` = 所选路径不是目录。
    Opened(Result<Option<ProjectInfo>, String>),
    /// `list_projects` 的结果。
    Listed(Result<Vec<ProjectInfo>, String>),
    Activate(i64),
    Close(i64),
}

pub struct ProjectsState {
    /// `None`:数据库打不开,项目功能不可用(原因在 `notice`),**不**退回临时库。
    host: Option<ProjectHost>,
    tabs: ProjectTabs,
    tabs_path: PathBuf,
    /// 磁盘上是不支持的高版本页签文件:内存里用空状态显示,**禁止回写**。
    tabs_blocked: bool,
    /// 注册表快照(最近优先)。页签只存 id,名字与路径以它为准。
    registry: Vec<ProjectInfo>,
    /// 目录已不存在的项目:页签标灰,不自动删除。
    missing: HashSet<i64>,
    notice: Option<String>,
}

impl ProjectsState {
    /// 在数据目录 `dir` 里打开注册表与页签文件。生产传 `paths::config_dir()`;测试传各自的临时目录。
    pub fn open_in(dir: &Path) -> Self {
        let loaded = ProjectTabs::load(&dir.join(paths::PROJECT_TABS_FILE));
        let (host, notice) = match ProjectHost::open(&dir.join(paths::PROJECTS_DB_FILE)) {
            Ok(host) => (Some(host), None),
            Err(e) => (None, Some(format!("项目功能不可用: {e}"))),
        };
        Self {
            host,
            tabs: loaded.tabs,
            tabs_path: dir.join(paths::PROJECT_TABS_FILE),
            tabs_blocked: loaded.save_blocked,
            registry: Vec::new(),
            missing: HashSet::new(),
            notice,
        }
    }

    /// 当前页签对应的项目;注册表还没读回来、或没有页签时为 `None`。
    pub fn current_project(&self) -> Option<&ProjectInfo> {
        let id = self.tabs.active()?;
        self.registry.iter().find(|p| p.id == id)
    }

    /// 当前项目 id(只读)。依赖 `project_id` 的面板(S3b 的 todo)从这里取。
    #[allow(dead_code)] // 第一个调用方是 S3b 的 todo 面板;在此之前只有测试用它
    pub fn current_project_id(&self) -> Option<i64> {
        self.current_project().map(|p| p.id)
    }

    #[cfg(test)]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 向注册表取最新列表(启动时与每次打开后调用)。
    pub fn refresh(&self) -> Task<Message> {
        let Some(host) = &self.host else {
            return Task::none();
        };
        let service = host.projects.clone();
        Task::perform(async move { service.list_projects().await.map_err(|e| e.to_string()) }, Message::Listed)
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::PickFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new().set_title("选择项目文件夹").pick_folder().await.map(|h| h.path().to_path_buf())
                },
                Message::FolderPicked,
            ),
            Message::FolderPicked(None) => Task::none(),
            Message::FolderPicked(Some(path)) => {
                let Some(host) = &self.host else {
                    return Task::none();
                };
                let service = host.projects.clone();
                let path = path.to_string_lossy().into_owned();
                Task::perform(async move { service.open_project(path).await.map_err(|e| e.to_string()) }, Message::Opened)
            }
            Message::Opened(Ok(Some(info))) => {
                self.notice = None;
                self.tabs.open_tab(info.id);
                // 立刻进快照:页签一出现就有名字,不等后面的 `Listed`。
                self.registry.retain(|p| p.id != info.id);
                self.registry.insert(0, info);
                self.save_tabs();
                self.refresh()
            }
            Message::Opened(Ok(None)) => {
                self.notice = Some("所选路径不是可打开的文件夹".into());
                Task::none()
            }
            Message::Opened(Err(e)) => {
                self.notice = Some(format!("打开项目失败: {e}"));
                Task::none()
            }
            Message::Listed(Ok(list)) => {
                let registered: HashSet<i64> = list.iter().map(|p| p.id).collect();
                self.tabs.prune(&registered);
                self.missing = list.iter().filter(|p| !Path::new(&p.path).is_dir()).map(|p| p.id).collect();
                self.registry = list;
                self.save_tabs();
                Task::none()
            }
            Message::Listed(Err(e)) => {
                // 读失败不等于"没有项目":保留现有页签与快照,只提示。
                self.notice = Some(format!("读取项目列表失败: {e}"));
                Task::none()
            }
            Message::Activate(id) => {
                if self.tabs.activate(id) {
                    self.save_tabs();
                }
                Task::none()
            }
            Message::Close(id) => {
                self.tabs.close_tab(id);
                self.save_tabs();
                Task::none()
            }
        }
    }

    fn save_tabs(&mut self) {
        if self.tabs_blocked {
            return;
        }
        if let Err(e) = self.tabs.save(&self.tabs_path) {
            self.notice = Some(format!("项目页签未保存: {e}"));
        }
    }

    /// 顶栏页签行:已开项目的页签 + "＋"。
    pub fn tabs_view(&self) -> Element<'_, Message> {
        let tokens = color::current();
        let dim = tokens.dim;
        let mut tabs = row![].spacing(6).align_y(Alignment::Center);
        for id in self.tabs.open() {
            // 注册表里没有的 id(尚未读回来)先不画。
            let Some(project) = self.registry.iter().find(|p| p.id == *id) else {
                continue;
            };
            let label_color = if self.missing.contains(id) || self.tabs.active() != Some(*id) { dim } else { tokens.body };
            let name = if self.missing.contains(id) { format!("{}(目录已不存在)", project.name) } else { project.name.clone() };
            tabs = tabs
                .push(button(text(name).size(font::body()).color(label_color)).style(button::text).on_press(Message::Activate(*id)))
                .push(button(text("×").size(font::body()).color(dim)).style(button::text).on_press(Message::Close(*id)));
        }
        if self.tabs.open().is_empty() {
            tabs = tabs.push(text("无项目").size(font::body()).color(dim));
        }
        let plus = button(icons::view(IconKind::Plus, icon_size::rail(), dim)).style(button::text).on_press_maybe(self.host.as_ref().map(|_| Message::PickFolder));
        let hint = if self.host.is_some() { "打开项目文件夹" } else { "项目功能不可用" };
        tabs = tabs.push(tooltip(plus, iced::widget::container(text(hint).size(font::body())).padding(6).style(iced::widget::container::rounded_box), tooltip::Position::Bottom));
        if let Some(notice) = &self.notice {
            tabs = tabs.push(text(notice.clone()).size(font::body()).color(tokens.gold));
        }
        tabs.into()
    }
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 4: 运行,确认通过**

```bash
cargo test -p digger-desktop projects 2>&1 | grep -E "test result|FAILED|panicked"
cargo build -p digger-desktop 2>&1 | grep -E "^warning: (unused|function|method|struct)" -A4 | head
```

Expected: `projects` 下共 **19** 条通过(`tabs` 9 + `tests` 10);`cargo build` 没有 `projects` 相关警告。(`current_project_id` 带 `#[allow(dead_code)]`,第一个调用方是 S3b。)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): ProjectsState drives the project tabs, folder picker and registry reads

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 10: 接进产品壳

**Files:** `crates/desktop/src/product.rs`

**Interfaces:**
- Consumes: `ProjectsState`、`projects::Message`(Task 9)。
- Produces: `Msg::Projects(projects::Message)`;`ProductShell.projects`;顶栏页签行由 `projects.tabs_view()` 绘制;信息面板显示当前 Project 的名字和路径;启动时发一次 `refresh()`;项目数据与布局同在 `shell_path` 的父目录(生产是产品数据根,测试是各自的临时目录)。

- [ ] **Step 1: 应用补丁**

补丁里已包含新测试 `a_project_opened_in_the_shell_is_current_and_comes_back_after_a_restart`。若 i18n 适配等改动让某个上下文行对不上,按补丁意图手工合并,不要改动补丁里的行为。

```diff
--- a/crates/desktop/src/product.rs
+++ b/crates/desktop/src/product.rs
@@ -16,7 +16,7 @@
 use byteui::interaction::icons::{self, IconKind};
 use byteui::theme::{color, font, icon_size};
 
-use iced::widget::{column, container, row, text, tooltip};
+use iced::widget::{column, container, row, text};
 use iced::{Element, Length, Size, Subscription, Task, window};
 
 use crate::{paths, shell};
@@ -39,6 +39,8 @@
     NativeUsage(crate::adapters::usage_native::Event),
     /// 「编辑器」面板的消息(ADR-013)。
     Markdown(crate::markdown::Message),
+    /// 项目页签与注册表(顶栏页签行)。
+    Projects(crate::projects::Message),
     /// 面板生命周期自检的推进(仅 `DIGGER_LIFECYCLE_SELFTEST` 设置时出现)。
     Lifecycle(crate::lifecycle_selftest::Phase),
     Tick,
@@ -65,6 +67,8 @@
     usage: crate::adapters::usage::UsagePanel,
     /// 「编辑器」面板:自包含的第一方面板(文本与协议在 `digger-editor`,这里只有宿主适配)。
     markdown: crate::markdown::MarkdownPanel,
+    /// 项目注册表与已开页签(Project 是 host 层共享业务,ADR-014)。
+    projects: crate::projects::ProjectsState,
     /// 生命周期自检的驱动(只在环境变量设置时有)。
     lifecycle: Option<crate::lifecycle_selftest::Driver>,
     /// [应用, 用量] 面板上一次是否可见:变为可见时触发各自的切入动作;应用面板变为不可见时关掉正在显示的应用。
@@ -142,6 +146,9 @@
         }
         let usage = crate::adapters::usage::UsagePanel::new(usage_service, scope_path.clone());
         let markdown = crate::markdown::MarkdownPanel::new();
+        // 项目数据与布局同在一个数据目录里:生产是产品数据根,测试是各自的临时目录。
+        let data_dir = shell_path.parent().map(std::path::Path::to_path_buf).unwrap_or_else(paths::config_dir);
+        let projects = crate::projects::ProjectsState::open_in(&data_dir);
         let lifecycle = crate::lifecycle_selftest::Driver::from_env();
         // 自检模式需要编辑器一开始就看得见。
         if markdown.is_selftest() || lifecycle.is_some() {
@@ -157,6 +164,7 @@
             apps,
             usage,
             markdown,
+            projects,
             lifecycle,
             visible_before: [false; 2],
             boot_note,
@@ -313,7 +321,11 @@
                 text(format!("面板目录已注册 {} 个(产品组合根)", self.catalog.len())).color(tokens.body),
                 text(format!("数据根:{}", paths::config_dir().display())).color(dim),
                 text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
-                text("当前 Project:无").color(tokens.body),
+                text(match self.projects.current_project() {
+                    Some(p) => format!("当前 Project:{}({})", p.name, p.path),
+                    None => "当前 Project:无".to_string(),
+                })
+                .color(tokens.body),
                 text(&self.boot_note).color(tokens.gold),
                 iced::widget::button("退出 Digger").on_press(Msg::Exit),
             ]
@@ -382,8 +394,8 @@
     }
 }
 
-/// 顶栏三个槽位。S1 没有 Project:标题只是展示,页签行显示"无项目","＋"置灰并提示。
-fn top_bar_slots<'a>() -> TopBarSlots<'a, Msg> {
+/// 顶栏三个槽位:标题只是展示;页签行与"＋"由项目注册表状态(`projects`)画。
+fn top_bar_slots<'a>(projects: &'a crate::projects::ProjectsState) -> TopBarSlots<'a, Msg> {
     let tokens = color::current();
     let dim = tokens.dim;
     let title = row![
@@ -392,12 +404,7 @@
     ]
     .spacing(6)
     .align_y(iced::Alignment::Center);
-    let plus = tooltip(
-        container(icons::view(IconKind::Plus, icon_size::rail(), dim)).padding(6),
-        container(text("Project 尚未接入").size(font::body())).padding(6).style(container::rounded_box),
-        tooltip::Position::Bottom,
-    );
-    let tabs = row![text("无项目").size(font::body()).color(dim), plus].spacing(10).align_y(iced::Alignment::Center);
+    let tabs = projects.tabs_view().map(Msg::Projects);
     TopBarSlots { title: title.into(), tabs: tabs.into(), right: iced::widget::Space::new().into() }
 }
 
@@ -425,7 +432,7 @@
     }
 
     fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
-        top_bar_slots()
+        top_bar_slots(&self.projects)
     }
 
     fn footbar<'a>(&'a self) -> El<'a, Msg> {
@@ -514,6 +521,7 @@
                 Task::none()
             }
             Msg::Markdown(m) => self.markdown.update(m).map(Msg::Markdown),
+            Msg::Projects(m) => self.projects.update(m).map(Msg::Projects),
             Msg::Lifecycle(phase) => {
                 let Some(mut driver) = self.lifecycle.take() else {
                     return Task::none();
@@ -660,7 +668,8 @@
         }
     }
     shell.visible_before = [shell::apps_id(), shell::usage_id()].map(|p| shell.panel_visible(&p));
-    (shell, Task::batch([size_task, reply_task, usage_task, native_usage, markdown_task, lifecycle_task, Task::batch(initial)]))
+    let projects_task = shell.projects.refresh().map(Msg::Projects);
+    (shell, Task::batch([projects_task, size_task, reply_task, usage_task, native_usage, markdown_task, lifecycle_task, Task::batch(initial)]))
 }
 
 
@@ -690,6 +699,22 @@
     fn starts_without_project() {
         let shell = ProductShell::new();
         assert!(shell.boot_note.contains("尚未打开 Project"));
+    }
+
+    #[test]
+    fn a_project_opened_in_the_shell_is_current_and_comes_back_after_a_restart() {
+        use crate::projects::Message as P;
+        let dir = test_temp_dir("project-restore");
+        let proj = dir.join("book");
+        std::fs::create_dir(&proj).unwrap();
+        let info = dozer_model::ProjectInfo { id: 1, path: proj.to_string_lossy().into_owned(), name: "book".into(), last_active_ms: 1, created_ms: 1, updated_ms: 1 };
+        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
+        send(&mut shell, Msg::Projects(P::Opened(Ok(Some(info.clone())))));
+        assert_eq!(shell.projects.current_project_id(), Some(1));
+
+        let mut again = ProductShell::with_shell_path(dir.join("shell.json"));
+        send(&mut again, Msg::Projects(P::Listed(Ok(vec![info]))));
+        assert_eq!(again.projects.current_project_id(), Some(1));
     }
 
     #[test]
```

- [ ] **Step 2: 编译并跑全部测试**

```bash
cargo build -p digger-desktop 2>&1 | grep -E "^(warning|error)" -A6 | grep -v "future version" | head -20; echo built
cargo test -p digger-desktop -p digger-store 2>&1 | grep -E "test result|FAILED|panicked"
```

Expected: 无 error、无新警告(`block v0.1.6` 的 future-incompat 提示是已有的);`digger-desktop` = 基线 + **21**(`tabs` 9 + `projects::tests` 10 + `paths` 1 + `product` 1),`digger-store` = 基线 + 2;全绿(已知偶发的 `closing_during_startup…` 见 Task 6)。

- [ ] **Step 3: 确认没有改坏 S1/S2 的既有断言**

```bash
cargo test -p digger-desktop product 2>&1 | grep -E "test result|FAILED"
```

Expected: `starts_without_project`、默认布局、用量配对等既有 `product` 测试全部仍通过(它们没有被修改)。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(desktop): the shell's top bar shows project tabs; the plus opens a folder

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 11: ADR-014 与文档勘误

**Files:** `docs/adr/ADR-014-project-as-host-business.md`(新建)、`docs/adr/ADR-003-project-sqlite-consistency-island.md`、`docs/superpowers/specs/2026-10-14-window-shell-design.md`、`docs/superpowers/specs/2026-10-15-project-host-design.md`

- [ ] **Step 1: 新建 ADR-014**

```markdown
# ADR-014: Project identity, registry and shared memory are host-level business

- Status: Accepted
- Date: 2026-10-15
- Decision owners: Digger Architecture
- Applies to: Project identity and registry, Project-level shared memory, the Dozer/Digger code-sharing boundary
- Supersedes (partially): ADR-003, its definition of what a Project is and where Project identity is stored

## Context

Dozer and Digger both need Projects, and the shared panels (todo, memory, conversations, group chat) are all keyed by `project_id`. ADR-003 made the Project a Digger-private directory with its own `project.db`. Keeping that would give two incompatible Project concepts and force every shared panel to be adapted twice.

## Decision

1. **A Project is a directory on disk.** Its identity (`ProjectInfo`: id, path, name, activity and creation times, git-aware update time), the registry of known Projects, and Project-level shared memory (`MemoryInfo` / `MemoryDetail` and their history) are host-level business, not product-private state.
2. **One implementation, shared as code.** `dozer-store-project` holds `ProjectStore`, `MemoryStore` and the in-process `LocalProjectService` / `LocalMemoryService`. Dozer's daemon and Digger both use it.
3. **Products stay independent at run time.** Each product opens its own SQLite file in its own data root (Digger: `<config>/projects.db`). There is no shared process, port, directory or database. What is shared is source code, pinned by git revision.
4. **The service seam is split.** `ProjectService` is the registry only (open, list, rename, remove). Transcript backfill/delete and summary batches are `TranscriptService`, which only Dozer implements.
5. **Open-tab state is the product's.** Which Project tabs are open, and which is active, stays in each product's shell until the shared top bar is adopted by both.

## Consequences

- ADR-003 no longer defines Project identity or storage. Its transactional design (one `BEGIN IMMEDIATE` transaction committing domain state, Commit, ProjectRevision and Outbox events) concerns Creator Domain state, which does not exist yet; it is re-evaluated when the first Creator Domain state lands, and is not decided here.
- Shared panels can use a real `project_id` in Digger. The first consumer is the todo panel (S3b).
- Unregistering a Project does not delete its memory rows (same as Dozer); ids are never reused, so orphaned rows cannot attach to a different Project.
- Dozer keeps working unchanged: `dozerd` re-exports the stores under their old paths and serves the same protocol.
```

- [ ] **Step 2: ADR-003 标注**

把 `docs/adr/ADR-003-project-sqlite-consistency-island.md` 里这一行:

```
- Status: Accepted
```

改成:

```
- Status: Accepted; partially superseded by ADR-014 (Project identity, registry and storage). The transactional design for Creator Domain state is unchanged and is re-evaluated when the first Creator Domain state is introduced.
```

- [ ] **Step 3: S1 spec 的 S5 一行**

`docs/superpowers/specs/2026-10-14-window-shell-design.md`:先在 `| S3 | 共享面板:todo |` 这一行**之前**插入一行,再替换 S5 那一行。

插入:

```
| S3p | Project 实体、注册表、共享记忆 | 前置于 S3b;Dozer 的 Project 提成共享 `dozer-store-project`,Digger 用它打开项目文件夹并在顶栏显示页签(见 `2026-10-15-project-host-design.md`,ADR-014) |
```

把:

```
| S5 | Digger 自有:project + files | 从 Dozer 复制起步,之后自行演化;Digger 目前没有 Project(ADR-003) |
```

替换为:

```
| S5 | Project 面板 + files | Project 身份、注册表、共享记忆已由 S3p 提供(ADR-014);S5 只做面板界面,从 Dozer 抽库共享 |
```

- [ ] **Step 4: S3p spec 勘误**

`docs/superpowers/specs/2026-10-15-project-host-design.md`:

1. §4.1 的"提供**进程内服务实现**"那一条末尾追加一句:`open_project` 先规整路径(去掉末尾分隔符与 `.` 段),避免同一目录因写法不同登记成两个项目;`rename_project` 去首尾空白、拒绝空名字(与 `dozerd` 的 `RenameProject` 同规则)。
2. §5 第 1 条里这句:
   `两个库用各自路径调用 \`ProjectStore::new(path)\`、\`MemoryStore::new(path)\`,与 \`dozerd\` 的用法一致,表结构逐字相同。`
   改为:`两个存储用**同一个** SQLite 文件(各持一个连接,表互不相交),与 \`dozerd\` 把所有 store 放进 \`dozer.db\` 同型,表结构逐字相同。`
   同一条开头的 `paths.rs 新增 projects_db_path()、memory_db_path()` 改为 `paths.rs 新增文件名常量 PROJECTS_DB_FILE(projects.db)与 PROJECT_TABS_FILE(project_tabs.json)`。
3. 顶部"状态"一行改为:`已实现(待用户目测验收:文件夹选择器与页签行);实施计划 \`docs/superpowers/plans/2026-10-15-project-host.md\``。

- [ ] **Step 5: Commit**

```bash
git add docs
git commit -m "docs(adr): ADR-014 Project as host-level business; ADR-003 partially superseded

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 12: 最终验证与交接

**Files:** 无。

- [ ] **Step 1: Digger 全量**

```bash
cd $ROOT/digger
cargo build --workspace 2>&1 | grep -E "^(warning|error)" | grep -v "future version" | head
cargo test --workspace 2>&1 | grep -E "test result|FAILED|failed"
cargo tree -d --prefix none 2>/dev/null | grep -E "^(dozer-model|dozer-services|byteui|bytehost-core) " || echo "no duplicates"
git status --short
git log --oneline main..HEAD
```

Expected: 无 error、无新警告;全绿(已知偶发见 Task 6);`no duplicates`;工作树干净;提交列表是 Task 6/7/8/9/10/11 的六个提交。

- [ ] **Step 2: 门禁脚本与真实窗口自检**

```bash
bash scripts/check-host-deps.sh              # 业务层(digger-store)不得依赖 bytehost-*
bash scripts/check-shell-regions.sh          # 需要 ../dozer 存在;缺了就如实记为未运行
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | tail -15
```

Expected: `digger host-deps check: ok`;自检结论与 S2 一致(各阶段通过,退出码 0)。若本机无法起窗口,或脚本缺少前置条件,如实记为"未运行",不要伪造通过。

- [ ] **Step 3: 更新记忆文件**

把 `/Users/chrischiang/.claude/projects/-Users-chrischiang-Projects-CoralProjects-byteboy-digger/memory/project-host-decision.md` 的正文补一段:S3p 已实现于 Dozer 分支 `feat/store-project`(worktree `../dozer-store-project`,未推送)与 Digger 分支 `feat/project-host`(未合入),列出两条分支的 tip hash;S3b 在 Dozer 提交被推送后才能用 git rev 依赖;Digger 的开发期 `[patch]` 还在。

- [ ] **Step 4: 向用户交接(原样列出,不要自己动手做)**

1. 文件夹选择器与页签行的目测:点顶栏"＋"选一个文件夹 → 出现页签 → 重启后页签还在 → 删掉该目录后重启,页签标灰且不消失 → 关页签。
2. Dozer 里 Project 面板、共享记忆相关功能的目测(本改动不应有任何差别)。
3. **用户门槛(Task 13,不由代理执行)**:
   a. 决定 Dozer `feat/store-project` 如何合入 `main` 并推送(与 S3a 的 `feat/panel-todo-lib` 都改了 `dozer-services`,先合哪个由用户定,后合的需要变基)。
   b. 推送后,在 Digger 把 `dozer-model`、`dozer-services`、`dozer-panel-usage`、`dozer-store-project`(以及已有的 `dozer-i18n`)**一起**改到新的已推送 rev,删掉 `Cargo.toml` 末尾的 `[patch."https://github.com/byteboyai/dozer"]` 整段,再跑 `cargo build --workspace && cargo test --workspace`,并确认 `cargo tree -d` 里没有重复的 `dozer-model`。
   c. 之后才能开始 S3b。

---

## Self-Review(写计划后自查)

**Spec 覆盖**:§4.1 新 crate 与 Local 服务 → Task 2、4;§4.2 拆 trait → Task 3;§5.1 存储位置 → Task 7(+Task 11 勘误为单文件);§5.2 打开项目与页签 → Task 8-10;§5.3 与已有面板的关系 → Task 9 暴露 `current_project_id`,未改动其它面板;§5.4 共享记忆 → Task 4 的测试;§5.5 目录语义 → Task 4(非目录返回 None、规整路径)与非 git 回落由搬来的 `compute_updated_ms` 测试覆盖;§5 ADR → Task 11;§6 验证 → Task 1/5/12 对账、Task 6 与 12 的重复依赖检查;§7 风险 1(推送门槛)→ Task 12 用户门槛;风险 5(首次引入 SQLite)→ Task 6 先编译。

**类型一致**:`ProjectHost.projects / .memory`、`ProjectsState::open_in / refresh / update / tabs_view / current_project(_id)`、`ProjectTabs::{open_tab, close_tab, activate, prune, load, save}`、`Loaded { tabs, save_blocked }` 在各任务里一致;`TranscriptService` 的四个方法与 `Services.transcript` 字段在 Task 3 的补丁和 Task 4 的测试里一致。

**测试数量**:Dozer:`dozer-store-project` 32(=搬来 23 + 新增 9)、`dozerd` 单元 −23、其余不变;Digger:`digger-store` +2、`digger-desktop` +21。
