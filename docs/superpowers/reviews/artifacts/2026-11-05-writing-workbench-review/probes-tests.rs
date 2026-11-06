#[test]
fn review_probe_open_document_drops_dirty_buffer_without_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut seed = workbench::Workbench::new();
    seed.open_project(dir.path().to_path_buf()).unwrap();
    let a = seed.create("A", "a.md").unwrap();
    let b = seed.create("B", "b.md").unwrap();
    let mut panel = MarkdownPanel::with_mode(Mode::Normal);
    panel.set_project(Some(dir.path().to_path_buf()));
    let _ = panel.update(Message::OpenDocument(a.id.clone()));
    panel.bridge.replace_document(None, "UNSAVED_A");
    panel.workbench.mark_dirty(2);
    let _ = panel.update(Message::OpenDocument(b.id));
    assert_ne!(panel.bridge.doc().text, "UNSAVED_A");
    let store = digger_store::writing::WritingStore::open(dir.path()).unwrap();
    assert_ne!(store.load(&a.id).unwrap().snapshot.text,"UNSAVED_A");
    let recovery = digger_store::writing::RecoveryStore::open(dir.path()).unwrap();
    assert!(recovery.list_document(&a.id).unwrap().is_empty());
}
