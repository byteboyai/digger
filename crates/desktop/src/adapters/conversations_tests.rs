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
        Box::pin(async { Ok(vec![]) })
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
    let mut panel = ConversationsPanel::new(
        fake.clone(),
        summary.clone(),
        Arc::new(crate::adapters::project::UnavailableService::new(
            "test transcript",
        )),
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
    assert!(texts.iter().any(|s| s == "审阅详情即将可用"));
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
    assert!(texts.iter().any(|s| s == "审阅详情即将可用"));
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
fn conversations_pair_uses_the_real_list_width_and_collapse_leaves_only_the_placeholder() {
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
    assert_eq!(
        texts.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        vec!["审阅详情即将可用"]
    );
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
    assert_eq!(detail.padding, iced_core::Padding::from(8.));
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
