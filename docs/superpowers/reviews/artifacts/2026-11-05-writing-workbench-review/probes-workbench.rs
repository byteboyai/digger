#[cfg(test)]
mod review_probes {
    use super::*;
    #[test]
    fn review_probe_first_import_mirror_overwrites_external_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "source").unwrap();
        let mut w = Workbench::new();
        w.open_project(dir.path().to_path_buf()).unwrap();
        w.import("a.md").unwrap();
        std::fs::write(dir.path().join("a.md"), "EXTERNAL_NEW").unwrap();
        w.save(snapshot_for("APP_NEW").1).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.md")).unwrap(), "APP_NEW");
        assert!(w.mirror_error().is_none());
    }
    #[test]
    fn review_probe_recovered_session_reuses_old_operation_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Workbench::new();
        w.open_project(dir.path().to_path_buf()).unwrap();
        let d = w.create("A", "a.md").unwrap();
        w.save(snapshot_for("official").1).unwrap();
        let (state, _) = snapshot_for("draft");
        w.write_recovery("draft", &serde_json::to_string(&state).unwrap(), 1, 2).unwrap();
        let mut reopened = Workbench::new();
        reopened.open_project(dir.path().to_path_buf()).unwrap();
        reopened.open_document(&d.id).unwrap();
        let (state, text) = reopened.accepted_recovery().unwrap();
        let result = reopened.save(WritingSnapshot::new(text, serde_json::to_string(&state).unwrap(), 1));
        assert!(matches!(result, Err(WritingError::OperationConflict { .. })), "{result:?}");
    }
    #[test]
    fn review_probe_behind_recovery_overwrites_new_official_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Workbench::new();
        w.open_project(dir.path().to_path_buf()).unwrap();
        let d = w.create("A", "a.md").unwrap();
        let (state, _) = snapshot_for("old-draft");
        w.write_recovery("old-draft", &serde_json::to_string(&state).unwrap(), 1, 2).unwrap();
        let store = WritingStore::open(dir.path()).unwrap();
        store.save(SaveRequest {document:d.id.clone(),expected_revision:1,operation_id:"external-update".into(),snapshot:snapshot_for("NEW_OFFICIAL").1,expected_file_digest:None}).unwrap();
        let mut reopened = Workbench::new();
        reopened.open_project(dir.path().to_path_buf()).unwrap();
        reopened.open_document(&d.id).unwrap();
        assert_eq!(reopened.recovery_pending().unwrap().class, RecoveryClass::Behind);
        let (state, text) = reopened.accepted_recovery().unwrap();
        reopened.save(WritingSnapshot::new(text, serde_json::to_string(&state).unwrap(), 1)).unwrap();
        assert_eq!(store.load(&d.id).unwrap().snapshot.text,"old-draft");
    }
    #[test]
    fn review_probe_invalid_identity_state_accepted_as_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = WritingStore::open(dir.path()).unwrap();
        let d = store.create(NewDocument{title:"A".into(),relative_path:"a.md".into(),snapshot:WritingSnapshot::new("text", "{}", 999),newline:NewlineStyle::Lf,bom:false}).unwrap();
        let CreateOutcome::Created(d)=d else {panic!()};
        let loaded=store.load(&d.id).unwrap();
        assert_eq!(loaded.snapshot.state_json,"{}");
        assert_eq!(loaded.snapshot.schema,999);
    }
}
