//! Durable SQLite scaling probe. Run in release; never opens product data.
use digger_store::{CompletionEvent, UsageDb};
use dozer_model::AgentKind;
use serde_json::json;
use std::time::Instant;
fn event(i: usize) -> CompletionEvent {
    CompletionEvent {
        run_id: format!("r{i}"),
        project_id: (i % 100) as i64,
        project_path: format!("/bench/p{}", i % 100),
        agent: AgentKind::Claude,
        title: format!("run {i}"),
        started_ts_ms: i as u64,
        finished_ts_ms: i as u64 + 1,
        turns: 1,
        tool_calls: 2,
        mutating_tool_calls: 1,
        files_touched: vec!["a.rs".into()],
        tokens_in: 100,
        tokens_out: 50,
        tokens_cache_read: 10,
        tokens_cache_write: 5,
    }
}
fn main() {
    let root = std::env::temp_dir().join(format!("digger-usage-scale-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut db = UsageDb::open(root.join("usage.sqlite3")).unwrap();
    let mut seeded = 0;
    for size in [1000, 10_000, 100_000] {
        let start = Instant::now();
        for i in seeded..size {
            db.record(event(i)).unwrap();
        }
        println!(
            "{}",
            json!({"kind":"seed", "rows":size,"ns":start.elapsed().as_nanos() as u64})
        );
        seeded = size;
        for sample in 0..41 {
            let mut update = event(sample * 7919 % size);
            update.tokens_in += 1;
            let start = Instant::now();
            assert!(!db.record(update).unwrap());
            let write_ns = start.elapsed().as_nanos() as u64;
            let start = Instant::now();
            let rows = db.query("/bench/p0", Some(size as u64 - 100)).unwrap();
            let query_ns = start.elapsed().as_nanos() as u64;
            assert_eq!(rows.len(), 1);
            println!(
                "{}",
                json!({"kind":"sample","rows":size,"sample":sample,"upsert_ns":write_ns,"query_ns":query_ns})
            );
        }
        assert_eq!(db.len().unwrap(), size);
    }
    db.checkpoint().unwrap();
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
