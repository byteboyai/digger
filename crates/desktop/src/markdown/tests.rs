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
    live_panel_with(Mode::Normal)
}

fn live_panel_with(mode: Mode) -> MarkdownPanel {
    let mut p = MarkdownPanel::with_mode(mode);
    p.creation = CreationState::Ready {
        generation: p.generation(),
    };
    p
}

fn sample_panel() -> MarkdownPanel {
    let mut p = live_panel();
    p.bridge.replace_document(None, SAMPLE);
    p
}

fn run_task(panel: &mut MarkdownPanel, task: iced::Task<Message>) {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return;
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                let next = panel.update(message);
                if let Some(next) = iced_runtime::task::into_stream(next) {
                    stream = Box::pin(stream.chain(next));
                }
            }
        }
    });
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
    let mut p = sample_panel();
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
fn normal_mode_starts_empty_and_selftest_keeps_its_sample_chapter() {
    let normal = MarkdownPanel::with_mode(Mode::Normal);
    assert!(normal.bridge.doc().text.is_empty());
    let selftest = MarkdownPanel::with_mode(Mode::SelfTest);
    assert!(selftest.bridge.doc().text.starts_with("# 第一章 开端"));
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

#[test]
fn a_project_document_can_be_created_saved_and_reopened_through_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    assert!(panel.workbench.current().is_none());
    let _ = panel.update(Message::NewTitleChanged("第一章".into()));
    let _ = panel.update(Message::NewPathChanged("book/01.md".into()));
    let task = panel.update(Message::Create);
    run_task(&mut panel, task);
    assert_eq!(panel.workbench.current().unwrap().document.title, "第一章");
    let task = panel.start_save_after_flush();
    run_task(&mut panel, task);
    assert!(matches!(panel.workbench.status(), WorkbenchStatus::Saved));
    assert!(dir.path().join("book/01.md").exists());

    let mut reopened = MarkdownPanel::with_mode(Mode::Normal);
    reopened.set_project(Some(dir.path().to_path_buf()));
    reopened.settle_load_for_test();
    assert_eq!(
        reopened.workbench.current().unwrap().document.title,
        "第一章"
    );
    assert!(reopened.bridge.doc().text.is_empty());
}

#[test]
fn a_recovery_tick_writes_an_atomic_draft_for_the_active_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    let _ = panel.update(Message::NewTitleChanged("草稿".into()));
    let _ = panel.update(Message::NewPathChanged("draft.md".into()));
    let task = panel.update(Message::Create);
    run_task(&mut panel, task);
    let doc = panel.workbench.current().unwrap().document.id.clone();
    panel.workbench.mark_dirty(1);
    let task = panel.update(Message::RecoveryTick);
    run_task(&mut panel, task);
    let working = dir.path().join(".digger/working").join(doc.as_str());
    let files = std::fs::read_dir(&working)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].extension().is_some_and(|ext| ext == "json"));
    assert!(!files[0].with_extension("json.tmp").exists());
}

#[test]
fn switching_documents_persists_a_dirty_buffer_as_recovery() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut seed = workbench::Workbench::new();
        seed.open_project(dir.path().to_path_buf()).unwrap();
        seed.create("A", "a.md").unwrap();
        seed.create("B", "b.md").unwrap();
    }
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    let a = panel.workbench.documents()[0].id.clone();
    let b = panel.workbench.documents()[1].id.clone();
    let task = panel.update(Message::OpenDocument(a.clone()));
    run_task(&mut panel, task);
    panel.bridge.replace_document(None, "UNSAVED_A");
    panel.workbench.mark_dirty(9);
    let task = panel.update(Message::OpenDocument(b));
    run_task(&mut panel, task);
    let task = panel.update(Message::TransitionKeep);
    run_task(&mut panel, task);
    assert_ne!(panel.bridge.doc().text, "UNSAVED_A");
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    assert!(
        !recovery.list_document(&a).unwrap().is_empty(),
        "dirty A must leave a recovery draft before B replaces it"
    );
}

#[test]
fn save_waits_for_the_page_flush_and_does_not_commit_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = live_panel();
    panel.ready = true;
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    let _ = panel.update(Message::NewTitleChanged("A".into()));
    let _ = panel.update(Message::NewPathChanged("a.md".into()));
    let task = panel.update(Message::Create);
    run_task(&mut panel, task);
    panel.ready = true;
    panel.creation = CreationState::Ready {
        generation: panel.generation(),
    };
    let _ = panel.update(Message::Save);
    assert!(
        panel.workbench.current().unwrap().document.revision == 1,
        "no commit before flushed"
    );
    assert!(panel.pending_flush.is_some());

    let g = panel.generation();
    let version = panel.bridge.doc().version;
    let flushed = page(
        &panel,
        json!({"t": "flushed", "request_id": 1, "version": version}),
    );
    let task = panel.update(Message::Page(g, flushed));
    run_task(&mut panel, task);
    assert!(matches!(panel.workbench.status(), WorkbenchStatus::Saved));
    assert!(dir.path().join("a.md").exists());
    assert!(panel.pending_flush.is_none());
}

#[test]
fn a_flush_timeout_keeps_the_document_dirty_and_visible() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = live_panel();
    panel.ready = true;
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    let _ = panel.update(Message::NewTitleChanged("A".into()));
    let _ = panel.update(Message::NewPathChanged("a.md".into()));
    let task = panel.update(Message::Create);
    run_task(&mut panel, task);
    panel.ready = true;
    let _ = panel.update(Message::Save);
    let request_id = panel.pending_flush.unwrap();
    let _ = panel.update(Message::FlushTimeout(request_id));
    assert!(panel.pending_flush.is_none());
    assert!(matches!(
        panel.workbench.status(),
        WorkbenchStatus::SaveFailed(_)
    ));
    assert_eq!(panel.workbench.current().unwrap().document.revision, 1);
    assert!(panel.workbench.current().is_some());
}

#[test]
fn a_project_switch_failure_keeps_the_old_session_and_reports_the_error() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    {
        let mut seed = workbench::Workbench::new();
        seed.open_project(a.path().to_path_buf()).unwrap();
        seed.create("A", "a.md").unwrap();
    }
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(a.path().to_path_buf()));
    panel.settle_load_for_test();
    panel.bridge.replace_document(None, "UNSAVED_A");
    panel.workbench.mark_dirty(3);
    let a_id = panel.workbench.current().unwrap().document.id.clone();
    // Make the recovery directory unusable: the switch must not discard the buffer.
    let working = a.path().join(".digger/working");
    let _ = std::fs::remove_dir_all(&working);
    std::fs::write(&working, "not a directory").unwrap();

    panel.set_project(Some(b.path().to_path_buf()));
    panel.settle_load_for_test();
    assert_eq!(
        panel.workbench.project_root(),
        Some(a.path()),
        "failed persistence must keep the old project"
    );
    assert_eq!(
        panel.workbench.current().unwrap().document.id,
        a_id,
        "failed persistence must keep the old document"
    );
    assert!(panel.last_error.is_some());
}

impl MarkdownPanel {
    pub(crate) fn test_edit_text(&mut self, text: &str) {
        self.ready = true;
        self.creation = CreationState::Ready {
            generation: self.generation(),
        };
        let raw = page(
            self,
            json!({"t":"edit", "base":self.bridge.doc().version,
            "changes":[{"from":0,"to":self.bridge.doc().len16(),"insert":text}]}),
        );
        let _ = self.update(Message::Page(self.generation(), raw));
    }

    pub(crate) fn test_flush_reply(&self) -> Message {
        Message::Page(
            self.generation(),
            page(
                self,
                json!({"t":"flushed",
            "request_id":self.pending_flush.unwrap(),"version":self.bridge.doc().version}),
            ),
        )
    }
}

fn writing_pair() -> (
    tempfile::TempDir,
    MarkdownPanel,
    digger_store::writing::DocumentId,
    digger_store::writing::DocumentId,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut seed = workbench::Workbench::new();
    seed.open_project(dir.path().to_path_buf()).unwrap();
    let a = seed.create("A", "a.md").unwrap().id;
    let b = seed.create("B", "b.md").unwrap().id;
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(dir.path().to_path_buf()));
    panel.settle_load_for_test();
    let task = panel.update(Message::OpenDocument(a.clone()));
    run_task(&mut panel, task);
    (dir, panel, a, b)
}

#[test]
fn late_save_completion_cannot_modify_another_document() {
    let (_dir, mut panel, _a, b) = writing_pair();
    panel.test_edit_text("A saved");
    let task = panel.start_save_after_flush();
    // Simulate the obsolete completion after an independent session replacement.
    // Normal user transitions are blocked while the save is pending as well.
    let loaded = panel.workbench.open_document(&b).unwrap();
    panel.load_document_into_bridge(&loaded);
    run_task(&mut panel, task);
    let current = panel.workbench.current().unwrap();
    assert_eq!(current.document.id, b);
    assert_eq!(current.document.revision, 1);
    assert_eq!(current.snapshot.text, "");
    assert_eq!(panel.bridge.doc().text, "");
    assert!(panel.pending_save.is_none());
}

#[test]
fn save_completion_keeps_later_input_dirty_and_preserves_its_recovery() {
    let (dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("saved snapshot");
    let task = panel.start_save_after_flush();
    panel.test_edit_text("newer unsaved input");
    panel.persist_transition_draft().unwrap();
    // Dirty must not allow a second overlapping save to bypass the busy check.
    let _ = panel.update(Message::Save);
    assert!(panel.pending_flush.is_none());
    run_task(&mut panel, task);
    assert!(panel.workbench.current().unwrap().dirty);
    assert!(matches!(panel.workbench.status(), WorkbenchStatus::Dirty));
    assert_eq!(panel.bridge.doc().text, "newer unsaved input");
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(store.load(&a).unwrap().snapshot.text, "saved snapshot");
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    let files = recovery.list_document(&a).unwrap();
    assert!(files.iter().any(|file| {
        let draft: digger_store::writing::RecoveryDraft =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        draft.text == "newer unsaved input"
    }));
}

#[test]
fn transition_flushes_even_a_clean_rust_buffer_before_switching() {
    let (dir, mut panel, a, b) = writing_pair();
    panel.ready = true;
    panel.creation = CreationState::Ready {
        generation: panel.generation(),
    };
    let _ = panel.update(Message::OpenDocument(b.clone()));
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    // A page edit that was still queued when the user selected B arrives before
    // its flush acknowledgement. It must belong to A's durable recovery.
    panel.test_edit_text("final queued input");
    let reply = panel.test_flush_reply();
    let task = panel.update(reply);
    run_task(&mut panel, task);
    let task = panel.update(Message::TransitionKeep);
    run_task(&mut panel, task);
    assert_eq!(panel.workbench.current().unwrap().document.id, b);
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    let file = recovery.list_document(&a).unwrap().pop().unwrap();
    let draft: digger_store::writing::RecoveryDraft =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(draft.text, "final queued input");
}

#[test]
fn delayed_recovery_cannot_overwrite_a_newer_transition_draft() {
    let (dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("old draft");
    let state = panel.bridge.export_snapshot();
    let plan = panel
        .workbench
        .prepare_recovery(
            &state.text,
            &serde_json::to_string(&state).unwrap(),
            1,
            state.version,
        )
        .unwrap();
    panel.bridge.restore_history(state.history);
    panel.test_edit_text("new transition draft");
    let _ = panel.update(Message::OpenDocument(b.clone()));
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    let task = panel.update(Message::TransitionKeep);
    run_task(&mut panel, task);
    plan.run().unwrap();
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    let file = recovery.list_document(&a).unwrap().pop().unwrap();
    let draft: digger_store::writing::RecoveryDraft =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(draft.text, "new transition draft");
    assert_eq!(panel.workbench.current().unwrap().document.id, b);
}

#[test]
fn transition_timeout_cancels_the_action_and_keeps_the_original_document() {
    let (_dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("keep me");
    let _ = panel.update(Message::OpenDocument(b));
    let request = panel.pending_flush.unwrap();
    let _ = panel.update(Message::FlushTimeout(request));
    assert!(panel.pending_transition.is_none());
    assert!(panel.pending_flush.is_none());
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    assert_eq!(panel.bridge.doc().text, "keep me");
}

#[test]
fn transition_cancel_keeps_dirty_text_and_does_not_commit_or_switch() {
    let (dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("unsaved A");
    let _ = panel.update(Message::OpenDocument(b));
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    assert!(panel.transition_choice);
    let task = panel.update(Message::TransitionCancel);
    run_task(&mut panel, task);
    assert!(!panel.transition_choice);
    assert!(panel.pending_transition.is_none());
    assert_eq!(panel.bridge.doc().text, "unsaved A");
    assert_eq!(panel.workbench.current().unwrap().document.id, a.clone());
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(store.load(&a).unwrap().snapshot.text, "");
}

#[test]
fn transition_save_commits_final_buffer_before_loading_the_next_document() {
    let (dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("saved before switch");
    let _ = panel.update(Message::OpenDocument(b.clone()));
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    assert!(panel.transition_choice);
    let task = panel.update(Message::TransitionSave);
    run_task(&mut panel, task);
    assert_eq!(panel.workbench.current().unwrap().document.id, b);
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(store.load(&a).unwrap().snapshot.text, "saved before switch");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "saved before switch"
    );
}

#[test]
fn a_stale_project_load_cannot_replace_the_current_project() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(a.path().to_owned()));
    panel.set_project(Some(b.path().to_owned()));
    panel.settle_load_for_test();
    assert_eq!(panel.workbench.project_root(), Some(b.path()));
}

#[test]
fn cancelled_transition_reply_cannot_complete_a_new_flush() {
    let (_dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("keep A");
    let _ = panel.update(Message::OpenDocument(b.clone()));
    let old_generation = panel.transition_generation;
    let old_identity = panel.workbench.identity().unwrap();
    let old_version = panel.bridge.doc().version;
    let task = panel.update(Message::TransitionCancel);
    run_task(&mut panel, task);
    let _ = panel.update(Message::OpenDocument(b));
    let request = panel.pending_flush;
    let task = panel.update(Message::TransitionPersisted(
        old_generation,
        old_identity,
        old_version,
        Ok(()),
    ));
    run_task(&mut panel, task);
    assert_eq!(panel.pending_flush, request);
    assert!(panel.pending_transition.is_some());
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    let _ = panel.update(Message::TransitionCancel);
}

#[test]
fn load_failure_keeps_the_dirty_document_and_a_new_attempt_can_succeed() {
    let (_dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("keep this buffer");
    let _ = panel.update(Message::OpenFile(std::path::PathBuf::from(
        "/outside/missing.md",
    )));
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    let task = panel.update(Message::TransitionKeep);
    run_task(&mut panel, task);
    assert!(!panel.loading);
    assert!(panel.last_error.is_some());
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    assert_eq!(panel.bridge.doc().text, "keep this buffer");
    assert!(panel.workbench.current().unwrap().dirty);
}

#[test]
fn old_worker_replies_do_not_clear_a_new_operation_or_its_notice() {
    let (_dir, mut panel, _a, _b) = writing_pair();
    let identity = panel.workbench.identity().unwrap();
    panel.recovery_operation = 10;
    panel.recovery_busy = true;
    let _ = panel.update(Message::RecoveryFinished(9, identity, Ok(())));
    assert!(panel.recovery_busy);
    panel.mirror_operation = 10;
    panel.mirror_busy = true;
    panel
        .workbench
        .set_mirror_notice(Some("current notice".into()));
    let _ = panel.update(Message::MirrorsChecked(9, Ok(None)));
    assert!(panel.mirror_busy);
    assert_eq!(panel.workbench.mirror_error(), Some("current notice"));
}

#[test]
fn recovering_then_saving_preserves_nonempty_aliases_and_pending_history_across_restart() {
    let (dir, mut panel, a, _b) = writing_pair();
    // Rich identity fixture through the production pure Doc/Bridge path; this
    // does not expose an Agent action in normal W1 product UI.
    panel
        .bridge
        .replace_document(None, "# Title\n\noriginal paragraph\n\nsecond paragraph\n");
    panel.bridge.stand_in(StandIn::RewriteParagraph(0)).unwrap();
    let expected = panel.bridge.export_snapshot();
    assert!(!expected.aliases.is_empty());
    assert!(!expected.history.is_empty());
    panel.bridge.restore_history(expected.history.clone());
    panel.workbench.mark_dirty(expected.version);
    panel.persist_transition_draft().unwrap();
    drop(panel);
    let mut recovered = MarkdownPanel::with_mode(Mode::Normal);
    recovered.set_project(Some(dir.path().to_owned()));
    recovered.settle_load_for_test();
    assert!(recovered.workbench.recovery_pending().is_some());
    let task = recovered.update(Message::AcceptRecovery);
    run_task(&mut recovered, task);
    recovered.ready = true;
    recovered.creation = CreationState::Ready {
        generation: recovered.generation(),
    };
    let _ = recovered.update(Message::Save);
    let task = recovered.update(recovered.test_flush_reply());
    run_task(&mut recovered, task);
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    let saved = store.load(&a).unwrap();
    let state: digger_editor::doc::State =
        serde_json::from_str(&saved.snapshot.state_json).unwrap();
    assert_eq!(state.ids, expected.ids);
    assert_eq!(state.next_id, expected.next_id);
    assert_eq!(state.aliases, expected.aliases);
    assert_eq!(state.history, expected.history);
    assert_eq!(state.runs, expected.runs);
    assert_eq!(state.text, expected.text);
    assert_eq!(saved.document.revision, 2);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        expected.text
    );
    drop(recovered);
    drop(store);
    let mut reopened = MarkdownPanel::with_mode(Mode::Normal);
    reopened.set_project(Some(dir.path().to_owned()));
    reopened.settle_load_for_test();
    assert_eq!(reopened.workbench.current().unwrap().document.id, a);
    let restored = reopened.bridge.export_snapshot();
    assert_eq!(restored.ids, expected.ids);
    assert_eq!(restored.aliases, expected.aliases);
    assert!(
        restored.history.is_empty(),
        "committed history must not become pending again on normal reopen"
    );
}

#[test]
fn an_admitted_save_settles_after_the_panel_and_its_future_are_dropped() {
    let (dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("admitted before window disappearance");
    let task = panel.start_save_after_flush();
    let (tx, rx) = std::sync::mpsc::channel();
    panel
        .io
        .dispatch(move || {
            tx.send(()).unwrap();
        })
        .unwrap();
    drop(task);
    drop(panel);
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        store.load(&a).unwrap().snapshot.text,
        "admitted before window disappearance"
    );
    assert_eq!(store.load(&a).unwrap().document.revision, 2);
}

#[test]
fn corrupt_recovery_stays_visible_and_intact_while_the_official_document_remains_editable() {
    let (dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("unfinished draft");
    panel.persist_transition_draft().unwrap();
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    let path = recovery.list_document(&a).unwrap().pop().unwrap();
    std::fs::write(&path, b"{corrupt recovery").unwrap();
    drop(panel);
    let mut reopened = MarkdownPanel::with_mode(Mode::Normal);
    reopened.set_project(Some(dir.path().to_owned()));
    reopened.settle_load_for_test();
    assert!(reopened.workbench.recovery_warning().is_some());
    assert!(reopened.workbench.recovery_pending().is_none());
    assert_eq!(reopened.workbench.current().unwrap().document.id, a.clone());
    reopened.test_edit_text("official can still be edited");
    let task = reopened.start_save_after_flush();
    run_task(&mut reopened, task);
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        store.load(&a).unwrap().snapshot.text,
        "official can still be edited"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{corrupt recovery");
    assert!(reopened.workbench.recovery_warning().is_some());
}

/// Explicit performance run; no timing threshold or GUI/IPC claim. Exercises
/// the production bounded queue with a real recovery write ahead of SavePlan.
#[test]
#[ignore = "manual backend queue samples: --ignored --nocapture"]
fn writing_queue_performance_samples() {
    for blocks in [100, 1000, 5000] {
        for sample in 0..5 {
            let dir = tempfile::tempdir().unwrap();
            let mut panel = MarkdownPanel::with_mode(Mode::Normal);
            panel.set_project(Some(dir.path().to_owned()));
            panel.settle_load_for_test();
            let _ = panel.update(Message::NewTitleChanged("benchmark".into()));
            let _ = panel.update(Message::NewPathChanged("chapter.md".into()));
            let task = panel.update(Message::Create);
            run_task(&mut panel, task);
            let text = (0..blocks)
                .map(|i| format!("第{i}段 mixed text 😀。\n\n"))
                .collect::<String>();
            panel.test_edit_text(&text);
            let recovery_task = panel.write_recovery_now();
            let state = panel.bridge.export_snapshot();
            let snapshot = WritingSnapshot::new(
                state.text.clone(),
                serde_json::to_string(&state).unwrap(),
                1,
            );
            let plan = panel.workbench.prepare_save(snapshot).unwrap();
            let queued_at = std::time::Instant::now();
            let receiver = panel
                .io
                .submit(move || {
                    let queue_us = queued_at.elapsed().as_micros();
                    let start = std::time::Instant::now();
                    let outcome = plan.run().unwrap();
                    (outcome, queue_us, start.elapsed().as_micros())
                })
                .unwrap();
            let (outcome, queue_us, save_plan_us) = receiver.blocking_recv().unwrap().unwrap();
            panel.workbench.apply_save(outcome);
            run_task(&mut panel, recovery_task);
            assert_eq!(
                std::fs::read_to_string(dir.path().join("chapter.md")).unwrap(),
                text
            );
            println!(
                "WRITING_QUEUE_SAMPLE {}",
                serde_json::json!({"blocks":blocks,"sample":sample,"profile":"test-debug","queue_us":queue_us,"save_plan_us":save_plan_us,"load":"one real recovery write ahead of SavePlan","ipc_paint_measured":false})
            );
        }
    }
}

struct HeldWritingIo {
    release: Option<std::sync::mpsc::Sender<()>>,
    done: Option<tokio::sync::oneshot::Receiver<Result<(), String>>>,
}
impl HeldWritingIo {
    fn new(panel: &MarkdownPanel) -> Self {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let done = panel
            .io
            .submit(move || {
                entered_tx.send(()).unwrap();
                wait.recv().unwrap();
            })
            .unwrap();
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Self {
            release: Some(release),
            done: Some(done),
        }
    }
}
impl Drop for HeldWritingIo {
    fn drop(&mut self) {
        let _ = self.release.take().unwrap().send(());
        self.done.take().unwrap().blocking_recv().unwrap().unwrap();
    }
}

#[test]
fn slow_load_rejects_old_page_edits_before_and_after_the_new_document_opens() {
    let (_dir, mut panel, a, b) = writing_pair();
    panel.test_edit_text("saved A");
    let task = panel.start_save_after_flush();
    run_task(&mut panel, task);
    let old_generation = panel.generation();
    let hold = HeldWritingIo::new(&panel);
    let _ = panel.update(Message::OpenDocument(b.clone()));
    let task = panel.update(panel.test_flush_reply());
    assert!(panel.loading);
    let old_page_edit = page(
        &panel,
        json!({"t":"edit","base":panel.bridge.doc().version,"changes":[{"from":0,"to":0,"insert":"stale"}]}),
    );
    let _ = panel.update(Message::Page(old_generation, old_page_edit));
    assert_eq!(panel.bridge.doc().text, "saved A");
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    drop(hold);
    run_task(&mut panel, task);
    assert_eq!(panel.workbench.current().unwrap().document.id, b);
    let old_page_edit = page(
        &panel,
        json!({"t":"edit","base":panel.bridge.doc().version,"changes":[{"from":0,"to":0,"insert":"stale after load"}]}),
    );
    let _ = panel.update(Message::Page(old_generation, old_page_edit));
    assert_eq!(panel.bridge.doc().text, "");
    assert!(!panel.workbench.current().unwrap().dirty);
}

#[test]
fn cancelled_slow_load_and_its_late_failure_cannot_replace_the_next_session() {
    let (_dir, mut panel, a, b) = writing_pair();
    let hold = HeldWritingIo::new(&panel);
    let first = panel.update(Message::OpenDocument(b));
    let obsolete = panel.load_ticket;
    assert!(panel.loading);
    let _ = panel.update(Message::CancelLoad);
    assert!(!panel.loading);
    let second = panel.update(Message::OpenDocument(a.clone()));
    assert!(panel.loading);
    drop(hold);
    run_task(&mut panel, first);
    assert!(
        panel.loading,
        "obsolete completion must not settle the new load"
    );
    run_task(&mut panel, second);
    let current = panel.workbench.identity();
    let _ = panel.update(Message::Loaded(
        obsolete,
        Err("late obsolete failure".into()),
    ));
    assert_eq!(panel.workbench.identity(), current);
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    assert!(panel.last_error.is_none());
}

#[test]
fn replacing_a_project_store_during_an_admitted_save_keeps_the_new_ui_clean() {
    let (dir, mut old_panel, _a, _b) = writing_pair();
    old_panel.test_edit_text("old admitted snapshot");
    let hold = HeldWritingIo::new(&old_panel);
    let old_save = old_panel.start_save_after_flush();
    let source = tempfile::tempdir().unwrap();
    let store = digger_store::writing::WritingStore::open(source.path()).unwrap();
    let mut fresh_doc = digger_editor::doc::Doc::new();
    fresh_doc.load("new store official");
    let state = fresh_doc.export_state();
    let created = store
        .create(digger_store::writing::NewDocument {
            title: "replacement".into(),
            relative_path: "replacement.md".into(),
            snapshot: WritingSnapshot::new(
                state.text.clone(),
                serde_json::to_string(&state).unwrap(),
                1,
            ),
            newline: digger_store::writing::NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap();
    let replacement_id = match created {
        digger_store::writing::CreateOutcome::Created(doc) => doc.id,
        _ => panic!(),
    };
    let backup = source.path().join("backup");
    digger_store::writing::backup_writing(&store, &backup).unwrap();
    // Move the complete old SQLite island, including its WAL/SHM, rather than
    // forming an invalid mixture of an old WAL and a different main DB.
    std::fs::rename(
        dir.path().join(".digger"),
        dir.path().join(".digger-retired"),
    )
    .unwrap();
    std::fs::rename(backup.join(".digger"), dir.path().join(".digger")).unwrap();
    drop(old_panel);
    let mut new_panel = MarkdownPanel::with_mode(Mode::Normal);
    new_panel.set_project(Some(dir.path().to_owned()));
    new_panel.settle_load_for_test();
    let identity = new_panel.workbench.identity();
    drop(hold);
    run_task(&mut new_panel, old_save);
    assert_eq!(new_panel.workbench.identity(), identity);
    assert_eq!(
        new_panel.workbench.current().unwrap().document.id,
        replacement_id
    );
    assert_eq!(new_panel.bridge.doc().text, "new store official");
    assert!(new_panel.last_error.is_none());
    let reopened = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.load(&replacement_id).unwrap().snapshot.text,
        "new store official"
    );
}

#[test]
fn native_hide_theme_and_retry_preserve_the_active_dirty_document() {
    let (_dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("dirty across native lifecycle");
    let identity = panel.workbench.identity();
    let version = panel.bridge.doc().version;
    let window = window::Id::unique();
    panel.shown = true;
    let generation = panel.generation();
    let _ = panel.sync_native(window, false, rect());
    let _ = panel.sync_theme();
    let _ = panel.sync_native(window, true, rect());
    assert_eq!(panel.generation(), generation);
    panel.creation = CreationState::Failed {
        generation,
        error: "controlled creation failure".into(),
    };
    let _ = panel.update(Message::Retry);
    let retry_generation = panel.generation();
    assert!(retry_generation > generation);
    let _ = panel.update(Message::Built(generation, Err("obsolete failure".into())));
    assert!(matches!(panel.creation, CreationState::Building { .. }));
    let _ = panel.update(Message::Built(retry_generation, Ok(())));
    let raw = page(&panel, json!({"t":"ready"}));
    let _ = panel.update(Message::Page(retry_generation, raw));
    assert_eq!(panel.workbench.identity(), identity);
    assert_eq!(panel.workbench.current().unwrap().document.id, a);
    assert_eq!(panel.bridge.doc().text, "dirty across native lifecycle");
    assert_eq!(panel.bridge.doc().version, version);
    assert!(panel.workbench.current().unwrap().dirty);
}

#[test]
fn a_sqlite_write_rejection_leaves_the_buffer_editable_and_save_can_be_retried() {
    let (dir, mut panel, a, _b) = writing_pair();
    let conn = rusqlite::Connection::open(digger_store::writing::WritingStore::database_path(
        dir.path(),
    ))
    .unwrap();
    conn.execute_batch("CREATE TRIGGER reject_writing_fixture BEFORE INSERT ON document_versions WHEN NEW.document_id=(SELECT id FROM documents WHERE relative_path='a.md') BEGIN SELECT RAISE(ABORT,'controlled SQLite write rejection'); END;").unwrap();
    panel.test_edit_text("buffer survives write rejection");
    let _ = panel.update(Message::Save);
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    assert!(matches!(
        panel.workbench.status(),
        WorkbenchStatus::SaveFailed(_)
    ));
    assert!(panel.last_error.is_some());
    assert!(panel.pending_save.is_none());
    assert_eq!(panel.workbench.current().unwrap().document.id, a.clone());
    assert_eq!(panel.workbench.current().unwrap().document.revision, 1);
    panel.test_edit_text("new input after the failed save");
    conn.execute_batch("DROP TRIGGER reject_writing_fixture;")
        .unwrap();
    let _ = panel.update(Message::Save);
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    assert!(matches!(panel.workbench.status(), WorkbenchStatus::Saved));
    assert!(panel.last_error.is_none());
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        store.load(&a).unwrap().snapshot.text,
        "new input after the failed save"
    );
    assert_eq!(store.load(&a).unwrap().document.revision, 2);
}

#[test]
fn a_corrupt_target_document_does_not_displace_the_dirty_source_or_prevent_its_save() {
    let (dir, mut panel, a, b) = writing_pair();
    let conn = rusqlite::Connection::open(digger_store::writing::WritingStore::database_path(
        dir.path(),
    ))
    .unwrap();
    conn.execute(
        "UPDATE document_versions SET digest='damaged-fixture' WHERE document_id=?1",
        rusqlite::params![b.as_str()],
    )
    .unwrap();
    panel.test_edit_text("source is still editable");
    let _ = panel.update(Message::OpenDocument(b));
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    let task = panel.update(Message::TransitionKeep);
    run_task(&mut panel, task);
    assert!(!panel.loading);
    assert!(panel.last_error.is_some());
    assert_eq!(panel.workbench.current().unwrap().document.id, a.clone());
    assert_eq!(panel.bridge.doc().text, "source is still editable");
    assert!(panel.workbench.current().unwrap().dirty);
    let _ = panel.update(Message::Save);
    let task = panel.update(panel.test_flush_reply());
    run_task(&mut panel, task);
    assert!(matches!(panel.workbench.status(), WorkbenchStatus::Saved));
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        store.load(&a).unwrap().snapshot.text,
        "source is still editable"
    );
}

#[test]
fn an_expired_external_confirmation_keeps_the_dirty_buffer_and_allows_a_fresh_copy() {
    let (dir, mut panel, a, _b) = writing_pair();
    panel.test_edit_text("unsaved author input");
    let source = dir.path().join("a.md");
    std::fs::write(&source, "external first").unwrap();
    let task = panel.update(Message::ReviewExternal);
    run_task(&mut panel, task);
    assert!(panel.external_review.is_some());
    std::fs::write(&source, "external second").unwrap();
    let task = panel.update(Message::ResolveExternal(false));
    run_task(&mut panel, task);
    assert!(panel.last_error.is_some());
    assert!(panel.external_review.is_some());
    assert!(!panel.mirror_busy);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "external second");
    assert_eq!(panel.bridge.doc().text, "unsaved author input");
    let task = panel.update(Message::ReviewExternal);
    run_task(&mut panel, task);
    let task = panel.update(Message::ResolveExternal(true));
    run_task(&mut panel, task);
    assert!(panel.last_error.is_none());
    assert!(panel.external_review.is_none());
    assert_eq!(panel.workbench.current().unwrap().document.id, a.clone());
    assert!(panel.workbench.current().unwrap().dirty);
    assert_eq!(panel.bridge.doc().text, "unsaved author input");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "external second");
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    let copy = store
        .list()
        .unwrap()
        .into_iter()
        .find(|doc| doc.title.ends_with("（外部副本）"))
        .unwrap();
    assert_eq!(
        store.load(&copy.id).unwrap().snapshot.text,
        "external second"
    );
    assert_eq!(store.load(&a).unwrap().snapshot.text, "");
}
