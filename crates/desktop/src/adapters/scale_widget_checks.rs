//! Shared icon-scale widget assertions for the conversations panel.
//!
//! The production widgets read byteui globals (`icon_size::scale()`), so the
//! three scale checks need a process whose scale they control. Both the unit
//! tests (default scale, single process) and the dedicated
//! `icon_scale_widget_probe` binary (one process per scale) call the functions
//! here. Keeping one implementation is what prevents the two paths from
//! drifting apart.
//!
//! The services below are deliberately minimal and always `Unavailable`: they
//! never import `dozer-client`, never open a daemon socket and never touch the
//! filesystem.

use std::sync::Arc;

use bytehost_chrome::{BodyArea, Rect, Side};
use dozer_model::ProjectInfo;
use dozer_panel_conversations::adapter::PanelDisplay;
use dozer_services::{
    BoxFuture, ConversationRows, ConversationService, ServiceError, ServiceErrorKind,
    SummaryService, TranscriptService,
};
use iced_core::{Point, Rectangle};

use super::agent::{AgentShell, Status};
use super::conversations::ConversationsPanel;
use super::conversations_display::{ConversationsDisplay, review_pane_style};
use crate::product::Msg;

struct UnavailableStub;

fn unavailable() -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, "scale widget probe stub")
}

impl ConversationService for UnavailableStub {
    fn get_conversation_turns(
        &self,
        _: String,
        _: i64,
        _: u32,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::TurnRecord>, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }

    fn list_conversations_with_summaries(
        &self,
        _: String,
        _: Option<dozer_model::AgentKind>,
        _: u32,
        _: u32,
    ) -> BoxFuture<'_, Result<ConversationRows, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }
}

impl SummaryService for UnavailableStub {
    fn submit_summary_job(&self, _: String, _: bool) -> BoxFuture<'_, Result<i64, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }

    fn get_summary_job(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryJobInfo>, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }
}

impl TranscriptService for UnavailableStub {
    fn delete_project_transcripts(&self, _: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }

    fn backfill_project_transcripts(&self, _: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }

    fn submit_summary_batch(
        &self,
        _: String,
        _: Option<dozer_model::AgentKind>,
        _: Option<String>,
    ) -> BoxFuture<'_, Result<(i64, u32), ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }

    fn get_summary_batch(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryBatchInfo>, ServiceError>> {
        Box::pin(async { Err(unavailable()) })
    }
}

/// Build the panel used by the probe: stub services plus an in-memory import
/// marker, so no daemon or filesystem is involved.
pub fn stub_panel() -> ConversationsPanel {
    let conversation: Arc<dyn ConversationService> = Arc::new(UnavailableStub);
    let summary: Arc<dyn SummaryService> = Arc::new(UnavailableStub);
    let transcript: Arc<dyn TranscriptService> = Arc::new(UnavailableStub);
    ConversationsPanel::new_headless(conversation, summary, transcript)
}

/// Run the three scale-sensitive layout checks and verify the process is at
/// `expected_scale`. The first error is returned with enough context to read in
/// a probe's stderr.
pub fn check_all(panel: &mut ConversationsPanel, expected_scale: f32) -> Result<(), String> {
    let actual = byteui::theme::icon_size::scale();
    if actual != expected_scale {
        return Err(format!(
            "global icon scale is {actual}, expected {expected_scale}"
        ));
    }
    check_header_and_divider(panel).map_err(|error| format!("header/divider: {error}"))?;
    check_collapse_control_above_review(panel)
        .map_err(|error| format!("collapse control: {error}"))?;
    check_collapsed_notice(panel).map_err(|error| format!("collapsed notice: {error}"))?;
    Ok(())
}

fn project() -> ProjectInfo {
    ProjectInfo {
        id: 1,
        path: "/a".into(),
        name: "project 1".into(),
        last_active_ms: 0,
        created_ms: 0,
        updated_ms: 0,
    }
}

/// The panel header (and its divider) must stay inside the exported preview
/// chrome, above the native review child.
pub(crate) fn check_header_and_divider(panel: &mut ConversationsPanel) -> Result<(), String> {
    let display = ConversationsDisplay::new(1);
    let mut head = display.panel_head(
        byteui::interaction::icons::IconKind::BotMessageSquare,
        "对话".into(),
        None,
    );
    let (_, head_node, _) = layout(&mut head, 600.);
    let divider = head_node
        .children()
        .last()
        .ok_or_else(|| "panel head has no divider".to_owned())?
        .bounds();
    let style = review_pane_style();
    if !(divider.height > 0.) {
        return Err(format!("actual divider {divider:?} has no height"));
    }
    if !(style.padding.top + divider.y + divider.height
        <= byteui::theme::geometry::preview_chrome_top_px())
    {
        return Err(format!(
            "actual divider {divider:?} plus pane top {} exceeds native chrome {} at scale {}",
            style.padding.top,
            byteui::theme::geometry::preview_chrome_top_px(),
            byteui::theme::icon_size::scale()
        ));
    }
    if !(head_node.size().height > byteui::theme::geometry::tab_button_size()) {
        return Err(format!(
            "header height {} must contain its existing divider {}",
            head_node.size().height,
            byteui::theme::geometry::tab_button_size()
        ));
    }

    let a = project();
    panel.ensure_state(&a);
    let shared = ready();
    for collapsed in [false, true] {
        let mut view = panel.view(Some(&a), &area(600., collapsed), &shared);
        let (_, node, texts) = layout(&mut view, 600.);
        let bounds = review_bounds_for(&node, collapsed)?;
        let header = texts
            .0
            .iter()
            .find(|(text, _)| text == "对话")
            .ok_or_else(|| format!("header missing at collapsed={collapsed}"))?
            .1;
        if !(header.y + header.height <= bounds.y) {
            return Err(format!("{header:?} covered by native {bounds:?}"));
        }
        let mut all = vec![];
        rectangles(iced_core::Layout::new(&node), &mut all);
        let mut found_divider = false;
        for rect in &all {
            if rect.height == 1.
                && rect.width > bounds.w / 2.
                && rect.x < bounds.x + bounds.w
                && rect.x + rect.width > bounds.x
            {
                found_divider = true;
                if !(rect.y + rect.height <= bounds.y) {
                    return Err(format!(
                        "actual pane divider {rect:?} covered by native {bounds:?} at scale {}",
                        byteui::theme::icon_size::scale()
                    ));
                }
            }
        }
        if !found_divider {
            return Err("actual review pane divider must be laid out".into());
        }
    }
    Ok(())
}

/// The real collapse/expand control must be hittable and above the native
/// review child in both expanded and collapsed layouts.
pub(crate) fn check_collapse_control_above_review(
    panel: &mut ConversationsPanel,
) -> Result<(), String> {
    let a = project();
    panel.ensure_state(&a);
    let shared = ready();
    for collapsed in [false, true] {
        let mut view = panel.view(Some(&a), &area(600., collapsed), &shared);
        let (_, node, _) = layout(&mut view, 600.);
        let bounds = review_bounds_for(&node, collapsed)?;
        let mut all = vec![];
        rectangles(iced_core::Layout::new(&node), &mut all);
        let mut found = false;
        for rect in all {
            if click(&mut view, rect.center(), 600.)
                .iter()
                .any(|msg| matches!(msg, Msg::Shell(bytehost_chrome::ShellMsg::TogglePane(_))))
            {
                found = true;
                if !(rect.y + rect.height <= bounds.y) {
                    return Err(format!(
                        "collapse button {rect:?} covered by native {bounds:?}"
                    ));
                }
            }
        }
        if !found {
            return Err("real collapse button must be hittable".into());
        }
    }
    Ok(())
}

/// A collapsed notice must keep its dismiss control inside the reserved
/// viewport and outside the native review child, even for arbitrarily long
/// error text.
pub(crate) fn check_collapsed_notice(panel: &mut ConversationsPanel) -> Result<(), String> {
    let a = project();
    panel.ensure_state(&a);
    let error = format!(
        "无法读取会话文件 /private/tmp/history/{}: connection lost",
        "nested-path/".repeat(50)
    );
    panel.notice.push(error.clone());
    let shared = ready();
    let mut view = panel.view(Some(&a), &area(600., true), &shared);
    let (_, node, texts) = layout(&mut view, 600.);
    let texts = texts.0;
    let close = texts
        .iter()
        .find(|(text, _)| text == "×")
        .ok_or_else(|| "dismiss control missing".to_owned())?
        .1;
    if !texts.iter().any(|(text, _)| text == &error) {
        return Err("notice text is not rendered".into());
    }
    if !(close.y >= 0. && close.y + close.height <= panel.review_notice_height(true)) {
        return Err(format!(
            "dismiss control {close:?} must be in the initial notice viewport"
        ));
    }
    let bounds = review_bounds(
        Rect {
            x: 0.,
            y: panel.review_notice_height(true),
            w: 600.,
            h: 600. - panel.review_notice_height(true),
        },
        byteui::theme::icon_size::scale(),
    );
    if !(close.y + close.height <= bounds.y) {
        return Err(format!("{close:?} {bounds:?}"));
    }
    let header = texts
        .iter()
        .find(|(text, _)| text == "对话")
        .ok_or_else(|| "header missing".to_owned())?
        .1;
    if !(header.y + header.height <= bounds.y) {
        return Err(format!("header {header:?} covered by native {bounds:?}"));
    }
    let mut rects = vec![];
    rectangles(iced_core::Layout::new(&node), &mut rects);
    let mut found = false;
    for rect in rects {
        if click(&mut view, rect.center(), 600.)
            .iter()
            .any(|msg| matches!(msg, Msg::ConversationsNoticeDismiss))
        {
            found = true;
            if !(rect.y >= 0. && rect.y + rect.height <= panel.review_notice_height(true)) {
                return Err(format!(
                    "entire dismiss hit bounds {rect:?} must be inside initial notice viewport at scale {}",
                    byteui::theme::icon_size::scale()
                ));
            }
        }
    }
    if !found {
        return Err("notice dismiss button must be hittable".into());
    }
    if !click(&mut view, close.center(), 600.)
        .iter()
        .any(|m| matches!(m, Msg::ConversationsNoticeDismiss))
    {
        return Err("notice dismiss button center must be hittable".into());
    }
    Ok(())
}

/// The actual pane toggle must be hittable in both list orientations. This is
/// orientation coverage, not scale coverage; the probe shares the same helper
/// so the click semantics have one implementation.
#[cfg(test)]
pub(crate) fn pane_toggle_clickable(
    panel: &ConversationsPanel,
    collapsed: bool,
    mirrored: bool,
) -> bool {
    let shared = ready();
    let mut area = area(1000., collapsed);
    area.mirrored = mirrored;
    area.side = if mirrored { Side::Left } else { Side::Right };
    area.pair = Some(bytehost_chrome::PairRects {
        first: bytehost_chrome::Rect {
            x: if mirrored { 0. } else { 650. },
            y: 0.,
            w: 350.,
            h: 600.,
        },
        second: bytehost_chrome::Rect {
            x: if mirrored { 356. } else { 0. },
            y: 0.,
            w: 644.,
            h: 600.,
        },
        divider: bytehost_chrome::Rect {
            x: if mirrored { 350. } else { 644. },
            y: 0.,
            w: 6.,
            h: 600.,
        },
    });
    let a = project();
    let mut view = panel.view(Some(&a), &area, &shared);
    let (_, node, _) = layout(&mut view, 1000.);
    let mut points = vec![];
    centers(iced_core::Layout::new(&node), &mut points);
    points.into_iter().any(|point| {
        click(&mut view, point, 1000.).iter().any(|message| {
            matches!(message,
            Msg::Shell(bytehost_chrome::ShellMsg::TogglePane(id))
                if *id == crate::shell::conversations_id())
        })
    })
}

fn review_bounds_for(node: &iced_core::layout::Node, collapsed: bool) -> Result<Rect, String> {
    let block = if collapsed {
        Rect::new(0., 0., 600., 600.)
    } else {
        let block = node
            .children()
            .last()
            .ok_or_else(|| "expanded layout has no review block".to_owned())?
            .bounds();
        Rect::new(block.x, block.y, block.width, block.height)
    };
    Ok(review_bounds(block, byteui::theme::icon_size::scale()))
}

fn review_bounds(block: Rect, scale: f32) -> Rect {
    super::review_geometry::review_bounds(block, scale)
}

fn rectangles(layout: iced_core::Layout<'_>, out: &mut Vec<Rectangle>) {
    out.push(layout.bounds());
    for child in layout.children() {
        rectangles(child, out);
    }
}

#[cfg(test)]
fn centers(layout: iced_core::Layout<'_>, points: &mut Vec<Point>) {
    points.push(layout.bounds().center());
    for child in layout.children() {
        centers(child, points);
    }
}

pub(crate) fn ready() -> AgentShell {
    AgentShell {
        status: Status::Ready,
    }
}

pub(crate) fn area(width: f32, collapsed: bool) -> BodyArea {
    BodyArea {
        side: Side::Right,
        rect: bytehost_chrome::Rect {
            x: 0.,
            y: 0.,
            w: width,
            h: 600.,
        },
        pair: None,
        mirrored: false,
        split: 0.35,
        collapsed,
    }
}

/// Exercise the actual iced layout and Operation::text tree without a desktop
/// window.
#[derive(Default)]
pub(crate) struct Texts(pub(crate) Vec<(String, Rectangle)>);

impl iced_core::widget::Operation for Texts {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
        operate(self)
    }

    fn text(&mut self, _: Option<&iced_core::widget::Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.into(), bounds));
    }
}

thread_local! {
    /// Per-thread software renderer. A fresh tiny-skia renderer lazily loads the
    /// system font database on its first SVG measurement, which is expensive;
    /// reuse one per test thread instead of creating one per layout call.
    static LAYOUT_RENDERER: std::cell::RefCell<Option<iced_renderer::Renderer>> =
        const { std::cell::RefCell::new(None) };
}

fn with_renderer<R>(f: impl FnOnce(&mut iced_renderer::Renderer) -> R) -> R {
    LAYOUT_RENDERER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let renderer = slot.get_or_insert_with(|| {
            iced_renderer::fallback::Renderer::Secondary(iced_tiny_skia::Renderer::new(
                iced::Font::DEFAULT,
                iced_core::Pixels(14.),
            ))
        });
        f(renderer)
    })
}

fn check_node(node: &iced_core::layout::Node) {
    let bounds = node.bounds();
    assert!(
        bounds.width >= 0. && bounds.height >= 0.,
        "negative geometry {bounds:?}"
    );
    for child in node.children() {
        check_node(child);
    }
}

fn layout_parts(
    view: &mut iced::Element<'_, Msg>,
    width: f32,
    renderer: &mut iced_renderer::Renderer,
) -> (iced_core::widget::Tree, iced_core::layout::Node, Texts) {
    let mut tree = iced_core::widget::Tree::new(view.as_widget());
    let node = view.as_widget_mut().layout(
        &mut tree,
        renderer,
        &iced_core::layout::Limits::new(iced_core::Size::ZERO, iced_core::Size::new(width, 600.)),
    );
    check_node(&node);
    let mut texts = Texts::default();
    view.as_widget_mut().operate(
        &mut tree,
        iced_core::Layout::new(&node),
        renderer,
        &mut texts,
    );
    (tree, node, texts)
}

pub(crate) fn layout(
    view: &mut iced::Element<'_, Msg>,
    width: f32,
) -> (iced_core::widget::Tree, iced_core::layout::Node, Texts) {
    with_renderer(|renderer| layout_parts(view, width, renderer))
}

pub(crate) fn with_layout<R>(
    view: &mut iced::Element<'_, Msg>,
    width: f32,
    consume: impl FnOnce(
        &mut iced::Element<'_, Msg>,
        &mut iced_core::widget::Tree,
        &iced_core::layout::Node,
        &mut iced_renderer::Renderer,
        &Texts,
    ) -> R,
) -> R {
    with_renderer(|renderer| {
        let (mut tree, node, texts) = layout_parts(view, width, renderer);
        consume(view, &mut tree, &node, renderer, &texts)
    })
}

#[cfg(test)]
pub(crate) fn view_texts(
    panel: &ConversationsPanel,
    project: Option<&ProjectInfo>,
    shell: &AgentShell,
    width: f32,
    collapsed: bool,
) -> Vec<String> {
    let mut view = panel.view(project, &area(width, collapsed), shell);
    layout(&mut view, width)
        .2
        .0
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

pub(crate) fn click(view: &mut iced::Element<'_, Msg>, point: Point, width: f32) -> Vec<Msg> {
    with_layout(view, width, |view, tree, node, renderer, _| {
        let mut messages = vec![];
        for event in [
            iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left),
            iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left),
        ] {
            let mut shell = iced_core::Shell::new(&mut messages);
            view.as_widget_mut().update(
                tree,
                &iced_core::Event::Mouse(event),
                iced_core::Layout::new(node),
                iced_core::mouse::Cursor::Available(point),
                renderer,
                &mut iced_core::clipboard::Null,
                &mut shell,
                &iced_core::Rectangle::with_size(iced_core::Size::new(width, 600.)),
            );
        }
        messages
    })
}
