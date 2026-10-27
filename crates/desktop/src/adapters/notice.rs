//! Shared dismissible notice bar used by Agent, Conversations and Group chat.
use iced::widget::{button, row, text};
use iced::{Element, Length};

#[derive(Debug, Default, Clone)]
pub struct NoticeBar {
    text: Option<String>,
}

impl std::ops::Deref for NoticeBar {
    type Target = Option<String>;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl NoticeBar {
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        eprintln!("digger: {text}");
        self.text = Some(text);
    }
    /// Set the line when the caller already emitted its own contextual log.
    pub fn push_quiet(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }
    pub fn dismiss(&mut self) {
        self.text = None;
    }
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
    pub fn is_some(&self) -> bool {
        self.text.is_some()
    }
    pub fn view<M: Clone + 'static>(
        &self,
        on_dismiss: M,
    ) -> Option<Element<'_, M, iced_widget::Theme, iced_renderer::Renderer>> {
        let tokens = byteui::theme::color::current();
        let notice = self.text.as_ref()?;
        Some(
            row![
                text(notice.clone())
                    .size(byteui::theme::font::caption())
                    .color(tokens.red)
                    .width(Length::Fill),
                button(
                    text("×")
                        .size(byteui::theme::font::caption())
                        .color(tokens.dim)
                )
                .padding(6)
                .on_press(on_dismiss),
            ]
            .spacing(8)
            .padding([6, 12])
            .align_y(iced::Alignment::Start)
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_bar_replaces_dismisses_and_keeps_one_line() {
        let mut bar = NoticeBar::default();
        assert!(bar.view::<()>(()).is_none());
        bar.push("first");
        assert_eq!(bar.text(), Some("first"));
        bar.push("second");
        assert_eq!(bar.text(), Some("second"));
        assert!(bar.view::<()>(()).is_some());
        bar.dismiss();
        assert!(bar.view::<()>(()).is_none());
    }
}
