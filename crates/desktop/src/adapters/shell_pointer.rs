//! Ordered main-window pointer samples, before the rail's press/hover intents.
use iced::Element;
use iced_core::{
    Clipboard, Event, Layout, Length, Rectangle, Shell, Size, Vector, Widget, layout, mouse,
    overlay, renderer, widget::Tree,
};
pub(crate) struct ShellPointer<'a> {
    content: Element<'a, crate::product::Msg>,
}
impl<'a> ShellPointer<'a> {
    pub(crate) fn new(content: Element<'a, crate::product::Msg>) -> Self {
        Self { content }
    }
}
impl Widget<crate::product::Msg, iced::Theme, iced::Renderer> for ShellPointer<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn tag(&self) -> iced_core::widget::tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> iced_core::widget::tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::product::Msg>,
        viewport: &Rectangle,
    ) {
        // Samples and widget intents must be in the same ordered message batch.
        // A delayed global cursor subscription must not become the drag origin.
        let point = match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => Some(*position),
            Event::Mouse(mouse::Event::ButtonPressed(_)) => cursor.position(),
            _ => None,
        };
        if let Some(point) = point {
            shell.publish(crate::product::Msg::Shell(
                bytehost_chrome::ShellMsg::CursorMoved {
                    x: point.x,
                    y: point.y,
                },
            ));
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            shell.publish(crate::product::Msg::Shell(
                bytehost_chrome::ShellMsg::ZoneClicked,
            ));
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        // Clear the pending gesture in the release event itself, even if a child
        // captured it; hovering after a click must never keep dragging.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) {
            shell.publish(crate::product::Msg::Shell(
                bytehost_chrome::ShellMsg::PointerReleased,
            ));
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport)
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced_core::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, crate::product::Msg, iced::Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
impl<'a> From<ShellPointer<'a>> for Element<'a, crate::product::Msg> {
    fn from(value: ShellPointer<'a>) -> Self {
        Element::new(value)
    }
}
