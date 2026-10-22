//! Agent 终端输入法状态机；控件层由下一阶段消费此状态。
use iced::{Element, Length};
use iced_core::input_method;
use iced_core::{
    Clipboard, Layout, Rectangle, Size, Vector, Widget, layout, mouse, overlay, renderer,
    widget::Tree,
};
use iced_core::{Event, Shell};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImeState {
    pub preedit: Option<String>,
    pub active: bool,
}
impl ImeState {
    pub fn reset(&mut self) {
        self.preedit = None;
        self.active = false;
    }
    pub fn on_event(&mut self, event: &input_method::Event) -> Option<String> {
        match event {
            input_method::Event::Opened => self.active = true,
            input_method::Event::Closed => self.reset(),
            input_method::Event::Preedit(text, _) => {
                self.preedit = (!text.is_empty()).then(|| text.clone())
            }
            input_method::Event::Commit(text) => {
                self.preedit = None;
                return (!text.is_empty()).then(|| text.clone());
            }
        }
        None
    }
}
/// IME 控件包装层的状态逻辑；Widget 转发层调用此函数，保证先消费 IME 再转发内容事件。
pub fn handle_event<M>(
    state: &mut ImeState,
    enabled: bool,
    event: &Event,
    shell: &mut Shell<'_, M>,
    on_commit: impl Fn(String) -> M,
    on_preedit: impl Fn(Option<String>) -> M,
) {
    if !enabled {
        state.reset();
        return;
    }
    if let Event::InputMethod(event) = event {
        let commit = state.on_event(event);
        if let Some(text) = commit {
            shell.publish(on_commit(text));
        }
        // 控件随每次 `view` 重建,`state` 不跨消息存活,所以不能靠"前后对比"判断预编辑是否变了:
        // 凡是会改预编辑的事件都发布当前值,由调用方去重。否则提交后宿主的预编辑文字永远清不掉。
        if changes_preedit(event) {
            shell.publish(on_preedit(state.preedit.clone()));
        }
    }
}
/// 这个输入法事件是否会改变预编辑(`Opened` 不会)。
fn changes_preedit(event: &input_method::Event) -> bool {
    !matches!(event, input_method::Event::Opened)
}
pub struct ImeSink<'a, M> {
    content: Element<'a, M>,
    enabled: bool,
    state: ImeState,
    cursor: Box<dyn Fn(Rectangle) -> Rectangle + 'a>,
    commit: Box<dyn Fn(String) -> M + 'a>,
    preedit: Box<dyn Fn(Option<String>) -> M + 'a>,
}
impl<'a, M: 'a> ImeSink<'a, M> {
    pub fn new(
        content: Element<'a, M>,
        enabled: bool,
        cursor: impl Fn(Rectangle) -> Rectangle + 'a,
        commit: impl Fn(String) -> M + 'a,
        preedit: impl Fn(Option<String>) -> M + 'a,
    ) -> Self {
        Self {
            content,
            enabled,
            state: ImeState::default(),
            cursor: Box::new(cursor),
            commit: Box::new(commit),
            preedit: Box::new(preedit),
        }
    }
}
impl<M> Widget<M, iced::Theme, iced::Renderer> for ImeSink<'_, M> {
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        handle_event(
            &mut self.state,
            self.enabled,
            event,
            shell,
            |s| (self.commit)(s),
            |s| (self.preedit)(s),
        );
        if self.enabled
            && matches!(
                event,
                Event::Window(iced_core::window::Event::RedrawRequested(_))
            )
        {
            let ime: iced_core::input_method::InputMethod<String> =
                iced_core::input_method::InputMethod::Enabled {
                    cursor: (self.cursor)(layout.bounds()),
                    purpose: iced_core::input_method::Purpose::Terminal,
                    preedit: None,
                };
            shell.request_input_method(&ime);
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
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
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
impl<'a, M: 'a> From<ImeSink<'a, M>> for Element<'a, M> {
    fn from(value: ImeSink<'a, M>) -> Self {
        Element::new(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preedit_update_replaces_the_previous_preedit() {
        let mut s = ImeState::default();
        s.on_event(&input_method::Event::Preedit("a".into(), None));
        s.on_event(&input_method::Event::Preedit("你好".into(), None));
        assert_eq!(s.preedit.as_deref(), Some("你好"));
    }
    #[test]
    fn commit_returns_the_text_and_clears_the_preedit() {
        let mut s = ImeState {
            preedit: Some("n".into()),
            active: true,
        };
        assert_eq!(
            s.on_event(&input_method::Event::Commit("你".into())),
            Some("你".into())
        );
        assert_eq!(s.preedit, None);
    }
    #[test]
    fn empty_commit_returns_none_and_still_clears_the_preedit() {
        let mut s = ImeState {
            preedit: Some("n".into()),
            active: true,
        };
        assert_eq!(s.on_event(&input_method::Event::Commit("".into())), None);
        assert_eq!(s.preedit, None);
    }
    #[test]
    fn closed_clears_everything() {
        let mut s = ImeState {
            preedit: Some("n".into()),
            active: true,
        };
        s.on_event(&input_method::Event::Closed);
        assert_eq!(s, ImeState::default());
    }
    #[test]
    fn commit_with_newlines_and_a_100k_char_string_is_returned_whole_without_panicking() {
        let mut s = ImeState::default();
        let text = format!("a\nb\r\n{}", "你".repeat(100_000));
        assert_eq!(
            s.on_event(&input_method::Event::Commit(text.clone())),
            Some(text)
        );
    }
    #[test]
    fn only_opened_leaves_the_preedit_alone() {
        assert!(!changes_preedit(&input_method::Event::Opened));
        assert!(changes_preedit(&input_method::Event::Closed));
        assert!(changes_preedit(&input_method::Event::Commit("x".into())));
        assert!(changes_preedit(&input_method::Event::Preedit(
            "x".into(),
            None
        )));
    }
    #[test]
    fn preedit_then_losing_enabled_is_cleared_by_the_sink() {
        let mut s = ImeState {
            preedit: Some("ni".into()),
            active: true,
        };
        s.reset();
        assert_eq!(s, ImeState::default());
    }
}
