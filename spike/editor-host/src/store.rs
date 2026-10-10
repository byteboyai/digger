//! 恢复日志(崩溃后的未提交缓冲区)的存放处。正式状态与历史在 `history.rs`。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct Journal {
    pub document_id: String,
    pub base_rev: u64,
    /// 块级增量(相对 `history[base_rev]`),与 edit/snapshot 载荷同形。
    pub delta: serde_json::Value,
}

pub struct Store {
    dir: PathBuf,
}

fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("create data dir");
        Self { dir }
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn journal_path(&self) -> PathBuf {
        self.dir.join("journal.json")
    }
    pub fn load_journal(&self) -> Option<Journal> {
        serde_json::from_slice(&std::fs::read(self.journal_path()).ok()?).ok()
    }
    pub fn save_journal(&self, j: &Journal) {
        write_durable(&self.journal_path(), &serde_json::to_vec(j).unwrap()).expect("save journal");
    }
    pub fn clear_journal(&self) {
        let _ = std::fs::remove_file(self.journal_path());
    }
}
