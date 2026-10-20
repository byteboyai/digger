//! 预览页签栏的 **Digger 部件**(S5b-2 P4 Task 6)。
//!
//! 外观照 Dozer `crates/dozer-app/src/chrome/tab_widget.rs`(静止 DIM、悬停
//! DIM→GOLD、选中 CREAM + CARD 实底 + 1px 边框),但**不**带 Dozer 的悬停
//! 动画表:Digger 的 D2 约定是「无 hover 动画」——`hover_t` 仍按接口透传,
//! 调用方恒传 0(见 [`crate::adapters::preview_display`]),于是页签的底色
//! 与文字色都取静止态;交互反馈只剩 `tabs::tab_core` 自带的指针光标与
//! `button::Status` 驱动的关闭按钮变色。
//!
//! 页签的布局数学(窗口翻页、溢出判定)在库里,这里只画单个页签 / 溢出
//! 入口 / 下拉,不参与任何宽度决策;宽度估算 [`tab_display_width`] 是库反
//! 过来要的那一份(估算而非测量,CJK 按全宽计)。

use byteui::interaction::{icons, tabs};
use iced_widget::core::{Border, Color, Element, Length, Padding};
use iced_widget::{MouseArea, column, container, row, scrollable, stack, text, tooltip};

/// 页签内边距,与 Dozer 的面板页签逐项相同(左侧单独放大给 hover 胶囊留白)。
const PANEL_TAB_PAD_LEFT: f32 = 10.0;
const PANEL_TAB_PAD_X: f32 = 4.0;
const PANEL_TAB_PAD_Y: f32 = 0.0;

/// 传给 [`tab_label`] 表示"不设上限"的名目值:横向页签随标题实际内容伸缩,
/// 标题超宽时由外层页签行自己裁切,不在这里补省略号。
const NO_TAB_W_LIMIT: f32 = f32::MAX;

/// 溢出下拉的最小/最大宽度,以及高度上限(定位钳制用)。同 Dozer。
const TAB_OVERFLOW_MENU_MIN_WIDTH: f32 = 220.0;
const TAB_OVERFLOW_MENU_MAX_WIDTH: f32 = 560.0;
const TAB_OVERFLOW_MENU_MAX_HEIGHT: f32 = 320.0;

/// 文本显示宽度的基础单元和:CJK 字符按全宽计 2,其余按半宽计 1。
pub fn text_width_units(s: &str) -> f32 {
    s.chars()
        .map(|c| if (c as u32) > 0x2E80 { 2.0 } else { 1.0 })
        .sum()
}

/// 预览页签的估算显示宽(逻辑像素):名称 + 关闭 × + 胶囊内边距的粗估。
/// 不追求精确——估偏几像素只会让翻页边界差一个页签。宽度随标题实际长度
/// 增长(无上限),与渲染侧按内容伸缩对齐。
pub fn tab_display_width(title: &str) -> f32 {
    text_width_units(title) * 8.0 + 18.0 + 12.0
}

/// "图标 + 标题"行,横向页签与下拉行共用。`active` 恒 CREAM,未选中按
/// `hover_t` 从 DIM 插值到 GOLD;`max_width` 只对下拉行起作用(横向页签传
/// [`NO_TAB_W_LIMIT`])。
fn tab_label<'a, M: 'a>(
    title: String,
    active: bool,
    hover_t: f32,
    max_width: f32,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
    let colors = byteui::theme::color::current();
    let title_color = if active {
        colors.cream
    } else {
        byteui::theme::color::mix(colors.dim, colors.gold, hover_t)
    };
    container(
        text(title)
            .size(byteui::theme::font::body())
            // iced 的 `Text` 默认按词换行:不显式关掉,长标题会折成两行。
            .wrapping(iced_widget::core::text::Wrapping::None)
            .color(title_color),
    )
    .width(Length::Shrink)
    .max_width(max_width)
    .clip(true)
    .into()
}

/// 页签容器的底色:选中态 CARD 实底 + 1px 边框;未选中只在 `hover > 0` 时
/// 铺一层半透明胶囊。横向页签与下拉行共用同一份判断。
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

/// 一个横向页签。`on_select`/`on_close` 由库的 `TabArgs` 给出,已经是产品
/// 顶层消息;两个 `hover` 闭包在 Digger 恒产出同一个 `Noop`(见模块文档)。
#[allow(clippy::too_many_arguments)] // 参数逐项对应库的 `TabArgs`,不再另立结构体。
pub fn panel_tab<'a, M, F1, F2>(
    title: String,
    active: bool,
    hover_t: f32,
    close_hover_t: f32,
    on_select: M,
    on_close: M,
    show_tooltip: bool,
    title_hover: F1,
    close_hover: F2,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
    F1: Fn(bool) -> M + 'a,
    F2: Fn(bool) -> M + 'a,
{
    let colors = byteui::theme::color::current();
    let close_sz = byteui::theme::geometry::tab_button_size();
    let hover = hover_t.max(close_hover_t).clamp(0.0, 1.0);
    let hovered = hover > 0.001;
    let title_row = tab_label(title.clone(), active, hover_t, NO_TAB_W_LIMIT);
    let close_color = Color {
        a: hover,
        ..byteui::theme::color::mix(colors.dim, colors.gold, close_hover_t)
    };
    let (select, close) = tabs::tab_core(tabs::TabCoreArgs {
        content: title_row,
        close_sz,
        close_color,
        close_interactive: hovered,
        on_select,
        on_close,
        on_select_hover: title_hover,
        on_close_hover: close_hover,
    });
    let tab_row = row![select, close]
        .spacing(2)
        .align_y(iced_widget::core::Alignment::Center);
    let el: Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> = container(tab_row)
        .padding(Padding {
            top: PANEL_TAB_PAD_Y,
            right: PANEL_TAB_PAD_X,
            bottom: PANEL_TAB_PAD_Y,
            left: PANEL_TAB_PAD_LEFT,
        })
        .width(Length::Shrink)
        .style(tab_container_style(active, hover))
        .into();
    if show_tooltip {
        // 页签在屏幕底部,气泡向上弹。只在"悬停足够久"时挂载(iced 0.14 的
        // `Tooltip` 没有延迟开关,离开即卸载)。
        tooltip(
            el,
            container(text(title).size(byteui::theme::font::body()))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Top,
        )
        .into()
    } else {
        el
    }
}

/// 溢出下拉入口:`ChevronsDown` 标准图标按钮。页签组非空时总显示(即使当前
/// 没有横向溢出,下拉也列出整组页签);空组返回 `None`。
pub fn overflow_button<'a, M: Clone + 'a>(
    tab_count: usize,
    hover_t: f32,
    on_press: M,
    on_hover: impl Fn(bool) -> M + 'a,
) -> Option<Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>> {
    if tab_count == 0 {
        return None;
    }
    Some(icons::icon_button_entry(
        icons::IconKind::ChevronsDown,
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

/// 页签栏右侧的模式切换按钮(预览/代码、网格/原文、树/文本三种)。
pub fn mode_button<'a, M: Clone + 'a>(
    kind: icons::IconKind,
    tooltip: impl Into<std::borrow::Cow<'a, str>>,
    hover_t: f32,
    on_press: M,
    on_hover: impl Fn(bool) -> M + 'a,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
    icons::icon_button_entry(
        kind,
        byteui::theme::icon_size::row(),
        false,
        false,
        hover_t,
        false,
        byteui::theme::icon_size::row() + 6.0,
        true,
        on_press,
        on_hover,
        tooltip,
    )
}

/// 溢出下拉的一行(对应组里某个页签)。
pub struct OverflowEntry {
    pub index: usize,
    pub title: String,
    pub active: bool,
    /// `Blank` 占位(fallback 页)固定存在、不可关闭,这一行不画 ×。
    pub closable: bool,
    pub hover_t: f32,
}

/// 溢出下拉:每行一个页签,选中行标题前画 CREAM 的 `>`(未选中行留空,
/// 文字左对齐);关闭 × 恒可点(下拉本就是"主动点开来看"的场景)。返回的
/// 元素带一层全窗透明遮罩接「点外部关闭」,调用方必须把它提到视图顶层
/// `stack` 里(锚点与窗口尺寸都是全窗坐标)。
pub fn overflow_menu<'a, M, FSel, FClose, FRH>(
    entries: Vec<OverflowEntry>,
    anchor: (f32, f32),
    window_size: (f32, f32),
    on_select: FSel,
    on_close: FClose,
    on_dismiss: M,
    on_row_hover: FRH,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
    FSel: Fn(usize) -> M + 'a,
    FClose: Fn(usize) -> M + 'a,
    FRH: Fn(usize, bool) -> M + 'a,
{
    let colors = byteui::theme::color::current();
    let close_sz = byteui::theme::geometry::tab_button_size();
    let avail_w = (window_size.0 - anchor.0 - 24.0).max(TAB_OVERFLOW_MENU_MIN_WIDTH);
    let menu_width = avail_w.min(TAB_OVERFLOW_MENU_MAX_WIDTH);
    let row_max_w = menu_width - close_sz - 16.0;
    let chevron_w = byteui::theme::icon_size::row() + byteui::theme::geometry::menu_gap();
    let label_max_w = (row_max_w - chevron_w).max(0.0);
    let on_row_hover = std::rc::Rc::new(on_row_hover);

    let mut rows: Vec<Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>> = Vec::new();
    for entry in entries {
        let idx = entry.index;
        let label = tab_label(entry.title, entry.active, 0.0, label_max_w);
        let marker: Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> = if entry.active {
            row![icons::view(
                icons::IconKind::ChevronRight,
                byteui::theme::icon_size::row(),
                colors.cream,
            )]
            .into()
        } else {
            iced_widget::Space::new().into()
        };
        let label: Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> =
            row![container(marker).width(Length::Fixed(chevron_w)), label,]
                .spacing(0)
                .align_y(iced_widget::core::Alignment::Center)
                .into();
        let (select, close) = tabs::tab_core(tabs::TabCoreArgs {
            content: label,
            close_sz,
            close_color: colors.dim,
            close_interactive: entry.closable,
            on_select: (on_select)(idx),
            on_close: (on_close)(idx),
            on_select_hover: {
                let f = on_row_hover.clone();
                move |h| f(idx, h)
            },
            on_close_hover: {
                let f = on_row_hover.clone();
                move |h| f(idx, h)
            },
        });
        let row_content = row![select, close]
            .spacing(4)
            .align_y(iced_widget::core::Alignment::Center);
        rows.push(
            container(row_content)
                .width(Length::Fill)
                .padding(Padding::new(4.0))
                .style(tab_container_style(false, entry.hover_t))
                .into(),
        );
    }

    let list = container(
        scrollable(column(rows).spacing(2))
            .height(Length::Shrink)
            .width(Length::Fixed(menu_width)),
    )
    .max_height(TAB_OVERFLOW_MENU_MAX_HEIGHT);

    let dismiss = MouseArea::new(
        container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(on_dismiss.clone());

    let (ax, ay) = anchor;
    let (window_w, window_h) = window_size;
    let x = ax.min((window_w - menu_width).max(0.0)).max(0.0);
    let y = ay
        .min((window_h - TAB_OVERFLOW_MENU_MAX_HEIGHT).max(0.0))
        .max(0.0);
    let positioned = container(list)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: y,
            left: x,
            right: 0.0,
            bottom: 0.0,
        });

    stack![dismiss, positioned].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_display_width_counts_cjk_as_full_width() {
        // "ab" = 2 个半宽单元;"中文" = 2 个全宽单元 = 4。
        let ascii = tab_display_width("ab");
        let cjk = tab_display_width("中文");
        assert_eq!(text_width_units("ab"), 2.0);
        assert_eq!(text_width_units("中文"), 4.0);
        assert_eq!(cjk - ascii, 2.0 * 8.0);
        // 空标题只剩 × 与内边距。
        assert_eq!(tab_display_width(""), 30.0);
    }

    #[test]
    fn tab_display_width_grows_with_the_title() {
        assert!(tab_display_width("a_very_long_file_name.rs") > tab_display_width("a.rs"));
    }

    #[test]
    fn overflow_entries_mark_the_blank_tab_as_not_closable() {
        // `closable` 由库的 `OverflowEntry` 原样带过来;这里钉住 Digger 的
        // 下拉行数据形状:Blank 占位是第 0 项且不画 ×。
        let entries = [
            OverflowEntry {
                index: 0,
                title: String::new(),
                active: false,
                closable: false,
                hover_t: 0.0,
            },
            OverflowEntry {
                index: 1,
                title: "a.rs".to_string(),
                active: true,
                closable: true,
                hover_t: 0.0,
            },
        ];
        assert!(!entries[0].closable);
        assert!(entries[1].closable);
        // 空组不出溢出入口;非空组出。
        assert!(
            overflow_button::<bytehost_chrome::ShellMsg>(
                0,
                0.0,
                bytehost_chrome::ShellMsg::Noop,
                |_| bytehost_chrome::ShellMsg::Noop,
            )
            .is_none()
        );
        assert!(
            overflow_button::<bytehost_chrome::ShellMsg>(
                1,
                0.0,
                bytehost_chrome::ShellMsg::Noop,
                |_| bytehost_chrome::ShellMsg::Noop,
            )
            .is_some()
        );
    }
}
