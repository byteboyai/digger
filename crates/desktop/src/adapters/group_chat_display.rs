//! Group-chat PanelDisplay implementation for Digger.
use bytehost_chrome::BodyArea;
use byteui::interaction::icons::{self, IconKind};
use dozer_model::{AgentKind, LocalizedText};
use dozer_panel_group_chat::Message;
use dozer_panel_group_chat::adapter::{GroupChatPane, PaneStyle, PanelDisplay};
use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Element, Length};
use std::cell::Cell;

pub struct GroupChatDisplay {
    pid: i64,
    collapsed: Cell<bool>,
    list_on_left: Cell<bool>,
}

impl GroupChatDisplay {
    pub fn new(pid: i64) -> Self {
        Self {
            pid,
            collapsed: Cell::new(false),
            list_on_left: Cell::new(false),
        }
    }
    pub fn collapse_icon(&self) -> IconKind {
        match (self.collapsed.get(), self.list_on_left.get()) {
            (false, true) => IconKind::PanelLeftClose,
            (true, true) => IconKind::PanelLeftOpen,
            (false, false) => IconKind::PanelRightClose,
            (true, false) => IconKind::PanelRightOpen,
        }
    }

    pub fn collapse_label(&self) -> &'static str {
        if self.collapsed.get() {
            "展开群聊列表"
        } else {
            "收起群聊列表"
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

impl PanelDisplay<Message> for GroupChatDisplay {
    fn wrap(&self, message: Message) -> Message {
        message
    }
    fn locale(&self) -> &str {
        "zh-CN"
    }
    fn panel_head<'a>(
        &self,
        icon: IconKind,
        title: String,
        actions: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let tokens = byteui::theme::color::current();
        let mut head = row![
            icons::view(
                icon,
                byteui::theme::icon_size::row(),
                tokens.tab_active_border
            ),
            text(title)
                .size(byteui::theme::font::title())
                .color(tokens.tab_active_border),
        ]
        .spacing(8)
        .height(Length::Fixed(byteui::theme::geometry::tab_button_size()))
        .align_y(iced::Alignment::Center);
        if let Some(actions) = actions {
            head = head.push(iced::widget::space::horizontal()).push(actions);
        }
        column![head, byteui::layout::divider::horizontal()]
            .spacing(4)
            .into()
    }
    fn list_collapse_button<'a>(&self, collapse: String, expand: String) -> Element<'a, Message> {
        let (icon, tip) = match (self.collapsed.get(), self.list_on_left.get()) {
            (false, true) => (IconKind::PanelLeftClose, collapse),
            (true, true) => (IconKind::PanelLeftOpen, expand),
            (false, false) => (IconKind::PanelRightClose, collapse),
            (true, false) => (IconKind::PanelRightOpen, expand),
        };
        tooltip(
            button(icons::view(
                icon,
                byteui::theme::icon_size::row(),
                byteui::theme::color::current().dim,
            ))
            .padding(6)
            .on_press(Message::Noop(self.pid)),
            container(text(tip).size(byteui::theme::font::body()))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .delay(super::hover::TOOLTIP_DELAY)
        .into()
    }
    fn pane_style(&self, pane: GroupChatPane) -> PaneStyle {
        let tokens = byteui::theme::color::current();
        match pane {
            GroupChatPane::List => PaneStyle {
                padding: iced_core::Padding {
                    top: 6.,
                    right: 12.,
                    bottom: 12.,
                    left: 12.,
                },
                gap: 8.,
                background: Some(tokens.bg),
                header_padding: iced_core::Padding::from(8.),
            },
            GroupChatPane::Content => PaneStyle {
                padding: iced_core::Padding::from(16),
                gap: 8.,
                background: Some(tokens.panel),
                header_padding: iced_core::Padding::ZERO,
            },
        }
    }
    fn agent_icon(&self, agent: AgentKind, color: iced::Color) -> Element<'static, Message> {
        icons::view(
            dozer_panel_agent::style::agent_icon(agent),
            byteui::theme::icon_size::row(),
            color,
        )
    }
    fn agent_icon_color(&self, agent: AgentKind) -> iced::Color {
        dozer_panel_agent::style::agent_dot_color(agent)
    }
    fn render_localized(&self, text: &LocalizedText) -> String {
        text.fallback.clone()
    }
}
