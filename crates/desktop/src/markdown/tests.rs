use super::*;
use digger_editor::bridge::Mode;
use serde_json::json;

fn page(panel: &MarkdownPanel, v: serde_json::Value) -> String {
    let mut v = v;
    v["n"] = json!(panel.nonce);
    v.to_string()
}

fn live_panel() -> MarkdownPanel {
    let mut p = MarkdownPanel::with_mode(Mode::Normal);
    p.live = true;
    p
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
