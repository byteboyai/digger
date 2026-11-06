//! 恢复草稿:项目 `.digger/working/<document>/<session>.json` 的原子读写与分类。
//!
//! 恢复草稿**不是** Official State。它只记录“页面/会话认为尚未提交的 buffer”,
//! 写入失败必须可见;损坏/未来格式保留原件,不静默删除或覆盖。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::{DocumentId, StoreId, WritingError};

pub const RECOVERY_SCHEMA: u32 = 1;
pub const MAX_RECOVERY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDraft {
    pub store_id: String,
    pub document_id: String,
    pub session_id: String,
    /// 草稿基于的 durable revision。
    pub base_revision: u64,
    /// 页面 buffer 自己递增的水位(不是数据库 revision)。
    pub buffer_revision: u64,
    pub text: String,
    pub state_json: String,
    pub schema: u32,
    pub updated_ms: i64,
    /// 覆盖上面字段的摘要;读取时必须重新计算并比对。
    pub digest: String,
}

impl RecoveryDraft {
    pub fn new(
        store: &StoreId,
        document: &DocumentId,
        session_id: impl Into<String>,
        base_revision: u64,
        buffer_revision: u64,
        text: impl Into<String>,
        state_json: impl Into<String>,
        schema: u32,
        updated_ms: i64,
    ) -> Self {
        let mut draft = Self {
            store_id: store.0.clone(),
            document_id: document.0.clone(),
            session_id: session_id.into(),
            base_revision,
            buffer_revision,
            text: text.into(),
            state_json: state_json.into(),
            schema,
            updated_ms,
            digest: String::new(),
        };
        draft.digest = draft.compute_digest();
        draft
    }

    fn compute_digest(&self) -> String {
        let mut hasher = Sha256::new();
        for part in [
            &self.store_id,
            &self.document_id,
            &self.session_id,
            &self.base_revision.to_string(),
            &self.buffer_revision.to_string(),
            &self.text,
            &self.state_json,
            &self.schema.to_string(),
        ] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    pub fn verify(&self) -> Result<(), WritingError> {
        if self.digest != self.compute_digest() {
            return Err(WritingError::Corrupt("恢复草稿摘要不匹配".into()));
        }
        if self.schema > RECOVERY_SCHEMA {
            return Err(WritingError::UnsupportedSchema {
                found: self.schema,
                supported: RECOVERY_SCHEMA,
            });
        }
        if self.text.len() > MAX_RECOVERY_BYTES {
            return Err(WritingError::Limit("恢复草稿超过容量上限".into()));
        }
        if serde_json::to_vec(self)
            .map_err(|e| WritingError::Unavailable(e.to_string()))?
            .len()
            > MAX_RECOVERY_BYTES
        {
            return Err(WritingError::Limit("恢复草稿封装超过容量上限".into()));
        }
        super::WritingSnapshot::new(self.text.clone(), self.state_json.clone(), self.schema)
            .validate(super::MAX_TEXT_BYTES, super::MAX_STATE_BYTES)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryRead {
    Missing,
    Draft(RecoveryDraft),
    /// 文件存在但不可解析/摘要错误/schema 更高:原件保留,调用方展示可导出错误。
    Corrupt {
        path: PathBuf,
        reason: String,
    },
}

/// 与当前 durable 版本比较后的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryClass {
    /// 基于当前版本,且内容与 durable 不同:可以展示恢复预览。
    Recoverable,
    /// 与 durable 内容相同:可证明冗余,允许清理。
    Redundant,
    /// 基版本落后:保留为冲突候选,不能直接覆盖当前正文。
    Behind,
    /// 基版本比库内还新(库被替换/回滚):保留为冲突候选。
    Conflict,
}

pub fn classify(
    draft: &RecoveryDraft,
    durable_revision: u64,
    durable: &super::types::WritingSnapshot,
) -> RecoveryClass {
    if draft.base_revision < durable_revision {
        return RecoveryClass::Behind;
    }
    if draft.base_revision > durable_revision {
        return RecoveryClass::Conflict;
    }
    if draft.text == durable.text && draft.state_json == durable.state_json {
        return RecoveryClass::Redundant;
    }
    RecoveryClass::Recoverable
}

#[derive(Debug)]
pub struct RecoveryStore {
    working: PathBuf,
}

impl RecoveryStore {
    /// Pure path preparation; all directory creation belongs to the IO worker.
    pub fn for_project(root: &Path) -> Self {
        Self {
            working: root.join(".digger/working"),
        }
    }

    pub fn open(project_root: &Path) -> Result<Self, WritingError> {
        let prepared = Self::for_project(project_root);
        let _ = prepared.path_for(&DocumentId("probe".into()), "probe")?;
        let working = prepared.working;
        std::fs::create_dir_all(&working)
            .map_err(|error| WritingError::Unavailable(format!("创建 working 失败: {error}")))?;
        Ok(Self { working })
    }

    /// 文件名只来自已验证的 store/document/session 标识,不拼接用户路径。
    fn path_for(&self, document: &DocumentId, session: &str) -> Result<PathBuf, WritingError> {
        if !safe_component(document.as_str()) || !safe_component(session) {
            return Err(WritingError::InvalidPath("恢复草稿身份含不安全字符".into()));
        }
        for directory in [
            self.working.parent().unwrap().to_path_buf(),
            self.working.clone(),
            self.working.join(".locks"),
            self.working.join(document.as_str()),
        ] {
            if let Ok(meta) = std::fs::symlink_metadata(directory)
                && (meta.file_type().is_symlink() || !meta.is_dir())
            {
                return Err(WritingError::InvalidPath(
                    "恢复草稿目录包含链接或非常规目录".into(),
                ));
            }
        }
        Ok(self
            .working
            .join(document.as_str())
            .join(format!("{session}.json")))
    }

    pub fn write(&self, draft: &RecoveryDraft) -> Result<(), WritingError> {
        draft.verify()?;
        let path = self.path_for(&DocumentId(draft.document_id.clone()), &draft.session_id)?;
        let _lock = dozer_file_io::lock::WriteLock::acquire(
            &self.working.join(".locks"),
            &path,
            std::time::Duration::from_secs(5),
        )
        .map_err(|error| WritingError::Unavailable(error.to_string()))?;
        if let RecoveryRead::Draft(existing) =
            self.read(&DocumentId(draft.document_id.clone()), &draft.session_id)?
        {
            if existing.store_id != draft.store_id {
                return Err(WritingError::Corrupt("恢复草稿所属库不一致".into()));
            }
            if (existing.base_revision, existing.buffer_revision)
                > (draft.base_revision, draft.buffer_revision)
            {
                return Ok(());
            }
        }
        let parent = path
            .parent()
            .ok_or_else(|| WritingError::InvalidPath("恢复草稿目录非法".into()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| WritingError::Unavailable(format!("创建草稿目录失败: {error}")))?;
        let bytes = serde_json::to_vec(draft)
            .map_err(|error| WritingError::Unavailable(format!("序列化草稿失败: {error}")))?;
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|error| {
                    WritingError::Unavailable(format!("创建草稿临时文件失败: {error}"))
                })?;
            file.write_all(&bytes)
                .map_err(|error| WritingError::Unavailable(format!("写草稿失败: {error}")))?;
            file.sync_all()
                .map_err(|error| WritingError::Unavailable(format!("同步草稿失败: {error}")))?;
        }
        #[cfg(test)]
        crate::crash_at("writing-recovery-before-rename");
        std::fs::rename(&tmp, &path)
            .map_err(|error| WritingError::Unavailable(format!("替换草稿失败: {error}")))?;
        // 目录 fsync 尽力而为:失败不回滚已替换的草稿,但错误可见。
        if let Some(dir) = path.parent()
            && let Ok(file) = std::fs::File::open(dir)
        {
            let _ = file.sync_all();
        }
        Ok(())
    }

    /// Only remove the exact committed buffer. A worker finishing an older save
    /// must never erase a newer recovery written while it was running.
    pub fn remove_committed(
        &self,
        document: &DocumentId,
        session: &str,
        snapshot: &super::types::WritingSnapshot,
    ) -> Result<(), WritingError> {
        let path = self.path_for(document, session)?;
        let _lock = dozer_file_io::lock::WriteLock::acquire(
            &self.working.join(".locks"),
            &path,
            std::time::Duration::from_secs(5),
        )
        .map_err(|error| WritingError::Unavailable(error.to_string()))?;
        if let RecoveryRead::Draft(draft) = self.read(document, session)? {
            if draft.text == snapshot.text && draft.state_json == snapshot.state_json {
                self.remove_unlocked(document, session)?;
            }
        }
        Ok(())
    }

    pub fn read(&self, document: &DocumentId, session: &str) -> Result<RecoveryRead, WritingError> {
        let path = self.path_for(document, session)?;
        // Bound allocation even if a recovery file grows during the read.
        let bytes = match (|| -> std::io::Result<Vec<u8>> {
            if !path.exists() {
                return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
            }
            let project = self.working.parent().unwrap().parent().unwrap();
            let root = dozer_file_io::ProjectRoot::open(project).map_err(std::io::Error::other)?;
            root.open_file(&path.strip_prefix(project).unwrap().to_string_lossy())
                .map_err(std::io::Error::other)?
                .read_limited(MAX_RECOVERY_BYTES)
                .map_err(std::io::Error::other)
        })() {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RecoveryRead::Missing);
            }
            Err(error) => {
                return Err(WritingError::Unavailable(format!(
                    "读取恢复草稿失败: {error}"
                )));
            }
        };
        match serde_json::from_slice::<RecoveryDraft>(&bytes) {
            Ok(draft) => match draft.verify() {
                Ok(()) if draft.document_id == document.as_str() && draft.session_id == session => {
                    Ok(RecoveryRead::Draft(draft))
                }
                Ok(()) => Ok(RecoveryRead::Corrupt {
                    path,
                    reason: "恢复草稿身份与文件位置不一致".into(),
                }),
                Err(error) => Ok(RecoveryRead::Corrupt {
                    path,
                    reason: error.to_string(),
                }),
            },
            Err(error) => Ok(RecoveryRead::Corrupt {
                path,
                reason: error.to_string(),
            }),
        }
    }

    pub fn remove(&self, document: &DocumentId, session: &str) -> Result<(), WritingError> {
        let path = self.path_for(document, session)?;
        let _lock = dozer_file_io::lock::WriteLock::acquire(
            &self.working.join(".locks"),
            &path,
            std::time::Duration::from_secs(5),
        )
        .map_err(|e| WritingError::Unavailable(e.to_string()))?;
        self.remove_unlocked(document, session)
    }

    fn remove_unlocked(&self, document: &DocumentId, session: &str) -> Result<(), WritingError> {
        let path = self.path_for(document, session)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(WritingError::Unavailable(format!(
                "删除恢复草稿失败: {error}"
            ))),
        }
    }

    pub fn list_document(&self, document: &DocumentId) -> Result<Vec<PathBuf>, WritingError> {
        if !safe_component(document.as_str()) {
            return Err(WritingError::InvalidPath("文稿身份不安全".into()));
        }
        let _ = self.path_for(document, "probe")?;
        let dir = self.working.join(document.as_str());
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => {
                return Err(WritingError::Unavailable(format!(
                    "读取草稿目录失败: {error}"
                )));
            }
        };
        Ok(entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .collect())
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
