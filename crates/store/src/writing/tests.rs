use super::types::{MAX_STATE_BYTES, MAX_TEXT_BYTES};
use super::*;
use tempfile::TempDir;

#[test]
fn delayed_recovery_write_cannot_replace_a_newer_buffer() {
    let dir = project();
    let recovery = RecoveryStore::open(dir.path()).unwrap();
    let document = DocumentId("document".into());
    let make = |text: &str, revision| {
        RecoveryDraft::new(
            &StoreId("store".into()),
            &document,
            "session",
            1,
            revision,
            text,
            snapshot(text).state_json,
            1,
            revision as i64,
        )
    };
    recovery.write(&make("new", 2)).unwrap();
    recovery.write(&make("old", 1)).unwrap();
    let RecoveryRead::Draft(draft) = recovery.read(&document, "session").unwrap() else {
        panic!()
    };
    assert_eq!(draft.text, "new");
    recovery
        .remove_committed(&document, "session", &snapshot("old"))
        .unwrap();
    assert!(matches!(
        recovery.read(&document, "session").unwrap(),
        RecoveryRead::Draft(_)
    ));
    recovery
        .remove_committed(&document, "session", &snapshot("new"))
        .unwrap();
    assert!(matches!(
        recovery.read(&document, "session").unwrap(),
        RecoveryRead::Missing
    ));
}

fn snapshot(text: &str) -> WritingSnapshot {
    let mut doc = digger_editor::doc::Doc::new();
    doc.load(text);
    let state = doc.export_state();
    WritingSnapshot::new(
        state.text.clone(),
        serde_json::to_string(&state).unwrap(),
        digger_editor::doc::STATE_SCHEMA,
    )
}

fn project() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn new_doc(title: &str, path: &str, text: &str) -> NewDocument {
    NewDocument {
        title: title.into(),
        relative_path: path.into(),
        snapshot: snapshot(text),
        newline: NewlineStyle::Lf,
        bom: false,
        source_file_digest: None,
    }
}

fn created(outcome: CreateOutcome) -> Document {
    match outcome {
        CreateOutcome::Created(document) => document,
        CreateOutcome::Existing(document) => panic!("unexpected existing {document:?}"),
    }
}

#[test]
fn create_load_and_reopen_preserve_identity_and_snapshot() {
    let dir = project();
    let (id, store_id) = {
        let store = WritingStore::open(dir.path()).unwrap();
        let doc = created(
            store
                .create(new_doc("第一章", "book/01.md", "# 一\n"))
                .unwrap(),
        );
        assert_eq!(doc.revision, 1);
        let loaded = store.load(&doc.id).unwrap();
        assert_eq!(loaded.snapshot.text, "# 一\n");
        assert_eq!(store.project_revision().unwrap(), 1);
        (doc.id, store.store_id().clone())
    };
    let store = WritingStore::open(dir.path()).unwrap();
    assert_eq!(store.store_id(), &store_id);
    let loaded = store.load(&id).unwrap();
    assert_eq!(loaded.document.title, "第一章");
    assert_eq!(loaded.document.relative_path, "book/01.md");
    assert_eq!(loaded.snapshot.digest(), snapshot("# 一\n").digest());
}

#[test]
fn duplicate_path_returns_existing_without_creating_a_copy() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let first = created(store.create(new_doc("A", "a.md", "a")).unwrap());
    match store.create(new_doc("B", "a.md", "b")).unwrap() {
        CreateOutcome::Existing(existing) => {
            assert_eq!(existing.id, first.id);
            assert_eq!(store.load(&first.id).unwrap().snapshot.text, "a");
        }
        CreateOutcome::Created(_) => panic!("duplicate path must not overwrite"),
    }
}

#[test]
fn save_is_atomic_and_advances_version_commit_outbox_and_project_revision() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    let receipt = store
        .save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 1,
            operation_id: "op-1".into(),
            snapshot: snapshot("two"),
            expected_file_digest: None,
        })
        .unwrap();
    assert_eq!(receipt.revision, 2);
    assert!(!receipt.replayed);
    assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "two");
    assert_eq!(store.project_revision().unwrap(), 2);
    let outbox = store.pending_outbox(10).unwrap();
    assert_eq!(outbox.len(), 2, "create + save each queue one mirror task");
    let saved = outbox.iter().find(|entry| entry.revision == 2).unwrap();
    assert_eq!(saved.target_digest, snapshot("two").digest());
}

#[test]
fn replaying_the_same_operation_does_not_commit_twice() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    let request = || SaveRequest {
        document: doc.id.clone(),
        expected_revision: 1,
        operation_id: "same-op".into(),
        snapshot: snapshot("two"),
        expected_file_digest: None,
    };
    let first = store.save(request()).unwrap();
    let replay = store.save(request()).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.commit_id, first.commit_id);
    assert_eq!(store.project_revision().unwrap(), 2);
    assert_eq!(store.pending_outbox(10).unwrap().len(), 2);
}

#[test]
fn same_operation_id_with_different_content_is_rejected() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    store
        .save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 1,
            operation_id: "op".into(),
            snapshot: snapshot("two"),
            expected_file_digest: None,
        })
        .unwrap();
    let error = store
        .save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 2,
            operation_id: "op".into(),
            snapshot: snapshot("three"),
            expected_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(error, WritingError::OperationConflict { .. }));
    assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "two");
}

#[test]
fn cross_connection_cas_allows_exactly_one_writer() {
    let dir = project();
    let first = WritingStore::open(dir.path()).unwrap();
    let second = WritingStore::open(dir.path()).unwrap();
    let doc = created(first.create(new_doc("A", "a.md", "one")).unwrap());
    let request = |op: &str, text: &str| SaveRequest {
        document: doc.id.clone(),
        expected_revision: 1,
        operation_id: op.into(),
        snapshot: snapshot(text),
        expected_file_digest: None,
    };
    first.save(request("op-a", "two")).unwrap();
    let error = second.save(request("op-b", "three")).unwrap_err();
    assert!(matches!(
        error,
        WritingError::RevisionConflict {
            expected: 1,
            actual: 2
        }
    ));
    assert_eq!(first.load(&doc.id).unwrap().snapshot.text, "two");
}

#[test]
fn invalid_snapshot_rolls_back_everything() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    let mismatch = WritingSnapshot::new(
        "different",
        serde_json::json!({ "text": "not the same" }).to_string(),
        1,
    );
    let error = store
        .save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 1,
            operation_id: "bad".into(),
            snapshot: mismatch,
            expected_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(error, WritingError::InvalidSnapshot(_)));
    assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "one");
    assert_eq!(store.project_revision().unwrap(), 1);
    assert!(store.pending_outbox(10).unwrap().len() == 1);
}

#[test]
fn path_and_size_limits_are_rejected() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    for path in ["/etc/passwd", "../a.md", "a/../../b.md", ".digger/x.md", ""] {
        assert!(
            store.create(new_doc("A", path, "x")).is_err(),
            "path {path:?} must be rejected"
        );
    }
    let huge = "x".repeat(MAX_TEXT_BYTES + 1);
    let error = store.create(new_doc("A", "huge.md", &huge)).unwrap_err();
    assert!(matches!(error, WritingError::Limit(_)));
    let huge_state = "x".repeat(MAX_STATE_BYTES + 1);
    let error = store
        .create(NewDocument {
            title: "A".into(),
            relative_path: "state.md".into(),
            snapshot: WritingSnapshot::new("x", huge_state, 1),
            newline: NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(error, WritingError::Limit(_)));
}

#[test]
fn invalid_or_future_identity_state_is_rejected_not_saved() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let empty = store
        .create(NewDocument {
            title: "A".into(),
            relative_path: "a.md".into(),
            snapshot: WritingSnapshot::new("text", "{}", 1),
            newline: NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(empty, WritingError::InvalidSnapshot(_)));
    let future = store
        .create(NewDocument {
            title: "B".into(),
            relative_path: "b.md".into(),
            snapshot: WritingSnapshot::new("text", "{}", 999),
            newline: NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(
        future,
        WritingError::UnsupportedSchema {
            found: 999,
            supported: 1
        }
    ));
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn identity_state_with_duplicate_ids_or_bad_next_id_is_rejected() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let mut doc = digger_editor::doc::Doc::new();
    doc.load("# one\n\ntwo\n");
    let state = doc.export_state();
    assert!(state.ids.len() >= 2);
    let mut duplicate = state.clone();
    duplicate.ids[1] = duplicate.ids[0];
    let error = store
        .create(NewDocument {
            title: "A".into(),
            relative_path: "a.md".into(),
            snapshot: WritingSnapshot::new(
                duplicate.text.clone(),
                serde_json::to_string(&duplicate).unwrap(),
                1,
            ),
            newline: NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(error, WritingError::InvalidSnapshot(_)), "{error}");

    let mut next_id = state.clone();
    next_id.next_id = next_id.ids.iter().copied().max().unwrap_or(0);
    let error = store
        .create(NewDocument {
            title: "B".into(),
            relative_path: "b.md".into(),
            snapshot: WritingSnapshot::new(
                next_id.text.clone(),
                serde_json::to_string(&next_id).unwrap(),
                1,
            ),
            newline: NewlineStyle::Lf,
            bom: false,
            source_file_digest: None,
        })
        .unwrap_err();
    assert!(matches!(error, WritingError::InvalidSnapshot(_)), "{error}");
}

#[test]
fn load_rejects_a_tampered_stored_digest() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    {
        let conn = rusqlite::Connection::open(dir.path().join(PROJECT_DB_RELATIVE)).unwrap();
        conn.execute(
            "UPDATE document_versions SET digest='tampered' WHERE document_id=?1",
            rusqlite::params![doc.id.as_str()],
        )
        .unwrap();
    }
    let error = store.load(&doc.id).unwrap_err();
    assert!(matches!(error, WritingError::Corrupt(_)), "{error}");
}

#[test]
fn future_schema_is_rejected_read_only() {
    let dir = project();
    {
        let store = WritingStore::open(dir.path()).unwrap();
        let _ = store.create(new_doc("A", "a.md", "one")).unwrap();
    }
    let db = dir.path().join(PROJECT_DB_RELATIVE);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE meta SET value='99' WHERE key='schema_version'", [])
        .unwrap();
    drop(conn);
    let error = WritingStore::open(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        WritingError::UnsupportedSchema {
            found: 99,
            supported: SCHEMA_VERSION
        }
    ));
}

#[test]
fn outbox_lifecycle_records_attempts_and_terminal_states() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    store
        .save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 1,
            operation_id: "op".into(),
            snapshot: snapshot("two"),
            expected_file_digest: Some("baseline".into()),
        })
        .unwrap();
    let entry = store
        .pending_outbox(10)
        .unwrap()
        .into_iter()
        .find(|entry| entry.revision == 2)
        .unwrap();
    assert_eq!(entry.expected_file_digest.as_deref(), Some("baseline"));
    store.mark_outbox_failed(entry.id, "disk full").unwrap();
    let retry = store
        .pending_outbox(10)
        .unwrap()
        .into_iter()
        .find(|entry| entry.revision == 2)
        .unwrap();
    assert_eq!(retry.attempts, 1);
    assert_eq!(retry.last_error.as_deref(), Some("disk full"));
    store.mark_outbox_synced(entry.id, "target").unwrap();
    assert!(
        store
            .pending_outbox(10)
            .unwrap()
            .iter()
            .all(|entry| entry.revision != 2)
    );
}

#[test]
fn recovery_round_trips_and_classifies_against_durable() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "one")).unwrap());
    let recovery = RecoveryStore::open(dir.path()).unwrap();
    let draft_snapshot = snapshot("draft text");
    let draft = RecoveryDraft::new(
        store.store_id(),
        &doc.id,
        "session-1",
        1,
        3,
        draft_snapshot.text.clone(),
        draft_snapshot.state_json.clone(),
        1,
        1,
    );
    recovery.write(&draft).unwrap();
    let read = recovery.read(&doc.id, "session-1").unwrap();
    let RecoveryRead::Draft(loaded) = read else {
        panic!("expected draft, got {read:?}");
    };
    assert_eq!(loaded.text, "draft text");
    assert_eq!(
        classify(&loaded, 1, &snapshot("one")),
        RecoveryClass::Recoverable
    );
    assert_eq!(
        classify(&loaded, 2, &snapshot("two")),
        RecoveryClass::Behind
    );
    assert_eq!(
        classify(&draft, 1, &draft_snapshot),
        RecoveryClass::Redundant
    );
    recovery.remove(&doc.id, "session-1").unwrap();
    assert!(matches!(
        recovery.read(&doc.id, "session-1").unwrap(),
        RecoveryRead::Missing
    ));
}

#[test]
fn corrupt_recovery_is_preserved_and_reported() {
    let dir = project();
    let recovery = RecoveryStore::open(dir.path()).unwrap();
    let doc = DocumentId("doc-1".into());
    let path = dir.path().join(".digger/working/doc-1/session.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{not json").unwrap();
    let read = recovery.read(&doc, "session").unwrap();
    assert!(matches!(read, RecoveryRead::Corrupt { .. }));
    assert!(
        path.exists(),
        "corrupt draft must never be silently deleted"
    );
}

#[test]
fn mirror_restores_newline_and_detects_external_change() {
    let dir = project();
    let root = dozer_file_io::ProjectRoot::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("a.md"), b"old").unwrap();
    let first = write_mirror(
        &root,
        "a.md",
        "line1\nline2\n",
        NewlineStyle::Crlf,
        true,
        &MirrorBaseline::Digest("old-digest".into()),
    );
    assert!(matches!(first, Err(MirrorError::ExternalChange(_))));

    let current = current_file_digest(&root, "a.md").unwrap().unwrap();
    let outcome = write_mirror(
        &root,
        "a.md",
        "line1\nline2\n",
        NewlineStyle::Crlf,
        true,
        &MirrorBaseline::Digest(current),
    )
    .unwrap();
    let bytes = std::fs::read(dir.path().join("a.md")).unwrap();
    assert_eq!(bytes, b"\xEF\xBB\xBFline1\r\nline2\r\n");
    assert_eq!(outcome.file_digest, digest_bytes(&bytes));
}

#[test]
fn mirror_refuses_paths_outside_the_project() {
    let dir = project();
    let root = dozer_file_io::ProjectRoot::open(dir.path()).unwrap();
    for path in ["../escape.md", "/tmp/escape.md", ".digger/x.md"] {
        assert!(
            write_mirror(
                &root,
                path,
                "x",
                NewlineStyle::Lf,
                false,
                &MirrorBaseline::Absent
            )
            .is_err()
        );
    }
}

#[test]
fn a_path_that_is_not_a_directory_is_unavailable() {
    let dir = project();
    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, "x").unwrap();
    assert!(WritingStore::open(&file).is_err());
}

// These tests run a separate process that exits without unwinding SQLite or
// filesystem handles. No environment failpoint exists in production builds.
#[test]
fn writing_crash_child() {
    let Ok(root) = std::env::var("DIGGER_WRITING_CRASH_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let store = WritingStore::open(root).unwrap();
    let doc = store.list().unwrap().remove(0);
    if std::env::var("DIGGER_TEST_CRASH_AT").unwrap() == "writing-recovery-before-rename" {
        RecoveryStore::open(root)
            .unwrap()
            .write(&RecoveryDraft::new(
                store.store_id(),
                &doc.id,
                "session",
                1,
                2,
                "new draft",
                snapshot("new draft").state_json,
                1,
                2,
            ))
            .unwrap();
    } else if std::env::var("DIGGER_TEST_CRASH_AT")
        .unwrap()
        .starts_with("writing-mirror")
    {
        store.mirror_pending(root, 8).unwrap();
    } else {
        store
            .save(SaveRequest {
                document: doc.id,
                expected_revision: 1,
                operation_id: "crash-op".into(),
                snapshot: snapshot("committed after crash"),
                expected_file_digest: None,
            })
            .unwrap();
    }
    panic!("failpoint was not reached");
}

fn crash_child(root: &std::path::Path, point: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "writing::tests::writing_crash_child",
            "--nocapture",
        ])
        .env("DIGGER_WRITING_CRASH_ROOT", root)
        .env("DIGGER_TEST_CRASH_AT", point)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86), "{point}");
}

#[test]
fn writing_real_process_crashes_preserve_atomic_commit_and_idempotent_reply() {
    for committed in [false, true] {
        let dir = project();
        let store = WritingStore::open(dir.path()).unwrap();
        let doc = created(store.create(new_doc("A", "a.md", "original")).unwrap());
        drop(store);
        crash_child(
            dir.path(),
            if committed {
                "writing-save-after-commit"
            } else {
                "writing-save-before-commit"
            },
        );
        let store = WritingStore::open(dir.path()).unwrap();
        let current = store.load(&doc.id).unwrap();
        assert_eq!(current.document.revision, if committed { 2 } else { 1 });
        assert_eq!(
            current.snapshot.text,
            if committed {
                "committed after crash"
            } else {
                "original"
            }
        );
        assert_eq!(
            store.project_revision().unwrap(),
            if committed { 2 } else { 1 }
        );
        assert_eq!(
            store.pending_outbox(8).unwrap().len(),
            if committed { 2 } else { 1 }
        );
        let reply = store
            .save(SaveRequest {
                document: doc.id,
                expected_revision: 1,
                operation_id: "crash-op".into(),
                snapshot: snapshot("committed after crash"),
                expected_file_digest: None,
            })
            .unwrap();
        assert_eq!(reply.replayed, committed);
        assert_eq!(reply.revision, 2);
    }
}

#[test]
fn writing_real_process_mirror_crash_replays_without_external_conflict() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    store.create(new_doc("A", "a.md", "original")).unwrap();
    drop(store);
    crash_child(dir.path(), "writing-mirror-after-rename");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "original"
    );
    let store = WritingStore::open(dir.path()).unwrap();
    assert_eq!(store.pending_outbox(8).unwrap().len(), 1);
    assert!(
        store
            .mirror_pending(dir.path(), 8)
            .unwrap()
            .iter()
            .all(|(_, result)| result.is_ok())
    );
    assert!(store.pending_outbox(8).unwrap().is_empty());
    assert!(store.mirror_notice().unwrap().is_none());
}

#[test]
fn blocked_mirror_prevents_future_passes_from_overwriting_external_content() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "original")).unwrap());
    store.mirror_pending(dir.path(), 8).unwrap();
    std::fs::write(dir.path().join("a.md"), "external").unwrap();
    for revision in 1..=2 {
        store
            .save(SaveRequest {
                document: doc.id.clone(),
                expected_revision: revision,
                operation_id: format!("op-{revision}"),
                snapshot: snapshot(&format!("new {revision}")),
                expected_file_digest: store.mirror_digest(&doc.id).unwrap(),
            })
            .unwrap();
        store.mirror_pending(dir.path(), 8).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "external"
    );
    assert!(store.mirror_notice().unwrap().is_some());
    assert!(
        store.pending_outbox(8).unwrap().is_empty(),
        "later passes must respect the persisted block"
    );
}

#[test]
fn backup_restore_preserves_wal_data_recovery_external_files_and_forks_identity() {
    let dir = project();
    let bundles = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "book/a.md", "official")).unwrap());
    store.mirror_pending(dir.path(), 8).unwrap();
    std::fs::write(dir.path().join("book/a.md"), "external retained").unwrap();
    let recovery = RecoveryStore::open(dir.path()).unwrap();
    recovery
        .write(&RecoveryDraft::new(
            store.store_id(),
            &doc.id,
            "session",
            1,
            5,
            "unsaved",
            snapshot("unsaved").state_json,
            1,
            123,
        ))
        .unwrap();
    let original_uuid = store.store_id().clone();
    let original = std::fs::read(dir.path().join("book/a.md")).unwrap();
    let bundle = bundles.path().join("backup");
    let restored = bundles.path().join("restored");
    backup_writing(&store, &bundle).unwrap();
    assert_eq!(
        std::fs::read_to_string(bundle.join("markdown/book/a.md")).unwrap(),
        "official"
    );
    restore_writing(&bundle, &restored).unwrap();
    let fork = WritingStore::open(&restored).unwrap();
    assert_ne!(fork.store_id(), &original_uuid);
    assert_eq!(fork.load(&doc.id).unwrap(), store.load(&doc.id).unwrap());
    assert_eq!(std::fs::read(restored.join("book/a.md")).unwrap(), original);
    let RecoveryRead::Draft(draft) = RecoveryStore::open(&restored)
        .unwrap()
        .read(&doc.id, "session")
        .unwrap()
    else {
        panic!();
    };
    assert_eq!(draft.store_id, fork.store_id().as_str());
    assert_eq!(draft.text, "unsaved");
    assert_eq!(
        std::fs::read(dir.path().join("book/a.md")).unwrap(),
        original
    );
    assert!(restore_writing(&bundle, &restored).is_err());
    std::fs::write(bundle.join("markdown/book/a.md"), "tampered").unwrap();
    assert!(restore_writing(&bundle, &bundles.path().join("bad")).is_err());
    assert!(!bundles.path().join("bad").exists());
}

#[test]
fn external_review_copy_keeps_official_identity_and_expired_confirmation_refuses_overwrite() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "official")).unwrap());
    store.mirror_pending(dir.path(), 8).unwrap();
    std::fs::write(dir.path().join("a.md"), "external one").unwrap();
    let review = store.review_external(&doc.id).unwrap();
    let copy = store.copy_reviewed_external(&review).unwrap();
    assert_ne!(copy.id, doc.id);
    assert_eq!(store.load(&copy.id).unwrap().snapshot.text, "external one");
    assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "official");
    std::fs::write(dir.path().join("a.md"), "external two").unwrap();
    assert!(store.overwrite_reviewed_mirror(&review).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "external two"
    );
    let fresh = store.review_external(&doc.id).unwrap();
    store.overwrite_reviewed_mirror(&fresh).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "official"
    );
    assert_eq!(store.load(&doc.id).unwrap().document.revision, 1);
}

#[test]
fn disk_full_and_readonly_failures_do_not_advance_any_durable_fact() {
    for full in [false, true] {
        let dir = project();
        let store = WritingStore::open(dir.path()).unwrap();
        let doc = created(store.create(new_doc("A", "a.md", "original")).unwrap());
        store.inject_sql_fault(full);
        let text = "larger paragraph to exhaust allocated pages.\n\n".repeat(4000);
        let result = store.save(SaveRequest {
            document: doc.id.clone(),
            expected_revision: 1,
            operation_id: "failed-save".into(),
            snapshot: snapshot(&text),
            expected_file_digest: None,
        });
        assert!(result.is_err(), "fixture must actually fail (full={full})");
        assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "original");
        assert_eq!(store.project_revision().unwrap(), 1);
        assert_eq!(store.pending_outbox(8).unwrap().len(), 1);
        drop(store);
        let reopened = WritingStore::open(dir.path()).unwrap();
        assert_eq!(reopened.load(&doc.id).unwrap().document.revision, 1);
    }
}

#[cfg(unix)]
#[test]
fn recovery_and_database_refuse_links_without_touching_the_original() {
    use std::os::unix::fs::symlink;
    let dir = project();
    let other = project();
    std::fs::write(other.path().join("sentinel"), "keep").unwrap();
    symlink(other.path(), dir.path().join(".digger")).unwrap();
    assert!(WritingStore::open(dir.path()).is_err());
    assert!(RecoveryStore::open(dir.path()).is_err());
    assert!(!other.path().join("project.db").exists());
    assert_eq!(
        std::fs::read_to_string(other.path().join("sentinel")).unwrap(),
        "keep"
    );
}

#[test]
fn recovery_crash_before_rename_retains_the_last_complete_draft() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "official")).unwrap());
    let recovery = RecoveryStore::open(dir.path()).unwrap();
    recovery
        .write(&RecoveryDraft::new(
            store.store_id(),
            &doc.id,
            "session",
            1,
            1,
            "old draft",
            snapshot("old draft").state_json,
            1,
            1,
        ))
        .unwrap();
    crash_child(dir.path(), "writing-recovery-before-rename");
    let RecoveryRead::Draft(draft) = recovery.read(&doc.id, "session").unwrap() else {
        panic!();
    };
    assert_eq!(draft.text, "old draft");
    assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "official");
}

#[test]
fn writing_cas_child() {
    let Ok(root) = std::env::var("DIGGER_WRITING_CAS_ROOT") else {
        return;
    };
    let label = std::env::var("DIGGER_WRITING_CAS_LABEL").unwrap();
    let root = std::path::Path::new(&root);
    let store = WritingStore::open(root).unwrap();
    let doc = store.list().unwrap().remove(0);
    std::fs::write(root.join(format!("ready-{label}")), "ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !root.join("start").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let result = store.save(SaveRequest {
        document: doc.id,
        expected_revision: 1,
        operation_id: label.clone(),
        snapshot: snapshot(&label),
        expected_file_digest: None,
    });
    let outcome = match result {
        Ok(_) => "committed",
        Err(WritingError::RevisionConflict { .. }) => "conflict",
        Err(error) => panic!("unexpected CAS result: {error}"),
    };
    std::fs::write(root.join(format!("result-{label}")), outcome).unwrap();
}

#[test]
fn two_real_processes_racing_the_same_revision_commit_exactly_once() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "original")).unwrap());
    let mut children = vec![];
    for label in ["a", "b"] {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "writing::tests::writing_cas_child"])
                .env("DIGGER_WRITING_CAS_ROOT", dir.path())
                .env("DIGGER_WRITING_CAS_LABEL", label)
                .spawn()
                .unwrap(),
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !dir.path().join("ready-a").exists() || !dir.path().join("ready-b").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::fs::write(dir.path().join("start"), "start").unwrap();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let mut outcomes = ["a", "b"]
        .map(|label| std::fs::read_to_string(dir.path().join(format!("result-{label}"))).unwrap());
    outcomes.sort();
    assert_eq!(outcomes, ["committed", "conflict"]);
    assert_eq!(store.load(&doc.id).unwrap().document.revision, 2);
    assert_eq!(store.project_revision().unwrap(), 2);
    assert_eq!(store.pending_outbox(8).unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn mirror_does_not_create_subdirectories_through_a_symlink_parent() {
    use std::os::unix::fs::symlink;
    let dir = project();
    let other = project();
    symlink(other.path(), dir.path().join("linked")).unwrap();
    let root = dozer_file_io::ProjectRoot::open(dir.path()).unwrap();
    assert!(
        write_mirror(
            &root,
            "linked/new/child.md",
            "content",
            NewlineStyle::Lf,
            false,
            &MirrorBaseline::Absent
        )
        .is_err()
    );
    assert!(!other.path().join("new").exists());
}

#[test]
fn exhausted_retry_stops_the_document_until_an_explicit_retry() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    store.create(new_doc("A", "a.md", "original")).unwrap();
    let entry = store.pending_outbox(8).unwrap().remove(0);
    for _ in 0..8 {
        store
            .mark_outbox_failed(entry.id, "fixture disk error")
            .unwrap();
    }
    assert!(store.pending_outbox(8).unwrap().is_empty());
    assert!(store.mirror_notice().unwrap().is_some());
    store.retry_failed_mirrors().unwrap();
    assert_eq!(store.pending_outbox(8).unwrap().len(), 1);
    assert!(store.mirror_pending(dir.path(), 8).unwrap()[0].1.is_ok());
}

#[test]
fn oversized_external_replacement_is_preserved_without_mirroring_over_it() {
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "official")).unwrap());
    store.mirror_pending(dir.path(), 8).unwrap();
    let large = vec![b'x'; MAX_TEXT_BYTES + 1];
    std::fs::write(dir.path().join("a.md"), &large).unwrap();
    store
        .save(SaveRequest {
            document: doc.id,
            expected_revision: 1,
            operation_id: "large-replacement".into(),
            snapshot: snapshot("new official"),
            expected_file_digest: Some(digest_bytes(b"official")),
        })
        .unwrap();
    assert!(store.mirror_pending(dir.path(), 8).unwrap()[0].1.is_err());
    assert_eq!(std::fs::read(dir.path().join("a.md")).unwrap(), large);
}

#[cfg(unix)]
#[test]
fn mirror_permission_failure_preserves_source_and_retains_retry() {
    use std::os::unix::fs::PermissionsExt;
    struct Reset(std::path::PathBuf);
    impl Drop for Reset {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let dir = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let doc = created(store.create(new_doc("A", "a.md", "original")).unwrap());
    store.mirror_pending(dir.path(), 8).unwrap();
    store
        .save(SaveRequest {
            document: doc.id,
            expected_revision: 1,
            operation_id: "permission-save".into(),
            snapshot: snapshot("new official"),
            expected_file_digest: Some(digest_bytes(b"original")),
        })
        .unwrap();
    let _reset = Reset(dir.path().to_owned());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(store.mirror_pending(dir.path(), 8).unwrap()[0].1.is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "original"
    );
    assert_eq!(store.pending_outbox(8).unwrap()[0].state, "retry");
}

#[cfg(unix)]
#[test]
fn replacing_the_database_instance_refuses_old_writes_and_mirrors() {
    let dir = project();
    let replacement = project();
    let store = WritingStore::open(dir.path()).unwrap();
    let old = created(
        store
            .create(new_doc("old", "old.md", "old official"))
            .unwrap(),
    );
    let fresh = WritingStore::open(replacement.path()).unwrap();
    let new = created(
        fresh
            .create(new_doc("new", "new.md", "new official"))
            .unwrap(),
    );
    let candidate = dir.path().join("replacement.db");
    fresh.snapshot_database(&candidate).unwrap();
    // Replace the complete SQLite file set. Leaving the previous WAL beside a
    // different main database is not a valid restore, regardless of this API.
    let retired = dir.path().join("retired");
    std::fs::create_dir(&retired).unwrap();
    for sidecar in ["project.db-wal", "project.db-shm"] {
        let path = dir.path().join(".digger").join(sidecar);
        if path.exists() {
            std::fs::rename(path, retired.join(sidecar)).unwrap();
        }
    }
    std::fs::rename(
        WritingStore::database_path(dir.path()),
        retired.join("project.db"),
    )
    .unwrap();
    std::fs::rename(candidate, WritingStore::database_path(dir.path())).unwrap();
    assert!(
        store
            .save(SaveRequest {
                document: old.id.clone(),
                expected_revision: 1,
                operation_id: "old-write".into(),
                snapshot: snapshot("must not commit"),
                expected_file_digest: None
            })
            .is_err()
    );
    assert!(store.mirror_pending(dir.path(), 8).is_err());
    assert!(!dir.path().join("old.md").exists());
    drop(store);
    let reopened = WritingStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.load(&new.id).unwrap().snapshot.text,
        "new official"
    );
    assert!(reopened.load(&old.id).is_err());
}

#[cfg(unix)]
#[test]
fn a_hardlink_alias_cannot_be_used_to_overwrite_a_mirror() {
    let dir = project();
    std::fs::write(dir.path().join("a.md"), "original").unwrap();
    std::fs::hard_link(dir.path().join("a.md"), dir.path().join("alias.md")).unwrap();
    let root = dozer_file_io::ProjectRoot::open(dir.path()).unwrap();
    assert!(
        write_mirror(
            &root,
            "alias.md",
            "replacement",
            NewlineStyle::Lf,
            false,
            &MirrorBaseline::Digest(digest_bytes(b"original"))
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("alias.md")).unwrap(),
        "original"
    );
}
