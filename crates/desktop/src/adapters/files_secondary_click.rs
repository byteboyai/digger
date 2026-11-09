//! macOS Control-click compatibility at the Files widget boundary.
//!
//! winit preserves Control-click as a left button event. Translate the entire
//! gesture before it reaches the shared tree, so it cannot also select, drag,
//! double-click or toggle the root. Actual secondary-button events pass through.

use iced::{Element, Length};
use iced_core::{
    Clipboard, Event, Layout, Rectangle, Shell, Size, Vector, Widget, keyboard, layout, mouse,
    overlay, renderer, widget::Tree,
};

#[derive(Default)]
struct State {
    control: bool,
    control_click: bool,
}

pub(super) struct SecondaryClick<'a, M> {
    content: Element<'a, M>,
}

impl<'a, M> SecondaryClick<'a, M> {
    pub(super) fn new(content: Element<'a, M>) -> Self {
        Self { content }
    }
}
impl<M> Widget<M, iced::Theme, iced::Renderer> for SecondaryClick<'_, M> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn tag(&self) -> iced_core::widget::tree::Tag {
        iced_core::widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> iced_core::widget::tree::State {
        iced_core::widget::tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let mapped = match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.control = modifiers.control();
                None
            }
            Event::Window(iced_core::window::Event::Unfocused) => {
                *state = State::default();
                None
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.control_click = state.control && cursor.is_over(layout.bounds());
                state
                    .control_click
                    .then_some(Event::Mouse(mouse::Event::ButtonPressed(
                        mouse::Button::Right,
                    )))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                std::mem::take(&mut state.control_click).then_some(Event::Mouse(
                    mouse::Event::ButtonReleased(mouse::Button::Right),
                ))
            }
            _ => None,
        };
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            mapped.as_ref().unwrap_or(event),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        )
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
            .operate(&mut tree.children[0], layout, renderer, operation)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
impl<'a, M: 'a> From<SecondaryClick<'a, M>> for Element<'a, M> {
    fn from(value: SecondaryClick<'a, M>) -> Self {
        Element::new(value)
    }
}
