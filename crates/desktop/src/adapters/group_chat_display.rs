//! Group-chat PanelDisplay implementation for Digger.
use bytehost_chrome::BodyArea;
use byteui::interaction::icons::{self, IconKind};
use dozer_model::{AgentKind, LocalizedText};
use dozer_panel_group_chat::Message;
use dozer_panel_group_chat::adapter::{GroupChatPane, PaneStyle, PanelDisplay};
use iced::Element;
use iced::widget::column;
use std::cell::Cell;

#[derive(Default)]
pub struct GroupChatDisplay {
    list_on_left: Cell<bool>,
}

impl GroupChatDisplay {
    pub fn new() -> Self {
        Self {
            list_on_left: Cell::new(false),
        }
    }
    pub fn configure(&self, area: &BodyArea) {
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
        crate::settings::language::code()
    }
    fn panel_head<'a>(
        &self,
        icon: IconKind,
        title: String,
        actions: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let head = byteui::layout::panel_heading::PanelHeading::new(icon, title)
            .actions(actions)
            .without_divider()
            .into_element();
        column![head, byteui::layout::divider::horizontal()]
            .spacing(4)
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
