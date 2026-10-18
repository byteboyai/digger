# S5b-2 P1 预览引擎纯逻辑核心 `dozer-preview-core` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Dozer 里新建共享库 `dozer-preview-core`,把预览引擎的纯逻辑层(路由、文件画像、策略、加载、大文本、保存、恢复、启动、后端选择、资源预算)从 `dozer-app/src/preview/` 搬进去;Dozer 改为依赖它,**用户可见行为不变**。本期**只动 Dozer**,Digger 不消费。

**Architecture:** 新 crate 依赖只含 `serde*`、`dozer-i18n`、`dozer-model`、标准库,**不含** iced、wry、`dozer-app`、`dozer-client`、`dozer-core`。`dozer-app/src/preview/mod.rs` 继续 `pub(crate) use` 这些类型,没搬走的 `state.rs` / `view.rs` / `webview*.rs` 的 `crate::preview::*` 路径不变。三处耦合改造:`crate::capabilities::current()` 全局改为显式传 `&ClientCapabilities`(全局单例留在 Dozer);`crate::app::PanelKind` 换成核心里的 `ViewerPanel`(Dozer 在边界做一次映射);`crate::tabular::is_tabular_extension` 把纯函数搬进核心。

**Tech Stack:** Rust、`dozer-i18n`、`serde`。

**Spec:** `docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md`(§5.1 P1、§6)。模板:S5a / S5b-1 计划的 Dozer 段(同一套 worktree、门禁登记方式)。

## Global Constraints

- **依赖闭包:** `cargo tree -p dozer-preview-core` 里不得出现 `iced*`、`wry`、`dozer-app`、`dozer-client`、`dozer-core`。
- **Dozer 用户可见行为不变**;被搬走的测试随代码一起走,**不删、不改期望值**;抽库前后 Rust 测试总数守恒。
- **路由表不删减**(设计 D3):`classify_preview` 对所有类型的输出保持原样。
- 只用 git tag 依赖;Dozer 仓库内部 `path = "../..."` 依赖除外(与 `dozer-panel-todo` 相同)。**本期不打 tag、不推送**:Digger 不消费核心,tag 留到 P2 一并发;合并到 Dozer `main` 前向用户确认。
- **主工作区别人未提交的改动不动**:Dozer 主工作区有未提交的 `Cargo.lock`、`dozer-app/Cargo.toml`、`Info.plist` 改动。实现在独立 worktree `../dozer-preview-core`、分支 `feat/preview-core` 里做,从 `main` 切出。
- 不引入新的日志依赖。
- **与设计文档的一处偏离(我的决定,标 D6):** 设计 §5.1 把 `trust` 列进 P1,实际 `trust.rs` 用的是 `dozer-app` 里各面板 webview 的 id 偏移常量(`TODO_CONTENT_ID_OFFSET` 等)与 `PanelKind`,属于 Dozer 的 webview 身份布局。它**留在 `dozer-app`**,到 P3 设计 webview 端口时再处理。`plantuml.rs` 也留在 `dozer-app`(不在第一批类型内)。

## Review Focus

1. **路由结果一字不差:** 对一批有代表性的文件名(各类扩展名、无扩展名、大小写混用、`Dockerfile` / `.env` / `LICENSE*` 等文件名规则、`.svg`、空文件、未知二进制、`.csv` / `.xlsx`)搬迁前后的 `classify_preview` 输出完全一致。→ Task 1 黄金测试。
2. **策略阈值边界:** 文件大小恰好等于、比预算小 1 字节、大 1 字节时的决策不变。→ Task 3。
3. **保存失败不损坏原文件:** 目标只读、目录不存在、磁盘写失败时,原文件字节不变,返回可读错误而不是 panic。→ Task 3。
4. **非 UTF-8 / 含 BOM / 含 NUL 的文件、空文件、指向目录的符号链接:** 画像与路由不 panic,给出稳定且可解释的结果。→ Task 2。
5. **资源管理器的预留与淘汰:** 超预算时拒绝或淘汰的顺序、对同一 `ViewerKey` 重复预留、释放不存在的键,行为与迁移前一致,不会计数漂移。→ Task 4。

---

## Part A — Dozer(仓库 `../dozer`,worktree `../dozer-preview-core`,分支 `feat/preview-core`)

### Task 1: 基线、黄金测试、空壳 crate、能力类型

**Files:**
- Create: `crates/dozer-preview-core/Cargo.toml`、`src/lib.rs`、`src/capabilities.rs`、`crates/dozer-app/tests/fixtures/preview_router_golden.txt`(黄金文件,见 Step 2)
- Modify: workspace `Cargo.toml`、`crates/dozer-app/Cargo.toml`、`crates/dozer-app/src/capabilities.rs`
- Test: `crates/dozer-app/src/preview/router.rs`(黄金测试暂放这里)

**Interfaces:**
- Produces(原样搬自 `dozer-app/src/capabilities.rs`,只改归属):`capabilities::{ResourceBudgets, CapabilityTier, HardwareCapabilities, ClientCapabilities, estimate_capabilities(raw: HardwareCapabilities) -> ClientCapabilities, full_file_load_bytes_for(total_ram_bytes: u64) -> u64}`。
- **留在 `dozer-app`:** `detect_hardware()`(依赖 `sysinfo`)与 `install` / `current()` 全局单例;它们 `pub use dozer_preview_core::capabilities::*` 后保持原路径。

- [ ] **Step 1: 记录基线。** 在 worktree 里 `cargo test -p dozer-app 2>&1 | grep "^test result"`,记下总通过数;再单独记 `cargo test -p dozer-app preview:: capabilities::` 的数量(预计迁移的测试共约 148 条:`file_policy` 9、`file_profile` 20、`router` 29、`loading` 8、`text_save` 6、`large_text` 19、`startup` 4、`recovery` 11、`native_editor` 2、`backend` 16、`resources` 15、`capabilities` 9,以实测为准)。
- [ ] **Step 2: 写黄金测试(在搬任何代码之前,在 Dozer 现有代码上)。** 在 `router.rs` 的测试模块里加 `classify_preview_golden_corpus`:对一个固定的路径清单(至少 60 条,覆盖 Review Focus 1 列的所有类别,以及文本/二进制内容探测需要的临时文件)逐条调 `classify_preview`,把 `(path, kind, reason 的稳定字符串)` 序列化成一份期望表,写入 `crates/dozer-app/tests/fixtures/preview_router_golden.txt`(用测试里的 `UPDATE_GOLDEN=1` 环境变量生成一次,提交该文件)。之后每次运行都与它比对。**黄金文件在搬迁过程中不得重新生成。**
- [ ] **Step 3: 建 crate。** `Cargo.toml` 依赖:`serde`、`serde_json`、`dozer-i18n`、`dozer-model`;dev-dependencies:`tempfile`。`cargo build -p dozer-preview-core` 通过,`cargo tree -p dozer-preview-core | grep -E "iced|wry|dozer-(app|client|core)"` 无输出。
- [ ] **Step 4: 搬 `capabilities` 的类型与 `estimate_capabilities` / `full_file_load_bytes_for` 及其测试;`dozer-app/src/capabilities.rs` 改为 `pub use` 转调 + 保留 `detect_hardware`、`install`、`current`。**
- [ ] **Step 5: 验证。** `cargo test -p dozer-preview-core -p dozer-app` 总数 = 基线 + 1(黄金测试);黄金测试通过。
- [ ] **Step 6: 提交** `refactor(preview): 新建 dozer-preview-core,搬能力类型;加路由黄金测试`。

### Task 2: 无耦合的纯模块

**Files:**
- Create: `crates/dozer-preview-core/src/{file_profile,large_text,startup,recovery,native_editor,tabular_ext}.rs`
- Modify: `crates/dozer-app/src/preview/mod.rs`(`mod` 声明与 `pub(crate) use` 改指向核心)、`crates/dozer-app/src/tabular/mod.rs`(`is_tabular_extension` 改为转调)、删除 `dozer-app/src/preview/` 下对应的 `.rs`
- Test: 随代码搬

**Interfaces:**
- Produces:`file_profile::{ContentKind, FileProfile, TextEncoding, ...}`(原样)、`large_text::*`、`startup::*`、`recovery::*`、`native_editor::{filename_code_rule, is_editable_extension, prefers_rendered_preview, wry_toggle_eligible}`(原样)、`tabular_ext::is_tabular_extension(path: &Path) -> bool`(原 `crate::tabular::is_tabular_extension`,扩展名集合 `xlsx xls ods csv tsv` 不变,大小写不敏感)。

- [ ] **Step 1: 写失败测试**(核心里新增,其余随代码搬来):`profile_never_panics_on_degenerate_inputs`(对空文件、仅 BOM、含 NUL 的字节、非 UTF-8 字节、指向目录的符号链接,调用 `FileProfile` 的探测入口都返回 `Ok`/稳定的类别,不 panic)、`tabular_ext_is_case_insensitive_and_exact`(`A.CSV` 为真,`a.csvx` 与 `a` 为假)。
- [ ] **Step 2: 运行确认失败。** `cargo test -p dozer-preview-core`。
- [ ] **Step 3: 搬 5 个模块与 `is_tabular_extension`(连同它们的单元测试);只改 `use` 路径。** `dozer-app/src/preview/mod.rs` 保持原来的 `pub(crate) use` 清单(现在指向核心),`dozer-app/src/tabular` 里的 `is_tabular_extension` 改为 `pub use dozer_preview_core::tabular_ext::is_tabular_extension`。
- [ ] **Step 4: 验证。** `cargo test -p dozer-preview-core -p dozer-app` 总数守恒(两库之和 = 基线 + 新增);黄金测试仍通过(此时 `router.rs` 还没搬,但它依赖的 `file_profile` 已换了来源)。
- [ ] **Step 5: 提交。**

### Task 3: 本地化,以及用到它的路由、策略、加载、保存

**Files:**
- Create: `crates/dozer-preview-core/src/{i18n,router,file_policy,loading,text_save}.rs`、`locales/{en,zh-CN}.json`(从 `crates/dozer-app/locales/panels/preview/` 整体搬来)
- Modify: `crates/dozer-app/src/i18n.rs`(owner `dozer.preview` 的 `include_str!` 改指向核心的 `locales/`,仿 `dozer.todo` 的写法)、`crates/dozer-app/src/preview/mod.rs`、`scripts/i18n/{check_migration,build_inventory,scan_sources}.py`(各加 `dozer-preview-core` 一条,按 `dozer-panel-todo` 的条目)
- Test: 随代码搬;黄金测试

**Interfaces:**
- Produces:
  - `dozer_preview_core::PREVIEW_OWNER: &str = "dozer.preview"`;`i18n::{t, t_args, keyed_args, render_localized}`,签名与 `crate::i18n::{panel_text, panel_text_args, panel_keyed_args, render_localized}` 的现有用法对应,解析走 `dozer-i18n`(同 `dozer-panel-todo::i18n`)。
  - `router::{PreviewKind, PreviewRoute, RouteReason, RouteReasonArg, classify_preview, ...}`(原样);`classify_preview` 现有形参 `_capabilities: &ClientCapabilities` 保持。
  - `file_policy::*`:`ResourceBudgets` 来自核心的 `capabilities`;**预算始终由调用方显式传入,核心里不存在任何全局**。
- Consumes: Task 2 的 `file_profile`、`native_editor`、`tabular_ext`。

- [ ] **Step 1: 写失败测试**(核心里):`file_policy_decision_flips_exactly_at_the_budget_boundary`(取 `full_file_load_bytes` 预算 `B`,断言大小为 `B-1`、`B`、`B+1` 的决策各自等于迁移前的结果;期望值先在 Dozer 现有代码上跑出来再写死)、`save_to_a_read_only_target_leaves_the_original_bytes_untouched`(临时文件设为只读,调用 `text_save` 的保存入口得到 `Err`,原字节不变)、`save_into_a_missing_directory_returns_an_error`、`reason_text_resolves_in_en_and_zh_cn`(对每个 `RouteReason` 变体,`en` 与 `zh-CN` 都解析出非空且不含 `{` 占位残留的文本)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬 `i18n` 与 `locales/`,改 `dozer-app/src/i18n.rs` 的 owner 注册与三个脚本。** `dozer-app` 里没搬走的 `view.rs` 等仍用 `crate::i18n::panel_text(PREVIEW_OWNER, ...)`,它们读到的是同一份静态资源,不受影响。
- [ ] **Step 4: 搬 `router`、`file_policy`、`loading`、`text_save`(连同单元测试);把 `crate::i18n::*` 改为本 crate 的 `i18n::*`,`crate::capabilities::*` 改为 `crate::capabilities::*`(核心自己的),`crate::tabular::is_tabular_extension` 改为 `crate::tabular_ext::...`。**
- [ ] **Step 5: 验证。** `bash scripts/check-i18n.sh` 通过;`cargo test -p dozer-preview-core -p dozer-app` 总数守恒(加新增);**黄金测试通过且黄金文件字节未变**(`git diff --stat crates/dozer-app/tests/fixtures/preview_router_golden.txt` 无输出)。
- [ ] **Step 6: 提交。**

### Task 4: 后端选择与资源预算

**Files:**
- Create: `crates/dozer-preview-core/src/{backend,resources}.rs`
- Modify: `crates/dozer-app/src/preview/mod.rs`、`crates/dozer-app/src/preview/{state,view,webview,code_host}.rs`(`ViewerKey` / `PanelKind` 的边界映射)、`crates/dozer-app/src/preview/resources_global.rs`(新建:Dozer 一侧的全局 `ResourceManager` 单例)
- Test: 随代码搬

**Interfaces:**
- Produces:
  - `resources::ViewerPanel { Files, Project, GitLog }`(`Debug + Clone + Copy + PartialEq + Eq + Hash + PartialOrd + Ord`,与原 `PanelKind` 在这三个取值上的派生一致)。
  - `resources::ViewerKey = (i64, ViewerPanel, usize)`;`ViewerRegistration::new(project_id: i64, panel: ViewerPanel, tab_id: usize) -> Self`;`ResourceManager::new(budgets: ResourceBudgets) -> Self`;其余方法原样,凡原来读 `capabilities::current()` 的位置改为使用构造时传入的 `budgets`。
  - `backend::*`(原样;`ClientCapabilities` 来自核心)。
  - `dozer-app` 一侧:`impl From<PanelKind> for Option<ViewerPanel>` 的等价函数 `fn viewer_panel(kind: PanelKind) -> Option<ViewerPanel>`(`Files`→`Files`、`Project`→`Project`、`GitLog`→`GitLog`,其余→`None`,调用点对 `None` 的处理与原 `panel_token` 里 `_ => "files"` 的兜底保持一致);全局单例 `preview::resources_global::manager() -> &'static Mutex<ResourceManager>`(原 `resources.rs:247` 一带,用 `capabilities::current().budgets` 构造)。

- [ ] **Step 1: 写失败测试**(核心里):`reserving_the_same_viewer_twice_does_not_double_count`(同一 `ViewerKey` 连续 `reserve` 两次,已用预算只增一次)、`releasing_an_unknown_key_is_a_no_op_and_keeps_totals`(`release` 一个从未登记的键,总量不变、不 panic)、`over_budget_reservation_evicts_or_denies_in_the_legacy_order`(构造三个页签占满预算再预留第四个,被淘汰/拒绝的对象与顺序等于迁移前的结果;期望先在 Dozer 现有代码上跑出再写死)、`viewer_panel_ordering_matches_the_legacy_panel_kind_order`(`Files < Project < GitLog` 的排序与迁移前一致,保证依赖排序的淘汰顺序不变)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 搬 `backend`、`resources`;去掉 `crate::app::PanelKind` 与 `capabilities::current()`;在 `dozer-app` 里加 `viewer_panel` 映射与全局单例;`state.rs` / `view.rs` / `webview.rs` / `code_host.rs` 里所有用 `ViewerKey` / `ViewerRegistration` 的调用点改为经 `viewer_panel` 转换。**
- [ ] **Step 4: 验证。** `cargo test -p dozer-preview-core -p dozer-app` 总数守恒(加新增);黄金测试通过且黄金文件未变。
- [ ] **Step 5: 提交。**

### Task 5: 清理、门禁与验收

- [ ] **Step 1: 清理。** `grep -rn "pub fn classify_preview\|pub struct ResourceManager\|pub fn estimate_capabilities\|pub fn is_tabular_extension" crates` 只在 `dozer-preview-core` 与转调处命中;`dozer-app/src/preview/` 里不再有已搬走模块的 `.rs`,只留 `state.rs`、`view.rs`、`webview.rs`、`webview_protocol.rs`、`code_host.rs`、`trust.rs`、`plantuml.rs`、`mod.rs`、`resources_global.rs`。
- [ ] **Step 2: 依赖闭包。** `cargo tree -p dozer-preview-core | grep -E "iced|wry|dozer-(app|client|core)"` **无输出**;在 `scripts/audit/` 的面板边界检查或 `scripts/check-shared-deps.sh` 的登记处加入 `dozer-preview-core`(按 `dozer-panel-todo` 的做法)。
- [ ] **Step 3: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-i18n.sh`;总数与基线一致(加新增);黄金文件字节未变。
- [ ] **Step 4: 请用户在 Dozer 上目测验收**:打开代码文件、Markdown、HTML、图片、JSON、CSV、PlantUML、大文件、二进制文件各一个,行为与外观与抽库前一致;编辑并保存一个文本文件;故意制造一个只读文件的保存失败,确认提示正常。**未验收不继续。**
- [ ] **Step 5: 合并到 Dozer `main`(向用户确认后执行),不打 tag、不推送。** 报告里写明:搬走了哪些模块、`trust` 与 `plantuml` 为什么留下(D6)、测试总数、黄金测试结论、`resources` 全局单例留在 Dozer 的原因。

---

## Self-Review

- **Spec 覆盖:** §5.1 `dozer-preview-core` 的依赖闭包约束 → Task 1 Step 3、Task 5 Step 2;§6 P1"搬第 1 层,Dozer 改引用,测试守恒" → Task 1–5;设计 D1(两个 crate)→ 本期只建第一个;D3(路由表不删减)→ 黄金测试;§7 测试守恒与依赖闭包 → Task 5;§8 Q3(固定默认值而非探测)→ 能力类型进核心、`detect_hardware` 与单例留在 Dozer,Digger 将来可直接构造 `ClientCapabilities`。
- **类型一致:** `ClientCapabilities` / `ResourceBudgets` 在 Task 1 定义,Task 3、4 引用同名;`ViewerPanel` / `ViewerKey` / `ViewerRegistration::new` 在 Task 4 定义,`dozer-app` 一侧经 `viewer_panel` 映射;`is_tabular_extension` 在 Task 2 定义,Task 3 的 `router` 引用同一路径。
- **风险点:** Task 4 里 `PanelKind` 到 `ViewerPanel` 的映射在 `state.rs` / `view.rs` / `webview.rs` / `code_host.rs` 的调用点数量我没有逐处数过,若映射对 `None`(非三个预览面板)的处理不止 `panel_token` 的 `_ => "files"` 一种,先停下报告;`resources` 的淘汰顺序依赖 `PanelKind` 的派生排序,Task 4 Step 1 有专门测试;Task 1 Step 2 的黄金文件是本期最重要的守恒依据,搬迁中途不得重新生成;`i18n` 的 owner `dozer.preview` 同时被核心和没搬走的 `view.rs` 读取,二者读的是同一份静态资源,但若 `scripts/i18n` 的检查对"同一 owner 两处声明"报错,先按脚本现有约定调整登记方式,不要改资源内容。
