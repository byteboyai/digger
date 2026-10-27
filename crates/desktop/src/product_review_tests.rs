// Product/window-action tests: the fake ID never executes a WKWebView closure.
use crate::adapters::conversations::tests::{project, receive, row};
use crate::adapters::review_native::Event;
use dozer_panel_conversations::Message as C;

fn host(shell: &mut ProductShell, msg: Msg) -> Task<Msg> {
    update_host(shell, HostMessage::Product(msg))
}
fn show(shell: &mut ProductShell, id: PanelId) {
    let _ = host(shell, Msg::Shell(ShellMsg::ShowPanel(id)));
}
fn review_shell(tag: &str) -> (ProductShell, tokio::runtime::Runtime) {
    let (mut shell, rt, fake) = conversations_shell(tag);
    shell.agent.attach_handle(rt.handle().clone());
    let a = project(1, "/a");
    shell.projects.seed_for_test(a.clone());
    show(&mut shell, shell::conversations_id());
    let _ = host(
        &mut shell,
        Msg::Conversations(1, C::SessionsRefreshed(1, Ok(vec![row("one")]))),
    );
    fake.turn_responses.lock().unwrap().push_back(Ok(vec![serde_json::from_value(serde_json::json!({"turn_index":0,"role":"human","content":"review text","thinking":false,"ts":null,"tokens_in":0,"tokens_out":0,"tokens_cache_read":0,"tokens_cache_write":0})).unwrap()]));
    let _ = host(
        &mut shell,
        Msg::Conversations(
            1,
            C::SessionOpen("one".into(), dozer_model::AgentKind::Claude),
        ),
    );
    for _ in 0..2 {
        let (pid, message) = receive(&mut shell.conversations, &rt);
        let _ = host(&mut shell, Msg::Conversations(pid, message));
    }
    shell.window = Some(window::Id::unique());
    let _ = host(&mut shell, Msg::Shell(ShellMsg::Noop));
    assert!(shell.conversations.native.document_identity().is_some());
    (shell, rt)
}
fn loaded(shell: &mut ProductShell) {
    let (generation, nonce, page_url) = shell.conversations.native.document_identity().unwrap();
    let _ = host(
        shell,
        Msg::ReviewNative(Event::Loaded {
            generation,
            nonce,
            page_url,
        }),
    );
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Show".into())
    );
}
fn destroyed(shell: &ProductShell) {
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Destroy".into())
    );
    assert!(shell.conversations.native.document_identity().is_none());
}
fn current_rect(shell: &ProductShell) -> bytehost_core::LogicalRect {
    let side = shell.shell.side_of(&shell::conversations_id()).unwrap();
    let content = shell
        .shell
        .rects(&metrics_from_theme())
        .content(side)
        .unwrap();
    let collapsed = shell.pane_collapsed(&shell::conversations_id());
    let mut block = if collapsed {
        content
    } else {
        shell::pair_rects(&shell.shell, side).unwrap().second
    };
    let notice = shell
        .conversations
        .review_notice_height(collapsed)
        .min(block.h.max(0.));
    block.y += notice;
    block.h = (block.h - notice).max(0.);
    logical(crate::adapters::review_geometry::review_bounds(
        block,
        byteui::theme::icon_size::scale(),
    ))
}

#[test]
#[cfg(target_os = "macos")]
fn opening_the_native_agent_filter_menu_keeps_the_review_webview_visible() {
    let (mut shell, _rt) = review_shell("review-native-filter");
    loaded(&mut shell);
    let _task = host(
        &mut shell,
        Msg::Conversations(1, C::AgentFilterOpen { at: (10., 20.) }),
    );
    assert!(!shell.conversations.state(1).unwrap().agent_picker_open());
    assert!(!shell.review_overlay_open());
    assert!(
        shell
            .conversations
            .native
            .scheduled_target
            .as_ref()
            .unwrap()
            .1
    );
    assert!(
        !shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Destroy".into())
    );
    assert!(
        shell.conversations.take_requests().is_empty(),
        "real native filter request was drained"
    );
}

#[test]
fn opening_the_agent_picker_or_overflow_menu_or_confirm_hides_it_too() {
    let (mut shell, _rt) = review_shell("review-agent-overlays");
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::PanelPress {
            panel: shell::agent_id(),
        }),
    );
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::RailDragMove {
            side: Side::Left,
            index: 0,
        }),
    );
    let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    show(&mut shell, shell::conversations_id());
    loaded(&mut shell);
    let mut state = dozer_panel_agent::state::AgentPanelState::new();
    {
        let _guard = _rt.enter();
        crate::adapters::agent::tests::insert_tab(
            &mut state,
            "running",
            dozer_model::AgentKind::Claude,
        );
    }
    state.tabs[0].agent_state = dozer_model::AgentState::Running;
    shell.agent.states.insert(1, state);
    let _ = host(
        &mut shell,
        Msg::Agent(1, dozer_panel_agent::update::Message::PickerToggle),
    );
    assert!(shell.agent.picker.is_some());
    destroyed(&shell);
    let _ = host(&mut shell, Msg::AgentPickerClose);
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Park".into())
    );
    loaded(&mut shell);
    let _ = host(
        &mut shell,
        Msg::Agent(
            1,
            dozer_panel_agent::update::Message::OverflowToggle { at: Some((0., 0.)) },
        ),
    );
    #[cfg(target_os = "macos")]
    {
        assert!(shell.agent.overflow.is_none());
        assert!(
            shell.conversations.native.document_identity().is_some(),
            "NSMenu is above native child"
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        destroyed(&shell);
    }
    let _ = host(&mut shell, Msg::AgentOverflowDismiss);
    let _ = host(
        &mut shell,
        Msg::Agent(1, dozer_panel_agent::update::Message::CloseTab(0)),
    );
    assert_eq!(
        shell.agent.states[&1].pending_close_tab.as_deref(),
        Some("running")
    );
    destroyed(&shell);
}

#[test]
fn switching_to_another_right_panel_destroys_it() {
    let (mut shell, _rt) = review_shell("review-panel-switch");
    loaded(&mut shell);
    assert!(
        shell
            .conversations
            .state(1)
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .loaded_nonce
            .is_some()
    );
    show(&mut shell, shell::usage_id());
    destroyed(&shell);
}
#[test]
fn collapsing_the_right_zone_or_maximizing_the_left_one_destroys_it() {
    for maximize in [false, true] {
        let (mut shell, _rt) = review_shell(if maximize {
            "review-max"
        } else {
            "review-collapse"
        });
        loaded(&mut shell);
        let message = if maximize {
            ShellMsg::Maximize(Side::Left)
        } else {
            ShellMsg::PanelPress {
                panel: shell::conversations_id(),
            }
        };
        let _ = host(&mut shell, Msg::Shell(message));
        destroyed(&shell);
    }
}
#[test]
fn window_closing_destroys_it_before_shutdown() {
    let (mut shell, _rt) = review_shell("review-close-window");
    loaded(&mut shell);
    let task = host(&mut shell, Msg::CloseRequested);
    destroyed(&shell);
    assert!(shell.closing);
    use iced::futures::StreamExt;
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    let mut pending_callbacks = vec![];
    _rt.block_on(async {
        while let Ok(Some(action)) =
            tokio::time::timeout(std::time::Duration::from_millis(30), stream.next()).await
        {
            assert!(
                !matches!(
                    &action,
                    iced_runtime::Action::Output(Msg::ShutdownFinished(_))
                ),
                "shutdown must await native window callbacks"
            );
            if matches!(
                &action,
                iced_runtime::Action::Window(iced_runtime::window::Action::Run(_, _))
            ) {
                pending_callbacks.push(action); // Keep callback response channels unresolved.
            }
        }
    });
    assert!(
        !pending_callbacks.is_empty(),
        "destroy reaches the runtime before shutdown"
    );
}
#[test]
fn the_first_frame_after_becoming_visible_is_parked_not_shown_until_loaded() {
    let (mut shell, _rt) = review_shell("review-first-frame");
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Park".into())
    );
    assert!(
        !shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Show".into())
    );
    loaded(&mut shell);
    show(&mut shell, shell::usage_id());
    show(&mut shell, shell::conversations_id());
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Create".into())
    );
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Park".into())
    );
    assert!(
        !shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Show".into()),
        "cached library loaded nonce cannot reveal a new child"
    );
    loaded(&mut shell);
}
#[test]
fn dragging_the_divider_moves_the_webview_with_the_block() {
    for divider in [Divider::Zones, Divider::Pair(Side::Right)] {
        let (mut shell, _rt) = review_shell(&format!("review-divider-{divider:?}"));
        loaded(&mut shell);
        let pair = shell::pair_rects(&shell.shell, Side::Right).unwrap();
        let _ = host(&mut shell, Msg::Shell(ShellMsg::DividerPress(divider)));
        let mut rects = vec![];
        for i in 0..20 {
            let x = match divider {
                Divider::Zones => 410. + i as f32 * 6.,
                Divider::Pair(_) => pair.second.x + pair.second.w * 0.55 + i as f32 * 3.,
            };
            let _ = host(&mut shell, Msg::Shell(ShellMsg::CursorMoved { x, y: 300. }));
            let rect = shell
                .conversations
                .native
                .scheduled_target
                .as_ref()
                .unwrap()
                .0;
            assert_eq!(rect, current_rect(&shell));
            assert!(
                shell
                    .conversations
                    .native
                    .scheduled_actions
                    .contains(&"SetBounds".into()),
                "{divider:?} move {i}: pair={pair:?} rect={rect:?} actions={:?}",
                shell.conversations.native.scheduled_actions
            );
            assert!(
                !rects.contains(&rect),
                "twenty distinct positions for {divider:?}"
            );
            rects.push(rect);
        }
        let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    }
}
#[test]
fn list_collapse_expands_the_review_to_the_full_width() {
    let (mut shell, _rt) = review_shell("review-list-collapse");
    loaded(&mut shell);
    let before = current_rect(&shell);
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::TogglePane(shell::conversations_id())),
    );
    let target = shell
        .conversations
        .native
        .scheduled_target
        .as_ref()
        .unwrap();
    assert_eq!(target.0, current_rect(&shell));
    assert!(target.0.width > before.width);
    let side = shell.shell.side_of(&shell::conversations_id()).unwrap();
    let content = shell
        .shell
        .rects(&metrics_from_theme())
        .content(side)
        .unwrap();
    assert_eq!(
        target.0,
        logical(crate::adapters::review_geometry::review_bounds(
            content,
            byteui::theme::icon_size::scale()
        ))
    );
}
#[test]
fn tick_queries_the_real_window_minimized_state() {
    use iced::futures::StreamExt;
    let (mut shell, rt) = review_shell("review-minimize-query");
    let id = shell.window.unwrap();
    let task = host(&mut shell, Msg::Tick);
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    let queried = rt.block_on(async {
        while let Ok(Some(action)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), stream.next()).await
        {
            if let iced_runtime::Action::Window(iced_runtime::window::Action::GetMinimized(
                window,
                _,
            )) = action
            {
                assert_eq!(window, id);
                return true;
            }
        }
        false
    });
    assert!(
        queried,
        "a tick must ask the OS; focus loss alone is not minimize"
    );
}

// Answer runtime window queries, without executing native Run closures.
fn answer_window_sample(
    task: Task<Msg>,
    rt: &tokio::runtime::Runtime,
    minimized: Option<bool>,
    size: Size,
) -> Msg {
    use iced::futures::StreamExt;
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    rt.block_on(async {
        while let Some(action) =
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                .await
                .unwrap()
        {
            match action {
                iced_runtime::Action::Window(iced_runtime::window::Action::GetMinimized(_, tx)) => {
                    let _ = tx.send(minimized);
                }
                iced_runtime::Action::Window(iced_runtime::window::Action::GetSize(_, tx)) => {
                    let _ = tx.send(size);
                }
                iced_runtime::Action::Output(m @ Msg::WindowStateSample { .. }) => return m,
                _ => {}
            }
        }
        panic!("window query never returned its sample")
    })
}
fn size(shell: &ProductShell) -> Size {
    let (width, height) = shell.shell.window();
    Size::new(width, height)
}
#[test]
fn minimize_and_restore_use_os_results_and_recreate_a_parked_child() {
    let (mut shell, rt) = review_shell("review-minimize-restore");
    loaded(&mut shell);
    let dimensions = size(&shell);
    let query = host(&mut shell, Msg::Tick);
    let result = answer_window_sample(query, &rt, Some(true), Size::ZERO);
    let _ = host(&mut shell, result);
    assert!(shell.window_minimized);
    assert_eq!(
        size(&shell),
        dimensions,
        "minimized OS dimensions must not overwrite the saved layout"
    );
    destroyed(&shell);
    let query = host(&mut shell, Msg::Tick);
    let result = answer_window_sample(query, &rt, Some(false), dimensions);
    let _ = host(&mut shell, result);
    assert!(!shell.window_minimized);
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Create".into())
    );
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Park".into())
    );
    assert!(
        !shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Show".into())
    );
    loaded(&mut shell);
}
#[test]
fn window_queries_ignore_stale_replies_old_windows_and_closing_owners() {
    let (mut shell, rt) = review_shell("review-stale-window");
    loaded(&mut shell);
    let dimensions = size(&shell);
    let old_query = host(&mut shell, Msg::Tick);
    let new_query = host(&mut shell, Msg::Tick);
    let old = answer_window_sample(old_query, &rt, Some(true), Size::new(500., 500.));
    let latest = answer_window_sample(new_query, &rt, Some(false), dimensions);
    let _ = host(&mut shell, latest);
    let _ = host(&mut shell, old);
    assert!(!shell.window_minimized);
    assert_eq!(size(&shell), dimensions);
    let query = host(&mut shell, Msg::Tick);
    let old_window = answer_window_sample(query, &rt, Some(true), dimensions);
    shell.window = Some(window::Id::unique());
    let _ = host(&mut shell, old_window);
    assert!(!shell.window_minimized);
    let query = host(&mut shell, Msg::Tick);
    let closing = answer_window_sample(query, &rt, Some(true), dimensions);
    let _ = host(&mut shell, Msg::CloseRequested);
    let _ = host(&mut shell, closing);
    assert!(!shell.window_minimized);
    assert!(shell.conversations.native.document_identity().is_none());
}
#[test]
fn native_menu_focus_loss_and_os_rescale_query_without_directly_hiding_review() {
    let (mut shell, rt) = review_shell("review-focus-scale");
    loaded(&mut shell);
    for event in [
        window::Event::Unfocused,
        window::Event::Focused,
        window::Event::Rescaled(2.),
    ] {
        let msg = window_state_event(
            iced::Event::Window(event),
            iced::event::Status::Ignored,
            shell.window.unwrap(),
        )
        .unwrap();
        let task = host(&mut shell, msg);
        assert!(!shell.window_minimized);
        assert!(shell.conversations.native.document_identity().is_some());
        let reply = answer_window_sample(task, &rt, Some(false), size(&shell));
        let _ = host(&mut shell, reply);
        assert!(
            shell
                .conversations
                .native
                .scheduled_target
                .as_ref()
                .unwrap()
                .1
        );
    }
}
#[test]
fn fullscreen_resize_keeps_native_bounds_current_and_invalidates_an_older_size_sample() {
    let (mut shell, rt) = review_shell("review-fullscreen-resize");
    loaded(&mut shell);
    let query = host(&mut shell, Msg::Tick);
    let old_sample = answer_window_sample(query, &rt, Some(false), size(&shell));
    for dimensions in [Size::new(1920., 1080.), Size::new(1200., 800.)] {
        let msg =
            bytehost_chrome::map_event(&iced::Event::Window(window::Event::Resized(dimensions)))
                .unwrap();
        let _ = host(&mut shell, Msg::Shell(msg));
        assert_eq!(
            shell
                .conversations
                .native
                .scheduled_target
                .as_ref()
                .unwrap()
                .0,
            current_rect(&shell)
        );
        assert!(
            shell
                .conversations
                .native
                .scheduled_actions
                .contains(&"SetBounds".into())
        );
    }
    let _ = host(&mut shell, old_sample);
    assert_eq!(size(&shell), Size::new(1200., 800.));
}
#[test]
fn moving_conversations_to_the_left_mirrors_the_block_and_native_header_geometry() {
    let (mut shell, _rt) = review_shell("review-left-mirror");
    loaded(&mut shell);
    let right = current_rect(&shell);
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::PanelPress {
            panel: shell::conversations_id(),
        }),
    );
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::RailDragMove {
            side: Side::Left,
            index: 0,
        }),
    );
    let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    show(&mut shell, shell::conversations_id());
    assert_eq!(
        shell.shell.side_of(&shell::conversations_id()),
        Some(Side::Left)
    );
    let rect = shell
        .conversations
        .native
        .scheduled_target
        .as_ref()
        .unwrap()
        .0;
    assert_eq!(rect, current_rect(&shell));
    assert!(rect.x < right.x);
    assert_eq!(rect.y, right.y);
}
#[test]
fn cached_review_remains_visible_when_daemon_is_unavailable_but_not_for_another_owner() {
    let (mut shell, _rt) = review_shell("review-cached-offline");
    loaded(&mut shell);
    let _ = host(
        &mut shell,
        Msg::AgentShell(crate::adapters::agent::AgentShellMsg::Ensured(Err(
            "offline".into(),
        ))),
    );
    let _ = host(&mut shell, Msg::Shell(ShellMsg::Noop));
    assert!(
        shell
            .conversations
            .native
            .scheduled_target
            .as_ref()
            .unwrap()
            .1
    );
    shell.projects.seed_for_test(project(2, "/b"));
    let _ = host(
        &mut shell,
        Msg::Projects(crate::projects::Message::Activate(2)),
    );
    destroyed(&shell);
    assert!(shell.conversations.native.scheduled_target.is_none());
}

#[test]
fn opening_project_delete_confirmation_hides_review_without_deleting_anything() {
    let (mut shell, _rt) = review_shell("review-project-delete-card");
    shell.project.attach_handle(_rt.handle().clone());
    loaded(&mut shell);
    show(&mut shell, shell::project_id());
    let _ = host(
        &mut shell,
        Msg::Project(dozer_panel_project::Message::DeleteProjectRequest),
    );
    assert!(shell.project.delete_card(1).is_some());
    destroyed(&shell);
    let _ = host(
        &mut shell,
        Msg::Project(dozer_panel_project::Message::DeleteProjectCancel),
    );
    assert!(shell.project.delete_card(1).is_none());
    assert!(
        shell
            .conversations
            .native
            .scheduled_actions
            .contains(&"Park".into())
    );
}
#[test]
#[cfg(not(target_os = "macos"))]
fn opening_the_iced_conversations_filter_hides_review() {
    let (mut shell, _rt) = review_shell("review-iced-filter");
    loaded(&mut shell);
    let _ = host(
        &mut shell,
        Msg::Conversations(1, C::AgentFilterOpen { at: (10., 20.) }),
    );
    assert!(shell.conversations.state(1).unwrap().agent_picker_open());
    destroyed(&shell);
}
#[test]
fn closing_the_review_owner_discards_its_native_document() {
    let (mut shell, _rt) = review_shell("review-owner-close");
    loaded(&mut shell);
    let old = shell.conversations.native.document_identity().unwrap();
    let _ = host(
        &mut shell,
        Msg::Projects(crate::projects::Message::Close(1)),
    );
    destroyed(&shell);
    assert!(shell.conversations.state(1).is_none());
    let _ = host(
        &mut shell,
        Msg::ReviewNative(Event::Loaded {
            generation: old.0,
            nonce: old.1,
            page_url: old.2,
        }),
    );
    assert!(shell.conversations.native.document_identity().is_none());
}
#[test]
fn actual_search_focus_feedback_blocks_agent_then_panel_switch_restores_input_eligibility() {
    use crate::adapters::conversations::tests::search_focus_feedback;
    let (mut shell, rt) = review_shell("review-search-keyboard");
    // Move Conversations across the rail while leaving Agent on the right.
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::PanelPress {
            panel: shell::conversations_id(),
        }),
    );
    let _ = host(
        &mut shell,
        Msg::Shell(ShellMsg::RailDragMove {
            side: Side::Left,
            index: 0,
        }),
    );
    let _ = host(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
    show(&mut shell, shell::conversations_id());
    show(&mut shell, shell::agent_id());
    assert!(!shell.agent_keys_blocked());
    let task = host(&mut shell, Msg::ConversationSearchFocusFrame(1));
    let feedback = search_focus_feedback(&shell.conversations, &project(1, "/a"), &rt, task, true);
    let _ = host(&mut shell, feedback);
    assert!(shell.conversations.state(1).unwrap().search_focused());
    assert!(shell.agent_keys_blocked());
    show(&mut shell, shell::markdown_id());
    assert!(!shell.conversations.state(1).unwrap().search_focused());
    assert!(!shell.agent_keys_blocked());
    // Returning from native detail also schedules Destroy, whose shared host restores
    // the content NSView first responder. Live typing/IME remains a manual check.
    assert!(shell.conversations.native.document_identity().is_none());
}
