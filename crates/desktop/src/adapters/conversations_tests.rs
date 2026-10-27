use super::*;
use crate::adapters::agent::{AgentShell, AgentShellMsg, Status};
use crate::adapters::conversations_display::ConversationsDisplay;
use dozer_model::{
    AgentKind, ConversationSummary, ProjectInfo, SessionRow, SessionSummaryPayload, SummaryJobInfo,
    TurnRecord,
};
use dozer_panel_conversations::adapter::{ConvHover, PanelDisplay};
use dozer_services::{BoxFuture, ConversationRows, ServiceError, ServiceErrorKind};
use std::sync::{Arc, Mutex};

type ListCall = (String, Option<AgentKind>, u32, u32);

#[derive(Default)]
pub(crate) struct FakeConversation {
    pub calls: Mutex<Vec<ListCall>>,
    pub turns: Mutex<Vec<(String, i64, u32)>>,
    pub error: Mutex<Option<ServiceError>>,
    pub turn_responses: Mutex<std::collections::VecDeque<Result<Vec<TurnRecord>, ServiceError>>>,
    pub responses:
        Mutex<std::collections::VecDeque<tokio::sync::oneshot::Receiver<ConversationRows>>>,
}
impl ConversationService for FakeConversation {
    fn list_conversations_with_summaries(
        &self,
        cwd: String,
        agent: Option<AgentKind>,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'_, Result<ConversationRows, ServiceError>> {
        self.calls
            .lock()
            .unwrap()
            .push((cwd.clone(), agent, limit, offset));
        if let Some(response) = self.responses.lock().unwrap().pop_front() {
            return Box::pin(async move { Ok(response.await.unwrap()) });
        }
        let result = self
            .error
            .lock()
            .unwrap()
            .clone()
            .map_or_else(|| Ok(vec![conversation(&cwd, "a title")]), Err);
        Box::pin(async move { result })
    }
    fn get_conversation_turns(
        &self,
        cid: String,
        after: i64,
        limit: u32,
    ) -> BoxFuture<'_, Result<Vec<TurnRecord>, ServiceError>> {
        self.turns.lock().unwrap().push((cid, after, limit));
        let result = self
            .turn_responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(vec![]));
        Box::pin(async { result })
    }
}
#[derive(Default)]
pub(crate) struct FakeSummary(pub Mutex<usize>);
impl SummaryService for FakeSummary {
    fn submit_summary_job(&self, _: String, _: bool) -> BoxFuture<'_, Result<i64, ServiceError>> {
        *self.0.lock().unwrap() += 1;
        Box::pin(async { Ok(1) })
    }
    fn get_summary_job(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<SummaryJobInfo>, ServiceError>> {
        Box::pin(async { Ok(None) })
    }
}
pub(crate) fn conversation(
    id: &str,
    title: &str,
) -> (ConversationSummary, Option<SessionSummaryPayload>) {
    (
        ConversationSummary {
            conversation_id: id.into(),
            agent: AgentKind::Claude,
            file_path: format!("/tmp/{id}.jsonl"),
            title: title.into(),
            first_ts: 0,
            last_ts: 0,
            turn_count: 1,
        },
        None,
    )
}
pub(crate) fn row(id: &str) -> SessionRow {
    let (c, s) = conversation(id, id);
    SessionRow::from_row(&c, s.as_ref())
}
pub(crate) fn project(id: i64, path: &str) -> ProjectInfo {
    ProjectInfo {
        id,
        path: path.into(),
        name: format!("project {id}"),
        last_active_ms: 0,
        created_ms: 0,
        updated_ms: 0,
    }
}
pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
pub(crate) fn panel(
    rt: &tokio::runtime::Runtime,
) -> (ConversationsPanel, Arc<FakeConversation>, Arc<FakeSummary>) {
    let fake = Arc::new(FakeConversation::default());
    let summary = Arc::new(FakeSummary::default());
    let mut panel = ConversationsPanel::new_with_marker_path(
        fake.clone(),
        summary.clone(),
        Arc::new(crate::adapters::project::UnavailableService::new(
            "test transcript",
        )),
        tempfile::tempdir().unwrap().keep().join("marker.json"),
    );
    panel.attach_handle(rt.handle().clone());
    (panel, fake, summary)
}
pub(crate) fn receive(
    panel: &mut ConversationsPanel,
    rt: &tokio::runtime::Runtime,
) -> (i64, Message) {
    rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            panel.rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap()
    })
}
fn apply(panel: &mut ConversationsPanel, rt: &tokio::runtime::Runtime, p: &ProjectInfo) {
    let (pid, m) = receive(panel, rt);
    let _ = panel.update(pid, m, Some(p));
}
fn ready() -> AgentShell {
    AgentShell {
        status: Status::Ready,
    }
}
pub(crate) fn area(width: f32, collapsed: bool) -> bytehost_chrome::BodyArea {
    bytehost_chrome::BodyArea {
        side: bytehost_chrome::Side::Right,
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
fn tree(
    panel: &ConversationsPanel,
    project: Option<&ProjectInfo>,
    shell: &AgentShell,
    width: f32,
    collapsed: bool,
) -> iced_core::widget::Tree {
    let view = panel.view(project, &area(width, collapsed), shell);
    iced_core::widget::Tree::new(view.as_widget())
}
#[test]
fn messages_for_one_project_never_touch_another_projects_list() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    let b = project(2, "/b");
    panel.ensure_state(&a);
    panel.ensure_state(&b);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(2, Ok(vec![row("b")])),
        Some(&b),
    );
    assert!(panel.state(1).unwrap().sessions().is_none());
    assert_eq!(
        panel.state(2).unwrap().sessions().unwrap()[0].conversation_id,
        "b"
    );
}
#[test]
fn a_message_for_a_project_without_state_is_ignored() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("x")])),
        Some(&a),
    );
    assert!(panel.state(1).is_none());
}
#[test]
fn late_messages_after_close_project_are_ignored() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    let _ = panel.activate(&a);
    let (id, m) = receive(&mut panel, &rt);
    panel.close_project(1);
    let _ = panel.update(id, m, Some(&a));
    assert!(panel.state(1).is_none());
}
#[test]
fn reopening_a_project_rejects_the_old_owners_tokens_and_errors() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let token = panel.state(1).unwrap().requests.begin(Some(1), "sessions");
    panel.close_project(1);
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::AsyncResult(
            token,
            Box::new(Message::SessionsRefreshed(
                1,
                Err(ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    "old owner",
                )),
            )),
        ),
        Some(&a),
    );
    assert!(panel.state(1).unwrap().sessions().is_none());
    assert!(panel.notice.is_none());
}
#[test]
fn the_list_only_contains_sessions_under_the_current_project_path() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a/");
    let _ = panel.activate(&a);
    apply(&mut panel, &rt, &a);
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec![("/a/".into(), None, 500, 0)]
    );
    assert_eq!(
        panel.state(1).unwrap().sessions().unwrap()[0].conversation_id,
        "/a/"
    );
}
#[test]
fn parent_and_child_projects_pass_their_own_cwd_and_never_each_others() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    for p in [project(1, "/a"), project(2, "/a/b")] {
        let _ = panel.activate(&p);
        apply(&mut panel, &rt, &p);
        assert_eq!(
            panel.state(p.id).unwrap().sessions().unwrap()[0].conversation_id,
            p.path
        );
    }
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec![("/a".into(), None, 500, 0), ("/a/b".into(), None, 500, 0)]
    );
}
#[test]
fn first_visibility_creates_the_state_and_refreshes_and_second_visibility_refreshes_again() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    assert!(panel.state(1).is_none());
    for _ in 0..2 {
        let _ = panel.activate(&a);
        apply(&mut panel, &rt, &a);
    }
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
}
#[test]
fn daemon_unavailable_shows_the_reason_not_an_empty_list() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let shell = AgentShell {
        status: Status::Unavailable("socket permission denied".into()),
    };
    let view = tree(&panel, Some(&a), &shell, 500., false);
    assert!(!view.children.is_empty());
    assert!(matches!(
        shell.body(true),
        crate::adapters::agent::Body::Unavailable("socket permission denied")
    ));
    let texts = view_texts(&panel, Some(&a), &shell, 500., false);
    assert!(texts.iter().any(|s| s == "socket permission denied"));
    assert!(texts.iter().any(|s| s == "重试"));
    assert!(
        !texts
            .iter()
            .any(|s| s.contains("没有会话") || s.contains("No conversations"))
    );
    let checking = AgentShell {
        status: Status::Checking,
    };
    assert!(
        view_texts(&panel, Some(&a), &checking, 500., false)
            .iter()
            .any(|s| s.contains("正在连接"))
    );
}
#[test]
fn retry_after_unavailable_refetches() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    let mut shell = AgentShell {
        status: Status::Unavailable("offline".into()),
    };
    let _ = shell.update(AgentShellMsg::Retry, iced::Task::none);
    let _ = shell.update(AgentShellMsg::Ensured(Ok(())), iced::Task::none);
    assert_eq!(shell.status, Status::Ready);
    let _ = panel.activate(&a);
    apply(&mut panel, &rt, &a);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
}
#[test]
fn service_failures_surface_as_a_dismissable_notice() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    *fake.error.lock().unwrap() = Some(ServiceError::new(
        ServiceErrorKind::Rejected,
        "read rejected",
    ));
    let _ = panel.activate(&a);
    apply(&mut panel, &rt, &a);
    assert!(panel.notice.as_deref().unwrap().contains("read rejected"));
    assert!(panel.states.get_mut(&1).unwrap().take_outbox().is_empty());
    let shared = ready();
    let texts = view_texts(&panel, Some(&a), &shared, 500., false);
    assert!(texts.iter().any(|s| s.contains("read rejected")));
    let mut view = panel.view(Some(&a), &area(500., false), &shared);
    let close = layout(&mut view, 500.)
        .3
        .0
        .into_iter()
        .find(|(s, _)| s == "×")
        .unwrap()
        .1
        .center();
    assert!(
        click(&mut view, close, 500.)
            .iter()
            .any(|m| matches!(m, Msg::ConversationsNoticeDismiss))
    );
    drop(view);
    panel.dismiss_notice();
    assert!(panel.notice.is_none());
}
#[test]
fn unavailable_list_failures_are_visible_even_when_the_library_outbox_is_empty() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    *fake.error.lock().unwrap() = Some(ServiceError::new(
        ServiceErrorKind::Unavailable,
        "connection lost",
    ));
    let _ = panel.activate(&a);
    apply(&mut panel, &rt, &a);
    assert!(panel.notice.as_deref().unwrap().contains("connection lost"));
}
#[test]
fn stale_list_failures_do_not_surface_as_new_notices() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let old = panel.state(1).unwrap().requests.begin(Some(1), "sessions");
    let new = panel.state(1).unwrap().requests.begin(Some(1), "sessions");
    let _ = panel.update(
        1,
        Message::AsyncResult(
            new,
            Box::new(Message::SessionsRefreshed(1, Ok(vec![row("new")]))),
        ),
        Some(&a),
    );
    let _ = panel.update(
        1,
        Message::AsyncResult(
            old,
            Box::new(Message::SessionsRefreshed(
                1,
                Err(ServiceError::new(ServiceErrorKind::Unavailable, "stale")),
            )),
        ),
        Some(&a),
    );
    assert!(panel.notice.is_none());
    assert_eq!(
        panel.state(1).unwrap().sessions().unwrap()[0].conversation_id,
        "new"
    );
}
#[test]
fn no_summary_button_and_no_linked_todo_text_in_the_view() {
    let rt = runtime();
    let (mut panel, _, summary) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&a),
    );
    assert!(!panel.caps.summaries && !panel.caps.linked_todos);
    assert_eq!(NoTodos.linked_text("one"), None);
    let texts = view_texts(&panel, Some(&a), &ready(), 500., false);
    assert!(
        texts.iter().any(|s| s.contains("暂无审阅内容")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|s| s.contains("生成总结")
            || s.contains("Generate summary")
            || s.contains("关联待办")
            || s.contains("Linked todo")),
        "{texts:?}"
    );
    let _ = panel.update(
        1,
        Message::SummaryGenerate("one".into(), AgentKind::Claude),
        Some(&a),
    );
    assert_eq!(*summary.0.lock().unwrap(), 0);
}
#[test]
fn hover_is_static_and_collapse_button_toggles_the_conversations_pane() {
    let display = ConversationsDisplay::new(1);
    assert_eq!(display.hover_progress(ConvHover::SearchSubmit), 0.);
    assert_eq!(display.hover_progress(ConvHover::More), 0.);
    let button = display.list_collapse_button("collapse".into(), "expand".into());
    assert!(
        !iced_core::widget::Tree::new(button.as_widget())
            .children
            .is_empty()
    );
    assert!(
        matches!(crate::adapters::conversations_display::toggle_pane_message(),Msg::Shell(bytehost_chrome::ShellMsg::TogglePane(id)) if id==crate::shell::conversations_id())
    );
}
#[test]
fn degenerate_states_build_without_panicking() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    let _ = tree(&panel, None, &ready(), 0., false);
    panel.ensure_state(&a);
    let _ = tree(&panel, Some(&a), &ready(), 0., false);
    for rows in [
        vec![],
        (0..500)
            .map(|i| row(&format!("{i}{}", "长标题".repeat(1000))))
            .collect(),
    ] {
        let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(rows)), Some(&a));
        for collapsed in [false, true] {
            let _ = view_texts(&panel, Some(&a), &ready(), 0., collapsed);
        }
    }
}
#[test]
fn session_open_uses_the_librarys_normal_two_hundred_turn_fetch() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&a),
    );
    let _ = panel.update(
        1,
        Message::SessionOpen("one".into(), AgentKind::Claude),
        Some(&a),
    );
    apply(&mut panel, &rt, &a);
    assert_eq!(*fake.turns.lock().unwrap(), vec![("one".into(), -1, 200)]);
    assert!(panel.state(1).unwrap().review.is_some());
    let texts = view_texts(&panel, Some(&a), &ready(), 500., false);
    assert!(texts.iter().any(|s| s.contains("暂无对话")), "{texts:?}");
}

/// Exercise the actual iced layout and Operation::text tree without a desktop window.
#[derive(Default)]
struct Texts(Vec<(String, iced_core::Rectangle)>);
impl iced_core::widget::Operation for Texts {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
        operate(self)
    }
    fn text(
        &mut self,
        _: Option<&iced_core::widget::Id>,
        bounds: iced_core::Rectangle,
        text: &str,
    ) {
        self.0.push((text.into(), bounds));
    }
}
fn renderer() -> iced_renderer::Renderer {
    iced_renderer::fallback::Renderer::Secondary(iced_tiny_skia::Renderer::new(
        iced::Font::DEFAULT,
        iced_core::Pixels(14.),
    ))
}
fn layout(
    view: &mut iced::Element<'_, Msg>,
    width: f32,
) -> (
    iced_core::widget::Tree,
    iced_core::layout::Node,
    iced_renderer::Renderer,
    Texts,
) {
    let renderer = renderer();
    let mut tree = iced_core::widget::Tree::new(view.as_widget());
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &iced_core::layout::Limits::new(iced_core::Size::ZERO, iced_core::Size::new(width, 600.)),
    );
    fn check(node: &iced_core::layout::Node) {
        let bounds = node.bounds();
        assert!(
            bounds.width >= 0. && bounds.height >= 0.,
            "negative geometry {bounds:?}"
        );
        for child in node.children() {
            check(child);
        }
    }
    check(&node);
    let mut texts = Texts::default();
    view.as_widget_mut().operate(
        &mut tree,
        iced_core::Layout::new(&node),
        &renderer,
        &mut texts,
    );
    (tree, node, renderer, texts)
}
pub(crate) fn view_texts(
    panel: &ConversationsPanel,
    project: Option<&ProjectInfo>,
    shell: &AgentShell,
    width: f32,
    collapsed: bool,
) -> Vec<String> {
    let mut view = panel.view(project, &area(width, collapsed), shell);
    layout(&mut view, width)
        .3
        .0
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}
fn click(view: &mut iced::Element<'_, Msg>, point: iced_core::Point, width: f32) -> Vec<Msg> {
    let (mut tree, node, renderer, _) = layout(view, width);
    let mut messages = vec![];
    for event in [
        iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left),
        iced_core::mouse::Event::ButtonReleased(iced_core::mouse::Button::Left),
    ] {
        let mut shell = iced_core::Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &iced_core::Event::Mouse(event),
            iced_core::Layout::new(&node),
            iced_core::mouse::Cursor::Available(point),
            &renderer,
            &mut iced_core::clipboard::Null,
            &mut shell,
            &iced_core::Rectangle::with_size(iced_core::Size::new(width, 600.)),
        );
    }
    messages
}
#[test]
fn conversations_display_relative_time_is_localized_and_saturates_future_timestamps() {
    use crate::adapters::conversations_display::relative_time;
    assert_eq!(relative_time(0, 120_000, "zh-CN"), "2 分钟前");
    assert_eq!(relative_time(0, 120_000, "en"), "2m ago");
    assert_eq!(relative_time(10, 0, "zh-CN"), "刚刚");
}
#[test]
fn conversations_display_wraps_messages_with_the_project_id_and_receiver_is_taken_once() {
    let display = ConversationsDisplay::new(7);
    assert!(matches!(
        display.wrap(Message::SearchSubmit),
        Msg::Conversations(7, Message::SearchSubmit)
    ));
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    assert!(panel.take_receiver().is_some());
    assert!(panel.take_receiver().is_none());
}
#[test]
fn conversations_collapse_tooltip_retains_its_tree_and_the_button_dispatches_shell_toggle() {
    let display = ConversationsDisplay::new(1);
    let mut view = display.list_collapse_button("collapse".into(), "expand".into());
    let (tree, node, _, _) = layout(&mut view, 100.);
    let before = tree.children.len();
    let messages = click(&mut view, node.bounds().center(), 100.);
    assert!(messages.iter().any(|m|matches!(m,Msg::Shell(bytehost_chrome::ShellMsg::TogglePane(id)) if *id==crate::shell::conversations_id())));
    let (tree, _, _, _) = layout(&mut view, 100.);
    assert_eq!(tree.children.len(), before);
}
#[test]
fn conversations_unavailable_retry_button_dispatches_the_shared_agent_shell_message() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let shell = AgentShell {
        status: Status::Unavailable("socket permission denied".into()),
    };
    let mut view = panel.view(Some(&a), &area(600., false), &shell);
    let texts = layout(&mut view, 600.).3;
    let retry = texts
        .0
        .iter()
        .find(|(s, _)| s == "重试")
        .unwrap()
        .1
        .center();
    assert!(
        click(&mut view, retry, 600.)
            .iter()
            .any(|m| matches!(m, Msg::AgentShell(AgentShellMsg::Retry)))
    );
}

#[test]
fn conversations_pair_uses_the_real_list_width_and_collapse_leaves_only_the_review() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&a),
    );
    let shared = ready();
    let mut area = area(1000., false);
    area.pair = Some(bytehost_chrome::PairRects {
        first: bytehost_chrome::Rect {
            x: 700.,
            y: 0.,
            w: 300.,
            h: 600.,
        },
        divider: bytehost_chrome::Rect {
            x: 694.,
            y: 0.,
            w: 6.,
            h: 600.,
        },
        second: bytehost_chrome::Rect {
            x: 0.,
            y: 0.,
            w: 694.,
            h: 600.,
        },
    });
    area.split = 0.3;
    let mut view = panel.view(Some(&a), &area, &shared);
    let (_, _, _, texts) = layout(&mut view, 1000.);
    let title = texts.0.iter().find(|(s, _)| s == "one").unwrap().1;
    assert!(title.x >= 694. && title.width <= 300., "{title:?}");
    drop(view);
    area.collapsed = true;
    let mut view = panel.view(Some(&a), &area, &shared);
    let texts = layout(&mut view, 1000.).3.0;
    assert!(
        texts
            .iter()
            .any(|(s, _)| s.contains("点击") || s.contains("审阅")),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|(s, _)| s == "one"));
    drop(view);
    assert!(view_texts(&panel, None, &shared, 1000., false).is_empty());
}

#[test]
fn conversations_pane_styles_match_the_agent_list_and_review_regions() {
    use dozer_panel_conversations::adapter::ConversationsPane;
    let display = ConversationsDisplay::new(1);
    let list = display.pane_style(ConversationsPane::List);
    assert_eq!(
        list.padding,
        iced_core::Padding {
            top: 6.,
            right: 12.,
            bottom: 12.,
            left: 12.
        }
    );
    let detail = display.pane_style(ConversationsPane::Detail);
    assert_eq!(
        detail.padding,
        iced_core::Padding {
            top: 8.,
            right: 8.,
            bottom: 3.,
            left: 8.
        }
    );
    assert_eq!(detail.header_padding, iced_core::Padding::ZERO);
    assert_eq!(detail.gap, 4.);
    assert_eq!(list.line_height, 1.2);
}

fn actual_view_has_pane_toggle(
    panel: &ConversationsPanel,
    project: &ProjectInfo,
    collapsed: bool,
    mirrored: bool,
) -> bool {
    let shared = ready();
    let mut area = area(1000., collapsed);
    area.mirrored = mirrored;
    area.side = if mirrored {
        bytehost_chrome::Side::Left
    } else {
        bytehost_chrome::Side::Right
    };
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
    let mut view = panel.view(Some(project), &area, &shared);
    let (_, node, _, _) = layout(&mut view, 1000.);
    fn centers(layout: iced_core::Layout<'_>, points: &mut Vec<iced_core::Point>) {
        points.push(layout.bounds().center());
        for child in layout.children() {
            centers(child, points);
        }
    }
    // Probe centers from the actual laid-out widget tree, including icon buttons
    // whose text is deliberately only present in their tooltip overlay.
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

#[test]
fn the_actual_expanded_conversations_view_has_a_clickable_collapse_control() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let project = project(1, "/a");
    panel.ensure_state(&project);
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(vec![])), Some(&project));
    for mirrored in [false, true] {
        assert!(
            actual_view_has_pane_toggle(&panel, &project, false, mirrored),
            "expanded mirrored={mirrored}"
        );
    }
}

#[test]
fn the_actual_collapsed_conversations_view_has_a_clickable_expand_control() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let project = project(1, "/a");
    panel.ensure_state(&project);
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(vec![])), Some(&project));
    for mirrored in [false, true] {
        assert!(
            actual_view_has_pane_toggle(&panel, &project, true, mirrored),
            "collapsed mirrored={mirrored}"
        );
    }
}

#[test]
fn turn_ended_refreshes_only_the_matching_visible_project() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    let b = project(2, "/b");
    panel.ensure_state(&a);
    let _ = panel.on_turn_ended(1, Some(&b), true);
    let _ = panel.on_turn_ended(2, Some(&b), true);
    let _ = panel.on_turn_ended(1, None, true);
    rt.block_on(async { tokio::task::yield_now().await });
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(panel.dirty.is_empty());
    let _ = panel.on_turn_ended(1, Some(&a), true);
    apply(&mut panel, &rt, &a);
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec![("/a".into(), None, 500, 0)]
    );
}

#[test]
fn turn_ended_while_hidden_marks_dirty_and_the_next_visibility_refreshes_once() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    for _ in 0..10 {
        let _ = panel.on_turn_ended(1, Some(&a), false);
    }
    assert!(panel.dirty.contains(&1));
    rt.block_on(async { tokio::task::yield_now().await });
    assert!(fake.calls.lock().unwrap().is_empty());
    let _ = panel.activate(&a);
    apply(&mut panel, &rt, &a);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    assert!(!panel.dirty.contains(&1));
    panel.close_project(1);
    let _ = panel.on_turn_ended(1, Some(&a), false);
    assert!(!panel.dirty.contains(&1));
}

#[test]
fn ten_turn_ended_events_in_a_row_leave_only_the_latest_result_applied() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let mut senders = vec![];
    for _ in 0..10 {
        let (tx, rx) = tokio::sync::oneshot::channel();
        senders.push(tx);
        fake.responses.lock().unwrap().push_back(rx);
        let _ = panel.on_turn_ended(1, Some(&a), true);
    }
    rt.block_on(async { tokio::task::yield_now().await });
    assert_eq!(fake.calls.lock().unwrap().len(), 10);
    for (index, sender) in senders.into_iter().enumerate().rev() {
        sender
            .send(vec![conversation(&index.to_string(), "result")])
            .unwrap();
        apply(&mut panel, &rt, &a);
    }
    assert_eq!(
        panel.state(1).unwrap().sessions().unwrap()[0].conversation_id,
        "9"
    );
}

#[test]
fn escape_and_outside_click_close_the_filter_menu_without_changing_the_filter() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::AgentFilterSelect(Some(AgentKind::Claude)),
        Some(&a),
    );
    for _ in 0..2 {
        panel.ensure_state(&a).set_agent_picker_open(true);
        let _ = panel.update(1, Message::AgentFilterClose, Some(&a));
        assert!(!panel.state(1).unwrap().agent_picker_open());
        assert_eq!(
            panel.state(1).unwrap().agent_filter(),
            Some(AgentKind::Claude)
        );
    }
}

#[test]
fn selecting_an_agent_resets_paging_and_closes_the_menu() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let rows = (0..100).map(|i| row(&format!("row {i}"))).collect();
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(rows)), Some(&a));
    let first = view_texts(&panel, Some(&a), &ready(), 500., false);
    let _ = panel.update(1, Message::ListMore, Some(&a));
    let more = view_texts(&panel, Some(&a), &ready(), 500., false);
    assert!(more.len() > first.len());
    panel.ensure_state(&a).set_agent_picker_open(true);
    let _ = panel.update(
        1,
        Message::AgentFilterSelect(Some(AgentKind::Claude)),
        Some(&a),
    );
    assert!(!panel.state(1).unwrap().agent_picker_open());
    assert_eq!(
        view_texts(&panel, Some(&a), &ready(), 500., false).len(),
        first.len()
    );
}

/// Execute the runtime's actual query Operation against the library's text input.
pub(crate) fn search_focus_feedback(
    panel: &ConversationsPanel,
    project: &ProjectInfo,
    rt: &tokio::runtime::Runtime,
    task: Task<Msg>,
    focused: bool,
) -> Msg {
    use iced::futures::StreamExt;
    let mut stream = iced_runtime::task::into_stream(task).expect("focus query task");
    let iced_runtime::Action::Widget(mut operation) = rt.block_on(stream.next()).unwrap() else {
        panic!("widget operation")
    };
    let ready = ready();
    let mut view = panel.view(Some(project), &area(500., false), &ready);
    let (mut tree, node, renderer, _) = layout(&mut view, 500.);
    let mut focus: Box<dyn iced_core::widget::Operation> = if focused {
        Box::new(iced_core::widget::operation::focusable::focus(
            dozer_panel_conversations::view::conversation_search_field_id(),
        ))
    } else {
        Box::new(iced_core::widget::operation::focusable::unfocus())
    };
    view.as_widget_mut().operate(
        &mut tree,
        iced_core::Layout::new(&node),
        &renderer,
        focus.as_mut(),
    );
    view.as_widget_mut().operate(
        &mut tree,
        iced_core::Layout::new(&node),
        &renderer,
        operation.as_mut(),
    );
    let _ = operation.finish();
    drop(operation);
    let iced_runtime::Action::Output(message) = rt.block_on(stream.next()).unwrap() else {
        panic!("focus feedback")
    };
    message
}

#[test]
fn conversations_real_search_text_input_captures_characters_before_agent_subscription() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let ready = ready();
    let mut view = panel.view(Some(&a), &area(500., false), &ready);
    let (mut tree, node, renderer, _) = layout(&mut view, 500.);
    view.as_widget_mut().operate(
        &mut tree,
        iced_core::Layout::new(&node),
        &renderer,
        &mut iced_core::widget::operation::focusable::focus(
            dozer_panel_conversations::view::conversation_search_field_id(),
        ),
    );
    let event = iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("x".into()),
        modified_key: iced::keyboard::Key::Character("x".into()),
        physical_key: iced::keyboard::key::Physical::Unidentified(
            iced::keyboard::key::NativeCode::Unidentified,
        ),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        text: Some("x".into()),
        repeat: false,
    };
    let mut messages = vec![];
    let mut shell = iced_core::Shell::new(&mut messages);
    view.as_widget_mut().update(
        &mut tree,
        &iced::Event::Keyboard(event.clone()),
        iced_core::Layout::new(&node),
        iced_core::mouse::Cursor::Unavailable,
        &renderer,
        &mut iced_core::clipboard::Null,
        &mut shell,
        &iced_core::Rectangle::with_size(iced_core::Size::new(500., 600.)),
    );
    assert!(shell.is_event_captured());
    assert!(crate::product::agent_key_message(shell.event_status(), &event).is_none());
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Msg::Conversations(1, Message::SearchInput(s)) if s == "x"))
    );
}

#[test]
fn conversations_filter_request_preserves_anchor_current_and_present_without_an_iced_overlay() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(17, "/a");
    panel.ensure_state(&a);
    let mut codex = row("codex");
    codex.agent = AgentKind::Codex;
    let _ = panel.update(
        17,
        Message::SessionsRefreshed(17, Ok(vec![row("claude"), codex])),
        Some(&a),
    );
    let _ = panel.update(
        17,
        Message::AgentFilterSelect(Some(AgentKind::Claude)),
        Some(&a),
    );
    let _ = panel.update(17, Message::AgentFilterOpen { at: (12., 34.) }, Some(&a));
    assert!(!panel.state(17).unwrap().agent_picker_open());
    let requests = panel.take_requests();
    let [
        (
            17,
            ConversationsRequest::ShowAgentFilter {
                at,
                current,
                present,
            },
        ),
    ] = requests.as_slice()
    else {
        panic!("one scoped request")
    };
    assert_eq!(*at, (12., 34.));
    assert_eq!(*current, Some(AgentKind::Claude));
    assert!(present.contains(&AgentKind::Claude) && present.contains(&AgentKind::Codex));
    assert!(panel.take_requests().is_empty());
}

#[test]
fn prompt_appears_only_for_an_empty_loaded_list_without_a_marker_and_with_a_live_daemon() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    assert!(panel.import_prompt(1, true).is_none());
    panel.ensure_state(&a);
    assert!(panel.import_prompt(1, true).is_none());
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(vec![])), Some(&a));
    assert!(panel.import_prompt(1, true).is_some());
    assert!(panel.import_prompt(1, false).is_none());
    panel.import_marker.mark(1, 7, 123);
    assert!(panel.import_prompt(1, true).is_none());
}

use super::super::conversations_import::{ImportEvent, ImportState, LOG_DISCLOSURE};
#[derive(Default)]
pub(crate) struct FakeTranscript {
    pub(crate) calls: Mutex<Vec<String>>,
    result: Mutex<Option<Result<u32, ServiceError>>>,
    block: bool,
}
impl TranscriptService for FakeTranscript {
    fn backfill_project_transcripts(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<u32, ServiceError>> {
        self.calls.lock().unwrap().push(cwd);
        if self.block {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let result = self.result.lock().unwrap().take().unwrap_or(Ok(7));
        Box::pin(async move { result })
    }
    fn delete_project_transcripts(&self, _: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        panic!("never delete")
    }
    fn submit_summary_batch(
        &self,
        _: String,
        _: Option<AgentKind>,
        _: Option<String>,
    ) -> BoxFuture<'_, Result<(i64, u32), ServiceError>> {
        panic!("never summarize")
    }
    fn get_summary_batch(
        &self,
        _: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryBatchInfo>, ServiceError>> {
        panic!("never summarize")
    }
}
pub(crate) fn import_panel(
    rt: &tokio::runtime::Runtime,
    path: &std::path::Path,
    fake: Arc<FakeTranscript>,
) -> ConversationsPanel {
    let mut panel = ConversationsPanel::new_with_marker_path(
        Arc::new(FakeConversation::default()),
        Arc::new(FakeSummary::default()),
        fake,
        path.to_path_buf(),
    );
    panel.attach_handle(rt.handle().clone());
    let p = project(1, "/a");
    panel.ensure_state(&p);
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(vec![])), Some(&p));
    panel
}
fn finish_import(panel: &mut ConversationsPanel, rt: &tokio::runtime::Runtime, p: &ProjectInfo) {
    let (pid, event) = rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            panel.import_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap()
    });
    let _ = panel.update_import(pid, event, Some(p), true, false);
}
#[test]
fn start_while_importing_is_ignored() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeTranscript::default());
    let mut panel = import_panel(&rt, &dir.path().join("marker.json"), fake.clone());
    let p = project(1, "/a");
    for _ in 0..10 {
        let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    }
    finish_import(&mut panel, &rt, &p);
    assert_eq!(*fake.calls.lock().unwrap(), vec!["/a"]);
}
#[test]
fn success_marks_saves_and_refreshes_and_completion_survives_nonempty_refresh_and_restart() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
    let p = project(1, "/a");
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    finish_import(&mut panel, &rt, &p);
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Done(7)));
    assert!(super::super::conversations_import::ImportMarker::load(&path).is_marked(1));
    apply(&mut panel, &rt, &p);
    assert_eq!(
        panel.state(1).unwrap().sessions().unwrap()[0].conversation_id,
        "/a"
    );
    let texts = view_texts(&panel, Some(&p), &ready(), 700., false).join("\n");
    assert!(texts.contains("已导入 7 条会话"));
    assert!(texts.contains(LOG_DISCLOSURE));
    assert!(texts.contains(&digger_paths::logs_dir().display().to_string()));
    assert!(!texts.contains("导入此项目的历史会话"));
    panel.close_project(1);
    assert!(panel.imports.is_empty());
    assert!(panel.dirty.is_empty());
    let panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
    assert!(panel.import_prompt(1, true).is_none());
    assert!(
        !view_texts(&panel, Some(&p), &ready(), 700., false)
            .join("\n")
            .contains("导入此项目的历史会话")
    );
}
#[test]
fn failure_does_not_mark_and_allows_retry() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let fake = Arc::new(FakeTranscript::default());
    *fake.result.lock().unwrap() = Some(Err(ServiceError::new(
        ServiceErrorKind::Unavailable,
        "whole import failed",
    )));
    let mut panel = import_panel(&rt, &path, fake.clone());
    let p = project(1, "/a");
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    finish_import(&mut panel, &rt, &p);
    assert!(
        matches!(panel.imports.get(&1),Some(ImportState::Failed(s)) if s.contains("whole import failed"))
    );
    assert!(!path.exists());
    assert!(
        panel
            .notice
            .as_ref()
            .unwrap()
            .contains("whole import failed")
    );
    let texts = view_texts(&panel, Some(&p), &ready(), 700., false).join("\n");
    assert!(texts.contains("重试"));
    assert!(texts.contains(LOG_DISCLOSURE));
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    finish_import(&mut panel, &rt, &p);
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    assert!(path.exists());
}
#[test]
fn zero_imported_still_marks_and_shows_that_nothing_was_found() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let fake = Arc::new(FakeTranscript::default());
    *fake.result.lock().unwrap() = Some(Ok(0));
    let mut panel = import_panel(&rt, &path, fake);
    let p = project(1, "/a");
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    finish_import(&mut panel, &rt, &p);
    assert!(panel.import_marker.is_marked(1));
    let texts = view_texts(&panel, Some(&p), &ready(), 700., false);
    assert!(texts.iter().any(|s| s == "没有找到可导入的会话"));
    assert!(texts.iter().any(|s| s == LOG_DISCLOSURE));
    apply(&mut panel, &rt, &p);
    assert!(!panel.state(1).unwrap().sessions().unwrap().is_empty());
    assert!(
        view_texts(&panel, Some(&p), &ready(), 700., false)
            .iter()
            .any(|s| s == "没有找到可导入的会话")
    );
}
#[test]
fn closing_the_project_during_import_drops_the_result_quietly_even_after_reopen() {
    for reopen in [false, true] {
        let rt = runtime();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marker.json");
        let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
        let p = project(1, "/a");
        let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
        panel.close_project(1);
        if reopen {
            panel.ensure_state(&p);
        }
        finish_import(&mut panel, &rt, &p);
        assert!(panel.imports.is_empty());
        assert!(!path.exists());
        assert!(panel.notice.is_none());
    }
}
#[test]
fn importing_during_exit_or_without_eligible_project_never_calls_the_service() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeTranscript::default());
    let p = project(1, "/a");
    let mut panel = import_panel(&rt, &dir.path().join("marker.json"), fake.clone());
    for (project, ready, closing) in [
        (Some(&p), true, true),
        (Some(&p), false, false),
        (None, true, false),
    ] {
        let _ = panel.update_import(1, ImportEvent::Start, project, ready, closing);
    }
    let _ = panel.update_import(1, ImportEvent::Start, Some(&project(2, "/b")), true, false);
    rt.block_on(async { tokio::task::yield_now().await });
    assert!(fake.calls.lock().unwrap().is_empty());
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    let (pid, event) = rt
        .block_on(panel.import_rx.as_mut().unwrap().recv())
        .unwrap();
    let _ = panel.update_import(pid, event, Some(&p), true, true);
    assert!(!panel.import_marker.is_marked(1));
}
#[test]
fn non_empty_list_never_shows_the_prompt_even_without_a_marker() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeTranscript::default());
    let mut panel = import_panel(&rt, &dir.path().join("marker.json"), fake.clone());
    let p = project(1, "/a");
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&p),
    );
    assert!(panel.import_prompt(1, true).is_none());
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    rt.block_on(async { tokio::task::yield_now().await });
    assert!(fake.calls.lock().unwrap().is_empty());
    assert!(
        !view_texts(&panel, Some(&p), &ready(), 700., false)
            .join("\n")
            .contains("导入此项目的历史会话")
    );
}
#[test]
fn disclosure_text_is_always_shown_with_the_button_including_importing_and_retry() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let mut panel = import_panel(
        &rt,
        &dir.path().join("marker.json"),
        Arc::new(FakeTranscript::default()),
    );
    let p = project(1, "/a");
    for state in [
        ImportState::Idle,
        ImportState::Importing,
        ImportState::Failed("error".into()),
    ] {
        panel.imports.insert(1, state);
        let texts = view_texts(&panel, Some(&p), &ready(), 700., false).join("\n");
        for expected in [
            "~/.claude/",
            "digger.db",
            LOG_DISCLOSURE,
            &digger_paths::logs_dir().display().to_string(),
        ] {
            assert!(texts.contains(expected), "{texts}");
        }
    }
}
#[test]
fn import_runs_off_the_ui_thread_even_when_constructing_the_future_blocks_and_count_is_5000() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeTranscript {
        block: true,
        ..Default::default()
    });
    *fake.result.lock().unwrap() = Some(Ok(5000));
    let mut panel = import_panel(&rt, &dir.path().join("marker.json"), fake);
    let p = project(1, "/a");
    let start = std::time::Instant::now();
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    assert!(start.elapsed() < std::time::Duration::from_millis(50));
    finish_import(&mut panel, &rt, &p);
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Done(5000)));
}
#[test]
fn marker_save_failure_is_visible_and_preserves_the_actual_completed_count() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("directory");
    std::fs::create_dir(&path).unwrap();
    let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
    let p = project(1, "/a");
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    finish_import(&mut panel, &rt, &p);
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Done(7)));
    assert!(panel.notice.as_ref().unwrap().contains("标记"));
    assert!(
        view_texts(&panel, Some(&p), &ready(), 700., false)
            .join("\n")
            .contains("已导入 7 条会话")
    );
}

#[test]
fn import_without_home_uses_injected_marker_and_actual_product_log_path_in_isolated_child() {
    const CHILD: &str = "S7B_IMPORT_HOME_UNSET_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert!(std::env::var_os("HOME").is_none());
        let rt = runtime();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marker.json");
        let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
        let p = project(1, "/a");
        let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
        finish_import(&mut panel, &rt, &p);
        assert!(path.exists());
        assert!(
            view_texts(&panel, Some(&p), &ready(), 700., false)
                .join("\n")
                .contains(&digger_paths::logs_dir().display().to_string())
        );
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let output=std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","adapters::conversations::tests::import_without_home_uses_injected_marker_and_actual_product_log_path_in_isolated_child","--nocapture"])
        .env_remove("HOME").env(CHILD,"1").env("DIGGER_DATA_DIR",data.path()).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn import_prompt_rejects_missing_project_state() {
    let rt = runtime();
    let (panel, _, _) = panel(&rt);
    assert!(panel.import_prompt(1, true).is_none());
}
#[test]
fn import_prompt_rejects_list_not_yet_loaded() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    panel.ensure_state(&project(1, "/a"));
    assert!(panel.import_prompt(1, true).is_none());
}
#[test]
fn import_prompt_rejects_marked_empty_project_without_affecting_another_project() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let mut panel = import_panel(
        &rt,
        &dir.path().join("marker.json"),
        Arc::new(FakeTranscript::default()),
    );
    let b = project(2, "/a/b");
    panel.ensure_state(&b);
    let _ = panel.update(2, Message::SessionsRefreshed(2, Ok(vec![])), Some(&b));
    panel.import_marker.mark(1, 7, 123);
    assert!(panel.import_prompt(1, true).is_none());
    assert!(panel.import_prompt(2, true).is_some());
}
#[test]
fn import_prompt_rejects_unavailable_daemon_and_actual_view_explains_shared_retry() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let panel = import_panel(
        &rt,
        &dir.path().join("marker.json"),
        Arc::new(FakeTranscript::default()),
    );
    let p = project(1, "/a");
    assert!(panel.import_prompt(1, false).is_none());
    let unavailable = AgentShell {
        status: Status::Unavailable("permission denied".into()),
    };
    let texts = view_texts(&panel, Some(&p), &unavailable, 700., false).join("\n");
    assert!(texts.contains("permission denied"));
    assert!(texts.contains("重试"));
    assert!(!texts.contains("导入此项目的历史会话"));
}
#[test]
fn activation_refresh_turn_ended_and_passive_visibility_never_import_history() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeTranscript::default());
    let mut panel = import_panel(&rt, &dir.path().join("marker.json"), fake.clone());
    let p = project(1, "/a");
    let _ = panel.activate(&p);
    apply(&mut panel, &rt, &p);
    let _ = panel.on_turn_ended(1, Some(&p), true);
    apply(&mut panel, &rt, &p);
    let _ = view_texts(&panel, Some(&p), &ready(), 700., false);
    assert!(fake.calls.lock().unwrap().is_empty());
}
#[test]
fn import_marker_is_loaded_once_and_never_substitutes_for_list_truth() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
    std::fs::write(
        &path,
        br#"{"schema_version":1,"projects":{"1":{"at_ms":1,"count":7}}}"#,
    )
    .unwrap();
    assert!(panel.import_prompt(1, true).is_some());
    let p = project(1, "/a");
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("live")])),
        Some(&p),
    );
    assert!(panel.import_prompt(1, true).is_none());
    assert!(
        view_texts(&panel, Some(&p), &ready(), 700., false)
            .iter()
            .any(|s| s == "live")
    );
}

#[test]
fn actual_import_and_retry_buttons_dispatch_start_and_importing_is_disabled() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let mut panel = import_panel(
        &rt,
        &dir.path().join("marker.json"),
        Arc::new(FakeTranscript::default()),
    );
    let p = project(1, "/a");
    let shared = ready();
    for (state, label, enabled) in [
        (ImportState::Idle, "导入此项目的历史会话", true),
        (ImportState::Importing, "正在导入…", false),
        (ImportState::Failed("retry me".into()), "重试", true),
    ] {
        panel.imports.insert(1, state);
        let mut view = panel.view(Some(&p), &area(700., false), &shared);
        let texts = layout(&mut view, 700.).3;
        let point = texts
            .0
            .iter()
            .find(|(text, _)| text == label)
            .unwrap()
            .1
            .center();
        let messages = click(&mut view, point, 700.);
        assert_eq!(
            messages
                .iter()
                .any(|message| matches!(message, Msg::ConversationsImport(1, ImportEvent::Start))),
            enabled
        );
    }
}

#[test]
fn reopened_project_new_import_rejects_old_owner_completion_while_importing() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let fake = Arc::new(FakeTranscript::default());
    let mut panel = import_panel(&rt, &path, fake.clone());
    let p = project(1, "/a");
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    let (pid, old) = rt
        .block_on(panel.import_rx.as_mut().unwrap().recv())
        .unwrap();
    panel.close_project(1);
    panel.ensure_state(&p);
    let _ = panel.update(1, Message::SessionsRefreshed(1, Ok(vec![])), Some(&p));
    let _ = panel.update_import(1, ImportEvent::Start, Some(&p), true, false);
    let _ = panel.update_import(pid, old, Some(&p), true, false);
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Importing));
    assert!(!path.exists());
    finish_import(&mut panel, &rt, &p);
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Done(7)));
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
}
#[test]
fn import_completion_cannot_cross_projects_or_apply_twice() {
    let rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("marker.json");
    let mut panel = import_panel(&rt, &path, Arc::new(FakeTranscript::default()));
    let a = project(1, "/a");
    let b = project(2, "/a/b");
    panel.ensure_state(&b);
    let _ = panel.update(2, Message::SessionsRefreshed(2, Ok(vec![])), Some(&b));
    let _ = panel.update_import(1, ImportEvent::Start, Some(&a), true, false);
    let (_, event) = rt
        .block_on(panel.import_rx.as_mut().unwrap().recv())
        .unwrap();
    let _ = panel.update_import(2, event.clone(), Some(&b), true, false);
    assert!(!panel.import_marker.is_marked(2));
    let _ = panel.update_import(1, event.clone(), Some(&a), true, false);
    let saved = std::fs::read(&path).unwrap();
    let _ = panel.update_import(1, event, Some(&a), true, false);
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert!(panel.import_prompt(2, true).is_some());
    assert_eq!(panel.imports.get(&1), Some(&ImportState::Done(7)));
}

#[test]
fn no_summary_button_in_the_review_header() {
    let rt = runtime();
    let (mut panel, fake, summary) = panel(&rt);
    let a = project(1, "/a");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    assert_eq!(*summary.0.lock().unwrap(), 0);
    let texts = view_texts(&panel, Some(&a), &ready(), 600., true);
    assert!(texts.iter().any(|s| s == "对话"), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|s| s.contains("生成总结") || s.contains("Generate Summary"))
    );
}

fn turn(index: i64, content: &str) -> TurnRecord {
    serde_json::from_value(serde_json::json!({"turn_index":index,"role":"human","content":content,"thinking":false,"ts":null,"tokens_in":0,"tokens_out":0,"tokens_cache_read":0,"tokens_cache_write":0})).unwrap()
}
fn start_review(
    panel: &mut ConversationsPanel,
    fake: &FakeConversation,
    rt: &tokio::runtime::Runtime,
    project: &ProjectInfo,
    cid: &str,
) {
    panel.ensure_state(project);
    let _ = panel.update(
        project.id,
        Message::SessionsRefreshed(project.id, Ok(vec![row(cid)])),
        Some(project),
    );
    fake.turn_responses
        .lock()
        .unwrap()
        .push_back(Ok(vec![turn(0, cid)]));
    let _ = panel.update(
        project.id,
        Message::SessionOpen(cid.into(), AgentKind::Claude),
        Some(project),
    );
    apply(panel, rt, project);
}
fn sync_review(panel: &mut ConversationsPanel, pid: Option<i64>) {
    let _ = panel.sync_native(
        iced::window::Id::unique(),
        true,
        bytehost_core::LogicalRect {
            x: 0.,
            y: 38.,
            width: 400.,
            height: 400.,
        },
        pid,
    );
}
fn load_native(panel: &mut ConversationsPanel, project: &ProjectInfo) {
    let (generation, nonce, page_url) = panel.native.document_identity().unwrap();
    let _ = panel.update_native(
        super::super::review_native::Event::Loaded {
            generation,
            nonce,
            page_url,
        },
        Some(project),
    );
}
#[test]
fn clicking_a_row_opens_the_detail_and_shows_loading_until_loaded() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&a),
    );
    fake.turn_responses
        .lock()
        .unwrap()
        .push_back(Ok(vec![turn(0, "hello")]));
    let shared = ready();
    let mut view = panel.view(Some(&a), &area(600., false), &shared);
    let at = layout(&mut view, 600.)
        .3
        .0
        .into_iter()
        .find(|(s, _)| s == "one")
        .unwrap()
        .1
        .center();
    let messages = click(&mut view, at, 600.);
    drop(view);
    for message in messages {
        if let Msg::Conversations(pid, msg) = message {
            let _ = panel.update(pid, msg, Some(&a));
        }
    }
    apply(&mut panel, &rt, &a);
    assert_eq!(*fake.turns.lock().unwrap(), vec![("one".into(), -1, 200)]);
    assert!(
        view_texts(&panel, Some(&a), &ready(), 600., false)
            .iter()
            .any(|s| s == "加载中…")
    );
    let _ = panel.update(1, Message::ReviewTraceLoaded, Some(&a));
    assert_eq!(
        panel
            .state(1)
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .loaded_nonce,
        None
    );
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    assert!(
        !view_texts(&panel, Some(&a), &ready(), 600., false)
            .iter()
            .any(|s| s == "加载中…")
    );
}
#[test]
fn two_quick_clicks_show_only_the_second_rows_detail_even_if_the_first_loaded_event_arrives_last() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    let old = panel.native.document_identity().unwrap();
    start_review(&mut panel, &fake, &rt, &a, "two");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    let _ = panel.update_native(
        super::super::review_native::Event::Loaded {
            generation: old.0,
            nonce: old.1,
            page_url: old.2,
        },
        Some(&a),
    );
    let review = panel.state(1).unwrap().review.as_ref().unwrap();
    assert_eq!(
        review.source,
        dozer_panel_conversations::snapshot::ReviewSource::Conversation("two".into())
    );
    assert_eq!(review.loaded_nonce, Some(review.nonce));
    assert!(
        panel
            .state(1)
            .unwrap()
            .review_snapshot()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("two")
    );
}
#[test]
fn load_more_appends_and_renavigates_with_a_new_nonce() {
    for initial_nonce in [0, u64::MAX - 1] {
        let rt = runtime();
        let (mut panel, fake, _) = panel(&rt);
        let a = project(1, "/a");
        panel.ensure_state(&a).review_nonce = initial_nonce;
        start_review(&mut panel, &fake, &rt, &a, "one");
        sync_review(&mut panel, Some(1));
        load_native(&mut panel, &a);
        let old = panel.native.document_identity().unwrap();
        assert_eq!(old.1, initial_nonce.wrapping_add(1));
        fake.turn_responses
            .lock()
            .unwrap()
            .push_back(Ok(vec![turn(1, "second")]));
        // This verifies the iced button's real hit target/message. Native coverage
        // and actual click-through behavior still require desktop GUI verification.
        let shared = ready();
        let mut view = panel.view(Some(&a), &area(600., true), &shared);
        let at = layout(&mut view, 600.)
            .3
            .0
            .into_iter()
            .find(|(label, _)| label == "加载更多…")
            .unwrap()
            .1
            .center();
        let messages = click(&mut view, at, 600.);
        drop(view);
        assert!(messages.iter().any(|message| matches!(message,
        Msg::Conversations(1, Message::DetailLoadMore(cid, 1)) if cid == "one")));
        for message in messages {
            if let Msg::Conversations(pid, message) = message {
                let _ = panel.update(pid, message, Some(&a));
            }
        }
        apply(&mut panel, &rt, &a);
        sync_review(&mut panel, Some(1));
        let next = panel.native.document_identity().unwrap();
        assert_eq!(old.0, next.0);
        assert_ne!(old.1, next.1);
        assert_eq!(next.1, initial_nonce.wrapping_add(2));
        assert_ne!(old.2, next.2);
        assert_eq!(
            panel
                .state(1)
                .unwrap()
                .review
                .as_ref()
                .unwrap()
                .entries
                .len(),
            2
        );
        assert_eq!(fake.turns.lock().unwrap()[1], ("one".into(), 1, 200));
        assert!(
            panel
                .desired_for_review(
                    true,
                    bytehost_core::LogicalRect {
                        x: 0.,
                        y: 0.,
                        width: 400.,
                        height: 400.
                    },
                    Some(1)
                )
                .spec
                .unwrap()
                .park_offscreen
        );
        // An old MAX (or ordinary previous nonce) must not reveal the new page.
        let _ = panel.update_native(
            super::super::review_native::Event::Loaded {
                generation: old.0,
                nonce: old.1,
                page_url: old.2,
            },
            Some(&a),
        );
        assert!(!panel.native.is_ready());
        assert_ne!(
            panel
                .state(1)
                .unwrap()
                .review
                .as_ref()
                .unwrap()
                .loaded_nonce,
            Some(next.1)
        );
        load_native(&mut panel, &a);
        assert!(panel.native.is_ready());
        let review = panel.state(1).unwrap().review.as_ref().unwrap();
        assert_eq!(review.loaded_nonce, Some(next.1));
        assert_eq!(
            dozer_panel_conversations::snapshot::review_pane_state(Some(review)),
            dozer_panel_conversations::snapshot::ReviewPaneState::Ready
        );
    }
}
#[test]
fn load_more_button_is_visible_when_entries_exist() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    assert!(
        !panel
            .desired_for_review(
                true,
                bytehost_core::LogicalRect {
                    x: 0.,
                    y: 0.,
                    width: 1.,
                    height: 1.
                },
                Some(1)
            )
            .visible
    );
    assert!(
        view_texts(&panel, Some(&a), &ready(), 600., true)
            .iter()
            .any(|s| s == "加载更多…")
    );
}
#[test]
fn empty_entries_or_error_show_the_fallback_text_and_no_webview() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("one")])),
        Some(&a),
    );
    let _ = panel.update(
        1,
        Message::SessionOpen("one".into(), AgentKind::Claude),
        Some(&a),
    );
    apply(&mut panel, &rt, &a);
    assert!(
        view_texts(&panel, Some(&a), &ready(), 600., true)
            .iter()
            .any(|s| s == "暂无对话")
    );
    let _ = panel.update(
        1,
        Message::ReviewLoaded {
            source: dozer_panel_conversations::snapshot::ReviewSource::Conversation("one".into()),
            append: false,
            result: Err("broken details".into()),
        },
        Some(&a),
    );
    assert!(
        view_texts(&panel, Some(&a), &ready(), 600., true)
            .iter()
            .any(|s| s.contains("broken details"))
    );
    assert!(
        panel
            .desired_for_review(
                true,
                bytehost_core::LogicalRect {
                    x: 0.,
                    y: 0.,
                    width: 400.,
                    height: 400.
                },
                Some(1)
            )
            .spec
            .is_none()
    );
}
#[test]
fn closing_the_project_that_owns_the_detail_destroys_the_webview() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    panel.close_project(1);
    sync_review(&mut panel, Some(1));
    assert!(panel.native.document_identity().is_none());
    assert!(panel.state(1).is_none());
}
#[test]
fn switching_to_a_project_without_a_detail_destroys_and_shows_the_empty_state() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    let b = project(2, "/b");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    panel.ensure_state(&b);
    sync_review(&mut panel, Some(2));
    assert!(panel.native.document_identity().is_none());
    assert!(
        view_texts(&panel, Some(&b), &ready(), 600., true)
            .iter()
            .any(|s| s.contains("暂无审阅内容"))
    );
}
#[test]
fn switching_back_shows_the_previous_projects_detail_again() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    let b = project(2, "/b");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    let first = panel.native.document_identity().unwrap();
    panel.ensure_state(&b);
    sync_review(&mut panel, Some(2));
    sync_review(&mut panel, Some(1));
    let next = panel.native.document_identity().unwrap();
    assert_ne!(first.0, next.0);
    assert_eq!(first.2, next.2);
    load_native(&mut panel, &a);
    assert!(
        view_texts(&panel, Some(&a), &ready(), 600., true)
            .iter()
            .any(|s| s == "加载更多…")
    );
    let unavailable = AgentShell {
        status: Status::Unavailable("daemon down".into()),
    };
    assert!(
        view_texts(&panel, Some(&a), &unavailable, 600., false)
            .iter()
            .any(|s| s == "加载更多…")
    );
    assert!(
        view_texts(&panel, Some(&a), &unavailable, 600., false)
            .iter()
            .any(|s| s.contains("daemon down"))
    );
}
#[test]
fn collapsed_notice_is_dismissible_and_outside_the_native_review() {
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let error = format!(
        "无法读取会话文件 /private/tmp/history/{}: connection lost",
        "nested-path/".repeat(50)
    );
    panel.notice.push(error.clone());
    let shared = ready();
    let mut view = panel.view(Some(&a), &area(600., true), &shared);
    let (_, node, _, texts) = layout(&mut view, 600.);
    let texts = texts.0;
    let close = texts.iter().find(|(s, _)| s == "×").unwrap().1;
    assert!(texts.iter().any(|(s, _)| s == &error));
    assert!(
        close.y >= 0. && close.y + close.height <= panel.review_notice_height(true),
        "dismiss control {close:?} must be in the initial notice viewport"
    );
    let bounds = super::super::review_geometry::review_bounds(
        bytehost_chrome::Rect {
            x: 0.,
            y: panel.review_notice_height(true),
            w: 600.,
            h: 600. - panel.review_notice_height(true),
        },
        byteui::theme::icon_size::scale(),
    );
    assert!(close.y + close.height <= bounds.y, "{close:?} {bounds:?}");
    let header = texts.iter().find(|(s, _)| s == "对话").unwrap().1;
    assert!(
        header.y + header.height <= bounds.y,
        "header {header:?} covered by native {bounds:?}"
    );
    fn rectangles(layout: iced_core::Layout<'_>, out: &mut Vec<iced_core::Rectangle>) {
        out.push(layout.bounds());
        for child in layout.children() {
            rectangles(child, out);
        }
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
            assert!(
                rect.y >= 0. && rect.y + rect.height <= panel.review_notice_height(true),
                "entire dismiss hit bounds {rect:?} must be inside initial notice viewport at scale {}",
                byteui::theme::icon_size::scale()
            );
        }
    }
    assert!(found, "notice dismiss button must be hittable");
    assert!(
        click(&mut view, close.center(), 600.)
            .iter()
            .any(|m| matches!(m, Msg::ConversationsNoticeDismiss))
    );
}

#[test]
fn actual_widget_header_and_collapse_control_are_above_native_review() {
    let display = ConversationsDisplay::new(1);
    let mut head = display.panel_head(
        byteui::interaction::icons::IconKind::BotMessageSquare,
        "对话".into(),
        None,
    );
    let (_, head_node, _, _) = layout(&mut head, 600.);
    let divider = head_node.children().last().unwrap().bounds();
    let style = super::super::conversations_display::review_pane_style();
    assert!(divider.height > 0.);
    assert!(
        style.padding.top + divider.y + divider.height
            <= byteui::theme::geometry::preview_chrome_top_px(),
        "actual divider {divider:?} plus pane top {} exceeds native chrome {} at scale {}",
        style.padding.top,
        byteui::theme::geometry::preview_chrome_top_px(),
        byteui::theme::icon_size::scale()
    );
    assert!(
        head_node.size().height > byteui::theme::geometry::tab_button_size(),
        "header must contain its existing divider"
    );
    let rt = runtime();
    let (mut panel, _, _) = panel(&rt);
    let a = project(1, "/a");
    panel.ensure_state(&a);
    let shared = ready();
    for collapsed in [false, true] {
        let mut view = panel.view(Some(&a), &area(600., collapsed), &shared);
        let (_, node, _, texts) = layout(&mut view, 600.);
        let bounds = super::super::review_geometry::review_bounds(
            if collapsed {
                bytehost_chrome::Rect::new(0., 0., 600., 600.)
            } else {
                let block = node.children().last().unwrap().bounds();
                bytehost_chrome::Rect::new(block.x, block.y, block.width, block.height)
            },
            byteui::theme::icon_size::scale(),
        );
        let header = texts.0.iter().find(|(s, _)| s == "对话").unwrap().1;
        assert!(
            header.y + header.height <= bounds.y,
            "{header:?} {bounds:?}"
        );
        fn rectangles(layout: iced_core::Layout<'_>, out: &mut Vec<iced_core::Rectangle>) {
            out.push(layout.bounds());
            for child in layout.children() {
                rectangles(child, out);
            }
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
                assert!(
                    rect.y + rect.height <= bounds.y,
                    "actual pane divider {rect:?} covered by native {bounds:?} at scale {}",
                    byteui::theme::icon_size::scale()
                );
            }
        }
        assert!(found_divider, "actual review pane divider must be laid out");
        let mut found = false;
        for rect in all {
            if click(&mut view, rect.center(), 600.)
                .iter()
                .any(|msg| matches!(msg, Msg::Shell(bytehost_chrome::ShellMsg::TogglePane(_))))
            {
                found = true;
                assert!(
                    rect.y + rect.height <= bounds.y,
                    "collapse button {rect:?} covered by native {bounds:?}"
                );
            }
        }
        assert!(found, "real collapse button must be hittable");
    }
}

#[test]
fn native_failure_keeps_a_readable_fallback_and_existing_collapsed_notice() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/a");
    start_review(&mut panel, &fake, &rt, &a, "one");
    sync_review(&mut panel, Some(1));
    let (generation, _, _) = panel.native.document_identity().unwrap();
    let _ = panel.update_native(
        super::super::review_native::Event::Built(generation, Err("child creation failed".into())),
        Some(&a),
    );
    let texts = view_texts(&panel, Some(&a), &ready(), 600., true);
    assert!(
        texts.iter().any(|s| s.contains("child creation failed")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|s| s == "×"));
    assert!(!texts.iter().any(|s| s == "加载中…"));
    assert!(
        panel
            .state(1)
            .unwrap()
            .review_snapshot()
            .lock()
            .unwrap()
            .is_some()
    );
}

#[test]
fn review_widgets_preserve_header_and_notice_clearance_at_supported_scales() {
    const CHILD: &str = "S7C_REVIEW_SCALE_WIDGET_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let expected: f32 = std::env::var(CHILD).unwrap().parse().unwrap();
        assert_eq!(byteui::theme::icon_size::scale(), expected);
        actual_widget_header_and_collapse_control_are_above_native_review();
        collapsed_notice_is_dismissible_and_outside_the_native_review();
        return;
    }
    // The production widgets read byteui globals. Isolate each scale in a child
    // running only this test, without mutating globals in the shared test runner.
    for scale in ["0.5", "1", "2"] {
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","adapters::conversations::tests::review_widgets_preserve_header_and_notice_clearance_at_supported_scales","--test-threads=1","--nocapture"])
            .env(CHILD,scale).env("BYTEUI_ICON_SCALE",scale).output().unwrap();
        assert!(
            output.status.success(),
            "scale {scale}: stdout {} stderr {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn twenty_distinct_rows_with_project_filter_resize_collapse_and_close_reject_stale_documents() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/synthetic-a");
    let b = project(2, "/synthetic-b");
    let mut stale = Vec::new();
    for index in 0..20 {
        let p = if index % 2 == 0 { &a } else { &b };
        start_review(&mut panel, &fake, &rt, p, &format!("row-{index}"));
        sync_review(&mut panel, Some(p.id));
        stale.push(panel.native.document_identity().unwrap());
        let _ = panel.update(
            p.id,
            Message::AgentFilterSelect(Some(AgentKind::Claude)),
            Some(p),
        );
        // These construct and lay out the actual shared widget at both pane widths.
        let _ = view_texts(
            &panel,
            Some(p),
            &ready(),
            400. + index as f32 * 10.,
            index % 3 == 0,
        );
        if index % 4 == 0 {
            let _ = panel.sync_native(
                iced::window::Id::unique(),
                false,
                bytehost_core::LogicalRect {
                    x: 0.,
                    y: 0.,
                    width: 0.,
                    height: 0.,
                },
                Some(p.id),
            );
            assert!(panel.native.document_identity().is_none());
        }
        panel.close_project(p.id);
        sync_review(&mut panel, Some(p.id));
        assert!(panel.native.document_identity().is_none());
    }
    start_review(&mut panel, &fake, &rt, &a, "current");
    sync_review(&mut panel, Some(a.id));
    for (generation, nonce, page_url) in stale {
        let _ = panel.update_native(
            super::super::review_native::Event::Loaded {
                generation,
                nonce,
                page_url,
            },
            Some(&a),
        );
    }
    assert!(!panel.native.is_ready());
    load_native(&mut panel, &a);
    assert!(panel.native.is_ready());
}

#[test]
fn daemon_fault_keeps_cached_snapshot_reports_next_detail_failure_and_recovers() {
    let rt = runtime();
    let (mut panel, fake, _) = panel(&rt);
    let a = project(1, "/synthetic-a");
    start_review(&mut panel, &fake, &rt, &a, "cached");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    let cached = panel
        .state(1)
        .unwrap()
        .review_snapshot()
        .lock()
        .unwrap()
        .clone();
    let unavailable = AgentShell {
        status: Status::Unavailable("synthetic daemon death".into()),
    };
    assert!(
        view_texts(&panel, Some(&a), &unavailable, 600., false)
            .iter()
            .any(|s| s.contains("synthetic daemon death"))
    );
    assert_eq!(
        panel
            .state(1)
            .unwrap()
            .review_snapshot()
            .lock()
            .unwrap()
            .clone(),
        cached
    );
    let _ = panel.update(
        1,
        Message::SessionsRefreshed(1, Ok(vec![row("cached"), row("next")])),
        Some(&a),
    );
    fake.turn_responses
        .lock()
        .unwrap()
        .push_back(Err(ServiceError::new(
            ServiceErrorKind::Unavailable,
            "synthetic connection refused",
        )));
    let _ = panel.update(
        1,
        Message::SessionOpen("next".into(), AgentKind::Claude),
        Some(&a),
    );
    apply(&mut panel, &rt, &a);
    assert!(
        panel
            .state(1)
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("synthetic connection refused")
    );
    start_review(&mut panel, &fake, &rt, &a, "next");
    sync_review(&mut panel, Some(1));
    load_native(&mut panel, &a);
    assert!(
        panel
            .state(1)
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .error
            .is_none()
    );
    assert!(panel.native.is_ready());
}
