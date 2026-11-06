//! Portable writing backups. Call after closing all writers: SQLite is copied
//! consistently with VACUUM INTO; recovery and external files are separate facts.
use super::{
    DocumentId, RecoveryDraft, RecoveryStore, StoreId, WritingError, WritingStore, digest_bytes,
    encode_mirror,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    store: String,
    files: Vec<(String, String)>,
}
fn io(error: impl std::fmt::Display) -> WritingError {
    WritingError::Unavailable(error.to_string())
}
fn safe(path: &str) -> Result<(), WritingError> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(WritingError::InvalidPath("备份路径非法".into()));
    }
    Ok(())
}
fn put(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), WritingError> {
    safe(relative)?;
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io)?;
    file.write_all(bytes).map_err(io)?;
    file.sync_all().map_err(io)
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, WritingError> {
    let meta = std::fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(WritingError::Limit(format!(
            "非常规文件或文件过大：{}",
            path.display()
        )));
    }
    std::fs::read(path).map_err(io)
}
fn files(root: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), WritingError> {
    let directory = root.join(prefix);
    if !directory.exists() {
        return Ok(());
    }
    if std::fs::symlink_metadata(&directory)
        .map_err(io)?
        .file_type()
        .is_symlink()
    {
        return Err(WritingError::InvalidPath("备份不跟随链接".into()));
    }
    for entry in std::fs::read_dir(directory).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io("文件名不是 UTF-8"))?;
        let relative = format!("{prefix}/{name}");
        let kind = entry.file_type().map_err(io)?;
        if kind.is_dir() {
            files(root, &relative, out)?;
        } else if kind.is_file() {
            out.push(relative);
        } else {
            return Err(WritingError::InvalidPath("备份包含链接或非常规文件".into()));
        }
    }
    Ok(())
}
struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn stage(destination: &Path) -> Result<Stage, WritingError> {
    if destination.exists() {
        return Err(WritingError::InvalidPath("目标已存在，请选择新目录".into()));
    }
    let parent = destination.parent().ok_or_else(|| io("目标缺少父目录"))?;
    std::fs::create_dir_all(parent).map_err(io)?;
    let path = parent.join(format!(".writing-backup-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir(&path).map_err(io)?;
    Ok(Stage(path))
}
fn publish(stage: &Stage, destination: &Path) -> Result<(), WritingError> {
    if destination.exists() {
        return Err(WritingError::InvalidPath("目标已存在".into()));
    }
    std::fs::rename(&stage.0, destination).map_err(io)?;
    std::fs::File::open(destination.parent().unwrap())
        .map_err(io)?
        .sync_all()
        .map_err(io)
}

/// Consistent DB plus all recovery files, authoritative Markdown exports, and
/// copies of existing managed source files (including externally changed bytes).
/// The caller must close other writers before collecting non-SQLite files.
pub fn backup_writing(store: &WritingStore, destination: &Path) -> Result<(), WritingError> {
    let stage = stage(destination)?;
    std::fs::create_dir(stage.0.join(".digger")).map_err(io)?;
    store.snapshot_database(&stage.0.join(".digger/project.db"))?;
    {
        let snapshot = WritingStore::open(&stage.0)?;
        let source = dozer_file_io::ProjectRoot::open(store.root()).map_err(io)?;
        for document in snapshot.list()? {
            let loaded = snapshot.load(&document.id)?;
            put(
                &stage.0,
                &format!("markdown/{}", document.relative_path),
                &encode_mirror(&loaded.snapshot.text, document.newline, document.bom),
            )?;
            match source.open_file(&document.relative_path) {
                Ok(file) => put(
                    &stage.0,
                    &format!("external/{}", document.relative_path),
                    &file.read_limited(super::MAX_TEXT_BYTES).map_err(io)?,
                )?,
                Err(dozer_file_io::FileError::NotFound) => {}
                Err(error) => return Err(io(error)),
            }
        }
    }
    let mut recovery_files = vec![];
    files(store.root(), ".digger/working", &mut recovery_files)?;
    for relative in recovery_files {
        if relative.ends_with(".json") || relative.ends_with(".tmp") {
            put(
                &stage.0,
                &relative,
                &read(&store.root().join(&relative), super::MAX_RECOVERY_BYTES * 4)?,
            )?;
        }
    }
    let mut paths = vec![".digger/project.db".into()];
    for prefix in [".digger/working", "markdown", "external"] {
        files(&stage.0, prefix, &mut paths)?;
    }
    paths.sort();
    let mut hashes = vec![];
    for relative in paths {
        hashes.push((
            relative.clone(),
            digest_bytes(&std::fs::read(stage.0.join(relative)).map_err(io)?),
        ));
    }
    let manifest = Manifest {
        schema: 1,
        store: store.store_id().as_str().into(),
        files: hashes,
    };
    put(
        &stage.0,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).map_err(io)?,
    )?;
    publish(&stage, destination)
}

/// Restore only into a new directory; original backup and project stay intact.
/// The restored instance receives a new UUID. Valid recovery envelopes are
/// rebound; corrupt/future envelopes stay unchanged for manual investigation.
pub fn restore_writing(backup: &Path, destination: &Path) -> Result<(), WritingError> {
    let manifest: Manifest =
        serde_json::from_slice(&read(&backup.join("manifest.json"), 8 * 1024 * 1024)?)
            .map_err(io)?;
    if manifest.schema != 1 {
        return Err(WritingError::UnsupportedSchema {
            found: manifest.schema,
            supported: 1,
        });
    }
    let stage = stage(destination)?;
    let mut seen = std::collections::HashSet::new();
    for (relative, digest) in &manifest.files {
        safe(relative)?;
        if !seen.insert(relative.clone()) {
            return Err(WritingError::Corrupt("备份路径重复".into()));
        }
        // Open via the directory-handle API: no symlink ancestors or hardlinks.
        let source = dozer_file_io::ProjectRoot::open(backup).map_err(io)?;
        let opened = source.open_file(relative).map_err(io)?;
        let bytes = opened.read_all().map_err(io)?;
        if digest_bytes(&bytes) != *digest {
            return Err(WritingError::Corrupt(format!("备份摘要不匹配：{relative}")));
        }
        put(&stage.0, relative, &bytes)?;
    }
    if !seen.contains(".digger/project.db") {
        return Err(WritingError::Corrupt("备份缺少 project.db".into()));
    }
    {
        let store = WritingStore::open(&stage.0)?;
        if store.store_id().as_str() != manifest.store {
            return Err(WritingError::Corrupt("备份库身份不匹配".into()));
        }
        for doc in store.list()? {
            store.load(&doc.id)?;
        }
        store.renew_instance_identity()?;
    }
    let store = WritingStore::open(&stage.0)?;
    let recovery = RecoveryStore::open(&stage.0)?;
    for (relative, _) in &manifest.files {
        if relative.starts_with(".digger/working/") && relative.ends_with(".json") {
            let bytes = std::fs::read(stage.0.join(relative)).map_err(io)?;
            if let Ok(draft) = serde_json::from_slice::<RecoveryDraft>(&bytes)
                && draft.verify().is_ok()
                && draft.store_id == manifest.store
            {
                let rebound = RecoveryDraft::new(
                    &StoreId(store.store_id().as_str().into()),
                    &DocumentId(draft.document_id.clone()),
                    draft.session_id.clone(),
                    draft.base_revision,
                    draft.buffer_revision,
                    draft.text,
                    draft.state_json,
                    draft.schema,
                    draft.updated_ms,
                );
                // Remove the copied envelope so its old instance cannot conflict.
                std::fs::remove_file(stage.0.join(relative)).map_err(io)?;
                recovery.write(&rebound)?;
            }
        }
    }
    // Retain pre-backup external bytes at their original paths. Any disagreement
    // with Official State remains a blocked mirror; exports are always available.
    for doc in store.list()? {
        let external = stage.0.join("external").join(&doc.relative_path);
        if external.exists() {
            put(
                &stage.0,
                &doc.relative_path,
                &std::fs::read(external).map_err(io)?,
            )?;
        } else {
            let loaded = store.load(&doc.id)?;
            put(
                &stage.0,
                &doc.relative_path,
                &encode_mirror(&loaded.snapshot.text, doc.newline, doc.bom),
            )?;
        }
    }
    drop(store);
    publish(&stage, destination)
}
