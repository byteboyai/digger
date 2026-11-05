// Product/window-action tests for the group-chat page: the fake window id never
// executes a WKWebView closure, so these assert the planned effects.
use crate::adapters::group_chat::tests::{Fake, project, services};
use crate::adapters::group_chat_native::Event as ChatEvent;
use dozer_model::{GroupAuthor, GroupInfo, GroupMessageInfo, GroupMessageStatus};
use dozer_panel_group_chat::{GroupChatWebviewEvent as Page, Message as G};

fn host(shell: &mut ProductShell, msg: Msg) -> Task<Msg> {
    update_host(shell, HostMessage::Product(msg))
}
fn show(shell: &mut ProductShell, id: PanelId) {
    let _ = host(shell, Msg::Shell(ShellMsg::ShowPanel(id)));
}
fn group(id: i64) -> GroupInfo {
    GroupInfo {
        id,
        project_id: 1,
        topic: format!("g{id}"),
        created_ms: 0,
        members: vec![],
    }
}
fn agent_message(id: i64, status: GroupMessageStatus) -> GroupMessageInfo {
    GroupMessageInfo {
        id,
        group_id: 1,
        seq: id,
        rev: id,
        author: GroupAuthor::System,
        text: "hi".into(),
        mentions: vec![],
        status: Some(status),
        duration_ms: None,
        created_ms: 0,
        todo_id: None,
    }
}
fn chat_shell(tag: &str) -> (ProductShell, tokio::runtime::Runtime, Arc<Fake>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let fake = Arc::new(Fake::default());
    let mut shell = fresh(tag);
    shell.group_chat = crate::adapters::group_chat::GroupChatPanel::new(services(fake.clone()));
    shell.group_chat.attach_handle(rt.handle().clone());
    shell.agent.attach_handle(rt.handle().clone());
    shell.agent.shell.status = crate::adapters::agent::Status::Ready;
    // Seeding opens (and activates) the tab: project 1 ends up current, 2 stays a real
    // second tab to switch to.
    shell.projects.seed_for_test(project(2));
    shell.projects.seed_for_test(project(1));
    show(&mut shell, shell::group_chat_id());
    shell.window = Some(window::Id::unique());
    let _ = host(&mut shell, Msg::Shell(ShellMsg::Noop));
    assert!(shell.group_chat.native.generation().is_some());
    (shell, rt, fake)
}
/// The child was built and the page said it is ready: the first frame it may be shown.
fn ready(shell: &mut ProductShell) {
    let generation = shell.group_chat.native.generation().unwrap();
    let _ = host(shell, Msg::GroupChatNative(ChatEvent::Built(generation, Ok(()))));
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(
        shell,
        Msg::GroupChatNative(ChatEvent::Page(generation, Page::Ready)),
    );
    assert!(shell.group_chat.native.scheduled_actions.contains(&"Show".into()));
}
fn actions(shell: &ProductShell) -> Vec<String> {
    shell.group_chat.native.scheduled_actions.clone()
}
fn expected_rect(shell: &ProductShell) -> bytehost_core::LogicalRect {
    let side = shell.shell.side_of(&shell::group_chat_id()).unwrap();
    let content = shell.shell.rects(&metrics_from_theme()).content(side).unwrap();
    let collapsed = shell.pane_collapsed(&shell::group_chat_id());
    logical(group_chat_native_rect(&shell.shell, content, collapsed))
}

#[test]
fn the_first_frame_is_parked_and_only_a_ready_page_is_shown() {
    let (mut shell, _rt, _) = chat_shell("chat-first-frame");
    let first = actions(&shell);
    assert!(first.contains(&"Create".into()) && first.contains(&"Park".into()), "{first:?}");
    assert!(!first.contains(&"Show".into()), "{first:?}");
    ready(&mut shell);
}

#[test]
fn opening_the_new_group_dialog_parks_the_page_and_closing_it_restores_it() {
    let (mut shell, _rt, _) = chat_shell("chat-dialog");
    ready(&mut shell);
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::GroupChat(G::NewGroupDialogToggle(1)));
    let hidden = actions(&shell);
    assert!(hidden.contains(&"Park".into()), "{hidden:?}");
    assert!(!hidden.contains(&"Destroy".into()), "alive, only hidden: {hidden:?}");
    assert!(!shell.group_chat.native.scheduled_target.as_ref().unwrap().1);
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::GroupChat(G::NewGroupDialogCancel(1)));
    let back = actions(&shell);
    assert!(back.contains(&"SetBounds".into()) && back.contains(&"Show".into()), "{back:?}");
    assert!(shell.group_chat.native.scheduled_target.as_ref().unwrap().1);
}

#[test]
fn the_agent_picker_and_close_confirm_hide_it_too() {
    let (mut shell, rt, _) = chat_shell("chat-agent-overlays");
    let _ = host(&mut shell, Msg::Shell(ShellMsg::PanelPress { panel: shell::agent_id() }));
    let _ = host(&mut shell, Msg::Shell(ShellMsg::RailDragMove { side: Side::Left, index: 0 }));
    let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    show(&mut shell, shell::group_chat_id());
    ready(&mut shell);
    let mut state = dozer_panel_agent::state::AgentPanelState::new();
    {
        let _guard = rt.enter();
        crate::adapters::agent::tests::insert_tab(&mut state, "running", dozer_model::AgentKind::Claude);
    }
    state.tabs[0].agent_state = dozer_model::AgentState::Running;
    shell.agent.states.insert(1, state);
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::Agent(1, dozer_panel_agent::update::Message::PickerToggle));
    assert!(shell.agent.picker.is_some());
    assert!(shell.chat_overlay_open());
    assert!(actions(&shell).contains(&"Park".into()));
    let _ = host(&mut shell, Msg::AgentPickerClose);
    assert!(!shell.chat_overlay_open());
    assert!(actions(&shell).contains(&"Show".into()));
    let _ = host(&mut shell, Msg::Agent(1, dozer_panel_agent::update::Message::CloseTab(0)));
    assert!(shell.agent.states[&1].pending_close_tab.is_some());
    assert!(shell.chat_overlay_open());
    assert!(!shell.group_chat.native.scheduled_target.as_ref().unwrap().1);
}

#[test]
fn switching_to_another_right_panel_destroys_it_and_coming_back_is_parked_again() {
    let (mut shell, _rt, _) = chat_shell("chat-panel-switch");
    ready(&mut shell);
    shell.group_chat.native.scheduled_actions.clear();
    show(&mut shell, shell::usage_id());
    assert!(actions(&shell).contains(&"Destroy".into()));
    assert!(shell.group_chat.native.generation().is_none());
    shell.group_chat.native.scheduled_actions.clear();
    show(&mut shell, shell::group_chat_id());
    let again = actions(&shell);
    assert!(again.contains(&"Create".into()) && again.contains(&"Park".into()), "{again:?}");
    assert!(!again.contains(&"Show".into()), "a new child never inherits readiness: {again:?}");
}

#[test]
fn collapsing_the_right_zone_or_maximizing_the_left_one_destroys_it() {
    for maximize in [false, true] {
        let (mut shell, _rt, _) = chat_shell(if maximize { "chat-max" } else { "chat-collapse" });
        ready(&mut shell);
        shell.group_chat.native.scheduled_actions.clear();
        let message = if maximize {
            ShellMsg::Maximize(Side::Left)
        } else {
            ShellMsg::PanelPress { panel: shell::group_chat_id() }
        };
        let _ = host(&mut shell, Msg::Shell(message));
        assert!(actions(&shell).contains(&"Destroy".into()), "maximize={maximize}");
    }
}

#[test]
fn window_closing_and_minimizing_destroy_it() {
    let (mut shell, _rt, _) = chat_shell("chat-close");
    ready(&mut shell);
    shell.group_chat.native.scheduled_actions.clear();
    let main = shell.window_id().expect("main window");
    let _ = host(&mut shell, Msg::CloseRequested(main));
    assert!(shell.closing);
    assert!(actions(&shell).contains(&"Destroy".into()));
    let (mut shell, _rt, _) = chat_shell("chat-minimize");
    ready(&mut shell);
    shell.group_chat.native.scheduled_actions.clear();
    shell.window_minimized = true;
    let _ = host(&mut shell, Msg::Shell(ShellMsg::Noop));
    assert!(actions(&shell).contains(&"Destroy".into()));
}

#[test]
fn dragging_the_divider_moves_the_webview_with_the_block() {
    for divider in [Divider::Zones, Divider::Pair(Side::Right)] {
        let (mut shell, _rt, _) = chat_shell(&format!("chat-divider-{divider:?}"));
        ready(&mut shell);
        let pair = shell::pair_rects(&shell.shell, Side::Right).unwrap();
        let _ = host(&mut shell, Msg::Shell(ShellMsg::DividerPress(divider)));
        let mut rects = vec![];
        for i in 0..20 {
            let x = match divider {
                Divider::Zones => 410. + i as f32 * 6.,
                Divider::Pair(_) => pair.second.x + pair.second.w * 0.55 + i as f32 * 3.,
            };
            let _ = host(&mut shell, Msg::Shell(ShellMsg::CursorMoved { x, y: 300. }));
            let rect = shell.group_chat.native.scheduled_target.as_ref().unwrap().0;
            assert_eq!(rect, expected_rect(&shell));
            assert!(!rects.contains(&rect), "twenty distinct positions for {divider:?}");
            rects.push(rect);
        }
        let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    }
}

#[test]
fn the_real_default_shell_matches_dozers_chat_geometry() {
    // Dozer c76302fc group_chat_content_pane_bounds_for, 1200x800, default shell:
    // list visible (693,54,291.2,712); collapsed (693,54,456,712).
    let d = shell::defaults();
    let mut model = ShellModel::new(d.rails, d.selected, d.left_width, (1200., 800.)).unwrap();
    let _ = shell::update(&mut model, ShellMsg::PanelPress { panel: shell::group_chat_id() });
    let side = model.side_of(&shell::group_chat_id()).unwrap();
    let content = model.rects(&metrics_from_theme()).content(side).unwrap();
    let open = group_chat_native_rect(&model, content, false);
    for (a, b) in [(open.x, 693.), (open.y, 54.), (open.w, 291.2), (open.h, 712.)] {
        assert!((a - b).abs() <= 1., "open {open:?}");
    }
    let collapsed = group_chat_native_rect(&model, content, true);
    // Dozer has no expand button, so the collapsed chat spans the full width with no strip.
    assert!((collapsed.w - 456.).abs() <= 1., "collapsed {collapsed:?}");
    assert!((collapsed.y - 54.).abs() <= 1., "collapsed {collapsed:?}");
    assert!((collapsed.y + collapsed.h - 766.).abs() <= 1., "collapsed {collapsed:?}");
}

#[test]
fn list_collapse_gives_the_chat_the_full_width() {
    let (mut shell, _rt, _) = chat_shell("chat-list-collapse");
    ready(&mut shell);
    let before = expected_rect(&shell);
    let _ = host(&mut shell, Msg::Shell(ShellMsg::TogglePane(shell::group_chat_id())));
    let target = shell.group_chat.native.scheduled_target.as_ref().unwrap().0;
    assert_eq!(target, expected_rect(&shell));
    assert!(target.width > before.width);
    assert_eq!(target.y, before.y, "no strip: the expand button is gone");
}

#[test]
fn switching_the_project_replaces_the_page_and_a_new_page_is_not_ready() {
    let (mut shell, _rt, _) = chat_shell("chat-project-switch");
    ready(&mut shell);
    let first = shell.group_chat.native.generation().unwrap();
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::Projects(crate::projects::Message::Activate(2)));
    let a = actions(&shell);
    let destroy = a.iter().position(|x| x == "Destroy").expect("destroyed");
    let create = a.iter().position(|x| x == "Create").expect("recreated");
    assert!(destroy < create, "old project's page is gone before the new one exists: {a:?}");
    assert!(!a.contains(&"Show".into()), "{a:?}");
    assert_ne!(shell.group_chat.native.generation(), Some(first));
    assert_eq!(shell.group_chat.native.project_id(), Some(2));
    // A late ready of the replaced generation changes nothing.
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::GroupChatNative(ChatEvent::Page(first, Page::Ready)));
    assert!(!shell.group_chat.native.is_ready());
    assert!(!actions(&shell).contains(&"Show".into()));
}

#[test]
fn a_late_event_from_a_closed_page_cannot_reach_another_project() {
    let (mut shell, _rt, fake) = chat_shell("chat-late-event");
    ready(&mut shell);
    let old = shell.group_chat.native.generation().unwrap();
    let _ = host(&mut shell, Msg::Projects(crate::projects::Message::Activate(2)));
    let calls = fake.calls.lock().unwrap().len();
    let _ = host(
        &mut shell,
        Msg::GroupChatNative(ChatEvent::Page(old, Page::CreateGroup { topic: "x".into() })),
    );
    assert_eq!(fake.calls.lock().unwrap().len(), calls, "no command from a dead generation");
}

fn with_selected_group_and_messages(
    shell: &mut ProductShell,
    messages: Vec<GroupMessageInfo>,
) {
    let _ = host(shell, Msg::GroupChat(G::GroupsLoaded(1, Ok(vec![group(1)]))));
    let _ = host(shell, Msg::GroupChat(G::SelectRequested(1, 1)));
    let _ = host(
        shell,
        Msg::GroupChat(G::Polled {
            project_id: 1,
            group_id: 1,
            result: Ok((messages, 9)),
        }),
    );
}

#[test]
fn open_todo_shows_the_todo_panel_and_push_todo_creates_exactly_one_todo_call() {
    let (mut shell, rt, fake) = chat_shell("chat-todo");
    ready(&mut shell);
    let generation = shell.group_chat.native.generation().unwrap();
    with_selected_group_and_messages(&mut shell, vec![agent_message(5, GroupMessageStatus::Done)]);
    let _ = host(
        &mut shell,
        Msg::GroupChatNative(ChatEvent::Page(
            generation,
            Page::PushTodo { message_id: 5, text: "do it".into() },
        )),
    );
    rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
    let pushes = fake.calls.lock().unwrap().iter().filter(|c| c.starts_with("push_todo")).count();
    assert_eq!(pushes, 1);
    assert!(!shell.panel_visible(&shell::todo_id()) || shell.panel_visible(&shell::group_chat_id()));
    // OpenTodo brings the Todo panel up (it lives on the other side).
    let _ = host(
        &mut shell,
        Msg::GroupChatNative(ChatEvent::Page(generation, Page::OpenTodo { todo_id: 3 })),
    );
    assert!(shell.panel_visible(&shell::todo_id()));
}

#[test]
fn forged_events_have_no_effect_and_never_spawn_an_agent() {
    let (mut shell, rt, fake) = chat_shell("chat-forged");
    ready(&mut shell);
    let generation = shell.group_chat.native.generation().unwrap();
    with_selected_group_and_messages(&mut shell, vec![agent_message(5, GroupMessageStatus::Running)]);
    // Let the selection's own list/poll calls settle before taking the
    // baseline; the forged pages below must not add anything.
    rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
    let before = fake.calls.lock().unwrap().len();
    for page in [
        Page::SelectGroup { group_id: 999 },
        Page::DeleteGroup { group_id: 999 },
        Page::Post { group_id: 999, text: "x".into() },
        Page::Retry { message_id: 777 },
        Page::PushTodo { message_id: 5, text: "still running".into() },
        Page::PushTodo { message_id: 777, text: "x".into() },
        Page::Post { group_id: 1, text: "字".repeat(20_001) },
        Page::OpenTodo { todo_id: 0 },
    ] {
        let _ = host(&mut shell, Msg::GroupChatNative(ChatEvent::Page(generation, page)));
    }
    rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
    // Lock once: a second `lock()` inside the assertion message would deadlock
    // this test against itself.
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), before, "{calls:?}");
    assert!(!shell.panel_visible(&shell::todo_id()));
}

#[test]
fn deleting_a_group_only_calls_delete_and_never_posts_or_retries() {
    let (mut shell, rt, fake) = chat_shell("chat-delete");
    ready(&mut shell);
    let generation = shell.group_chat.native.generation().unwrap();
    with_selected_group_and_messages(&mut shell, vec![]);
    let _ = host(
        &mut shell,
        Msg::GroupChatNative(ChatEvent::Page(generation, Page::DeleteGroup { group_id: 1 })),
    );
    rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.contains(&"delete:1".to_string()), "{calls:?}");
    assert!(!calls.iter().any(|c| c == "post" || c == "retry"), "{calls:?}");
}

#[test]
fn a_ready_timeout_shows_the_failure_shell_destroys_the_page_and_retry_recreates_it() {
    let (mut shell, _rt, _) = chat_shell("chat-timeout");
    // Built but never ready.
    let generation = shell.group_chat.native.generation().unwrap();
    let _ = host(&mut shell, Msg::GroupChatNative(ChatEvent::Built(generation, Ok(()))));
    let now = std::time::Instant::now();
    let id = shell.window.unwrap();
    let rect = expected_rect(&shell);
    let _ = shell.group_chat.sync_native(id, true, false, rect, Some(1), now);
    assert!(shell.group_chat.page_failure().is_none());
    shell.group_chat.native.scheduled_actions.clear();
    let _ = shell.group_chat.sync_native(
        id,
        true,
        false,
        rect,
        Some(1),
        now + dozer_panel_group_chat::protocol::READY_TIMEOUT + std::time::Duration::from_secs(1),
    );
    assert!(shell.group_chat.page_failure().is_some());
    assert!(actions(&shell).contains(&"Destroy".into()), "{:?}", actions(&shell));
    // A ready that arrives after the failure does not undo it.
    let _ = host(&mut shell, Msg::GroupChatNative(ChatEvent::Page(generation, Page::Ready)));
    assert!(shell.group_chat.page_failure().is_some());
    shell.group_chat.native.scheduled_actions.clear();
    let _ = host(&mut shell, Msg::GroupChatRetry);
    assert!(shell.group_chat.page_failure().is_none());
    assert!(actions(&shell).contains(&"Create".into()), "{:?}", actions(&shell));
}

#[test]
fn a_build_failure_is_visible_in_the_notice_bar_and_the_failure_shell() {
    let (mut shell, _rt, _) = chat_shell("chat-build-fail");
    let generation = shell.group_chat.native.generation().unwrap();
    let _ = host(
        &mut shell,
        Msg::GroupChatNative(ChatEvent::Built(generation, Err("wry exploded".into()))),
    );
    assert!(shell.group_chat.page_failure().unwrap().contains("wry exploded"));
    assert!(shell.group_chat.notice.as_deref().is_some_and(|n| n.contains("wry exploded")));
}

#[test]
fn state_is_pushed_only_to_a_built_page_of_the_same_project() {
    // A page that is not built yet must not get (and so must not "consume") a push.
    let (mut shell, _rt, _) = chat_shell("chat-push-gate");
    let id = shell.window.unwrap();
    let rect = expected_rect(&shell);
    let task = shell.group_chat.sync_native(id, true, false, rect, Some(1), std::time::Instant::now());
    drop(task);
    assert!(!shell.group_chat.native.is_ready());
}

#[test]
fn window_closing_waits_for_the_page_destruction_before_shutdown_finishes() {
    use iced::futures::StreamExt;
    let (mut shell, rt, _) = chat_shell("chat-close-await");
    ready(&mut shell);
    shell.group_chat.native.scheduled_actions.clear();
    let main = shell.window_id().expect("main window");
    let task = host(&mut shell, Msg::CloseRequested(main));
    assert!(actions(&shell).contains(&"Destroy".into()));
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    let mut pending_callbacks = vec![];
    rt.block_on(async {
        while let Ok(Some(action)) =
            tokio::time::timeout(std::time::Duration::from_millis(30), stream.next()).await
        {
            assert!(
                !matches!(&action, iced_runtime::Action::Output(Msg::ShutdownFinished(_))),
                "shutdown must await the native window callbacks"
            );
            if matches!(
                &action,
                iced_runtime::Action::Window(iced_runtime::window::Action::Run(_, _))
            ) {
                pending_callbacks.push(action); // keep the response channels unresolved
            }
        }
    });
    assert!(!pending_callbacks.is_empty(), "destroy reaches the runtime before shutdown");
}
