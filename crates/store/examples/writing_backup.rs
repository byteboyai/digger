//! Close Digger and other project writers before running this recovery utility.
use digger_store::writing::{WritingStore, backup_writing, restore_writing};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: writing_backup backup|restore SOURCE NEW_DESTINATION".into());
    }
    match args[1].as_str() {
        "backup" => backup_writing(
            &WritingStore::open(Path::new(&args[2]))?,
            Path::new(&args[3]),
        )?,
        "restore" => restore_writing(Path::new(&args[2]), Path::new(&args[3]))?,
        _ => return Err("expected backup or restore".into()),
    }
    Ok(())
}
