# S5b-2 P4 复审修复清单(第二轮)

- 复审对象:Dozer `feat/preview-events-lib`(`966c0796`)、Digger `feat/preview-pairing`(`e93393c`)。第一轮清单见 `2026-10-19-s5b2-p4-review-fixes.md`。
- 复审方式:只读;跑过 `dozer-panel-preview` 391 个测试和 `digger-desktop --bin digger` 211 个测试(全过);**没有真机,没跑 `dozer-app` 测试**。标「推断」的必须由修复者用测试或真机证实。
- 结论:**Dozer 侧 No;Digger 侧 No。** 第一轮里不少「已修」的测试在修复前同样会通过,不能证明修复。
- 规则同第一轮:先写会失败的测试再改代码;搬迁过的测试不得删除或削弱;**合并、切 tag、推送、发布、提交版本号一律等用户明确确认**;每条在本文件里标「已修 + 提交号」或「不修 + 理由」。**请把第一轮清单里 D-* 各条也补上标注(目前 Dozer 部分一条都没标)。**

## 0. 最先修(决定预览能不能显示)

### N1 Digger 预览 webview 的 `sync` 任务被丢弃(Critical,推断,真机必验)
- 位置:`crates/desktop/src/product.rs` 约 732 行 `let _: Task<Msg> = window::run(window, move |w| host.sync(w, &desired)).discard();`,约 744 行异步块里同样 `let _ = window::run(..)` 注入脚本。
- 依据(复审者读 iced_runtime 0.14 `window::run` / `task::oneshot`):动作放在 stream 里,Task 没被返回给 `update` 就不会被轮询,正常路径下 `host.sync`(建 / 挪 / 销毁)和脚本注入都不会执行。只有 G-C5 新加的「无项目」路径把 `window::run(..)` 放进了返回的 `Vec<Task>`,是对的。
- 为什么没被发现:引入于 `a5a8f0f`(Task 8),selftest 是独立 pane、不走这个函数,所有测试盖不到。
- 修:`sync_preview_native` 返回 `Task<Msg>`,把两个 `window::run(..)` 放进返回的任务列表(`Task::batch` 或 `chain`,脚本注入必须在建完 webview 之后);**不要 `.discard()` 后丢弃**。
- 测试:`sync_preview_native_returns_the_native_tasks`(断言返回任务非 `Task::none()`,例如把 `window::run` 的构造抽成可检查的 `Vec<Op>` 加一个「任务数」断言);**真机验收:打开 `.py` 和 `.md` 后右半边确实出现 webview。**

## 1. Dozer

### D-I2(Critical,**已用探针证实可越权**)AllowedFiles 判与读不是同一个路径

**已修(Dozer,`8010a778`)** —— `assets.rs` 新增 `canonical_allowed_path`,`serve_allowlisted_file` 先拒绝含 `..` 的请求再查白名单,判定与读取走同一个 canonical 路径。新增 4 条 + 改 3 条测试,探针场景验红。
- 探针(复审者在 `966c0796` 的临时 worktree 里):白名单只含 `…/proj/ok.txt`,`proj/link` 是指向 `secret/deeper` 的符号链接;请求 `__file__/…/proj/link/../ok.txt` 返回 **200,内容是 `secret/ok.txt` 的 "SECRET"**。`/tmp/…` 与 `/private/tmp/…` 判为不等价。
- 原因:`assets.rs` 的 `clean_allowed_path` 是纯词法处理(文档仍写「不 canonicalize」),`contains` 看到的是折叠后的 `/proj/ok.txt`,`serve_allowlisted_file` 却 `fs::read(&原始解码路径)`,由系统解析 `link/..`。
- 修:
  - `insert` 与 `contains` 都 `canonicalize`(失败 → 拒绝,不降级放行);**判定与读取使用同一个 canonical `PathBuf`**;
  - `serve_allowlisted_file` 像 `serve_html_file` 一样拒绝含 `..` 分量的请求;
  - 显式拒绝目录;
  - 保持 `&self` + `Clone` 或在注释里写明 `Arc<Mutex<_>>` 的取舍。
- 测试(按这些名字出现;把探针转成回归测试):
  - `allowlist_rejects_traversal_encoded_slash_nul_and_symlink_escape`(含 `link/../ok.txt` 必须 404)
  - `allowlist_compares_canonical_paths_and_rejects_directories`(含 `/tmp` ↔ `/private/tmp`)
  - `file_url_roundtrips_names_with_space_cjk_hash_question_percent_ampersand`

### D-I3(未做)计划点名的 8 个安全测试与保存路径约束

**已修(Dozer,`8010a778`)** —— 新增 `pub(crate) fn path_within_root(root, path)`;`SaveRequested` 在 update 里拒绝超大文本(> 8 MiB → `text_save.too_large`)与项目根之外的路径(→ `text_save.outside_root`)。4 条新测试(后两条验红)。
- 现状:`host_event_for_a_closed_tab_is_ignored`、`host_event_from_a_stale_generation_is_ignored`、`save_is_rejected_when_the_event_path_differs_from_the_bound_path`、`save_is_rejected_outside_the_project_root`、`oversized_text_change_is_rejected_without_changing_the_tab`,以及上面三条白名单测试,在 `966c0796` **全部不存在**。
- 约束未实现:`host_update.rs` 没用 `ctx.project_root`,保存路径没有「在项目根内」校验;`Effect::SaveFile` 文档注释声称在 update 里校验,实际没有。
- 修:实现项目根校验(canonical 后前缀比较);在 update 里限制超大文本(不只靠 `classify_ipc` 的 8 MiB);补齐上述测试。

### D-I4(未恢复)审计测试仍被削弱

**已修(Dozer,`8010a778`)** —— `workspace/tests.rs` 的 `UI_HANDLER_FILES` 加入 `../dozer-panel-preview/src/host_update.rs`、`../dozer-panel-preview/src/effects.rs`;新增 `call_is_inside_spawn(source, at)` 与 `enclosing_fn_name(fragment)`,恢复「调用必须在 `handle.spawn*` 内」的强断言。两处破坏性验证均红,全量 1333 passed。
- `crates/dozer-app/src/workspace/tests.rs` 的 `ui_handlers_do_not_call_blocking_preview_entry_points`:强断言 `assert!(audited > 0)` 仍被换成「为 0 时只检查源码含某些字样」;新增禁止名单里的 `save_text_atomic(` 在 `dozer-app` 内无调用点,审计空转;范围没扩到 `dozer-panel-preview/src/host_update.rs`。
- 修:恢复强断言(或对新位置给出同等强度的断言);把审计范围扩到库里的 `host_update.rs`。

### D-C1 测试是循环论证

**已修(Dozer,`8010a778`)** —— 删掉 `workspace_for_host_event`,新增纯函数 `host_event_route` + `HostEventRoute`;3 条测试(含 `App::host_event` 端到端),把 `project_id` 写死为 2 立刻红。
- 代码对(`App::host_event` 按 `event.project_id()` 路由,项目不存在则丢弃),但两条测试(`update.rs` 约 5200–5320)只测了生产代码从不调用的 `workspace_for_host_event` 包装,真正的 `App::host_event` 没被测到。
- 修:测试走真实的 `App::host_event`(或删掉死函数并直接测 `preview_ui_for`),断言「未聚焦项目的事件作用在它自己的状态上」和「已移除项目的事件被丢弃」。

### N2 `save_finished` 在 revision 过期时仍清恢复快照(Important)

**已修(Dozer,`8010a778`)** —— 过期回执不再清快照;`editor_event` 补 `close_settled`,「保存后关闭」在 revision 过期时保持页签打开。`stale_save_receipt_keeps_the_recovery_snapshot` 与 `stale_save_receipt_does_not_close_a_tab_with_newer_edits` 验红。
- 位置:`crates/dozer-panel-preview/src/host_update.rs` 约 90–100:revision 不匹配时 `dirty` 保持为真,但 `ClearRecoverySnapshot` 照推 → 页签还脏而磁盘恢复文件被删,崩溃丢最近编辑。「保存后关闭」路径里 revision 过期的回执也会直接关页签,丢写盘期间的新编辑。
- 修:只在 `revision >= web_revision` 时清快照;revision 过期且 `pending_close` 时重新请求保存或保持页签打开。
- 测试:`stale_save_receipt_keeps_the_recovery_snapshot`、`stale_save_receipt_does_not_close_a_tab_with_newer_edits`。

### D-M1 / D-M2(未处理)

**D-M1 已修** —— rebase reword 为 `5295a911`,说明改为实测「库 +13 个 `#[test]`、app 0 新增(改 1 个既有)、删除 0」。**D-M2 已修(Dozer,`8010a778`)** —— 删除无产出点的 `Effect::PushHostScript` 及 Digger 侧对应分支,`push_script` 保留。
- D-M1:`4e05f0a4` 的提交说明「+27 / +9」与实际(main → `966c0796` 净增 `#[test]` 约 22,删 0)不符,更正。
- D-M2:`Effect::PushHostScript` 在库里仍是空操作且无产出点,Digger 却接了 `push_script`:接上或删除,并在注释写明。

### N7 / N8 / N9(Minor)

**N7 已修** —— 纯格式化噪声作为独立提交 `e7b2c710` 回退。**N8 已修(Dozer,`8010a778`)** —— 删除 `let _: &'a str = locale;` 占位,生命周期收窄写进提交说明。**N9 已修** —— GitLogDiff 的过滤行为写进 `12847f98` 说明。
- 提交里混入 `dozer-panel-files`、`extensions/files`、`app/view.rs` 的纯格式化 diff,拆出去或回退。
- `fallback_page` 的 `let _: &'a str = locale;` 占位断言无作用;生命周期改动是公共 API 签名变更,在提交说明里写明。
- GitLogDiff 事件现在也受 `loaded_workspace_mut` 过滤(绑定项目没加载就丢弃),与 main 行为不同,大概率是对的,但请在提交说明里说明。

## 2. Digger

### N1b 每帧 `Box::leak` 一个 `PreviewDisplay`(Important)

**已修(Digger,`1dd3d16`)** —— display 存进 `ProductShell::preview_displays` 两个 `Cell` 槽位,`view` 借用字段;`preview_display_is_not_leaked_every_frame` 断言 `preview_pane` 体内 grep 不到 `leak`。
- 位置:`product.rs::preview_pane` 约 894。每帧两份,60fps 下约每小时 10 MB 以上的无界泄漏;是为绕过库 `view::pane` 要求 host 活得和 Element 一样久。
- 修:把 display 存进 `ProductShell` 字段(在 `update` / 布局阶段更新),`view(&self)` 借用字段。
- 测试:不要靠计数泄漏;用「`preview_pane` 多次调用不增长」的结构性断言(例如返回借用的 display,类型上不再 `leak`),并在代码审查里确认 grep 不到 `Box::leak`。

### N6 协议闭包每次请求合并所有项目的白名单(Important)

**已修(Digger,`1dd3d16`)** —— 新增 `allowed_for_uri(&Mutex<HashMap<i64, AllowedFiles>>, uri)`,按 URL query 的 `proj=` 只查那一份,无 `proj` 返回空白名单;`a_page_of_project_a_cannot_read_a_file_allowed_only_for_project_b` 验红。
- 位置:`adapters/preview_native/mod.rs` 约 247–253。分项目存只实现了「撤销隔离」,没有实现「访问隔离」:A 项目的页面能读到 B 项目授权的文件;每次请求克隆全部路径,O(N)。
- 修:协议请求带上所属项目(例如在 URL 的绑定查询串里已有 `proj`,按它只查那一份);不属于任何项目的请求拒绝。
- 测试:`a_page_of_project_a_cannot_read_a_file_allowed_only_for_project_b`。

### 测试有效性(第一轮「已修」里测试不可信的几条)
- G-I1:**已修(Digger,`1dd3d16`)。** 改为走 `hub.host_scripts(ViewerPanel::Project, 1)`(测试经 `set_test_live_ids` 注入存活 id),断言返回 id == `binding.webview_id()`;把偏移加回去 → 红(`left: 2000001, right: 1000001`)。
  原:`project_panel_host_scripts_target_the_right_webview_id` 断言的是它自己构造的恒等函数 `with_pool_ids(..)`,没碰生产路径 `hub.host_scripts()`。重写成:往 Project 面板排一条命令,经 `hub.host_scripts()` 取出,断言目标 id 等于库给出的 `binding.webview_id()`(不再加偏移);把偏移加回去该测试必须失败。
- G-C1:**已修(Digger,`1dd3d16`)。** 两条都重写:`switching_projects_keeps_the_persisted_preview_state` 走 `open()` 真实路径(落盘由库侧 `PersistTabs` 效果执行),再 `set_current_project` 切走切回断言恢复;`forget_project_is_only_called_on_delete` 改名 `switching_projects_never_forgets_a_project`,改成行为断言(切走不撤授权/不删盘、切回重建、`forget_project` 才回收)。破坏性验证(切走时 `clear_allowed_for` + `persistence.remove`)→ 两测试均红。`persist_now` 文档已改为「生产代码不调用,只有测试用」,并修正 G-C1 标注里「切走前落盘」的说法。
  原:`switching_projects_keeps_the_persisted_preview_state` 在 hub 层跑,旧 bug 在 `product.rs`,修复前也通过,且靠只有测试调用的 `persist_now` 落盘。`forget_project_is_only_called_on_delete` 数 `forget_project(` 行数,修复前后都等于 1,是循环论证。改成:通过 `on_current_project_changed` 的真实路径切走再切回,断言 `preview_state/<id>.json` 仍在、页签恢复;并把「生产代码从未调用 `persist_now`」这个事实处理掉(接上或删除),修正 G-C1 标注里「切走前落盘」的说法。
- G-C3:**已修(Digger,`1dd3d16`)。** 把 `sync_preview_native` 的纯数据半段抽成 `collect_preview_desired(preview, shell, rects, project)`(真实接线,`window::Id` 无法构造),`syncing_the_native_pages_writes_back_the_tab_bar_width` 用它并断言 `hub.tab_bar_px(Files) == slot.w - 32*3 - 4*3`;去掉 `set_tab_bar_width` → 红(`left: 0.0, right: 297.6`)。**真机 50 页签翻页仍待用户点。**
  原:测试调用的是 `preview_display` 辅助函数并手动 `set_tab_bar_width`,没测 `preview_pane` 的接线,也没测 `sync_preview_native` 真的写回宽度。补一条走真实接线的测试;真机验证 50 个页签翻页。
- G-C5:**已修(Digger,`1dd3d16`)。** `sync_native_asks_preview_project_before_desiring_anything`:源码断言 `sync_preview_native` 体内含 `preview_project(` 与 `host.sync(w, &[])`,外加纯函数三态行为断言。N1 已让空清单分支真的同步。


### N4 注释与实现不一致

**已修(Digger,`1dd3d16`)** —— `set_current_project` 文档改为「切走不碰磁盘、不撤任何项目授权」,`persist_now` 改为「生产代码不调用,只有测试用」。
- `persist_now` 的文档注释和 G-C1 标注都说「切走前落盘」,实际只有测试调用;`set_current_project` 的注释说「切走只让旧项目的授权失效」,测试却断言「切走不撤」。统一为真实行为。

### N5 `.pdf` 注释与能力矛盾(用户 2026-10-20 裁决:PDF 要开)

**已修(Digger,`1dd3d16`)** —— `digger_capabilities()` 改 `pdf: true`;`scripts/package-macos.sh` 不再删 `flyfish/vendor/pdf`(改为删 `vendor/docx`);`preview_assets::REQUIRED_FILES` 加 `flyfish/vendor/pdf/pdf.worker.mjs`;夹具头与 `preview_smoke` 注释同步。`pdf_capability_is_on_and_the_library_honors_it` 断言能力开、`backend_enabled` 放行 `.pdf`、关掉则拒;改回 `pdf: false` → 红。
- `digger_capabilities()` 的注释和夹具头说「PDF 走降级页」,实际 `hub.desired` 对 `doc.pdf` 产生了 flyfish webview(9 个夹具的 desired 总数 5 = `.md .html .png .svg .pdf`,与 selftest 硬编码的 5 一致)。要么 `pdf:false` 没生效(那就是 bug,先查原因),要么只是注释错;和 `package-macos.sh` 排除 `flyfish/vendor/pdf` 对照,若 PDF 会进 webview 而资源被排除则页面会坏。**需要用户确认 PDF 要不要开**,再统一注释、能力、selftest、打包。

### N3 测试改了进程级 `PATH`(Minor)

**已修(Digger,`1dd3d16`)** —— 删除 `unsafe set_var("PATH", ..)` 的测试,改为注入记录闭包(`system_opener_receives_the_path_verbatim_as_a_single_argument`)加源码断言(`the_system_opener_never_goes_through_a_shell`:含 `.arg(path)`,不含 `sh -c`/`bash -c`/`cmd /C`)。
- `adapters/files.rs` 约 657:`unsafe set_var`,注释声称 `--test-threads=1`,但仓库里没有任何地方强制;假 opener 先建空日志再写入,测试每 10 ms 轮询读,可能读到空文件。改成注入假 opener(函数参数),不改全局环境。

### 其它
- 迟到消息过滤没有会话代次:**已修(Digger,`1dd3d16`)** —— `PreviewHub` 加 `epoch`,`set_current_project` / `forget_project` 改变项目时 `wrapping_add(1)`;`Msg::Preview` 与 `Msg::PreviewEvent` 回投都带 epoch,入口用 `PreviewHub::accepts(project_id, epoch)` 同时比对项目与代次;`PreviewDisplay` 新增 `epoch` 字段。`going_away_and_back_invalidates_the_previous_epoch` 验红(让 `accepts` 只看项目 id → 红)。
- 分支落后 `main`(`git diff main..HEAD` 里审查文件显示为删除):合并前先 rebase / 合并 `main`。

## 3. 仍需真机或用户拍板

- **G-I2(webview 矩形):已修(Digger,`1dd3d16`)。** `preview_webview_rect` 底部不再减 `inset`,`rect.y + rect.h == slot.y + slot.h`(贴齐库 `view::pane` 内容块底边,与 Dozer 非放大态 `h` 到 `content` 底边同口径);测试 `preview_rect_is_the_second_block_below_the_chrome` 改断言并验红(加回 `- inset` → 红 446 vs 454)。**顶部是否多 8px 仍待并排截图确认。**
  **复审原文:** 复审者核对:Dozer `webview_geometry.rs` 非放大态 `y = top_bar + zone.margin.top + preview_chrome_top_px(38)`,`h = window_h - y - margin.bottom - footbar`,**底部不留 8px**(代码注释说明故意贴着 footbar);Digger 是 `slot.y + 38 + 8`、底部再减 8。顶部是否多 8px 取决于 byteui 的 38 是否已含顶部 padding(静态无法确定),底部肯定与 Dozer 不同。修复者只证明了与 iced 布局自洽。**必须 Dozer 与 Digger 同一文件、同一窗口尺寸并排截图对比 webview 顶 / 底边**,不一致就改公式(并更新钉死当前公式的测试)。
- **G-I5(原生 webview 盖住 iced 浮层):受阻,按用户 2026-10-20 决定跳过,单开一轮。** 结构障碍已实测:Dozer 的 `native_menu.rs` 的 `install_content_view(&winit::window::Window)` 要 winit 窗口的 `NSView` 指针,Digger 是纯 iced 应用(无 `winit` / `raw-window-handle` 直接依赖),拿不到那个引用;绕过需 `objc2` + `objc2-app-kit` + `objc2-foundation` + `tiny-skia` + `usvg` + `resvg` 六个新依赖,并改 `overflow_menu`「返回 `Element`」的签名(原生 NSMenu 是同步副作用,不返回 Element)。用户裁决:先记录为受阻项,其余先收尾。
  **复审原文:** Dozer 在 macOS 上用原生 `NSMenu`(`dozer-app/src/chrome/native_menu.rs`,695 行,依赖 `dozer_core` 的日志宏,**在 `dozer-app` 里,Digger 不能直接引用**)弹页签溢出菜单等,删除确认用独立原生窗口(`platform/project_delete_overlay.rs`)。Digger 的落地方式(我的决定,可推翻):
  - **溢出菜单:** 把 `native_menu.rs` 移植到 `crates/desktop/src/adapters/native_menu.rs`(去掉 `dozer_core` 日志依赖,改用 `tracing`;`objc2` / `objc2-app-kit` / `objc2-foundation` 依赖按 Dozer 的最小 feature 集加进 Digger),`tab_chrome::overflow_menu` 在 macOS 走它;与 S6d 的 D9「Digger 暂时复制、以后迁进 byteui」同一性质,在模块头注释写明来源和待迁移。Files 右键菜单在左侧列表里,不会与预览 webview 重叠,暂不改。
  - **Project 删除确认卡:** 不移植独立 overlay 窗口(代价大);显示卡片期间让预览 webview 暂时不在期望清单里(销毁或藏起),关闭卡片后恢复。**未做** —— 与溢出菜单同属 G-I5,一并单开一轮。
  - 测试:`overflow_menu_uses_the_native_menu_on_macos`(菜单项描述构造为纯数据,可无头测);`delete_card_hides_the_preview_webviews_while_it_is_open`;**真机验收:** 溢出菜单与删除卡展开时可见、可点,不被 webview 遮住。
- **PDF:用户 2026-10-20 裁决 —— 要开。已修(Digger,`1dd3d16`)。** 原方案: 修:`digger_capabilities()` 的 `pdf: true`;打包脚本不再删除 `flyfish/vendor/pdf`(`vendor/docx` 仍按 Word 未开保持排除,除非用户另说);`digger_hosts_url`、selftest、`preview_assets` 的必需目录清单、夹具头与能力注释统一为「`.pdf` 走 flyfish webview」;包体积增量写进提交说明(flyfish 目录约 21 MB,大头是 PDF 的 cmap)。测试:`pdf_opens_in_a_webview_in_the_preview_hub`;**真机验收:** `doc.pdf` 能翻页显示;打包产物里 `Contents/Resources/flyfish/vendor/pdf` 存在。N5 随之关闭(注释与行为一致)。

## 4. 流程问题(需用户裁决,开发者不得自行处理)

- `libs-2026-10-19h`(轻量 tag,指向 `966c0796`,提交时间 10:27:25)已推送到 `origin`,**在用户验收 Dozer 之前**;`libs-2026-10-19g`(指向有 bug 的 `4e05f0a4`)仍在 `origin`。
- `origin` 上现在有 `feat/preview-events-lib`:reflog 显示 10:24:14 推送了 `30c0f05f`,10:27:33 又推送了 `966c0796`(提交被 amend 两次后强制推送)。第一轮审查时 origin 上还没有这个分支。
- 没有 `chore(release)`、版本号变更或合并到 `main` 的动作:`main` 与 `origin/main` 均在 `2eaba8cf`;Digger 分支未推送。
- **用户裁决(2026-10-20,按建议):** 修完并验收后统一切一个新 tag,旧的 `libs-2026-10-19g`、`libs-2026-10-19h` 删掉。**删除远端 tag 与新 tag 的推送仍是对外动作,到时由我向用户确认后执行,开发者不得自行处理;在此之前不要再切 tag、不要再 force 推送、不要再让 Digger 钉新的未验收 tag。**

## 5. 验收标准(本轮)

- 每条标注「已修 + 提交号」或「不修 + 理由」(Dozer 的 D-* 也要标)。
- 每个「已修」的测试,在修复前必须失败;复审者会抽查。
- Dozer:`cargo test --workspace`(含 `dozer-app`)总数 ≥ 修复前、无删除 / 削弱;三份黄金字节不变;`dozer-panel-preview` 依赖闭包无 `wry` / `winit` / `dozer-app` / `dozer-client` / `dozer-core`;`clippy`、`check-i18n`、`check-shared-deps`、边界脚本通过。
- Digger:`cargo test --workspace`、`clippy`、`fmt --check`、`check-shared-deps.sh`、`check-host-deps.sh --self-test`、`check-shell-regions.sh`、`run-editor-selftest.sh` 通过;打一个调试包,`preview_smoke --selftest` 在包内通过;**真机上配对预览确实显示(N1)**。
- 修完再交我复审(只读)。未经用户确认:不合并、不切 tag、不推送、不发布。

## 6. 第三轮处理结果(2026-10-20)

- **更正(G-I5):** 上文「沿用 Dozer 的原生 `NSMenu`」说错了。核实 Dozer:页签溢出菜单就是 iced 菜单;原生 `NSMenu` 只用于右键菜单。Dozer 的做法是**浮层开着就把 webview 真隐藏**(`app_modal_open` / `tab_overflow_open` / `panel_popup_open`)。Digger 已照做:页签溢出菜单、Files 右键菜单 / 对话框、Project 删除确认卡任一打开,预览 webview 隐藏而不是销毁(`hide_for_overlay`,有测试)。**不移植 `native_menu`。**
- **N1:** 预览同步任务现在作为返回值交给 iced 运行时(`native_preview_task`),新增用 `task::into_stream` 数窗口动作的测试和「构造后丢弃发出 0 个」的反面对照。
- **N6:** 页面所属项目在创建 webview 时由 Rust 定下,请求 URL 里的 `proj` 不再起作用。
- **Dozer:** `path_within_root` 改为 fail-closed;补 `save_is_rejected_when_the_event_path_differs_from_the_bound_path` 与 `path_within_root_is_fail_closed`;`scratch()` 并行竞争已修;残留格式化噪声与误提交的 Digger 审查文档已移除。
- **合并与 tag(用户已批准):** Dozer `main` = `b9373b02`,新 tag `libs-2026-10-20a`;远端 `libs-2026-10-19g`、`libs-2026-10-19h` 与分支 `feat/preview-events-lib` 已删。Digger `feat/preview-pairing` 已改钉 `libs-2026-10-20a`。
- **门禁(Digger,钉新 tag 后):** `fmt --check`、`clippy --workspace --all-targets` 无输出(干净);`cargo test --workspace` 全过(桌面 223、store 35、11 …);`check-shared-deps`、`check-host-deps`(含 `--self-test`)、`check-shell-regions` 通过;`dozer-*` 无重复版本。
- **仍待人工:** G-I2 webview 上下边距与 Dozer 并排对比;预览是否真的显示、编辑保存、切项目、放大、PDF、浮层不被盖;打包产物里 `flyfish/vendor/pdf` 存在。
- **未做的已知项:** `apps::tests::closing_during_startup_waits_and_releases_the_root` 偶发失败(单独跑稳定通过,在 Apps 代码里,与本分支无关);D-C1 历史提交说明里「端到端」措辞偏强,以本节为准。
