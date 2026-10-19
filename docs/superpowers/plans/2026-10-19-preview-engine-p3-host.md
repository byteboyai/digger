# S5b-2 P3 预览 webview 宿主(第一批):协议应答、web 资源、IPC 分类进共享库,Digger 写自己的宿主 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把预览引擎里"不碰 wry 句柄"的宿主部分搬进 Dozer 的 `dozer-panel-preview`(`dozer://` 协议应答与路径白名单、web 资源、宿主注入脚本、来自页面的 IPC 消息分类与校验、页面事件作用到 `PreviewPane` 的逻辑),Dozer 改为薄调用、**用户可见行为不变**;Digger 新增自己的预览 wry 宿主 `adapters/preview_native.rs` 并用一个 dev 示例在真机上看到代码与 Markdown/HTML/图片。**Digger 的状态持有与配对布局仍是 P4。不发布产品版本。**

**Architecture:** 库不含 `wry`(spec §5.1)。库新增四件:`assets`(`AssetRoots` + `AllowedFiles` + `serve_preview`,纯函数)、`host_script`(注入脚本构造)、`host_ipc`(`classify_ipc`:原始 IPC 字符串 → 已按绑定校验的 `HostIpc`)、以及页面事件在 `update` 里的作用(`Message::Host(HostEvent)` → `Effect`)。Dozer 的 wry 池、`webview_geometry`、信任判定、资源预算、浏览器/应用面板的专属分支留在 `dozer-app`,只把闭包里的"解析 + 校验"换成 `classify_ipc`。Digger 的宿主与 `markdown/host.rs` **各自一张句柄表、互不共用**(spec Q1),`plan()` 把"期望清单 vs 已存在句柄"算成 `Create/Update/Destroy` 纯数据后再执行,可无头测试。

**Tech Stack:** Rust、iced 0.14、wry(仅产品侧)、`dozer-panel-preview`/`dozer-preview-core`(P1/P2)、tokio、serde。

**Spec:** `docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md`(§2 第 3、4 层、§5.2 端口、§5.5 安全、§6 P3、§7、§8 Q1–Q3)。前置:P1、P2 已在 Dozer `main`(`libs-2026-10-19a`)。模板:P2 计划 `2026-10-18-preview-engine-p2-panel.md` 的"清单先行、用户过目后再动手"做法;Digger 侧参考 `adapters/usage_native.rs`(句柄表 + `window::run`)与 `markdown/host.rs`。

## Global Constraints

- **依赖闭包:** `cargo tree -p dozer-panel-preview`(默认特性)不得出现 `dozer-app`、`dozer-client`、`dozer-core`、`dozerd`、`wry`、`winit`;`wry` 只在 `dozer-app` 与 `digger-desktop`。
- **Dozer 用户可见行为不变:** 搬走的测试随代码走、不删、不改期望值,Rust 测试总数守恒;`crates/dozer-app/tests/fixtures/preview_webview_specs_golden.txt` 字节不变;新增的协议黄金与注入脚本黄金在**搬迁前**生成、搬迁后字节不变。
- **安全约束原样保留(spec §5.5):** webview 与 `(project_id, panel, tab_id, path)` 绑定,路径只来自 Rust;文件字节端点只服务白名单路径(规范化后比较,拒绝 `..`、符号链接逃逸、编码斜杠、NUL);保存目标必须等于绑定路径;IPC 消息超过 `MAX_MESSAGE_BYTES`(8 MiB)或绑定不符一律丢弃并记日志,不 panic。
- **`dozer://` scheme 在 Digger 沿用**(Q2 的倾向),`Origin` 同源行为在 Task 8 的真机里确认;不通则停下报告。
- **预览 webview 的句柄表与章节编辑器(`markdown/host.rs`)永不共用,** nonce 空间不同;章节编辑器零改动。
- 只用 git tag 依赖;Dozer 内部 `path = "../.."` 除外。**合并、切 tag、推送是对外动作,向用户确认后执行。**
- 主工作区别人未提交的改动不动。Dozer 用 worktree `../dozer-preview-host`、分支 `feat/preview-host-lib`(从当前 `main` 切出);Digger 用 worktree `../digger-preview-p3`、分支 `feat/preview-p3-host`。
- **我的决定(可推翻):**
  - **D1** 宿主三件(脚本、IPC 分类、协议应答)进库,wry 构造与池留在产品:两个产品共享"页面能说什么、能读什么",不共享"怎么建 webview"。
  - **D2 Digger 本期不持有预览状态、不改布局。** 与 P2 的 D2 同理:Project/Files 到 P4 才有配对视图,现在把 `PreviewPane` 接进 Digger 只会留下不被渲染的死代码。P3 的 Digger 部分是"宿主 + dev 示例",由示例直接构造库的 `PreviewPane` 驱动。**请你审阅这条对设计 §6 P3 的收窄**(设计写的是"能在 Digger 里真正看到",示例满足"真机看到",但不在应用主界面里)。
  - **D3 web 资源搬进库的 `assets/`(`git mv`),Digger 以资源目录方式交付,不编译期内嵌。** 理由:`flyfish` 21 MB、`editor` 1.7 MB,内嵌会让二进制涨 25 MB 以上;Dozer 现在就是资源目录。开发态从库源码路径读(`env!("CARGO_MANIFEST_DIR")`,git 依赖检出目录在开发机上存在),打包态由 `scripts/package-macos.sh` 从 `cargo metadata` 找到的库目录拷到 `Contents/Resources/`。Digger 只拷第一批用到的目录:`editor`、`flyfish`、`image-annotate`(不含 `vendor/pdf`、`vendor/docx`,因为 `pdf`、`media`、Word 在能力里关闭)。**这是与 usage 面板(内嵌)不同的做法,请审阅。**
  - **D4** Digger 的预览资源预算先用固定保守默认值(Q3 的倾向),不做探测。

## Review Focus

1. **白名单越权:** 未显式打开的路径、`..`/`%2e%2e`/`%2f`/NUL、指向白名单外的符号链接、大小写不同的路径,协议应答一律 404;目录不能当文件读。→ Task 2。
2. **IPC 伪造与过期:** 页面带别的 `tab`/`proj`/路径、过期代数、超长消息、非 JSON、未知 `type`、多字节边界处截断,全部被丢弃且不影响后续消息。→ Task 3。
3. **路径里的特殊字符:** 含空格、中日文、`#`、`?`、`%`、`&` 的文件名走 `file URL → 协议 → 白名单` 全程往返一致。→ Task 2。
4. **生命周期泄漏:** 关页签、切项目、放大另一侧、能力关闭(json/csv/pdf)时 webview 被销毁或藏起,不留孤儿句柄;能力关闭的类型**根本不创建** webview。→ Task 6、7。
5. **与章节编辑器共存:** 同窗口里预览 webview 与章节编辑器同时存在时,各自的 IPC 只进各自的通道;预览宿主销毁不动编辑器的句柄表。→ Task 7。

---

## Dozer(仓库 `../dozer`,worktree `../dozer-preview-host`,分支 `feat/preview-host-lib`)

### Task 1: 基线、黄金与施工图

**Files:**
- Create:`crates/dozer-app/tests/fixtures/preview_protocol_golden.txt`、`crates/dozer-app/tests/fixtures/preview_init_script_golden.txt`
- Modify:`crates/dozer-app/src/assets.rs`(只加黄金测试)、`crates/dozer-app/src/runtime.rs`(只加一个把注入脚本构造抽成函数的原地重构,不搬位置)
- Create(digger 仓库):`docs/superpowers/notes/2026-10-19-preview-p3-inventory.md`

- [ ] **Step 1: 记录基线。** worktree 里 `cargo test --workspace 2>&1 | grep "^test result"`;单独记 `dozer-app` bin 测试数、`assets::` 测试数、`dozer-panel-preview` 测试数。
- [ ] **Step 2: 协议黄金(搬迁前)。** `handle_protocol_matches_the_golden`:对固定 URI 表(`flyfish/` 首页与一个 renderer 脚本、`editor/index.html`、`json-editor/index.html`、`tabular/…`、`html/host.html`、`image-annotate/host.html`、`plantuml-viewer/index.html`、`…/__file__<白名单内路径>`、`…/__file__<白名单外>`、`…/__file__/../x`、未知 scheme、未知命名空间)逐条记录 `status`、`mime`、`body` 的字节数与 FNV 哈希(文件内容用 tempdir 里的固定夹具),与 fixture 逐行比较(`UPDATE_GOLDEN=1` 仅此一次)。
- [ ] **Step 3: 注入脚本黄金(搬迁前)。** 把 `runtime.rs` 里拼 `script` 的那段原地抽成 `fn init_script(editor_binding_present: bool, is_flyfish_host: bool, report_title_for: Option<usize>) -> String`(逻辑不变),写 `init_script_matches_the_golden` 覆盖 2×2×{None,Some(7)} 的全部组合并与 fixture 逐字比较。
- [ ] **Step 4: 施工图(交用户过目后再继续)。** 写 `2026-10-19-preview-p3-inventory.md`,三部分,每项判 **进库 / 留 dozer-app(原因)/ 拆开(怎么拆)**:① `runtime.rs` `sync_webview_pool` 与 IPC 闭包里的每个分支(焦点、缩放、查找、粘贴、`title:`、各 host 的 envelope);② `app/update.rs` 中 `EditorWebviewEvent`、`JsonEditorEvent`、`FlyfishEvent`、`ImageAnnotateEvent`、`PlantUmlEvent`、`Tabular*` 及 Git 日志差异变体的处理函数,标出哪些碰 Dozer 全局(Agent/MCP 预览上下文、`dozerd`、Toast、弹窗);③ 把"待发给页面的命令"(`EditorCommand`/`TabularCommand`/PlantUML/Flyfish 搜索)送进 `evaluate_script` 的代码路径。**拿不准的停下报告,不要猜。**
- [ ] **Step 5: 验证并提交。** 全部测试通过、总数 = 基线 + 新增黄金测试数;提交说明写基线数字。

### Task 2: web 资源与协议应答进库

**Files:**
- Move:`crates/dozer-app/assets/{editor,flyfish,json-editor,tabular-host,image-annotate,plantuml-viewer}` → `crates/dozer-panel-preview/assets/`;`crates/dozer-app/src/{html_host.html,image_annotate_host.html}` → `crates/dozer-panel-preview/assets/hosts/`
- Create:`crates/dozer-panel-preview/src/assets.rs`
- Modify:`crates/dozer-app/src/assets.rs`(只保留非预览命名空间并委托)、`crates/dozer-app/src/runtime.rs`(白名单类型)、`scripts/build-macos-app.sh`(拷贝源目录)、`crates/dozer-app/src/workspace/state.rs`(`allowed_files` 字段类型)

**Interfaces:**
- Produces(`dozer_panel_preview::assets`):
  - `pub struct ProtocolReply { pub status: u16, pub mime: &'static str, pub body: Vec<u8> }`
  - `#[derive(Clone, Default)] pub struct AllowedFiles(..)`,方法 `insert(&self, path: PathBuf)`、`contains(&self, path: &Path) -> bool`(内部存规范化后的路径)、`len(&self) -> usize`
  - `pub struct AssetRoots { .. }`,构造 `AssetRoots::from_flyfish_root(PathBuf)`(兄弟目录推导,沿用现行规则)、`AssetRoots::from_lib_source()`(开发态)、`AssetRoots::in_dir(PathBuf)`(打包态,目录下直接放各命名空间子目录)
  - `pub fn serve_preview(roots: &AssetRoots, allowed: &AllowedFiles, uri: &str) -> Option<ProtocolReply>`:只处理 `flyfish/ editor/ json-editor/ tabular/ html/ image-annotate/ plantuml-viewer/` 七个命名空间,其余返回 `None`

- [ ] **Step 1: 先写失败的库测试,把 Task 1 黄金里的预览部分与 Review Focus 1、3 钉住。** 测试名:`serve_preview_matches_the_golden_rows`、`allowlist_rejects_traversal_encoded_slash_nul_and_symlink_escape`、`allowlist_compares_canonical_paths_and_rejects_directories`、`file_url_roundtrips_names_with_space_cjk_hash_question_percent_ampersand`、`serve_preview_returns_none_for_other_panels_namespaces`。
- [ ] **Step 2: `git mv` 资源与两个内嵌页面,** 在 `crates/dozer-panel-preview/src/assets.rs` 实现上面的接口:函数体从 `dozer-app/src/assets.rs` 的 `serve_vendored`、`serve_allowlisted_file`、`serve_html_file`、`mime_for`、`percent_decode` 与七个命名空间分支原样搬过来(逐字,不"顺手改进");`assets_root()` 的 `.app` 边界探测留在 `dozer-app`。
- [ ] **Step 3: `dozer-app` 的 `handle_protocol` 先调 `serve_preview`,`None` 时落到自己剩下的命名空间(`review-trace`、`usage-content`、`codehealth-content`、`todo-content`、`group-chat-content`);** `Workspace.allowed_files` 与 `runtime.rs` 里的 `Arc<Mutex<HashSet<PathBuf>>>` 换成 `AllowedFiles`,`Effect::AllowFile` 的执行改调 `insert`。`assets.rs` 里预览命名空间的测试随代码搬进库,其余留下。
- [ ] **Step 4: 改 `scripts/build-macos-app.sh` 的拷贝源路径,** `AssetRoots::from_flyfish_root` 的开发态路径改指库的 `assets/flyfish`;`cargo test -p dozer-panel-preview` 与 Task 1 的协议黄金必须字节不变。
- [ ] **Step 5: 验证并提交。** `cargo tree -p dozer-panel-preview` 无禁用依赖;`bash scripts/build-macos-app.sh debug` 能打包且包内 `Resources` 目录结构与改前一致(`diff <(cd <旧包>/Contents/Resources && find . | sort) <(…新包…)` 为空)。

### Task 3: 注入脚本与 IPC 分类进库

**Files:**
- Create:`crates/dozer-panel-preview/src/host_script.rs`、`crates/dozer-panel-preview/src/host_ipc.rs`
- Modify:`crates/dozer-app/src/runtime.rs`(IPC 闭包改用 `classify_ipc`;浏览器标题、应用面板、usage/codehealth/todo/group-chat 单槽分支留下)

**Interfaces:**
- Produces:
  - `pub fn init_script(opts: InitScriptOptions) -> String`,`pub struct InitScriptOptions { pub is_editor_host: bool, pub is_flyfish_host: bool, pub report_title_for: Option<usize> }`;输出与 Task 1 黄金逐字相同
  - `pub struct HostIpcContext { pub webview_id: usize, pub url: String, pub editor_binding: Option<EditorHostBinding> }`
  - `pub enum ZoomStep { In, Out, Reset }`
  - `pub enum HostIpc { Focus, MouseUp, Zoom(ZoomStep), FindPreview, FindPage, FindNative, PasteRequest, Event(HostEvent), Rejected(String), Other(String) }`
  - `pub enum HostEvent { Editor(EditorHostBinding, EditorEvent), JsonEditor(EditorHostBinding, JsonEvent), Flyfish(HostBinding, FlyfishEvent), ImageAnnotate(HostBinding, ImageAnnotateEvent), PlantUml(HostBinding, PlantUmlEvent), Tabular(EditorHostBinding, TabularEvent) }`(绑定类型沿用现有 `Message` 变体里的那些)
  - `pub fn classify_ipc(ctx: &HostIpcContext, body: &str) -> HostIpc`:`FindPreview` 仅 `is_flyfish_host` 时产出;信封消息先 `validate(binding)`,失败给 `Rejected(原因)`;解析失败给 `Rejected`;不认识的纯文本给 `Other(body)`(浏览器 `title:`、`paste` 之外的产品专属消息由产品处理)

- [ ] **Step 1: 失败的库测试。** `init_script_matches_the_golden_for_every_option_combination`;`classify_ipc` 表驱动:每个纯文本词汇各一行;每个 host 的合法 envelope → `Event`;**Review Focus 2 全组:** 别的 `tab`、别的 `proj`、别的 `panel`、别的路径、过期 `doc`、`body.len() > MAX_MESSAGE_BYTES`、`{` 开头但非 JSON、未知 `type`、空串、含 NUL → `Rejected` 或 `Other`,且之后同一上下文的合法消息仍被接受(`a_rejected_message_does_not_poison_later_ones`)。
- [ ] **Step 2: 实现。** `init_script` 的字符串逐字取自 Task 1 抽出的函数;`classify_ipc` 把 `runtime.rs` 闭包 `_ =>` 分支里 `parse_* + validate` 的判断顺序原样搬过来。
- [ ] **Step 3: `runtime.rs` 的闭包改为 `match classify_ipc(..)`,** 每个 `HostIpc` 映射回今天发送的同一个 `Message`(`Event(Editor(..))` → `Message::EditorWebviewEvent`,Git 日志面板的分流保留在映射里);`Rejected(r)` 打 `log_warn!`;`Other` 落到原有产品分支。用一个映射表测试(`host_ipc_maps_to_the_same_dozer_messages`)固定。
- [ ] **Step 4: 验证并提交。** 总测试数守恒 + 新增;注入脚本黄金不变。

### Task 4: 页面事件作用到 `PreviewPane`(进库)

**Files:**
- Modify:`crates/dozer-panel-preview/src/{message.rs,effects.rs,update.rs,pane.rs}`、`crates/dozer-app/src/app/update.rs`(对应分支改为转调)
- Test:`crates/dozer-panel-preview/src/update_tests.rs`、`crates/dozer-app/src/app/update.rs` 里随之搬走的测试

**Interfaces:**
- Consumes:Task 3 的 `HostEvent`;Task 1 施工图第 ② 部分的"进库"清单。
- Produces:`Message::Host(HostEvent)`;`update` 对它返回的 `Effect`(沿用 P2 的 `Effect` 枚举,新增只允许是:`SaveFile`、`OpenExternal(PathBuf)`、`PushHostScript { webview_id: usize, script: String }`、`AllowFile(PathBuf)`);`PreviewPane::take_host_scripts(&mut self, panel: ViewerPanel) -> Vec<(usize, String)>`(把待发给页面的命令编码为 `dispatch_script(..)` 字符串,取走后清空)。

- [ ] **Step 1: 失败的库测试,** 按施工图判为"进库"的每个事件至少一条:编辑器改动置脏并更新行列/选区;保存回执清脏、保存失败不丢缓冲;`Flyfish`/`ImageAnnotate`/`PlantUml` 的 ready/rendered/failed 驱动页签状态;**安全钉死:** `save_is_rejected_when_target_differs_from_bound_path`、`save_is_rejected_outside_project_dir`、`event_for_a_closed_tab_is_ignored`。
- [ ] **Step 2: 实现。** 逐个把 `app/update.rs` 里判为"进库"的处理体搬进 `update`(只碰 `PreviewPane`/核心/本地 IO 的部分);碰 Dozer 全局的部分原地留下,以 `Effect` 或适配层回调的形式分离(施工图已定)。
- [ ] **Step 3: `app/update.rs` 的各分支改成** `self.preview_ui(panel, Message::Host(event))` + 执行 `PushHostScript` 时调现有 `evaluate_script` 路径;`take_host_scripts` 取代 `dozer-app` 里分散的命令编码代码。
- [ ] **Step 4: 验证并提交。** 总数守恒 + 新增;`WebviewSpec` 黄金不变。

### Task 5: Dozer 收尾与验收

**Files:**
- Modify:`docs/` 下 Dozer 的预览文档(若有);必要时 `CLAUDE.md` 的架构段

- [ ] **Step 1: 全量门禁。** `cargo test --workspace`、`cargo fmt --check`、`cargo clippy --workspace --all-targets`(警告数不高于基线)、边界脚本、`check-shared-deps`、`check-i18n`(只允许基线里已有的失败),依赖闭包检查。
- [ ] **Step 2: 用户目测验收清单(交给用户做,我只给截图):** Files 与 Project 两个面板各打开 `.py`(编辑、保存、脏标)、`.md`、`.html`、`.png`、`.svg`、`.pdf`、`.json`、`.csv`、`.puml`;⌘F 查找(代码与 Flyfish 各一);⌘C/⌘V/⌘X/⌘A;缩放快捷键;放大一侧时 webview 被藏起;多页签切换。**验收通过前不合并。**
- [ ] **Step 3: 向用户确认后合并、切 `libs-<date><letter>` tag、推送。** 不发布产品版本。

---

## Digger(仓库 `digger`,worktree `../digger-preview-p3`,分支 `feat/preview-p3-host`;在 Dozer tag 之后开始)

### Task 6: 钉版与资源交付

**Files:**
- Modify:`Cargo.toml`、`Cargo.lock`、`crates/desktop/Cargo.toml`(`dozer-panel-preview` 依赖,不开 `tabular-loader`)、`scripts/package-macos.sh`、`scripts/check-host-deps.sh`(若有白名单)
- Create:`crates/desktop/src/preview_assets.rs`(`pub fn roots() -> AssetRoots`:打包态读 `Contents/Resources`,开发态 `AssetRoots::from_lib_source()`)

- [ ] **Step 1: 失败的测试。** `bundled_layout_has_the_three_first_batch_dirs`(对一个用 `package-macos.sh` 打出的调试包或其目录布局夹具断言 `editor/`、`flyfish/`、`image-annotate/` 存在且 `flyfish/vendor/pdf` 不存在);`roots_in_dev_resolve_to_the_pinned_checkout`。
- [ ] **Step 2: 升级到 Dozer 新 tag,** `package-macos.sh` 用 `cargo metadata --format-version 1` 取 `dozer-panel-preview` 的 `manifest_path` 所在目录,拷第一批目录到 `Contents/Resources/`;`check-shared-deps.sh`、`check-host-deps.sh` 通过。
- [ ] **Step 3: 验证并提交。** `bash scripts/package-macos.sh debug` 成功,包大小增量记入提交说明。

### Task 7: Digger 预览宿主 `preview_native`

**Files:**
- Create:`crates/desktop/src/adapters/preview_native.rs`、`crates/desktop/src/adapters/preview_native/plan.rs`
- Modify:`crates/desktop/src/adapters/mod.rs`
- Test:`plan.rs` 内的无头测试

**Interfaces:**
- Consumes:库的 `PreviewWebviewSpec`、`serve_preview`、`AllowedFiles`、`classify_ipc`、`init_script`、`HostEvent`;`bytehost_core::LogicalRect`。
- Produces:
  - `pub enum Op { Create { spec: PreviewWebviewSpec, rect: LogicalRect }, Update { id: usize, url: Option<String>, rect: LogicalRect, visible: bool }, Destroy { id: usize } }`
  - `pub fn plan(live: &BTreeMap<usize, LiveView>, desired: &[(PreviewWebviewSpec, LogicalRect)]) -> Vec<Op>`(纯函数;`url` 变化才 `Some`,`park_offscreen` 的宿主保持 `visible=true`)
  - `pub struct PreviewNative`:`new(allowed: AllowedFiles, roots: AssetRoots) -> Self`、`sync(&mut self, window: iced::window::Id, desired: Vec<(PreviewWebviewSpec, LogicalRect)>) -> iced::Task<Event>`、`update(&mut self, Event) -> Option<HostEvent>`、`destroy_all(&mut self, window: iced::window::Id) -> iced::Task<Event>`、`live_count(&self) -> usize`
  - `pub enum Event { Built(usize, Result<(), String>), Ipc(HostIpc), Done }`

- [ ] **Step 1: 失败的 `plan()` 测试。** `empty_to_empty_is_no_ops`;`new_spec_creates`;`url_change_updates_only_url`;`removed_spec_destroys`;`rect_only_change_updates_rect_without_url`;`parked_host_stays_visible_when_not_shown`;`capability_disabled_kinds_never_appear_in_desired_so_nothing_is_created`(用库 `desired_webviews` 对 json/csv/pdf 页签 + Digger 能力 `{code, rendered}` 求清单,断言为空);`destroy_all_lists_every_live_id`。
- [ ] **Step 2: 实现 `plan()`**,再实现 `PreviewNative`:句柄表是本模块私有 `thread_local`(`BTreeMap<usize, (u64 /*generation*/, wry::WebView)>`),创建用 `WebViewBuilder::new().with_url(..).with_bounds(..).with_initialization_script(init_script(..)).with_custom_protocol("dozer".into(), ..)`(应答调 `serve_preview`,`None` → 404)、`with_ipc_handler`(→ `classify_ipc` → `Event::Ipc`)、`with_navigation_handler`(只允许 `dozer://`)、`with_new_window_req_handler`(拒绝)、`with_download_started_handler`(拒绝)、`with_devtools(false)`、`build_as_child`;`generation` 与 `usage_native` 一致,过期的 `Built`/`Ipc` 丢弃。
- [ ] **Step 3: 窗口线程安全。** 所有句柄操作经 `window::run`;销毁在句柄表里 `remove` 后立即 drop,**不**跨帧持有。
- [ ] **Step 4: 验证并提交。** 无头测试全绿;`clippy`/`fmt`;**不接进产品主循环**(D2)。

### Task 8: dev 示例 `preview_gallery` 与真机验收

**Files:**
- Create:`crates/desktop/examples/preview_gallery.rs`、`crates/desktop/examples/fixtures/preview/{hello.py,notes.md,page.html,pic.png,logo.svg,data.json,sheet.csv,doc.pdf,"带 空格 #1.txt"}`

- [ ] **Step 1: 示例。** 一个 iced 窗口,左边文件列表(夹具),右边调用库 `PreviewPane` 打开所选文件,`desired_webviews` 的结果交给 `PreviewNative::sync`(矩形取窗口右半),`Event::Ipc` 经 `classify_ipc` 回灌 `Message::Host`;顶部有"关闭当前页签"、"放大右侧(藏起 webview)"、"全部销毁"三个按钮;底部状态行显示 `live_count()`。
- [ ] **Step 2: 自动检查。** 示例提供 `--selftest`:依次打开每个夹具、等就绪、断言 `live_count()` 与期望一致(`.py` `.md` `.html` `.png` `.svg` 各 1;`.json` `.csv` `.pdf` 为 0 且页签显示降级页)、关闭全部后为 0;退出码非零即失败。
- [ ] **Step 3: 真机验收(截图给用户,点击由用户做):** `.py` 可编辑并保存(磁盘内容变化);`.md` 渲染;`.html` 渲染且隔离;`.png`、`.svg` 渲染;`.json`/`.csv`/`.pdf` 降级并可用系统应用打开;`带 空格 #1.txt` 正常打开;放大/还原时 webview 藏起/回来;连续开关 50 次后 `live_count()` 回到 0;同时运行章节编辑器自检 `scripts/run-editor-selftest.sh` 不受影响。**同时确认 Q2:`dozer://` 在 Digger 里页面可以 fetch 同命名空间的 `__file__` 端点;不通则停下报告。**
- [ ] **Step 4: 文档与提交。** 更新 spec 状态(P3 完成、D2 收窄的结论)、本计划勾选;**不发布产品版本,合并/推送向用户确认。** 下一步 P4 计划在 S5b-1 落地后另写。

## 自检记录

- 覆盖:spec §6 P3 的"清单、协议应答、web 资源 / Digger 自己的宿主 / 真机看到"分别落在 Task 3、2、7–8;§7 的三条安全钉死测试落在 Task 2、3、4;Q1–Q3 分别由 Global Constraints、Task 8 Step 3、D4 回应。
- 已知收窄:D2(Digger 不在主界面里显示预览)与 D3(资源目录交付)需用户确认;Task 4 的范围由 Task 1 施工图决定,若施工图显示事件处理耦合 Dozer 全局过重,允许把 Task 4 的一部分顺延到 P4 并在本计划里记录。
