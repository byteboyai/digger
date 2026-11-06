//! Production persistence samples only: no IPC, GUI paint or IME measurement.
use digger_editor::doc::Doc;
use digger_store::writing::*;
use std::time::Instant;
fn snapshot(doc: &mut Doc) -> WritingSnapshot {
    let state = doc.export_state();
    WritingSnapshot::new(
        state.text.clone(),
        serde_json::to_string(&state).unwrap(),
        1,
    )
}
fn main() {
    for blocks in [100, 1000, 5000] {
        for sample in 0..5 {
            let dir = tempfile::tempdir().unwrap();
            let text = (0..blocks)
                .map(|i| format!("第{i}段，中英文 mixed text 😀。\n\n"))
                .collect::<String>();
            let mut doc = Doc::new();
            doc.load(&text);
            assert_eq!(doc.export_state().ids.len(), blocks);
            let store = WritingStore::open(dir.path()).unwrap();
            let CreateOutcome::Created(created) = store
                .create(NewDocument {
                    title: "bench".into(),
                    relative_path: "chapter.md".into(),
                    snapshot: snapshot(&mut doc),
                    newline: NewlineStyle::Lf,
                    bom: false,
                    source_file_digest: None,
                })
                .unwrap()
            else {
                panic!();
            };
            store.mirror_pending(dir.path(), 8).unwrap();
            let start = Instant::now();
            doc.edit(doc.version, &[(2, 2, "新增字".into())]).unwrap();
            let edit_us = start.elapsed().as_micros();
            let start = Instant::now();
            let saved = snapshot(&mut doc);
            let snapshot_us = start.elapsed().as_micros();
            let start = Instant::now();
            store
                .save(SaveRequest {
                    document: created.id.clone(),
                    expected_revision: 1,
                    operation_id: "benchmark-save".into(),
                    snapshot: saved.clone(),
                    expected_file_digest: store.mirror_digest(&created.id).unwrap(),
                })
                .unwrap();
            let save_us = start.elapsed().as_micros();
            let start = Instant::now();
            let mirrors = store.mirror_pending(dir.path(), 8).unwrap();
            assert!(mirrors.iter().all(|(_, result)| result.is_ok()));
            let mirror_us = start.elapsed().as_micros();
            let recovery = RecoveryStore::open(dir.path()).unwrap();
            let draft = RecoveryDraft::new(
                store.store_id(),
                &created.id,
                "bench-session",
                2,
                doc.version,
                &saved.text,
                &saved.state_json,
                1,
                1,
            );
            let start = Instant::now();
            recovery.write(&draft).unwrap();
            let recovery_write_us = start.elapsed().as_micros();
            let start = Instant::now();
            let _ = recovery.read(&created.id, "bench-session").unwrap();
            let recovery_read_us = start.elapsed().as_micros();
            drop(store);
            let start = Instant::now();
            let reopened = WritingStore::open(dir.path()).unwrap();
            assert_eq!(reopened.load(&created.id).unwrap().snapshot, saved);
            let reopen_us = start.elapsed().as_micros();
            println!(
                "{}",
                serde_json::json!({"blocks":blocks,"sample":sample,"profile":if cfg!(debug_assertions) {"debug"} else {"release"},"edit_us":edit_us,"snapshot_us":snapshot_us,"save_us":save_us,"mirror_us":mirror_us,"recovery_write_us":recovery_write_us,"recovery_read_us":recovery_read_us,"reopen_us":reopen_us,"bytes":text.len()})
            );
        }
    }
}
