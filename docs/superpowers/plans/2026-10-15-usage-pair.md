# S2 用量面板接入"列表 | 内容"配对 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Digger 右侧面板区里的「用量」从单栏变成 Dozer 里的"内容 | agent 筛选列表"配对:列表可拖宽拖窄、可收起展开,比例与收起状态落盘,原生图表只占内容栏。

**Architecture:** 共享骨架 `bytehost-chrome` 新增一条通用消息 `ShellMsg::TogglePane`(翻转配对面板的收起态)。Digger 侧 `shell::rules()` 给用量返回配对规格;`product.rs` 用**一个函数** `usage_layout()` 决定三态(无数据 / 收起 / 拼分栏),面板 body 的渲染和原生 webview 的矩形都读它。真实窗口自检新增一个 `usage` 场景,读用量 webview 的真实 NSView。

**Tech Stack:** Rust 2024、iced 0.14、wry、`bytehost-chrome` / `bytehost-desktop`(`PaneStore`、`apply_pane_split_drag`)、共享库 `dozer-panel-usage`(**钉在 `67ab0e6`,不升级**)。

**Spec:** `docs/superpowers/specs/2026-10-15-usage-pair-design.md`(S1 的 spec 与计划:`2026-10-14-window-shell-design.md`、`plans/2026-10-14-window-shell.md`)

## Global Constraints

- **不升级 `dozer-panel-usage`**:Cargo.toml 里它的 `rev = "67ab0e657d75135b882ace2f3aa7a56617776c60"` 一个字符都不许动(spec §1)。
- **不推送、不打 tag**:S1 留下的本地 worktree 与开发期 `[patch]` 原样保留(`../bytehost-chrome`、`../byteui-shell`)。
- **持久化文件名与面板 id 不变**:`shell.json`,面板 id `digger.usage`。
- **不新增依赖**:`bytehost-desktop`、`dozer-model`、`dozer-panel-usage` 在 `digger-desktop` 里都已经是依赖。
- **不改 byteui、不改 Dozer。**
- **独立的期望值**:真实窗口自检里的期望矩形只用字面量和算术手算,不调用骨架的几何函数(沿用 S1 的做法)。
- 注释与现有代码风格一致(中文注释,说"为什么");不留 TODO / 占位。
- 每个任务先写测试并**看它失败**,再写实现。
- 目录约定:`ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy`;`$ROOT/bytehost-chrome` 是 `bytehost` 的 worktree(分支 `feat/chrome`);Digger 在 `$ROOT/digger`。

## Review Focus

spec 没细说、但最可能伤到真实使用者的五类输入;每一条都落到了具体任务的具体测试里:

1. **列表被拖到窄于最小宽度而收起**:收起后没有分隔线、没有列表,图表 webview 必须立即变宽占满;再点按钮要能展开,比例回到收起前。→ Task 1 `toggle_pane…`、Task 4 `native_rect_follows_the_layout`。
2. **数据来了又没了**(再次进入面板会重新取数,取数期间 `loading`):布局从分栏回到"无数据"时,body 和原生矩形必须**同时**回到整个内容区,不能 body 已经单栏、图表还停在分栏位置。→ Task 4 `usage_layout_has_three_states`、`native_rect_follows_the_layout`。
3. **手改 / 损坏的 `shell.json` 里用量的条目**:`collapsed: true` 但此刻没有数据,不能显示成一个空列表占位;`split` 为 0 或 1 的条目已被 S1 读盘消毒丢弃。→ Task 4 `collapsed_without_data_is_content_only`。
4. **库的 `ToggleListCollapse` / `CollapseHover` 消息一旦送进库的 `update` 就 `unreachable!` 崩溃**(现有占位按钮发的正是 `ToggleListCollapse`,是个已存在的潜在崩溃)。→ Task 3 `library_messages_that_the_kernel_must_intercept_never_reach_the_library`。
5. **用量被拖到左栏(镜像)或在放大态里**:图表 webview 必须跟着内容栏换位置,而不是留在原处。→ Task 4 的 `native_rect_follows_the_layout`(镜像矩形)与 Task 5 的 U6(真实 NSView)。

---

### Task 0: 分支与基线

**Files:** 无改动。

- [ ] **Step 1: 确认起点干净,建 Digger 分支**

```bash
export ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy
cd $ROOT/digger && git status --short && git branch --show-current
git switch -c feat/usage-pair
git -C $ROOT/bytehost-chrome status --short && git -C $ROOT/bytehost-chrome branch --show-current
git -C $ROOT/bytehost status --short | wc -l     # 记作 N:他人的 i18n 改动文件数,之后不得变化
```

Expected: Digger 只有 `?? .dozer/` 与 `?? spike/corpus/`;`bytehost-chrome` 的状态为空、分支 `feat/chrome`;`bytehost` 主工作区的数字记作 N(S1 结束时是 25,别人继续改它时可能变,只要不是我们造成的)。

- [ ] **Step 2: 基线测试全绿**

```bash
cd $ROOT/bytehost-chrome && cargo test -p bytehost-chrome 2>&1 | grep -E "test result|FAILED"
cd $ROOT/digger && cargo test -p digger-desktop 2>&1 | grep -E "test result|FAILED"
```

Expected: `66 passed`(chrome)和 `77 passed`(digger-desktop);无 `FAILED`。

---

### Task 1: 共享骨架 —— `ShellMsg::TogglePane`

**Files:**(在 `$ROOT/bytehost-chrome`,分支 `feat/chrome`)
- Modify: `crates/bytehost-chrome/src/model.rs`

**Interfaces:**
- Produces: `ShellMsg::TogglePane(PanelId)` —— 翻转该面板在 `PaneStore` 里的收起态:面板有配对规格时翻转,保留已有 `split`,条目不存在用 `default_split` 建;返回 `Outcome { persist: true, .. }`。面板没有配对规格(含未知 id)时什么都不做、`persist: false`。

- [ ] **Step 1: 先写测试(加在 `model.rs` 的 `mod tests` 里,放在 `a_panel_without_a_pair_ignores_pair_drags` 之后)**

```rust
    // ---- 切换配对收起 ----
    #[test]
    fn toggle_pane_flips_collapsed_keeps_the_split_and_asks_to_persist() {
        let mut s = model();
        // 还没有条目:第一次切换用 default_split(0.4)建一条已收起的
        let out = run(&mut s, ShellMsg::TogglePane(id("t.list")));
        assert!(out.persist);
        assert_eq!(s.panes().get(&id("t.list")), Some(PaneDims::new(0.4, true)));
        // 再切换一次展开
        assert!(run(&mut s, ShellMsg::TogglePane(id("t.list"))).persist);
        assert_eq!(s.panes().get(&id("t.list")), Some(PaneDims::new(0.4, false)));
        // 拖出一个 0.5 的比例,收起再展开后比例还在
        run(&mut s, ShellMsg::DividerPress(Divider::Pair(Side::Left)));
        run(&mut s, ShellMsg::CursorMoved { x: 51.0 + 312.0, y: 300.0 });
        run(&mut s, ShellMsg::PointerReleased);
        run(&mut s, ShellMsg::TogglePane(id("t.list")));
        let d = s.panes().get(&id("t.list")).unwrap();
        assert!(d.collapsed && (d.split - 0.5).abs() < 1e-3);
        run(&mut s, ShellMsg::TogglePane(id("t.list")));
        let d = s.panes().get(&id("t.list")).unwrap();
        assert!(!d.collapsed && (d.split - 0.5).abs() < 1e-3, "the dragged split survives a collapse/expand round trip");
    }

    #[test]
    fn toggle_pane_on_a_panel_without_a_pair_does_nothing() {
        let mut s = model();
        for p in ["t.plain", "t.nope"] {
            let out = run(&mut s, ShellMsg::TogglePane(id(p)));
            assert!(!out.persist, "{p}");
        }
        assert!(s.panes().is_empty());
    }

    #[test]
    fn a_collapsed_pair_expands_again_when_dragged_back_out() {
        let mut s = model();
        run(&mut s, ShellMsg::TogglePane(id("t.list")));
        assert!(s.panes().get(&id("t.list")).unwrap().collapsed);
        // 收起时没有分隔线可拖;直接模拟"仍在拖拽中"的光标回到宽处:展开
        run(&mut s, ShellMsg::DividerPress(Divider::Pair(Side::Left)));
        run(&mut s, ShellMsg::CursorMoved { x: 51.0 + 250.0, y: 300.0 });
        assert!(!s.panes().get(&id("t.list")).unwrap().collapsed);
    }
```

- [ ] **Step 2: 运行,确认失败**

```bash
cd $ROOT/bytehost-chrome
cargo test -p bytehost-chrome toggle_pane 2>&1 | grep -E "^error|no variant|test result" | head
```

Expected: 编译失败,`no variant named TogglePane`。

- [ ] **Step 3: 实现**

`model.rs` 的 `ShellMsg` 里,在 `MaximizeClose,` 之后加:

```rust
    /// 切换配对面板"第一个 slot 收起"(列表的收起 / 展开按钮)。面板没有配对规格时什么都不做。
    TogglePane(PanelId),
```

`ShellModel::update` 里,在 `ShellMsg::MaximizeClose => { ... }` 这一支之后加:

```rust
            ShellMsg::TogglePane(panel) => {
                let Some(pane) = (ctx.rules)(&panel).and_then(|r| r.pane) else {
                    return Outcome::default();
                };
                let collapsed = self.panes.get_or(&panel, PaneDims::new(pane.default_split, false)).collapsed;
                self.panes.set_collapsed(&panel, !collapsed, pane.default_split);
                Outcome { persist: true, ..Outcome::default() }
            }
```

- [ ] **Step 4: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 69 passed`(66 + 3),没有 warning。

- [ ] **Step 5: Commit(在 `$ROOT/bytehost-chrome`)**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): ShellMsg::TogglePane flips a pair panel's collapsed state

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Digger —— 用量的配对规格

**Files:**(在 `$ROOT/digger`,分支 `feat/usage-pair`)
- Modify: `crates/desktop/src/shell.rs`

**Interfaces:**
- Consumes: Task 1 的 `ShellMsg::TogglePane`(本任务不用,Task 3 用);S1 的 `PanelRules` / `PaneRules`。
- Produces:
  - `shell::list_min_width() -> f32` —— 列表窄于它就收起;与 Dozer `project::footer_min_width()` 同一公式。
  - `shell::rules(&PanelId) -> Option<PanelRules>` —— 仅 `digger.usage` 返回 `Some`(右栏、`PaneSpec::new(false, true)`、`default_split = byteui default_split_ratio()`、`min_first_px = list_min_width()`),其余面板仍为 `None`。
  - `shell::pair_rects(&ShellModel, Side) -> Option<PairRects>` —— 按产品的度量与规则算某侧的配对矩形。

- [ ] **Step 1: 先写测试(`shell.rs` 的 `mod tests` 末尾)**

```rust
    #[test]
    fn only_the_usage_panel_is_a_pair_and_it_matches_dozers_usage_pane() {
        let r = rules(&usage_id()).expect("usage has rules");
        assert_eq!(r.default_side, Side::Right, "usage lives on the right by default, like Dozer");
        let pane = r.pane.expect("usage is a pair");
        // 列表是第一块(默认渲染在内容之后),且可拖窄收起
        assert_eq!(pane.spec, bytehost_desktop::PaneSpec::new(false, true));
        assert_eq!(pane.default_split, byteui::theme::geometry::default_split_ratio());
        assert_eq!(pane.min_first_px, list_min_width());
        for p in [markdown_id(), info_id(), apps_id()] {
            assert!(rules(&p).is_none(), "{p} stays a single-column panel in S2");
        }
    }

    #[test]
    fn the_list_min_width_is_dozers_footer_formula_at_the_default_theme() {
        // 默认主题(缩放 1.0):行图标 14、标签字号 13。
        // 按钮宽 = 14 + 6 + 13*4 + 16 = 88;最小宽 = 88*2 + 6 + 16 = 198。
        assert_eq!(list_min_width(), 198.0);
    }

    #[test]
    fn pair_rects_for_usage_split_the_right_zone_with_the_list_second_and_the_content_first() {
        let d = defaults();
        let model = ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, (1200.0, 800.0)).unwrap();
        let p = pair_rects(&model, Side::Right).expect("the right side shows usage");
        // zones = 1200-88-8 = 1104,左区 640 ⇒ 右区 464;右区内容:x 44+640+8+1=693,宽 464-6-2=456;
        // 配对宽 456-8=448,列表占 0.35 ⇒ 156.8,内容 291.2
        assert!((p.second.x - 693.0).abs() < 1e-3, "content first, on the left");
        assert!((p.second.w - 291.2).abs() < 1e-3);
        assert!((p.first.w - 156.8).abs() < 1e-3);
        assert!(p.first.x > p.second.x, "the list is to the right of the content");
        assert!(pair_rects(&model, Side::Left).is_none(), "the editor is not a pair");
    }
```

- [ ] **Step 2: 运行,确认失败**

```bash
cd $ROOT/digger
cargo test -p digger-desktop shell:: 2>&1 | grep -E "^error|cannot find|test result" | head
```

Expected: 编译失败(`list_min_width` / `pair_rects` 未定义),或 `rules` 对用量返回 `None`。

- [ ] **Step 3: 实现**

`shell.rs` 的 `use` 行改为:

```rust
use bytehost_chrome::{Ctx, Defaults, Loaded, Outcome, PaneRules, PairRects, PanelMeta, PanelRules, ShellModel, ShellMsg, Side, metrics_from_theme, persist};
use bytehost_core::id::PanelId;
use bytehost_desktop::PaneSpec;
use byteui::interaction::icons::IconKind;
```

把现有的 `rules` 函数(带 "S1 里 Digger 还没有……" 注释的那个)整体替换为:

```rust
/// 配对里"列表"那一块的最小宽度:再窄就收起,免得底部按钮行被挤爆。
/// 与 Dozer `extensions/project/view.rs` 的 `footer_min_width()` 同一公式(两颗按钮 + 间距)。
pub fn list_min_width() -> f32 {
    let icon = byteui::theme::icon_size::row();
    let label_px = byteui::theme::font::label() as f32;
    let button_w = icon + 6.0 + label_px * 4.0 + 16.0;
    button_w * 2.0 + 6.0 + 16.0
}

/// 面板的配对规则。S2 起用量是"内容 | agent 列表"配对,与 Dozer 的用量一致:列表是第一块,默认渲染在内容之后
/// (所以在右栏时是"内容 | 列表",拖到左栏镜像成"列表 | 内容"),可拖窄收起。其余面板仍是单栏。
pub fn rules(panel: &PanelId) -> Option<PanelRules> {
    match panel.as_str() {
        "digger.usage" => Some(PanelRules {
            default_side: Side::Right,
            pane: Some(PaneRules { spec: PaneSpec::new(false, true), default_split: byteui::theme::geometry::default_split_ratio(), min_first_px: list_min_width() }),
        }),
        _ => None,
    }
}

/// 某侧当前面板的配对矩形(度量与规则取产品的);该侧面板不是配对面板时为 `None`。
pub fn pair_rects(model: &ShellModel, side: Side) -> Option<PairRects> {
    let metrics = metrics_from_theme();
    model.pair_rects(side, &Ctx { metrics: &metrics, rules: &rules })
}
```

(`use bytehost_chrome::Side` 已在 `mod tests` 里单独引入,与顶部的 `Side` 不冲突:删除 `mod tests` 里的那一行 `use bytehost_chrome::Side;`,因为 `use super::*;` 已带进来。)

- [ ] **Step 4: 运行**

```bash
cargo test -p digger-desktop 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 80 passed`(77 + 3);没有 warning。

- [ ] **Step 5: Commit**

```bash
git add crates/desktop/src/shell.rs
git commit -m "feat(desktop): the usage panel is a list | content pair, like Dozer's

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Digger —— 用量面板的视图、展示适配与崩溃防护

**Files:**
- Modify: `crates/desktop/src/adapters/usage.rs`(拆出 `content_view` / `list_view`、`has_agent_filter`、展示适配改接骨架、样例数据)
- Modify: `crates/desktop/src/product.rs`(拦截库里"必须由宿主拦截"的两条消息)

**Interfaces:**
- Consumes: Task 1 的 `ShellMsg::TogglePane`;`shell::usage_id()`。
- Produces:
  - `UsagePanel::has_agent_filter(&self) -> bool`
  - `UsagePanel::content_view(&self, collapsed: bool) -> Element<'_, Msg>` —— 内容栏;`collapsed` 只决定标题行末尾那颗按钮显示"展开"还是"收起"。**替换**原来的 `view()`。
  - `UsagePanel::list_view(&self) -> Element<'_, Msg>` —— agent 筛选列表。
  - `adapters::usage::sample_loaded_message() -> dozer_panel_usage::Message` —— 一条带一行样例数据的 `Loaded`(自检与测试用,让面板进入"有数据"态)。
  - `PanelDisplay::toggle_list_collapse()` 现在返回 `Msg::Shell(ShellMsg::TogglePane(usage_id()))`;`collapse_hover(_)` 返回 `Msg::Shell(ShellMsg::Noop)`。

- [ ] **Step 1: 先写测试**

`adapters/usage.rs` 末尾(文件现在没有 `mod tests`)加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::ShellMsg;

    fn panel() -> UsagePanel {
        let svc: Arc<dyn UsageService> = Arc::new(digger_store::UnavailableUsageService::new("test".into()));
        UsagePanel::new(svc, PathBuf::from("/tmp/digger-usage-test"))
    }

    #[test]
    fn the_list_collapse_button_talks_to_the_shell_not_to_the_library() {
        let p = panel();
        assert!(matches!(p.toggle_list_collapse(), Msg::Shell(ShellMsg::TogglePane(id)) if id == crate::shell::usage_id()));
        assert!(matches!(p.collapse_hover(true), Msg::Shell(ShellMsg::Noop)));
        assert!(matches!(p.collapse_hover(false), Msg::Shell(ShellMsg::Noop)));
    }

    #[test]
    fn the_agent_filter_exists_only_once_there_is_data() {
        let mut p = panel();
        assert!(!p.has_agent_filter(), "nothing loaded yet");
        p.update(sample_loaded_message());
        assert!(p.has_agent_filter());
    }

    #[test]
    fn both_panes_build_in_both_collapse_states() {
        let mut p = panel();
        p.update(sample_loaded_message());
        let _ = p.content_view(false);
        let _ = p.content_view(true);
        let _ = p.list_view();
    }
}
```

`product.rs` 的 `mod tests` 里(`the_view_builds_in_the_states_a_user_can_reach` 之后)加:

```rust
    /// 评审关注点 4:库的 `update` 对 `ToggleListCollapse` / `CollapseHover` 是 `unreachable!`(它们必须由宿主拦截)。
    /// 任何一条漏进去都会让进程崩溃,所以产品在 `Msg::Usage` 入口先拦截。
    #[test]
    fn library_messages_that_the_kernel_must_intercept_never_reach_the_library() {
        let mut shell = fresh("intercept");
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::CollapseHover(true))));
        assert!(shell.shell.panes().get(&shell::usage_id()).is_none(), "hover changes nothing");
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::ToggleListCollapse)));
        assert_eq!(shell.shell.panes().get(&shell::usage_id()).map(|d| d.collapsed), Some(true), "the library's toggle is routed to the shell");
    }
```

- [ ] **Step 2: 运行,确认失败**

```bash
cargo test -p digger-desktop usage 2>&1 | grep -E "^error|cannot find|test result" | head
```

Expected: 编译失败(`has_agent_filter` / `content_view` / `list_view` / `sample_loaded_message` 未定义)。

- [ ] **Step 3: 实现 —— `adapters/usage.rs`**

`use` 区加:

```rust
use bytehost_chrome::ShellMsg;
```

把 `UsagePanel::view`(带 "原生标题与加载态……" 注释的整个函数)替换为:

```rust
    /// 库的 agent 筛选栏只在有数据时才拼:加载中 / 还没数据时调用方只显示内容栏(同 Dozer)。
    pub fn has_agent_filter(&self) -> bool {
        self.state.has_agent_filter()
    }

    /// 内容栏:标题 + 加载态;图表由同源 UsageSurface 和嵌入资源驱动的原生 webview 盖在它下面。
    /// `collapsed` 只决定标题行末尾那颗按钮显示"展开"还是"收起"(列表是否收起由骨架的 `PaneStore` 记账)。
    pub fn content_view(&self, collapsed: bool) -> Element<'_, Msg> {
        if let Some(error) = &self.native.error {
            return iced::widget::text(format!("用量视图创建失败：{error}；切换页签可重试")).into();
        }
        // 区域外框已由骨架画,这里不再加边框。
        dozer_panel_usage::view::content_pane(self, collapsed, &self.state, Length::Fill, Border::default())
    }

    /// agent 筛选列表。
    pub fn list_view(&self) -> Element<'_, Msg> {
        dozer_panel_usage::view::list_pane(self, &self.state, Length::Fill, Border::default())
    }
```

在文件里 `receiver_stream` 函数之前加样例数据:

```rust
/// 一条带一行样例数据的 `Loaded`:让面板进入"有数据"态(真实窗口自检与单元测试用)。
/// 代数取 `u64::MAX`,保证不被库当成"被更新的刷新取代"而丢弃。
pub fn sample_loaded_message() -> UsageMessage {
    let meta = dozer_model::ConversationMeta {
        path: PathBuf::from("/selftest/session.jsonl"),
        title: "selftest".to_owned(),
        modified_ms: 1,
        agent: AgentKind::Claude,
    };
    let usage = dozer_panel_usage::aggregate::ConversationUsage { turns: 1, tokens_in: 100, tokens_out: 50, ..Default::default() };
    UsageMessage::Loaded(1, u64::MAX, vec![(meta, usage)], 0, std::collections::BTreeMap::new())
}
```

`PanelDisplay` 实现里,把 `list_collapse_button`、`collapse_hover`、`toggle_list_collapse` 三个函数整体替换为:

```rust
    fn list_collapse_button<'a>(
        &self,
        collapsed: bool,
        tooltip_collapse: &'a str,
        tooltip_expand: &'a str,
        on_select: Msg,
        _on_hover: impl Fn(bool) -> Msg + 'a,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        use byteui::interaction::icons::{self, IconKind};
        use iced::widget::{button, container, text, tooltip};
        // 列表在右侧(默认位置):收起 = 把右边的列表收进去。
        let (icon, tip) = if collapsed { (IconKind::PanelRightOpen, tooltip_expand) } else { (IconKind::PanelRightClose, tooltip_collapse) };
        tooltip(
            button(icons::view(icon, byteui::theme::icon_size::row(), byteui::theme::color::current().dim)).on_press(on_select),
            container(text(tip).size(byteui::theme::font::body())).padding(6).style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn collapse_hover(&self, _hovered: bool) -> Msg {
        // 悬停只是库为按钮高亮预留的接缝;Digger 的按钮自带悬停样式,这里什么都不做。
        Msg::Shell(ShellMsg::Noop)
    }

    fn toggle_list_collapse(&self) -> Msg {
        // 收起状态只存在骨架的 `PaneStore` 里(落盘、可拖拽收起),不在用量自己的状态里重复一份。
        Msg::Shell(ShellMsg::TogglePane(crate::shell::usage_id()))
    }
```

- [ ] **Step 4: 实现 —— `product.rs` 的拦截**

`Msg::Usage(m) => { ... }` 这一支整体替换为:

```rust
            Msg::Usage(m) => {
                // 库的 `update` 对这两条是 `unreachable!`(它们必须由宿主拦截):折叠转给骨架,悬停无事可做。
                match m {
                    dozer_panel_usage::Message::ToggleListCollapse => return self.on_shell(ShellMsg::TogglePane(shell::usage_id())),
                    dozer_panel_usage::Message::CollapseHover(_) => return Task::none(),
                    m => self.usage.update(m),
                }
                // Digger 一期无 Toast:把库的 outbox 提示落地到 stderr,不静默吞掉。
                for (level, text) in self.usage.take_notices() {
                    eprintln!("digger: usage 面板/{level}: {text}"); // cli-output
                }
                Task::none()
            }
```

`usage_view` 暂时改成(Task 4 再换成三态):

```rust
    fn usage_view(&self) -> Element<'_, Msg> {
        container(self.usage.content_view(false)).padding(0).width(Length::Fill).height(Length::Fill).into()
    }
```

- [ ] **Step 5: 运行**

```bash
cargo test -p digger-desktop 2>&1 | grep -E "^(error|warning)|test result|FAILED|panicked"
```

Expected: `test result: ok. 84 passed`(80 + 4:adapter 3 + product 1);没有 warning。

- [ ] **Step 6: Commit**

```bash
git add crates/desktop
git commit -m "feat(desktop): usage content and list views, a collapse button wired to the shell

The old placeholder button sent the library's ToggleListCollapse, which the
library treats as unreachable: it would have panicked. Both intercepted
messages are now handled by the product.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Digger —— `usage_layout()`、body 与原生矩形共用同一判断

**Files:**
- Modify: `crates/desktop/src/product.rs`

**Interfaces:**
- Consumes: Task 2 的 `shell::pair_rects`;Task 3 的 `content_view` / `list_view` / `has_agent_filter` / `sample_loaded_message`;S1 的 `pair_view`、`Divider`。
- Produces:
  - `product::UsageLayout::{ContentOnly, Collapsed, Paired}`
  - `ProductShell::usage_layout(&self) -> UsageLayout`(**唯一**判断)
  - `fn usage_native_rect(layout: UsageLayout, content: Rect, pair: Option<PairRects>) -> Rect`(纯函数)

- [ ] **Step 1: 先写测试(`product.rs` 的 `mod tests` 里,Task 3 那条之后)**

```rust
    fn send(shell: &mut ProductShell, m: Msg) {
        let _ = Product::update(shell, HostMessage::Product(m));
    }

    fn load_sample_rows(shell: &mut ProductShell) {
        send(shell, Msg::Usage(crate::adapters::usage::sample_loaded_message()));
    }

    #[test]
    fn usage_layout_has_three_states() {
        let mut shell = fresh("layout3");
        assert_eq!(shell.usage_layout(), UsageLayout::ContentOnly, "no data yet: nothing to pair");
        load_sample_rows(&mut shell);
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
        send(&mut shell, Msg::Shell(ShellMsg::TogglePane(shell::usage_id())));
        assert_eq!(shell.usage_layout(), UsageLayout::Collapsed);
        send(&mut shell, Msg::Shell(ShellMsg::TogglePane(shell::usage_id())));
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
    }

    /// 评审关注点 3:文件里写着 collapsed 但此刻没有数据 —— 不能显示成一个空列表占位。
    #[test]
    fn collapsed_without_data_is_content_only() {
        let dir = test_temp_dir("collapsed-nodata");
        let path = dir.join("shell.json");
        std::fs::write(&path, r#"{ "panes": { "digger.usage": { "split": 0.35, "collapsed": true } } }"#).unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert_eq!(shell.shell.panes().get(&shell::usage_id()).map(|d| d.collapsed), Some(true));
        assert_eq!(shell.usage_layout(), UsageLayout::ContentOnly);
        load_sample_rows(&mut shell);
        assert_eq!(shell.usage_layout(), UsageLayout::Collapsed, "the stored state shows up as soon as there is data");
    }

    /// 评审关注点 1、2、5:原生图表只占内容栏,且和 body 读同一个 `usage_layout()`。
    #[test]
    fn native_rect_follows_the_layout() {
        use bytehost_chrome::PairRects;
        let content = Rect::new(693.0, 47.0, 448.0, 700.0);
        // 右区默认:内容在左、列表在右
        let paired = PairRects { first: Rect::new(939.0, 47.0, 154.0, 700.0), second: Rect::new(693.0, 47.0, 286.0, 700.0), divider: Rect::new(979.0, 47.0, 8.0, 700.0) };
        assert_eq!(usage_native_rect(UsageLayout::Paired, content, Some(paired)), paired.second, "only the content pane carries the chart");
        // 无数据 / 收起:整个内容区,即便此刻还拿得到一份旧的配对矩形
        assert_eq!(usage_native_rect(UsageLayout::ContentOnly, content, Some(paired)), content);
        assert_eq!(usage_native_rect(UsageLayout::Collapsed, content, Some(paired)), content);
        // 镜像(拖到左栏):列表在左、内容在右 —— 取的仍是 `second`,位置由骨架几何给出
        let mirrored = PairRects { first: Rect::new(693.0, 47.0, 154.0, 700.0), second: Rect::new(855.0, 47.0, 286.0, 700.0), divider: Rect::new(847.0, 47.0, 8.0, 700.0) };
        assert_eq!(usage_native_rect(UsageLayout::Paired, content, Some(mirrored)), mirrored.second);
        // 没有配对矩形(该侧不是用量)时回落到内容区,不 panic
        assert_eq!(usage_native_rect(UsageLayout::Paired, content, None), content);
    }

    #[tokio::test]
    async fn the_usage_view_builds_in_every_layout_and_side() {
        let mut shell = fresh("usage-views");
        let _ = Product::view(&shell); // ContentOnly
        load_sample_rows(&mut shell);
        let _ = Product::view(&shell); // Paired
        send(&mut shell, Msg::Shell(ShellMsg::TogglePane(shell::usage_id())));
        let _ = Product::view(&shell); // Collapsed
        send(&mut shell, Msg::Shell(ShellMsg::TogglePane(shell::usage_id())));
        // 拖到左栏(镜像)与放大态
        send(&mut shell, Msg::Shell(ShellMsg::PanelPress { panel: shell::apps_id() }));
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        send(&mut shell, Msg::Shell(ShellMsg::PanelPress { panel: shell::usage_id() }));
        send(&mut shell, Msg::Shell(ShellMsg::RailDragMove { side: Side::Left, index: 0 }));
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert_eq!(shell.shell.side_of(&shell::usage_id()), Some(Side::Left));
        let _ = Product::view(&shell);
        send(&mut shell, Msg::Shell(ShellMsg::Maximize(Side::Left)));
        let _ = Product::view(&shell);
    }
```

- [ ] **Step 2: 运行,确认失败**

```bash
cargo test -p digger-desktop usage_layout 2>&1 | grep -E "^error|cannot find|test result" | head
```

Expected: 编译失败(`UsageLayout` / `usage_native_rect` 未定义)。

- [ ] **Step 3: 实现**

`product.rs` 的 `use bytehost_chrome::{...}` 行改为:

```rust
use bytehost_chrome::{BodyArea, Divider, El, Loaded, PairRects, PanelMeta, PanelRules, Rect, ShellHost, ShellModel, ShellMsg, TopBarSlots, metrics_from_theme, pair_view};
```

在 `below_header` 函数之后加:

```rust
/// 用量面板此刻怎么摆(Dozer `PanelKind::Usage` 分支的三态)。**`body()` 与 `sync_native()` 共用这一个判断**,
/// 谁也不自己再算一遍,免得 body 已经是单栏、图表还停在分栏的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageLayout {
    /// 加载中 / 还没数据:没有列表可拼,内容栏独占全宽。
    ContentOnly,
    /// 有数据、列表被收起:内容栏独占全宽,标题行里留"展开"按钮。
    Collapsed,
    /// 有数据且列表展开:内容 | 列表。
    Paired,
}

/// 用量的原生图表只放在内容栏里:拼分栏时是配对的第二块(镜像与否由骨架几何给出),其余两种状态是整个面板内容区。
fn usage_native_rect(layout: UsageLayout, content: Rect, pair: Option<PairRects>) -> Rect {
    match (layout, pair) {
        (UsageLayout::Paired, Some(p)) => p.second,
        _ => content,
    }
}
```

`impl ProductShell` 里(`panel_visible` 之后)加:

```rust
    /// 用量面板此刻的三态(唯一判断,见 [`UsageLayout`])。
    pub(crate) fn usage_layout(&self) -> UsageLayout {
        if !self.usage.has_agent_filter() {
            return UsageLayout::ContentOnly;
        }
        if self.shell.panes().get(&shell::usage_id()).is_some_and(|d| d.collapsed) {
            UsageLayout::Collapsed
        } else {
            UsageLayout::Paired
        }
    }
```

`sync_native` 里,把

```rust
        let usage_rect = content_of(&shell::usage_id());
```

改为:

```rust
        let usage_rect = content_of(&shell::usage_id()).map(|content| {
            let pair = self.shell.side_of(&shell::usage_id()).and_then(|side| shell::pair_rects(&self.shell, side));
            usage_native_rect(self.usage_layout(), content, pair)
        });
```

把 `usage_view` 整个替换为:

```rust
    fn usage_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let tokens = color::current();
        let body: Element<'_, Msg> = match self.usage_layout() {
            UsageLayout::ContentOnly => self.usage.content_view(self.pane_collapsed(&shell::usage_id())),
            UsageLayout::Collapsed => self.usage.content_view(true),
            // 列表是配对的第一块;"内容在前还是列表在前"由骨架按镜像算好,`pair_view` 直接读。
            UsageLayout::Paired => pair_view(area, self.usage.list_view(), self.usage.content_view(false), Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))), tokens.bg, tokens.panel),
        };
        container(body).padding(0).width(Length::Fill).height(Length::Fill).into()
    }

    /// 配对面板的第一块此刻是否被收起(骨架的 `PaneStore` 记账)。
    fn pane_collapsed(&self, panel: &PanelId) -> bool {
        self.shell.panes().get(panel).is_some_and(|d| d.collapsed)
    }
```

(`usage_layout` 里的收起判断改为调用 `self.pane_collapsed(&shell::usage_id())`,保持一处。)

`ShellHost::body` 里,把

```rust
    fn body<'a>(&'a self, panel: &PanelId, _area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(),
```

改为:

```rust
    fn body<'a>(&'a self, panel: &PanelId, area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(area),
```

- [ ] **Step 4: 运行**

```bash
cargo test -p digger-desktop 2>&1 | grep -E "^(error|warning)|test result|FAILED|panicked"
```

Expected: `test result: ok. 88 passed`(84 + 4);没有 warning。若 `the_usage_view_builds…` 里 `RailDragMove` 之后用量没到左栏,先看 S1 `shell.rs`/模型的拖拽语义(`press` 已激活图标会先收起该区):测试里先 `press(apps)` 再 `press(usage)` 就是为了避开这点。

- [ ] **Step 5: Commit**

```bash
git add crates/desktop/src/product.rs
git commit -m "feat(desktop): one usage_layout() decides the usage body and the chart rectangle

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 真实窗口自检 —— `usage` 场景

**Files:**
- Modify: `crates/desktop/src/adapters/usage_native.rs`(探针)
- Modify: `crates/desktop/src/product.rs`(自检读取用的访问器)
- Modify: `crates/desktop/src/lifecycle_selftest.rs`(场景化)
- Modify: `scripts/run-editor-selftest.sh`(加 `usage` 模式)

**Interfaces:**
- Consumes: Task 4 的行为;S1 的 `Driver` / `Phase`。
- Produces:
  - `usage_native::{Probe, Event::Probed(Probe)}`、`Native::{probe(id) -> Task<Event>, last_probe() -> Option<Probe>, is_ready()}`
  - `ProductShell::{usage_ready, usage_last_probe, usage_probe}`
  - `lifecycle_selftest::Scenario::{Shell, Usage}`;环境变量 `DIGGER_LIFECYCLE_SELFTEST=usage` 选用量场景(其它取值 = S1 的场景)
  - 用量场景 7 步(U1–U7),期望值手算,全部基于 **窗口 1200×800、左区偏好 640** 的字面量

手算期望(窗口 1200×800,默认布局,左区偏好 640):
- zones = 1200 − 2·44 − 8 = **1104**。左宽 = clamp(640, 320, max(1104 − 320, 320) = 784) = 640。右区外框宽 = 1104 − 640 = 464。
- 右区内容(外边距 上 6 右 6,内边距 1):x = 44 + 640 + 8 + 1 = **693**,y = 40 + 6 + 1 = **47**,w = 464 − 6 − 2 = **456**,h = (800 − 40 − 22) − 6 − 2 = **730**。
- 用量原生头部 80(`USAGE_HEADER_HEIGHT` 的字面量副本):图表 y = 47 + 80 = **127**,h = 730 − 80 = **650**。
- 配对宽 = 456 − 8 = **448**;默认列表占 0.35 ⇒ 列表 156.8,内容 291.2。
- 左区内容(外边距 上 6 左 6,内边距 1):x = 44 + 6 + 1 = **51**,w = 640 − 6 − 2 = **632**;配对宽 = 624。

各步期望(图表 webview 的 x, w;y 恒为 127(右区)或 127(左区,y 同为 47+80),h 恒为 650):
- U1 无数据(`ContentOnly`),右区:x 693,w 456。
- U2 注入样例数据后拼分栏,右区(内容在左):x 693,w 448·(1−0.35) = 291.2。
- U3 点收起按钮:x 693,w 456。
- U4 再点:回到 x 693,w 291.2。
- U5 拖配对分隔线到 x = 693 + 0.5·448 = 917:内容占 0.5 ⇒ x 693,w 224(列表 224 ≥ 最小宽 198,不会收起)。
- U6 用量拖到左栏(镜像:列表在左、内容在右),列表占 0.5 ⇒ 列表 312,内容 312:x = 51 + 312 + 8 = 371,w 312;y 同上(左区内容 y 也是 47)。
- U7 关闭:webview 销毁。

- [ ] **Step 1: 探针(`usage_native.rs`)**

文件顶部 `use` 之后,`Event` 枚举改为并新增 `Probe`:

```rust
#[derive(Debug, Clone)]
pub enum Event {
    Ready(u64),
    Built(u64, Result<(), String>),
    Done,
    /// 生命周期自检读到的真实 view 状态。
    Probed(Probe),
}

/// 真实用量 webview 此刻的样子(生命周期自检读它:读的是 NSView 本身,不是我们自己的记账)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    pub exists: bool,
    pub visible: bool,
    /// NSView 的实际位置与大小(逻辑像素:x, y, 宽, 高;y 从顶部算)
    pub bounds: Option<(f64, f64, f64, f64)>,
    pub handles: usize,
}

fn probe_view(generation: u64) -> Probe {
    use wry::WebViewExtMacOS;
    let handles = VIEW.with(|v| usize::from(v.borrow().is_some()));
    VIEW.with(|v| match v.borrow().as_ref() {
        Some((g, view)) if *g == generation => {
            // wry 的 `bounds()` 读取时不处理翻转坐标系的父视图,读出来的 y 是错的:直接读 NSView 的 frame 自己换算。
            let ns = view.webview();
            let frame = ns.frame();
            // SAFETY: 在窗口线程上读一个仍在句柄表里的、存活的 NSView 及其父视图的几何,只读。
            let bounds = unsafe { ns.superview() }.map(|parent| {
                let y = if parent.isFlipped() { frame.origin.y } else { parent.frame().size.height - frame.origin.y - frame.size.height };
                (frame.origin.x, y, frame.size.width, frame.size.height)
            });
            Probe { exists: true, visible: !ns.isHidden(), bounds, handles }
        }
        _ => Probe { exists: false, visible: false, bounds: None, handles },
    })
}
```

`Native` 结构体里加字段 `last_probe: Option<Probe>,`(`new()` 里 `last_probe: None,`),`update` 的 `match` 里在 `_ => {}` 之前加:

```rust
            Event::Probed(probe) => self.last_probe = Some(probe),
```

并加方法(放在 `take_receiver` 之后):

```rust
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn last_probe(&self) -> Option<Probe> {
        self.last_probe
    }

    /// 读真实 webview 的状态(生命周期自检用),结果经 [`Event::Probed`] 回来。
    pub fn probe(&self, id: window::Id) -> Task<Event> {
        let generation = self.epoch.load(Ordering::SeqCst);
        window::run(id, move |_| Event::Probed(probe_view(generation)))
    }
```

(`probe_view` 只在 macOS 上有 `WebViewExtMacOS`;S1 的 markdown 探针同样如此。给 `probe_view` 与 `Probe` 的 macOS 专用部分不另加 cfg——Digger 当前只在 macOS 上编译自检,与 `markdown/host.rs` 一致。)

- [ ] **Step 2: 访问器(`product.rs` 的 "生命周期自检要读的东西" 那个 `impl` 里加)**

```rust
    pub fn usage_ready(&self) -> bool {
        self.usage.native.is_ready()
    }

    pub fn usage_last_probe(&self) -> Option<crate::adapters::usage_native::Probe> {
        self.usage.native.last_probe()
    }

    pub fn usage_probe(&self, id: window::Id) -> Task<Msg> {
        self.usage.native.probe(id).map(Msg::NativeUsage)
    }
```

- [ ] **Step 3: 先写自检的单元测试(`lifecycle_selftest.rs` 的 `mod tests` 里加)**

```rust
    #[test]
    fn the_usage_expectations_are_the_hand_computed_ones() {
        let w = (1200.0, 800.0);
        // 右区内容 x 693 y 47 w 456;图表在 80 的头部之下:y 127 h 650
        assert_eq!(want_usage(UsageWhere::RightFull, w), (693.0, 127.0, 456.0, 650.0));
        // 配对宽 448,默认列表占 0.35 ⇒ 内容 291.2
        let (x, y, cw, h) = want_usage(UsageWhere::RightPaired { list: 0.35 }, w);
        assert_eq!((x, y, h), (693.0, 127.0, 650.0));
        assert!((cw - 291.2).abs() < 1e-9);
        // 镜像:左区内容 x 51 w 632,配对宽 624,列表 0.5 ⇒ 312;内容 x = 51 + 312 + 8
        assert_eq!(want_usage(UsageWhere::LeftPaired { list: 0.5 }, w), (371.0, 127.0, 312.0, 650.0));
    }

    #[test]
    fn the_usage_scenario_has_seven_steps_and_the_last_one_closes_the_window() {
        let steps = steps(Scenario::Usage);
        assert_eq!(steps.len(), 7);
        assert!(steps.iter().all(|s| !s.name.is_empty()));
        assert!(matches!(steps.last().unwrap().actions.as_slice(), [Action::Close]));
        assert_eq!(super::steps(Scenario::Shell).len(), 11, "the S1 scenario is untouched");
    }
```

并把 `mod tests` 里原有的 `the_expected_rectangles…`、`judge_accepts…`、`every_step_has_a_name…` 三个测试按下面 Step 4 的新签名改(`judge` 接收 `Option<Seen>`,`steps()` 接收场景)。

- [ ] **Step 4: 实现自检的场景化(`lifecycle_selftest.rs`)**

`use` 区改为:

```rust
use std::time::{Duration, Instant};

use bytehost_chrome::{Divider, ShellMsg, Side};
use iced::{Size, Task};

use crate::markdown::{BAR_HEIGHT, Probe};
use crate::product::{Msg, ProductShell};
use crate::shell;
```

在 `Expect` 之前加场景与用量期望:

```rust
/// 自检场景:`Shell` 是 S1 的编辑器场景;`Usage` 是 S2 的用量配对场景(`DIGGER_LIFECYCLE_SELFTEST=usage`)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scenario {
    Shell,
    Usage,
}

/// 期望用量图表 webview 在哪(窗口 1200×800、左区偏好 640 的前提下)。`list` 是列表占配对宽度的比例。
#[derive(Clone, Copy, Debug, PartialEq)]
enum UsageWhere {
    /// 右区,整个内容区(无数据 / 收起)
    RightFull,
    /// 右区,内容在左、列表在右
    RightPaired { list: f64 },
    /// 左区(用量被拖到左栏,镜像):列表在左、内容在右
    LeftPaired { list: f64 },
}
```

`Expect` 加变体:

```rust
enum Expect {
    Visible(Area),
    Hidden,
    Destroyed,
    UsageAt(UsageWhere),
}
```

`Action` 加变体:

```rust
enum Action {
    Msg(ShellMsg),
    Resize(f32, f32),
    Close,
    /// 给用量面板注入一行样例数据,让它进入"有数据"态。
    InjectUsage,
}
```

把现有的 `fn steps() -> Vec<Step> { ... }` 改名为 `fn shell_steps() -> Vec<Step> { ... }`(函数体不变),并在其后加:

```rust
fn steps(scenario: Scenario) -> Vec<Step> {
    match scenario {
        Scenario::Shell => shell_steps(),
        Scenario::Usage => usage_steps(),
    }
}

fn usage_steps() -> Vec<Step> {
    let usage = shell::usage_id;
    let apps = shell::apps_id;
    // 配对宽 448,右区内容 x 693:光标放在 693 + 0.5·448 = 917,内容占一半 ⇒ 列表占 0.5
    let drag_to_half = vec![Action::Msg(ShellMsg::DividerPress(Divider::Pair(Side::Right))), Action::Msg(ShellMsg::CursorMoved { x: 917.0, y: 300.0 }), release()];
    vec![
        Step { name: "U1 with no data the chart fills the whole usage content area", actions: vec![Action::Resize(1200.0, 800.0)], expect: Expect::UsageAt(UsageWhere::RightFull) },
        Step { name: "U2 once there is data the zone becomes content | list and the chart takes the content pane", actions: vec![Action::InjectUsage], expect: Expect::UsageAt(UsageWhere::RightPaired { list: 0.35 }) },
        Step { name: "U3 collapsing the list gives the chart the whole width", actions: vec![Action::Msg(ShellMsg::TogglePane(usage()))], expect: Expect::UsageAt(UsageWhere::RightFull) },
        Step { name: "U4 expanding it again brings back the same split", actions: vec![Action::Msg(ShellMsg::TogglePane(usage()))], expect: Expect::UsageAt(UsageWhere::RightPaired { list: 0.35 }) },
        Step { name: "U5 dragging the pair divider resizes the chart", actions: drag_to_half, expect: Expect::UsageAt(UsageWhere::RightPaired { list: 0.5 }) },
        Step {
            // 先选中应用再选中用量(点已激活的图标会收起该区),再拖到左栏;切入会重新取数(空),所以最后再注入数据
            name: "U6 dragging usage to the left rail mirrors the pair: list left, chart on the right",
            actions: vec![press(apps()), release(), press(usage()), Action::Msg(ShellMsg::RailDragMove { side: Side::Left, index: 0 }), release(), Action::InjectUsage],
            expect: Expect::UsageAt(UsageWhere::LeftPaired { list: 0.5 }),
        },
        Step { name: "U7 closing destroys the chart view", actions: vec![Action::Close], expect: Expect::Destroyed },
    ]
}
```

`Driver` 结构体加字段 `scenario: Scenario,`;`from_env` 改为:

```rust
    pub fn from_env() -> Option<Self> {
        let value = std::env::var(ENV).ok()?;
        let scenario = if value == "usage" { Scenario::Usage } else { Scenario::Shell };
        Some(Self { step: 0, scenario, steps: steps(scenario), started: Instant::now(), passes: 0, fails: 0 })
    }
```

`advance` 的 `Phase::Probe` 分支改为:

```rust
            Phase::Probe => match shell.window_id() {
                Some(id) => match self.scenario {
                    Scenario::Shell => shell.markdown_probe(id),
                    Scenario::Usage => shell.usage_probe(id),
                }
                .chain(after(SETTLE / 2, Phase::Check)),
                None => self.fail_now("no window"),
            },
```

`act` 里就绪判断与动作映射改为:

```rust
        // 第一步先等页面说过 ready:之后才有「正常显示」可言
        let ready = match self.scenario {
            Scenario::Shell => shell.markdown_ready(),
            Scenario::Usage => shell.usage_ready(),
        };
        if self.step == 0 && !ready {
            return self.wait_or_die("the page never became ready");
        }
```

(把原来的 `if self.step == 0 && !shell.markdown_ready() { return self.wait_or_die("the editor page never became ready"); }` 整段替换为上面。)动作映射的 `match action` 里加一支:

```rust
                Action::InjectUsage => Task::done(Msg::Usage(crate::adapters::usage::sample_loaded_message())),
```

`check` 里取探针的地方改为:

```rust
        let seen = match self.scenario {
            Scenario::Shell => shell.markdown_last_probe().map(Seen::from),
            Scenario::Usage => shell.usage_last_probe().map(Seen::from),
        };
        let (ok, detail) = judge(step.expect, seen, shell.window_size());
```

在 `near` 之前加 `Seen` 与期望函数,并改写 `judge`:

```rust
/// 两个面板的探针统一成这一种,`judge` 不关心是哪个面板。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seen {
    exists: bool,
    visible: bool,
    bounds: Option<(f64, f64, f64, f64)>,
    handles: usize,
}

impl From<Probe> for Seen {
    fn from(p: Probe) -> Self {
        Self { exists: p.exists, visible: p.visible, bounds: p.bounds, handles: p.handles }
    }
}

impl From<crate::adapters::usage_native::Probe> for Seen {
    fn from(p: crate::adapters::usage_native::Probe) -> Self {
        Self { exists: p.exists, visible: p.visible, bounds: p.bounds, handles: p.handles }
    }
}

/// 用量原生头部的高度(`product.rs` 的 `USAGE_HEADER_HEIGHT` 的字面量副本:那个值是估值,校准它时这里要一起改)。
const USAGE_HEADER: f64 = 80.0;

/// 用量图表 webview 的期望矩形 (x, y, w, h)。只用字面量和算术。
fn want_usage(place: UsageWhere, window: (f32, f32)) -> (f64, f64, f64, f64) {
    let (w, h) = (window.0 as f64, window.1 as f64);
    let zones = (w - 2.0 * RAIL - DIVIDER).max(0.0);
    let left_w = 640f64.clamp(MIN_ZONE, (zones - MIN_ZONE).max(MIN_ZONE));
    let body_h = h - TOP_BAR - FOOTBAR;
    let y = TOP_BAR + 6.0 + 1.0 + USAGE_HEADER;
    let ch = body_h - 6.0 - 2.0 - USAGE_HEADER;
    // 右区内容:外边距 上6 右6,内边距 1;左区内容:外边距 上6 左6,内边距 1
    let (rx, rw) = (RAIL + left_w + DIVIDER + 1.0, (zones - left_w) - 6.0 - 2.0);
    let (lx, lw) = (RAIL + 6.0 + 1.0, left_w - 6.0 - 2.0);
    match place {
        UsageWhere::RightFull => (rx, y, rw, ch),
        UsageWhere::RightPaired { list } => (rx, y, (rw - DIVIDER) * (1.0 - list), ch),
        UsageWhere::LeftPaired { list } => {
            let pair_w = lw - DIVIDER;
            (lx + pair_w * list + DIVIDER, y, pair_w * (1.0 - list), ch)
        }
    }
}
```

`judge` 改为:

```rust
fn judge(expect: Expect, probe: Option<Seen>, window: (f32, f32)) -> (bool, String) {
    let Some(p) = probe else {
        return (false, "no probe result arrived".into());
    };
    let (ok, want_detail) = match expect {
        Expect::Visible(area) => {
            let want = want(area, window);
            (p.exists && p.visible && p.handles == 1 && p.bounds.is_some_and(|b| near(b, want)), format!("want bounds {want:?}"))
        }
        Expect::UsageAt(place) => {
            let want = want_usage(place, window);
            (p.exists && p.visible && p.handles == 1 && p.bounds.is_some_and(|b| near(b, want)), format!("want bounds {want:?}"))
        }
        Expect::Hidden => (p.exists && !p.visible && p.handles == 1, "want hidden but alive".to_owned()),
        Expect::Destroyed => (!p.exists && p.handles == 0, "want destroyed, no handles".to_owned()),
    };
    (ok, if ok { String::new() } else { format!("probe {p:?}, {want_detail}") })
}
```

`mod tests` 里的辅助函数 `probe(...)` 改为返回 `Option<Seen>`:

```rust
    fn probe(exists: bool, visible: bool, bounds: Option<(f64, f64, f64, f64)>, handles: usize) -> Option<Seen> {
        Some(Seen { exists, visible, bounds, handles })
    }
```

并把 `every_step_has_a_name_and_the_last_one_closes_the_window` 里的 `steps()` 改为 `steps(Scenario::Shell)`。

- [ ] **Step 5: 脚本加 `usage` 模式(`scripts/run-editor-selftest.sh`)**

脚本开头的说明里,`lifecycle` 那两行之后加一行:

```bash
#   scripts/run-editor-selftest.sh usage        用量配对(S2):无数据 / 拼分栏 / 收起 / 拖分隔线 / 镜像 / 关闭,
#                                               读用量图表 webview 的真实 NSView
```

`case` 里加一支:

```bash
  usage) export DIGGER_LIFECYCLE_SELFTEST=usage ;;
```

并把用法提示 `[harness|lifecycle]` 改为 `[harness|lifecycle|usage]`。

- [ ] **Step 6: 运行单元测试**

```bash
bash -n scripts/run-editor-selftest.sh && echo syntax-ok
cargo test -p digger-desktop 2>&1 | grep -E "^(error|warning)|test result|FAILED|panicked"
```

Expected: `syntax-ok`;`test result: ok. 90 passed`(88 + 2);没有 warning。

- [ ] **Step 7: 跑真实窗口自检**

```bash
bash scripts/run-editor-selftest.sh usage 2>&1 | tail -12
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | tail -3
```

Expected: U1…U7 全部 `PASS`,`SUMMARY shell lifecycle selftest: 7 passed, 0 failed`,`editor selftest: ok`;原有 `lifecycle` 场景仍 `11 passed, 0 failed`。**需要图形会话**(会弹窗,几秒后自己关)。

若 U2 以后失败且探针里图表的宽度是整行(456):说明样例数据被随后到达的真实取数结果(空)盖掉了 —— 取数很快(空账本),但若发生就在 U2 的 `InjectUsage` 之前加一个 `Action::Msg(ShellMsg::Noop)`(多等 250ms),不要改产品代码。

- [ ] **Step 8: 变异检查 —— 自检必须真的抓得住问题**

变异 A(图表矩形取错:拼分栏时仍取整个内容区)。**先提交 Step 1–6 的改动**(`git add crates/desktop scripts && git commit -m "wip: usage selftest scenario"`,最后一步再 `git commit --amend` 合并或保留为两个提交都行),变异用备份还原:

```bash
cp crates/desktop/src/product.rs /tmp/product.rs.bak
python3 - <<'PYEOF'
p='crates/desktop/src/product.rs'
s=open(p).read()
assert "(UsageLayout::Paired, Some(p)) => p.second," in s
open(p,'w').write(s.replace("(UsageLayout::Paired, Some(p)) => p.second,","(UsageLayout::Paired, Some(_)) => content,",1))
PYEOF
bash scripts/run-editor-selftest.sh usage 2>&1 | grep -E "^(FAIL|SUMMARY)|selftest:"
cp /tmp/product.rs.bak crates/desktop/src/product.rs
git status --short crates/desktop
```

Expected: U2、U4、U5、U6 失败(拼分栏的步骤),`SUMMARY … 3 passed, 4 failed`,`editor selftest: 失败`。还原后 `git status --short crates/desktop` 为空(备份与已提交内容一致)。

变异 B(头部高度漂 1px):

```bash
sed -i '' 's/const USAGE_HEADER: f64 = 80.0;/const USAGE_HEADER: f64 = 81.0;/' crates/desktop/src/lifecycle_selftest.rs
bash scripts/run-editor-selftest.sh usage 2>&1 | grep -E "^(FAIL|SUMMARY)|selftest:"
sed -i '' 's/const USAGE_HEADER: f64 = 81.0;/const USAGE_HEADER: f64 = 80.0;/' crates/desktop/src/lifecycle_selftest.rs
git status --short crates/desktop
```

Expected: U1–U6 失败(1px 漂移被 0.51 的容差抓住),U7 通过,`SUMMARY … 1 passed, 6 failed`。还原后 `git status` 为空。

- [ ] **Step 9: 还原后确认回到全绿**

```bash
bash scripts/run-editor-selftest.sh usage 2>&1 | tail -2
```

Expected: `SUMMARY shell lifecycle selftest: 7 passed, 0 failed` 和 `editor selftest: ok`。

- [ ] **Step 10: Commit(若 Step 8 已提交过 wip,就用 `git commit --amend` 改成下面这条信息)**

```bash
git add crates/desktop scripts/run-editor-selftest.sh
git commit -m "test(desktop): a real-window selftest scenario for the usage pair

Seven steps read the usage chart's NSView against hand-computed rectangles:
no data, paired, collapsed, expanded, divider drag, mirrored on the left rail,
closed. DIGGER_LIFECYCLE_SELFTEST=usage selects it; the S1 scenario is untouched.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 文档、整体验证与真机验收清单

**Files:**
- Modify: `docs/superpowers/specs/2026-10-15-usage-pair-design.md`(状态行 + 与实现的两处出入)
- Modify: `README.md`(文档清单加一行)

- [ ] **Step 1: spec 的状态行与两处出入**

`docs/superpowers/specs/2026-10-15-usage-pair-design.md` 里:

- `- 状态:待评审` 改为 `- 状态:已实现(待真机验收,见计划 Task 6)`。
- §6 的表格下面加一句:`收起时配对矩形的第二块恰好就是整个内容区,所以实现里只有"拼分栏"这一种状态取配对的第二块,其余两种直接取内容区(行为与表格一致)。`
- §10 "真实窗口自检" 一节里,`S12–S15` 的描述改为:`用量配对用独立的场景 `DIGGER_LIFECYCLE_SELFTEST=usage`(U1–U7),不并入 S1 的 11 步:S1 的场景结束时编辑器已被拖到右栏、布局不再是默认布局,接着测用量要先复位,不如分开。`

- [ ] **Step 2: README 的文档清单里加一行**

在 `README.md` 里 S1 那一行之后加:

```markdown
- [S2 用量面板接入"列表 | 内容"配对](docs/superpowers/specs/2026-10-15-usage-pair-design.md)
```

- [ ] **Step 3: 整体验证**

```bash
cd $ROOT/digger
cargo test -p digger-desktop -p digger-app-host -p digger-store -p digger-editor --lib --bins 2>&1 | grep -E "test result|FAILED|error"
bash scripts/check-host-deps.sh 2>&1 | tail -1
BYTEUI=$ROOT/byteui-shell bash scripts/check-shell-regions.sh
bash scripts/build-editor-web.sh --check 2>&1 | tail -1
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | tail -2
bash scripts/run-editor-selftest.sh usage 2>&1 | tail -2
(cd $ROOT/bytehost-chrome && cargo test -p bytehost-chrome 2>&1 | grep -E "test result|FAILED")
git -C $ROOT/bytehost status --short | wc -l        # 仍是 Task 0 记下的 N
git -C $ROOT/byteui-shell status --short            # 为空
git status --short                                   # 只剩 .dozer/ 与 spike/corpus/
grep -n 'dozer-panel-usage = ' Cargo.toml            # rev 仍是 67ab0e657d75135b882ace2f3aa7a56617776c60
```

Expected: 所有套件无 `FAILED`(`digger-desktop` 90、`bytehost-chrome` 69);门禁、一致性、网页包检查都打印 `ok`;两个自检场景各自 `ok`;`dozer-panel-usage` 的 rev 未变。

- [ ] **Step 4: Commit**

```bash
git add docs README.md
git commit -m "docs: record the S2 usage pair in the spec status and the README

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: 真机验收清单(交给用户,执行者不要替用户勾选)**

这几项在没有显示器 / 截图的验证环境里做不了:

1. **有数据时的外观**:`cargo run -p digger-desktop` 后让用量有数据(跑一个应用任务产生用量记录,或等 Project 接入后的真实数据),右区应是"统计内容 | agent 列表",列表占约 35%。
2. **头部高度**:图表 webview 应紧贴「用量」标题行下沿。`product.rs` 的 `USAGE_HEADER_HEIGHT = 80.0` 是估值;不对就同时改它和 `lifecycle_selftest.rs` 里的 `USAGE_HEADER`(两处必须一致,否则 `usage` 场景会失败)。
3. **分隔线与收起按钮**:拖配对分隔线改比例;拖到列表窄于约 198px 时列表收起、图表变宽;标题行末尾的按钮能收起 / 展开,图标与提示文字随状态变。
4. **持久化**:改了比例或收起后退出再启动,状态还在;把 `<config>/shell.json` 里 `panes."digger.usage"` 改成 `{"split": 0.0}` 或类型错的值,启动不崩溃、回到默认。
5. **拖到左栏**:把用量图标拖到左栏,应变成"列表 | 内容"。
6. **无数据 / 加载中**:没有用量记录时只有内容栏,没有空的列表占位。
7. **图表渲染**:图表本身画得对不对(自检只管位置)。
8. **harness 自检**(亮屏):`bash scripts/run-editor-selftest.sh` 仍 `24 passed, 0 failed`。

---

## Self-Review(写完计划后的核对)

- **Spec 覆盖**:§3 配对规格 → Task 2;§4 `TogglePane` → Task 1;§5 唯一判断 → Task 4;§6 原生矩形 → Task 4 + Task 5;§7 展示适配 → Task 3;§8 持久化 → 无新代码,由 Task 1 的 `TogglePane` 返回 `persist: true` 与 S1 的 `persist` 承担,Task 4 `collapsed_without_data_is_content_only` 覆盖读盘;§9 不做 → Global Constraints;§10 验证 → Task 1–5 的测试与 Task 5 的自检、Task 6 的验收清单;§11 风险 → Review Focus 与对应测试。
- **与 spec 有意不同的两处**(已在 Task 6 Step 1 同步回 spec):(a) 自检用独立场景 U1–U7,不并入 S1 的 11 步(S1 结束时布局已不是默认);(b) 实现里"收起"与"无数据"不再各取一种矩形,只有"拼分栏"取配对的第二块,其余取内容区 —— 收起时二者数值相同。
- **spec 的一个未验证假设已验证**:自检注入数据可行 —— 库的 `Message::Loaded(.., generation, rows, ..)` 公开,代数用 `u64::MAX` 不会被丢弃(Task 3 `sample_loaded_message`)。
- **发现的既有缺陷**:占位按钮发的 `ToggleListCollapse` 在库里是 `unreachable!`,按一下就会崩。Task 3 修了,并有测试。
- **占位符**:没有 TBD。Task 5 Step 7 里"若 U2 以后失败"是一段针对已知时序风险的处置说明,不是遗留项。
- **类型一致性**:`UsageLayout` / `usage_layout()` / `usage_native_rect` / `pane_collapsed` 在 Task 4 内部定义并使用;`Probe`(usage)、`Seen`、`Scenario`、`UsageWhere`、`want_usage`、`Action::InjectUsage` 在 Task 5 内部定义并使用;`sample_loaded_message`、`content_view`、`list_view`、`has_agent_filter` 在 Task 3 定义、Task 4/5 使用;`shell::pair_rects`、`list_min_width` 在 Task 2 定义、Task 4 使用;`ShellMsg::TogglePane` 在 Task 1 定义、Task 3/4/5 使用。
