//! Stop Digger first, then export to a NEW path. SQLite and archived sources are retained.
use digger_store::{UsageDb, UsageLedger};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage_export <existing usage.sqlite3> <new output.json>; stop Digger first".into(),
        );
    }
    let (source, target) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    if !source.is_file() {
        return Err("source database does not exist".into());
    }
    if target.exists() {
        return Err("output already exists; choose a new backup path".into());
    }
    let db = UsageDb::open(source)?;
    db.export_legacy_json(&target)?;
    UsageLedger::load(&target)?;
    println!(
        "exported {} runs to {} (legacy reader verified)",
        db.len()?,
        target.display()
    );
    Ok(())
}
