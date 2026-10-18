# S6d-1 探针与骨架:键位转换、输入法探针、字体、Agent 面板壳 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Digger 右栏最前出现 Agent 面板的**壳**(配对布局、登记、旧布局迁移、`diggerd` 不可用态),并把后面两期依赖的三块底座先做实:iced 键盘事件 → 终端字节的转换、macOS 中文输入法的送达探针(`ImeSink`)、终端字体与 `TermStyle`。**本期不建会话、不连终端**,那是 S6d-2。

**Architecture:** 全部在 `digger-desktop`(宿主适配层)里。`adapters/agent_keys.rs` 把 `iced::keyboard::{Key, Modifiers}` 转成 `dozer_panel_term::input::{TermKey, TermMods}`;`adapters/agent_ime.rs` 的 `ImeSink` 是一个包住任意 `Element` 的薄控件,在 `Widget::update` 里处理 `Event::InputMethod` 并 `shell.request_input_method(..)`;`fonts.rs` 注册内嵌 JetBrains Mono 并产出 `TermStyle`;`shell.rs` / `product.rs` 登记 `digger.agent`。输入法是否真能送达只能由人在真机上验证,所以本期有一个**人工验收门**,探针失败则停下报告,不自行选退路。

**Tech Stack:** Rust、iced 0.14(`iced_core::input_method`、`Widget`)、`dozer-panel-agent` / `dozer-panel-term`(Dozer `libs-*` tag)、byteui、bytehost-chrome。

**Spec:** `docs/superpowers/specs/2026-10-18-agent-panel-s6d-design.md`(§6.1、§6.4、§6.5、§7 S6d-1、§9 Q1–Q3)。已批准的取舍:Q1 新用户默认选中 Agent;Q2 先做探针再定退路;Q3 卡片文字回落 transcript 最后一句。

## Global Constraints

- **前置:** S5a 已合并(左栏有 Project、`project_id()` 与旧布局迁移的做法已存在);S6a 已合并(`diggerd`、`daemon::Daemon`);Dozer 已按 S6b、S6c-1、S6c-2 合并并切出 `libs-*` tag(含 `dozer-panel-agent`、`dozer-panel-term`)。tag 名以 S6c-2 Task 4 的实际结果为准,Digger 里所有 dozer libs 同一 tag。
- 依赖方向:`dozer-panel-agent`、`dozer-panel-term` 只允许 `digger-desktop` 依赖;`scripts/check-host-deps.sh`、`scripts/check-shared-deps.sh`、Cargo.lock 重复检查必须通过(同一 crate 一个版本)。
- **不改 Dozer 与库**;库缺什么就停下报告。
- **已有用户的 `shell.json` 不被重置:** 旧文件里没有 `digger.agent` → 回默认栏并移到右栏最前,**该用户右栏当前选中的面板不变**;左栏不动。新用户(无文件)右栏默认选中 Agent(Q1)。
- 面板 id `digger.agent` 是落盘的线上名,一经发布不得改。
- 主工作区有别人未提交的改动(S5a 在做),**不动**;实现在独立 worktree `../digger-s6d1`、分支 `feat/agent-shell`(从已含前置的 `main` 切出)。
- 不引入新的日志依赖。字体文件用 SIL OFL 的 JetBrains Mono,随仓库附许可证文本。
- 终端字体度量与 Dozer 一致:`font_size = 14.0`、`line_height_factor = 1.2`(Dozer `assets/theme/terminal.json`)。

## Review Focus

1. **按键进错地方或丢失:** 项目面板/编辑器的输入框聚焦时按键不得进终端;⌘ 组合键绝不进 PTY;修饰键单独按下、死键、组合键中间态不产生字节。→ Task 1。
2. **输入法状态残留:** 预编辑被取消、焦点离开、面板被切走后,候选框与预编辑文字必须清除;提交文本为空串、含换行、含超长文本时不 panic。→ Task 2。
3. **旧布局迁移的边界:** 旧文件只有一侧、右栏为空、右栏已含 `digger.agent`(重复迁移)、更高 schema 版本(不得回写);迁移幂等。→ Task 4。
4. **配对面板的退化尺寸:** 右区窄到 `min_first_px` 以下时列表自动收起,终端块宽/高算出 0 或负数时不 panic、网格至少 1×1。→ Task 4。
5. **`diggerd` 不可用时的体验:** 面板显示可读原因与「重试」,重试成功后面板正常;重复点击不并发多个 `ensure`;主窗口其余面板不受影响。→ Task 4。

---

## Digger(本仓库,worktree `../digger-s6d1`,分支 `feat/agent-shell`)

### Task 1: 键位转换 `adapters/agent_keys.rs`

**Files:**
- Create:`crates/desktop/src/adapters/agent_keys.rs`
- Modify:`crates/desktop/src/adapters/mod.rs`、`crates/desktop/Cargo.toml`(加 `dozer-panel-term`)、workspace `Cargo.toml`(升 dozer libs tag、加 `dozer-panel-term`、`dozer-panel-agent` 的 workspace 依赖)、`Cargo.lock`
- Test:同文件

**Interfaces:**
- Produces:
  - `agent_keys::to_term_key(key: &iced::keyboard::Key) -> Option<TermKey>`:`Key::Character(s)` → `TermKey::Char(s)`;`Key::Named(n)` → 对应 `TermNamedKey`(覆盖 S6c-1 Task 2 的 `TermNamedKey` 全部分支:`Space`、`Enter`、`Backspace`、`Tab`、`Escape`、四个方向键、`Home`、`End`、`PageUp`、`PageDown`、`Delete`、`Insert`、`F1`–`F12`);其余具名键(`Shift`、`Control`、`Alt`、`Super`、`CapsLock`、`Dead` 等)与 `Key::Unidentified` → `None`。
  - `agent_keys::to_term_mods(m: iced::keyboard::Modifiers) -> TermMods`(`ctrl = m.control()`、`alt = m.alt()`、`shift = m.shift()`、`super_key = m.logo()`)。
  - `agent_keys::key_bytes(key: &Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>>`:`mods.logo()` 为真恒 `None`(⌘ 组合键不进 PTY,应用快捷键另行处理);否则 `to_term_key` + `dozer_panel_term::input::key_to_bytes`。
  - `agent_keys::AppShortcut { Copy, Paste }` 与 `agent_keys::shortcut(key: &Key, mods: Modifiers) -> Option<AppShortcut>`(⌘C → `Copy`,⌘V → `Paste`,其余 `None`;规则对照 Dozer `platform/window_events.rs` 约 929–1010 行,实现者把对照结论写在文件头注释里)。
- Consumes:`dozer_panel_term::input::{TermKey, TermNamedKey, TermMods, key_to_bytes}`(S6c-1 Task 2)。

- [ ] **Step 1: 建 worktree 并升依赖。** 把 workspace `Cargo.toml` 里所有 dozer libs 的 `tag` 改为 S6c-2 的新 tag,新增 `dozer-panel-term`、`dozer-panel-agent`(后者本期只登记不使用,供 Task 4 引用 `AgentCapabilities` 等类型时用;若 Task 4 实际不引用则先不加)。`cargo update --workspace` 后跑 Cargo.lock 重复检查与 `scripts/check-shared-deps.sh`。
- [ ] **Step 2: 写失败测试**(同文件):
  - `byte_matrix_matches_dozers_known_outputs`:表驱动,期望字节取自 Dozer `dozer-panel-term::input` 的测试(实现者从该库的 `key_to_bytes` 测试逐条抄值,**不自己推**):字符键 `a`、`A`(shift)、空格、回车(`\r`)、退格(`0x7f`)、Tab、Esc、Ctrl+A..Z(`0x01..0x1a`)、Ctrl+C(`0x03`)、Alt+字符(`ESC` 前缀)、方向键在 `app_cursor` 真/假下(`ESC [ A` / `ESC O A`)、Home/End/PageUp/PageDown/Delete/Insert、F1–F12;
  - `command_combinations_never_reach_the_pty`(⌘+任意字符/具名键 `key_bytes` 恒 `None`);
  - `modifier_only_and_dead_keys_produce_nothing`(`Shift`、`Control`、`Alt`、`Super`、`CapsLock`、`Dead`、`Unidentified`);
  - `every_term_named_key_is_reachable_from_an_iced_named_key`(遍历 `TermNamedKey` 的全部变体,断言存在一个 iced `Named` 能映射到它,防止新增变体漏接);
  - `shortcut_recognizes_command_c_and_v_only`(含大小写、带 shift 的 ⌘⇧C 不算复制);
  - `multi_char_character_keys_are_passed_through_unchanged`(输入法提交之外的、`Key::Character` 含多个 Unicode 标量时原样转成 `Char`)。
- [ ] **Step 3: 运行确认失败**,然后实现(纯函数,不碰全局状态)。
- [ ] **Step 4: 验证。** `cargo test -p digger-desktop agent_keys`。
- [ ] **Step 5: 提交** `feat(agent): iced 键位到终端字节的转换`。

### Task 2: 输入法探针 `ImeSink`

**Files:**
- Create:`crates/desktop/src/adapters/agent_ime.rs`、`crates/desktop/examples/ime_probe.rs`(探针程序,不进产品)
- Modify:`crates/desktop/src/adapters/mod.rs`
- Test:`agent_ime.rs`

**Interfaces:**
- Produces:
  - `agent_ime::ImeState { preedit: Option<String>, active: bool }`(`Default`),方法 `on_event(&mut self, ev: &input_method::Event) -> Option<String>`:`Preedit(text, _)` 空串 → `preedit = None`、非空 → `Some(text)`,返回 `None`;`Commit(text)` → 清预编辑,**非空**时返回 `Some(text)`(调用方写入会话),空串返回 `None`;`Opened` → `active = true`;`Closed` → `active = false` 且清预编辑。
  - `agent_ime::ImeSink<'a, M>`:`ImeSink::new(content: Element<'a, M>, enabled: bool, cursor: impl Fn(Rectangle) -> Rectangle + 'a, on_commit: impl Fn(String) -> M + 'a, on_preedit: impl Fn(Option<String>) -> M + 'a) -> Self`,实现 `Into<Element<'a, M>>`。`enabled` 为真时,在每次 `RedrawRequested` 里 `shell.request_input_method(&InputMethod::Enabled { cursor: cursor(layout.bounds()), purpose: Purpose::Terminal, preedit })`;收到 `Event::InputMethod` 时通过 `ImeState` 处理并 `shell.publish(..)`;`enabled` 为假时不请求(输入法关闭)。`draw`、`layout`、`mouse_interaction`、`operate`、`overlay` 全部原样转给 `content`。
- Consumes:`iced_core::{input_method, Widget, Shell}`。

- [ ] **Step 1: 写失败测试**(状态机,纯逻辑):`preedit_update_replaces_the_previous_preedit`、`commit_returns_the_text_and_clears_the_preedit`、`empty_commit_returns_none_and_still_clears_the_preedit`、`commit_with_newlines_and_a_100k_char_string_is_returned_whole_without_panicking`、`closed_clears_everything`、`preedit_then_losing_enabled_is_cleared_by_the_sink`(`ImeSink` 的 `enabled` 由真变假时下一次事件处理清掉预编辑——用对 `ImeState` 的显式 `reset()` 调用固定)。
- [ ] **Step 2: 运行确认失败**,然后实现状态机与 `ImeSink`。`ImeSink` 的 `Widget::update` 先处理自己的事件,再调用 `content.update`(内容仍要收到鼠标/键盘事件)。
- [ ] **Step 3: 写探针程序 `examples/ime_probe.rs`:** 一个最小 `iced::application`:中间是一块深色矩形(代表终端格,固定光标格矩形),`ImeSink` 包住它,窗口里用等宽文本显示「已提交文本」与「当前预编辑」,另有一行状态显示最近的 `Opened/Closed` 事件。点击矩形获得焦点(`enabled = true`),点别处取消(`false`)。
- [ ] **Step 4: 人工验收门(用户在真机上做,我只负责启动和截图)。** `cargo run -p digger-desktop --example ime_probe`,依次验证:① 切到系统中文拼音输入法,输入 `nihao` 看到候选框出现在光标格附近、预编辑文字出现;② 回车/空格选词后「已提交文本」得到汉字;③ 输入中途 Esc 取消,预编辑清空;④ 点击矩形外,候选框消失;⑤ 连续快速输入一整句。把每一项的结果(通过/失败/现象)记进 `docs/superpowers/notes/2026-10-18-ime-probe.md`。**任一项失败:停下,把现象报告给用户,不选退路、不继续 Task 3 之后的输入法相关工作**(退路是用户的决定,见设计 Q2);与输入法无关的 Task 3、4 可以继续。
- [ ] **Step 5: 验证与提交。** `cargo test -p digger-desktop agent_ime`;提交 `feat(agent): ImeSink 与输入法探针`(探针结论文档一并提交)。

### Task 3: 字体与 `TermStyle`

**Files:**
- Create:`crates/desktop/src/fonts.rs`、`crates/desktop/assets/fonts/JetBrainsMono[wght].ttf`(从 Dozer `crates/dozer-app/assets/fonts/` 拷贝)、`crates/desktop/assets/fonts/OFL.txt`
- Modify:`crates/desktop/src/main.rs`(应用构建器注册字体、启动时剔除毒字体)、`crates/desktop/src/main.rs` 的 `mod` 声明
- Test:`crates/desktop/src/fonts.rs`

**Interfaces:**
- Produces:`fonts::{CODE_FONT_FAMILY: &str = "JetBrains Mono", JETBRAINS_MONO: &[u8] (include_bytes!), code_font() -> iced::Font, terminal_style() -> dozer_panel_term::style::TermStyle, sanitize_font_db()}`;`terminal_style()` 为 `TermStyle { font: code_font(), font_size: 14.0, line_height_factor: 1.2 }`。`sanitize_font_db` 与 Dozer `assets/fonts.rs` 同一做法(剔除纯位图的 "GB18030 Bitmap" / `NISC18030.ttf`,否则 CJK 回退会把整行排到 x=inf);重复调用无害。

- [ ] **Step 1: 写失败测试:** `code_font_is_jetbrains_mono`(`family == Family::Name("JetBrains Mono")`)、`embedded_font_bytes_are_a_truetype_font_that_names_itself_jetbrains_mono`(前四字节为 `00 01 00 00` 或 `true`,且字节里含 `JetBrains Mono` 的 UTF-16BE 或 ASCII 名字串)、`terminal_style_matches_dozers_terminal_json`(14.0 与 1.2)、`terminal_grid_size_is_stable_for_a_known_pane`(同输入同输出:对 `terminal_style().grid_size(w, h)` 取 `(600.0, 400.0)` 与 Dozer 同输入的结果相等,期望值由实现者在 Dozer worktree 里用同一公式算出后写死,附算式)、`sanitize_font_db_is_idempotent`。
- [ ] **Step 2: 运行确认失败**,然后实现。字体注册优先用 `iced::application(..).font(fonts::JETBRAINS_MONO)`;`sanitize_font_db` 在 `main()` 里 `run` 之前调用,拿 font system 的方式沿用 Dozer 的写法(经 iced 的 wgpu 图形层),如 Digger 的依赖里拿不到就停下报告。
- [ ] **Step 3: 验证。** `cargo test -p digger-desktop fonts`;`cargo build -p digger-desktop` 通过。
- [ ] **Step 4: 提交。**

### Task 4: Agent 面板壳:登记、布局迁移、不可用态

**Files:**
- Create:`crates/desktop/src/adapters/agent.rs`(本期只含壳:`AgentShellView` 与不可用态,会话状态留给 S6d-2)
- Modify:`crates/desktop/src/shell.rs`(`agent_id()`、`all_panels`、`meta`、`rules`、`defaults`、旧布局迁移)、`crates/desktop/src/product.rs`(`ShellHost::body` 增 `"digger.agent"`、`catalog` 登记、`on_visible`、`Msg::Agent(AgentShellMsg)`、与 `daemon::Daemon` 的 `ensure` 接线)、`crates/desktop/src/adapters/mod.rs`
- Test:`crates/desktop/src/shell.rs`、`crates/desktop/src/adapters/agent.rs`、`crates/desktop/src/product.rs`

**Interfaces:**
- Produces:
  - `shell::agent_id() -> PanelId`(`"digger.agent"`);`meta`:标题「Agent」、`IconKind::Brain`;`rules`:`default_side: Side::Right`,`pane: Some(PaneRules { spec: PaneSpec::new(false, true), default_split: byteui::theme::geometry::default_split_ratio(), min_first_px: list_min_width() })`(列表是第一块、可拖窄收起,与用量同形;卡片行宽度的取值真机上再调);`defaults()`:右栏 `[agent, apps, usage]`,`selected` 的右侧为 `agent`(Q1)。
  - `shell::migrate_missing_agent(model: &mut ShellModel, had_agent_in_file: bool)`:旧文件里没有 `digger.agent` 时把它从落点移到右栏第一位,**不改任何一侧的选中面板**;已含则什么都不做(幂等)。用 S5a 迁移 Project 时用过的现有移动消息,不新造。
  - `adapters::agent::AgentShellMsg { Retry, Ensured(Result<(), String>) }` 与 `AgentShell { status: Status }`,`Status { Unknown, Checking, Ready, Unavailable(String) }`;`AgentShell::view(&self) -> Element<'_, Msg>`:`Unavailable(reason)` 显示原因文案与「重试」按钮(按下发 `Retry`);`Checking` 显示「正在连接 Agent 服务…」;`Ready` 显示占位文案「Agent 面板即将可用」(S6d-2 替换为真实内容);无项目时显示与 Project 面板一致的空态。`update(&mut self, msg) -> Task<Msg>`:`Retry` 在 `Checking` 状态下被忽略(不并发多个 `ensure`),否则置 `Checking` 并 `Task::perform(daemon.ensure())`。
- Consumes:S6a 的 `daemon::Daemon::ensure`、`DaemonError`(`Display` 为可读中文);S1 的 `ShellModel`、`persist::load`。

- [ ] **Step 1: 写失败测试:**
  - `shell.rs`:`agent_panel_id_is_the_persisted_wire_name`(`"digger.agent"`);`the_default_layout_puts_agent_first_on_the_right_and_selects_it`(右栏以 `agent` 开头、`selected(Right) == agent`、左栏与 S5a 之后的断言一致);`an_old_file_without_the_agent_panel_moves_it_to_the_front_of_the_right_rail_and_keeps_the_users_selection`(文件右栏 `["digger.apps","digger.usage"]`、选中 `usage` → 迁移后 `[agent, apps, usage]`、选中仍是 `usage`、`left_width` 不变);`migration_is_idempotent_and_never_touches_a_file_that_already_has_the_agent`;`a_file_with_an_empty_right_rail_gets_the_agent_alone`;`a_newer_schema_file_is_still_never_written`(沿用已有断言并覆盖本迁移路径);`agent_is_a_pair_panel_and_usage_still_is_too`(更新原 `only_the_usage_panel_is_a_pair…`:现在有两个配对面板,其余仍单栏);`pair_rects_for_agent_on_the_right_zone_have_a_nonnegative_size_at_the_minimum_window`(最小窗口尺寸下 `first.w`、`second.w` 均 ≥ 0,宽到不足 `min_first_px` 时列表被收起而不是出现负宽)。
  - `agent.rs`:`unavailable_shows_the_reason_and_a_retry_that_checks_once`(连续两次 `Retry` 只发出一个 `ensure` 任务)、`a_successful_retry_turns_unavailable_into_ready`、`no_project_shows_the_same_empty_state_as_the_project_panel`。
  - `product.rs`:`the_agent_panel_is_registered_in_the_catalog_and_its_body_is_not_empty`、`switching_to_the_agent_panel_triggers_its_on_visible_action_once`(沿用 `visible_before` 机制)。
- [ ] **Step 2: 运行确认失败。**
- [ ] **Step 3: 实现。** `boot` 里的 `Daemon::ensure` 结果(S6a 已有)同时喂给 `AgentShell`(`Ensured`),不重复调用;`body` 的 `"digger.agent"` 分支返回 `AgentShell::view`,配对面板的双块摆放本期不做(S6d-2 替换成 `pair_view(列表, 终端)`)。
- [ ] **Step 4: 验证。** `cargo test -p digger-desktop`;`cargo build -p digger-desktop`;手动起一次 Digger(沙箱 `DIGGER_DATA_DIR`),截图确认右栏最前有 Agent 图标且默认选中、点开显示占位文案,`diggerd` 不可用(临时把 `diggerd` 改名)时显示原因与重试。
- [ ] **Step 5: 提交。**

### Task 5: 门禁与交付

- [ ] **Step 1: 全量。** `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets && cargo fmt --check`;`bash scripts/check-host-deps.sh`、`bash scripts/check-shared-deps.sh`、Cargo.lock 重复检查。
- [ ] **Step 2: 旧布局真机验证。** 用沙箱数据根放一份**不含 `digger.agent` 的旧 `shell.json`**(取自现有用户的真实形状),启动后右栏顺序、选中面板、左栏均符合 Task 4 的断言;再启动一次确认文件不再变化。
- [ ] **Step 3: 报告** 写明:探针每一项的结果与截图路径、哪些项依赖人工、依赖升级到的 tag、`min_first_px` 取值、遇到的与计划不符之处。**探针失败时在这里把「是否继续 S6d-2」交回用户。**
- [ ] **Step 4: 合并到 `main` 前向用户确认。**

---

## Self-Review

- **Spec 覆盖:** 设计 §6.1 登记/布局/旧布局迁移/Q1 → Task 4;§6.4 键盘转换与 ⌘ 规则 → Task 1,输入法 `ImeSink` 与探针 → Task 2;§6.5 字体与 `TermStyle`(网格尺寸的实际同步属 S6d-2)→ Task 3;§6.3 末句与 G6(`diggerd` 不可用态)→ Task 4;§7 S6d-1 的「依赖升级」→ Task 1 Step 1。**不在本期:** 会话、每项目状态、懒启动、尺寸同步、选择器、页签条、确认框、退出语义(S6d-2 / S6d-3)。
- **类型一致:** `TermKey` / `TermMods` / `key_to_bytes` 来自 S6c-1 Task 2,Task 1 引用同名;`ImeState`、`ImeSink` 在 Task 2 定义;`terminal_style()` 在 Task 3 定义,S6d-2 的 `PanelDisplay::term_style` 复用;`AgentShell` 在 Task 4 定义,S6d-2 扩成完整的 `AgentPanel`。
- **风险点:** ① `TermNamedKey` 的变体清单以 S6c-1 实际落地为准,Task 1 的「每个 TermNamedKey 可达」测试会在清单变化时失败提醒;② `ImeSink` 只在 `RedrawRequested` 期间请求才生效(iced 文档明示),实现者要确认 `update` 里在该事件分支调用;③ 输入法行为无法自动化验证,Step 4 的人工门是本期唯一的真实证据;④ 字体注册与 `sanitize_font_db` 取 font system 的路径依赖 Digger 的 iced 特性,拿不到就停下报告;⑤ `min_first_px` 先沿用 `list_min_width()`,卡片实际需要的宽度在 S6d-2 有卡片后再调;⑥ S5a、S5b-1 若尚未合并,Task 4 里关于左栏的断言以实际合并结果为准,只保证右栏与选中面板。
