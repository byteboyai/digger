# S1 窗口骨架 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Dozer 的窗口结构(顶栏 + 左右图标栏 + 左右面板区 + 分隔线 + 配对视图 + 放大态 + 底栏 + 布局持久化)抽成 bytehost 的共享 crate `bytehost-chrome`,让 Digger 先接入,现有四个面板(信息 / 应用 / 用量 / 编辑器)装进去。

**Architecture:** 三个仓库各改一处。`byteui` 新增 `theme::region`(外壳区域样式,数值与 Dozer 的 `workspace.json` 逐项一致);`bytehost` 新增 crate `bytehost-chrome`(以 `PanelId` 为键的 `ShellModel` 状态机 + 纯几何 + iced 控件 + macOS 窗口装饰,产品实现 `ShellHost` 即可接入);`digger` 删掉标签页壳,改由 `ShellModel` 驱动,原生 webview 的矩形和可见性统一来自骨架的内容矩形。

**Tech Stack:** Rust 2024、iced 0.14(`iced_widget` / `iced_renderer`,不开渲染器 feature)、wry 0.55.1、byteui、bytehost-core / bytehost-desktop(`PaneStore`、`apply_pane_split_drag`)、objc2 0.6 / objc2-app-kit 0.3(macOS)。

**Spec:** `docs/superpowers/specs/2026-10-14-window-shell-design.md`(含 §8 修订记录)。执行者两份都要读。

## 已在临时副本里验证过什么

本计划里的代码**不是凭空写的**:先在一个临时的三仓库副本里(bytehost 取 `v0.2.0` 内容 + 新 crate、byteui 加区域样式、Digger 接入)完整实现并跑过:

- `bytehost-chrome`:63 个单元测试通过;byteui 新模块:4 个测试通过,byteui 基线 79 + 4 = 83;
- Digger:`digger-desktop` 76 个测试通过,`digger-editor` / `digger-app-host` / `digger-store` 不受影响,依赖门禁通过;
- **真实窗口**里骨架版生命周期自检 11/11 通过(期望值是独立手算的),并做了两次故意破坏(隐藏逻辑失效、几何偏 1px),对应步骤都失败,还原后恢复全绿;
- 区域样式一致性脚本对着 Dozer 真实的 `workspace.json` 通过,故意改一个数值后失败。

**没有验证到的(需要执行者或用户在真机上做)**:① harness 自检(依赖页面动画帧,临时环境里显示器不可用,旧二进制也卡在同一处,是环境问题);② 与 Dozer 的**视觉对照**(截图在临时环境里也不可用);③ 应用 / 用量面板头部高度常量(`APPS_HEADER_HEIGHT` / `USAGE_HEADER_HEIGHT`)的目测校准;④ 顶栏双击缩放是否与系统原生双击重复触发。这几项在 Task 15 的"真机验收"里逐条列出。

## Global Constraints

- **仓库与路径**(兄弟目录,下文 `$ROOT` = `/Users/chrischiang/Projects/CoralProjects/byteboy`):`$ROOT/digger`、`$ROOT/bytehost`、`$ROOT/byteui`;本计划新建两个 worktree:`$ROOT/bytehost-chrome`(分支 `feat/chrome`,从 tag `v0.2.0` 切出)、`$ROOT/byteui-shell`(分支 `feat/shell-regions`,从 `main` 切出)。
- **`$ROOT/bytehost` 主工作区有他人未提交的 i18n 改动(十几个文件)**:不得修改、暂存、stash、提交;不得让 Digger 依赖它。bytehost 的一切改动只在 `$ROOT/bytehost-chrome` worktree 里做。每个涉及 bytehost 的任务结束时 `git -C $ROOT/bytehost status --short | wc -l` 必须与 Task 0 记下的数相同。
- **bytehost、byteui 只做本地提交**,不推送、不打 tag、不合并到 `main`;这些由用户决定(Task 16)。**Dozer 仓库在 S1 里不动。**
- 版本:iced `0.14`(`default-features = false`,不绑渲染器 feature)、wry `0.55.1`、Rust edition `2024`。`bytehost-chrome` 的控件签名一律 `Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>`(与 byteui 一致)。
- 骨架以 `PanelId` 为键,不出现任何具体面板;面板稳定 id 是落盘的线上名字,不得改:`digger.markdown` / `digger.info` / `digger.apps` / `digger.usage`。
- 业务 crate(`digger-editor`、`digger-store`)不得依赖 `bytehost-*`、`iced*`、`wry`(`scripts/check-host-deps.sh` 把关),`bytehost-chrome` 只允许 `digger-desktop` 依赖。
- ByteBoy2077 主题数值(几何测试用的字面量):图标栏宽 44、分隔线宽 8、最小区宽 320、顶栏高 40、底栏高 22、放大浮层内边距 40、最小分割比例 0.2 / 最大 0.8;左区外边距 `[上6, 右0, 下0, 左6]`、右区外边距 `[上6, 右6, 下0, 左0]`、两区内边距各 1。
- 提交信息用 Conventional Commits(英文),结尾加 `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`。
- 运行 `digger-desktop` 的测试用 debug 模式(`cargo test -p digger-desktop`):release 模式下 `iced_renderer` 会因没开渲染器 feature 而 `compile_error!`,这是既有现象,与本计划无关。
- 磁盘:一份完整的 Digger 构建约数 GB。**不要为每个仓库各开一份独立的 `CARGO_TARGET_DIR`**;`bytehost-chrome` worktree 和 `byteui-shell` worktree 各用自己默认的 `target`,Digger 用它自己的。

## Review Focus

这五类输入 spec 没细说、但最可能伤到真实使用者;每一条都落到了具体任务的具体测试里:

1. **鼠标释放事件没送达**(拖着图标 / 分隔线时窗口失去焦点):拖拽状态会一直悬着,之后光标划过图标栏就把图标误重排。期望:失去焦点等同松手。→ Task 9 `events_map_to_shell_messages`。
2. **手改 / 损坏的 `shell.json`**:顶层不是对象(`[1,2]` 会被 serde 当"位置序列"解析成功)、尺寸为 0 或 NaN、分割比例 0 或 1(配对里一块会变成 0 宽)、重复 id、两侧都收起。期望:回落到合法值,绝不 panic、不整份重置用户的其余偏好。→ Task 7 `persist` 全部测试。
3. **别的版本写的 `shell.json`**:新版本新增的面板不能让老布局被整体重置(回到它的默认栏);更高 schema 版本的文件不得被覆盖。→ Task 7、Task 11 的 `shell.rs` 测试。
4. **窗口比两个最小区加起来还窄 / 极小窗口**:右区不能被挤成 0 宽,任何矩形不能出现负数。→ Task 3 `a_narrow_window_clamps…`、`tiny_windows_never_produce_negative_sizes`,Task 6 `resizing_never_rewrites_the_preferred_left_width`。
5. **原生 webview 面板所在区被收起 / 被另一侧放大盖住 / 被拖到另一条栏**:原生视图不听 iced 的绘制顺序,必须真的隐藏 / 跟着走,否则会盖在别的面板上。→ Task 14 的 S2、S4、S9、S10(读真实 NSView)。

---

### Task 0: 隔离的 worktree 与开发期依赖接线

**Files:**
- Create(仓库之外):worktree `$ROOT/bytehost-chrome`、worktree `$ROOT/byteui-shell`
- Modify: `$ROOT/digger/Cargo.toml`(末尾追加 `[patch]`)、`$ROOT/digger/Cargo.lock`

**Interfaces:**
- Produces: Digger 的依赖图里只有**一份** `bytehost-core` 和**一份** byteui,分别来自两个 worktree。后续所有任务都建立在这条之上。

- [ ] **Step 1: 记下 bytehost 主工作区现状(之后每个涉及 bytehost 的任务都要对照)**

```bash
export ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy
cd $ROOT/bytehost && git status --short | wc -l && git rev-parse v0.2.0
```

Expected: 第一行是一个数字,**记作 N**(他人的 i18n 改动文件数);第二行是 `68ac404da340c65b891431fef5640632feae05c9`。

- [ ] **Step 2: 建两个 worktree**

```bash
git -C $ROOT/bytehost worktree add ../bytehost-chrome -b feat/chrome v0.2.0
git -C $ROOT/byteui worktree add ../byteui-shell -b feat/shell-regions main
git -C $ROOT/bytehost status --short | wc -l          # 必须仍是 N
git -C $ROOT/bytehost-chrome log -1 --format=%H       # 68ac404da340c65b891431fef5640632feae05c9
git -C $ROOT/byteui-shell log -1 --format='%h %s'     # 3ff652e fix(form): relax …(0.4.2)
```

Expected: 主工作区文件数不变;两个 worktree 都建成。

- [ ] **Step 3: 在 Digger 根 `Cargo.toml` 末尾追加开发期 `[patch]`**

`$ROOT/digger/Cargo.toml` 末尾追加:

```toml

# ---- 开发期临时配置(S1 最后一个任务移除)----
# 把 bytehost 与 byteui 的 git 来源指向本地 worktree,使依赖图里只有一份 bytehost-core / byteui:
# 两份会让 `PanelId` 类型不兼容,也会让 byteui 的全局主题状态分裂(用量面板的主题会错)。
[patch."https://github.com/byteboyai/bytehost"]
bytehost-core = { path = "../bytehost-chrome/crates/bytehost-core" }
bytehost-desktop = { path = "../bytehost-chrome/crates/bytehost-desktop" }
bytehost-webview = { path = "../bytehost-chrome/crates/bytehost-webview" }
bytehost-apps = { path = "../bytehost-chrome/crates/bytehost-apps" }
bytehost-client = { path = "../bytehost-chrome/crates/bytehost-client" }
bytehost-panel = { path = "../bytehost-chrome/crates/bytehost-panel" }

[patch."https://github.com/byteboyai/byteui"]
byteui = { path = "../byteui-shell" }
```

- [ ] **Step 4: 让锁文件改用 worktree 里的 byteui,并确认依赖图只有一份**

```bash
cd $ROOT/digger
cargo update -p byteui
cargo tree -p digger-desktop -d | grep -E "^(bytehost-core|bytehost-desktop|byteui)" || echo "no duplicates"
```

Expected: `cargo update` 打印 `Adding byteui v0.4.2 (…/byteui-shell)` 和 `Removing byteui v0.4.1 (…)`;第二条命令打印 `no duplicates`。

- [ ] **Step 5: 确认 Digger 原有测试在新依赖下不变**

```bash
cargo test -p digger-desktop -p digger-app-host -p digger-editor --lib --bins 2>&1 | grep -E "test result|FAILED|error"
```

Expected: 全部 `ok`,没有 `FAILED`。(`v0.2.0` 的 worktree 与 Digger 当前依赖的 tag 是同一份代码,所以不应有任何接口漂移。)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): dev-time [patch] to the bytehost and byteui worktrees for the S1 window shell

Temporary: removed in the last S1 task, once the new tags exist.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 1: byteui —— 外壳区域样式 `theme::region`

**Files:**(都在 `$ROOT/byteui-shell`,分支 `feat/shell-regions`)
- Create: `src/theme/region.rs`
- Create: `examples/dump_shell_regions.rs`
- Modify: `src/theme/mod.rs`、`Cargo.toml`(版本 `0.4.3`)、`CHANGELOG.md`

**Interfaces:**
- Produces(`bytehost-chrome` 的控件和度量依赖这些,签名不得改):
  - `byteui::theme::region::RegionStyle { background: Option<Color>, border: Option<Border>, padding: Padding, gap: f32, margin: Padding }`
  - `byteui::theme::region::MaximizeOverlayStyle { scrim_background: Color, scrim_padding: f32, border: Border }`
  - `pub fn top_bar() / left_icon_rail() / right_icon_rail() / status_bar() / left_zone() / right_zone() -> RegionStyle`
  - `pub fn maximize_overlay() -> MaximizeOverlayStyle`、`pub fn window_background() -> Color`
  - `#[doc(hidden)] pub fn spec_dump() -> serde_json::Value`(跨仓库一致性检查用)
- 设计要点:解析函数 `resolve` / `resolve_overlay` 显式接收配色和缩放,公开接口只是用当前全局值去调它们 —— 测试因此不碰进程级全局,不会与别的模块的主题测试互相干扰。

- [ ] **Step 1: 先写测试(文件里 `#[cfg(test)] mod tests` 那一段)**

在 `src/theme/region.rs` 里先只放下面完整文件的 `mod tests` 部分和它需要的 `use`,运行(Step 3 的命令)确认因类型未定义而编译失败。

- [ ] **Step 2: 写完整实现**

`src/theme/region.rs`:

```rust
//! 外壳区域样式:顶栏、左右图标栏、左右面板区、放大浮层、底栏。
//!
//! 数值与 Dozer `crates/dozer-app/assets/theme/workspace.json` 的同名区域**逐项相同**(那份 JSON 仍是 Dozer 自己的
//! 数据源,Dozer 迁移到本模块之前两处并存;`examples/dump_shell_regions.rs` 导出同结构的规格,供跨仓库一致性检查比对)。
//! 颜色令牌每次现读 `color::current()`(运行时可切配色),`padding` / `margin` / `gap` / 放大浮层尺寸乘全局
//! `icon_size::scale()`。
//!
//! 解析函数显式接收配色和缩放,公开接口只是用当前全局值去调它们——测试因此不碰进程级全局,不会与别的模块的
//! 主题测试互相干扰。
use iced_widget::core::{Border, Color, Padding};
use serde_json::{Value, json};

use super::{color, icon_size};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionStyle {
    pub background: Option<Color>,
    pub border: Option<Border>,
    pub padding: Padding,
    pub gap: f32,
    /// 区域整体相对外围容器的外边距(悬浮留白)。渲染时套一层外容器做 inset,几何侧(原生子视图的矩形)要同步扣减。
    pub margin: Padding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaximizeOverlayStyle {
    pub scrim_background: Color,
    pub scrim_padding: f32,
    pub border: Border,
}

#[derive(Clone, Copy)]
enum Token {
    Bg,
    Panel,
    Border,
    Gold,
    Scrim,
    TabActiveBorder,
}

impl Token {
    fn color(self, c: &color::ColorTokens) -> Color {
        match self {
            Token::Bg => c.bg,
            Token::Panel => c.panel,
            Token::Border => c.border,
            Token::Gold => c.gold,
            Token::Scrim => c.scrim,
            Token::TabActiveBorder => c.tab_active_border,
        }
    }

    /// Dozer `workspace.json` 里的令牌名。
    fn name(self) -> &'static str {
        match self {
            Token::Bg => "BG",
            Token::Panel => "PANEL",
            Token::Border => "BORDER",
            Token::Gold => "GOLD",
            Token::Scrim => "SCRIM",
            Token::TabActiveBorder => "TAB_ACTIVE_BORDER",
        }
    }
}

struct Spec {
    background: Option<Token>,
    /// (颜色, 线宽, 圆角)
    border: Option<(Token, f32, f32)>,
    /// [top, right, bottom, left]
    padding: [f32; 4],
    gap: f32,
    margin: [f32; 4],
}

const ZERO: [f32; 4] = [0.0; 4];

const TOP_BAR: Spec = Spec { background: Some(Token::Bg), border: Some((Token::Border, 1.0, 0.0)), padding: [0.0, 10.0, 0.0, 78.0], gap: 8.0, margin: ZERO };
const LEFT_ICON_RAIL: Spec = Spec { background: Some(Token::Panel), border: None, padding: [16.0, 6.0, 0.0, 6.0], gap: 12.0, margin: ZERO };
const RIGHT_ICON_RAIL: Spec = Spec { background: Some(Token::Panel), border: None, padding: [16.0, 6.0, 0.0, 6.0], gap: 12.0, margin: ZERO };
const STATUS_BAR: Spec = Spec { background: Some(Token::Panel), border: Some((Token::Border, 1.0, 0.0)), padding: [0.0, 8.0, 0.0, 8.0], gap: 0.0, margin: ZERO };
const LEFT_ZONE: Spec = Spec { background: Some(Token::Bg), border: Some((Token::Border, 0.0, 8.0)), padding: [1.0; 4], gap: 0.0, margin: [6.0, 0.0, 0.0, 6.0] };
const RIGHT_ZONE: Spec = Spec { background: Some(Token::Bg), border: Some((Token::Border, 0.0, 8.0)), padding: [1.0; 4], gap: 0.0, margin: [6.0, 6.0, 0.0, 0.0] };

/// 放大浮层:遮罩背景令牌、遮罩内边距、描边(颜色令牌, 线宽, 圆角)。
const MAXIMIZE_SCRIM: Token = Token::Scrim;
const MAXIMIZE_SCRIM_PADDING: f32 = 40.0;
const MAXIMIZE_BORDER: (Token, f32, f32) = (Token::Gold, 1.5, 10.0);

fn scaled(p: [f32; 4], s: f32) -> Padding {
    Padding { top: p[0] * s, right: p[1] * s, bottom: p[2] * s, left: p[3] * s }
}

fn resolve(spec: &Spec, c: &color::ColorTokens, s: f32) -> RegionStyle {
    RegionStyle {
        background: spec.background.map(|t| t.color(c)),
        border: spec.border.map(|(t, width, radius)| Border { color: t.color(c), width, radius: radius.into() }),
        padding: scaled(spec.padding, s),
        gap: spec.gap * s,
        margin: scaled(spec.margin, s),
    }
}

fn resolve_overlay(c: &color::ColorTokens, s: f32) -> MaximizeOverlayStyle {
    let (t, width, radius) = MAXIMIZE_BORDER;
    MaximizeOverlayStyle {
        scrim_background: MAXIMIZE_SCRIM.color(c),
        scrim_padding: MAXIMIZE_SCRIM_PADDING * s,
        border: Border { color: t.color(c), width: width * s, radius: (radius * s).into() },
    }
}

pub fn top_bar() -> RegionStyle {
    resolve(&TOP_BAR, &color::current(), icon_size::scale())
}
pub fn left_icon_rail() -> RegionStyle {
    resolve(&LEFT_ICON_RAIL, &color::current(), icon_size::scale())
}
pub fn right_icon_rail() -> RegionStyle {
    resolve(&RIGHT_ICON_RAIL, &color::current(), icon_size::scale())
}
pub fn status_bar() -> RegionStyle {
    resolve(&STATUS_BAR, &color::current(), icon_size::scale())
}
/// 左面板区整体外框:把"左1左2两栏"框成一个视觉整体,不是某一栏自己的边框。
pub fn left_zone() -> RegionStyle {
    resolve(&LEFT_ZONE, &color::current(), icon_size::scale())
}
/// 右面板区整体外框,同 `left_zone`。
pub fn right_zone() -> RegionStyle {
    resolve(&RIGHT_ZONE, &color::current(), icon_size::scale())
}
pub fn maximize_overlay() -> MaximizeOverlayStyle {
    resolve_overlay(&color::current(), icon_size::scale())
}
/// 窗口根容器底色 / 渲染清屏色(Dozer `workspace.json` 的顶层 `background`,令牌 `TAB_ACTIVE_BORDER`)。
pub fn window_background() -> Color {
    Token::TabActiveBorder.color(&color::current())
}

fn spec_json(spec: &Spec) -> Value {
    json!({
        "background": spec.background.map(Token::name),
        "border": spec.border.map(|(t, width, radius)| json!({ "color": t.name(), "width": width, "radius": radius })),
        "padding": spec.padding,
        "gap": spec.gap,
        "margin": spec.margin,
    })
}

/// 7 个外壳区域的原始规格(令牌名 + 未缩放的数值),结构与 Dozer `workspace.json` 的 `regions` 对应项一致
/// (`padding` / `margin` 一律展开为 `[top, right, bottom, left]`)。仅供跨仓库一致性检查使用。
#[doc(hidden)]
pub fn spec_dump() -> Value {
    let (t, width, radius) = MAXIMIZE_BORDER;
    json!({
        "top_bar": spec_json(&TOP_BAR),
        "left_icon_rail": spec_json(&LEFT_ICON_RAIL),
        "right_icon_rail": spec_json(&RIGHT_ICON_RAIL),
        "status_bar": spec_json(&STATUS_BAR),
        "left_zone": spec_json(&LEFT_ZONE),
        "right_zone": spec_json(&RIGHT_ZONE),
        "maximize_overlay": {
            "scrim": { "background": MAXIMIZE_SCRIM.name(), "padding": MAXIMIZE_SCRIM_PADDING },
            "border": { "color": t.name(), "width": width, "radius": radius },
        },
        "background": Token::TabActiveBorder.name(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> color::ColorTokens {
        color::ColorTokens::byteboy2077()
    }

    fn pad(t: f32, r: f32, b: f32, l: f32) -> Padding {
        Padding { top: t, right: r, bottom: b, left: l }
    }

    /// 防漂移锚:数值必须与 Dozer `workspace.json` 的同名区域一致(跨仓库一致性检查是第二道闸)。
    #[test]
    fn the_regions_match_dozers_workspace_json_literals() {
        let c = tokens();
        let top = resolve(&TOP_BAR, &c, 1.0);
        assert_eq!(top.background, Some(c.bg));
        assert_eq!(top.border, Some(Border { color: c.border, width: 1.0, radius: 0.0.into() }));
        assert_eq!(top.padding, pad(0.0, 10.0, 0.0, 78.0), "left 78 leaves room for the traffic lights");
        assert_eq!(top.gap, 8.0);

        for rail in [&LEFT_ICON_RAIL, &RIGHT_ICON_RAIL] {
            let r = resolve(rail, &c, 1.0);
            assert_eq!(r.background, Some(c.panel));
            assert_eq!(r.border, None);
            assert_eq!(r.padding, pad(16.0, 6.0, 0.0, 6.0));
            assert_eq!(r.gap, 12.0);
        }

        let status = resolve(&STATUS_BAR, &c, 1.0);
        assert_eq!(status.background, Some(c.panel));
        assert_eq!(status.padding, pad(0.0, 8.0, 0.0, 8.0));

        let left = resolve(&LEFT_ZONE, &c, 1.0);
        assert_eq!(left.margin, pad(6.0, 0.0, 0.0, 6.0));
        assert_eq!(left.padding, Padding::from(1.0));
        assert_eq!(left.border, Some(Border { color: c.border, width: 0.0, radius: 8.0.into() }));
        let right = resolve(&RIGHT_ZONE, &c, 1.0);
        assert_eq!(right.margin, pad(6.0, 6.0, 0.0, 0.0));
        assert_eq!(left.background, Some(c.bg));

        let overlay = resolve_overlay(&c, 1.0);
        assert_eq!(overlay.scrim_background, c.scrim);
        assert_eq!(overlay.scrim_padding, 40.0);
        assert_eq!(overlay.border, Border { color: c.gold, width: 1.5, radius: 10.0.into() });
    }

    #[test]
    fn padding_margin_gap_and_the_overlay_follow_the_global_scale_but_borders_stay_put() {
        let c = tokens();
        let z = resolve(&LEFT_ZONE, &c, 2.0);
        assert_eq!(z.margin, pad(12.0, 0.0, 0.0, 12.0));
        assert_eq!(z.padding, Padding::from(2.0));
        assert_eq!(z.border.unwrap().width, 0.0);
        assert_eq!(z.border.unwrap().radius, 8.0.into(), "region borders are not scaled");
        assert_eq!(resolve(&LEFT_ICON_RAIL, &c, 2.0).gap, 24.0);
        let o = resolve_overlay(&c, 2.0);
        assert_eq!(o.scrim_padding, 80.0);
        assert_eq!(o.border.width, 3.0);
        assert_eq!(o.border.radius, 20.0.into(), "the overlay frame scales with the UI");
    }

    #[test]
    fn colors_follow_whatever_tokens_are_passed_in() {
        let mut c = tokens();
        c.bg = Color::from_rgb(0.1, 0.2, 0.3);
        assert_eq!(resolve(&TOP_BAR, &c, 1.0).background, Some(c.bg));
        assert_eq!(resolve(&LEFT_ZONE, &c, 1.0).background, Some(c.bg));
    }

    #[test]
    fn the_exported_spec_has_the_dozer_json_shape() {
        let d = spec_dump();
        assert_eq!(d["top_bar"]["padding"], json!([0.0, 10.0, 0.0, 78.0]));
        assert_eq!(d["left_zone"]["margin"], json!([6.0, 0.0, 0.0, 6.0]));
        assert_eq!(d["left_icon_rail"]["border"], Value::Null);
        assert_eq!(d["maximize_overlay"]["border"], json!({ "color": "GOLD", "width": 1.5, "radius": 10.0 }));
        assert_eq!(d["background"], "TAB_ACTIVE_BORDER");
        for region in ["top_bar", "left_icon_rail", "right_icon_rail", "status_bar", "left_zone", "right_zone"] {
            for key in ["background", "border", "padding", "gap", "margin"] {
                assert!(d[region].get(key).is_some(), "{region}.{key}");
            }
        }
    }
}
```

`src/theme/mod.rs` 里加一行:

```rust
pub mod region;
```

(放在 `pub mod icon_size;` 之后,保持字母序。)

`examples/dump_shell_regions.rs`:

```rust
//! 把 7 个外壳区域的规格以 JSON 打到标准输出,供跨仓库一致性检查与 Dozer 的 `workspace.json` 比对。
//!   cargo run -q --example dump_shell_regions
fn main() {
    println!("{}", serde_json::to_string_pretty(&byteui::theme::region::spec_dump()).expect("spec serialises"));
}
```

- [ ] **Step 3: 运行新模块的测试**

```bash
cd $ROOT/byteui-shell
cargo test --lib region
```

Expected: `test result: ok. 4 passed`。

- [ ] **Step 4: 版本号与更新日志**

`Cargo.toml` 把 `version = "0.4.2"` 改为 `version = "0.4.3"`。`CHANGELOG.md` 在 `# Changelog` 之后、`## 0.4.1` 之前插入:

```markdown
## 0.4.3

- 新增 `theme::region`:外壳区域样式(`top_bar`、`left_icon_rail`、`right_icon_rail`、`status_bar`、`left_zone`、`right_zone`、`maximize_overlay`、`window_background`)。数值与 Dozer `workspace.json` 的同名区域一致;颜色令牌每次现读,`padding` / `margin` / `gap` 与放大浮层尺寸乘全局 UI scale。供共享窗口骨架 `bytehost-chrome` 使用。
- 新增 `examples/dump_shell_regions.rs`:导出这些区域的规格 JSON,供跨仓库一致性检查使用。
```

(CHANGELOG 里没有 0.4.2 一节,不用补。)

- [ ] **Step 5: 整个 byteui 测试集仍全绿**

```bash
cargo test 2>&1 | grep -E "test result|FAILED|error"
cargo run -q --example dump_shell_regions | head -5
```

Expected: 单元测试 `83 passed`(基线 79 + 新增 4),`0 failed`;示例打印以 `{` 开头的 JSON。

- [ ] **Step 6: Commit(在 `$ROOT/byteui-shell`)**

```bash
git add -A src examples Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "feat(theme): shell region styles for the shared window shell (0.4.3)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 跨仓库一致性检查脚本

**Files:**(在 `$ROOT/digger`)
- Create: `scripts/check-shell-regions.sh`

**Interfaces:**
- Consumes: Task 1 的 `dump_shell_regions` 示例、Dozer 的 `crates/dozer-app/assets/theme/workspace.json`。
- Produces: `scripts/check-shell-regions.sh` —— byteui 的 7 个外壳区域与 Dozer 的 `workspace.json` 逐项一致时退出码 0,否则打印每一处差异并以 1 退出。

- [ ] **Step 1: 写脚本**

`scripts/check-shell-regions.sh`:

```bash
#!/usr/bin/env bash
# 跨仓库一致性检查:byteui 的外壳区域样式 ≡ Dozer `workspace.json` 的同名区域(S1 期间两处并存,迁移后才合并)。
#
#   scripts/check-shell-regions.sh                       默认 ../byteui 与 ../dozer
#   BYTEUI=/path/to/byteui DOZER=/path/to/dozer scripts/check-shell-regions.sh
#
# 不一致时打印每一处差异并以非零退出。
set -euo pipefail
cd "$(dirname "$0")/.."

BYTEUI="${BYTEUI:-../byteui}"
DOZER="${DOZER:-../dozer}"
JSON="$DOZER/crates/dozer-app/assets/theme/workspace.json"
[ -f "$JSON" ] || { echo "找不到 Dozer 的 workspace.json:$JSON" >&2; exit 2; }

ours="$(mktemp)"
trap 'rm -f "$ours"' EXIT
(cd "$BYTEUI" && cargo run -q --example dump_shell_regions) > "$ours"

python3 -I - "$JSON" "$ours" <<'PY'
import json, sys

dozer = json.load(open(sys.argv[1]))
ours = json.load(open(sys.argv[2]))

def pad4(p):
    """Dozer 的 padding 有三种写法:单值 / [v,h] / [top,right,bottom,left];统一展开成四边。"""
    if p is None:
        return [0.0, 0.0, 0.0, 0.0]
    if isinstance(p, (int, float)):
        return [float(p)] * 4
    if len(p) == 2:
        return [float(p[0]), float(p[1]), float(p[0]), float(p[1])]
    return [float(x) for x in p]

def norm_region(r):
    border = r.get("border")
    return {
        "background": r.get("background"),
        "border": None if border is None else {"color": border["color"], "width": float(border["width"]), "radius": float(border["radius"])},
        "padding": pad4(r["padding"]),
        "gap": float(r["gap"]),
        "margin": pad4(r.get("margin")),
    }

bad = []
for name in ["top_bar", "left_icon_rail", "right_icon_rail", "status_bar", "left_zone", "right_zone"]:
    a, b = norm_region(dozer["regions"][name]), norm_region(ours[name])
    for key in a:
        if a[key] != b[key]:
            bad.append(f"{name}.{key}: Dozer={a[key]!r} byteui={b[key]!r}")

d, o = dozer["regions"]["maximize_overlay"], ours["maximize_overlay"]
if d["scrim"]["background"] != o["scrim"]["background"] or float(d["scrim"]["padding"]) != float(o["scrim"]["padding"]):
    bad.append(f"maximize_overlay.scrim: Dozer={d['scrim']!r} byteui={o['scrim']!r}")
db, ob = d["border"], o["border"]
if (db["color"], float(db["width"]), float(db["radius"])) != (ob["color"], float(ob["width"]), float(ob["radius"])):
    bad.append(f"maximize_overlay.border: Dozer={db!r} byteui={ob!r}")
if dozer["background"] != ours["background"]:
    bad.append(f"background: Dozer={dozer['background']!r} byteui={ours['background']!r}")

if bad:
    print("byteui 外壳区域样式与 Dozer 的 workspace.json 不一致:", file=sys.stderr)
    for line in bad:
        print("  " + line, file=sys.stderr)
    sys.exit(1)
print("shell regions: ok(byteui 的 7 个外壳区域与 Dozer workspace.json 逐项一致)")
PY
```

```bash
chmod +x scripts/check-shell-regions.sh
```

- [ ] **Step 2: 正向:一致时通过**

```bash
cd $ROOT/digger
BYTEUI=$ROOT/byteui-shell bash scripts/check-shell-regions.sh; echo "exit=$?"
```

Expected: `shell regions: ok(byteui 的 7 个外壳区域与 Dozer workspace.json 逐项一致)` 和 `exit=0`。

- [ ] **Step 3: 变异:故意改一个数值,必须失败**

```bash
sed -i '' 's/padding: \[0.0, 10.0, 0.0, 78.0\]/padding: [0.0, 10.0, 0.0, 80.0]/' $ROOT/byteui-shell/src/theme/region.rs
BYTEUI=$ROOT/byteui-shell bash scripts/check-shell-regions.sh; echo "exit=$?"
sed -i '' 's/padding: \[0.0, 10.0, 0.0, 80.0\]/padding: [0.0, 10.0, 0.0, 78.0]/' $ROOT/byteui-shell/src/theme/region.rs
git -C $ROOT/byteui-shell diff --stat
```

Expected: 第一条打印 `top_bar.padding: Dozer=[0.0, 10.0, 0.0, 78.0] byteui=[0.0, 10.0, 0.0, 80.0]` 和 `exit=1`;还原后 `git diff --stat` 为空。

- [ ] **Step 4: Commit(在 `$ROOT/digger`)**

```bash
git add scripts/check-shell-regions.sh
git commit -m "test(scripts): check byteui's shell regions against Dozer's workspace.json

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

## bytehost-chrome(在 `$ROOT/bytehost-chrome`,分支 `feat/chrome`)

下面 Task 3–10 都在这个 worktree 里做,命令默认 `cd $ROOT/bytehost-chrome`。每个任务的写法一致:先写文件里的 `#[cfg(test)] mod tests`(运行一次,确认因类型 / 函数未定义而编译失败 = 红),再补全实现(运行,绿),再提交。文件整体给出一次,**测试模块在文件末尾**。第一次编译会拉取并编译 wry / iced,要几分钟。

### Task 3: crate 骨架、度量与纯几何

**Files:**
- Create: `crates/bytehost-chrome/Cargo.toml`、`src/lib.rs`、`src/metrics.rs`、`src/geometry.rs`

**Interfaces:**
- Produces:
  - `metrics::{Insets, ZoneInsets, ChromeMetrics}`;`ChromeMetrics::byteboy2077()`(测试用字面量)
  - `geometry::{Rect, Side, ZoneState, ShellRects, layout, clamp_left_width, left_zone_width, right_zone_width, zones_width, maximize_box, zone_at_x}`
  - `Side::{Left, Right}`、`Side::other()`、`Side::index()`;`Rect::{inset, right, bottom, contains}`
  - `ShellRects::content(side) -> Option<Rect>`:面板内容矩形(外框 − 外边距 − 内边距;放大态是放大盒子);收起或被另一侧放大盖住时是 `None`

- [ ] **Step 1: crate 清单**

`crates/bytehost-chrome/Cargo.toml`:

```toml
[package]
name = "bytehost-chrome"
version = "0.1.0"
edition.workspace = true
license.workspace = true
publish = false
description = "bytehost 共享窗口骨架:顶栏、左右图标栏、左右面板区、分隔线、配对视图、放大态与布局持久化。以 PanelId 为键,不理解任何具体面板。"

[dependencies]
bytehost-core.workspace = true
# 配对分割的存储与拖拽换算(`PaneStore` / `apply_pane_split_drag`)复用 bytehost-desktop 已有实现。
bytehost-desktop = { path = "../bytehost-desktop" }
serde.workspace = true
serde_json.workspace = true
# 控件与度量读 byteui 的主题(几何、配色、外壳区域样式)。
byteui = { git = "https://github.com/byteboyai/byteui", tag = "v0.4.1" }
# 控件返回 `Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>`(与 byteui 一致),不开渲染器 feature。
iced_widget = { version = "0.14", default-features = false }
iced_renderer = { version = "0.14", default-features = false }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-app-kit = "0.3"
objc2-foundation = "0.3"
block2 = "0.6"
raw-window-handle = "0.6"
```

(此时 `byteui` 依赖按 tag `v0.4.1` 解析即可 —— `metrics` / `geometry` 还没有用到 byteui;开发期 `[patch]` 留到 Task 8 真正需要 byteui 的新模块时再加。)

- [ ] **Step 2: `src/lib.rs`(本任务的版本,后续任务逐步加行)**

```rust
//! bytehost-chrome:共享窗口骨架。(完整的 crate 文档见 Task 10 的最终 lib.rs)

pub mod geometry;
pub mod metrics;

pub use geometry::{Rect, ShellRects, Side};
pub use metrics::ChromeMetrics;
```

- [ ] **Step 3: 写 `src/metrics.rs` 与 `src/geometry.rs`**

`src/metrics.rs`:

```rust
//! 外壳度量:几何函数只读这一份数,控件也读它,不允许各自写常量。
//! 纯数据,不依赖 byteui / iced,所以几何可以在没有窗口的测试里验证。

/// 四边距,字段顺序与 iced `Padding` 一致。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const ZERO: Self = Self { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 };

    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self { top, right, bottom, left }
    }

    pub const fn uniform(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }

    pub fn plus(&self, other: Insets) -> Insets {
        Insets::new(
            self.top + other.top,
            self.right + other.right,
            self.bottom + other.bottom,
            self.left + other.left,
        )
    }
}

/// 一个面板区外框的留白:`margin` 在外框之外(悬浮留白),`padding` 在外框之内。
/// 面板内容矩形 = 外框 − margin − padding。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneInsets {
    pub margin: Insets,
    pub padding: Insets,
}

impl ZoneInsets {
    pub fn total(&self) -> Insets {
        self.margin.plus(self.padding)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeMetrics {
    pub top_bar_height: f32,
    pub footbar_height: f32,
    pub icon_rail_width: f32,
    pub divider_width: f32,
    pub min_zone_width: f32,
    pub min_split_ratio: f32,
    pub max_split_ratio: f32,
    /// 放大态:遮罩到金色描边盒子的内边距。
    pub maximize_padding: f32,
    pub left_zone: ZoneInsets,
    pub right_zone: ZoneInsets,
}

impl ChromeMetrics {
    /// ByteBoy2077 主题下的数值,与 `byteui::theme::geometry::GeometryTokens::byteboy2077()` 和
    /// Dozer `assets/theme/workspace.json` 的 `left_zone` / `right_zone` 一致。仅供测试与文档对照;
    /// 运行时一律从 byteui 读(见 `theme::metrics_from_byteui`)。
    pub const fn byteboy2077() -> Self {
        Self {
            top_bar_height: 40.0,
            footbar_height: 22.0,
            icon_rail_width: 44.0,
            divider_width: 8.0,
            min_zone_width: 320.0,
            min_split_ratio: 0.2,
            max_split_ratio: 0.8,
            maximize_padding: 40.0,
            left_zone: ZoneInsets { margin: Insets::new(6.0, 0.0, 0.0, 6.0), padding: Insets::uniform(1.0) },
            right_zone: ZoneInsets { margin: Insets::new(6.0, 6.0, 0.0, 0.0), padding: Insets::uniform(1.0) },
        }
    }
}
```

`src/geometry.rs`(先只写 `mod tests`,运行一次看红,再补函数):

```rust
//! 纯几何:窗口尺寸 + 骨架状态 → 各区域矩形。无 iced、无窗口。
//!
//! 公式来自 Dozer(`app/state.rs` 的 `zones_width` / `clamp_left_width` / `left_zone_width` /
//! `right_zone_width`,`app/layout.rs` 的 `maximized_box_*`),但以 `Side` + 中立度量表达。

use crate::metrics::{ChromeMetrics, Insets};

/// 逻辑像素矩形,原点在窗口左上角。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 向内缩 `insets`,宽高不会为负。
    pub fn inset(&self, insets: Insets) -> Rect {
        Rect::new(
            self.x + insets.left,
            self.y + insets.top,
            (self.w - insets.horizontal()).max(0.0),
            (self.h - insets.vertical()).max(0.0),
        )
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// 几何需要的骨架状态(`ShellModel` 提供)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneState {
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    /// 持久化的左面板区外框宽度(未夹取)。
    pub left_width: f32,
    pub maximized: Option<Side>,
}

/// 窗口里所有区域的矩形。`None` = 这个区域此刻不存在 / 不可见。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellRects {
    pub top_bar: Rect,
    pub left_rail: Rect,
    pub right_rail: Rect,
    pub footbar: Rect,
    /// 左右面板区之间那条恒在的分隔线。
    pub divider: Rect,
    /// 面板区外框(含 margin)。收起的一侧是 `None`。
    pub left_zone: Option<Rect>,
    pub right_zone: Option<Rect>,
    /// 面板内容矩形(外框 − margin − padding;放大态是放大盒子本身)。
    /// 收起、或被另一侧放大盖住的一侧是 `None`。
    pub left_content: Option<Rect>,
    pub right_content: Option<Rect>,
    /// 放大态的金色描边盒子。
    pub maximize_box: Option<Rect>,
}

impl ShellRects {
    pub fn content(&self, side: Side) -> Option<Rect> {
        match side {
            Side::Left => self.left_content,
            Side::Right => self.right_content,
        }
    }

    pub fn zone(&self, side: Side) -> Option<Rect> {
        match side {
            Side::Left => self.left_zone,
            Side::Right => self.right_zone,
        }
    }
}

/// 两条图标栏与恒在的分隔线之外,留给左右面板区的总宽。
/// 分隔线无条件占位(收起一侧也保留拖拽手柄),所以恒扣。
pub fn zones_width(window_w: f32, m: &ChromeMetrics) -> f32 {
    (window_w - 2.0 * m.icon_rail_width - m.divider_width).max(0.0)
}

/// 把持久化的左宽夹进"当前窗口宽度下合法"的区间:下限 `min_zone_width`,上限给对侧也留够
/// `min_zone_width`。窗口窄到上限低于下限时用下限垫平,`clamp` 恒不 panic。
/// 这是**唯一**一处夹取:渲染、几何、拖拽都走它。
pub fn clamp_left_width(window_w: f32, left_width: f32, m: &ChromeMetrics) -> f32 {
    let upper = (zones_width(window_w, m) - m.min_zone_width).max(m.min_zone_width);
    left_width.clamp(m.min_zone_width, upper)
}

/// 左面板区外框当前实际宽度:收起 0;对侧收起时独占;否则夹取后的持久化宽。
pub fn left_zone_width(window_w: f32, s: &ZoneState, m: &ChromeMetrics) -> f32 {
    if s.left_collapsed {
        0.0
    } else if s.right_collapsed {
        zones_width(window_w, m)
    } else {
        clamp_left_width(window_w, s.left_width, m)
    }
}

/// 右面板区外框当前实际宽度:收起 0;否则是左区没占走的剩余。
pub fn right_zone_width(window_w: f32, s: &ZoneState, m: &ChromeMetrics) -> f32 {
    if s.right_collapsed {
        return 0.0;
    }
    (zones_width(window_w, m) - left_zone_width(window_w, s, m)).max(0.0)
}

/// 放大态盒子:两条图标栏之间、顶栏之下、直到窗口底边(遮罩也盖住底栏),四周留 `maximize_padding`。
pub fn maximize_box(window_w: f32, window_h: f32, m: &ChromeMetrics) -> Rect {
    let p = m.maximize_padding;
    Rect::new(
        m.icon_rail_width + p,
        m.top_bar_height + p,
        (window_w - 2.0 * m.icon_rail_width - 2.0 * p).max(0.0),
        (window_h - m.top_bar_height - 2.0 * p).max(0.0),
    )
}

/// 窗口逻辑尺寸 + 骨架状态 → 全部区域矩形。
pub fn layout(window_w: f32, window_h: f32, s: &ZoneState, m: &ChromeMetrics) -> ShellRects {
    let rail_h = (window_h - m.top_bar_height).max(0.0);
    let body_y = m.top_bar_height;
    let body_h = (window_h - m.top_bar_height - m.footbar_height).max(0.0);
    let left_w = left_zone_width(window_w, s, m);
    let right_w = right_zone_width(window_w, s, m);
    let zones_x = m.icon_rail_width;

    // 左区(收起时宽 0)、分隔线、右区依次排开;一侧收起时另一侧独占 `zones_width`(见上面两个函数)。
    let left_outer = Rect::new(zones_x, body_y, left_w, body_h);
    let divider = Rect::new(zones_x + left_w, body_y, m.divider_width, body_h);
    let right_outer = Rect::new(divider.right(), body_y, right_w, body_h);

    let left_zone = (!s.left_collapsed).then_some(left_outer);
    let right_zone = (!s.right_collapsed).then_some(right_outer);

    let mut left_content = left_zone.map(|z| z.inset(m.left_zone.total()));
    let mut right_content = right_zone.map(|z| z.inset(m.right_zone.total()));
    let mut maximize = None;
    if let Some(side) = s.maximized {
        let b = maximize_box(window_w, window_h, m);
        maximize = Some(b);
        match side {
            Side::Left => {
                left_content = (!s.left_collapsed).then_some(b);
                right_content = None;
            }
            Side::Right => {
                right_content = (!s.right_collapsed).then_some(b);
                left_content = None;
            }
        }
    }
    ShellRects {
        top_bar: Rect::new(0.0, 0.0, window_w, m.top_bar_height),
        left_rail: Rect::new(0.0, body_y, m.icon_rail_width, rail_h),
        right_rail: Rect::new(window_w - m.icon_rail_width, body_y, m.icon_rail_width, rail_h),
        footbar: Rect::new(zones_x, window_h - m.footbar_height, (window_w - 2.0 * m.icon_rail_width).max(0.0), m.footbar_height),
        divider,
        left_zone,
        right_zone,
        left_content,
        right_content,
        maximize_box: maximize,
    }
}

/// 点击落在哪一侧面板区(整区)。落在图标栏、或点在不存在的那一侧时是 `None`,调用方保持原聚焦。
/// 放大态:整个内容区就是放大的那一侧。
pub fn zone_at_x(x: f32, window_w: f32, s: &ZoneState, m: &ChromeMetrics) -> Option<Side> {
    if x < m.icon_rail_width || x > window_w - m.icon_rail_width {
        return None;
    }
    if let Some(side) = s.maximized {
        return Some(side);
    }
    match (s.left_collapsed, s.right_collapsed) {
        (true, true) => None,
        (true, false) => Some(Side::Right),
        (false, true) => Some(Side::Left),
        (false, false) => {
            let boundary = m.icon_rail_width + left_zone_width(window_w, s, m);
            Some(if x < boundary { Side::Left } else { Side::Right })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1440.0;
    const H: f32 = 900.0;

    fn m() -> ChromeMetrics {
        ChromeMetrics::byteboy2077()
    }

    fn open(left_width: f32) -> ZoneState {
        ZoneState { left_collapsed: false, right_collapsed: false, left_width, maximized: None }
    }

    #[test]
    fn both_zones_open_split_the_space_between_the_rails() {
        let r = layout(W, H, &open(640.0), &m());
        assert_eq!(zones_width(W, &m()), 1344.0);
        assert_eq!(r.left_zone, Some(Rect::new(44.0, 40.0, 640.0, 838.0)));
        assert_eq!(r.divider, Rect::new(684.0, 40.0, 8.0, 838.0));
        assert_eq!(r.right_zone, Some(Rect::new(692.0, 40.0, 704.0, 838.0)));
        // 右区右缘正好贴着右图标栏
        assert_eq!(r.right_zone.unwrap().right(), W - 44.0);
        assert_eq!(r.right_rail, Rect::new(1396.0, 40.0, 44.0, 860.0));
        assert_eq!(r.top_bar, Rect::new(0.0, 0.0, W, 40.0));
        assert_eq!(r.footbar, Rect::new(44.0, 878.0, 1352.0, 22.0));
    }

    #[test]
    fn content_rects_subtract_the_zone_margin_and_padding() {
        let r = layout(W, H, &open(640.0), &m());
        // 左: margin top 6 left 6, padding 1 ⇒ x 44+6+1, y 40+6+1, w 640-6-0-2, h 838-6-0-2
        assert_eq!(r.left_content, Some(Rect::new(51.0, 47.0, 632.0, 830.0)));
        // 右: margin top 6 right 6 ⇒ x 692+0+1, w 704-0-6-2
        assert_eq!(r.right_content, Some(Rect::new(693.0, 47.0, 696.0, 830.0)));
    }

    #[test]
    fn a_narrow_window_clamps_the_left_width_and_never_starves_the_right_zone() {
        // 700 宽:zones = 700-96 = 604;上限 = max(604-320, 320) = 320
        assert_eq!(clamp_left_width(700.0, 640.0, &m()), 320.0);
        let r = layout(700.0, H, &open(640.0), &m());
        assert_eq!(r.left_zone.unwrap().w, 320.0);
        assert_eq!(r.right_zone.unwrap().w, 284.0);
        // 极窄:上限被下限垫平,不 panic
        assert_eq!(clamp_left_width(300.0, 640.0, &m()), 320.0);
        // 宽窗口:持久化宽原样保留(夹取只发生在渲染 / 几何时刻,不回写)
        assert_eq!(clamp_left_width(2000.0, 640.0, &m()), 640.0);
        // 太小的持久化宽被抬到下限
        assert_eq!(clamp_left_width(W, 12.0, &m()), 320.0);
    }

    #[test]
    fn collapsing_one_side_gives_the_other_the_whole_space() {
        let s = ZoneState { left_collapsed: true, ..open(640.0) };
        let r = layout(W, H, &s, &m());
        assert_eq!(r.left_zone, None);
        assert_eq!(r.left_content, None);
        // 分隔线仍在图标栏之后,右区从分隔线之后起,独占 zones_width
        assert_eq!(r.divider, Rect::new(44.0, 40.0, 8.0, 838.0));
        assert_eq!(r.right_zone, Some(Rect::new(52.0, 40.0, 1344.0, 838.0)));

        let s = ZoneState { right_collapsed: true, ..open(640.0) };
        let r = layout(W, H, &s, &m());
        assert_eq!(r.right_zone, None);
        assert_eq!(r.left_zone, Some(Rect::new(44.0, 40.0, 1344.0, 838.0)));
        assert_eq!(r.divider.x, 1388.0);
        assert_eq!(r.divider.right(), W - 44.0);
    }

    #[test]
    fn both_collapsed_leaves_no_zone() {
        let s = ZoneState { left_collapsed: true, right_collapsed: true, ..open(640.0) };
        let r = layout(W, H, &s, &m());
        assert_eq!((r.left_zone, r.right_zone, r.left_content, r.right_content), (None, None, None, None));
    }

    #[test]
    fn maximizing_one_side_hides_the_other_and_fills_the_box() {
        let s = ZoneState { maximized: Some(Side::Left), ..open(640.0) };
        let r = layout(W, H, &s, &m());
        let b = Rect::new(84.0, 80.0, 1272.0, 780.0);
        assert_eq!(r.maximize_box, Some(b));
        assert_eq!(r.left_content, Some(b));
        assert_eq!(r.right_content, None, "the covered side must report no content so its native view is hidden");
        let s = ZoneState { maximized: Some(Side::Right), ..open(640.0) };
        let r = layout(W, H, &s, &m());
        assert_eq!(r.right_content, Some(b));
        assert_eq!(r.left_content, None);
    }

    #[test]
    fn tiny_windows_never_produce_negative_sizes() {
        let r = layout(50.0, 50.0, &open(640.0), &m());
        for rect in [r.top_bar, r.left_rail, r.right_rail, r.footbar, r.divider].into_iter().chain([r.left_zone, r.right_zone, r.left_content, r.right_content].into_iter().flatten()) {
            assert!(rect.w >= 0.0 && rect.h >= 0.0, "{rect:?}");
        }
        assert!(maximize_box(50.0, 50.0, &m()).w >= 0.0);
    }

    #[test]
    fn a_click_lands_in_the_zone_under_it() {
        let s = open(640.0);
        assert_eq!(zone_at_x(10.0, W, &s, &m()), None, "on the left rail");
        assert_eq!(zone_at_x(W - 10.0, W, &s, &m()), None, "on the right rail");
        assert_eq!(zone_at_x(300.0, W, &s, &m()), Some(Side::Left));
        assert_eq!(zone_at_x(900.0, W, &s, &m()), Some(Side::Right));
        assert_eq!(zone_at_x(683.9, W, &s, &m()), Some(Side::Left));
        assert_eq!(zone_at_x(684.0, W, &s, &m()), Some(Side::Right));
        let s = ZoneState { left_collapsed: true, ..open(640.0) };
        assert_eq!(zone_at_x(100.0, W, &s, &m()), Some(Side::Right));
        let s = ZoneState { left_collapsed: true, right_collapsed: true, ..open(640.0) };
        assert_eq!(zone_at_x(100.0, W, &s, &m()), None);
        let s = ZoneState { maximized: Some(Side::Right), ..open(640.0) };
        assert_eq!(zone_at_x(100.0, W, &s, &m()), Some(Side::Right));
    }
}
```

- [ ] **Step 4: 运行**

```bash
cd $ROOT/bytehost-chrome
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 8 passed`,没有 warning。

- [ ] **Step 5: Commit**

```bash
git add crates/bytehost-chrome Cargo.lock
git commit -m "feat(chrome): bytehost-chrome crate with chrome metrics and pure shell geometry

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 图标栏的纯逻辑(同栏重排、跨栏移动、"确保可见")

**Files:**
- Create: `src/rail.rs`
- Modify: `src/lib.rs`(加 `pub mod rail;`)

**Interfaces:**
- Consumes: `geometry::Side`
- Produces:
  - `rail::Rails = [Vec<PanelId>; 2]`、`rail::RailDrag { source_side, source_index, origin_index, pending_cross, press_pos }`
  - `rail::{side_of, drag_move_into, cross_apply, dragged_panel, past_threshold, show_panel_in, DRAG_THRESHOLD_PX}`、`rail::ShowPanel`
- 行为照 Dozer `chrome/rail.rs`(`rail_drag_move_into` / `rail_cross_apply` / `show_panel_in` / `rail_drag_past_threshold`)和它的现有测试,以 `PanelId` 为键重写。

- [ ] **Step 1: 写 `src/rail.rs`(测试在末尾,先写测试看红)**

```rust
//! 图标栏的纯逻辑:同栏重排、跨栏移动、拖拽阈值、"确保可见"。以 `PanelId` 为键。
//! 行为照 Dozer `chrome/rail.rs`(`rail_drag_move_into` / `rail_cross_apply` / `show_panel_in` /
//! `rail_drag_past_threshold`),不依赖 iced,可直接单测。

use bytehost_core::id::PanelId;

use crate::geometry::Side;

/// 两条图标栏,`[左, 右]`。
pub type Rails = [Vec<PanelId>; 2];

/// 正在进行的图标栏拖拽(同栏重排 / 跨栏移动)。
#[derive(Debug, Clone, PartialEq)]
pub struct RailDrag {
    pub source_side: Side,
    pub source_index: usize,
    /// 拖拽开始时 `source_index` 的原始值;同栏重排期间 `source_index` 会变,据此判断是否真的重排过。
    pub origin_index: usize,
    /// 悬停到另一栏时记录的目标位置;松手才提交,悬停期间不搬、不落盘。
    pub pending_cross: Option<(Side, usize)>,
    /// 按下瞬间的光标位置,用来区分"单击"与"真的在拖"。
    pub press_pos: (f32, f32),
}

/// 从"按下武装"到"算一次真的拖拽"所需的最小位移(逻辑像素),避免快速单击闪出拖拽视觉。
pub const DRAG_THRESHOLD_PX: f32 = 4.0;

pub fn side_of(rails: &Rails, panel: &PanelId) -> Option<Side> {
    if rails[0].contains(panel) {
        Some(Side::Left)
    } else if rails[1].contains(panel) {
        Some(Side::Right)
    } else {
        None
    }
}

/// 同栏:立即重排并更新 `source_index`,清掉跨栏悬停残留;跨栏:只记悬停目标,松手时再提交。
pub fn drag_move_into(rails: &mut Rails, drag: &mut RailDrag, side: Side, to: usize) {
    if drag.source_side == side {
        drag.pending_cross = None;
        let panels = &mut rails[side.index()];
        let from = drag.source_index;
        if from == to || from >= panels.len() || to >= panels.len() {
            return;
        }
        let panel = panels.remove(from);
        panels.insert(to, panel);
        drag.source_index = to;
    } else {
        drag.pending_cross = Some((side, to));
    }
}

/// 跨栏移动的落地:把 `source_side` 第 `source_index` 个面板搬到 `target_side` 第 `target_index` 位。
/// 源栏只剩这一个(搬走会清空栏)或下标越界时返回 `None` 且不改动;目标下标越界时取末尾。
pub fn cross_apply(rails: &mut Rails, source_side: Side, source_index: usize, target_side: Side, target_index: usize) -> Option<PanelId> {
    if source_side == target_side {
        return None;
    }
    let source = &rails[source_side.index()];
    if source.len() <= 1 || source_index >= source.len() {
        return None;
    }
    let panel = rails[source_side.index()].remove(source_index);
    let target = &mut rails[target_side.index()];
    let at = target_index.min(target.len());
    target.insert(at, panel.clone());
    Some(panel)
}

/// 此刻鼠标下真正拖着的那个面板(同栏重排会实时更新 `source_index`)。
pub fn dragged_panel(rails: &Rails, drag: Option<&RailDrag>) -> Option<PanelId> {
    let drag = drag?;
    rails[drag.source_side.index()].get(drag.source_index).cloned()
}

/// 自按下以来光标是否已越过阈值(越过才算拖拽,不是单击)。
pub fn past_threshold(drag: &RailDrag, cursor: (f32, f32)) -> bool {
    let dx = cursor.0 - drag.press_pos.0;
    let dy = cursor.1 - drag.press_pos.1;
    dx * dx + dy * dy > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX
}

/// `show_panel_in` 的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ShowPanel {
    pub selected: PanelId,
    pub collapsed: bool,
    pub maximized: Option<Side>,
    /// 该侧原本显示的是别的面板(真切换)。
    pub switched: bool,
    /// `false` = 面板本来就看得见,什么都不用做。
    pub changed: bool,
}

/// "把某面板显示出来"的纯逻辑——与图标栏点击的区别:**绝不收起**。已经看得见就什么都不做;被收起就展开;
/// 显示的是别的就切过去;被另一侧的放大态盖住就退出放大。
pub fn show_panel_in(side: Side, panel: &PanelId, selected: &PanelId, collapsed: bool, maximized: Option<Side>) -> ShowPanel {
    let visible = selected == panel && !collapsed;
    let covered = maximized.is_some_and(|m| m != side);
    if visible && !covered {
        return ShowPanel { selected: selected.clone(), collapsed, maximized, switched: false, changed: false };
    }
    ShowPanel { selected: panel.clone(), collapsed: false, maximized: None, switched: selected != panel, changed: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> PanelId {
        PanelId::new(s).unwrap()
    }

    fn rails() -> Rails {
        [vec![id("t.a"), id("t.b"), id("t.c")], vec![id("t.x"), id("t.y")]]
    }

    fn drag(side: Side, index: usize) -> RailDrag {
        RailDrag { source_side: side, source_index: index, origin_index: index, pending_cross: None, press_pos: (0.0, 0.0) }
    }

    fn names(r: &Rails, side: Side) -> Vec<&str> {
        r[side.index()].iter().map(|p| p.as_str()).collect()
    }

    #[test]
    fn same_side_reorder_moves_the_panel_and_follows_it() {
        let mut r = rails();
        let mut d = drag(Side::Left, 0);
        drag_move_into(&mut r, &mut d, Side::Left, 2);
        assert_eq!(names(&r, Side::Left), ["t.b", "t.c", "t.a"]);
        assert_eq!(d.source_index, 2);
        assert_eq!(d.origin_index, 0, "origin stays so the end can tell a reorder happened");
        assert_eq!(dragged_panel(&r, Some(&d)), Some(id("t.a")));
    }

    #[test]
    fn same_side_reorder_to_the_same_or_a_bad_index_is_a_no_op() {
        let mut r = rails();
        let mut d = drag(Side::Left, 1);
        drag_move_into(&mut r, &mut d, Side::Left, 1);
        drag_move_into(&mut r, &mut d, Side::Left, 9);
        assert_eq!(names(&r, Side::Left), ["t.a", "t.b", "t.c"]);
        assert_eq!(d.source_index, 1);
    }

    #[test]
    fn hovering_the_other_side_only_records_a_pending_target() {
        let mut r = rails();
        let mut d = drag(Side::Left, 1);
        drag_move_into(&mut r, &mut d, Side::Right, 1);
        assert_eq!(d.pending_cross, Some((Side::Right, 1)));
        assert_eq!(names(&r, Side::Left), ["t.a", "t.b", "t.c"], "nothing moves until the button is released");
        // hovering back cancels the pending move
        drag_move_into(&mut r, &mut d, Side::Left, 1);
        assert_eq!(d.pending_cross, None);
    }

    #[test]
    fn cross_side_move_relocates_and_clamps_the_target_index() {
        let mut r = rails();
        assert_eq!(cross_apply(&mut r, Side::Left, 1, Side::Right, 1), Some(id("t.b")));
        assert_eq!(names(&r, Side::Left), ["t.a", "t.c"]);
        assert_eq!(names(&r, Side::Right), ["t.x", "t.b", "t.y"]);
        // an index past the end lands the panel last
        assert_eq!(cross_apply(&mut r, Side::Left, 0, Side::Right, 99), Some(id("t.a")));
        assert_eq!(names(&r, Side::Right), ["t.x", "t.b", "t.y", "t.a"]);
    }

    #[test]
    fn a_side_is_never_emptied_by_a_cross_side_move() {
        let mut r: Rails = [vec![id("t.only")], vec![id("t.x")]];
        assert_eq!(cross_apply(&mut r, Side::Left, 0, Side::Right, 0), None);
        assert_eq!(names(&r, Side::Left), ["t.only"]);
        assert_eq!(names(&r, Side::Right), ["t.x"]);
    }

    #[test]
    fn a_stale_source_index_or_same_side_target_changes_nothing() {
        let mut r = rails();
        assert_eq!(cross_apply(&mut r, Side::Left, 7, Side::Right, 0), None);
        assert_eq!(cross_apply(&mut r, Side::Left, 0, Side::Left, 1), None);
        assert_eq!(r, rails());
    }

    #[test]
    fn dragged_panel_is_none_without_a_drag_or_with_a_bad_index() {
        let r = rails();
        assert_eq!(dragged_panel(&r, None), None);
        assert_eq!(dragged_panel(&r, Some(&drag(Side::Right, 9))), None);
        assert_eq!(dragged_panel(&r, Some(&drag(Side::Right, 1))), Some(id("t.y")));
    }

    #[test]
    fn the_drag_threshold_separates_a_click_from_a_drag() {
        let mut d = drag(Side::Left, 0);
        d.press_pos = (10.0, 10.0);
        assert!(!past_threshold(&d, (10.0, 10.0)));
        assert!(!past_threshold(&d, (14.0, 10.0)), "exactly at the threshold is still a click");
        assert!(past_threshold(&d, (14.1, 10.0)));
        assert!(past_threshold(&d, (13.0, 13.0)), "diagonal distance counts");
    }

    #[test]
    fn show_panel_does_nothing_when_it_is_already_visible() {
        let out = show_panel_in(Side::Left, &id("t.a"), &id("t.a"), false, None);
        assert!(!out.changed && !out.switched);
    }

    #[test]
    fn show_panel_never_collapses_an_open_panel_and_keeps_its_own_maximize() {
        let out = show_panel_in(Side::Left, &id("t.a"), &id("t.a"), false, Some(Side::Left));
        assert!(!out.changed);
        assert_eq!(out.maximized, Some(Side::Left));
    }

    #[test]
    fn show_panel_expands_a_collapsed_active_panel_without_a_switch() {
        let out = show_panel_in(Side::Left, &id("t.a"), &id("t.a"), true, None);
        assert!(out.changed && !out.switched && !out.collapsed);
    }

    #[test]
    fn show_panel_switches_when_the_side_shows_another_panel() {
        let out = show_panel_in(Side::Left, &id("t.b"), &id("t.a"), false, None);
        assert!(out.changed && out.switched);
        assert_eq!(out.selected, id("t.b"));
    }

    #[test]
    fn show_panel_exits_a_maximize_that_covers_it_and_a_switch_always_exits_it() {
        // the other side is maximized and covers us: leave the maximize so we can be seen
        let out = show_panel_in(Side::Left, &id("t.a"), &id("t.a"), false, Some(Side::Right));
        assert!(out.changed);
        assert_eq!(out.maximized, None);
        // a switch always drops the maximize
        let out = show_panel_in(Side::Left, &id("t.b"), &id("t.a"), false, Some(Side::Left));
        assert_eq!(out.maximized, None);
    }

    #[test]
    fn side_of_finds_each_panel() {
        let r = rails();
        assert_eq!(side_of(&r, &id("t.b")), Some(Side::Left));
        assert_eq!(side_of(&r, &id("t.y")), Some(Side::Right));
        assert_eq!(side_of(&r, &id("t.nope")), None);
    }
}
```

`src/lib.rs` 加:

```rust
pub mod rail;
```

- [ ] **Step 2: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 22 passed`(8 + 14)。

- [ ] **Step 3: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): rail operations keyed by PanelId (reorder, cross-rail move, show panel)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 动画状态机(悬停渐变、槽位让位)

**Files:**
- Create: `src/anim.rs`
- Modify: `src/lib.rs`(加 `pub mod anim;`)

**Interfaces:**
- Consumes: `geometry::Side`
- Produces: `anim::HoverAnim { set, advance, active, t }`、`anim::SlotAnim { new, retarget, active, current }`(照 Dozer `HoverAnim` / `RailSlotAnim`:每拍残余 50% 的指数逼近,足够接近就 snap)

- [ ] **Step 1: 写 `src/anim.rs`**

```rust
//! 图标栏的两种动画状态机(照 Dozer `HoverAnim` / `RailSlotAnim`):悬停颜色渐变、换位时相邻图标滑动让位。
//! 都是"每拍残余 50% 的指数逼近,足够接近就 snap",由产品的 ~60fps 定时器驱动 `Tick`。

use crate::geometry::Side;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HoverAnim {
    progress: f32,
    target: f32,
}

impl HoverAnim {
    pub fn set(&mut self, hovered: bool) {
        self.target = if hovered { 1.0 } else { 0.0 };
    }
    pub fn advance(&mut self) {
        let next = self.progress + (self.target - self.progress) * 0.5;
        self.progress = if (next - self.target).abs() < 0.01 { self.target } else { next };
    }
    pub fn active(&self) -> bool {
        (self.progress - self.target).abs() > 0.001
    }
    pub fn t(&self) -> f32 {
        self.progress
    }
}

/// 图标按钮的槽位动画:`current` 是本帧渲染用的浮点槽位号,逼近 `target`;`side` 记录上次逼近所在的栏——
/// 同栏重排时平滑滑动,跨栏时两条栏是完全不同的物理列,直接 snap。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotAnim {
    current: f32,
    side: Side,
}

impl SlotAnim {
    pub fn new(side: Side, target: f32) -> Self {
        Self { current: target, side }
    }
    pub fn retarget(&mut self, side: Side, target: f32) {
        if self.side != side {
            self.side = side;
            self.current = target;
            return;
        }
        let next = self.current + (target - self.current) * 0.5;
        self.current = if (next - target).abs() < 0.02 { target } else { next };
    }
    pub fn active(&self, side: Side, target: f32) -> bool {
        self.side == side && (self.current - target).abs() > 0.005
    }
    pub fn current(&self, side: Side) -> Option<f32> {
        (self.side == side).then_some(self.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_converges_in_a_few_ticks_and_reports_activity() {
        let mut h = HoverAnim::default();
        assert!(!h.active());
        h.set(true);
        assert!(h.active());
        let mut ticks = 0;
        while h.active() {
            h.advance();
            ticks += 1;
            assert!(ticks < 20, "must converge");
        }
        assert_eq!(h.t(), 1.0);
        h.set(false);
        h.advance();
        assert!(h.t() < 1.0 && h.t() > 0.0, "eases out, does not jump");
    }

    #[test]
    fn a_slot_slides_within_a_rail_and_snaps_across_rails() {
        let mut a = SlotAnim::new(Side::Left, 0.0);
        a.retarget(Side::Left, 2.0);
        let first = a.current(Side::Left).unwrap();
        assert!(first > 0.0 && first < 2.0, "slides, does not jump: {first}");
        for _ in 0..12 {
            a.retarget(Side::Left, 2.0);
        }
        assert_eq!(a.current(Side::Left), Some(2.0));
        assert!(!a.active(Side::Left, 2.0));
        a.retarget(Side::Right, 1.0);
        assert_eq!(a.current(Side::Right), Some(1.0), "a cross-rail move snaps");
        assert_eq!(a.current(Side::Left), None, "the old side no longer owns the animation");
    }
}
```

`src/lib.rs` 加 `pub mod anim;`。

- [ ] **Step 2: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 24 passed`。

- [ ] **Step 3: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): hover and rail-slot animation state machines

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `ShellModel` —— 骨架状态机

**Files:**
- Create: `src/model.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: Task 3 的 `geometry::*`、`metrics::ChromeMetrics`;Task 4 的 `rail::*`;Task 5 的 `anim::*`;`bytehost_desktop::{PaneDims, PaneDragInput, PaneSpec, PaneStore, apply_pane_split_drag}`
- Produces:
  - `model::ShellModel::new(rails: Rails, selected: [PanelId; 2], left_width: f32, window: (f32, f32)) -> Result<Self, ModelError>`
  - 读:`rails() / rail(side) / selected(side) / collapsed(side) / left_width() / window() / panes() / maximized() / active_zone() / rail_drag() / dragging_divider() / cursor() / rail_drag_confirmed() / dragged_panel() / side_of(&PanelId) / hover_progress / slot_position / animating / zone_state / rects(&ChromeMetrics) / effective_left_width / pair_rects(side, &Ctx)`
  - 写:`update(&mut self, ShellMsg, &Ctx) -> Outcome`、`restore(collapsed, left_width, PaneStore)`
  - `ShellMsg::{PanelPress{panel}, Hover{panel,hovered}, Tick, Noop, RailDragMove{side,index}, CursorMoved{x,y}, PointerReleased, DividerPress(Divider), ShowPanel(PanelId), Maximize(Side), MaximizeClose, ZoneClicked, Resized{w,h}}`
  - `Outcome { persist: bool, switched_in: Option<PanelId> }`、`Divider::{Zones, Pair(Side)}`
  - `PanelRules { default_side, pane: Option<PaneRules> }`、`PaneRules { spec, default_split, min_first_px }`、`Ctx { metrics, rules }`
  - `model::pair_rects(content, dims, spec, mirrored, &ChromeMetrics) -> PairRects { first, second, divider }`
- 要点:**拖分隔线时保持配对里第一个 slot 的像素宽不变**(否则用户只想调左右分界,列表却跟着缩放);**点已激活的图标收起 / 展开,但对侧也收起时这一侧不能关**;**图标栏点击一律退出放大态**;**跨栏落地后源栏选中面板若正是被搬走的,退到源栏现在的第一个**。

- [ ] **Step 1: 写 `src/model.rs`(测试在末尾,先写测试看红)**

```rust
//! 骨架状态机:无界面、以 `PanelId` 为键。产品把 `ShellMsg` 交给 `update`,`update` 只改状态并
//! 告诉调用方"要不要落盘 / 哪个面板刚被切入";文件读写与面板切入动作由产品决定。

use bytehost_core::id::PanelId;
use bytehost_desktop::{PaneDims, PaneDragInput, PaneSpec, PaneStore, apply_pane_split_drag};

use std::collections::HashMap;

use crate::anim::{HoverAnim, SlotAnim};
use crate::geometry::{self, Rect, ShellRects, Side, ZoneState};
use crate::metrics::ChromeMetrics;
use crate::rail::{self, RailDrag, Rails};

/// 产品对一个面板的静态描述(骨架不理解面板含义,只按这几条规则摆放)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelRules {
    /// 默认挂在哪条栏;被拖到另一栏时配对视图左右镜像。
    pub default_side: Side,
    /// 有"列表 | 内容"配对的面板才有。
    pub pane: Option<PaneRules>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRules {
    pub spec: PaneSpec,
    pub default_split: f32,
    /// 第一个 slot 窄于它就收起(例如列表底部按钮行放得下的最小宽度)。
    pub min_first_px: f32,
}

/// `update` 需要的只读环境。
pub struct Ctx<'a> {
    pub metrics: &'a ChromeMetrics,
    pub rules: &'a dyn Fn(&PanelId) -> Option<PanelRules>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divider {
    /// 左右两个面板区之间那条恒在的分隔线。
    Zones,
    /// 某一侧当前面板的"列表 | 内容"分隔线。
    Pair(Side),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShellMsg {
    /// 在图标栏某图标上按下:选中 / 切换收起,并武装拖拽(拖拽起点取模型记录的光标位置)。
    PanelPress { panel: PanelId },
    /// 图标悬停进出(驱动颜色渐变)。
    Hover { panel: PanelId, hovered: bool },
    /// 动画推进一拍(~60fps,仅在 `animating()` 时需要)。
    Tick,
    /// 什么都不做(控件需要一个"吞掉点击"的消息时用)。
    Noop,
    /// 光标悬停到某栏第 `index` 个图标(含"末尾命中区",`index == len`)上,拖拽期间才有意义。
    RailDragMove { side: Side, index: usize },
    /// 光标移动(窗口逻辑坐标):记录光标位置,并驱动正在拖的分隔线。
    CursorMoved { x: f32, y: f32 },
    /// 松开鼠标:结束图标栏拖拽与分隔线拖拽。
    PointerReleased,
    DividerPress(Divider),
    /// 程序化"确保这个面板看得见"(绝不收起)。
    ShowPanel(PanelId),
    Maximize(Side),
    MaximizeClose,
    /// 左键按下:按模型记录的光标位置决定聚焦哪一侧。
    ZoneClicked,
    Resized { w: f32, h: f32 },
}

/// `update` 的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// 持久状态变了,调用方应落盘。
    pub persist: bool,
    /// 这个面板刚被"真切换"进来(此前该侧显示的是别的面板),产品可据此触发切入动作。
    pub switched_in: Option<PanelId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    EmptyRail(Side),
    Duplicate(PanelId),
    SelectedNotInRail(Side),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRail(s) => write!(f, "{s:?} 栏不能为空"),
            Self::Duplicate(p) => write!(f, "面板 {p} 在两条栏里重复出现"),
            Self::SelectedNotInRail(s) => write!(f, "{s:?} 侧选中的面板不在该栏里"),
        }
    }
}

impl std::error::Error for ModelError {}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellModel {
    rails: Rails,
    selected: [PanelId; 2],
    collapsed: [bool; 2],
    left_width: f32,
    window: (f32, f32),
    panes: PaneStore,
    // 瞬态,不落盘
    maximized: Option<Side>,
    active_zone: Option<Side>,
    rail_drag: Option<RailDrag>,
    divider_drag: Option<Divider>,
    cursor: (f32, f32),
    hover: HashMap<PanelId, HoverAnim>,
    slots: HashMap<PanelId, SlotAnim>,
}

impl ShellModel {
    pub fn new(rails: Rails, selected: [PanelId; 2], left_width: f32, window: (f32, f32)) -> Result<Self, ModelError> {
        for side in [Side::Left, Side::Right] {
            let items = &rails[side.index()];
            if items.is_empty() {
                return Err(ModelError::EmptyRail(side));
            }
            if !items.contains(&selected[side.index()]) {
                return Err(ModelError::SelectedNotInRail(side));
            }
        }
        for p in &rails[0] {
            if rails[1].contains(p) {
                return Err(ModelError::Duplicate(p.clone()));
            }
        }
        for side in [0, 1] {
            let mut seen = std::collections::HashSet::new();
            for p in &rails[side] {
                if !seen.insert(p) {
                    return Err(ModelError::Duplicate(p.clone()));
                }
            }
        }
        Ok(Self {
            rails,
            selected,
            collapsed: [false, false],
            left_width,
            window,
            panes: PaneStore::new(),
            maximized: None,
            active_zone: None,
            rail_drag: None,
            divider_drag: None,
            cursor: (0.0, 0.0),
            hover: HashMap::new(),
            slots: HashMap::new(),
        })
    }

    // ---- 读 ----
    pub fn rails(&self) -> &Rails {
        &self.rails
    }
    pub fn rail(&self, side: Side) -> &[PanelId] {
        &self.rails[side.index()]
    }
    pub fn selected(&self, side: Side) -> &PanelId {
        &self.selected[side.index()]
    }
    pub fn collapsed(&self, side: Side) -> bool {
        self.collapsed[side.index()]
    }
    pub fn left_width(&self) -> f32 {
        self.left_width
    }
    pub fn window(&self) -> (f32, f32) {
        self.window
    }
    pub fn panes(&self) -> &PaneStore {
        &self.panes
    }
    pub fn maximized(&self) -> Option<Side> {
        self.maximized
    }
    pub fn active_zone(&self) -> Option<Side> {
        self.active_zone
    }
    pub fn rail_drag(&self) -> Option<&RailDrag> {
        self.rail_drag.as_ref()
    }
    pub fn dragging_divider(&self) -> Option<Divider> {
        self.divider_drag
    }
    pub fn cursor(&self) -> (f32, f32) {
        self.cursor
    }
    /// 被拖拽的图标已越过阈值(不是单击)。
    pub fn rail_drag_confirmed(&self) -> bool {
        self.rail_drag.as_ref().is_some_and(|d| rail::past_threshold(d, self.cursor))
    }
    pub fn dragged_panel(&self) -> Option<PanelId> {
        rail::dragged_panel(&self.rails, self.rail_drag.as_ref())
    }
    pub fn side_of(&self, panel: &PanelId) -> Option<Side> {
        rail::side_of(&self.rails, panel)
    }

    /// 图标悬停进度(0..=1),给视图做颜色插值。
    pub fn hover_progress(&self, panel: &PanelId) -> f32 {
        self.hover.get(panel).map(HoverAnim::t).unwrap_or(0.0)
    }

    /// `panel` 在 `side` 栏此刻应渲染的浮点槽位号(逼近中的下标);没有动画记录或刚跨栏时就是目标下标本身。
    pub fn slot_position(&self, side: Side, panel: &PanelId, target_index: usize) -> f32 {
        self.slots.get(panel).and_then(|a| a.current(side)).unwrap_or(target_index as f32)
    }

    /// 还有动画在进行,产品需要继续排 `Tick`。
    pub fn animating(&self) -> bool {
        self.hover.values().any(HoverAnim::active)
            || [Side::Left, Side::Right].into_iter().any(|side| {
                self.rails[side.index()].iter().enumerate().any(|(i, p)| self.slots.get(p).is_some_and(|a| a.active(side, i as f32)))
            })
    }

    pub fn zone_state(&self) -> ZoneState {
        ZoneState {
            left_collapsed: self.collapsed[0],
            right_collapsed: self.collapsed[1],
            left_width: self.left_width,
            maximized: self.maximized,
        }
    }

    pub fn rects(&self, m: &ChromeMetrics) -> ShellRects {
        geometry::layout(self.window.0, self.window.1, &self.zone_state(), m)
    }

    /// 左面板区外框当前实际宽度(已夹取)。
    pub fn effective_left_width(&self, m: &ChromeMetrics) -> f32 {
        geometry::left_zone_width(self.window.0, &self.zone_state(), m)
    }

    // ---- 持久状态的整体写入(读盘后用) ----
    pub fn restore(&mut self, collapsed: [bool; 2], left_width: f32, panes: PaneStore) {
        self.collapsed = collapsed;
        self.left_width = left_width;
        self.panes = panes;
    }

    fn pair_geometry(&self, side: Side, m: &ChromeMetrics) -> Option<(f32, f32)> {
        let content = self.rects(m).content(side)?;
        Some((content.x, (content.w - m.divider_width).max(0.0)))
    }

    /// 某侧当前面板的配对矩形(无配对返回 `None`)。
    pub fn pair_rects(&self, side: Side, ctx: &Ctx) -> Option<PairRects> {
        let panel = self.selected(side).clone();
        let rules = (ctx.rules)(&panel)?;
        let pane = rules.pane?;
        let content = self.rects(ctx.metrics).content(side)?;
        let dims = self.panes.get_or(&panel, PaneDims::new(pane.default_split, false));
        let mirrored = side != rules.default_side;
        Some(pair_rects(content, dims, pane.spec, mirrored, ctx.metrics))
    }

    // ---- 更新 ----
    pub fn update(&mut self, msg: ShellMsg, ctx: &Ctx) -> Outcome {
        match msg {
            ShellMsg::PanelPress { panel } => self.press(panel),
            ShellMsg::Hover { panel, hovered } => {
                self.hover.entry(panel).or_default().set(hovered);
                Outcome::default()
            }
            ShellMsg::Noop => Outcome::default(),
            ShellMsg::Tick => {
                for a in self.hover.values_mut() {
                    a.advance();
                }
                for side in [Side::Left, Side::Right] {
                    for (i, p) in self.rails[side.index()].iter().enumerate() {
                        self.slots.entry(p.clone()).or_insert(SlotAnim::new(side, i as f32)).retarget(side, i as f32);
                    }
                }
                Outcome::default()
            }
            ShellMsg::RailDragMove { side, index } => {
                if let Some(mut drag) = self.rail_drag.take() {
                    rail::drag_move_into(&mut self.rails, &mut drag, side, index);
                    self.rail_drag = Some(drag);
                }
                Outcome::default()
            }
            ShellMsg::CursorMoved { x, y } => {
                self.cursor = (x, y);
                self.drag_divider(x, ctx);
                Outcome::default()
            }
            ShellMsg::PointerReleased => self.release(),
            ShellMsg::DividerPress(d) => {
                self.divider_drag = Some(d);
                Outcome::default()
            }
            ShellMsg::ShowPanel(panel) => self.show(panel),
            ShellMsg::Maximize(side) => {
                if !self.collapsed[side.index()] {
                    self.maximized = Some(side);
                }
                Outcome::default()
            }
            ShellMsg::MaximizeClose => {
                self.maximized = None;
                Outcome::default()
            }
            ShellMsg::ZoneClicked => {
                if let Some(side) = geometry::zone_at_x(self.cursor.0, self.window.0, &self.zone_state(), ctx.metrics) {
                    self.active_zone = Some(side);
                }
                Outcome::default()
            }
            ShellMsg::Resized { w, h } => {
                self.window = (w, h);
                Outcome::default()
            }
        }
    }

    fn press(&mut self, panel: PanelId) -> Outcome {
        let Some(side) = self.side_of(&panel) else {
            return Outcome::default();
        };
        let idx = self.rails[side.index()].iter().position(|p| *p == panel).expect("side_of found it");
        self.rail_drag = Some(RailDrag { source_side: side, source_index: idx, origin_index: idx, pending_cross: None, press_pos: self.cursor });
        let mut switched_in = None;
        if self.selected[side.index()] == panel {
            // 点已激活的图标:收起 / 展开;对侧也收起时,当前侧是最后一个开着的区,不能关。
            if !self.collapsed[side.other().index()] {
                self.collapsed[side.index()] = !self.collapsed[side.index()];
            }
        } else {
            self.selected[side.index()] = panel.clone();
            self.collapsed[side.index()] = false;
            switched_in = Some(panel);
        }
        // 图标栏点击一律退出放大态:遮罩不拦图标栏上的点击,不清就会留下空的描边浮层。
        self.maximized = None;
        Outcome { persist: true, switched_in }
    }

    fn release(&mut self) -> Outcome {
        let mut out = Outcome::default();
        if let Some(d) = self.divider_drag.take() {
            let _ = d;
            out.persist = true;
        }
        let Some(drag) = self.rail_drag.take() else {
            return out;
        };
        let Some((target_side, target_index)) = drag.pending_cross else {
            // 纯同栏重排:真重排过才落盘
            if drag.source_index != drag.origin_index {
                out.persist = true;
            }
            return out;
        };
        let Some(moved) = rail::cross_apply(&mut self.rails, drag.source_side, drag.source_index, target_side, target_index) else {
            return out; // 会清空源栏:挡住,栏未改动
        };
        // 源栏正显示的就是被搬走的面板:退到该栏现在的第一个,免得两侧同时显示同一面板
        if self.selected[drag.source_side.index()] == moved {
            self.selected[drag.source_side.index()] = self.rails[drag.source_side.index()][0].clone();
        }
        // 被移动的面板成为目标栏新的选中并展开
        self.selected[target_side.index()] = moved;
        self.collapsed[target_side.index()] = false;
        out.persist = true;
        out
    }

    fn show(&mut self, panel: PanelId) -> Outcome {
        let Some(side) = self.side_of(&panel) else {
            return Outcome::default();
        };
        let r = rail::show_panel_in(side, &panel, &self.selected[side.index()], self.collapsed[side.index()], self.maximized);
        if !r.changed {
            return Outcome::default();
        }
        self.selected[side.index()] = r.selected;
        self.collapsed[side.index()] = r.collapsed;
        self.maximized = r.maximized;
        Outcome { persist: true, switched_in: r.switched.then_some(panel) }
    }

    fn drag_divider(&mut self, x: f32, ctx: &Ctx) {
        match self.divider_drag {
            Some(Divider::Zones) => self.drag_zones(x, ctx),
            Some(Divider::Pair(side)) => self.drag_pair(side, x, ctx),
            None => {}
        }
    }

    fn drag_zones(&mut self, x: f32, ctx: &Ctx) {
        let m = ctx.metrics;
        // 拖动前两侧配对里"第一个 slot"的像素宽,拖完要保持不变——否则用户只想调左右分界,
        // 配对里的列表却跟着按比例缩放。
        let mut fixed_px = Vec::new();
        for side in [Side::Left, Side::Right] {
            if let (Some(rules), Some((_, pair_w))) = ((ctx.rules)(self.selected(side)), self.pair_geometry(side, m)) {
                if let Some(pane) = rules.pane {
                    let ratio = self.panes.get_or(self.selected(side), PaneDims::new(pane.default_split, false)).split;
                    fixed_px.push((side, pair_w * ratio));
                }
            }
        }
        self.left_width = geometry::clamp_left_width(self.window.0, x - m.icon_rail_width, m);
        for (side, px) in fixed_px {
            let Some((_, new_w)) = self.pair_geometry(side, m) else { continue };
            if new_w <= 0.0 {
                continue;
            }
            let panel = self.selected(side).clone();
            let ratio = (px / new_w).clamp(m.min_split_ratio, m.max_split_ratio);
            let collapsed = self.panes.get(&panel).is_some_and(|d| d.collapsed);
            self.panes.set(panel, PaneDims::new(ratio, collapsed));
        }
    }

    fn drag_pair(&mut self, side: Side, x: f32, ctx: &Ctx) {
        let m = ctx.metrics;
        let panel = self.selected(side).clone();
        let Some(rules) = (ctx.rules)(&panel) else { return };
        let Some(pane) = rules.pane else { return };
        let Some((x0, pair_w)) = self.pair_geometry(side, m) else { return };
        if pair_w <= 0.0 {
            return;
        }
        let current = self.panes.get_or(&panel, PaneDims::new(pane.default_split, false));
        let out = apply_pane_split_drag(
            current,
            pane.spec,
            side != rules.default_side,
            PaneDragInput { x0, pair_w, logical_x: x, min_ratio: m.min_split_ratio, max_ratio: m.max_split_ratio, min_first_px: pane.min_first_px },
        );
        self.panes.set(panel, PaneDims::new(out.split, out.collapsed));
    }
}

/// 配对视图的两个 slot 与中间分隔线的矩形(窗口坐标,已按镜像排好屏幕位置)。
/// 第一个 slot 收起时它宽 0、没有分隔线,第二个 slot 占满整个内容区。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairRects {
    pub first: Rect,
    pub second: Rect,
    pub divider: Rect,
}

pub fn pair_rects(content: Rect, dims: PaneDims, spec: PaneSpec, mirrored: bool, m: &ChromeMetrics) -> PairRects {
    if dims.collapsed {
        return PairRects { first: Rect::new(content.x, content.y, 0.0, content.h), second: content, divider: Rect::new(content.x, content.y, 0.0, content.h) };
    }
    let pair_w = (content.w - m.divider_width).max(0.0);
    let first_w = pair_w * dims.split;
    let second_w = pair_w - first_w;
    let first_on_left = spec.default_first_first != mirrored;
    let (left_w, right_w) = if first_on_left { (first_w, second_w) } else { (second_w, first_w) };
    let left = Rect::new(content.x, content.y, left_w, content.h);
    let divider = Rect::new(left.right(), content.y, m.divider_width, content.h);
    let right = Rect::new(divider.right(), content.y, right_w, content.h);
    if first_on_left {
        PairRects { first: left, second: right, divider }
    } else {
        PairRects { first: right, second: left, divider }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> PanelId {
        PanelId::new(s).unwrap()
    }

    fn m() -> ChromeMetrics {
        ChromeMetrics::byteboy2077()
    }

    /// t.list 是左栏默认的配对面板(列表在前,可收起);t.agent 是右栏默认的配对面板(列表在后)。
    fn rules(p: &PanelId) -> Option<PanelRules> {
        match p.as_str() {
            "t.list" => Some(PanelRules {
                default_side: Side::Left,
                pane: Some(PaneRules { spec: PaneSpec::new(true, true), default_split: 0.4, min_first_px: 150.0 }),
            }),
            "t.agent" => Some(PanelRules {
                default_side: Side::Right,
                pane: Some(PaneRules { spec: PaneSpec::new(false, true), default_split: 0.4, min_first_px: 150.0 }),
            }),
            "t.plain" | "t.a" | "t.b" | "t.x" | "t.y" => Some(PanelRules { default_side: Side::Left, pane: None }),
            _ => None,
        }
    }

    fn model() -> ShellModel {
        ShellModel::new(
            [vec![id("t.list"), id("t.plain"), id("t.a")], vec![id("t.agent"), id("t.x")]],
            [id("t.list"), id("t.agent")],
            640.0,
            (1440.0, 900.0),
        )
        .unwrap()
    }

    fn run(model: &mut ShellModel, msg: ShellMsg) -> Outcome {
        let metrics = m();
        let r = |p: &PanelId| rules(p);
        model.update(msg, &Ctx { metrics: &metrics, rules: &r })
    }

    fn press(model: &mut ShellModel, p: &str) -> Outcome {
        run(model, ShellMsg::PanelPress { panel: id(p) })
    }

    // ---- 构造不变量 ----
    #[test]
    fn construction_rejects_broken_layouts() {
        let ok = || [vec![id("t.a")], vec![id("t.x")]];
        assert!(ShellModel::new(ok(), [id("t.a"), id("t.x")], 640.0, (1440.0, 900.0)).is_ok());
        assert_eq!(ShellModel::new([vec![], vec![id("t.x")]], [id("t.a"), id("t.x")], 640.0, (1.0, 1.0)), Err(ModelError::EmptyRail(Side::Left)));
        assert_eq!(ShellModel::new([vec![id("t.a")], vec![]], [id("t.a"), id("t.x")], 640.0, (1.0, 1.0)), Err(ModelError::EmptyRail(Side::Right)));
        assert_eq!(ShellModel::new(ok(), [id("t.b"), id("t.x")], 640.0, (1.0, 1.0)), Err(ModelError::SelectedNotInRail(Side::Left)));
        assert_eq!(ShellModel::new([vec![id("t.a")], vec![id("t.a")]], [id("t.a"), id("t.a")], 640.0, (1.0, 1.0)), Err(ModelError::Duplicate(id("t.a"))));
        assert_eq!(ShellModel::new([vec![id("t.a"), id("t.a")], vec![id("t.x")]], [id("t.a"), id("t.x")], 640.0, (1.0, 1.0)), Err(ModelError::Duplicate(id("t.a"))));
    }

    // ---- 图标栏点击 ----
    #[test]
    fn pressing_another_icon_selects_it_expands_the_side_and_reports_the_switch() {
        let mut s = model();
        run(&mut s, ShellMsg::Maximize(Side::Left));
        let out = press(&mut s, "t.plain");
        assert_eq!(s.selected(Side::Left), &id("t.plain"));
        assert!(!s.collapsed(Side::Left));
        assert_eq!(out, Outcome { persist: true, switched_in: Some(id("t.plain")) });
        assert_eq!(s.maximized(), None, "a rail click always leaves the maximized state");
    }

    #[test]
    fn pressing_the_active_icon_toggles_its_zone() {
        let mut s = model();
        let out = press(&mut s, "t.list");
        assert!(s.collapsed(Side::Left));
        assert_eq!(out.switched_in, None);
        press(&mut s, "t.list");
        assert!(!s.collapsed(Side::Left));
    }

    #[test]
    fn the_last_open_zone_cannot_be_collapsed() {
        let mut s = model();
        press(&mut s, "t.agent"); // right collapses
        assert!(s.collapsed(Side::Right));
        press(&mut s, "t.list"); // left is the only open one: stays open
        assert!(!s.collapsed(Side::Left));
    }

    #[test]
    fn pressing_an_unknown_panel_does_nothing() {
        let mut s = model();
        let before = s.clone();
        assert_eq!(press(&mut s, "t.nope"), Outcome::default());
        assert_eq!(s, before);
    }

    // ---- 图标栏拖拽 ----
    #[test]
    fn a_click_without_movement_does_not_reorder_but_still_persists_the_selection() {
        let mut s = model();
        press(&mut s, "t.plain");
        let out = run(&mut s, ShellMsg::PointerReleased);
        assert_eq!(out, Outcome::default(), "the release itself has nothing to save");
        assert_eq!(s.rail(Side::Left)[1], id("t.plain"));
        assert!(s.rail_drag().is_none());
    }

    #[test]
    fn dragging_within_a_rail_reorders_live_and_persists_on_release() {
        let mut s = model();
        press(&mut s, "t.list");
        run(&mut s, ShellMsg::RailDragMove { side: Side::Left, index: 2 });
        assert_eq!(s.rail(Side::Left), [id("t.plain"), id("t.a"), id("t.list")]);
        let out = run(&mut s, ShellMsg::PointerReleased);
        assert!(out.persist);
    }

    #[test]
    fn dragging_to_the_other_rail_moves_the_panel_on_release_and_selects_it_there() {
        let mut s = model();
        press(&mut s, "t.list"); // selected on the left; collapses it, then press again to keep it open
        press(&mut s, "t.list");
        assert!(!s.collapsed(Side::Left));
        run(&mut s, ShellMsg::RailDragMove { side: Side::Right, index: 1 });
        assert_eq!(s.rail(Side::Left).len(), 3, "nothing moves while hovering");
        let out = run(&mut s, ShellMsg::PointerReleased);
        assert!(out.persist);
        assert_eq!(s.rail(Side::Right), [id("t.agent"), id("t.list"), id("t.x")]);
        assert_eq!(s.selected(Side::Right), &id("t.list"));
        assert!(!s.collapsed(Side::Right));
        // the left side was showing the moved panel: it falls back to the first one left there
        assert_eq!(s.selected(Side::Left), &id("t.plain"));
    }

    #[test]
    fn a_cross_rail_drag_that_would_empty_the_source_is_refused() {
        let mut s = ShellModel::new([vec![id("t.a")], vec![id("t.x"), id("t.y")]], [id("t.a"), id("t.x")], 640.0, (1440.0, 900.0)).unwrap();
        press(&mut s, "t.a");
        run(&mut s, ShellMsg::RailDragMove { side: Side::Right, index: 0 });
        let out = run(&mut s, ShellMsg::PointerReleased);
        assert!(!out.persist);
        assert_eq!(s.rail(Side::Left), [id("t.a")]);
    }

    #[test]
    fn the_drag_is_confirmed_only_after_the_cursor_travels() {
        let mut s = model();
        run(&mut s, ShellMsg::CursorMoved { x: 20.0, y: 20.0 });
        run(&mut s, ShellMsg::PanelPress { panel: id("t.plain") });
        assert!(!s.rail_drag_confirmed());
        run(&mut s, ShellMsg::CursorMoved { x: 21.0, y: 20.0 });
        assert!(!s.rail_drag_confirmed());
        run(&mut s, ShellMsg::CursorMoved { x: 40.0, y: 20.0 });
        assert!(s.rail_drag_confirmed());
        assert_eq!(s.dragged_panel(), Some(id("t.plain")));
    }

    // ---- 程序化显示 ----
    #[test]
    fn show_panel_never_collapses_and_reports_only_real_switches() {
        let mut s = model();
        assert_eq!(run(&mut s, ShellMsg::ShowPanel(id("t.list"))), Outcome::default(), "already visible");
        press(&mut s, "t.list"); // collapse it
        let out = run(&mut s, ShellMsg::ShowPanel(id("t.list")));
        assert!(!s.collapsed(Side::Left));
        assert_eq!(out, Outcome { persist: true, switched_in: None });
        let out = run(&mut s, ShellMsg::ShowPanel(id("t.a")));
        assert_eq!(s.selected(Side::Left), &id("t.a"));
        assert_eq!(out.switched_in, Some(id("t.a")));
    }

    // ---- 放大 ----
    #[test]
    fn maximize_is_transient_and_ignored_for_a_collapsed_side() {
        let mut s = model();
        run(&mut s, ShellMsg::Maximize(Side::Right));
        assert_eq!(s.maximized(), Some(Side::Right));
        run(&mut s, ShellMsg::MaximizeClose);
        assert_eq!(s.maximized(), None);
        press(&mut s, "t.agent"); // collapse the right side
        run(&mut s, ShellMsg::Maximize(Side::Right));
        assert_eq!(s.maximized(), None, "a collapsed side cannot be maximized");
    }

    // ---- 聚焦区与缩放 ----
    #[test]
    fn a_click_sets_the_focused_zone_but_a_click_on_a_rail_keeps_it() {
        let mut s = model();
        let click = |s: &mut ShellModel, x: f32| {
            run(s, ShellMsg::CursorMoved { x, y: 300.0 });
            run(s, ShellMsg::ZoneClicked);
        };
        click(&mut s, 900.0);
        assert_eq!(s.active_zone(), Some(Side::Right));
        click(&mut s, 10.0);
        assert_eq!(s.active_zone(), Some(Side::Right));
        click(&mut s, 300.0);
        assert_eq!(s.active_zone(), Some(Side::Left));
    }

    #[test]
    fn resizing_never_rewrites_the_preferred_left_width() {
        let mut s = model();
        run(&mut s, ShellMsg::Resized { w: 700.0, h: 600.0 });
        assert_eq!(s.left_width(), 640.0);
        assert_eq!(s.effective_left_width(&m()), 320.0);
        run(&mut s, ShellMsg::Resized { w: 1440.0, h: 900.0 });
        assert_eq!(s.effective_left_width(&m()), 640.0, "widening the window restores the preference");
    }

    // ---- 动画 ----
    #[test]
    fn hover_and_slot_animations_run_on_ticks_and_settle() {
        let mut s = model();
        assert!(!s.animating());
        run(&mut s, ShellMsg::Hover { panel: id("t.a"), hovered: true });
        assert!(s.animating());
        for _ in 0..12 {
            run(&mut s, ShellMsg::Tick);
        }
        assert_eq!(s.hover_progress(&id("t.a")), 1.0);
        assert!(!s.animating());
        // reordering slides the neighbours instead of jumping
        press(&mut s, "t.list");
        run(&mut s, ShellMsg::Tick);
        run(&mut s, ShellMsg::RailDragMove { side: Side::Left, index: 2 });
        assert!(s.animating(), "the target slot changed, the animation has not caught up yet");
        run(&mut s, ShellMsg::Tick);
        let slid = s.slot_position(Side::Left, &id("t.plain"), 0);
        assert!(slid > 0.0 && slid < 1.0, "t.plain moved from slot 1 to 0 and is on its way: {slid}");
        assert!(s.animating());
        for _ in 0..12 {
            run(&mut s, ShellMsg::Tick);
        }
        assert_eq!(s.slot_position(Side::Left, &id("t.plain"), 0), 0.0);
        assert!(!s.animating());
    }

    // ---- 左右分隔线 ----
    #[test]
    fn dragging_the_zone_divider_sets_the_left_width_clamped_and_persists_on_release() {
        let mut s = model();
        run(&mut s, ShellMsg::DividerPress(Divider::Zones));
        run(&mut s, ShellMsg::CursorMoved { x: 544.0, y: 300.0 });
        assert_eq!(s.left_width(), 500.0, "x minus the rail width");
        run(&mut s, ShellMsg::CursorMoved { x: 5.0, y: 300.0 });
        assert_eq!(s.left_width(), 320.0, "never narrower than the minimum zone");
        run(&mut s, ShellMsg::CursorMoved { x: 9000.0, y: 300.0 });
        assert_eq!(s.left_width(), 1344.0 - 320.0);
        let out = run(&mut s, ShellMsg::PointerReleased);
        assert!(out.persist);
        // moving the cursor after release changes nothing
        run(&mut s, ShellMsg::CursorMoved { x: 600.0, y: 300.0 });
        assert_eq!(s.left_width(), 1024.0);
    }

    #[test]
    fn dragging_the_zone_divider_keeps_the_pair_list_width_in_pixels() {
        let mut s = model();
        let metrics = m();
        let before = s.pair_rects(Side::Left, &Ctx { metrics: &metrics, rules: &rules }).unwrap();
        run(&mut s, ShellMsg::DividerPress(Divider::Zones));
        run(&mut s, ShellMsg::CursorMoved { x: 44.0 + 800.0, y: 300.0 });
        let after = s.pair_rects(Side::Left, &Ctx { metrics: &metrics, rules: &rules }).unwrap();
        assert!((before.first.w - after.first.w).abs() < 0.5, "list stayed {} px, got {}", before.first.w, after.first.w);
        assert!(after.second.w > before.second.w, "the content side takes the extra space");
    }

    // ---- 配对分隔线 ----
    #[test]
    fn dragging_a_pair_divider_changes_the_split_and_collapses_when_dragged_narrow() {
        let mut s = model();
        run(&mut s, ShellMsg::DividerPress(Divider::Pair(Side::Left)));
        // left content: x 51, w 632 ⇒ pair_w 624, list first. Cursor at x0 + 0.5*pair_w.
        run(&mut s, ShellMsg::CursorMoved { x: 51.0 + 312.0, y: 300.0 });
        let d = s.panes().get(&id("t.list")).unwrap();
        assert!((d.split - 0.5).abs() < 1e-3 && !d.collapsed);
        // dragged almost to the left edge: below min_first_px ⇒ collapsed, ratio frozen
        run(&mut s, ShellMsg::CursorMoved { x: 60.0, y: 300.0 });
        let d = s.panes().get(&id("t.list")).unwrap();
        assert!(d.collapsed);
        assert!((d.split - 0.5).abs() < 1e-3);
        // dragged back out: expands again
        run(&mut s, ShellMsg::CursorMoved { x: 51.0 + 200.0, y: 300.0 });
        assert!(!s.panes().get(&id("t.list")).unwrap().collapsed);
        assert!(run(&mut s, ShellMsg::PointerReleased).persist);
    }

    #[test]
    fn a_panel_without_a_pair_ignores_pair_drags() {
        let mut s = model();
        press(&mut s, "t.plain");
        run(&mut s, ShellMsg::DividerPress(Divider::Pair(Side::Left)));
        run(&mut s, ShellMsg::CursorMoved { x: 300.0, y: 300.0 });
        assert!(s.panes().is_empty());
    }

    // ---- 配对矩形 ----
    #[test]
    fn pair_rects_place_the_list_first_unless_the_panel_is_mirrored() {
        let content = Rect::new(51.0, 47.0, 632.0, 830.0);
        let d = PaneDims::new(0.4, false);
        let spec = PaneSpec::new(true, true);
        let p = pair_rects(content, d, spec, false, &m());
        // pair_w = 632 - 8 = 624; first = 249.6
        assert!((p.first.w - 249.6).abs() < 1e-3);
        assert_eq!(p.first.x, 51.0, "list on the left");
        assert!((p.divider.x - (51.0 + 249.6)).abs() < 1e-3);
        assert!((p.second.x - (p.divider.right())).abs() < 1e-3);
        assert!((p.first.w + p.divider.w + p.second.w - content.w).abs() < 1e-3);
        let mir = pair_rects(content, d, spec, true, &m());
        assert!(mir.second.x < mir.first.x, "mirrored: the list moves to the right");
        assert_eq!((mir.first.w, mir.second.w), (p.first.w, p.second.w), "widths do not depend on the mirror");
    }

    #[test]
    fn a_collapsed_pair_gives_the_whole_content_to_the_second_slot() {
        let content = Rect::new(51.0, 47.0, 632.0, 830.0);
        let p = pair_rects(content, PaneDims::new(0.4, true), PaneSpec::new(true, true), false, &m());
        assert_eq!(p.second, content);
        assert_eq!(p.first.w, 0.0);
        assert_eq!(p.divider.w, 0.0);
    }

    #[test]
    fn a_panel_whose_list_is_not_first_by_default_renders_the_list_on_the_right() {
        // t.agent: list (first slot) is rendered AFTER the content by default (default_first_first = false)
        let s = model();
        let metrics = m();
        let p = s.pair_rects(Side::Right, &Ctx { metrics: &metrics, rules: &rules }).unwrap();
        assert!(p.first.x > p.second.x, "list on the right by default");
    }
}
```

`src/lib.rs` 加:

```rust
pub mod model;

pub use model::{Ctx, Divider, Outcome, PaneRules, PairRects, PanelRules, ShellModel, ShellMsg};
```

- [ ] **Step 2: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 46 passed`(24 + 22),没有 warning。

- [ ] **Step 3: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): ShellModel, the headless shell state machine

Rail clicks, icon drags, zone and pair dividers, maximize, resize and
animation ticks, all keyed by PanelId. update() reports whether state worth
saving changed and which panel was just switched in; it never touches files.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 落盘与读盘消毒

**Files:**
- Create: `src/persist.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `model::ShellModel`、`rail::Rails`
- Produces:
  - `persist::{SCHEMA_VERSION, ShellFile, Defaults, Loaded, load, to_file, to_json, save_atomic}`
  - `Defaults { rails, selected, left_width, window, min_window }`
  - `Loaded::{Fresh(ShellModel), Restored(ShellModel), Newer{model, found}}`;`.model() / .into_model() / .may_save()`
  - `load(json: Option<&str>, known: &[PanelId], defaults: &Defaults, metrics: &ChromeMetrics) -> Loaded`
- 消毒规则(**与 Dozer 有意不同**:Dozer 对布局"不做部分修复"、任何不符就整体回落,因此它为"新增面板"专门写了迁移函数;这里改为清单里有而文件里没有的面板回到它的默认栏末尾,新版本新增面板不会重置用户布局。任一栏为空仍然整体回落默认):顶层必须是对象;丢未知 / 重复 id;选中面板不在栏里 → 取该栏第一个;窗口小于下限或非有限 → 默认;左宽非正 → 默认;分割比例不在 `[min, max]` → 丢弃该条;两侧都收起 → 展开左侧;schema 版本更高 → `Newer`(调用方不得回写)。

- [ ] **Step 1: 写 `src/persist.rs`(测试在末尾,先写测试看红)**

```rust
//! 骨架状态的落盘与读盘消毒。
//!
//! 文件是应用级偏好 JSON,字段全部 `#[serde(default)]`:缺字段补默认,而不是整份读失败把用户攒下来的
//! 宽度 / 比例全部重置。读盘后统一消毒——磁盘上的值不一定是本程序写的(手改过、别的版本写的、写了一半)。
//! 更高 schema 版本的文件**拒绝覆盖**:内存里用默认值,调用方据 [`Loaded::Newer`] 不要回写。

use std::io;
use std::path::Path;

use bytehost_core::id::PanelId;
use bytehost_desktop::{PaneDims, PaneStore};
use serde::{Deserialize, Serialize};

use crate::geometry::Side;
use crate::metrics::ChromeMetrics;
use crate::model::ShellModel;
use crate::rail::Rails;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellFile {
    pub schema_version: u32,
    pub window_width: f32,
    pub window_height: f32,
    /// 两条图标栏,面板稳定 id 的顺序。读盘时按字符串读,逐个校验后再转成 `PanelId`。
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub selected_left: Option<String>,
    pub selected_right: Option<String>,
    pub left_collapsed: bool,
    pub right_collapsed: bool,
    pub left_width: f32,
    pub panes: PaneStore,
}

impl Default for ShellFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            window_width: 0.0,
            window_height: 0.0,
            left: Vec::new(),
            right: Vec::new(),
            selected_left: None,
            selected_right: None,
            left_collapsed: false,
            right_collapsed: false,
            left_width: 0.0,
            panes: PaneStore::new(),
        }
    }
}

/// 产品给的默认值(全新安装 / 文件不可用时用)。
#[derive(Debug, Clone, PartialEq)]
pub struct Defaults {
    pub rails: Rails,
    pub selected: [PanelId; 2],
    pub left_width: f32,
    pub window: (f32, f32),
    /// 窗口尺寸下限;读到更小的值视为损坏,回落默认。
    pub min_window: (f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    /// 没有文件、解析失败或布局无法修复:用默认值,可正常回写。
    Fresh(ShellModel),
    /// 从文件恢复(可能经过消毒)。
    Restored(ShellModel),
    /// 文件的 schema 版本高于本程序支持的:内存用默认值,**不得回写**原文件。
    Newer { model: ShellModel, found: u32 },
}

impl Loaded {
    pub fn model(&self) -> &ShellModel {
        match self {
            Loaded::Fresh(m) | Loaded::Restored(m) | Loaded::Newer { model: m, .. } => m,
        }
    }

    pub fn into_model(self) -> ShellModel {
        match self {
            Loaded::Fresh(m) | Loaded::Restored(m) | Loaded::Newer { model: m, .. } => m,
        }
    }

    pub fn may_save(&self) -> bool {
        !matches!(self, Loaded::Newer { .. })
    }
}

fn default_model(d: &Defaults) -> ShellModel {
    ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).expect("产品给的默认布局必须合法")
}

fn parse_ids(raw: &[String], known: &[PanelId], seen: &mut Vec<PanelId>) -> Vec<PanelId> {
    let mut out = Vec::new();
    for s in raw {
        let Ok(id) = PanelId::new(s.clone()) else { continue };
        if known.contains(&id) && !seen.contains(&id) {
            seen.push(id.clone());
            out.push(id);
        }
    }
    out
}

fn pick_selected(rail: &[PanelId], want: &Option<String>) -> PanelId {
    want.as_ref().and_then(|w| rail.iter().find(|p| p.as_str() == w)).unwrap_or(&rail[0]).clone()
}

/// 读盘:`json` 为 `None` = 没有文件。`known` 是产品面板清单里的全部面板。
pub fn load(json: Option<&str>, known: &[PanelId], d: &Defaults, m: &ChromeMetrics) -> Loaded {
    let Some(json) = json else {
        return Loaded::Fresh(default_model(d));
    };
    // 顶层必须是对象:serde 会把 `[1,2]` 这样的数组按"位置序列"塞进结构体,把损坏文件当成已恢复。
    let Some(file) = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .filter(serde_json::Value::is_object)
        .and_then(|v| serde_json::from_value::<ShellFile>(v).ok())
    else {
        return Loaded::Fresh(default_model(d));
    };
    if file.schema_version > SCHEMA_VERSION {
        return Loaded::Newer { model: default_model(d), found: file.schema_version };
    }

    // 1. 栏位:丢未知 / 重复,清单里有而文件里没有的面板补回它的默认栏末尾。
    let mut seen = Vec::new();
    let mut rails: Rails = [parse_ids(&file.left, known, &mut seen), parse_ids(&file.right, known, &mut seen)];
    for p in known {
        if !seen.contains(p) {
            let side = if d.rails[1].contains(p) { Side::Right } else { Side::Left };
            rails[side.index()].push(p.clone());
        }
    }
    // 2. 任一栏为空 = 无法修复,整体回落默认。
    if rails[0].is_empty() || rails[1].is_empty() {
        return Loaded::Fresh(default_model(d));
    }
    let selected = [pick_selected(&rails[0], &file.selected_left), pick_selected(&rails[1], &file.selected_right)];

    // 3. 尺寸:窗口小于下限或非有限 → 默认;左宽非有限或非正 → 默认(夹取在渲染时做,不回写)。
    let ok = |v: f32, min: f32| v.is_finite() && v >= min;
    let window = if ok(file.window_width, d.min_window.0) && ok(file.window_height, d.min_window.1) { (file.window_width, file.window_height) } else { d.window };
    let left_width = if file.left_width.is_finite() && file.left_width > 0.0 { file.left_width } else { d.left_width };

    // 4. 分割比例:0 / 1 / 非有限的会让配对里的一块彻底消失(FillPortion(0)),整条丢弃回默认;
    //    只保留清单里的面板。
    let mut panes = PaneStore::new();
    for p in known {
        if let Some(PaneDims { split, collapsed }) = file.panes.get(p) {
            if split.is_finite() && split >= m.min_split_ratio && split <= m.max_split_ratio {
                panes.set(p.clone(), PaneDims::new(split, collapsed));
            }
        }
    }
    // 5. 两侧都收起 = 界面上什么都没有,展开左侧。
    let mut collapsed = [file.left_collapsed, file.right_collapsed];
    if collapsed[0] && collapsed[1] {
        collapsed[0] = false;
    }

    let Ok(mut model) = ShellModel::new(rails, selected, left_width, window) else {
        return Loaded::Fresh(default_model(d));
    };
    model.restore(collapsed, left_width, panes);
    Loaded::Restored(model)
}

pub fn to_file(model: &ShellModel) -> ShellFile {
    let ids = |side: Side| model.rail(side).iter().map(|p| p.as_str().to_owned()).collect();
    ShellFile {
        schema_version: SCHEMA_VERSION,
        window_width: model.window().0,
        window_height: model.window().1,
        left: ids(Side::Left),
        right: ids(Side::Right),
        selected_left: Some(model.selected(Side::Left).as_str().to_owned()),
        selected_right: Some(model.selected(Side::Right).as_str().to_owned()),
        left_collapsed: model.collapsed(Side::Left),
        right_collapsed: model.collapsed(Side::Right),
        left_width: model.left_width(),
        panes: model.panes().clone(),
    }
}

pub fn to_json(model: &ShellModel) -> String {
    serde_json::to_string_pretty(&to_file(model)).expect("骨架状态可序列化")
}

/// 原子落盘:先写同目录临时文件,再 rename 覆盖;中途崩溃不会留下半截文件。
pub fn save_atomic(path: &Path, model: &ShellModel) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, to_json(model))?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> PanelId {
        PanelId::new(s).unwrap()
    }

    fn m() -> ChromeMetrics {
        ChromeMetrics::byteboy2077()
    }

    fn known() -> Vec<PanelId> {
        ["t.a", "t.b", "t.c", "t.x", "t.y"].iter().map(|s| id(s)).collect()
    }

    fn defaults() -> Defaults {
        Defaults {
            rails: [vec![id("t.a"), id("t.b"), id("t.c")], vec![id("t.x"), id("t.y")]],
            selected: [id("t.a"), id("t.x")],
            left_width: 640.0,
            window: (1440.0, 900.0),
            min_window: (800.0, 480.0),
        }
    }

    fn load_str(json: &str) -> Loaded {
        load(Some(json), &known(), &defaults(), &m())
    }

    fn default_json() -> String {
        to_json(&default_model(&defaults()))
    }

    #[test]
    fn no_file_gives_the_defaults() {
        let l = load(None, &known(), &defaults(), &m());
        assert!(matches!(l, Loaded::Fresh(_)));
        assert!(l.may_save());
        assert_eq!(l.model().left_width(), 640.0);
        assert_eq!(l.model().window(), (1440.0, 900.0));
    }

    #[test]
    fn a_saved_model_round_trips_exactly() {
        let mut model = default_model(&defaults());
        let metrics = m();
        let r = |_: &PanelId| None;
        let ctx = crate::model::Ctx { metrics: &metrics, rules: &r };
        model.update(crate::model::ShellMsg::PanelPress { panel: id("t.b") }, &ctx);
        model.update(crate::model::ShellMsg::RailDragMove { side: Side::Left, index: 2 }, &ctx);
        model.update(crate::model::ShellMsg::PointerReleased, &ctx);
        model.restore([false, true], 512.0, {
            let mut p = PaneStore::new();
            p.set(id("t.b"), PaneDims::new(0.35, true));
            p
        });
        let back = load_str(&to_json(&model)).into_model();
        assert_eq!(back.rails(), model.rails());
        assert_eq!(back.selected(Side::Left), model.selected(Side::Left));
        assert_eq!(back.collapsed(Side::Right), true);
        assert_eq!(back.left_width(), 512.0);
        assert_eq!(back.panes(), model.panes());
        assert_eq!(back.window(), model.window());
    }

    #[test]
    fn corrupt_json_gives_the_defaults() {
        assert!(matches!(load_str("not json"), Loaded::Fresh(_)));
        assert!(matches!(load_str(""), Loaded::Fresh(_)));
        assert!(matches!(load_str("[1,2]"), Loaded::Fresh(_)));
    }

    #[test]
    fn a_newer_schema_is_never_overwritten() {
        let l = load_str(r#"{ "schema_version": 999, "left": ["t.a"], "right": ["t.x"] }"#);
        assert!(matches!(l, Loaded::Newer { found: 999, .. }));
        assert!(!l.may_save());
        assert_eq!(l.model().rails(), &defaults().rails, "the in-memory layout is the default one");
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_instead_of_failing_the_whole_file() {
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "left_width": 500.0 }"#);
        let model = l.model();
        assert_eq!(model.left_width(), 500.0);
        assert_eq!(model.window(), (1440.0, 900.0), "no window size in the file");
        assert_eq!(model.selected(Side::Left), &id("t.a"));
    }

    #[test]
    fn unknown_and_duplicate_ids_are_dropped_and_missing_panels_come_back_on_their_default_side() {
        // t.c and t.y are missing, t.zzz is unknown, t.a appears twice, "Bad Id" is not even a PanelId
        let l = load_str(r#"{ "left": ["t.a","t.zzz","Bad Id","t.a"], "right": ["t.b","t.x"] }"#);
        let model = l.model();
        assert_eq!(model.rail(Side::Left), [id("t.a"), id("t.c")], "t.c returns to the left, where it lives by default");
        assert_eq!(model.rail(Side::Right), [id("t.b"), id("t.x"), id("t.y")], "t.y returns to the right; the user's t.b stays right");
    }

    #[test]
    fn an_unrepairable_layout_uses_the_defaults_and_unusable_ids_leave_nothing_behind() {
        // everything is on the left and the right rail is empty: no half-repairs, the whole default layout is used
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c","t.x","t.y"], "right": [] }"#);
        assert!(matches!(l, Loaded::Fresh(_)));
        assert_eq!(l.model().rails(), &defaults().rails);
        // no usable ids at all: every panel comes back on its default side, which is the default layout again
        let l = load_str(r#"{ "left": ["nope"], "right": ["nada"] }"#);
        assert_eq!(l.model().rails(), &defaults().rails);
    }

    #[test]
    fn a_selected_panel_that_is_not_in_its_rail_becomes_the_first_one() {
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "selected_left": "t.x", "selected_right": "t.zzz" }"#);
        assert_eq!(l.model().selected(Side::Left), &id("t.a"));
        assert_eq!(l.model().selected(Side::Right), &id("t.x"));
    }

    #[test]
    fn bad_numbers_are_sanitized() {
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "window_width": 0.0, "window_height": 10.0, "left_width": -5.0 }"#);
        assert_eq!(l.model().window(), (1440.0, 900.0));
        assert_eq!(l.model().left_width(), 640.0);
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "window_width": 1000.0, "window_height": 700.0 }"#);
        assert_eq!(l.model().window(), (1000.0, 700.0));
    }

    #[test]
    fn split_ratios_outside_the_legal_range_are_dropped_so_no_pane_vanishes() {
        let mut json: serde_json::Value = serde_json::from_str(&default_json()).unwrap();
        json["panes"] = serde_json::json!({ "t.a": {"split": 0.0, "collapsed": false}, "t.b": {"split": 1.0, "collapsed": false}, "t.c": {"split": 0.5, "collapsed": true}, "t.zzz": {"split": 0.5, "collapsed": false} });
        let l = load_str(&json.to_string());
        let panes = l.model().panes();
        assert!(panes.get(&id("t.a")).is_none() && panes.get(&id("t.b")).is_none(), "0.0 and 1.0 would make one side zero-width");
        assert_eq!(panes.get(&id("t.c")), Some(PaneDims::new(0.5, true)));
        assert_eq!(panes.len(), 1, "unknown panels are not kept");
    }

    #[test]
    fn both_zones_collapsed_in_the_file_is_repaired_by_opening_the_left_one() {
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "left_collapsed": true, "right_collapsed": true }"#);
        assert!(!l.model().collapsed(Side::Left) && l.model().collapsed(Side::Right));
    }

    #[test]
    fn a_stale_old_field_in_the_file_is_ignored() {
        let l = load_str(r#"{ "left": ["t.a","t.b","t.c"], "right": ["t.x","t.y"], "files_split": 0.0, "rail_layout": {} }"#);
        assert!(matches!(l, Loaded::Restored(_)));
    }

    #[test]
    fn saving_is_atomic_and_creates_the_directory() {
        let dir = std::env::temp_dir().join(format!("chrome-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("shell.json");
        save_atomic(&path, &default_model(&defaults())).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists(), "the temp file is renamed away");
        let back = load(Some(&std::fs::read_to_string(&path).unwrap()), &known(), &defaults(), &m());
        assert!(matches!(back, Loaded::Restored(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

`src/lib.rs` 加:

```rust
pub mod persist;

pub use persist::{Defaults, Loaded};
```

- [ ] **Step 2: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 59 passed`(46 + 13)。

- [ ] **Step 3: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): shell file persistence with load-time sanitising

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 度量从 byteui 主题读出

**Files:**
- Create: `src/theme.rs`
- Modify: `src/lib.rs`、根 `Cargo.toml`(追加开发期 `[patch]`,单独一次提交)

**Interfaces:**
- Consumes: Task 1 的 `byteui::theme::region::{left_zone, right_zone}`、`byteui::theme::geometry::*`;`metrics::ChromeMetrics`
- Produces: `theme::metrics_from_theme() -> ChromeMetrics`(已含全局 UI scale;运行时每次要几何时现读)

- [ ] **Step 1: 先让 byteui 指向带新模块的 worktree(开发期 `[patch]`)**

`$ROOT/bytehost-chrome/Cargo.toml`(根)末尾追加:

```toml

# 开发期临时配置:byteui 的外壳区域样式还在 sibling worktree 里,没有发布 tag。打 tag 之前删除本段,并把
# crates/bytehost-chrome/Cargo.toml 里的 byteui 依赖改成新 tag。
[patch."https://github.com/byteboyai/byteui"]
byteui = { path = "../byteui-shell" }
```

```bash
cd $ROOT/bytehost-chrome
cargo update -p byteui 2>&1 | tail -2
git add Cargo.toml Cargo.lock
git commit -m "chore: dev-time [patch] of byteui to the sibling worktree (drop before tagging)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

Expected: `Adding byteui v0.4.3 (…/byteui-shell)`(区域样式模块所在的版本)。

- [ ] **Step 2: 写 `src/theme.rs`**

```rust
//! 把 byteui 的运行时主题读成一份 [`ChromeMetrics`]。几何函数只读这份数,控件读的是同一批 byteui 来源。

use crate::metrics::{ChromeMetrics, Insets, ZoneInsets};

fn insets(p: iced_widget::core::Padding) -> Insets {
    Insets::new(p.top, p.right, p.bottom, p.left)
}

fn zone(style: byteui::theme::region::RegionStyle) -> ZoneInsets {
    ZoneInsets { margin: insets(style.margin), padding: insets(style.padding) }
}

/// 当前主题下的度量(已含全局 UI scale)。运行时每次需要几何时现读,这样 Ctrl +/- 缩放后几何与控件一起变。
pub fn metrics_from_theme() -> ChromeMetrics {
    use byteui::theme::{geometry as g, region as r};
    ChromeMetrics {
        top_bar_height: g::top_bar_height(),
        footbar_height: g::footbar_height(),
        icon_rail_width: g::icon_rail_width(),
        divider_width: g::divider_width(),
        min_zone_width: g::min_zone_width(),
        min_split_ratio: g::min_split_ratio(),
        max_split_ratio: g::max_split_ratio(),
        maximize_padding: g::maximize_overlay_padding(),
        left_zone: zone(r::left_zone()),
        right_zone: zone(r::right_zone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ChromeMetrics::byteboy2077()` 是几何测试用的数字;它必须与 byteui 默认主题读出来的一致,否则几何测试在测一个
    /// 运行时不存在的世界。(本 crate 的测试不改 byteui 的进程级全局,所以这里读到的是默认值。)
    #[test]
    fn the_test_constants_match_what_the_default_byteui_theme_reads() {
        assert_eq!(metrics_from_theme(), ChromeMetrics::byteboy2077());
    }
}
```

`src/lib.rs` 加:

```rust
pub mod theme;

pub use theme::metrics_from_theme;
```

- [ ] **Step 3: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 60 passed`。这条测试同时保证:几何测试用的字面量 `ChromeMetrics::byteboy2077()` 与 byteui 默认主题读出来的一致。

- [ ] **Step 4: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): read ChromeMetrics from the byteui theme

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: iced 控件与事件映射

**Files:**
- Create: `src/view/mod.rs`、`src/view/divider.rs`、`src/view/pair.rs`、`src/view/rail.rs`、`src/view/zone.rs`、`src/view/maximize.rs`、`src/view/topbar.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `model::*`、`geometry::*`、`byteui::theme::region::*`、`byteui::interaction::icons::{icon_button_entry, view, IconKind}`
- Produces:
  - `view::ShellHost<M: Clone + 'static>` trait:`lift(ShellMsg) -> M`(关联函数)、`meta(&self, &PanelId) -> Option<PanelMeta>`、`rules(&self, &PanelId) -> Option<PanelRules>`、`body<'a>(&'a self, &PanelId, &BodyArea) -> El<'a, M>`、`top_bar<'a>(&'a self) -> TopBarSlots<'a, M>`、`footbar<'a>(&'a self) -> El<'a, M>`、`on_top_bar_double_click(&self) -> M`
  - `view::view(&ShellModel, &host, &ChromeMetrics) -> El<M>` —— 整个窗口的根视图
  - `view::{divider_bar, pair_view, map_event}`、`view::{PanelMeta, BodyArea, TopBarSlots, El}`
  - `map_event(&Event) -> Option<ShellMsg>`:产品在自己的 `listen_with` 里调用;**窗口失去焦点映射成 `PointerReleased`**(Review Focus 1)
- 控件要点(都是 Dozer 踩过的坑,代码注释里有原因):图标栏按钮用 `MouseArea::on_press` 而不是内层 `button::on_press`(后者松手才触发,武装拖拽就晚了);配对两块宽度由各自外层容器声明 `FillPortion`,不再套包装容器;放大浮层的描边盒显式 `Length::Fill`;放大内容罩一层吃掉点击 / 滚轮的 `MouseArea`。

- [ ] **Step 1: 写七个文件**

`src/view/mod.rs`:

```rust
//! 骨架的 iced 控件。产品实现 [`ShellHost`],骨架负责 rail / 分隔线 / 区域外框 / 放大浮层 / 顶栏外壳 / 底栏槽。
//!
//! 控件返回 `Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>`(与 byteui 的签名一致),
//! 不额外开渲染器 feature。产品用 [`ShellHost::lift`] 把 [`ShellMsg`] 包进自己的消息枚举。

mod divider;
mod maximize;
mod pair;
mod rail;
mod topbar;
mod zone;

use bytehost_core::id::PanelId;
use byteui::interaction::icons::IconKind;
use iced_widget::core::{Element, Length};
use iced_widget::{column, container, row, stack};

use crate::geometry::{Rect, Side};
use crate::metrics::ChromeMetrics;
use crate::model::{Divider, PairRects, PanelRules, ShellModel, ShellMsg};

pub use divider::divider_bar;
pub use pair::pair_view;
pub use topbar::TopBarSlots;

pub type El<'a, M> = Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>;

/// 一个面板在图标栏上的呈现。
#[derive(Debug, Clone, PartialEq)]
pub struct PanelMeta {
    pub title: String,
    pub icon: IconKind,
}

/// 面板 body 被放进哪个区、占多大。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyArea {
    pub side: Side,
    /// 面板内容矩形(窗口坐标);放大态下就是放大盒子。
    pub rect: Rect,
    /// 面板是配对面板时的两个 slot 矩形;宽度由骨架按 split 与镜像算好。
    pub pair: Option<PairRects>,
    pub mirrored: bool,
    /// 配对的 split(第一个 slot 的占比);收起时 `collapsed`。
    pub split: f32,
    pub collapsed: bool,
}

pub trait ShellHost<M: Clone + 'static> {
    /// 把骨架消息包进产品消息。
    fn lift(msg: ShellMsg) -> M
    where
        Self: Sized;
    fn meta(&self, panel: &PanelId) -> Option<PanelMeta>;
    fn rules(&self, panel: &PanelId) -> Option<PanelRules>;
    /// 渲染某面板的 body。配对面板用 [`pair_view`] 排版自己的两个 slot。
    fn body<'a>(&'a self, panel: &PanelId, area: &BodyArea) -> El<'a, M>;
    fn top_bar<'a>(&'a self) -> TopBarSlots<'a, M>;
    fn footbar<'a>(&'a self) -> El<'a, M>;
    /// 双击顶栏空白处(原生标题栏没了,缩放窗口的手势要自己补)。
    fn on_top_bar_double_click(&self) -> M;
}

fn body_area<M: Clone + 'static, H: ShellHost<M>>(model: &ShellModel, host: &H, side: Side, rects: &crate::geometry::ShellRects, m: &ChromeMetrics) -> Option<BodyArea> {
    use bytehost_desktop::PaneDims;
    let rect = rects.content(side)?;
    let panel = model.selected(side);
    let rules = host.rules(panel);
    let pane = rules.and_then(|r| r.pane.map(|p| (r, p)));
    let (pair, mirrored, split, collapsed) = match pane {
        Some((r, p)) => {
            let dims = model.panes().get_or(panel, PaneDims::new(p.default_split, false));
            let mirrored = side != r.default_side;
            (Some(crate::model::pair_rects(rect, dims, p.spec, mirrored, m)), mirrored, dims.split, dims.collapsed)
        }
        None => (None, false, 0.5, false),
    };
    Some(BodyArea { side, rect, pair, mirrored, split, collapsed })
}

/// 整个窗口的根视图:顶栏 / 两条图标栏 / 两个面板区 / 分隔线 / 底栏,以及放大浮层与拖拽幽灵图标。
pub fn view<'a, M, H>(model: &'a ShellModel, host: &'a H, m: &ChromeMetrics) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let rects = model.rects(m);
    let top = topbar::top_bar(host.top_bar(), host.on_top_bar_double_click(), m);
    let left = zone::zone_area::<M, H>(model, host, Side::Left, body_area(model, host, Side::Left, &rects, m), m);
    let right = zone::zone_area::<M, H>(model, host, Side::Right, body_area(model, host, Side::Right, &rects, m), m);
    let body = row![
        rail::icon_rail::<M, H>(model, host, Side::Left, m),
        column![
            row![left, divider_bar(Divider::Zones, byteui::theme::color::current().bg, byteui::theme::color::current().bg, H::lift(ShellMsg::DividerPress(Divider::Zones))), right].height(Length::Fill),
            container(host.footbar()).width(Length::Fill).height(Length::Fixed(m.footbar_height)),
        ]
        .width(Length::Fill),
        rail::icon_rail::<M, H>(model, host, Side::Right, m),
    ];
    let base: El<'a, M> = column![top, body].into();

    let mut layers: Vec<El<'a, M>> = vec![base];
    if let Some(side) = model.maximized() {
        if let Some(area) = body_area(model, host, side, &rects, m) {
            layers.push(maximize::maximize_overlay::<M, H>(host, model.selected(side), &area, m));
        }
    }
    if model.rail_drag_confirmed() {
        layers.push(rail::rail_drag_ghost::<M, H>(model, host));
    }
    if layers.len() == 1 {
        return layers.pop().expect("one layer");
    }
    stack(layers).width(Length::Fill).height(Length::Fill).into()
}

/// 把 iced 事件映射成骨架消息(产品在自己的 `listen_with` 里调用,非捕获闭包即可)。
pub fn map_event(event: &iced_widget::core::Event) -> Option<ShellMsg> {
    use iced_widget::core::{Event, mouse, window};
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => Some(ShellMsg::CursorMoved { x: position.x, y: position.y }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => Some(ShellMsg::ZoneClicked),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(ShellMsg::PointerReleased),
        Event::Window(window::Event::Resized(size)) => Some(ShellMsg::Resized { w: size.width, h: size.height }),
        // 拖着图标 / 分隔线时窗口失去焦点,鼠标释放事件可能永远不会送达:当作松手,免得拖拽状态一直悬着,
        // 之后光标划过图标栏就把图标误重排。
        Event::Window(window::Event::Unfocused) => Some(ShellMsg::PointerReleased),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Ctx, PaneRules};
    use bytehost_desktop::PaneSpec;

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum Msg {
        Shell(ShellMsg),
        Zoom,
    }

    struct Host;

    fn id(s: &str) -> PanelId {
        PanelId::new(s).unwrap()
    }

    impl ShellHost<Msg> for Host {
        fn lift(msg: ShellMsg) -> Msg {
            Msg::Shell(msg)
        }
        fn meta(&self, panel: &PanelId) -> Option<PanelMeta> {
            Some(PanelMeta { title: panel.as_str().to_owned(), icon: IconKind::Home })
        }
        fn rules(&self, panel: &PanelId) -> Option<PanelRules> {
            (panel.as_str() == "t.list").then_some(PanelRules {
                default_side: Side::Left,
                pane: Some(PaneRules { spec: PaneSpec::new(true, true), default_split: 0.4, min_first_px: 150.0 }),
            })
        }
        fn body<'a>(&'a self, panel: &PanelId, area: &BodyArea) -> El<'a, Msg> {
            match area.pair {
                Some(_) => {
                    let bg = byteui::theme::color::current().bg;
                    pair_view(area, iced_widget::text("list").into(), iced_widget::text("content").into(), Self::lift(ShellMsg::DividerPress(Divider::Pair(area.side))), bg, bg)
                }
                None => iced_widget::text(panel.as_str().to_owned()).into(),
            }
        }
        fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
            TopBarSlots { title: iced_widget::text("T").into(), tabs: iced_widget::text("tabs").into(), right: iced_widget::text("R").into() }
        }
        fn footbar<'a>(&'a self) -> El<'a, Msg> {
            iced_widget::text("foot").into()
        }
        fn on_top_bar_double_click(&self) -> Msg {
            Msg::Zoom
        }
    }

    fn model() -> ShellModel {
        ShellModel::new([vec![id("t.list"), id("t.a")], vec![id("t.x"), id("t.y")]], [id("t.list"), id("t.x")], 640.0, (1440.0, 900.0)).unwrap()
    }

    fn step(s: &mut ShellModel, msg: ShellMsg) {
        let metrics = ChromeMetrics::byteboy2077();
        let rules = |p: &PanelId| Host.rules(p);
        s.update(msg, &Ctx { metrics: &metrics, rules: &rules });
    }

    /// Building the element tree must never panic, whatever state the shell is in.
    #[test]
    fn the_view_builds_in_every_shell_state() {
        let m = ChromeMetrics::byteboy2077();
        let host = Host;
        let mut s = model();
        let _ = view(&s, &host, &m);
        // collapsed sides
        step(&mut s, ShellMsg::PanelPress { panel: id("t.list") });
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::PanelPress { panel: id("t.list") });
        step(&mut s, ShellMsg::PanelPress { panel: id("t.x") });
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::PanelPress { panel: id("t.x") });
        // maximized, each side
        step(&mut s, ShellMsg::Maximize(Side::Left));
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::Maximize(Side::Right));
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::MaximizeClose);
        // a collapsed pair, a mirrored pair, a rail drag in progress
        step(&mut s, ShellMsg::DividerPress(Divider::Pair(Side::Left)));
        step(&mut s, ShellMsg::CursorMoved { x: 60.0, y: 300.0 });
        step(&mut s, ShellMsg::PointerReleased);
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::CursorMoved { x: 20.0, y: 80.0 });
        step(&mut s, ShellMsg::PanelPress { panel: id("t.a") });
        step(&mut s, ShellMsg::CursorMoved { x: 400.0, y: 400.0 });
        step(&mut s, ShellMsg::RailDragMove { side: Side::Right, index: 1 });
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::PointerReleased);
        let _ = view(&s, &host, &m);
        step(&mut s, ShellMsg::Resized { w: 300.0, h: 200.0 });
        let _ = view(&s, &host, &m);
    }

    #[test]
    fn events_map_to_shell_messages() {
        use iced_widget::core::{Event, Point, Size, mouse, window};
        assert_eq!(map_event(&Event::Mouse(mouse::Event::CursorMoved { position: Point::new(3.0, 4.0) })), Some(ShellMsg::CursorMoved { x: 3.0, y: 4.0 }));
        assert_eq!(map_event(&Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))), Some(ShellMsg::ZoneClicked));
        assert_eq!(map_event(&Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))), None);
        assert_eq!(map_event(&Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))), Some(ShellMsg::PointerReleased));
        assert_eq!(map_event(&Event::Window(window::Event::Resized(Size::new(800.0, 600.0)))), Some(ShellMsg::Resized { w: 800.0, h: 600.0 }));
        assert_eq!(map_event(&Event::Window(window::Event::Focused)), None);
        assert_eq!(map_event(&Event::Window(window::Event::Unfocused)), Some(ShellMsg::PointerReleased), "losing focus mid-drag must end the drag");
    }
}
```

`src/view/divider.rs`:

```rust
use iced_widget::core::{Color, Length, mouse};
use iced_widget::{MouseArea, container, row};

use super::El;
use crate::model::Divider;

/// 分隔线:命中区 `divider_width` 宽、`Fill` 高,中间一条 2px 边框色竖线。
///
/// 配对视图内部:`left_bg` / `right_bg` 是分隔线两侧紧贴的 pane 底色,命中区左右两半分别填上这两色,只留中间
/// 2px 竖线——否则 8px 命中区是透明的,会露出 zone 的底色,在两块 pane 之间顶出一条浅色"沟"。
/// `Divider::Zones` 不画竖线也不填色:它两侧各自套着整体外框,这条缝是故意空出来给两侧圆角边框收边的。
///
/// `on_press` 只发起拖拽;持续追踪交给全局的 `CursorMoved` / `PointerReleased`(`MouseArea` 的 `on_move` 要求光标
/// 不离开这条窄带,快速拖拽会断)。
pub fn divider_bar<'a, M: Clone + 'a>(kind: Divider, left_bg: Color, right_bg: Color, on_press: M) -> El<'a, M> {
    let width = byteui::theme::geometry::divider_width();
    let body: El<'a, M> = if matches!(kind, Divider::Zones) {
        iced_widget::Space::new().width(Length::Fixed(width)).height(Length::Fill).into()
    } else {
        let line_w = 2.0_f32;
        let side_w = (width - line_w) / 2.0;
        let fill = |c: Color| {
            container(iced_widget::Space::new()).width(Length::Fixed(side_w)).height(Length::Fill).style(move |_t: &iced_widget::Theme| container::Style { background: Some(c.into()), ..container::Style::default() })
        };
        let line = container(iced_widget::Space::new())
            .width(Length::Fixed(line_w))
            .height(Length::Fill)
            .style(|_t: &iced_widget::Theme| container::Style { background: Some(byteui::theme::color::current().border.into()), ..container::Style::default() });
        row![fill(left_bg), line, fill(right_bg)].width(Length::Fixed(width)).height(Length::Fill).into()
    };
    MouseArea::new(body).interaction(mouse::Interaction::ResizingColumn).on_press(on_press).into()
}
```

`src/view/pair.rs`:

```rust
use iced_widget::core::Length;
use iced_widget::{container, row};

use super::{BodyArea, El};
use crate::model::Divider;

/// 配对视图的 `FillPortion` 权重。`FillPortion` 在扣掉中间固定宽的分隔线之后按权重分剩余空间,
/// 与几何里的 `pair_w = content.w - divider_width` 同源。
pub fn split_portions(split: f32) -> (u16, u16) {
    let first = (split * 10_000.0).round() as u16;
    let second = ((1.0 - split) * 10_000.0).round() as u16;
    (first, second)
}

/// 把配对面板的两个 slot 排成"列表 | 分隔线 | 内容"(或镜像)。第一个 slot 收起时只渲染第二个,占满整个内容区。
///
/// 两块的宽度由它们**自己的外层容器**声明成 `FillPortion`,不再套一层包装容器:`Limits::width(Fixed(w))` 会把子元素的
/// min/max 都钉成 `w`,父级的 `FillPortion` 只约束包装容器本身、传不进子元素,曾导致两块 pane 全部以 0 宽布局。
pub fn pair_view<'a, M: Clone + 'a>(area: &BodyArea, first: El<'a, M>, second: El<'a, M>, on_divider_press: M, first_bg: iced_widget::core::Color, second_bg: iced_widget::core::Color) -> El<'a, M> {
    if area.collapsed {
        return container(second).width(Length::Fill).height(Length::Fill).into();
    }
    let (p_first, p_second) = split_portions(area.split);
    let first = container(first).width(Length::FillPortion(p_first)).height(Length::Fill);
    let second = container(second).width(Length::FillPortion(p_second)).height(Length::Fill);
    // 第一个 slot 此刻是否渲染在左边:几何里已经按镜像算好,直接读,不重复推导。
    let first_on_left = area.pair.map(|p| p.first.x <= p.second.x).unwrap_or(true);
    if first_on_left {
        row![first, super::divider_bar(Divider::Pair(area.side), first_bg, second_bg, on_divider_press), second].height(Length::Fill).into()
    } else {
        row![second, super::divider_bar(Divider::Pair(area.side), second_bg, first_bg, on_divider_press), first].height(Length::Fill).into()
    }
}
```

`src/view/rail.rs`:

```rust
use bytehost_core::id::PanelId;
use byteui::interaction::icons;
use iced_widget::core::{Border, Length, Padding, alignment, mouse};
use iced_widget::{MouseArea, Stack, container};

use super::{El, ShellHost};
use crate::geometry::Side;
use crate::metrics::ChromeMetrics;
use crate::model::{ShellModel, ShellMsg};

/// 图标栏:按 `model.rail(side)` 的顺序渲染。左右两条栏共用这一份实现,差异(区域样式、选中态取哪一侧)
/// 通过 `side` 分派。每个按钮占一个绝对定位的 `Stack` 图层,纵向偏移按动画槽位号换算,换位时相邻按钮平滑让位。
pub(super) fn icon_rail<'a, M, H>(model: &'a ShellModel, host: &'a H, side: Side, m: &ChromeMetrics) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let region = match side {
        Side::Left => byteui::theme::region::left_icon_rail(),
        Side::Right => byteui::theme::region::right_icon_rail(),
    };
    // 视觉"选中" = 该侧选中它 **且**面板区展开。点已选中的图标会收起面板区,此时图标退回未选中态。
    let active = model.selected(side).clone();
    let open = !model.collapsed(side);
    let button_size = byteui::theme::geometry::rail_button_size();
    let step = button_size + region.gap;
    let confirmed = model.rail_drag_confirmed();
    let dragged = model.dragged_panel();

    let mut layers: Vec<El<'a, M>> = Vec::new();
    for (idx, panel) in model.rail(side).iter().enumerate() {
        let Some(meta) = host.meta(panel) else { continue };
        let press = H::lift(ShellMsg::PanelPress { panel: panel.clone() });
        let hover_panel = panel.clone();
        // `interactive: false`:选中走外层 `rail_drag_surface` 的 `MouseArea::on_press`(按下瞬间触发),
        // 不走内层 `button::on_press`(松手才触发,武装拖拽的时机就晚了)。
        let base: El<'a, M> = icons::icon_button_entry(
            meta.icon,
            byteui::theme::icon_size::rail(),
            *panel == active && open,
            confirmed && dragged.as_ref() == Some(panel),
            model.hover_progress(panel),
            true,
            button_size,
            false,
            press.clone(),
            move |hovered| H::lift(ShellMsg::Hover { panel: hover_panel.clone(), hovered }),
            meta.title,
        );
        let y = region.padding.top + model.slot_position(side, panel, idx) * step;
        layers.push(positioned(drag_surface::<M, H>(base, side, idx, confirmed, press), y, &region));
    }

    // 补一块"末尾"命中区:每个按钮的 `on_move` 只挂在按钮自己的矩形上,悬停能上报的下标上限永远是
    // len - 1,跨栏拖拽永远到不了真正的末位;这块透明区从最后一个按钮往下铺满,上报下标固定为 len。
    let tail_index = model.rail(side).len();
    let tail = MouseArea::new(iced_widget::Space::new().width(Length::Fill).height(Length::Fill)).on_move(move |_| H::lift(ShellMsg::RailDragMove { side, index: tail_index }));
    layers.push(positioned(tail.into(), region.padding.top + tail_index as f32 * step, &region));

    // 拖拽悬停期间在预计插入点画一条金色插入线(同栏重排 = 已生效的顺序;跨栏悬停 = 将要落的位置),只在越过阈值后画。
    let insertion = model.rail_drag().filter(|_| confirmed).and_then(|d| match d.pending_cross {
        Some((cross_side, idx)) => (cross_side == side).then_some(idx),
        None => (d.source_side == side).then_some(d.source_index),
    });
    if let Some(idx) = insertion {
        let bar_h = 3.0;
        let y = (region.padding.top + idx as f32 * step - region.gap / 2.0 - bar_h / 2.0).max(0.0);
        let marker = container(iced_widget::Space::new()).width(Length::Fixed(button_size)).height(Length::Fixed(bar_h)).style(|_t: &iced_widget::Theme| container::Style {
            background: Some(byteui::theme::color::current().gold.into()),
            ..container::Style::default()
        });
        layers.push(positioned(marker.into(), y, &region));
    }

    let _ = m;
    container(Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(byteui::theme::geometry::icon_rail_width()))
        .height(Length::Fill)
        .style(move |_t: &iced_widget::Theme| container::Style { background: region.background.map(Into::into), border: region.border.unwrap_or_default(), ..container::Style::default() })
        .into()
}

fn positioned<'a, M: 'a>(content: El<'a, M>, y: f32, region: &byteui::theme::region::RegionStyle) -> El<'a, M> {
    container(content)
        .padding(Padding { top: y, left: region.padding.left, right: region.padding.right, bottom: 0.0 })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Left)
        .align_y(alignment::Vertical::Top)
        .into()
}

/// 给一个图标栏按钮包上"拖拽换栏 / 换位"的感应层。这一层是按钮**唯一**的选中 / 拖拽入口:
/// `MouseArea::on_press`(按下瞬间触发)武装拖拽;`on_move`:光标移到这个按钮上时上报悬停下标,没在拖拽时是无害的 no-op。
fn drag_surface<'a, M, H>(content: El<'a, M>, side: Side, index: usize, armed: bool, on_press: M) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let area = MouseArea::new(content).on_press(on_press).on_move(move |_| H::lift(ShellMsg::RailDragMove { side, index }));
    if armed { area.interaction(mouse::Interaction::Grabbing).into() } else { area.into() }
}

/// 图标栏拖拽期间跟随光标的幽灵图标:圆角方卡 + 金框,里面是被拖面板的图标,整体以光标为中心悬浮,纯展示。
pub(super) fn rail_drag_ghost<'a, M, H>(model: &'a ShellModel, host: &'a H) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let empty = || iced_widget::column![].into();
    if !model.rail_drag_confirmed() {
        return empty();
    }
    let Some(panel): Option<PanelId> = model.dragged_panel() else { return empty() };
    let Some(meta) = host.meta(&panel) else { return empty() };
    let size = byteui::theme::geometry::rail_button_size();
    let colors = byteui::theme::color::current();
    let ghost = container(icons::view(meta.icon, byteui::theme::icon_size::rail(), colors.gold))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(move |_t: &iced_widget::Theme| container::Style { background: Some(colors.card.into()), border: Border { color: colors.gold, width: 1.0, radius: 8.0.into() }, ..container::Style::default() });
    // 以光标为中心,并钳制在窗口范围内,避免贴着窗口边缘拖拽时图标一半画到窗口外。
    let (cx, cy) = model.cursor();
    let (w, h) = model.window();
    let x = (cx - size / 2.0).clamp(0.0, (w - size).max(0.0));
    let y = (cy - size / 2.0).clamp(0.0, (h - size).max(0.0));
    container(ghost).width(Length::Fill).height(Length::Fill).padding(Padding { top: y, left: x, right: 0.0, bottom: 0.0 }).into()
}
```

`src/view/zone.rs`:

```rust
use iced_widget::core::{Border, Length, Padding};
use iced_widget::{column, container};

use super::{BodyArea, El, ShellHost};
use crate::geometry::Side;
use crate::metrics::ChromeMetrics;
use crate::model::ShellModel;

/// 一侧的面板区。收起时渲染成空元素;非放大态带整体外框(`left_zone` / `right_zone`),本区拿到焦点时描金边。
///
/// 左区在两侧都展开时宽度是 `Fixed(有效左宽)`,对侧收起时 `Fill`(独占);右区恒 `Fill`。
/// 这与几何函数 `left_zone_width` / `right_zone_width` 的规则一一对应——控件和几何不得各自推导。
pub(super) fn zone_area<'a, M, H>(model: &'a ShellModel, host: &'a H, side: Side, area: Option<BodyArea>, m: &ChromeMetrics) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let other = side.other();
    if model.collapsed(side) {
        // 左收起:对侧也收起时占满(空),否则缩成零宽,让右区拿走全部空间。
        return if side == Side::Left && model.collapsed(other) { iced_widget::space::horizontal().into() } else { column![].into() };
    }
    let Some(area) = area else { return column![].into() };
    let inner = host.body(model.selected(side), &area);

    let region = match side {
        Side::Left => byteui::theme::region::left_zone(),
        Side::Right => byteui::theme::region::right_zone(),
    };
    let focused = model.active_zone() == Some(side);
    let base_border = region.border.unwrap_or_default();
    let zone_box = container(inner).width(Length::Fill).height(Length::Fill).padding(region.padding).clip(true).style(move |_t: &iced_widget::Theme| container::Style {
        background: region.background.map(Into::into),
        border: if focused { Border { color: byteui::theme::color::current().gold, width: 2.0, radius: base_border.radius } } else { base_border },
        ..container::Style::default()
    });

    let width = match side {
        Side::Left if !model.collapsed(Side::Right) => Length::Fixed(model.effective_left_width(m)),
        _ => Length::Fill,
    };
    let mg = region.margin;
    container(zone_box).width(width).height(Length::Fill).padding(Padding { top: mg.top, right: mg.right, bottom: mg.bottom, left: mg.left }).into()
}
```

`src/view/maximize.rs`:

```rust
use bytehost_core::id::PanelId;
use iced_widget::core::Length;
use iced_widget::{MouseArea, column, container, row};

use super::{BodyArea, El, ShellHost};
use crate::metrics::ChromeMetrics;
use crate::model::ShellMsg;

/// 放大态浮层:两条图标栏之间、顶栏之下的整个内容区变暗,放大的那一侧内容占满并描金边。点变暗区域退出放大。
///
/// `bordered` 显式给 `Length::Fill`:iced 0.14 的 `Limits` 有 compression 传染机制,`Shrink` 容器包 `Fill` 子元素时
/// Fill 会退化成贴着内容收缩,金色描边就会贴着一小块内容而不是撑满放大区域。
pub(super) fn maximize_overlay<'a, M, H>(host: &'a H, panel: &PanelId, area: &BodyArea, m: &ChromeMetrics) -> El<'a, M>
where
    M: Clone + 'static,
    H: ShellHost<M>,
{
    let style = byteui::theme::region::maximize_overlay();
    let inner = host.body(panel, area);
    let bordered = container(inner).width(Length::Fill).height(Length::Fill).style(move |_t: &iced_widget::Theme| container::Style { border: style.border, ..container::Style::default() });
    // 放大内容自己再罩一层"吃掉点击 / 滚轮"的 MouseArea:iced 事件子先父后,内容里真正可交互的控件会先 capture,
    // 到得了这一层的正是"不消费点击的地方",不吃掉就会穿到外层遮罩的"退出放大",一点正文就退出。
    let noop = H::lift(ShellMsg::Noop);
    let scroll_noop = noop.clone();
    let guard = MouseArea::new(bordered).on_press(noop).on_scroll(move |_delta| scroll_noop.clone());
    let dim = MouseArea::new(container(guard).padding(style.scrim_padding).width(Length::Fill).height(Length::Fill).style(move |_t: &iced_widget::Theme| container::Style { background: Some(style.scrim_background.into()), ..container::Style::default() }))
        .on_press(H::lift(ShellMsg::MaximizeClose));
    // 顶部垫一条顶栏高的透明 Space,把遮罩钉在顶栏之下;两侧垫图标栏宽,让变暗区域精确对齐 body 的渲染范围。
    column![
        iced_widget::Space::new().height(Length::Fixed(m.top_bar_height)),
        row![iced_widget::Space::new().width(Length::Fixed(m.icon_rail_width)), dim, iced_widget::Space::new().width(Length::Fixed(m.icon_rail_width))],
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
```

`src/view/topbar.rs`:

```rust
use iced_widget::core::{Alignment, Length};
use iced_widget::{MouseArea, container, row, stack};

use super::El;
use crate::metrics::ChromeMetrics;

/// 顶栏三个槽位:左侧标题(恒在最左,不参与收窄)、中间页签行(占满剩余空间)、右侧按钮。
pub struct TopBarSlots<'a, M> {
    pub title: El<'a, M>,
    pub tabs: El<'a, M>,
    pub right: El<'a, M>,
}

/// 顶栏外壳:固定 `top_bar_height` 高,区域样式 `top_bar`(左内边距 78 给红绿灯让位)。
/// 双击顶栏空白处缩放窗口:背景 `MouseArea` 放在 `bar` 下面一层,真正的按钮 / 页签会先吃掉落在自己身上的点击,
/// 双击只有落在没有任何控件的空白处才会穿透到这层背景。
pub(super) fn top_bar<'a, M: Clone + 'a>(slots: TopBarSlots<'a, M>, on_double_click: M, m: &ChromeMetrics) -> El<'a, M> {
    let region = byteui::theme::region::top_bar();
    let tabs = container(slots.tabs).width(Length::Fill);
    let bar = row![slots.title, tabs, slots.right].spacing(region.gap).padding(region.padding).height(Length::Fixed(m.top_bar_height)).align_y(Alignment::Center);
    let background = MouseArea::new(container(iced_widget::Space::new()).width(Length::Fill).height(Length::Fill)).on_double_click(on_double_click);
    container(stack![background, bar])
        .width(Length::Fill)
        .height(Length::Fixed(m.top_bar_height))
        .style(move |_t: &iced_widget::Theme| container::Style { background: region.background.map(Into::into), border: region.border.unwrap_or_default(), ..container::Style::default() })
        .into()
}
```

`src/lib.rs` 加:

```rust
pub mod view;

pub use view::{BodyArea, El, PanelMeta, ShellHost, TopBarSlots, map_event, pair_view, view};
```

- [ ] **Step 2: 运行**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
```

Expected: `test result: ok. 62 passed`(60 + 2:`the_view_builds_in_every_shell_state`、`events_map_to_shell_messages`),没有 warning。

- [ ] **Step 3: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): iced widgets for the shell and the event mapping

ShellHost is the whole interface a product implements. Losing window focus
ends a drag, so a release that never arrives cannot leave an icon drag armed.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 10: macOS 窗口装饰(红绿灯居中、全屏)与最终 `lib.rs`

**Files:**
- Create: `src/platform_macos.rs`
- Modify: `src/lib.rs`(写成最终版)

**Interfaces:**
- Produces(仅 `target_os = "macos"`):
  - `platform_macos::traffic_light_origin_y(bounds_origin_y, bounds_height, button_height, top_bar_height, flipped) -> f64`(纯函数)
  - `platform_macos::center_traffic_lights(window: &dyn HasWindowHandle, top_bar_height: f32)` —— 把三个红绿灯垂直居中到自绘顶栏里;放在 `Resized` 里重设,幂等
  - `platform_macos::install_fullscreen_guard(window: &dyn HasWindowHandle)` —— 原生全屏不绘制标题栏,红绿灯会跟着移出窗口;全屏期间把同一组原生按钮临时挂到内容视图
- **有意没有移植** Dozer 的"顶栏拖窗保护"(覆写 `mouseDownCanMoveWindow`):S1 的顶栏没有任何可交互控件,不需要;顶栏有了项目页签(S5)时再移植并加真实窗口验证。

- [ ] **Step 1: 写 `src/platform_macos.rs`**

```rust
//! macOS 窗口装饰胶水(移植自 Dozer `platform/window.rs`):窗口用"透明标题栏 + 全尺寸内容视图",顶栏取代系统标题栏后,
//! 要自己处理两件事:
//! 1. 红绿灯垂直居中到顶栏里(系统仍按 28pt 旧标题栏排版,会贴着顶栏上沿);
//! 2. 原生全屏不绘制标题栏,红绿灯会跟着移出窗口:全屏期间把同一组原生按钮临时挂到内容视图。
//!
//! 与 Dozer 的区别:Dozer 自己持有 winit 窗口;这里经 iced 的 `window::run` 拿 `&dyn HasWindowHandle`。
//!
//! **没有移植 Dozer 的"顶栏拖窗保护"**(覆写 `mouseDownCanMoveWindow`):透明标题栏保留 ~28pt 的原生拖窗带,落在带内的
//! 按下事件在送到 iced 之前就被 AppKit 当拖窗,顶栏上可交互的页签 / 按钮会被吞。S1 的顶栏没有任何可交互控件
//! (标题只是展示,"＋"置灰),不需要它;顶栏有了项目页签(S5)时再移植,并在那时加真实窗口验证。

use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSView, NSWindow, NSWindowButton};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 红绿灯按钮的 y(AppKit 坐标)。目标 y 直接由按钮父视图的**当前** bounds 算,不缓存首次看到的 frame:
/// 窗口从最大化恢复时 `Resized` 可能早于 AppKit 对标题栏按钮的重新布局,缓存旧 frame 会让红绿灯永久偏低。
/// 纯函数,可单测。
pub fn traffic_light_origin_y(bounds_origin_y: f64, bounds_height: f64, button_height: f64, top_bar_height: f64, flipped: bool) -> f64 {
    if flipped {
        bounds_origin_y + (top_bar_height - button_height) / 2.0
    } else {
        bounds_origin_y + bounds_height - (top_bar_height + button_height) / 2.0
    }
}

fn content_view(window: &dyn HasWindowHandle) -> Option<&NSView> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(ah) = handle.as_raw() else { return None };
    // SAFETY: `ns_view` 是已装进窗口的合法 NSView 指针,窗口由主线程持有;这里只借只读引用,不持有 / 释放。
    Some(unsafe { &*(ah.ns_view.as_ptr() as *const NSView) })
}

fn window_buttons(window: &NSWindow) -> [Option<Retained<NSButton>>; 3] {
    [
        window.standardWindowButton(NSWindowButton::CloseButton),
        window.standardWindowButton(NSWindowButton::MiniaturizeButton),
        window.standardWindowButton(NSWindowButton::ZoomButton),
    ]
}

/// 把三个红绿灯在垂直方向居中到 `top_bar_height` 高的自绘顶栏里。放在 `Resized`(含首屏、全屏进出、拖拽缩放)里重设;幂等。
pub fn center_traffic_lights(window: &dyn HasWindowHandle, top_bar_height: f32) {
    let Some(content) = content_view(window) else { return };
    let Some(ns_window) = content.window() else { return };
    for button in window_buttons(&ns_window).into_iter().flatten() {
        // SAFETY: 读按钮的父视图(标题栏容器),只读几何。
        let Some(superview) = (unsafe { button.superview() }) else { continue };
        let frame = button.frame();
        let bounds = superview.bounds();
        let mut origin = frame.origin;
        origin.y = traffic_light_origin_y(bounds.origin.y, bounds.size.height, frame.size.height, top_bar_height as f64, superview.isFlipped());
        button.setFrameOrigin(origin);
    }
}

// ---- 全屏期间的红绿灯 ----

#[derive(Clone, Copy)]
struct TrafficLightHome {
    parent: usize,
    frame: objc2_foundation::NSRect,
    content_frame: objc2_foundation::NSRect,
}

thread_local! {
    static HOMES: std::cell::RefCell<Option<[TrafficLightHome; 3]>> = const { std::cell::RefCell::new(None) };
}

/// 原生全屏不绘制标题栏,标准红绿灯也随之移出窗口。进入全屏前保存按钮在内容视图中的等价位置;
/// 全屏完成后把同一组原生按钮临时挂到内容视图(外观、hover、点击仍由 AppKit 提供);退出全屏再交还原来的标题栏父视图。
fn save_home(content: &NSView) {
    let Some(window) = content.window() else { return };
    let buttons = window_buttons(&window);
    HOMES.with(|saved| {
        let mut saved = saved.borrow_mut();
        if saved.is_some() {
            return;
        }
        *saved = Some(std::array::from_fn(|i| {
            let button = buttons[i].as_ref().expect("standard window button");
            // SAFETY: 读按钮父视图指针,只用于之后原样放回。
            let parent = unsafe { button.superview() }.expect("window button superview");
            TrafficLightHome { parent: std::ptr::from_ref(&*parent) as usize, frame: button.frame(), content_frame: content.convertRect_fromView(button.frame(), Some(&parent)) }
        }));
    });
}

fn move_to_content(content: &NSView) {
    save_home(content);
    let Some(window) = content.window() else { return };
    let buttons = window_buttons(&window);
    HOMES.with(|saved| {
        let saved = saved.borrow();
        let homes = saved.as_ref().expect("homes saved above");
        for (button, home) in buttons.into_iter().zip(homes) {
            let Some(button) = button else { continue };
            button.removeFromSuperview();
            content.addSubview(&button);
            button.setFrame(home.content_frame);
            button.setHidden(false);
        }
    });
}

fn restore(content: &NSView) {
    let Some(window) = content.window() else { return };
    let buttons = window_buttons(&window);
    HOMES.with(|saved| {
        let Some(homes) = saved.borrow_mut().take() else { return };
        for (button, home) in buttons.into_iter().zip(homes) {
            let Some(button) = button else { continue };
            // SAFETY: 保存的父视图是按钮原来的标题栏视图,属于同一个存活的 NSWindow,全屏过渡期间由该视图层级持有。
            let parent = unsafe { &*(home.parent as *const NSView) };
            button.removeFromSuperview();
            parent.addSubview(&button);
            button.setFrame(home.frame);
            button.setHidden(false);
        }
    });
}

/// 在 AppKit 的全屏生命周期边界搬移 / 还原红绿灯(只靠 `Resized` 不可靠:最后一次 resize 可能发生在 AppKit 把标题栏隐藏之前)。
pub fn install_fullscreen_guard(window: &dyn HasWindowHandle) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification, NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification};
    use objc2_foundation::{NSNotificationCenter, NSOperationQueue};

    let Some(content) = content_view(window) else { return };
    let content_ptr = content as *const NSView as usize;
    let Some(ns_window) = content.window() else { return };
    let center = NSNotificationCenter::defaultCenter();
    let queue = NSOperationQueue::mainQueue();

    let install = |name, action: u8| {
        let block = RcBlock::new(move |_| {
            // SAFETY: 观察者的生命周期与进程相同,主窗口 / 内容视图比它活得久;AppKit 在主线程投递这些通知。
            let content = unsafe { &*(content_ptr as *const NSView) };
            match action {
                0 => save_home(content),
                1 => move_to_content(content),
                _ => restore(content),
            }
        });
        // SAFETY: `name` 是 AppKit 的通知名常量。
        let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), Some(&ns_window), Some(&queue), &block) };
        // 主窗口活多久观察者就活多久;不为拆除再另设一个所有者。
        std::mem::forget(observer);
    };
    install(unsafe { NSWindowWillEnterFullScreenNotification }, 0);
    install(unsafe { NSWindowDidEnterFullScreenNotification }, 1);
    install(unsafe { NSWindowWillExitFullScreenNotification }, 2);
    install(unsafe { NSWindowDidExitFullScreenNotification }, 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_lights_are_centered_in_the_top_bar_in_both_coordinate_systems() {
        // unflipped (origin bottom-left): a 40pt bar at the top of a 28pt title bar region of height 28, 16pt buttons
        assert_eq!(traffic_light_origin_y(0.0, 28.0, 16.0, 40.0, false), 0.0 + 28.0 - (40.0 + 16.0) / 2.0);
        assert_eq!(traffic_light_origin_y(4.0, 32.0, 16.0, 40.0, false), 8.0);
        // flipped (origin top-left)
        assert_eq!(traffic_light_origin_y(0.0, 28.0, 16.0, 40.0, true), 12.0);
        // repeated calls with the same bounds give the same answer (idempotent, no cached baseline)
        let a = traffic_light_origin_y(1.0, 30.0, 14.0, 40.0, false);
        assert_eq!(a, traffic_light_origin_y(1.0, 30.0, 14.0, 40.0, false));
    }
}
```

- [ ] **Step 2: `src/lib.rs` 写成最终版(整份替换)**

```rust
//! bytehost-chrome:共享窗口骨架。
//!
//! 顶栏、左右两条图标栏、左右两个面板区、分隔线、"列表 | 内容"配对视图、放大态与布局持久化。**以 `PanelId` 为键,
//! 不理解任何具体面板**:产品实现 [`view::ShellHost`] 给出面板清单与 body 渲染,骨架负责其余。
//!
//! 分层(从下往上,每层只依赖下面的):
//! - [`metrics`] / [`geometry`]:纯数据与纯几何,无 iced、无窗口,可直接单测;
//! - [`rail`] / [`anim`]:图标栏的纯逻辑与动画状态机;
//! - [`model`]:无界面的骨架状态机 `ShellModel` + `ShellMsg`;
//! - [`persist`]:落盘与读盘消毒;
//! - [`theme`]:把 byteui 的运行时主题读成 `ChromeMetrics`;
//! - [`view`]:iced 控件;
//! - [`platform_macos`]:macOS 红绿灯与全屏的窗口装饰胶水。

pub mod anim;
pub mod geometry;
pub mod metrics;
pub mod model;
pub mod persist;
#[cfg(target_os = "macos")]
pub mod platform_macos;
pub mod rail;
pub mod theme;
pub mod view;

pub use geometry::{Rect, ShellRects, Side};
pub use metrics::ChromeMetrics;
pub use model::{Ctx, Divider, Outcome, PaneRules, PairRects, PanelRules, ShellModel, ShellMsg};
pub use persist::{Defaults, Loaded};
pub use theme::metrics_from_theme;
pub use view::{BodyArea, El, PanelMeta, ShellHost, TopBarSlots, map_event, pair_view, view};
```

- [ ] **Step 3: 运行,并确认这个 worktree 没有碰他人的工作区**

```bash
cargo test -p bytehost-chrome 2>&1 | grep -E "^(error|warning)|test result|FAILED"
git -C $ROOT/bytehost status --short | wc -l      # 仍是 Task 0 记下的 N
```

Expected: `test result: ok. 63 passed`,没有 warning;文件数 = N。

- [ ] **Step 4: Commit**

```bash
git add crates/bytehost-chrome
git commit -m "feat(chrome): macOS traffic-light centering and fullscreen guard

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

## Digger(在 `$ROOT/digger`)

### Task 11: Digger 的骨架适配层(`shell.rs`)与依赖

**Files:**
- Modify: `Cargo.toml`(workspace 依赖 + `[patch]` 里加 `bytehost-chrome`)、`crates/desktop/Cargo.toml`、`crates/desktop/src/paths.rs`(**加** `shell_path`,`layout_path` 暂留到 Task 12)、`crates/desktop/src/main.rs`(加 `mod shell;`)
- Create: `crates/desktop/src/shell.rs`

**Interfaces:**
- Consumes: Task 3–10 的 `bytehost_chrome::*`
- Produces(`product.rs` 在 Task 12 用):
  - `shell::{info_id, apps_id, usage_id, markdown_id}() -> PanelId`、`shell::all_panels() -> Vec<PanelId>`
  - `shell::meta(&PanelId) -> Option<PanelMeta>`、`shell::rules(&PanelId) -> Option<PanelRules>`(S1 里全是 `None`:还没有"列表 | 内容"配对的面板)
  - `shell::defaults() -> Defaults`:左 rail `[markdown, info]`、右 rail `[apps, usage]`;选中左 = 编辑器、右 = 用量;左宽 640
  - `shell::load(&Path) -> Loaded`、`shell::update(&mut ShellModel, ShellMsg) -> Outcome`
  - `paths::shell_path() -> PathBuf`(`<config>/shell.json`)

- [ ] **Step 1: 依赖**

根 `Cargo.toml` 的 `[workspace.dependencies]`,在 `bytehost-core` 那行之后加:

```toml
# 共享窗口骨架(S1)。开发期靠文件末尾的 [patch] 指向本地 worktree;打 tag 后改成新 tag 并删掉 [patch]。
bytehost-chrome = { git = "https://github.com/byteboyai/bytehost", tag = "v0.2.0" }
```

根 `Cargo.toml` 末尾 `[patch."https://github.com/byteboyai/bytehost"]` 段里加一行(放在 `bytehost-core` 之后):

```toml
bytehost-chrome = { path = "../bytehost-chrome/crates/bytehost-chrome" }
```

`crates/desktop/Cargo.toml` 的 `[dependencies]`,在 `bytehost-core.workspace = true` 之后加:

```toml
# 共享窗口骨架:顶栏 / 图标栏 / 面板区 / 分隔线 / 放大态 / 布局持久化。
bytehost-chrome.workspace = true
```

- [ ] **Step 2: 先写 `shell.rs` 里的测试(末尾 `mod tests`),运行看红;再补实现**

`crates/desktop/src/shell.rs`:

```rust
//! Digger 对共享窗口骨架(`bytehost-chrome`)的适配:面板 id、清单、默认布局、图标与标题、落盘路径。
//!
//! 骨架以 `PanelId` 为键、不理解任何具体面板;这里是产品给它的全部输入。面板 body 的渲染在 `product.rs`
//! (`ShellHost` 实现),因为 body 要读各面板自己的状态。

use std::path::Path;

use bytehost_chrome::{Ctx, Defaults, Loaded, Outcome, PanelMeta, PanelRules, ShellModel, ShellMsg, metrics_from_theme, persist};
use bytehost_core::id::PanelId;
use byteui::interaction::icons::IconKind;

fn id(s: &str) -> PanelId {
    PanelId::new(s).expect("合法 panel id")
}

/// 产品信息 / 状态面板。
pub fn info_id() -> PanelId {
    id("digger.info")
}
/// 应用管理面板。
pub fn apps_id() -> PanelId {
    id("digger.apps")
}
/// 用量面板(共享 `dozer-panel-usage` 库)。
pub fn usage_id() -> PanelId {
    id("digger.usage")
}
/// Markdown 编辑器面板(ADR-013)。
pub fn markdown_id() -> PanelId {
    id("digger.markdown")
}

/// 产品的全部面板。落盘时清单里有而文件里没有的面板,会回到它的默认栏(新版本新增面板不会重置用户布局)。
pub fn all_panels() -> Vec<PanelId> {
    vec![markdown_id(), info_id(), apps_id(), usage_id()]
}

/// 图标栏上的图标与悬停标题。
pub fn meta(panel: &PanelId) -> Option<PanelMeta> {
    let (title, icon) = match panel.as_str() {
        "digger.markdown" => ("编辑器", IconKind::FileText),
        "digger.info" => ("信息", IconKind::SquareActivity),
        "digger.apps" => ("应用", IconKind::LayoutList),
        "digger.usage" => ("用量", IconKind::BarChart3),
        _ => return None,
    };
    Some(PanelMeta { title: title.to_owned(), icon })
}

/// 面板的配对规则。S1 里 Digger 还没有"列表 | 内容"配对的面板(那要等 S2 起的共享面板),所以都是 `None`。
pub fn rules(_panel: &PanelId) -> Option<PanelRules> {
    None
}

/// 默认布局:沿用 Dozer 的分工,左区放内容型面板、右区放辅助型面板。左宽 640 与 Dozer 一致。
pub fn defaults() -> Defaults {
    let (w, h) = byteui::theme::geometry::initial_window_size();
    Defaults {
        rails: [vec![markdown_id(), info_id()], vec![apps_id(), usage_id()]],
        selected: [markdown_id(), usage_id()],
        left_width: 640.0,
        window: (w, h),
        min_window: (byteui::theme::geometry::min_window_width(), byteui::theme::geometry::min_window_height()),
    }
}

/// 读盘:文件不存在、损坏、或布局无法修复时用默认值;更高 schema 版本的文件拒绝覆盖(见 [`Loaded::may_save`])。
pub fn load(path: &Path) -> Loaded {
    let json = std::fs::read_to_string(path).ok();
    persist::load(json.as_deref(), &all_panels(), &defaults(), &metrics_from_theme())
}

/// 把一条骨架消息交给模型,度量与面板规则取产品的。
pub fn update(model: &mut ShellModel, msg: ShellMsg) -> Outcome {
    let metrics = metrics_from_theme();
    model.update(msg, &Ctx { metrics: &metrics, rules: &rules })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;

    #[test]
    fn panel_ids_are_the_persisted_wire_names_and_must_not_change() {
        assert_eq!(info_id().as_str(), "digger.info");
        assert_eq!(apps_id().as_str(), "digger.apps");
        assert_eq!(usage_id().as_str(), "digger.usage");
        assert_eq!(markdown_id().as_str(), "digger.markdown");
    }

    #[test]
    fn every_panel_has_an_icon_and_a_title_and_unknown_ids_have_none() {
        for p in all_panels() {
            let m = meta(&p).unwrap_or_else(|| panic!("{p} has no meta"));
            assert!(!m.title.is_empty());
        }
        assert!(meta(&id("digger.nope")).is_none());
    }

    #[test]
    fn the_default_layout_is_legal_and_covers_every_panel_exactly_once() {
        let d = defaults();
        let model = ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).expect("legal default layout");
        let mut seen: Vec<PanelId> = model.rail(Side::Left).iter().chain(model.rail(Side::Right)).cloned().collect();
        seen.sort();
        let mut want = all_panels();
        want.sort();
        assert_eq!(seen, want);
        assert_eq!(model.rail(Side::Left), [markdown_id(), info_id()], "content panels on the left, like Dozer");
        assert_eq!(model.rail(Side::Right), [apps_id(), usage_id()]);
        assert_eq!(model.selected(Side::Left), &markdown_id());
        assert_eq!(model.selected(Side::Right), &usage_id());
        assert_eq!(d.left_width, 640.0);
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_may_be_saved() {
        let dir = std::env::temp_dir().join(format!("digger-shell-missing-{}", std::process::id()));
        let loaded = load(&dir.join("shell.json"));
        assert!(matches!(loaded, Loaded::Fresh(_)));
        assert!(loaded.may_save());
    }

    /// 更高版本的文件退出时不得被覆盖:读到 `Newer` 就不能再写。
    #[test]
    fn a_newer_schema_file_is_left_byte_for_byte_alone() {
        let dir = std::env::temp_dir().join(format!("digger-shell-newer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&path, original).unwrap();
        let loaded = load(&path);
        assert!(!loaded.may_save());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panel_added_in_a_later_version_returns_to_its_default_side_without_resetting_the_user_layout() {
        let dir = std::env::temp_dir().join(format!("digger-shell-added-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        // an older file that predates the usage panel and has the user's own order on the left
        std::fs::write(&path, r#"{ "left": ["digger.info", "digger.markdown"], "right": ["digger.apps"], "left_width": 500.0 }"#).unwrap();
        let model = load(&path).into_model();
        assert_eq!(model.rail(Side::Left), [info_id(), markdown_id()], "the user's order is kept");
        assert_eq!(model.rail(Side::Right), [apps_id(), usage_id()], "the new panel lands on its default side");
        assert_eq!(model.left_width(), 500.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

`crates/desktop/src/main.rs` 在 `mod product;` 之后加一行:

```rust
mod shell;
```

`crates/desktop/src/paths.rs`:在 `layout_path` 函数之后加,并在它的测试旁加一条测试:

```rust
/// 窗口骨架的布局与偏好文件:全局 `shell.json`(取代旧的 `layout.json`)。
pub fn shell_path() -> PathBuf {
    config_dir().join("shell.json")
}
```

```rust
    #[test]
    fn shell_path_is_under_config() {
        let p = shell_path();
        assert_eq!(p.file_name().unwrap(), "shell.json");
        assert!(p.starts_with(config_dir()));
    }
```

- [ ] **Step 3: 运行**

```bash
cd $ROOT/digger
cargo test -p digger-desktop shell 2>&1 | grep -E "^error|test result|FAILED"
```

Expected: `test result: ok.`,其中 `shell::tests` 6 个全过(另有 `paths::tests::shell_path_is_under_config`);`product.rs` 还在用 `layout_path`,编译不受影响(只会有"未使用"警告)。

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/desktop
git commit -m "feat(desktop): the Digger side of the shared window shell (panel ids, defaults, paths)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 12: 产品壳改用骨架 —— 重写 `product.rs` 与生命周期自检

`product.rs` 与 `lifecycle_selftest.rs` 互相引用(`Driver`、`Phase`、`ProductShell` 的访问器),必须同一个任务里一起换,否则编译不过。

**Files:**
- Replace: `crates/desktop/src/product.rs`、`crates/desktop/src/lifecycle_selftest.rs`
- Modify: `crates/desktop/src/paths.rs`(删 `layout_path` 与它的测试)

**Interfaces:**
- Consumes: Task 11 的 `shell::*`、`bytehost_chrome::{ShellHost, ShellModel, ShellMsg, view, map_event, metrics_from_theme, persist, platform_macos}`、`crate::markdown::{MarkdownPanel, BAR_HEIGHT, Probe}`
- Produces:
  - `product::Msg::Shell(ShellMsg)`、`Msg::ZoomToggle`;**删除** `Msg::SelectPanel`、`Msg::Resized`、`ActivePanel`、`DiggerLayoutAdapter`、`load_layout` 及其测试
  - `impl ShellHost<Msg> for ProductShell`(面板 body、顶栏三个槽位、底栏占位槽)
  - `ProductShell::{window_id, window_size, markdown_ready, markdown_last_probe, markdown_probe}`(自检读取用)
  - `lifecycle_selftest::{Driver, Phase, ENV}`:11 步骨架版自检,**期望值独立手算**,不调用骨架的几何函数
- 行为要点:
  - 原生 webview 的矩形来自 `shell.rects(..).content(side)`,再各自减去面板自己头部的高度(`APPS_HEADER_HEIGHT` / `USAGE_HEADER_HEIGHT` 常量,编辑器的 56px 条在 `markdown` 模块里);**骨架负责在哪、看不看得见,面板只负责自己头部多高**。
  - 应用面板从可见变成不可见时 `close_selected()`;启动时已经可见的面板(默认右栏的用量)也要触发各自的"切入时动作"。
  - 落盘:每次 `Outcome.persist` 都写 `shell.json`;磁盘上是更高 schema 版本时拒绝回写并在信息面板提示。

- [ ] **Step 1: 替换 `crates/desktop/src/product.rs`(整份)**

先写文件末尾的 `mod tests`(运行看红),再补实现。完整文件:

```rust
//! Digger 产品壳:在共享 bytehost 宿主上启动的产品组合根。
//!
//! 窗口结构(顶栏 / 左右图标栏 / 左右面板区 / 分隔线 / 放大态 / 布局持久化)来自共享骨架 `bytehost-chrome`,与 Dozer 的
//! 窗口结构同源;本文件只做产品侧的事:注册面板、实现 `ShellHost`、把骨架消息接到各面板的切入动作和原生 webview 的
//! 同步上。拥有自己的数据根、主题、布局落盘与退出,**不复制** Dozer 的 `App`/`Workspace`。
//!
//! 范围(S1):以**无 Project** 状态启动;顶栏的项目页签行保留结构、内容留空,"＋"置灰;**不伪造**任何写入能力。

use std::path::PathBuf;

use bytehost_chrome::persist::SCHEMA_VERSION;
use bytehost_chrome::{BodyArea, El, Loaded, PanelMeta, PanelRules, Rect, ShellHost, ShellModel, ShellMsg, TopBarSlots, metrics_from_theme};
use bytehost_core::registry::PanelCatalog;
use bytehost_core::{LogicalRect, PanelId, PanelMeta as CatalogEntry};
use bytehost_desktop::{HostMessage, Product, route_lifecycle};
use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};

use iced::widget::{column, container, row, text, tooltip};
use iced::{Element, Length, Size, Subscription, Task, window};

use crate::{paths, shell};

/// 应用面板自己的头部(返回按钮 + 应用 id + 状态)的高度:原生应用 webview 摆在它下面。
const APPS_HEADER_HEIGHT: f32 = 96.0;
/// 用量面板自己的原生头部(标题 + 加载态)的高度:图表 webview 摆在它下面。
const USAGE_HEADER_HEIGHT: f32 = 80.0;

/// 产品消息。**不进入**宿主 enum(宿主只认 `HostMessage<M>`)。
#[derive(Debug, Clone)]
pub enum Msg {
    WindowReady(window::Id, Size),
    /// 窗口骨架的消息(点图标、拖分隔线、窗口缩放……),由 `bytehost-chrome` 的 `ShellModel` 处理。
    Shell(ShellMsg),
    /// 「应用」面板的消息(Task 14)。
    Apps(crate::apps::Message),
    /// 「用量」面板的库消息(Task 20)。
    Usage(dozer_panel_usage::Message),
    NativeUsage(crate::adapters::usage_native::Event),
    /// 「编辑器」面板的消息(ADR-013)。
    Markdown(crate::markdown::Message),
    /// 面板生命周期自检的推进(仅 `DIGGER_LIFECYCLE_SELFTEST` 设置时出现)。
    Lifecycle(crate::lifecycle_selftest::Phase),
    Tick,
    /// 双击顶栏空白处:缩放窗口(系统标题栏没了,这个手势要自己补)。
    ZoomToggle,
    ShutdownFinished(Result<(), String>),
    ForceExit,
    CloseRequested,
    Exit,
}

/// Digger 产品壳状态。刻意保持最小:不持有任何 Domain/持久化状态,只有 UI 交互态 + 布局 + 主题。
pub struct ProductShell {
    window: Option<window::Id>,
    catalog: PanelCatalog,
    /// 窗口骨架状态(布局、选中、收起、放大、尺寸……)。
    shell: ShellModel,
    shell_path: PathBuf,
    /// 非 `None` 表示磁盘上是**不支持的高版本**布局:内存默认布局仅用于显示,**禁止回写**该文件。
    shell_save_blocked: Option<String>,
    /// 「应用」面板的组合根(状态机 + 执行器 + 内嵌应用服务)。
    apps: crate::apps::AppsState,
    /// 「用量」面板适配(共享 `dozer-panel-usage` 库 + 注入的真实用量服务)。
    usage: crate::adapters::usage::UsagePanel,
    /// 「编辑器」面板:自包含的第一方面板(文本与协议在 `digger-editor`,这里只有宿主适配)。
    markdown: crate::markdown::MarkdownPanel,
    /// 生命周期自检的驱动(只在环境变量设置时有)。
    lifecycle: Option<crate::lifecycle_selftest::Driver>,
    /// 应用面板上一次是否可见;从可见变为不可见时关掉正在显示的应用。
    apps_was_visible: bool,
    /// 产品未接业务服务时的启动说明,显示在信息面板。
    boot_note: String,
    closing: bool,
    shutdown_started: bool,
    shutdown_error: Option<String>,
    /// 测试用:记录收到的生命周期回调,验证 Surface 事件恰好路由一次(复盘发现 6)。
    #[cfg(test)]
    surface_callbacks: Vec<bytehost_desktop::SurfaceSlot>,
}

fn logical(r: Rect) -> LogicalRect {
    LogicalRect { x: r.x as f64, y: r.y as f64, width: r.w as f64, height: r.h as f64 }
}

/// 向下缩 `top` 像素(面板自己的头部占掉的高度)。
fn below_header(r: Rect, top: f32) -> Rect {
    Rect::new(r.x, r.y + top, r.w, (r.h - top).max(1.0))
}

impl ProductShell {
    /// 构造:注册产品面板目录、读布局(缺省用默认布局),不打开任何 Project。
    pub fn new() -> Self {
        Self::with_shell_path(paths::shell_path())
    }

    pub(crate) fn with_shell_path(shell_path: PathBuf) -> Self {
        let mut catalog = PanelCatalog::new();
        for (id, title) in [(shell::info_id(), "Digger"), (shell::apps_id(), "应用"), (shell::usage_id(), "用量")] {
            catalog.register(CatalogEntry { id, title: title.into(), surfaces: vec![] }).expect("首次注册产品面板");
        }
        catalog.register(CatalogEntry::single_surface(shell::markdown_id(), "编辑器")).expect("首次注册 digger.markdown");

        let loaded = shell::load(&shell_path);
        let shell_save_blocked = match &loaded {
            Loaded::Newer { found, .. } => Some(format!("布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**")),
            _ => None,
        };
        let mut model = loaded.into_model();

        // 任务完成用量账本(Task 20):生产者(应用操作)写、用量面板读,同一实例。
        // 载入失败(损坏)不 panic,也**不**退回一次性临时空账本(复审 #9)——后者会把"读不出来"伪装成 "Ok(空)",且新记录
        // 写进下次启动就不再引用的临时文件,等于静默丢。改为:查询恒返回 Unavailable、**不接线**用量生产者(没有记录悄悄
        // 写走),原损坏文件保持原样。
        let scope_path = paths::config_dir();
        let (usage_service, usage_ledger, boot_note) = load_usage_ledger(paths::usage_ledger_path());

        let mut apps = crate::apps::AppsState::new();
        // 只有账本可用时才接用量生产者;损坏时**不接**,避免记录写进无引用的临时文件。
        if let Some(ledger) = &usage_ledger {
            apps.attach_usage(ledger.clone(), scope_path.to_string_lossy().to_string());
        }
        let usage = crate::adapters::usage::UsagePanel::new(usage_service, scope_path.clone());
        let markdown = crate::markdown::MarkdownPanel::new();
        let lifecycle = crate::lifecycle_selftest::Driver::from_env();
        // 自检模式需要编辑器一开始就看得见。
        if markdown.is_selftest() || lifecycle.is_some() {
            shell::update(&mut model, ShellMsg::ShowPanel(shell::markdown_id()));
        }

        Self {
            window: None,
            catalog,
            shell: model,
            shell_path,
            shell_save_blocked,
            apps,
            usage,
            markdown,
            lifecycle,
            apps_was_visible: false,
            boot_note,
            closing: false,
            shutdown_started: false,
            shutdown_error: None,
            #[cfg(test)]
            surface_callbacks: Vec::new(),
        }
    }

    /// 落盘当前骨架状态(退出 / 布局变化时调用)。失败不 panic,记入 boot_note。
    /// 磁盘上是**不支持的高版本**布局时**拒绝回写**(复盘发现 4:绝不覆盖原文件)。
    fn save_shell(&mut self) {
        if let Some(note) = &self.shell_save_blocked {
            self.boot_note = format!("布局未保存:{note}");
            return;
        }
        if let Err(e) = bytehost_chrome::persist::save_atomic(&self.shell_path, &self.shell) {
            self.boot_note = format!("布局保存失败:{e}");
        }
    }

    /// 面板此刻是否真的呈现在用户眼前:它是所在栏的选中面板、所在区没收起、也没被另一侧的放大态盖住。
    pub(crate) fn panel_visible(&self, panel: &PanelId) -> bool {
        let Some(side) = self.shell.side_of(panel) else { return false };
        self.shell.selected(side) == panel && !self.shell.collapsed(side) && self.shell.maximized().is_none_or(|m| m == side)
    }

    /// 面板刚被切入(或启动时本来就可见):触发各自的"切入时动作"。
    fn on_visible(&mut self, panel: &PanelId) -> Task<Msg> {
        if *panel == shell::apps_id() {
            // 首次进入「应用」面板:触发一次探测 + 拉列表(Task 14)。
            return self.apps.update(crate::apps::Message::Opened).map(Msg::Apps);
        }
        if *panel == shell::usage_id() {
            // 进入「用量」面板:触发一次取数(库的 on_activate,Task 20)。
            self.usage.activate();
        }
        Task::none()
    }

    fn on_shell(&mut self, msg: ShellMsg) -> Task<Msg> {
        if self.closing {
            return Task::none();
        }
        let resized = matches!(msg, ShellMsg::Resized { .. });
        let out = shell::update(&mut self.shell, msg);
        if out.persist {
            self.save_shell();
        }
        let mut tasks = Vec::new();
        if let Some(panel) = out.switched_in {
            tasks.push(self.on_visible(&panel));
        }
        // 应用面板从可见变成不可见(切走 / 收起 / 被放大盖住):关掉正在显示的应用,别留一个看不见的 webview 盖在别处。
        let apps_visible = self.panel_visible(&shell::apps_id());
        if self.apps_was_visible && !apps_visible {
            self.apps.close_selected();
        }
        self.apps_was_visible = apps_visible;
        if resized {
            tasks.push(self.restyle_window());
        }
        Task::batch(tasks)
    }

    /// macOS:红绿灯垂直居中到顶栏里;窗口缩放(含全屏进出)后重设,幂等。其它平台无事可做。
    fn restyle_window(&self) -> Task<Msg> {
        #[cfg(target_os = "macos")]
        if let Some(id) = self.window {
            let top = byteui::theme::geometry::top_bar_height();
            return window::run(id, move |w| bytehost_chrome::platform_macos::center_traffic_lights(w, top)).discard();
        }
        Task::none()
    }

    /// 原生 webview 的位置与可见性:矩形取自骨架的面板内容矩形(外框 − 外边距 − 内边距;放大态是放大盒子),
    /// 再各自减掉面板自己头部占的高度。**骨架负责"在哪、看不看得见",面板只负责自己头部多高。**
    fn sync_native(&mut self) -> Task<Msg> {
        let Some(window) = self.window else {
            return Task::none();
        };
        let rects = self.shell.rects(&metrics_from_theme());
        let content_of = |panel: &PanelId| -> Option<Rect> {
            let side = self.shell.side_of(panel)?;
            if self.shell.selected(side) != panel {
                return None;
            }
            rects.content(side)
        };
        let apps_rect = content_of(&shell::apps_id());
        let usage_rect = content_of(&shell::usage_id());
        let markdown_rect = content_of(&shell::markdown_id());
        let hidden = LogicalRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
        let closing = self.closing;

        let apps_task = match apps_rect {
            Some(r) if !closing => self.apps.sync_native(window, logical(below_header(r, APPS_HEADER_HEIGHT))).map(Msg::Apps),
            // 关闭时由 `apps.prepare_shutdown` 收尾;不可见时 `on_shell` 已 `close_selected`,这里仍要让待办的原生动作跑完。
            _ => self.apps.sync_native(window, hidden).map(Msg::Apps),
        };
        let usage_task = self
            .usage
            .sync_native(window, usage_rect.is_some() && !closing, usage_rect.map(|r| logical(below_header(r, USAGE_HEADER_HEIGHT))).unwrap_or(hidden))
            .map(Msg::NativeUsage);
        // 关闭时销毁编辑器 webview 并丢掉文档;否则只在可见时显示,切走则隐藏。
        let markdown_task = if closing {
            self.markdown.close(window)
        } else {
            self.markdown.sync_native(window, markdown_rect.is_some(), markdown_rect.map(logical).unwrap_or(hidden))
        }
        .map(Msg::Markdown);
        Task::batch([apps_task, usage_task, markdown_task])
    }

    fn info_view(&self) -> Element<'_, Msg> {
        let tokens = color::current();
        let dim = tokens.dim;
        container(
            column![
                text(format!("面板目录已注册 {} 个(产品组合根)", self.catalog.len())).color(tokens.body),
                text(format!("数据根:{}", paths::config_dir().display())).color(dim),
                text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
                text("当前 Project:无").color(tokens.body),
                text(&self.boot_note).color(tokens.gold),
                iced::widget::button("退出 Digger").on_press(Msg::Exit),
            ]
            .spacing(8)
            .width(Length::Fill),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn apps_view(&self) -> Element<'_, Msg> {
        container(self.apps.view().map(Msg::Apps)).padding(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn usage_view(&self) -> Element<'_, Msg> {
        container(self.usage.view()).padding(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn markdown_view(&self) -> Element<'_, Msg> {
        // 条在上;下面留空,原生 webview 由 `sync_native` 摆在条的下方。
        column![self.markdown.view().map(Msg::Markdown), iced::widget::Space::new().height(Length::Fill)].into()
    }
}

/// 生命周期自检要读的东西。
impl ProductShell {
    pub fn window_id(&self) -> Option<window::Id> {
        self.window
    }

    pub fn window_size(&self) -> (f32, f32) {
        self.shell.window()
    }

    pub fn markdown_ready(&self) -> bool {
        self.markdown.is_ready()
    }

    pub fn markdown_last_probe(&self) -> Option<crate::markdown::Probe> {
        self.markdown.last_probe()
    }

    pub fn markdown_probe(&self, id: window::Id) -> Task<Msg> {
        self.markdown.probe(id).map(Msg::Markdown)
    }
}

/// 顶栏三个槽位。S1 没有 Project:标题只是展示,页签行显示"无项目","＋"置灰并提示。
fn top_bar_slots<'a>() -> TopBarSlots<'a, Msg> {
    let tokens = color::current();
    let dim = tokens.dim;
    let title = row![
        icons::view(IconKind::Home, icon_size::home(), dim),
        text("Digger").size(font::body()).color(dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let plus = tooltip(
        container(icons::view(IconKind::Plus, icon_size::rail(), dim)).padding(6),
        container(text("Project 尚未接入").size(font::body())).padding(6).style(container::rounded_box),
        tooltip::Position::Bottom,
    );
    let tabs = row![text("无项目").size(font::body()).color(dim), plus].spacing(10).align_y(iced::Alignment::Center);
    TopBarSlots { title: title.into(), tabs: tabs.into(), right: iced::widget::Space::new().into() }
}

impl ShellHost<Msg> for ProductShell {
    fn lift(msg: ShellMsg) -> Msg {
        Msg::Shell(msg)
    }

    fn meta(&self, panel: &PanelId) -> Option<PanelMeta> {
        shell::meta(panel)
    }

    fn rules(&self, panel: &PanelId) -> Option<PanelRules> {
        shell::rules(panel)
    }

    fn body<'a>(&'a self, panel: &PanelId, _area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(),
            "digger.markdown" => self.markdown_view(),
            _ => column![].into(),
        }
    }

    fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
        top_bar_slots()
    }

    fn footbar<'a>(&'a self) -> El<'a, Msg> {
        // 底栏占位槽:S1 里没有内容,但高度与 Dozer 一致,窗口结构不变。
        iced::widget::Space::new().into()
    }

    fn on_top_bar_double_click(&self) -> Msg {
        Msg::ZoomToggle
    }
}

/// 载入用量账本并决定产品侧接线(复审 #9)。
///
/// 返回 `(用量服务, 可选共享账本, boot 提示)`:
/// - 成功 → 真账本 + [`LedgerUsageService`](digger_store::LedgerUsageService);
/// - 损坏/读失败 → **不退回临时空账本**,日志查询恒 `Unavailable`,账本为 `None`(调用方据此
///   不接线用量生产者——没有记录会悄悄写进一个下次启动就不再引用的文件)。原文件保持原样。
fn load_usage_ledger(
    path: std::path::PathBuf,
) -> (
    std::sync::Arc<dyn dozer_services::UsageService>,
    Option<std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>>,
    String,
) {
    match digger_store::UsageLedger::load(path) {
        Ok(ledger) => {
            let ledger = std::sync::Arc::new(std::sync::Mutex::new(ledger));
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::LedgerUsageService::new(ledger.clone()));
            let note = "尚未打开 Project。可在「应用」页管理并运行应用。".to_string();
            (svc, Some(ledger), note)
        }
        Err(e) => {
            let note = format!("用量账本载入失败,用量查询暂不可用(不会丢数据:原文件未改动): {e}");
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::UnavailableUsageService::new(note.clone()));
            (svc, None, note)
        }
    }
}

impl Default for ProductShell {
    fn default() -> Self {
        Self::new()
    }
}

impl Product<Msg> for ProductShell {
    fn update(&mut self, msg: HostMessage<Msg>) -> Task<Msg> {
        let HostMessage::Product(m) = msg else {
            // Surface 事件**不在这里**再分发给生命周期:那由外层入口 [`update_host`] 单次调用
            // `route_lifecycle` 完成(复盘发现 6:在此调用会让 `route_lifecycle` → `update` 形成无限递归)。
            return Task::none();
        };
        match m {
            Msg::WindowReady(id, size) => {
                self.window = Some(id);
                let resized = self.on_shell(ShellMsg::Resized { w: size.width, h: size.height });
                // macOS:全屏期间把红绿灯临时挂到内容视图(原生全屏不绘制标题栏,红绿灯会跟着移出窗口)。
                #[cfg(target_os = "macos")]
                let guard = window::run(id, |w| bytehost_chrome::platform_macos::install_fullscreen_guard(w)).discard();
                #[cfg(not(target_os = "macos"))]
                let guard = Task::none();
                Task::batch([resized, guard])
            }
            Msg::Shell(m) => self.on_shell(m),
            Msg::ZoomToggle => self.window.map(window::toggle_maximize).unwrap_or_else(Task::none),
            Msg::Apps(m) => self.apps.update(m).map(Msg::Apps),
            Msg::Usage(m) => {
                self.usage.update(m);
                // Digger 一期无 Toast:把库的 outbox 提示落地到 stderr,不静默吞掉。
                for (level, text) in self.usage.take_notices() {
                    eprintln!("digger: usage 面板/{level}: {text}"); // cli-output
                }
                Task::none()
            }
            Msg::Tick => self.apps.update(crate::apps::Message::Tick).map(Msg::Apps),
            Msg::NativeUsage(event) => {
                self.usage.native.update(event);
                Task::none()
            }
            Msg::Markdown(m) => self.markdown.update(m).map(Msg::Markdown),
            Msg::Lifecycle(phase) => {
                let Some(mut driver) = self.lifecycle.take() else {
                    return Task::none();
                };
                let task = driver.advance(self, phase);
                self.lifecycle = Some(driver);
                task
            }
            Msg::ShutdownFinished(result) => match result {
                // 生命周期自检要在关闭之后读句柄表,检查完自己退出
                Ok(()) if self.lifecycle.is_some() => Task::none(),
                Ok(()) => iced::exit(),
                Err(error) => {
                    self.shutdown_error = Some(error);
                    Task::none()
                }
            },
            Msg::ForceExit => iced::exit(),
            Msg::CloseRequested | Msg::Exit => {
                if self.closing {
                    return Task::none();
                }
                self.closing = true;
                self.save_shell();
                self.apps.prepare_shutdown();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::button;

        if self.closing {
            let mut body = column![text("正在关闭 Digger…")].spacing(16).padding(20);
            if let Some(error) = &self.shutdown_error {
                body = body.push(text(error)).push(button("强制退出").on_press(Msg::ForceExit));
            }
            return body.into();
        }
        bytehost_chrome::view(&self.shell, self, &metrics_from_theme())
    }

    fn on_activated(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }

    fn on_closed(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }
}

/// 产品壳的**唯一**宿主入口:Surface 事件在这里**单次**经 `route_lifecycle` 分发到产品生命周期回调
/// (复盘发现 6),产品消息则走 `Product::update`。`Product::update` 的 Surface 分支只消费、不再重投,
/// 二者分工使路由不成环。
pub fn update_host(shell: &mut ProductShell, msg: HostMessage<Msg>) -> Task<Msg> {
    let task = match msg {
        HostMessage::Surface(event) => route_lifecycle(shell, event),
        HostMessage::Product(m) => Product::update(shell, HostMessage::Product(m)),
    };
    // 退出分支自己将销毁与 shutdown 串行；其他更新后同步内容与几何。
    let native = shell.sync_native();
    if shell.closing && !shell.shutdown_started && shell.apps.native_idle() {
        shell.shutdown_started = true;
        Task::batch([task, native]).chain(Task::perform(shell.apps.shutdown(), Msg::ShutdownFinished))
    } else {
        Task::batch([task, native])
    }
}

/// 订阅:窗口关闭请求、骨架需要的鼠标 / 缩放事件,以及仅在有动画时才开的 ~60fps 动画拍。
pub fn subscription(shell: &ProductShell) -> Subscription<Msg> {
    let events = iced::event::listen_with(|evt, _status, _id| {
        if let iced::Event::Window(window::Event::CloseRequested) = evt {
            return Some(Msg::CloseRequested);
        }
        bytehost_chrome::map_event(&evt).map(Msg::Shell)
    });
    if shell.closing {
        return events;
    }
    let mut subs = vec![events, iced::time::every(std::time::Duration::from_secs(1)).map(|_| Msg::Tick)];
    if shell.shell.animating() {
        subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Msg::Shell(ShellMsg::Tick)));
    }
    Subscription::batch(subs)
}

/// 入口用:初始窗口尺寸 = 上次退出时保存的尺寸(没有就是默认尺寸)。
pub fn default_window_size_for_boot() -> Size {
    let (w, h) = shell::load(&paths::shell_path()).model().window();
    Size::new(w, h)
}

/// 应用初始化:确保产品数据根、设置 ByteBoy2077 主题,异步取窗口尺寸。
pub fn boot() -> (ProductShell, Task<Msg>) {
    if let Err(e) = paths::ensure_data_root() {
        // 数据根不可用不 panic:产品壳仍可打开,只是无法持久化布局。
        // (转为可见提示由产品壳承载。)
        eprintln!("digger: 数据根不可用: {e}"); // cli-output
    }
    color::set_theme(color::ColorTokens::byteboy2077());
    font::set_theme(font::FontTokens::byteboy2077());

    let mut shell = ProductShell::new();
    let size = default_window_size_for_boot();
    // 用量面板要 iced 运行时句柄在后台取数:`boot` 在 iced 运行时内执行,这里补挂。
    shell.usage.attach_handle(tokio::runtime::Handle::current());
    // 应用面板后台结果回流:一次性流出接收端,持续到进程结束(与窗口尺寸任务并行)。
    use iced::futures::StreamExt;
    let reply_task = match crate::apps::take_reply_receiver(&shell.apps.reply_receiver()) {
        Some(rx) => iced::Task::stream(crate::apps::reply_stream(rx).map(Msg::Apps)),
        None => Task::none(),
    };
    // 用量面板后台取数结果回流:同型,接成 iced 流回投 `Msg::Usage`。
    let usage_task = match shell.usage.take_receiver() {
        Some(rx) => iced::Task::stream(crate::adapters::usage::receiver_stream(rx).map(Msg::Usage)),
        None => Task::none(),
    };
    let size_task = bytehost_desktop::panel::oldest_window().then(move |id| match id {
        Some(id) => bytehost_desktop::panel::window_size(id).map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    let native_usage = match shell.usage.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|event| (Msg::NativeUsage(event), rx)) })),
        None => Task::none(),
    };
    let markdown_task = match shell.markdown.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|event| (Msg::Markdown(event), rx)) })),
        None => Task::none(),
    };
    let lifecycle_task = if shell.lifecycle.is_some() { crate::lifecycle_selftest::Driver::first() } else { Task::none() };
    // 启动时就可见的面板(例如默认右栏选中的用量)也要触发各自的"切入时动作"。
    let mut initial = Vec::new();
    for panel in [shell::apps_id(), shell::usage_id()] {
        if shell.panel_visible(&panel) {
            initial.push(shell.on_visible(&panel));
        }
    }
    shell.apps_was_visible = shell.panel_visible(&shell::apps_id());
    (shell, Task::batch([size_task, reply_task, usage_task, native_usage, markdown_task, lifecycle_task, Task::batch(initial)]))
}


#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;
    use bytehost_desktop::SurfaceEvent;

    fn test_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("digger-product-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 带独立落盘路径的壳:互不干扰,也不碰真实配置目录。
    fn fresh(tag: &str) -> ProductShell {
        ProductShell::with_shell_path(test_temp_dir(tag).join("shell.json"))
    }

    fn press(shell: &mut ProductShell, panel: &PanelId) {
        let _ = Product::update(shell, HostMessage::Product(Msg::Shell(ShellMsg::PanelPress { panel: panel.clone() })));
    }

    #[test]
    fn starts_without_project() {
        let shell = ProductShell::new();
        assert!(shell.boot_note.contains("尚未打开 Project"));
    }

    #[test]
    fn registers_product_panels() {
        let shell = ProductShell::new();
        for id in shell::all_panels() {
            assert!(shell.catalog.contains(&id), "{id}");
        }
        assert_eq!(shell.catalog.len(), 4);
        // 编辑器声明一个主 Surface 槽位;纯 iced 原生面板不声明。
        assert_eq!(shell.catalog.get(&shell::markdown_id()).unwrap().surfaces, vec![bytehost_desktop::SurfaceSlot::MAIN]);
        assert!(shell.catalog.get(&shell::info_id()).unwrap().surfaces.is_empty());
    }

    #[test]
    fn the_default_shell_shows_the_editor_on_the_left_and_usage_on_the_right() {
        let shell = fresh("default");
        assert!(shell.panel_visible(&shell::markdown_id()));
        assert!(shell.panel_visible(&shell::usage_id()));
        assert!(!shell.panel_visible(&shell::info_id()));
        assert!(!shell.panel_visible(&shell::apps_id()));
        assert_eq!(shell.shell.side_of(&shell::apps_id()), Some(Side::Right));
    }

    #[test]
    fn a_panel_is_not_visible_when_its_zone_is_collapsed_or_covered_by_the_other_sides_maximize() {
        let mut shell = fresh("visible");
        press(&mut shell, &shell::markdown_id()); // collapses the left zone
        assert!(!shell.panel_visible(&shell::markdown_id()));
        press(&mut shell, &shell::markdown_id()); // expands it again
        assert!(shell.panel_visible(&shell::markdown_id()));
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Right))));
        assert!(!shell.panel_visible(&shell::markdown_id()), "covered by the right side's maximize");
        assert!(shell.panel_visible(&shell::usage_id()));
    }

    #[test]
    fn a_shell_change_is_saved_and_restored_on_the_next_start() {
        let dir = test_temp_dir("saved");
        let path = dir.join("shell.json");
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        assert!(path.exists(), "a layout change is written right away");
        let again = ProductShell::with_shell_path(path);
        assert_eq!(again.shell.selected(Side::Left), &shell::info_id());
    }

    /// 复盘发现 4:高版本布局文件退出时**不得覆盖**。
    #[test]
    fn a_newer_schema_shell_file_is_never_overwritten() {
        let dir = test_temp_dir("newer");
        let path = dir.join("shell.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&path, original).unwrap();
        let mut shell = ProductShell::with_shell_path(path.clone());
        assert!(shell.shell_save_blocked.is_some());
        press(&mut shell, &shell::info_id());
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "the original file stays byte for byte");
        assert!(shell.boot_note.contains("布局未保存"));
    }

    #[test]
    fn the_native_rect_loses_exactly_the_panel_header_and_never_goes_below_one_pixel() {
        let r = below_header(Rect::new(10.0, 100.0, 300.0, 400.0), 96.0);
        assert_eq!((r.x, r.y, r.w, r.h), (10.0, 196.0, 300.0, 304.0));
        assert_eq!(below_header(Rect::new(0.0, 0.0, 50.0, 40.0), 96.0).h, 1.0);
    }

    /// 点到「应用」面板会触发探测任务(`tokio::spawn`),所以需要运行时。
    #[tokio::test]
    async fn the_view_builds_in_the_states_a_user_can_reach() {
        let mut shell = fresh("view");
        let _ = Product::view(&shell);
        for panel in shell::all_panels() {
            press(&mut shell, &panel);
            let _ = Product::view(&shell);
        }
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Left))));
        let _ = Product::view(&shell);
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        let _ = Product::view(&shell);
    }

    /// 复审 #9:损坏账本必须显式 `Unavailable` + **不接线**生产者,而不是退回临时空账本。
    #[tokio::test]
    async fn a_corrupt_usage_ledger_is_unavailable_and_left_untouched() {
        use dozer_services::ServiceErrorKind;

        let dir = test_temp_dir("usage-corrupt");
        let path = dir.join("usage_ledger.json");
        let original = b"{ this is not json";
        std::fs::write(&path, original).unwrap();

        let (service, ledger, note) = load_usage_ledger(path.clone());
        assert!(ledger.is_none(), "损坏时不得接线任何账本(含临时空账本)");
        assert!(note.contains("暂不可用"), "boot 提示应说明不可用:{note}");
        let err = service
            .usage_summary("/p/a".into(), None)
            .await
            .expect_err("损坏账本查询必须 Unavailable,而不是 Ok(空)");
        assert_eq!(err.kind, ServiceErrorKind::Unavailable);

        // 原文件保持原样(未被覆盖/未被悄悄迁移)。
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 复盘发现 6:真实 `ProductShell` 的 Surface 事件经 `update_host` 单次路由,不得无限递归。
    /// `Created`/`Resumed` → 一次 `on_activated`;`Destroyed` → 一次 `on_closed`;`Hidden` → 无回调。
    /// 修复前本用例会因 `route_lifecycle` → `update` → `route_lifecycle` 递归至栈溢出。
    #[test]
    fn surface_events_route_once_without_recursion() {
        let slot = bytehost_desktop::SurfaceSlot(7);
        let mut shell = ProductShell::new();

        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Created 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Resumed { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Resumed 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Hidden { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Hidden 不触发生命周期回调"
        );

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Destroyed { slot }),
        );
        assert_eq!(
            shell.surface_callbacks,
            vec![slot],
            "Destroyed 触发一次关闭"
        );

        // 直接投 Surface 给 `Product::update` 也不得递归(它只消费,不再重投)。
        shell.surface_callbacks.clear();
        let _ = Product::update(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Surface 只在 update_host 路由一次,Product::update 不再分发"
        );
    }
}
```

- [ ] **Step 2: 替换 `crates/desktop/src/lifecycle_selftest.rs`(整份)**

```rust
//! 窗口骨架 + 面板生命周期自检(ADR-013 验证项 5 的骨架版):让真实的产品壳自己点图标、收起 / 展开、拖分隔线、缩放窗口、
//! 把编辑器拖到另一条栏、放大、关闭,每一步之后读**真实 NSView** 的可见性、位置、大小和句柄数,与期望比较。
//!
//!   DIGGER_LIFECYCLE_SELFTEST=1 digger        (通常经 scripts/run-editor-selftest.sh lifecycle)
//!
//! 期望值由本文件**独立**算出(只用下面这几个字面量和算术,不调用骨架的几何函数),所以骨架的几何一旦与实际摆放脱节就会失败。
//! 全部通过退出码 0;任何一项失败、或面板没有按时就绪,退出码非 0。

use std::time::{Duration, Instant};

use bytehost_chrome::{Divider, ShellMsg, Side};
use iced::{Size, Task};

use crate::markdown::{BAR_HEIGHT, Probe};
use crate::product::{Msg, ProductShell};
use crate::shell;

pub const ENV: &str = "DIGGER_LIFECYCLE_SELFTEST";

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    /// 动作之间的停顿,什么也不做
    Pause,
    Act,
    Probe,
    Check,
}

/// 期望编辑器 webview 在哪。`pref` 是持久化的左宽偏好(夹取在算期望时独立做)。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Area {
    Left { pref: f64 },
    Right { pref: f64 },
    Maximized,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Visible(Area),
    Hidden,
    Destroyed,
}

enum Action {
    Msg(ShellMsg),
    Resize(f32, f32),
    Close,
}

struct Step {
    name: &'static str,
    actions: Vec<Action>,
    expect: Expect,
}

fn press(panel: bytehost_core::id::PanelId) -> Action {
    Action::Msg(ShellMsg::PanelPress { panel })
}

fn release() -> Action {
    Action::Msg(ShellMsg::PointerReleased)
}

fn steps() -> Vec<Step> {
    let editor = shell::markdown_id;
    let info = shell::info_id;
    vec![
        Step { name: "S1 the editor is shown in the left zone, below its own bar, with the default 640 px zone", actions: vec![], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step { name: "S2 selecting another panel on the same rail hides the editor (it stays alive)", actions: vec![press(info()), release()], expect: Expect::Hidden },
        Step { name: "S3 selecting the editor again shows it at the same place", actions: vec![press(editor()), release()], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step { name: "S4 clicking the active icon collapses the zone and hides the editor", actions: vec![press(editor()), release()], expect: Expect::Hidden },
        Step { name: "S5 clicking it again expands the zone and shows the editor", actions: vec![press(editor()), release()], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step {
            name: "S6 dragging the divider between the zones resizes the editor",
            actions: vec![Action::Msg(ShellMsg::DividerPress(Divider::Zones)), Action::Msg(ShellMsg::CursorMoved { x: 44.0 + 500.0, y: 300.0 }), release()],
            expect: Expect::Visible(Area::Left { pref: 500.0 }),
        },
        Step { name: "S7 enlarging the window moves and resizes it", actions: vec![Action::Resize(1000.0, 760.0)], expect: Expect::Visible(Area::Left { pref: 500.0 }) },
        Step {
            name: "S8 a resize that happens while the editor is hidden is applied when it comes back",
            actions: vec![press(info()), release(), Action::Resize(1100.0, 780.0), press(editor()), release()],
            expect: Expect::Visible(Area::Left { pref: 500.0 }),
        },
        Step { name: "S9 maximizing its zone fills the maximize box", actions: vec![Action::Msg(ShellMsg::Maximize(Side::Left))], expect: Expect::Visible(Area::Maximized) },
        Step {
            name: "S10 dragging the editor icon to the other rail moves it into the right zone",
            actions: vec![Action::Msg(ShellMsg::MaximizeClose), press(info()), release(), press(editor()), Action::Msg(ShellMsg::RailDragMove { side: Side::Right, index: 1 }), release()],
            expect: Expect::Visible(Area::Right { pref: 500.0 }),
        },
        Step { name: "S11 closing destroys the webview and the handle table is empty again", actions: vec![Action::Close], expect: Expect::Destroyed },
    ]
}

/// 状态机:每一步 Act(做动作)→ 等 → Probe(读真实 view)→ 等 → Check(比较)。
pub struct Driver {
    step: usize,
    steps: Vec<Step>,
    started: Instant,
    passes: u32,
    fails: u32,
}

const SETTLE: Duration = Duration::from_millis(700);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

fn after(delay: Duration, phase: Phase) -> Task<Msg> {
    Task::perform(tokio::time::sleep(delay), move |_| Msg::Lifecycle(phase))
}

// ---- 独立的期望值:只用字面量和算术 ----
const RAIL: f64 = 44.0;
const DIVIDER: f64 = 8.0;
const TOP_BAR: f64 = 40.0;
const FOOTBAR: f64 = 22.0;
const MIN_ZONE: f64 = 320.0;
const MAXIMIZE_PADDING: f64 = 40.0;

/// 编辑器 webview 的期望矩形 (x, y, w, h):面板内容矩形(区外框 − 外边距 − 1px 内边距)再减去编辑器自己的顶栏。
fn want(area: Area, window: (f32, f32)) -> (f64, f64, f64, f64) {
    let (w, h) = (window.0 as f64, window.1 as f64);
    let bar = BAR_HEIGHT;
    let zones = (w - 2.0 * RAIL - DIVIDER).max(0.0);
    let left_w = |pref: f64| pref.clamp(MIN_ZONE, (zones - MIN_ZONE).max(MIN_ZONE));
    let body_h = h - TOP_BAR - FOOTBAR;
    let (x, y, cw, ch) = match area {
        // 左区:外边距 上6 左6,内边距 1
        Area::Left { pref } => (RAIL + 6.0 + 1.0, TOP_BAR + 6.0 + 1.0, left_w(pref) - 6.0 - 2.0, body_h - 6.0 - 2.0),
        // 右区:外边距 上6 右6,内边距 1;它的外框从 左图标栏 + 左区 + 分隔线 之后开始
        Area::Right { pref } => (RAIL + left_w(pref) + DIVIDER + 1.0, TOP_BAR + 6.0 + 1.0, (zones - left_w(pref)) - 6.0 - 2.0, body_h - 6.0 - 2.0),
        // 放大盒子:两条图标栏之间、顶栏之下直到窗口底边,四周留 40
        Area::Maximized => (RAIL + MAXIMIZE_PADDING, TOP_BAR + MAXIMIZE_PADDING, w - 2.0 * RAIL - 2.0 * MAXIMIZE_PADDING, h - TOP_BAR - 2.0 * MAXIMIZE_PADDING),
    };
    (x, y + bar, cw, ch - bar)
}

impl Driver {
    pub fn from_env() -> Option<Self> {
        std::env::var_os(ENV).map(|_| Self { step: 0, steps: steps(), started: Instant::now(), passes: 0, fails: 0 })
    }

    pub fn first() -> Task<Msg> {
        Task::done(Msg::Lifecycle(Phase::Act))
    }

    pub fn advance(&mut self, shell: &mut ProductShell, phase: Phase) -> Task<Msg> {
        match phase {
            Phase::Pause => Task::none(),
            Phase::Act => self.act(shell),
            Phase::Probe => match shell.window_id() {
                Some(id) => shell.markdown_probe(id).chain(after(SETTLE / 2, Phase::Check)),
                None => self.fail_now("no window"),
            },
            Phase::Check => self.check(shell),
        }
    }

    fn act(&mut self, shell: &mut ProductShell) -> Task<Msg> {
        let Some(id) = shell.window_id() else {
            return self.wait_or_die("the window never became ready");
        };
        // 第一步先等页面说过 ready:之后才有「正常显示」可言
        if self.step == 0 && !shell.markdown_ready() {
            return self.wait_or_die("the editor page never became ready");
        }
        // 动作逐个排队,每个之间留一点时间让界面处理完;最后统一等 SETTLE 再读真实 view
        let mut chain: Task<Msg> = Task::none();
        for action in &self.steps[self.step].actions {
            let t = match action {
                Action::Msg(m) => Task::done(Msg::Shell(m.clone())),
                Action::Resize(w, h) => iced::window::resize(id, Size::new(*w, *h)),
                Action::Close => Task::done(Msg::Exit),
            };
            chain = chain.chain(t).chain(after(Duration::from_millis(250), Phase::Pause));
        }
        chain.chain(after(SETTLE, Phase::Probe))
    }

    fn wait_or_die(&mut self, why: &str) -> Task<Msg> {
        if self.started.elapsed() > READY_TIMEOUT {
            return self.fail_now(why);
        }
        after(Duration::from_millis(300), Phase::Act)
    }

    fn fail_now(&mut self, why: &str) -> Task<Msg> {
        println!("FAIL: lifecycle selftest could not run: {why}"); // cli-output
        std::process::exit(1);
    }

    fn check(&mut self, shell: &mut ProductShell) -> Task<Msg> {
        let step = &self.steps[self.step];
        let (ok, detail) = judge(step.expect, shell.markdown_last_probe(), shell.window_size());
        println!("{}: {}{}", if ok { "PASS" } else { "FAIL" }, step.name, if detail.is_empty() { String::new() } else { format!("\n      {detail}") }); // cli-output
        if ok {
            self.passes += 1
        } else {
            self.fails += 1
        }
        self.step += 1;
        if self.step < self.steps.len() {
            return Task::done(Msg::Lifecycle(Phase::Act));
        }
        println!("SUMMARY shell lifecycle selftest: {} passed, {} failed", self.passes, self.fails); // cli-output
        std::process::exit(i32::from(self.fails > 0));
    }
}

/// 原生 view 的大小是取整的,所以容差取半个像素:够吸收取整,又抓得住 1 像素的几何漂移。
fn near(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    const TOLERANCE: f64 = 0.51;
    (a.0 - b.0).abs() <= TOLERANCE && (a.1 - b.1).abs() <= TOLERANCE && (a.2 - b.2).abs() <= TOLERANCE && (a.3 - b.3).abs() <= TOLERANCE
}

/// 把真实 view 的状态与期望比较。纯函数,可单测。
fn judge(expect: Expect, probe: Option<Probe>, window: (f32, f32)) -> (bool, String) {
    let Some(p) = probe else {
        return (false, "no probe result arrived".into());
    };
    let (ok, want_detail) = match expect {
        Expect::Visible(area) => {
            let want = want(area, window);
            (p.exists && p.visible && p.handles == 1 && p.bounds.is_some_and(|b| near(b, want)), format!("want bounds {want:?}"))
        }
        Expect::Hidden => (p.exists && !p.visible && p.handles == 1, "want hidden but alive".to_owned()),
        Expect::Destroyed => (!p.exists && p.handles == 0, "want destroyed, no handles".to_owned()),
    };
    (ok, if ok { String::new() } else { format!("probe {p:?}, {want_detail}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(exists: bool, visible: bool, bounds: Option<(f64, f64, f64, f64)>, handles: usize) -> Option<Probe> {
        Some(Probe { exists, visible, bounds, handles })
    }

    const WINDOW: (f32, f32) = (1440.0, 900.0);

    #[test]
    fn the_expected_rectangles_are_the_hand_computed_ones() {
        // left zone 640: content x 44+6+1, y 40+6+1, w 640-8, h 900-40-22-8; the editor bar (56) comes off the top
        assert_eq!(want(Area::Left { pref: 640.0 }, WINDOW), (51.0, 103.0, 632.0, 774.0));
        // a narrow window clamps the preference: W=700 ⇒ zones 604, upper max(604-320,320)=320
        assert_eq!(want(Area::Left { pref: 640.0 }, (700.0, 900.0)).2, 320.0 - 8.0);
        // right zone with left 500: zones 1344, right = 844; x = 44+500+8+1
        assert_eq!(want(Area::Right { pref: 500.0 }, WINDOW), (553.0, 103.0, 844.0 - 8.0, 774.0));
        // maximize box: x 84, y 80+56, w 1440-88-80, h 900-40-80-56
        assert_eq!(want(Area::Maximized, WINDOW), (84.0, 136.0, 1272.0, 724.0));
    }

    #[test]
    fn judge_accepts_the_right_state_and_refuses_each_wrong_one() {
        let w = want(Area::Left { pref: 640.0 }, WINDOW);
        let visible = Expect::Visible(Area::Left { pref: 640.0 });
        assert!(judge(visible, probe(true, true, Some(w), 1), WINDOW).0);
        assert!(judge(Expect::Hidden, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(judge(Expect::Destroyed, probe(false, false, None, 0), WINDOW).0);
        // hidden when it should show / wrong place / wrong size / leaked handle / not destroyed / no probe
        assert!(!judge(visible, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some((w.0, w.1 - 56.0, w.2, w.3)), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some((w.0, w.1, w.2 - 40.0, w.3)), 1), WINDOW).0);
        // a one pixel drift is caught, half a pixel of rounding is not
        assert!(!judge(visible, probe(true, true, Some((w.0 + 1.0, w.1, w.2, w.3)), 1), WINDOW).0);
        assert!(judge(visible, probe(true, true, Some((w.0 + 0.4, w.1, w.2 - 0.4, w.3)), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some(w), 2), WINDOW).0);
        assert!(!judge(Expect::Hidden, probe(true, true, Some(w), 1), WINDOW).0);
        assert!(!judge(Expect::Hidden, probe(false, false, None, 0), WINDOW).0, "hidden is not destroyed");
        assert!(!judge(Expect::Destroyed, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(!judge(visible, None, WINDOW).0);
    }

    #[test]
    fn every_step_has_a_name_and_the_last_one_closes_the_window() {
        let steps = steps();
        assert_eq!(steps.len(), 11);
        assert!(steps.iter().all(|s| !s.name.is_empty()));
        assert!(matches!(steps.last().unwrap().actions.as_slice(), [Action::Close]));
    }
}
```

- [ ] **Step 3: `paths.rs` 删掉旧的 `layout_path`**

删除 `pub fn layout_path()` 及其文档注释,以及测试 `layout_path_is_under_config`(`shell_path_is_under_config` 已在 Task 11 加好)。把剩下的注释里提到 `layout.json` 的地方改成 `shell.json`。

- [ ] **Step 4: 运行**

```bash
cd $ROOT/digger
cargo test -p digger-desktop 2>&1 | grep -E "^(error|warning: unused)|test result|FAILED|panicked"
```

Expected: `test result: ok. 76 passed; 0 failed`。其中新增的:`product::tests::{registers_product_panels, the_default_shell_shows_the_editor_on_the_left_and_usage_on_the_right, a_panel_is_not_visible_when_its_zone_is_collapsed_or_covered_by_the_other_sides_maximize, a_shell_change_is_saved_and_restored_on_the_next_start, a_newer_schema_shell_file_is_never_overwritten, the_native_rect_loses_exactly_the_panel_header_and_never_goes_below_one_pixel, the_view_builds_in_the_states_a_user_can_reach}`、`lifecycle_selftest::tests::*`(3 个)。

- [ ] **Step 5: 其余 crate 与依赖门禁不受影响**

```bash
cargo test -p digger-editor -p digger-app-host -p digger-store --lib 2>&1 | grep -E "test result|FAILED"
bash scripts/check-host-deps.sh 2>&1 | tail -1
```

Expected: 无 `FAILED`;`digger host-deps check: ok(审视了 2 个业务 crate)`。

- [ ] **Step 6: Commit**

```bash
git add crates/desktop
git commit -m "feat(desktop): drive the product shell with the shared window shell

The tab strip and ActivePanel are gone. Native webview rectangles and
visibility now come from the shell's panel content rectangles; each panel only
knows how tall its own header is. The lifecycle selftest becomes the shell
version: eleven steps read the real NSView against hand-computed expectations.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 13: 窗口装饰 —— 透明标题栏

**Files:**
- Modify: `crates/desktop/src/main.rs`

**Interfaces:**
- Consumes: `byteui::theme::geometry::{min_window_width, min_window_height}`(已有);`product` 里 `Msg::WindowReady` 已在 Task 12 里装好全屏保护并重设红绿灯。
- Produces: 窗口用"透明标题栏 + 全尺寸内容视图",顶栏取代系统标题栏。

- [ ] **Step 1: 改 `main()` 里的窗口设置**

把

```rust
        .window(iced::window::Settings {
            exit_on_close_request: false,
            min_size: Some(iced::Size::new(640.0, 480.0)),
            ..Default::default()
        })
```

改为:

```rust
        .window(iced::window::Settings {
            exit_on_close_request: false,
            min_size: Some(iced::Size::new(byteui::theme::geometry::min_window_width(), byteui::theme::geometry::min_window_height())),
            // 与 Dozer 一致:透明标题栏 + 全尺寸内容视图,顶栏取代系统标题栏(红绿灯由 `bytehost-chrome` 摆进顶栏里)。
            platform_specific: iced::window::settings::PlatformSpecific { title_hidden: true, titlebar_transparent: true, fullsize_content_view: true },
            ..Default::default()
        })
```

- [ ] **Step 2: 编译并跑测试**

```bash
cargo build -p digger-desktop --bin digger 2>&1 | grep -E "^(error|warning: unused)" -A6 | head
cargo test -p digger-desktop 2>&1 | grep -E "test result|FAILED"
```

Expected: 无 error;`76 passed`。

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/src/main.rs
git commit -m "feat(desktop): transparent title bar and full-size content view, like Dozer

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 14: 真实窗口自检与脚本

**Files:**
- Modify: `scripts/run-editor-selftest.sh`

**Interfaces:**
- Consumes: Task 12 的 `DIGGER_LIFECYCLE_SELFTEST`(骨架版 11 步)、既有 `DIGGER_EDITOR_SELFTEST`(harness)
- Produces: `scripts/run-editor-selftest.sh [harness|lifecycle]`;没有 `SUMMARY` 行一律算失败(已有),并在 harness 卡住时给出"屏幕息屏 / 锁屏"的提示

- [ ] **Step 1: 改脚本的说明和失败提示**

脚本开头的说明改为:

```bash
#!/usr/bin/env bash
#   scripts/run-editor-selftest.sh              harness:T1–T23 + T17b(验证项 4)
#   scripts/run-editor-selftest.sh lifecycle    窗口骨架 + 面板生命周期:点图标 / 收起 / 拖分隔线 / 缩放 / 放大 /
#                                               跨栏移动 / 关闭,读真实 NSView(ADR-013 验证项 5;S1 骨架版)
#
# 在产品二进制上跑编辑器自检(ADR-013 验证项 4):用 harness 页面启动真实的 digger,
# 走生产的 webview 构建器与生产的桥,自动跑 T1–T23,任何一项失败(或没跑完)都以非零退出。
# 需要图形会话(开一个真实窗口)。harness 模式还要求屏幕亮着且未锁:屏幕息屏 / 锁屏时页面的动画帧会被暂停,
# harness 会卡在 T6;lifecycle 模式只读 NSView 状态,不依赖动画帧。
```

没有 `SUMMARY` 那段改为:

```bash
if ! grep -q '^SUMMARY' "$out"; then
  echo "editor selftest: 失败(没有 SUMMARY 行:自检没有跑完)" >&2
  echo "  harness 卡在 T6 多半是屏幕息屏或锁屏(页面的动画帧被暂停):唤醒并解锁屏幕后重试" >&2
  exit 1
fi
```

```bash
bash -n scripts/run-editor-selftest.sh && echo syntax-ok
```

- [ ] **Step 2: 跑骨架版生命周期自检**

```bash
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | tail -16
```

Expected: S1…S11 全部 `PASS`,`SUMMARY shell lifecycle selftest: 11 passed, 0 failed`,最后一行 `editor selftest: ok`。(会弹出一个窗口,自己点图标、拖分隔线、缩放,几秒后自己关。)

- [ ] **Step 3: 变异检查 —— 自检必须真的抓得住问题**

变异 A(几何漂移 1px):

```bash
sed -i '' 's/margin: \[6.0, 0.0, 0.0, 6.0\] };/margin: [6.0, 0.0, 0.0, 7.0] };/' $ROOT/byteui-shell/src/theme/region.rs
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | grep -E "^(FAIL|SUMMARY)|selftest:"
git -C $ROOT/byteui-shell checkout src/theme/region.rs
```

Expected: S1、S3、S5、S6、S7、S8 等失败,`SUMMARY … 5 passed, 6 failed`,`editor selftest: 失败`。还原后 `git -C $ROOT/byteui-shell status --short` 为空。

变异 B(切走不隐藏):

```bash
cp crates/desktop/src/markdown/mod.rs /tmp/markdown_mod.bak
python3 - <<'PYEOF'
p='crates/desktop/src/markdown/mod.rs'
s=open(p).read()
s=s.replace("host::set_visible(generation, false);","let _ = generation;",1)
open(p,'w').write(s)
PYEOF
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | grep -E "^(FAIL|SUMMARY)|selftest:"
cp /tmp/markdown_mod.bak crates/desktop/src/markdown/mod.rs
git status --short crates/desktop
```

Expected: S2、S4 失败,`SUMMARY … 9 passed, 2 failed`。还原后 `git status` 对 `crates/desktop` 为空。

- [ ] **Step 4: 还原后确认回到全绿**

```bash
bash scripts/run-editor-selftest.sh lifecycle 2>&1 | tail -2
```

Expected: `SUMMARY shell lifecycle selftest: 11 passed, 0 failed` 和 `editor selftest: ok`。

- [ ] **Step 5: harness 自检不回归(需要屏幕亮着且未锁)**

```bash
bash scripts/run-editor-selftest.sh 2>&1 | grep -E "FAIL|SUMMARY|selftest:"
```

Expected: `SUMMARY digger-editor selftest: 24 passed, 0 failed` 和 `editor selftest: ok`。若卡住后提示"屏幕息屏或锁屏",唤醒并解锁屏幕后重试;不要把它当成失败去改代码。

- [ ] **Step 6: Commit**

```bash
git add scripts/run-editor-selftest.sh
git commit -m "test(scripts): shell lifecycle selftest, and a hint when the harness stalls on a sleeping display

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 15: 文档、整体验证与真机验收清单

**Files:**
- Modify: `docs/adr/ADR-013-markdown-panel-from-md-engine-spike.md`、`README.md`、`docs/superpowers/specs/2026-10-14-window-shell-design.md`(状态行)

- [ ] **Step 1: ADR-013 追加一段**

在 `## Implementation status (2026-10-14)` 一节末尾追加:

```markdown
**Update (2026-10-14, S1 window shell).** The product shell is now driven by the shared window shell
(`bytehost-chrome`; spec `docs/superpowers/specs/2026-10-14-window-shell-design.md`). The tab strip and the
per-panel `sync_native` rectangles described in the deviations above are replaced: the editor's native rectangle
and visibility come from the shell's panel content rectangle, minus the editor's own 56 px bar. `digger.markdown`
sits in the left rail by default. The lifecycle selftest (validation 5) became the shell version
(`scripts/run-editor-selftest.sh lifecycle`, eleven steps against hand-computed rectangles).
```

- [ ] **Step 2: README 的文档清单里加一行**

在 `README.md` 里 ADR-013 那一行之后加:

```markdown
- [S1 窗口骨架:共享 bytehost-chrome](docs/superpowers/specs/2026-10-14-window-shell-design.md)
```

- [ ] **Step 3: spec 的状态行**

`docs/superpowers/specs/2026-10-14-window-shell-design.md` 的 `- 状态:…` 改为 `- 状态:已实现(待真机验收,见计划 Task 15);2026-10-14 读完 Dozer 实现后修订(见 §8)`。

- [ ] **Step 4: 整体验证**

```bash
cd $ROOT/digger
cargo test -p digger-desktop -p digger-app-host -p digger-store -p digger-editor --lib --bins 2>&1 | grep -E "test result|FAILED|error"
bash scripts/check-host-deps.sh 2>&1 | tail -1
BYTEUI=$ROOT/byteui-shell bash scripts/check-shell-regions.sh
bash scripts/build-editor-web.sh --check
cd $ROOT/bytehost-chrome && cargo test -p bytehost-chrome 2>&1 | grep -E "test result|FAILED"
cd $ROOT/byteui-shell && cargo test --lib 2>&1 | grep -E "test result|FAILED"
git -C $ROOT/bytehost status --short | wc -l        # 仍是 N
git -C $ROOT/digger status --short                  # 只剩 .dozer/ 与 spike/corpus/
```

Expected: 所有套件无 `FAILED`(`digger-desktop` 76、`bytehost-chrome` 63、byteui 83);门禁、一致性、网页包检查都打印 `ok`;bytehost 主工作区文件数不变。

- [ ] **Step 5: Commit**

```bash
cd $ROOT/digger
git add docs README.md
git commit -m "docs: record the S1 window shell in ADR-013, README and the spec

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: 真机验收清单(交给用户,执行者不要替用户勾选)**

这几项在临时验证环境里**做不了**(没有可用的显示器 / 截图),必须在用户亮屏的机器上做:

1. **与 Dozer 的视觉对照**:`cargo run -p digger-desktop` 和 Dozer 并排,逐项看:顶栏高度与底线、红绿灯是否垂直居中在顶栏里、左右图标栏宽与图标间距、面板区的圆角外框与悬浮留白(左 6 / 右 6)、分隔线、选中图标的金框。**S1 顶栏左侧的"Digger"标题目前是纯展示的图标 + 文字**,Dozer 的是页签样式的按钮;如果对照下来差别明显,单列一个小任务补页签样式(不要改共享骨架的结构)。
2. **应用 / 用量面板的原生视图顶边**:两个面板的 webview 应当紧贴各自面板头部下沿。`product.rs` 里 `APPS_HEADER_HEIGHT = 96.0`、`USAGE_HEADER_HEIGHT = 80.0` 是按旧布局估的值,目测不对就调这两个常量。
3. **顶栏双击缩放**:在顶栏空白处双击一次,窗口应只缩放 / 还原**一次**;如果系统原生的双击标题栏(顶部约 28pt 内)和我们的处理叠在一起导致缩放两次(等于没变),把 `on_top_bar_double_click` 限制到原生拖动带以外。
4. **图标栏手感**:点图标切换 / 收起 / 展开;按住拖到同栏另一位置(相邻图标让位滑动);拖到另一条栏;拖出窗口后松手;拖着图标时切到别的应用再切回来(不应卡在拖拽态)。
5. **分隔线与缩放**:拖左右分隔线,窗口缩到最窄再拉宽(偏好的左宽应在窗口拉宽后自动恢复);全屏进出后红绿灯位置正确。
6. **持久化**:改了布局后退出再启动,位置、选中、收起、窗口大小都恢复;手动把 `<config>/shell.json` 改坏,启动不崩溃且回到默认布局。
7. **harness 自检**(`bash scripts/run-editor-selftest.sh`,亮屏):`24 passed, 0 failed`。
8. **IME**:编辑器里中日文输入法仍然正常(骨架改了它的容器,没改它的内容)。

---

### Task 16: 发布接线 —— 打 tag、撤掉开发期 `[patch]`(**需要用户批准,执行者到此停手**)

这一步要推送、打 tag、并处理 bytehost 主线上他人的未提交改动,都是对外可见或不可逆的动作。**没有用户明确批准,不要执行。**

**用户需要先决定:**
- `byteui` 的 `feat/shell-regions` 合并进 `main` 并打 `v0.4.3`(干净,无冲突风险)。
- `bytehost` 的 `feat/chrome`(从 `v0.2.0` 切出,只含新 crate)如何并入 `main`:`main` 上现在有他人未提交的 i18n 改动,合并时要先由他们提交;并入后打新 tag(下文记作 `vX.Y.Z`)。**并入 i18n 之后 bytehost 的接口会变**(例如 `AppSummary` 多了 `display_i18n`、`AppSurfaceBuilt` 的形状变了),Digger 的 `app-host` / `apps` 要随之适配 —— 那是另一件事,不属于 S1。

**批准后的步骤(草案,到时按实际 tag 名替换):**

- [ ] bytehost 的 `feat/chrome` 里:删掉根 `Cargo.toml` 末尾的 `[patch."https://github.com/byteboyai/byteui"]` 段;`crates/bytehost-chrome/Cargo.toml` 里 byteui 依赖的 `tag` 改成 `v0.4.3`;`cargo test -p bytehost-chrome` 通过后提交。
- [ ] Digger 的根 `Cargo.toml`:`bytehost-*` 全部改到 `vX.Y.Z`,`byteui` 改到 `v0.4.3`;删掉两段开发期 `[patch]`;`cargo update`;跑 Task 15 Step 4 的全部验证。
- [ ] 清理 worktree:`git -C $ROOT/bytehost worktree remove ../bytehost-chrome`、`git -C $ROOT/byteui worktree remove ../byteui-shell`。
- [ ] Dozer 迁移到共享骨架是另一个子项目,不在这里。

---

## Self-Review(写完计划后的核对)

- **Spec 覆盖**:§4.1 边界 → Task 3–9;§4.2 状态与不变量 → Task 6;持久化与消毒 → Task 7;§4.3 度量与几何 → Task 1、3、8;原生 webview 规则 → Task 12 的 `sync_native` + Task 14;§4.4 Digger 一侧(默认布局、顶栏结构、窗口装饰、放大态只有消息入口、底栏占位槽)→ Task 11–13;§5 三仓库与 worktree 隔离 → Task 0、16;§6 验收 1–5 → Task 3–7、11、14、15;§7 风险 → 开头"没有验证到的"与 Task 15 Step 6。
- **与 spec 有意不同的两处**(已在对应任务写明原因):`shell.json` 用自己的 DTO 而不是 `bytehost_core::Layout`(骨架恒为左右两栏,`Layout` 是 N 栏且栏宽语义不同);读盘时缺失的面板回到默认栏而不是整体回落(Dozer 为此不得不写迁移函数)。spec §4.2 与 §8 已同步这两处。
- **占位符**:代码步骤都给出了完整代码(取自已编译、已测试的文件)。Task 16 标明是"草案、需批准",这是有意的。
- **类型一致性**:`ShellMsg` / `Outcome` / `Defaults` / `Loaded` / `PanelRules` / `ShellHost` / `TopBarSlots` / `BodyArea` 的名字与签名在 Task 3–12 之间取自同一批已编译的文件,没有手抄。
