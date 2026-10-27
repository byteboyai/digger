use super::*;
fn desired(n: u64) -> Desired {
    Desired {
        visible: true,
        rect: LogicalRect {
            x: 10.,
            y: 20.,
            width: 300.,
            height: 200.,
        },
        spec: Some(ReviewWebviewSpec {
            url: format!("digger-review://review-trace/host.html?_r={n}&locale=en"),
            visible: false,
            park_offscreen: true,
        }),
        project_id: Some(1),
    }
}
fn live(d: &Desired) -> Live {
    Live {
        project_id: d.project_id.unwrap(),
        desired: host_desired(d, false).unwrap(),
        generation: 3,
        nonce: nonce(&d.spec.as_ref().unwrap().url).unwrap(),
        loaded: false,
    }
}
#[test]
fn plan_destroys_without_a_spec_or_when_not_visible_and_does_nothing_if_nothing_exists() {
    let mut d = desired(1);
    let l = live(&d);
    d.spec = None;
    assert_eq!(plan(Some(&l), &d), vec![Action::Destroy]);
    assert!(plan(None, &d).is_empty());
    d = desired(1);
    d.visible = false;
    assert_eq!(plan(Some(&l), &d), vec![Action::Destroy]);
}
#[test]
fn plan_creates_once_then_navigates_only_when_the_url_changes() {
    let d = desired(1);
    assert!(plan(None, &d).contains(&Action::Create));
    let l = live(&d);
    assert!(plan(Some(&l), &d).is_empty());
    assert_eq!(
        plan(Some(&l), &desired(2)),
        vec![Action::Park, Action::Navigate]
    );
}
#[test]
fn plan_parks_offscreen_until_loaded_then_shows_at_the_rect() {
    let mut d = desired(1);
    assert_eq!(
        plan(None, &d),
        vec![Action::Create, Action::Zoom, Action::Park]
    );
    let mut l = live(&d);
    l.loaded = true;
    d.spec.as_mut().unwrap().visible = true;
    d.spec.as_mut().unwrap().park_offscreen = false;
    assert_eq!(plan(Some(&l), &d), vec![Action::SetBounds, Action::Show]);
}
#[test]
fn plan_destroys_then_creates_when_the_project_changes() {
    let d = desired(1);
    let mut l = live(&d);
    l.loaded = true;
    l.desired.loaded = true;
    let mut next = d;
    next.project_id = Some(2);
    assert_eq!(
        plan(Some(&l), &next),
        vec![Action::Destroy, Action::Create, Action::Zoom, Action::Park]
    );
}

#[test]
fn plan_sets_bounds_only_when_the_rect_changed() {
    let mut d = desired(1);
    let mut l = live(&d);
    l.loaded = true;
    l.desired.loaded = true;
    d.rect.width += 1.;
    assert_eq!(plan(Some(&l), &d), vec![Action::SetBounds]);
}
#[test]
fn plan_hides_when_any_overlay_is_open_or_the_window_is_closing() {
    for _ in ["overlay", "closing"] {
        let mut d = desired(1);
        let l = live(&d);
        d.visible = false;
        assert_eq!(plan(Some(&l), &d), vec![Action::Destroy]);
    }
}
#[test]
fn loaded_matches_requires_the_current_nonce_and_rejects_garbage_urls() {
    assert!(loaded_matches(
        "digger-review://review-trace/host.html?_r=18446744073709551615",
        u64::MAX
    ));
    assert!(loaded_matches(
        "digger-review://review-trace/host.html?_r=0",
        0
    ));
    for url in [
        "digger-review://review-trace/host.html",
        "digger-review://review-trace/host.html?_r=abc",
        "digger-review://review-trace/host.html?_r=1&_r=1",
        "digger-review://review-trace.evil/host.html?_r=1",
        "digger-review://review-trace@evil/host.html?_r=1",
        "digger-review://review-trace/host.html?_r=%FF",
        "digger-review://review-trace/%FF?_r=1",
        "digger-review://review-trace/host.html?_r=18446744073709551616",
        "digger-review://review-trace/host.html?_r=0000000000000000000001",
        "digger-review://review-trace/host.html?_r=2",
    ] {
        assert!(!loaded_matches(url, 1), "{url}");
    }
}
#[test]
fn late_loaded_from_an_older_generation_is_ignored() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    assert_eq!(
        n.update(Event::Loaded {
            generation: 99,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        None
    );
    let old = n.live.as_ref().unwrap().generation;
    let _ = n.sync(
        window::Id::unique(),
        Desired {
            visible: false,
            ..desired(1)
        },
        None,
    );
    let _ = n.sync(window::Id::unique(), desired(1), None);
    assert_eq!(
        n.update(Event::Loaded {
            generation: old,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        None
    );
}
#[test]
fn protocol_serves_404_for_data_json_when_the_snapshot_is_none_and_the_json_when_set() {
    let s = Serving::default();
    assert_eq!(s.serve("/data.json").unwrap().0, 404);
    s.set(Some(Arc::new(Mutex::new(Some("{}".into())))));
    let (status, mime, body) = s.serve("/data.json").unwrap();
    assert_eq!(
        (status, mime, body),
        (200, "application/json", b"{}".to_vec())
    );
    assert_eq!(s.serve("/missing").unwrap().0, 404);
}
#[test]
fn switching_the_project_clears_the_served_data_before_swapping() {
    let s = Serving::default();
    let old = Arc::new(Mutex::new(Some("old".into())));
    s.set(Some(old.clone()));
    s.set(None);
    assert_eq!(s.serve("/data.json").unwrap().0, 404);
    assert_eq!(old.lock().unwrap().as_deref(), Some("old"));
    s.set(Some(Arc::new(Mutex::new(Some("new".into())))));
    assert_eq!(s.serve("/data.json").unwrap().2, b"new");
}
#[test]
fn navigation_handler_allows_only_the_review_trace_origin() {
    assert!(allows_navigation("digger-review://review-trace/host.html"));
    for u in [
        "https://review-trace/",
        "digger-review://review-trace.evil/",
        "digger-review://review-trace@evil/",
    ] {
        assert!(!allows_navigation(u));
    }
}

#[test]
fn new_window_and_download_requests_are_denied() {
    assert!(!native_host::allows_download());
    assert!(matches!(
        native_host::new_window_response(),
        wry::NewWindowResponse::Deny
    ));
}
#[test]
fn same_nonce_from_another_url_and_closed_owner_are_ignored() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    let generation = n.live.as_ref().unwrap().generation;
    assert_eq!(
        n.update(Event::Loaded {
            generation,
            nonce: 1,
            page_url: "digger-review://review-trace/host.html?_r=1&locale=fr".into()
        }),
        None
    );
    n.invalidate();
    assert_eq!(
        n.update(Event::Loaded {
            generation,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        None
    );
}

#[test]
fn project_replacement_unlinks_old_protocol_closures_and_preserves_cached_snapshots() {
    let mut n = ReviewNative::new();
    let old = Arc::new(Mutex::new(Some("old".into())));
    let _ = n.sync(window::Id::unique(), desired(1), Some(old.clone()));
    let previous_protocol = n.serving.clone();
    let old_generation = n.live.as_ref().unwrap().generation;
    let mut next = desired(1);
    next.project_id = Some(2);
    let _ = n.sync(
        window::Id::unique(),
        next,
        Some(Arc::new(Mutex::new(Some("new".into())))),
    );
    assert_eq!(previous_protocol.serve("/data.json").unwrap().0, 404);
    assert_eq!(old.lock().unwrap().as_deref(), Some("old"));
    assert_eq!(n.serving.serve("/data.json").unwrap().2, b"new");
    assert_eq!(
        n.update(Event::Loaded {
            generation: old_generation,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        None
    );
}
#[test]
fn navigation_resets_readiness_and_rebuild_requires_a_fresh_load() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    let generation = n.live.as_ref().unwrap().generation;
    assert_eq!(
        n.update(Event::Loaded {
            generation,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        Some(1)
    );
    let _ = n.sync(window::Id::unique(), desired(2), None);
    assert!(!n.live.as_ref().unwrap().loaded);
    assert_eq!(
        n.update(Event::Loaded {
            generation,
            nonce: 1,
            page_url: desired(1).spec.unwrap().url
        }),
        None
    );
    n.invalidate();
    let _ = n.sync(window::Id::unique(), desired(2), None);
    assert!(!n.live.as_ref().unwrap().loaded);
    assert_ne!(n.live.as_ref().unwrap().generation, generation);
}
#[test]
fn built_and_protocol_failures_are_generation_guarded_and_visible() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    let generation = n.live.as_ref().unwrap().generation;
    n.update(Event::Built(
        generation.wrapping_add(1),
        Err("stale".into()),
    ));
    assert!(n.failed().is_none());
    n.update(Event::Built(generation, Err("build failed".into())));
    assert_eq!(n.failed(), Some("build failed"));
    n.update(Event::Failed(generation, "bad IPC".into()));
    assert_eq!(n.failed(), Some("bad IPC"));
}

#[test]
fn loaded_matches_rejects_overlong_page_urls() {
    let url = format!(
        "digger-review://review-trace/host.html?_r=1&padding={}",
        "x".repeat(8192)
    );
    assert!(!loaded_matches(&url, 1));
    let url = format!("digger-review://review-trace/{}?_r=1", "x".repeat(8192));
    assert!(!loaded_matches(&url, 1));
}

#[test]
fn closing_an_unrelated_project_preserves_the_live_owner() {
    let mut n = ReviewNative::new();
    let snapshot = Arc::new(Mutex::new(Some("owner".into())));
    let _ = n.sync(window::Id::unique(), desired(1), Some(snapshot));
    let generation = n.live.as_ref().unwrap().generation;
    n.update(Event::Loaded {
        generation,
        nonce: 1,
        page_url: desired(1).spec.unwrap().url,
    });
    n.invalidate_project(2);
    assert_eq!(n.epoch.load(Ordering::SeqCst), generation);
    assert!(n.live.as_ref().unwrap().loaded);
    assert_eq!(n.serving.serve("/data.json").unwrap().2, b"owner");
    let _ = n.sync(
        window::Id::unique(),
        desired(1),
        Some(Arc::new(Mutex::new(Some("owner".into())))),
    );
    assert_eq!(n.live.as_ref().unwrap().generation, generation);
    assert!(n.live.as_ref().unwrap().loaded);
    n.invalidate_project(1);
    assert_ne!(n.epoch.load(Ordering::SeqCst), generation);
    assert_eq!(n.serving.serve("/data.json").unwrap().0, 404);
}

#[test]
fn shared_sync_failures_stop_before_show_and_reach_the_review_notice_channel() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    let generation = n.live.as_ref().unwrap().generation;
    let mut executed = Vec::new();
    let result = native_host::apply_actions(vec![Action::Navigate, Action::Show], |action| {
        executed.push(action);
        Err("injected navigation failure".into())
    });
    let event = native_host::sync_outcome(generation, result, Event::Built, Event::Done);
    n.update(event);
    assert_eq!(n.failed(), Some("injected navigation failure"));
    assert_eq!(executed, vec![Action::Navigate]);
    assert!(!n.live.as_ref().unwrap().loaded);
}

#[test]
fn late_document_errors_from_a_previous_url_do_not_poison_the_current_document() {
    let mut n = ReviewNative::new();
    let _ = n.sync(window::Id::unique(), desired(1), None);
    let generation = n.live.as_ref().unwrap().generation;
    let _ = n.sync(window::Id::unique(), desired(2), None);
    n.update(Event::DocumentFailed {
        generation,
        page_url: desired(1).spec.unwrap().url,
        error: "old IPC or navigation error".into(),
    });
    assert!(n.failed().is_none());
    n.update(Event::DocumentFailed {
        generation,
        page_url: desired(2).spec.unwrap().url,
        error: "current document error".into(),
    });
    assert_eq!(n.failed(), Some("current document error"));
}

fn window_actions(task: Task<Event>) -> usize {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return 0;
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let mut count = 0;
            while let Ok(Some(action)) =
                tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await
            {
                if matches!(action, iced_runtime::Action::Window(_)) {
                    count += 1;
                }
            }
            count
        })
}

#[test]
fn shown_document_failure_schedules_park_preserves_error_and_allows_valid_loaded_recovery() {
    let mut native = ReviewNative::new();
    let id = window::Id::unique();
    let d = desired(1);
    let snapshot = Arc::new(Mutex::new(Some("cached transcript".into())));
    let _ = native.sync(id, d.clone(), Some(snapshot.clone()));
    let generation = native.live.as_ref().unwrap().generation;
    let loaded = || Event::Loaded {
        generation,
        nonce: 1,
        page_url: d.spec.as_ref().unwrap().url.clone(),
    };
    assert_eq!(native.update(loaded()), Some(1));
    assert_eq!(
        window_actions(native.sync(id, d.clone(), Some(snapshot.clone()))),
        1,
        "show must reach iced runtime"
    );
    native.update(Event::DocumentFailed {
        generation,
        page_url: d.spec.as_ref().unwrap().url.clone(),
        error: "page rendering failed".into(),
    });
    assert_eq!(plan(native.live.as_ref(), &d), vec![Action::Park]);
    assert_eq!(
        window_actions(native.sync(id, d.clone(), Some(snapshot.clone()))),
        1,
        "parking an already shown child must reach iced runtime"
    );
    assert_eq!(native.failed(), Some("page rendering failed"));
    assert_eq!(
        native.serving.serve("/data.json").unwrap().2,
        b"cached transcript"
    );
    assert_eq!(
        window_actions(native.sync(id, d.clone(), Some(snapshot.clone()))),
        0,
        "repeated sync must not recreate or loop on the error"
    );
    assert_eq!(native.failed(), Some("page rendering failed"));
    assert_eq!(
        native.update(loaded()),
        Some(1),
        "a valid current Loaded may show the page's own error fallback"
    );
    assert!(native.failed().is_none());
    assert_eq!(
        window_actions(native.sync(id, d, Some(snapshot))),
        1,
        "recovered document must be shown again"
    );
}
