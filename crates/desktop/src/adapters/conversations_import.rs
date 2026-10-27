//! Explicit history import and its independent, versioned prompt marker.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use bytehost_core::request::RequestToken;
use serde::{Deserialize, Serialize};

pub const LOG_DISCLOSURE: &str = "个别文件读取失败不会在这里列出,详情见 diggerd 日志";
pub const READ_DISCLOSURE: &str = "只显示 Digger 装好 hook 之后产生的会话。导入会读取 ~/.claude/ 等位置的会话文件,把文本存进 digger.db。";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImportState {
    #[default]
    Idle,
    Importing,
    Done(u32),
    Failed(String),
}
#[derive(Debug, Clone)]
pub enum ImportEvent {
    Start,
    Finished(RequestToken, Result<u32, String>),
}
#[derive(Debug, Clone, Copy)]
pub struct ImportPrompt;

#[derive(Debug, Serialize, Deserialize)]
struct ProjectMarker {
    at_ms: u64,
    count: u32,
}
#[derive(Debug, Serialize, Deserialize)]
struct MarkerFile {
    schema_version: u64,
    projects: BTreeMap<i64, ProjectMarker>,
}
impl Default for MarkerFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            projects: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Default)]
pub struct ImportMarker {
    file: MarkerFile,
    read_only: bool,
}
impl ImportMarker {
    pub fn load(path: &Path) -> Self {
        let Some(value) = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        else {
            return Self::default();
        };
        // Check the version before decoding the payload: future payloads can have
        // an entirely different shape and must still remain untouched.
        if value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .is_some_and(|v| v > 1)
        {
            return Self {
                read_only: true,
                ..Self::default()
            };
        }
        match serde_json::from_value::<MarkerFile>(value) {
            Ok(file) if file.schema_version == 1 => Self {
                file,
                read_only: false,
            },
            _ => Self::default(),
        }
    }
    pub fn is_marked(&self, pid: i64) -> bool {
        self.file.projects.contains_key(&pid)
    }
    pub fn mark(&mut self, pid: i64, count: u32, at_ms: u64) {
        self.file
            .projects
            .insert(pid, ProjectMarker { count, at_ms });
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.save_with_replace(path, |source, dest| std::fs::rename(source, dest))
    }
    fn save_with_replace(
        &self,
        path: &Path,
        replace: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(&self.file)?;
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let (temp, mut file) = loop {
            let temp = parent.join(format!(
                ".conversations-imported-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
            {
                Ok(file) => break (temp, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            replace(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_for_project_a_does_not_affect_project_b_and_paths_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        let mut marker = ImportMarker::load(&a);
        marker.mark(1, 7, 123);
        marker.save(&a).unwrap();
        let restored = ImportMarker::load(&a);
        assert!(restored.is_marked(1));
        assert!(!restored.is_marked(2));
        assert!(!ImportMarker::load(&b).is_marked(1));
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(a).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["projects"]["1"]["count"], 7);
        assert_eq!(value["projects"]["1"]["at_ms"], 123);
    }

    #[test]
    fn marker_file_corrupt_missing_or_newer_is_treated_as_empty_and_newer_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marker.json");
        assert!(!ImportMarker::load(&path).is_marked(1));
        for bytes in [
            b"broken".as_slice(),
            br#"{"schema_version":1,"projects":null}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(!ImportMarker::load(&path).is_marked(1));
        }
        let future = br#"{"schema_version":999,"projects":{"1":{"future":"shape"}}}"#;
        std::fs::write(&path, future).unwrap();
        let mut marker = ImportMarker::load(&path);
        assert!(!marker.is_marked(1));
        marker.mark(1, 7, 123);
        marker.save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), future);
    }

    #[test]
    fn marker_save_is_atomic_and_failed_replace_preserves_old_complete_bytes_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marker.json");
        let mut marker = ImportMarker::load(&path);
        marker.mark(1, 7, 123);
        marker.save(&path).unwrap();
        let old = std::fs::read(&path).unwrap();
        marker.mark(2, 8, 124);
        assert!(
            marker
                .save_with_replace(&path, |_, _| Err(std::io::Error::other(
                    "injected rename failure"
                )))
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        marker.save(&path).unwrap();
        assert!(ImportMarker::load(&path).is_marked(2));
    }
}
