use super::*;
fn desired() -> Desired {
    Desired {
        identity: "project-7".into(),
        url: "test://localhost/host.html?r=1".into(),
        rect: LogicalRect {
            x: 1.,
            y: 2.,
            width: 100.,
            height: 80.,
        },
        scale: 1.,
        visible: true,
        loaded: true,
    }
}
#[test]
fn create_always_applies_zoom_and_geometry() {
    assert_eq!(
        plan(None, Some(&desired())),
        vec![
            Action::Create,
            Action::Zoom,
            Action::SetBounds,
            Action::Show
        ]
    );
}
#[test]
fn hidden_destroys_and_unchanged_is_idle() {
    let d = desired();
    assert_eq!(plan(Some(&d), None), vec![Action::Destroy]);
    assert!(plan(Some(&d), Some(&d)).is_empty());
}
#[test]
fn project_change_replaces_the_single_slot() {
    let a = desired();
    let mut b = a.clone();
    b.identity = "project-8".into();
    assert_eq!(
        plan(Some(&a), Some(&b)),
        vec![
            Action::Destroy,
            Action::Create,
            Action::Zoom,
            Action::SetBounds,
            Action::Show
        ]
    );
}
#[test]
fn nonce_navigation_parks_until_loaded_then_shows() {
    let a = desired();
    let mut b = a.clone();
    b.url.push_str("&nonce=2");
    b.loaded = false;
    assert_eq!(
        plan(Some(&a), Some(&b)),
        vec![Action::Park, Action::Navigate]
    );
    let mut c = b.clone();
    c.loaded = true;
    assert_eq!(
        plan(Some(&b), Some(&c)),
        vec![Action::SetBounds, Action::Show]
    );
}
#[test]
fn geometry_and_scale_changes_reuse_the_view() {
    let a = desired();
    let mut b = a.clone();
    b.rect.width += 1.;
    b.scale = 1.25;
    assert_eq!(
        plan(Some(&a), Some(&b)),
        vec![Action::Zoom, Action::SetBounds]
    );
}
#[test]
fn navigation_requires_exact_unencoded_authority() {
    assert!(allows_navigation(
        "test",
        "localhost",
        "test://localhost/host.html?_r=123#x"
    ));
    for url in [
        "https://localhost/",
        "test://localhost.evil/",
        "test://localhost@evil/",
        "test://user@localhost/",
        "test://localhost:80/",
        "test://%6cocalhost/",
        "test://localhost%2fevil/",
        "test://localhost\\@evil/",
        "test://localhost",
        "test://LOCALHOST/",
        "test://localhost/\n",
    ] {
        assert!(!allows_navigation("test", "localhost", url), "{url}");
    }
}
#[test]
fn review_trace_authority_is_supported_without_subdomains_credentials_or_ports() {
    assert!(allows_navigation(
        "digger-review",
        "review-trace",
        "digger-review://review-trace/host.html?_r=7"
    ));
    for url in [
        "digger-review://review-trace.evil/",
        "digger-review://review-trace@evil/",
        "digger-review://user@review-trace/",
        "digger-review://review-trace:80/",
        "digger-review://review%2dtrace/",
        "digger-review://localhost/",
    ] {
        assert!(
            !allows_navigation("digger-review", "review-trace", url),
            "{url}"
        );
    }
}
#[test]
fn not_loaded_creation_parks_and_loaded_transition_restores_bounds() {
    let mut pending = desired();
    pending.loaded = false;
    assert_eq!(
        plan(None, Some(&pending)),
        vec![Action::Create, Action::Zoom, Action::Park]
    );
    let loaded = desired();
    assert_eq!(
        plan(Some(&pending), Some(&loaded)),
        vec![Action::SetBounds, Action::Show]
    );
}
