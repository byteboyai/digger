use super::*;
use digger_editor::bridge::Mode;
use serde_json::json;

fn page(panel: &MarkdownPanel, v: serde_json::Value) -> String {
    let mut v = v;
    v["n"] = json!(panel.nonce);
    if v["t"] == "ready" {
        v["protocol"] = json!(digger_editor::protocol::PROTOCOL_VERSION);
    }
    v.to_string()
}

fn live_panel() -> MarkdownPanel {
    let mut p = MarkdownPanel::with_mode(Mode::Normal);
    p.creation = CreationState::Ready {
        generation: p.generation(),
    };
    p
}

fn rect() -> LogicalRect {
    LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    }
}

#[test]
fn a_failed_creation_retries_with_a_new_generation_and_ignores_the_old_one() {
    let mut p = MarkdownPanel::with_mode(Mode::Normal);
    p.window = Some(window::Id::unique());
    p.bounds = Some(rect());
    p.shown = true;
    p.creation = CreationState::Failed {
        generation: 0,
        error: "boom".into(),
    };
    assert_eq!(p.failed_error(), Some("boom"));

    let _ = p.update(Message::Retry);
    assert_eq!(
        p.creation,
        CreationState::Building { generation: 1 },
        "retry advances the generation and clears the old error"
    );
    assert_eq!(p.failed_error(), None);

    // A late success / failure from the failed generation must not touch the new instance.
    let _ = p.update(Message::Built(0, Ok(())));
    assert_eq!(p.creation, CreationState::Building { generation: 1 });

    let _ = p.update(Message::Built(1, Err("again".into())));
    assert_eq!(
        p.creation,
        CreationState::Failed {
            generation: 1,
            error: "again".into()
        }
    );

    let _ = p.update(Message::Retry);
    let _ = p.update(Message::Built(2, Ok(())));
    assert_eq!(p.creation, CreationState::Ready { generation: 2 });
}

#[test]
fn hiding_and_showing_a_ready_panel_does_not_rebuild_it() {
    let mut p = live_panel();
    let generation = p.generation();
    let id = window::Id::unique();
    p.shown = true;
    p.bounds = Some(rect());
    let _ = p.sync_native(id, false, rect());
    assert!(!p.shown);
    let _ = p.sync_native(id, true, rect());
    assert!(p.shown);
    assert_eq!(
        p.generation(),
        generation,
        "show/hide keeps the same webview"
    );
    assert_eq!(p.creation, CreationState::Ready { generation });
}

#[test]
fn a_failed_panel_rebuilds_when_it_is_shown_again() {
    let mut p = MarkdownPanel::with_mode(Mode::Normal);
    p.creation = CreationState::Failed {
        generation: 0,
        error: "boom".into(),
    };
    p.shown = false;
    let _ = p.sync_native(window::Id::unique(), true, rect());
    assert_eq!(p.creation, CreationState::Building { generation: 1 });
    assert!(p.shown);
}

#[test]
fn closing_sets_closed_and_advances_the_generation() {
    let mut p = live_panel();
    let generation = p.generation();
    let _ = p.close(window::Id::unique());
    assert_eq!(p.creation, CreationState::Closed);
    assert_eq!(p.generation(), generation + 1);
    assert!(!p.is_live());
    assert!(!p.shown);
}

#[test]
fn a_message_from_an_old_generation_is_dropped() {
    let mut p = live_panel();
    let before = p.bridge.doc().text.clone();
    let v = p.bridge.doc().version;
    let stale = page(
        &p,
        json!({"t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}),
    );
    // the panel was closed and reopened: the epoch moved on
    p.epoch.fetch_add(1, Ordering::SeqCst);
    let _ = p.update(Message::Page(0, stale));
    assert_eq!(
        p.bridge.doc().text,
        before,
        "a stale generation must never be applied"
    );
    assert_eq!(p.bridge.doc().version, v);
}

#[test]
fn a_message_for_the_current_generation_is_applied() {
    let mut p = live_panel();
    let v = p.bridge.doc().version;
    let edit = page(
        &p,
        json!({"t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}),
    );
    let g = p.generation();
    let _ = p.update(Message::Page(g, edit));
    assert_eq!(p.bridge.doc().version, v + 1);
}

#[test]
fn a_message_is_dropped_when_there_is_no_webview() {
    let mut p = MarkdownPanel::with_mode(Mode::Normal);
    let v = p.bridge.doc().version;
    let edit = page(
        &p,
        json!({"t": "edit", "base": v, "changes": [{"from": 0, "to": 0, "insert": "x"}]}),
    );
    let _ = p.update(Message::Page(0, edit));
    assert_eq!(p.bridge.doc().version, v);
}

#[test]
fn ready_opens_the_document_and_enables_the_agent_stand_in() {
    let mut p = live_panel();
    assert!(!p.ready);
    let g = p.generation();
    let _ = p.update(Message::Page(g, page(&p, json!({"t": "ready"}))));
    assert!(p.ready);
}

#[test]
fn the_stand_in_does_nothing_before_the_page_is_ready() {
    let mut p = live_panel();
    let before = p.bridge.doc().text.clone();
    let _ = p.update(Message::StandIn(StandIn::Append));
    assert_eq!(p.bridge.doc().text, before);
}

#[test]
fn the_stand_in_rewrites_a_different_paragraph_each_time() {
    let mut p = live_panel();
    let g = p.generation();
    let _ = p.update(Message::Page(g, page(&p, json!({"t": "ready"}))));
    let t0 = p.bridge.doc().text.clone();
    let _ = p.update(Message::StandIn(StandIn::RewriteParagraph(0)));
    let t1 = p.bridge.doc().text.clone();
    let _ = p.update(Message::StandIn(StandIn::RewriteParagraph(0)));
    let t2 = p.bridge.doc().text.clone();
    assert!(t0 != t1 && t1 != t2 && t0 != t2);
}

#[test]
fn a_refused_message_is_counted_and_shown_not_hidden() {
    let mut p = live_panel();
    let g = p.generation();
    let _ = p.update(Message::Page(g, "not json".into()));
    assert_eq!(p.rejects, 1);
    assert!(
        p.last_reject
            .as_deref()
            .is_some_and(|r| r.contains("unparsable"))
    );
}

#[test]
fn the_webview_agent_hook_is_refused_in_normal_operation() {
    let mut p = live_panel();
    let g = p.generation();
    let _ = p.update(Message::Page(g, page(&p, json!({"t": "ready"}))));
    let before = p.bridge.doc().text.clone();
    let _ = p.update(Message::Page(
        g,
        page(&p, json!({"t": "agent", "op": "append", "text": "pwned"})),
    ));
    assert_eq!(p.bridge.doc().text, before);
    assert_eq!(p.rejects, 1);
}

#[test]
fn the_panel_opens_the_sample_chapter() {
    let p = MarkdownPanel::with_mode(Mode::Normal);
    assert!(p.bridge.doc().text.starts_with("# 第一章 开端"));
}

#[test]
fn the_panel_owns_its_webview_table_and_names_no_other_panels() {
    // independence: the module depends on no other panel and on no application-host crate
    for (name, src) in [
        ("mod.rs", include_str!("mod.rs")),
        ("host.rs", include_str!("host.rs")),
    ] {
        let code = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            "crate::apps",
            "crate::adapters",
            "digger_app_host",
            "bytehost_apps",
            "bytehost_client",
            "bytehost_panel",
            "crate::product",
        ] {
            assert!(
                !code
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .any(|l| l.contains(banned)),
                "{name} must not depend on {banned}"
            );
        }
    }
}
