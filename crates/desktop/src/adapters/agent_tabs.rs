//! Agent 页签四件与溢出菜单(S6d-3 Task 2)。
//!
//! 页签、溢出入口、溢出下拉的渲染已迁到 `byteui::interaction::panel_tabs`(X1,与 Dozer 共用;原先
//! 这里抄过一份 Dozer 的 `chrome/tab_widget.rs`)。本文件只剩把库的显示参数(`TabArgs`、`OverflowEntry`,
//! 带状态点颜色)映射到 byteui 部件的接线,以及 Agent 页签条自己的布局常量。
//!
//! Digger 的已知差异(设计 D3):悬停是阶跃不是渐变;延迟提示精度是 1 秒 `Tick`。
//!
//! **溢出下拉的几何现在和 Dozer 一致,不随 UI 缩放**(宽度夹在 220–560、边距 24、高度上限 320,都是逻辑像素)。
//! 迁移前这里的 `overflow_geometry` 把三个常量都乘了 scale;scale = 1 时两者完全相同(byteui 里有等价性测试),
//! 只有放大/缩小界面时下拉的宽度与高度上限会和以前不同。

use byteui::interaction::panel_tabs::{self, PanelTabArgs, TabOverflowEntry, TabOverflowMenuArgs};
use dozer_panel_agent::display::{El, OverflowEntry, TabArgs};
#[cfg(test)]
use dozer_panel_agent::state::AgentPanelState;
#[cfg(test)]
use iced_widget::core::Color;
use iced_widget::core::Element;
#[cfg(test)]
use iced_widget::text;

use crate::product::Msg;

pub use byteui::interaction::panel_tabs::{close_color, combined_hover, title_color};

/// 页签条右侧固定占宽:两颗按钮(折叠 / 溢出)32×2 + 间距 4×2。
const TAB_BAR_RESERVE: f32 = 72.0;

/// 页签条可用宽度 = 终端块宽 − 72·scale(不小于 0)。
pub fn tab_bar_avail_px(block_width: f32, scale: f32) -> f32 {
    (block_width - TAB_BAR_RESERVE * scale).max(0.0)
}

/// 一个横向页签。`hover`/`close_hover` 是产品阶跃悬停值(0 或 1)。渲染委托给 byteui 的 `panel_tab`,
/// 标题前带库给的状态点。
pub fn panel_tab<'a, F1, F2>(
    args: TabArgs<Msg>,
    hover: f32,
    close_hover: f32,
    show_tooltip: bool,
    on_select_hover: F1,
    on_close_hover: F2,
) -> El<'a, Msg>
where
    F1: Fn(bool) -> Msg + 'a,
    F2: Fn(bool) -> Msg + 'a,
{
    panel_tabs::panel_tab(PanelTabArgs {
        title: args.title,
        active: args.active,
        hover_t: hover,
        close_hover_t: close_hover,
        prefix: Some(byteui::feedback::status::dot(args.dot)),
        suffix: None,
        on_select: args.on_select,
        on_close: args.on_close,
        show_tooltip,
        title_hover: on_select_hover,
        close_hover: on_close_hover,
    })
}

/// 延迟提示:气泡恒在页签上方(`Top`),2 秒延迟与结构恒定见 byteui 的 `controlled_tooltip`。
pub fn controlled_tooltip<'a, M>(
    content: Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>,
    label: String,
    show: bool,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
{
    byteui::feedback::tooltip::controlled_tooltip(
        content,
        label,
        iced_widget::tooltip::Position::Top,
        show,
    )
}

/// 溢出入口:`ChevronsDown`;页签组非空时总显示(即使当前没有横向溢出)。
pub fn overflow_button<'a>(
    tab_count: usize,
    hover_t: f32,
    on_press: Msg,
    on_hover: impl Fn(bool) -> Msg + 'a,
) -> Option<El<'a, Msg>> {
    panel_tabs::tab_overflow_button(tab_count, hover_t, on_press, on_hover)
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct OverflowRowView {
    pub title: String,
    pub active: bool,
    pub dot: Color,
}

/// 当前状态 → 每页签一行,顺序与页签一致。
#[cfg(test)]
pub fn row_views(state: &AgentPanelState) -> Vec<OverflowRowView> {
    state
        .tabs
        .iter()
        .map(|tab| OverflowRowView {
            title: dozer_panel_agent::tabs::tab_title(
                tab.agent,
                tab.cwd.as_deref(),
                &tab.info.name,
            ),
            active: false,
            dot: dozer_panel_agent::style::dot_color(tab.agent_state, tab.alive),
        })
        .enumerate()
        .map(|(idx, mut row)| {
            row.active = idx == state.active;
            row
        })
        .collect()
}

/// 溢出下拉:固定宽 `>` 列(仅选中行有箭头,其余留白,保证文字起点相同)+ 状态点 + 标题 + 关闭 ×。
/// 渲染与定位委托给 byteui 的 `tab_overflow_menu`;`hover_of` 把库的行悬停键换成这一行的悬停进度。
#[allow(clippy::too_many_arguments)] // 参数逐项对应库的 overflow 接线 + 行 hover。
pub fn overflow_menu<'a, FSel, FClose, FHover, FRowHover>(
    entries: Vec<OverflowEntry>,
    anchor: (f32, f32),
    window: (f32, f32),
    on_select: FSel,
    on_close: FClose,
    on_row_hover: FRowHover,
    hover_of: FHover,
    on_dismiss: Msg,
) -> El<'a, Msg>
where
    FSel: Fn(usize) -> Msg + 'a,
    FClose: Fn(usize) -> Msg + 'a,
    FRowHover: Fn(usize, bool) -> Msg + 'a,
    FHover: Fn(dozer_panel_agent::display::AgentHover) -> f32 + 'a,
{
    panel_tabs::tab_overflow_menu(TabOverflowMenuArgs {
        entries: entries
            .into_iter()
            .map(|entry| TabOverflowEntry {
                index: entry.index,
                prefix: Some(byteui::feedback::status::dot(entry.dot)),
                title: entry.title,
                active: entry.active,
                closable: true,
                hover_t: hover_of(entry.row),
            })
            .collect(),
        anchor,
        window_size: window,
        on_select,
        on_close,
        on_dismiss,
        on_row_hover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::{AgentKind, AgentState, SessionInfo};
    use dozer_panel_agent::display::AgentHover;
    use dozer_panel_agent::tabs::{SessionTab, TabBackend};
    use dozer_panel_term::model::TerminalModel;
    use dozer_panel_term::osc::OscScanner;

    #[test]
    fn the_tooltip_widget_tree_shape_does_not_depend_on_show() {
        // 到期才把页签包进 `Tooltip`,控件树结构随之翻转,里面 `MouseArea` 的状态被重置成
        // "未悬停":下一帧会再发一次 enter(重置提示计时,气泡闪一下就没),翻转那一帧
        // 若光标已离开则 exit 永远不触发(悬停底色卡住)。结构必须恒定,延迟交给
        // `Tooltip::delay`。
        let shape = |show: bool| {
            let content: Element<'_, (), iced_widget::Theme, iced_renderer::Renderer> =
                text("tab").into();
            let el = controlled_tooltip(content, "full title".into(), show);
            (el.as_widget().tag(), el.as_widget().children().len())
        };
        assert_eq!(shape(true), shape(false));
    }

    #[test]
    fn tab_bar_avail_is_block_width_minus_72_and_never_negative() {
        assert_eq!(tab_bar_avail_px(400.0, 1.0), 328.0);
        assert_eq!(tab_bar_avail_px(400.0, 2.0), 256.0);
        assert_eq!(tab_bar_avail_px(40.0, 1.0), 0.0);
    }

    fn approx(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-5
            && (a.g - b.g).abs() < 1e-5
            && (a.b - b.b).abs() < 1e-5
            && (a.a - b.a).abs() < 1e-5
    }

    #[test]
    fn tab_title_font_is_medium_like_dozers_top_bar_font() {
        assert_eq!(
            byteui::theme::font::top_bar_font().weight,
            iced_widget::core::font::Weight::Medium
        );
    }

    #[test]
    fn hovering_the_close_button_also_shows_the_capsule() {
        assert_eq!(combined_hover(0.0, 1.0), 1.0);
        assert_eq!(combined_hover(1.0, 0.0), 1.0);
        assert_eq!(combined_hover(0.0, 0.0), 0.0);
    }

    #[test]
    fn inactive_overflow_row_titles_are_dim_and_active_is_cream() {
        assert_eq!(title_color(false, 0.0), byteui::theme::color::current().dim);
        assert_eq!(
            title_color(true, 0.0),
            byteui::theme::color::current().cream
        );
    }

    #[test]
    fn tab_title_colors_follow_active_and_hover() {
        let colors = byteui::theme::color::current();
        assert_eq!(title_color(true, 0.0), colors.cream);
        assert_eq!(title_color(true, 1.0), colors.cream);
        assert_eq!(title_color(false, 0.0), colors.dim);
        assert!(approx(title_color(false, 1.0), colors.gold));
    }

    #[test]
    fn close_button_is_invisible_at_rest_and_gold_on_hover() {
        assert_eq!(close_color(0.0, 0.0).a, 0.0);
        let hovered = close_color(1.0, 1.0);
        assert_eq!(hovered.a, 1.0);
        assert!(approx(hovered, byteui::theme::color::current().gold));
    }

    fn tab(id: &str, state: AgentState, alive: bool) -> SessionTab {
        SessionTab {
            info: SessionInfo {
                id: id.into(),
                name: id.into(),
                command: "sh".into(),
                cwd: "/tmp".into(),
                alive,
                created_ms: 0,
                agent_state: state,
                transcript_path: None,
                project_id: Some(7),
                agent: AgentKind::Unknown,
            },
            model: TerminalModel::new(20, 5),
            alive,
            agent_state: state,
            agent: AgentKind::Unknown,
            transcript_path: None,
            osc: OscScanner::new(),
            cwd: None,
            last_exit: None,
            llm_model: None,
            permission_mode: None,
            last_activity: None,
            workspace_override: None,
            tab_id: 0,
            forwarder: tokio::runtime::Handle::current().spawn(async {}),
            backend: TabBackend::Daemon,
        }
    }

    #[tokio::test]
    async fn overflow_menu_has_one_row_per_tab_with_the_chevron_only_on_the_active_row() {
        let mut state = AgentPanelState::new();
        state.tabs.push(tab("a", AgentState::Running, true));
        state.tabs.push(tab("b", AgentState::Idle, true));
        state.active = 1;
        let rows = row_views(&state);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter().map(|r| r.active).collect::<Vec<_>>(),
            [false, true]
        );
        assert_eq!(rows[0].title, "a");
    }

    #[test]
    fn panel_tab_with_empty_or_huge_titles_builds_without_panicking() {
        for title in ["", "a".repeat(10_000).as_str(), "中文标题", "emoji 🚀"] {
            let args = TabArgs {
                title: title.to_string(),
                active: false,
                dot: byteui::theme::color::current().cyan,
                item: AgentHover::TabItem(0),
                close: AgentHover::TabClose(0),
                on_select: Msg::ZoomToggle,
                on_close: Msg::ZoomToggle,
            };
            let _ = panel_tab(
                args,
                0.0,
                0.0,
                false,
                |_| Msg::ZoomToggle,
                |_| Msg::ZoomToggle,
            );
            let _ = panel_tab(
                args2(title),
                1.0,
                1.0,
                true,
                |_| Msg::ZoomToggle,
                |_| Msg::ZoomToggle,
            );
        }
    }

    fn args2(title: &str) -> TabArgs<Msg> {
        TabArgs {
            title: title.to_string(),
            active: true,
            dot: byteui::theme::color::current().red,
            item: AgentHover::TabItem(1),
            close: AgentHover::TabClose(1),
            on_select: Msg::ZoomToggle,
            on_close: Msg::ZoomToggle,
        }
    }

    #[test]
    fn dismiss_layer_is_part_of_the_menu_element() {
        // 菜单外框带整窗透明遮罩(byteui 的 `tab_overflow_menu`);空条目也应能构造。
        let el = overflow_menu(
            Vec::new(),
            (10.0, 10.0),
            (800.0, 600.0),
            |_| Msg::ZoomToggle,
            |_| Msg::ZoomToggle,
            |_, _| Msg::ZoomToggle,
            |_| 0.0,
            Msg::ZoomToggle,
        );
        let _ = el;
    }

    #[test]
    fn overflow_menu_builds_at_every_corner_and_in_tiny_windows() {
        // 定位数学现在在 byteui(逐字搬自 Dozer);这里只钉住 Digger 的接线在极端锚点/窗口下不 panic。
        for window in [(800.0, 600.0), (100.0, 100.0), (0.0, 0.0)] {
            for anchor in [
                (-50.0, -50.0),
                (790.0, 590.0),
                (400.0, 300.0),
                (5000.0, 5000.0),
            ] {
                let _ = overflow_menu(
                    vec![OverflowEntry {
                        index: 0,
                        title: "t".repeat(500),
                        active: true,
                        dot: byteui::theme::color::current().cyan,
                        row: AgentHover::TabItem(0),
                    }],
                    anchor,
                    window,
                    |_| Msg::ZoomToggle,
                    |_| Msg::ZoomToggle,
                    |_, _| Msg::ZoomToggle,
                    |_| 0.0,
                    Msg::ZoomToggle,
                );
            }
        }
    }
}
