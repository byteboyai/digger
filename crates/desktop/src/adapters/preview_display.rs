//! 预览面板的 **Digger 展示适配**(S5b-2 P4 Task 6)。
//!
//! 库 `dozer-panel-preview` 的视图只认 [`PanelDisplay`](库 trait):悬停、页签
//! 部件、折叠按钮、配色区域、顶层消息类型都由产品提供。这里把 Files 与
//! Project 两个预览面板的展示语义一次性实现,靠 `panel: ViewerPanel` 参数
//! 区分,与 Dozer 的 `host_adapters/preview_panel.rs` 同型。
//!
//! Digger 的取舍:
//! - **无悬停动画(D2):** `hover_progress` 恒 0、`hover_tooltip_ready` 恒假,
//!   `hover_message` 返回 `ShellMsg::Noop`——库照常发悬停事件,Digger 不记账。
//! - **列表列恒在左侧(D3):** 收起按钮一律画成"收起左列表"(`PanelLeftClose`
//!   / `PanelLeftOpen`),与骨架 `PairRules.default_side = Left` 一致。
//! - **无 i18n 解析器:** 库的 `LocalizedText` 取官方英文兜底。
//! - 页签宽度 / 页签条可用宽度 / 行高都不乘 `scale`:byteui 的
//!   `font::body()` 与 `geometry::*` 取出的已经是含全局缩放的逻辑像素,
//!   再乘一次会与渲染侧的真实宽度错位。

#![allow(dead_code)] // Task 8 才会把 `PreviewDisplay` 交给库的 `view::pane`。

use std::path::PathBuf;

use bytehost_chrome::ShellMsg;
use dozer_panel_preview::display::{
    ActionButtonStyle, Element, OverflowMenuArgs, PaneStyle, PanelDisplay, PreviewHover, TabArgs,
};
use dozer_panel_preview::{Message as LibMessage, ViewerPanel};
use iced_widget::core::{Border, Color, Padding};
use iced_widget::{button, container, text, tooltip};

use crate::CURRENT_WINDOW;
use crate::product::Msg;
use crate::shell;

use super::tab_chrome;

/// 预览内容区的内边距(库的 `pane_style().padding` 与本文件的 `content_padding` 共用)。
const PREVIEW_PADDING: f32 = 8.0;

/// 预览面板的展示上下文:外观只跟 locale / 页签条可用宽度走。
#[derive(Clone, Copy)]
pub struct PreviewDisplay<'a> {
    pub locale: &'a str,
    /// 页签条此刻的可用宽度(逻辑像素):翻页窗口数学用,由骨架的配对几何给出。
    pub tab_bar_px: f32,
    /// Digger 一期的配对布局把列表放在左边(D3),收起按钮据此画方向。
    pub list_on_left: bool,
}

impl PreviewDisplay<'_> {
    /// 溢出下拉的定位需要整个窗口的尺寸(库给的锚点是窗口坐标)。
    fn menu_window(&self) -> (f32, f32) {
        CURRENT_WINDOW.with(|w| w.get()).unwrap_or((1280.0, 800.0))
    }

    /// 配对里预览那一块的底色(分隔线两侧由产品给,必须与 `pane_style` 同值,
    /// 否则竖线两边颜色会差一层)。
    pub fn background() -> Color {
        byteui::theme::color::current().panel
    }

    /// 预览内容区四周的内边距(库的 `view::pane` 用 `pane_style().padding`)。
    pub fn content_padding() -> f32 {
        PREVIEW_PADDING
    }

    fn collapsed(panel: ViewerPanel) -> bool {
        let id = if panel == ViewerPanel::Project {
            shell::project_id()
        } else {
            shell::files_id()
        };
        crate::product::pane_collapsed_of(&id)
    }
}

impl PanelDisplay<Msg> for PreviewDisplay<'_> {
    fn wrap(&self, panel: ViewerPanel, message: LibMessage) -> Msg {
        Msg::Preview(panel, message)
    }

    fn locale(&self) -> &str {
        self.locale
    }

    fn product_name(&self) -> &str {
        "Digger"
    }

    fn render_text(&self, text: &dozer_model::LocalizedText) -> String {
        text.fallback.clone()
    }

    fn pane_style(&self) -> PaneStyle {
        // 与 Dozer 的 `theme::region::preview_pane()` 同值:8px 内边距、4px 间距、
        // 面板底色(区域外框由骨架画,Dozer 这一层也不再画边框)。
        PaneStyle {
            padding: Padding::from(PREVIEW_PADDING),
            gap: 4.0,
            background: Some(Self::background()),
        }
    }

    fn hover_progress(&self, _panel: ViewerPanel, _hover: PreviewHover) -> f32 {
        0.0
    }

    fn hover_tooltip_ready(&self, _panel: ViewerPanel, _hover: PreviewHover) -> bool {
        false
    }

    fn hover_message(&self, _panel: ViewerPanel, _hover: PreviewHover, _hovered: bool) -> Msg {
        Msg::Shell(ShellMsg::Noop)
    }

    fn tab_bar_avail_px(&self, _panel: ViewerPanel) -> f32 {
        self.tab_bar_px
    }

    fn tab_display_width(&self, title: &str) -> f32 {
        tab_chrome::tab_display_width(title)
    }

    fn tab_divider<'a>(&self) -> Element<'a, Msg> {
        byteui::layout::divider::horizontal()
    }

    fn panel_tab<'a>(&self, args: TabArgs<'a, Msg>) -> Element<'a, Msg> {
        let TabArgs {
            title,
            active,
            hover_t,
            close_hover_t,
            on_select,
            on_close,
            show_tooltip,
            title_hover,
            close_hover,
        } = args;
        tab_chrome::panel_tab(
            title,
            active,
            hover_t,
            close_hover_t,
            on_select,
            on_close,
            show_tooltip,
            title_hover,
            close_hover,
        )
    }

    fn tab_gestures<'a>(
        &self,
        _panel: ViewerPanel,
        _index: usize,
        tab: Element<'a, Msg>,
        _path: Option<PathBuf>,
    ) -> Element<'a, Msg> {
        // Digger 一期不做页签拖拽换位,也不挂页签右键菜单(D3 的右键菜单只留
        // 在文件树 / Project 链接上)。
        tab
    }

    fn overflow_button<'a>(
        &self,
        tab_count: usize,
        hover_t: f32,
        on_press: Msg,
        on_hover: Box<dyn Fn(bool) -> Msg + 'a>,
    ) -> Option<Element<'a, Msg>> {
        tab_chrome::overflow_button(tab_count, hover_t, on_press, on_hover)
    }

    fn render_mode_button<'a>(
        &self,
        in_code_mode: bool,
        hover_t: f32,
        on_press: Msg,
        on_hover: Box<dyn Fn(bool) -> Msg + 'a>,
    ) -> Element<'a, Msg> {
        let icon = if in_code_mode {
            byteui::interaction::icons::IconKind::FilePlay
        } else {
            byteui::interaction::icons::IconKind::FileCode
        };
        tab_chrome::mode_button(icon, "", hover_t, on_press, on_hover)
    }

    fn tabular_mode_button<'a>(
        &self,
        in_text: bool,
        hover_t: f32,
        on_press: Msg,
        on_hover: Box<dyn Fn(bool) -> Msg + 'a>,
    ) -> Element<'a, Msg> {
        let (icon, tip) = if in_text {
            (byteui::interaction::icons::IconKind::Table, "View table")
        } else {
            (
                byteui::interaction::icons::IconKind::FileCode,
                "View source",
            )
        };
        tab_chrome::mode_button(icon, tip, hover_t, on_press, on_hover)
    }

    fn json_mode_button<'a>(
        &self,
        in_text: bool,
        hover_t: f32,
        on_press: Msg,
        on_hover: Box<dyn Fn(bool) -> Msg + 'a>,
    ) -> Element<'a, Msg> {
        let (icon, tip) = if in_text {
            (byteui::interaction::icons::IconKind::Table, "View tree")
        } else {
            (byteui::interaction::icons::IconKind::FileCode, "View text")
        };
        tab_chrome::mode_button(icon, tip, hover_t, on_press, on_hover)
    }

    fn collapse_button<'a>(&self, panel: ViewerPanel) -> Element<'a, Msg> {
        use byteui::interaction::icons::{self, IconKind};
        let collapsed = Self::collapsed(panel);
        let id = if panel == ViewerPanel::Project {
            shell::project_id()
        } else {
            shell::files_id()
        };
        let left = self.list_on_left;
        let (icon, tip) = match (collapsed, left) {
            (false, true) => (IconKind::PanelLeftClose, "Collapse list"),
            (true, true) => (IconKind::PanelLeftOpen, "Expand list"),
            (false, false) => (IconKind::PanelRightClose, "Collapse list"),
            (true, false) => (IconKind::PanelRightOpen, "Expand list"),
        };
        tooltip(
            button(icons::view(
                icon,
                byteui::theme::icon_size::row(),
                byteui::theme::color::current().dim,
            ))
            .on_press(Msg::Shell(ShellMsg::TogglePane(id))),
            container(text(tip).size(byteui::theme::font::body()))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn overflow_menu<'a>(&self, args: OverflowMenuArgs<'a, Msg>) -> Element<'a, Msg> {
        let OverflowMenuArgs {
            entries,
            anchor,
            on_select,
            on_close,
            on_dismiss,
            on_row_hover,
        } = args;
        let entries = entries
            .into_iter()
            .map(|e| tab_chrome::OverflowEntry {
                index: e.index,
                title: e.title,
                active: e.active,
                closable: e.closable,
                hover_t: e.hover_t,
            })
            .collect();
        tab_chrome::overflow_menu(
            entries,
            anchor,
            self.menu_window(),
            on_select,
            on_close,
            on_dismiss,
            on_row_hover,
        )
    }

    fn action_button_style(&self, text_color: Color) -> ActionButtonStyle {
        Box::new(move |_t: &iced_widget::Theme, status: button::Status| {
            let colors = byteui::theme::color::current();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(if hovered {
                    Color {
                        a: 0.15,
                        ..colors.tab_hover
                    }
                    .into()
                } else {
                    colors.panel.into()
                }),
                text_color: if hovered { colors.cream } else { text_color },
                border: Border {
                    color: if hovered { colors.gold } else { colors.border },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..button::Style::default()
            }
        })
    }

    fn find_font_size(&self) -> f32 {
        byteui::theme::font::body() as f32
    }

    fn line_height_factor(&self) -> f32 {
        // 与 Dozer 的文件树行距一致(终端行高倍数)。
        1.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> PreviewDisplay<'static> {
        PreviewDisplay {
            locale: "en",
            tab_bar_px: 400.0,
            list_on_left: true,
        }
    }

    #[test]
    fn hover_progress_is_always_zero_and_tooltip_never_ready() {
        let d = display();
        for panel in [ViewerPanel::Files, ViewerPanel::Project] {
            for hover in [
                PreviewHover::TabItem(1),
                PreviewHover::TabClose(1),
                PreviewHover::TabOverflow,
                PreviewHover::TabOverflowRow(1),
                PreviewHover::RenderMode,
                PreviewHover::TabularMode,
                PreviewHover::JsonMode,
                PreviewHover::FindNext,
                PreviewHover::FindReplaceAll,
            ] {
                assert_eq!(d.hover_progress(panel, hover), 0.0);
                assert!(!d.hover_tooltip_ready(panel, hover));
                assert!(matches!(
                    d.hover_message(panel, hover, true),
                    Msg::Shell(ShellMsg::Noop)
                ));
            }
        }
    }

    #[test]
    fn wrap_routes_each_panel_to_its_own_message() {
        let d = display();
        let files = d.wrap(ViewerPanel::Files, LibMessage::Retry(0));
        let project = d.wrap(ViewerPanel::Project, LibMessage::Retry(0));
        assert!(matches!(
            files,
            Msg::Preview(ViewerPanel::Files, LibMessage::Retry(0))
        ));
        assert!(matches!(
            project,
            Msg::Preview(ViewerPanel::Project, LibMessage::Retry(0))
        ));
    }

    #[test]
    fn the_product_is_named_digger_and_the_error_text_falls_back_to_english() {
        let d = display();
        assert_eq!(d.product_name(), "Digger");
        let text = dozer_model::LocalizedText::literal("dozer.preview_panel", "Nope");
        assert_eq!(d.render_text(&text), "Nope");
    }

    #[test]
    fn the_tab_bar_width_is_the_one_the_shell_gave() {
        let d = display();
        assert_eq!(d.tab_bar_avail_px(ViewerPanel::Files), 400.0);
        assert_eq!(
            d.tab_display_width("a.rs"),
            tab_chrome::tab_display_width("a.rs")
        );
    }
}
