//! Product rendering seams; shared Agent icon and color mappings remain authoritative.
use crate::{product::Msg, shell};
use bytehost_chrome::{BodyArea, ShellMsg};
use byteui::interaction::icons::{self, IconKind};
use dozer_model::AgentKind;
use dozer_panel_conversations::{
    Message,
    adapter::{ConvHover, ConversationsPane, PaneStyle, PanelDisplay},
};
use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Element, Length};
use std::cell::Cell;

pub struct ConversationsDisplay {
    pid: i64,
    collapsed: Cell<bool>,
    list_on_left: Cell<bool>,
}
impl ConversationsDisplay {
    pub fn new(pid: i64) -> Self {
        Self {
            pid,
            collapsed: Cell::new(false),
            list_on_left: Cell::new(false),
        }
    }
    pub fn configure(&self, area: &BodyArea) {
        self.collapsed.set(area.collapsed);
        self.list_on_left.set(
            area.pair
                .map(|p| p.first.x <= p.second.x)
                .unwrap_or(!area.mirrored),
        );
    }
}
pub fn toggle_pane_message() -> Msg {
    Msg::Shell(ShellMsg::TogglePane(shell::conversations_id()))
}
impl PanelDisplay<Msg> for ConversationsDisplay {
    fn wrap(&self, message: Message) -> Msg {
        Msg::Conversations(self.pid, message)
    }
    fn locale(&self) -> &str {
        "zh-CN"
    }
    fn cursor_position(&self) -> (f32, f32) {
        let at = crate::CURRENT_CURSOR.get();
        (at.x, at.y)
    }
    fn hover_progress(&self, _: ConvHover) -> f32 {
        0.
    }
    fn panel_head<'a>(
        &self,
        icon: IconKind,
        title: String,
        actions: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        let tokens = byteui::theme::color::current();
        let mut head = row![
            icons::view(
                icon,
                byteui::theme::icon_size::row(),
                tokens.tab_active_border
            ),
            text(title)
                .size(byteui::theme::font::title())
                .color(tokens.tab_active_border)
        ]
        .spacing(8)
        .height(Length::Fixed(byteui::theme::geometry::tab_button_size()))
        .align_y(iced::Alignment::Center);
        if let Some(actions) = actions {
            head = head.push(iced::widget::space::horizontal()).push(actions);
        }
        column![head, self.tab_divider()].spacing(4).into()
    }
    fn list_collapse_button<'a>(&self, collapse: String, expand: String) -> Element<'a, Msg> {
        let (icon, tip) = match (self.collapsed.get(), self.list_on_left.get()) {
            (false, true) => (IconKind::PanelLeftClose, collapse),
            (true, true) => (IconKind::PanelLeftOpen, expand),
            (false, false) => (IconKind::PanelRightClose, collapse),
            (true, false) => (IconKind::PanelRightOpen, expand),
        };
        // Always retain Tooltip in the tree; hover transitions never replace wrappers.
        tooltip(
            button(icons::view(
                icon,
                byteui::theme::icon_size::row(),
                byteui::theme::color::current().dim,
            ))
            .on_press(toggle_pane_message()),
            container(text(tip).size(byteui::theme::font::body()))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .delay(super::hover::TOOLTIP_DELAY)
        .into()
    }
    fn tab_divider<'a>(&self) -> Element<'a, Msg> {
        byteui::layout::divider::horizontal()
    }
    fn pane_style(&self, pane: ConversationsPane) -> PaneStyle {
        let tokens = byteui::theme::color::current();
        match pane {
            ConversationsPane::List => PaneStyle {
                padding: iced_core::Padding {
                    top: 6.,
                    right: 12.,
                    bottom: 12.,
                    left: 12.,
                },
                gap: 8.,
                background: Some(tokens.bg),
                header_padding: iced_core::Padding::from(8.),
                line_height: 1.2,
            },
            ConversationsPane::Detail => review_pane_style(),
        }
    }
    fn agent_icon(&self, agent: AgentKind, color: iced::Color) -> Element<'static, Msg> {
        icons::view(
            dozer_panel_agent::style::agent_icon(agent),
            byteui::theme::icon_size::row(),
            color,
        )
    }
    fn agent_icon_color(&self, agent: AgentKind) -> iced::Color {
        dozer_panel_agent::style::agent_dot_color(agent)
    }
    fn relative_time(&self, last: u64, now: u64) -> String {
        relative_time(last, now, self.locale())
    }
    fn menu_item<'a>(
        &self,
        leading: Option<Element<'a, Msg>>,
        label: String,
        color: iced::Color,
        message: Option<Msg>,
    ) -> Element<'a, Msg> {
        let mut content = row![].spacing(8).align_y(iced::Alignment::Center);
        if let Some(icon) = leading {
            content = content.push(icon);
        }
        button(content.push(text(label).color(color)))
            .on_press_maybe(message)
            .width(Length::Fill)
            .into()
    }
    fn menu_shell<'a>(&self, items: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
        container(column(items).spacing(4))
            .padding(6)
            .style(container::rounded_box)
            .into()
    }
}
/// S7a's catalog has no relative-time keys; the library explicitly accepts a
/// product callback, used by both row labels and SessionOpen's summary time.
pub fn relative_time(last: u64, now: u64, locale: &str) -> String {
    let seconds = now.saturating_sub(last) / 1000;
    let zh = locale.starts_with("zh");
    match seconds {
        0..=59 => {
            if zh {
                "刚刚".into()
            } else {
                "just now".into()
            }
        }
        60..=3599 => {
            if zh {
                format!("{} 分钟前", seconds / 60)
            } else {
                format!("{}m ago", seconds / 60)
            }
        }
        3600..=86399 => {
            if zh {
                format!("{} 小时前", seconds / 3600)
            } else {
                format!("{}h ago", seconds / 3600)
            }
        }
        _ => {
            if zh {
                format!("{} 天前", seconds / 86400)
            } else {
                format!("{}d ago", seconds / 86400)
            }
        }
    }
}

/// The library already pads the entire pane. Padding its header a second time
/// would put the collapse button beneath the native child. Keep the header in
/// the exported preview chrome, with the same horizontal inset as other panes.
pub fn review_pane_style() -> PaneStyle {
    let inset = super::preview_display::PreviewDisplay::content_padding()
        * byteui::theme::icon_size::scale();
    PaneStyle {
        padding: iced_core::Padding {
            top: inset,
            right: inset,
            bottom: review_bottom_inset(byteui::theme::icon_size::scale()),
            left: inset,
        },
        gap: inset / 2.,
        background: Some(byteui::theme::color::current().panel),
        header_padding: iced_core::Padding::ZERO,
        line_height: 1.2,
    }
}

/// Shared chrome excludes the footbar and its zone padding. Dozer review uses
/// the status-bar edge instead. Reserve the remaining exported difference
/// inside this product pane, without moving the shared shell or its footer.
pub fn review_bottom_inset(scale: f32) -> f32 {
    let geometry = byteui::theme::geometry::current();
    let zone = bytehost_chrome::metrics_from_theme().right_zone;
    ((geometry.status_bar_height - geometry.footbar_height) * scale.max(0.) - zone.padding.bottom)
        .max(0.)
}
