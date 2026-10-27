//! Iced modal shell for the group-creation card (click backdrop does not close).
use iced::widget::{container, mouse_area, stack};
use iced::{Element, Length};

use crate::product::Msg;

pub fn dialog_overlay<'a>(card: Element<'a, Msg>, _cancel: Msg) -> Element<'a, Msg> {
    let scrim: Element<'a, Msg> = mouse_area(
        container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_t: &iced::Theme| container::Style {
                background: Some(
                    iced::Color {
                        a: 0.35,
                        ..iced::Color::BLACK
                    }
                    .into(),
                ),
                ..container::Style::default()
            }),
    )
    .on_press(Msg::Shell(bytehost_chrome::ShellMsg::Noop))
    .into();
    let centered: Element<'a, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(24)
        .into();
    stack![scrim, centered].into()
}
