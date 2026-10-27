use super::*;

fn desired() -> Desired {
    Desired {
        shown: true,
        overlay_open: false,
        rect: LogicalRect {
            x: 10.,
            y: 20.,
            width: 300.,
            height: 400.,
        },
        project_id: Some(7),
    }
}
fn live_native(generation: u64, ready: bool) -> GroupChatNative {
    let mut native = GroupChatNative::new();
    native.epoch.store(generation, Ordering::SeqCst);
    native.live = Some(Live {
        project_id: 7,
        desired: host_desired(&desired(), ready).unwrap(),
        generation,
        built: true,
        ready,
    });
    native
}
fn plan_names(native: &GroupChatNative, d: &Desired) -> Vec<String> {
    native.plan(d).iter().map(|a| format!("{a:?}")).collect()
}

#[test]
fn the_url_uses_the_trusted_origin_and_stays_parked_until_ready() {
    let waiting = host_desired(&desired(), false).unwrap();
    assert_eq!(
        waiting.url,
        "digger-groupchat://group-chat-content/host.html?theme=dark"
    );
    assert!(!waiting.loaded);
    assert!(host_desired(&desired(), true).unwrap().loaded);
    assert!(allows_page(&waiting.url));
}

#[test]
fn nothing_is_desired_without_a_project_or_when_not_shown() {
    let mut d = desired();
    d.shown = false;
    assert!(host_desired(&d, true).is_none());
    let mut d = desired();
    d.project_id = None;
    assert!(host_desired(&d, true).is_none());
}

#[test]
fn an_overlay_hides_the_page_without_destroying_it_and_closing_it_restores_it() {
    let native = live_native(1, true);
    let mut d = desired();
    d.overlay_open = true;
    let hidden = plan_names(&native, &d);
    assert_eq!(hidden, ["Park"]);
    assert!(!host_desired(&d, true).unwrap().visible);
    // Same state as before the overlay: nothing to do.
    assert!(plan_names(&native, &desired()).is_empty());
}

#[test]
fn unusable_one_pixel_blocks_never_get_a_visible_child() {
    let mut d = desired();
    d.rect.width = 1.;
    assert!(!host_desired(&d, true).unwrap().visible);
}

#[test]
fn a_project_change_replaces_the_single_child_and_not_ready_stays_parked() {
    let native = live_native(1, true);
    let mut d = desired();
    d.project_id = Some(8);
    // The replacement is a new document: even though the old page was ready it starts parked.
    assert_eq!(
        plan_names(&native, &d),
        ["Destroy", "Create", "Zoom", "Park"]
    );
    let mut parked = native;
    parked.live.as_mut().unwrap().ready = false;
    assert!(!plan_names(&parked, &desired()).iter().any(|a| a == "Show"));
}

#[test]
fn not_shown_destroys_and_a_stale_generation_is_replaced_never_reused() {
    let native = live_native(1, true);
    let mut d = desired();
    d.shown = false;
    assert_eq!(plan_names(&native, &d), ["Destroy"]);
    let mut stale = live_native(1, true);
    stale.invalidate();
    let actions = plan_names(&stale, &desired());
    assert_eq!(actions[0], "Destroy");
    assert!(actions.contains(&"Create".into()));
}

#[test]
fn scripts_are_only_injected_into_a_built_current_child_of_the_same_project() {
    let native = live_native(1, true);
    assert!(native.is_available_for(&desired()));
    let mut other = desired();
    other.project_id = Some(8);
    assert!(!native.is_available_for(&other));
    let mut hidden = desired();
    hidden.shown = false;
    assert!(!native.is_available_for(&hidden));
    let mut unbuilt = live_native(1, true);
    unbuilt.live.as_mut().unwrap().built = false;
    assert!(!unbuilt.is_available_for(&desired()));
    let mut stale = live_native(1, true);
    stale.invalidate();
    assert!(!stale.is_available_for(&desired()));
    assert!(!GroupChatNative::new().is_available_for(&desired()));
}

#[test]
fn ipc_from_another_origin_is_dropped_and_not_parsed() {
    let ready = r#"{"kind":"ready"}"#;
    assert!(matches!(
        classify(
            "digger-groupchat://group-chat-content/host.html?theme=dark",
            ready
        ),
        Some(Ok(GroupChatWebviewEvent::Ready))
    ));
    for url in [
        "",
        "https://example.com/",
        "file:///etc/passwd",
        "digger-groupchat://group-chat-content.evil/host.html",
        "digger-groupchat://group-chat-content@evil/host.html",
        "digger-groupchat://group-chat-content:80/host.html",
        "digger-groupchat://evil/host.html",
        "digger-review://review-trace/host.html",
        "digger-groupchat://group-chat-content\\.evil/host.html",
        "digger-groupchat://group-chat-content/host.html\n",
    ] {
        assert!(classify(url, ready).is_none(), "{url:?}");
    }
    let long = format!(
        "digger-groupchat://group-chat-content/host.html?{}",
        "a".repeat(MAX_PAGE_URL_BYTES)
    );
    assert!(classify(&long, ready).is_none());
}

#[test]
fn oversized_or_malformed_ipc_is_rejected_for_reporting_not_executed() {
    let url = "digger-groupchat://group-chat-content/host.html?theme=dark";
    assert!(matches!(classify(url, "not json"), Some(Err(_))));
    assert!(matches!(classify(url, r#"{"kind":"nope"}"#), Some(Err(_))));
    let huge = format!(
        r#"{{"kind":"set_draft","group_id":1,"text":"{}"}}"#,
        "x".repeat(MAX_EVENT_BYTES)
    );
    assert!(matches!(classify(url, &huge), Some(Err(m)) if m.contains("过大")));
}

#[test]
fn forged_ids_are_left_to_the_library_to_reject_not_interpreted_here() {
    let url = "digger-groupchat://group-chat-content/host.html?theme=dark";
    // The host only parses; id existence and limits belong to `route_event`.
    assert!(matches!(
        classify(url, r#"{"kind":"delete_group","group_id":999999999}"#),
        Some(Ok(GroupChatWebviewEvent::DeleteGroup {
            group_id: 999999999
        }))
    ));
}

#[test]
fn ready_marks_the_current_child_only_and_late_events_of_an_old_generation_are_ignored() {
    let mut native = live_native(2, false);
    assert!(
        native
            .update(Event::Page(1, GroupChatWebviewEvent::Ready))
            .is_none()
    );
    assert!(!native.is_ready());
    assert!(matches!(
        native.update(Event::Page(2, GroupChatWebviewEvent::Ready)),
        Some(GroupChatWebviewEvent::Ready)
    ));
    assert!(native.is_ready());
    native.invalidate();
    assert!(
        native
            .update(Event::Page(2, GroupChatWebviewEvent::Ready))
            .is_none()
    );
    assert!(!native.is_ready());
}

#[test]
fn build_failures_and_rejections_become_a_single_visible_error_for_the_current_child() {
    let mut native = live_native(3, false);
    assert!(native.update(Event::Built(2, Err("old".into()))).is_none());
    assert!(native.failed().is_none());
    assert!(native.update(Event::Built(3, Err("boom".into()))).is_none());
    assert_eq!(native.failed(), Some("boom"));
    assert_eq!(native.take_error().as_deref(), Some("boom"));
    assert!(native.failed().is_none());
    assert!(native.update(Event::Rejected(3, "bad".into())).is_none());
    assert_eq!(native.failed(), Some("bad"));
}

#[test]
fn the_protocol_serves_the_three_files_and_404s_everything_else() {
    for path in [
        "/host.html",
        "host.html",
        "/group-chat-content.js",
        "/group-chat-content.css",
    ] {
        let (status, _, body) = serve(path);
        assert_eq!(status, 200, "{path}");
        assert!(!body.is_empty());
    }
    for path in [
        "/",
        "",
        "/data.json",
        "/../host.html",
        "/%2e%2e/host.html",
        "/host.html/",
        "//host.html",
        "/host.html?x=1",
        "/Host.html",
    ] {
        let (status, mime, body) = serve(path);
        assert_eq!((status, mime), (404, "text/plain"), "{path}");
        assert!(body.is_empty());
    }
}

#[test]
fn new_window_and_download_requests_are_denied_by_the_shared_executor() {
    assert!(!native_host::allows_download());
    assert!(matches!(
        native_host::new_window_response(),
        wry::NewWindowResponse::Deny
    ));
}
