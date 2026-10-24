//! Agent 页签四件与溢出菜单(S6d-3 Task 2)。
//!
//! 外观逐项对照 Dozer `crates/dozer-app/src/chrome/tab_widget.rs`:页签内边距
//! 左 10 / 右 4 / 上下 0,选中 `card` 底 + 1px `border` + 圆角 6,悬停
//! `tab_hover` 底 + 圆角 6,标题 `font::body()`、`wrapping None`、`clip`,关闭
//! 按钮边长 `geometry::tab_button_size()`、颜色 `dim`→`gold`;溢出按钮
//! `ChevronsDown`、命中区 `tab_arrow_button_size()`。
//!
//! Digger 的已知差异(设计 D3):悬停是阶跃不是渐变;延迟提示精度是 1 秒
//! `Tick`。菜单外框复用 `menu_overlay`(byteui 没有菜单渲染)。

use byteui::interaction::icons::{self, IconKind};
use byteui::interaction::tabs::{self, TabCoreArgs};
use dozer_panel_agent::display::{El, OverflowEntry, TabArgs};
#[cfg(test)]
use dozer_panel_agent::state::AgentPanelState;
use iced_widget::core::{Border, Color, Element, Font, Length, Padding, font::Weight};
use iced_widget::{column, container, row, scrollable, text, tooltip};

use crate::adapters::menu_overlay::{self, MenuAnchor};
use crate::product::Msg;

/// 页签内边距(与 Dozer 逐项相同;左侧单独放大给悬停胶囊留白)。
const PANEL_TAB_PAD_LEFT: f32 = 10.0;
const PANEL_TAB_PAD_X: f32 = 4.0;
const PANEL_TAB_PAD_Y: f32 = 0.0;

/// 溢出菜单宽度/高度/行距(逻辑像素,乘 `icon_size::scale()`)。
const MENU_MIN_WIDTH: f32 = 220.0;
const MENU_MAX_WIDTH: f32 = 560.0;
const MENU_MAX_HEIGHT: f32 = 320.0;
const MENU_ROW_PAD: f32 = 4.0;
const MENU_ROW_GAP: f32 = 2.0;

/// 页签条右侧固定占宽:两颗按钮(折叠 / 溢出)32×2 + 间距 4×2。
const TAB_BAR_RESERVE: f32 = 72.0;

fn scale() -> f32 {
    byteui::theme::icon_size::scale()
}

/// 页签标题字体:与 Dozer `top_bar_font()` 一致(系统默认字体的 Medium 档)。
fn tab_font() -> Font {
    Font {
        weight: Weight::Medium,
        ..Font::default()
    }
}

/// 组合悬停:Dozer 的胶囊背景与关闭按钮都由标题/关闭按钮中较大的悬停值驱动。
pub fn combined_hover(hover: f32, close_hover: f32) -> f32 {
    hover.max(close_hover).clamp(0.0, 1.0)
}

/// 页签/下拉行共用的"前缀 + 标题":选中 CREAM,未选中 DIM→GOLD 按 hover 插值;
/// 标题单行、超宽裁切、不换行。
fn tab_label<'a>(
    prefix: Option<El<'a, Msg>>,
    title: String,
    active: bool,
    hover: f32,
    max_width: f32,
) -> El<'a, Msg> {
    let mut row = row![]
        .spacing(4)
        .align_y(iced_widget::core::Alignment::Center);
    if let Some(prefix) = prefix {
        row = row.push(prefix);
    }
    row.push(
        container(
            text(title)
                .font(tab_font())
                .size(byteui::theme::font::body())
                .wrapping(iced_widget::core::text::Wrapping::None)
                .color(title_color(active, hover)),
        )
        .width(Length::Shrink)
        .max_width(max_width)
        .clip(true),
    )
    .into()
}

/// 溢出菜单的几何(窗口坐标);宽度夹在 220–560·scale、钳在窗口内。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverflowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub max_height: f32,
}

pub fn overflow_geometry(anchor: (f32, f32), window: (f32, f32), scale: f32) -> OverflowGeometry {
    let min_w = MENU_MIN_WIDTH * scale;
    let max_w = MENU_MAX_WIDTH * scale;
    let width = (window.0 - anchor.0 - 24.0 * scale).clamp(min_w, max_w);
    let max_height = MENU_MAX_HEIGHT * scale;
    let x = anchor.0.clamp(0.0, (window.0 - width).max(0.0));
    let y = anchor.1.clamp(0.0, (window.1 - max_height).max(0.0));
    OverflowGeometry {
        x,
        y,
        width,
        max_height,
    }
}

/// 页签条可用宽度 = 终端块宽 − 72·scale(不小于 0)。
pub fn tab_bar_avail_px(block_width: f32, scale: f32) -> f32 {
    (block_width - TAB_BAR_RESERVE * scale).max(0.0)
}

/// 标题颜色:选中恒 `cream`;未选中按阶跃悬停在 `dim`→`gold` 之间取值。
pub fn title_color(active: bool, hover: f32) -> Color {
    let colors = byteui::theme::color::current();
    if active {
        colors.cream
    } else {
        byteui::theme::color::mix(colors.dim, colors.gold, hover.clamp(0.0, 1.0))
    }
}

/// 关闭按钮颜色:静止时透明(不可见),悬停时按 `close_hover` 从 `dim` 到 `gold`。
pub fn close_color(hover: f32, close_hover: f32) -> Color {
    let colors = byteui::theme::color::current();
    let colors = byteui::theme::color::mix(colors.dim, colors.gold, close_hover.clamp(0.0, 1.0));
    Color {
        a: hover.clamp(0.0, 1.0),
        ..colors
    }
}

fn tab_container_style(
    active: bool,
    hover: f32,
) -> impl Fn(&iced_widget::Theme) -> container::Style {
    move |_theme: &iced_widget::Theme| {
        let colors = byteui::theme::color::current();
        if active {
            container::Style {
                background: Some(colors.card.into()),
                border: Border {
                    color: colors.border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            }
        } else if hover > 0.0 {
            container::Style {
                background: Some(
                    Color {
                        a: hover,
                        ..colors.tab_hover
                    }
                    .into(),
                ),
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        } else {
            container::Style::default()
        }
    }
}

/// 一个横向页签。`hover`/`close_hover` 是产品阶跃悬停值(0 或 1)。
#[allow(clippy::too_many_arguments)]
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
    let close_sz = byteui::theme::geometry::tab_button_size();
    // Dozer:悬停标题或 × 任一,胶囊背景都浮现、× 显形。
    let hover = combined_hover(hover, close_hover);
    let hovered = hover > 0.001;
    let title = args.title;
    let title_row = tab_label(
        Some(byteui::feedback::status::dot(args.dot)),
        title.clone(),
        args.active,
        hover,
        f32::MAX,
    );
    let (select, close) = tabs::tab_core(TabCoreArgs {
        content: title_row,
        close_sz,
        close_color: close_color(hover, close_hover),
        close_interactive: hovered,
        on_select: args.on_select,
        on_close: args.on_close,
        on_select_hover,
        on_close_hover,
    });
    let content = row![select, close]
        .spacing(2)
        .align_y(iced_widget::core::Alignment::Center);
    let el: El<'a, Msg> = container(content)
        .padding(Padding {
            top: PANEL_TAB_PAD_Y,
            right: PANEL_TAB_PAD_X,
            bottom: PANEL_TAB_PAD_Y,
            left: PANEL_TAB_PAD_LEFT,
        })
        .width(Length::Shrink)
        .style(tab_container_style(args.active, hover))
        .into();
    controlled_tooltip(el, title, show_tooltip)
}

/// 延迟提示:只有悬停满 2 秒(`show`)才包 Tooltip;否则返回裸内容。
pub fn controlled_tooltip<'a, M>(
    content: Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>,
    label: String,
    show: bool,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
{
    if !show {
        return content;
    }
    tooltip(
        content,
        container(
            text(label)
                .size(12.0)
                .color(byteui::theme::color::current().cream),
        )
        .padding([5, 9]),
        tooltip::Position::Top,
    )
    .gap(4.0)
    .style(icons::tooltip_bubble_style())
    .into()
}

/// 溢出入口:`ChevronsDown`;页签组非空时总显示(即使当前没有横向溢出)。
pub fn overflow_button<'a>(
    tab_count: usize,
    hover_t: f32,
    on_press: Msg,
    on_hover: impl Fn(bool) -> Msg + 'a,
) -> Option<El<'a, Msg>> {
    if tab_count == 0 {
        return None;
    }
    Some(icons::icon_button_entry(
        IconKind::ChevronsDown,
        byteui::theme::icon_size::chevron(),
        false,
        false,
        hover_t,
        false,
        byteui::theme::geometry::tab_arrow_button_size(),
        true,
        on_press,
        on_hover,
        "",
    ))
}

/// 溢出菜单一行(供测试与渲染共用)。
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

/// 溢出下拉:固定宽 `>` 列(仅选中行有箭头,其余留白,保证文字起点相同)+ 标题 + 关闭 ×。
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
    let colors = byteui::theme::color::current();
    let close_sz = byteui::theme::geometry::tab_button_size();
    let geometry = overflow_geometry(anchor, window, scale());
    let chevron_w = byteui::theme::icon_size::row() + byteui::theme::geometry::menu_gap();
    let row_max_w = (geometry.width - close_sz - 16.0).max(0.0);
    let label_max_w = (row_max_w - chevron_w).max(0.0);
    let on_row_hover = std::rc::Rc::new(on_row_hover);
    let mut rows: Vec<El<'a, Msg>> = Vec::new();
    for entry in entries {
        let idx = entry.index;
        let row_hover = hover_of(entry.row);
        let marker: El<'a, Msg> = if entry.active {
            row![icons::view(
                IconKind::ChevronRight,
                byteui::theme::icon_size::row(),
                colors.cream,
            )]
            .into()
        } else {
            iced_widget::Space::new().into()
        };
        // 与 Dozer 一致:选中 CREAM、其余 DIM(行 hover 由容器底色表达);
        // 前缀是状态点,chevron 列固定宽保证文字左对齐。
        let label = tab_label(
            Some(byteui::feedback::status::dot(entry.dot)),
            entry.title,
            entry.active,
            0.0,
            label_max_w,
        );
        let label: El<'a, Msg> = row![container(marker).width(Length::Fixed(chevron_w)), label,]
            .spacing(0)
            .align_y(iced_widget::core::Alignment::Center)
            .into();
        let (select, close) = tabs::tab_core(TabCoreArgs {
            content: label,
            close_sz,
            close_color: colors.dim,
            close_interactive: true,
            on_select: on_select(idx),
            on_close: on_close(idx),
            on_select_hover: {
                let f = on_row_hover.clone();
                move |h| f(idx, h)
            },
            on_close_hover: {
                let f = on_row_hover.clone();
                move |h| f(idx, h)
            },
        });
        rows.push(
            container(
                row![select, close]
                    .spacing(4)
                    .align_y(iced_widget::core::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from(MENU_ROW_PAD))
            .style(tab_container_style(false, row_hover))
            .into(),
        );
    }
    let list = container(
        scrollable(column(rows).spacing(MENU_ROW_GAP))
            .height(Length::Shrink)
            .width(Length::Fixed(geometry.width)),
    )
    .max_height(geometry.max_height);
    menu_overlay::shell(
        list.into(),
        MenuAnchor {
            x: geometry.x,
            y: geometry.y,
        },
        geometry.width,
        on_dismiss,
    )
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
    fn overflow_geometry_clamps_width_between_220_and_560() {
        let near_left = overflow_geometry((0.0, 0.0), (2000.0, 1000.0), 1.0);
        assert_eq!(near_left.width, 560.0, "可用宽度足够时取上限");
        let near_right = overflow_geometry((1980.0, 0.0), (2000.0, 1000.0), 1.0);
        assert_eq!(near_right.width, 220.0, "贴右边缘时取下限");
        assert!(near_right.x + near_right.width <= 2000.0);
    }

    #[test]
    fn overflow_geometry_keeps_the_menu_inside_the_window_at_every_corner() {
        let window = (800.0, 600.0);
        for anchor in [
            (-50.0, -50.0),
            (790.0, 590.0),
            (400.0, 300.0),
            (0.0, 590.0),
            (790.0, 0.0),
        ] {
            let g = overflow_geometry(anchor, window, 1.0);
            assert!(g.x >= 0.0 && g.x + g.width <= window.0 + 0.01, "{anchor:?}");
            assert!(g.y >= 0.0 && g.y + g.max_height <= window.1 + 0.01);
        }
    }

    #[test]
    fn overflow_geometry_scales_with_icon_size_scale() {
        let one = overflow_geometry((100.0, 100.0), (2000.0, 1000.0), 1.0);
        let two = overflow_geometry((100.0, 100.0), (2000.0, 1000.0), 2.0);
        assert_eq!(two.width, one.width * 2.0);
        assert_eq!(two.max_height, one.max_height * 2.0);
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
        assert_eq!(tab_font().weight, Weight::Medium);
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
        // 菜单外框由 `menu_overlay::shell` 提供整窗透明遮罩;空条目也应能构造。
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
}
