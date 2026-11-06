//! 写字台的真实文稿状态:项目 → WritingStore → 单活动文稿。
//!
//! 这是宿主适配层的纯状态部分:没有窗口、没有 webview。`MarkdownPanel` 负责把
//! 这里的文稿投影到页面;测试可以直接用临时项目验证保存/重开/恢复。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use digger_editor::doc::{Doc, State};
use digger_store::writing::{
    CreateOutcome, Document, DocumentId, LoadedDocument, NewDocument, NewlineStyle, RecoveryClass,
    RecoveryDraft, RecoveryRead, RecoveryStore, SaveReceipt, SaveRequest, WritingError,
    WritingSnapshot, WritingStore, classify, read_source,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NoProject,
    Empty,
    Ready,
    Dirty,
    Saving,
    Saved,
    SaveFailed(String),
    RecoveryPending { base_revision: u64 },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OpenDocument {
    pub document: Document,
    pub snapshot: WritingSnapshot,
    pub dirty: bool,
    pub buffer_revision: u64,
}

#[derive(Debug, Clone)]
pub struct RecoveredDraft {
    pub draft: RecoveryDraft,
    pub class: RecoveryClass,
}

/// 在后台线程执行的一次保存计划。UI 只负责准备/应用,不碰 SQLite 或镜像 IO。
pub struct SavePlan {
    store: Arc<WritingStore>,
    root: Option<PathBuf>,
    document: DocumentId,
    expected_revision: u64,
    operation_id: String,
    snapshot: WritingSnapshot,
    recovery: Option<Arc<RecoveryStore>>,
    recovery_sessions: Vec<String>,
    identity: WritingIdentity,
    buffer_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritingIdentity {
    pub root: PathBuf,
    pub store: String,
    pub document: DocumentId,
    pub epoch: u64,
}

#[derive(Debug, Clone)]
pub struct SaveOutcome {
    pub identity: WritingIdentity,
    pub buffer_revision: u64,
    pub receipt: SaveReceipt,
    pub snapshot: WritingSnapshot,
    pub mirror_error: Option<String>,
    pub documents: Option<Vec<Document>>,
}

pub struct RecoveryPlan {
    recovery: Arc<RecoveryStore>,
    draft: RecoveryDraft,
}

impl RecoveryPlan {
    pub fn run(self) -> Result<(), WritingError> {
        self.recovery.write(&self.draft)
    }
}

impl SavePlan {
    pub fn run(self) -> Result<SaveOutcome, WritingError> {
        let receipt = self.store.save(SaveRequest {
            document: self.document.clone(),
            expected_revision: self.expected_revision,
            operation_id: self.operation_id,
            snapshot: self.snapshot.clone(),
            expected_file_digest: self.store.mirror_digest(&self.document)?,
        })?;
        let mut mirror_error = None;
        if let Some(root) = &self.root {
            match self.store.mirror_pending(root, 8) {
                Ok(results) => {
                    mirror_error = results.into_iter().find_map(|(_, result)| result.err())
                }
                Err(error) => mirror_error = Some(error.to_string()),
            }
        }
        if mirror_error.is_none() {
            mirror_error = self
                .store
                .mirror_notice()
                .unwrap_or_else(|e| Some(e.to_string()));
        }
        if let Some(recovery) = &self.recovery {
            for session in &self.recovery_sessions {
                let _ = recovery.remove_committed(&self.document, session, &self.snapshot);
            }
        }
        let documents = self.store.list().ok();
        Ok(SaveOutcome {
            identity: self.identity,
            buffer_revision: self.buffer_revision,
            receipt,
            snapshot: self.snapshot,
            mirror_error,
            documents,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Workbench {
    epoch: u64,
    project_root: Option<PathBuf>,
    store: Option<Arc<WritingStore>>,
    recovery: Option<Arc<RecoveryStore>>,
    session_id: String,
    documents: Vec<Document>,
    current: Option<OpenDocument>,
    status: Status,
    operation_seq: u64,
    last_receipt: Option<SaveReceipt>,
    recovery_pending: Option<RecoveredDraft>,
    /// 用户显式接受的那份草稿所属会话;保存成功后只清这一份。
    accepted_recovery_session: Option<String>,
    last_mirror_error: Option<String>,
    recovery_warning: Option<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn snapshot_for(text: &str) -> (State, WritingSnapshot) {
    let mut doc = Doc::new();
    doc.load(text);
    let state = doc.export_state();
    let snapshot = WritingSnapshot::new(
        state.text.clone(),
        serde_json::to_string(&state).expect("editor state serialises"),
        1,
    );
    (state, snapshot)
}

impl Workbench {
    pub fn new() -> Self {
        Self {
            epoch: 0,
            project_root: None,
            store: None,
            recovery: None,
            session_id: format!("session-{}-{}", std::process::id(), now_ms()),
            documents: vec![],
            current: None,
            status: Status::NoProject,
            operation_seq: 0,
            last_receipt: None,
            recovery_pending: None,
            accepted_recovery_session: None,
            last_mirror_error: None,
            recovery_warning: None,
        }
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn current(&self) -> Option<&OpenDocument> {
        self.current.as_ref()
    }

    pub fn recovery_warning(&self) -> Option<&str> {
        self.recovery_warning.as_deref()
    }

    pub fn recovery_pending(&self) -> Option<&RecoveredDraft> {
        self.recovery_pending.as_ref()
    }

    pub fn retry_mirrors(&self) -> Result<(), WritingError> {
        if let Some(store) = &self.store {
            store.retry_failed_mirrors()?;
        }
        Ok(())
    }

    pub fn replay_mirrors(&mut self) -> Result<(), WritingError> {
        let (Some(store), Some(root)) = (&self.store, &self.project_root) else {
            return Ok(());
        };
        self.last_mirror_error = store
            .mirror_pending(root, 8)?
            .into_iter()
            .find_map(|(_, result)| result.err());
        if self.last_mirror_error.is_none() {
            self.last_mirror_error = store.mirror_notice()?;
        }
        Ok(())
    }

    pub fn external_review(&self) -> Result<digger_store::writing::ExternalReview, WritingError> {
        self.store
            .as_ref()
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?
            .review_external(
                &self
                    .current
                    .as_ref()
                    .ok_or_else(|| WritingError::NotFound("当前文稿".into()))?
                    .document
                    .id,
            )
    }

    pub fn resolve_external(
        &self,
        review: &digger_store::writing::ExternalReview,
        copy: bool,
    ) -> Result<Vec<Document>, WritingError> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?;
        if copy {
            store.copy_reviewed_external(review)?;
        } else {
            store.overwrite_reviewed_mirror(review)?;
        }
        store.list()
    }

    pub fn apply_document_list(&mut self, documents: Vec<Document>) {
        self.documents = documents;
    }
    pub fn set_mirror_notice(&mut self, notice: Option<String>) {
        self.last_mirror_error = notice;
    }

    pub fn mirror_error(&self) -> Option<&str> {
        self.last_mirror_error.as_deref()
    }

    /// 打开项目。只读取已有 Writing 库;没有库时保持空态,等首次新建/导入再创建。
    pub fn open_project(&mut self, root: PathBuf) -> Result<(), WritingError> {
        if !root.is_dir() {
            self.status = Status::Error(format!("项目目录不存在: {}", root.display()));
            return Ok(());
        }
        self.project_root = Some(root.clone());
        self.current = None;
        self.recovery_pending = None;
        self.recovery_warning = None;
        self.accepted_recovery_session = None;
        self.last_mirror_error = None;
        self.recovery = None;
        self.store = None;
        self.documents.clear();
        if WritingStore::exists(&root) {
            let store = Arc::new(WritingStore::open(&root)?);
            self.documents = store.list()?;
            self.store = Some(store);
        }
        self.status = if self.documents.is_empty() {
            Status::Empty
        } else {
            Status::Ready
        };
        Ok(())
    }

    pub fn close_project(&mut self) {
        self.epoch = self.epoch.checked_add(1).expect("writing epoch exhausted");
        self.project_root = None;
        self.store = None;
        self.recovery = None;
        self.documents.clear();
        self.current = None;
        self.recovery_pending = None;
        self.recovery_warning = None;
        self.accepted_recovery_session = None;
        self.last_mirror_error = None;
        self.status = Status::NoProject;
    }

    fn ensure_store(&mut self) -> Result<Arc<WritingStore>, WritingError> {
        if let Some(store) = &self.store {
            return Ok(Arc::clone(store));
        }
        let root = self
            .project_root
            .as_ref()
            .ok_or_else(|| WritingError::NotFound("项目".into()))?;
        let store = Arc::new(WritingStore::open(root)?);
        self.store = Some(Arc::clone(&store));
        Ok(store)
    }

    fn recovery_store(&mut self) -> Result<Arc<RecoveryStore>, WritingError> {
        if let Some(recovery) = &self.recovery {
            return Ok(Arc::clone(recovery));
        }
        let root = self
            .project_root
            .as_ref()
            .ok_or_else(|| WritingError::NotFound("项目".into()))?;
        let recovery = Arc::new(RecoveryStore::for_project(root));
        self.recovery = Some(Arc::clone(&recovery));
        Ok(recovery)
    }

    fn refresh_documents(&mut self) -> Result<(), WritingError> {
        let Some(store) = &self.store else {
            self.documents.clear();
            return Ok(());
        };
        self.documents = store.list()?;
        Ok(())
    }

    /// 新建空白文稿。标题为空时用路径 stem。
    pub fn create(&mut self, title: &str, relative_path: &str) -> Result<Document, WritingError> {
        let (_, snapshot) = snapshot_for("");
        self.create_with_snapshot(
            title,
            relative_path,
            snapshot,
            NewlineStyle::Lf,
            false,
            None,
        )
    }

    /// 导入项目内 Markdown。目标路径已存在时返回已有文稿,不覆盖源文件。
    pub fn import(&mut self, relative_path: &str) -> Result<Document, WritingError> {
        let root = self
            .project_root
            .clone()
            .ok_or_else(|| WritingError::NotFound("项目".into()))?;
        let source = read_source(&root, relative_path)
            .map_err(|error| WritingError::Unavailable(error.to_string()))?;
        let (_, snapshot) = snapshot_for(&source.text);
        let title = Path::new(relative_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(relative_path)
            .to_owned();
        self.create_with_snapshot(
            &title,
            relative_path,
            snapshot,
            source.newline,
            source.bom,
            Some(source.file_digest),
        )
    }

    fn create_with_snapshot(
        &mut self,
        title: &str,
        relative_path: &str,
        snapshot: WritingSnapshot,
        newline: NewlineStyle,
        bom: bool,
        source_file_digest: Option<String>,
    ) -> Result<Document, WritingError> {
        let store = self.ensure_store()?;
        let outcome = store.create(NewDocument {
            title: title.to_owned(),
            relative_path: relative_path.to_owned(),
            snapshot,
            newline,
            bom,
            source_file_digest,
        })?;
        let document = match outcome {
            CreateOutcome::Created(document) | CreateOutcome::Existing(document) => document,
        };
        self.refresh_documents()?;
        self.open_document(&document.id)?;
        Ok(document)
    }

    pub fn open_document(&mut self, id: &DocumentId) -> Result<LoadedDocument, WritingError> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?;
        let loaded = store.load(id)?;
        self.epoch = self.epoch.checked_add(1).expect("writing epoch exhausted");
        self.session_id = format!("session-{}", super::fresh_nonce());
        self.current = Some(OpenDocument {
            document: loaded.document.clone(),
            snapshot: loaded.snapshot.clone(),
            dirty: false,
            buffer_revision: 0,
        });
        self.status = Status::Ready;
        self.accepted_recovery_session = None;
        self.recovery_pending = None;
        self.recovery_warning = None;
        self.find_recovery();
        Ok(loaded)
    }

    fn find_recovery(&mut self) {
        let Some(current) = self.current.clone() else {
            return;
        };
        let Ok(recovery) = self.recovery_store() else {
            return;
        };
        let paths = match recovery.list_document(&current.document.id) {
            Ok(paths) => paths,
            Err(error) => {
                self.recovery_warning = Some(format!("恢复草稿不可读取，原件保留：{error}"));
                return;
            }
        };
        let mut drafts = vec![];
        for path in paths {
            let Some(session) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let draft = match recovery.read(&current.document.id, session) {
                Ok(RecoveryRead::Draft(draft)) => draft,
                Ok(RecoveryRead::Missing) => continue,
                Ok(RecoveryRead::Corrupt { reason, .. }) => {
                    self.recovery_warning =
                        Some(format!("恢复草稿损坏或格式不受支持，原件保留：{reason}"));
                    continue;
                }
                Err(error) => {
                    self.recovery_warning = Some(format!("恢复草稿不可读取，原件保留：{error}"));
                    continue;
                }
            };
            if self
                .store
                .as_ref()
                .is_none_or(|store| draft.store_id != store.store_id().as_str())
            {
                self.recovery_warning = Some("发现属于另一文稿库的恢复草稿，原件保留".into());
                continue;
            }
            drafts.push(draft);
        }
        drafts.sort_by_key(|draft| std::cmp::Reverse(draft.updated_ms));
        for draft in drafts {
            let class = classify(&draft, current.document.revision, &current.snapshot);
            if class != RecoveryClass::Redundant {
                self.status = Status::RecoveryPending {
                    base_revision: draft.base_revision,
                };
                self.recovery_pending = Some(RecoveredDraft { draft, class });
                return;
            }
        }
    }

    pub fn mark_dirty(&mut self, buffer_revision: u64) {
        if let Some(current) = self.current.as_mut() {
            current.dirty = true;
            current.buffer_revision = buffer_revision;
            self.status = Status::Dirty;
        }
    }

    /// 准备一次后台恢复草稿写入。
    pub fn prepare_recovery(
        &mut self,
        text: &str,
        state_json: &str,
        schema: u32,
        buffer_revision: u64,
    ) -> Result<RecoveryPlan, WritingError> {
        let (document, revision) = self
            .current
            .as_ref()
            .map(|current| (current.document.id.clone(), current.document.revision))
            .ok_or_else(|| WritingError::NotFound("当前文稿".into()))?;
        let store_id = self
            .store
            .as_ref()
            .map(|store| store.store_id().clone())
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?;
        let recovery = self.recovery_store()?;
        let draft = RecoveryDraft::new(
            &store_id,
            &document,
            self.session_id.clone(),
            revision,
            buffer_revision,
            text,
            state_json,
            schema,
            now_ms(),
        );
        Ok(RecoveryPlan { recovery, draft })
    }

    pub fn identity(&self) -> Option<WritingIdentity> {
        Some(WritingIdentity {
            root: self.project_root.clone()?,
            store: self.store.as_ref()?.store_id().as_str().to_owned(),
            document: self.current.as_ref()?.document.id.clone(),
            epoch: self.epoch,
        })
    }

    pub fn mark_save_failed(&mut self, error: impl std::fmt::Display) {
        self.status = Status::SaveFailed(error.to_string());
    }

    /// 写入恢复草稿。调用方提供页面/桥确认过的正文与块身份。
    pub fn write_recovery(
        &mut self,
        text: &str,
        state_json: &str,
        schema: u32,
        buffer_revision: u64,
    ) -> Result<(), WritingError> {
        let (document, revision) = self
            .current
            .as_ref()
            .map(|current| (current.document.id.clone(), current.document.revision))
            .ok_or_else(|| WritingError::NotFound("当前文稿".into()))?;
        let store_id = self
            .store
            .as_ref()
            .map(|store| store.store_id().clone())
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?;
        let recovery = self.recovery_store()?;
        let draft = RecoveryDraft::new(
            &store_id,
            &document,
            self.session_id.clone(),
            revision,
            buffer_revision,
            text,
            state_json,
            schema,
            now_ms(),
        );
        recovery.write(&draft)
    }

    pub fn clear_recovery_notice(&mut self) {
        self.recovery_pending = None;
        self.status = if self.current.as_ref().is_some_and(|doc| doc.dirty) {
            Status::Dirty
        } else {
            Status::Ready
        };
    }

    pub fn discard_recovery(&mut self) -> Result<(), WritingError> {
        let Some(document) = self
            .current
            .as_ref()
            .map(|current| current.document.id.clone())
        else {
            return Ok(());
        };
        let session = self
            .recovery_pending
            .as_ref()
            .map(|pending| pending.draft.session_id.clone())
            .unwrap_or_else(|| self.session_id.clone());
        let recovery = self.recovery_store()?;
        recovery.remove(&document, &session)?;
        self.recovery_pending = None;
        if let Some(current) = self.current.as_ref() {
            self.status = if current.dirty {
                Status::Dirty
            } else {
                Status::Ready
            };
        }
        Ok(())
    }

    /// 准备一次保存:在 UI 线程只做快照/身份准备,真正的 SQLite 与镜像 IO 由
    /// [`SavePlan::run`] 在后台执行。
    pub fn prepare_save(&mut self, snapshot: WritingSnapshot) -> Result<SavePlan, WritingError> {
        let store = self
            .store
            .clone()
            .ok_or_else(|| WritingError::NotFound("Writing 库".into()))?;
        let current = self
            .current
            .clone()
            .ok_or_else(|| WritingError::NotFound("当前文稿".into()))?;
        self.operation_seq += 1;
        let operation_id = format!("{}-{}", self.session_id, self.operation_seq);
        self.status = Status::Saving;
        let mut recovery_sessions = vec![self.session_id.clone()];
        if let Some(session) = &self.accepted_recovery_session {
            recovery_sessions.push(session.clone());
        }
        Ok(SavePlan {
            store: Arc::clone(&store),
            root: self.project_root.clone(),
            document: current.document.id,
            expected_revision: current.document.revision,
            operation_id,
            snapshot,
            recovery: self.recovery.clone(),
            recovery_sessions,
            identity: self.identity().expect("active writing identity"),
            buffer_revision: current.buffer_revision,
        })
    }

    pub fn apply_save(&mut self, outcome: SaveOutcome) -> Option<SaveReceipt> {
        if self.identity().as_ref() != Some(&outcome.identity) {
            return None;
        }
        if let Some(current) = self.current.as_mut() {
            current.snapshot = outcome.snapshot;
            current.document.revision = outcome.receipt.revision;
            current.document.updated_ms = now_ms();
            current.dirty = current.buffer_revision != outcome.buffer_revision;
        }
        self.last_receipt = Some(outcome.receipt.clone());
        self.recovery_pending = None;
        self.accepted_recovery_session = None;
        self.last_mirror_error = outcome.mirror_error;
        if let Some(documents) = outcome.documents {
            self.documents = documents;
        }
        self.status = if self.current.as_ref().is_some_and(|current| current.dirty) {
            Status::Dirty
        } else {
            Status::Saved
        };
        Some(outcome.receipt)
    }

    /// 同步保存(单元测试/显式转换路径);生产面板使用 prepare + 后台 run + apply。
    #[cfg(test)]
    pub fn save(&mut self, snapshot: WritingSnapshot) -> Result<SaveReceipt, WritingError> {
        let plan = match self.prepare_save(snapshot) {
            Ok(plan) => plan,
            Err(error) => {
                self.status = Status::SaveFailed(error.to_string());
                return Err(error);
            }
        };
        match plan.run() {
            Ok(outcome) => {
                let receipt = outcome.receipt.clone();
                self.apply_save(outcome);
                Ok(receipt)
            }
            Err(error) => {
                self.status = Status::SaveFailed(error.to_string());
                Err(error)
            }
        }
    }

    /// 可恢复草稿转成显式 Recovery Change 的输入。
    ///
    /// Behind/Conflict 不能在这里进入普通 Save:它们必须走另存副本或显式冲突流程,
    /// 否则会用旧基线的草稿覆盖更新的 Official State。
    pub fn accepted_recovery(&mut self) -> Option<(State, String)> {
        let pending = self.recovery_pending.as_ref()?;
        if pending.class != RecoveryClass::Recoverable {
            return None;
        }
        let state = serde_json::from_str::<State>(&pending.draft.state_json).ok()?;
        self.accepted_recovery_session = Some(pending.draft.session_id.clone());
        let text = pending.draft.text.clone();
        // Restoring may reset the editor's buffer revision. Give that projection
        // a fresh identity instead of comparing it against the superseded buffer.
        self.epoch = self.epoch.checked_add(1).expect("writing epoch exhausted");
        self.session_id = format!("session-{}", super::fresh_nonce());
        self.recovery_pending = None;
        self.status = Status::Dirty;
        Some((state, text))
    }

    /// 把任意恢复草稿另存为新文稿,不覆盖当前 Official State。
    pub fn save_recovery_copy(&mut self) -> Result<Document, WritingError> {
        let Some(pending) = self.recovery_pending.clone() else {
            return Err(WritingError::NotFound("恢复草稿".into()));
        };
        let state: State = serde_json::from_str(&pending.draft.state_json)
            .map_err(|error| WritingError::InvalidSnapshot(error.to_string()))?;
        let snapshot = WritingSnapshot::new(
            pending.draft.text.clone(),
            serde_json::to_string(&state).expect("state serialises"),
            pending.draft.schema,
        );
        let base = self
            .current
            .as_ref()
            .map(|current| current.document.relative_path.clone())
            .unwrap_or_else(|| "recovered.md".into());
        let stem = std::path::Path::new(&base)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("recovered");
        let parent = std::path::Path::new(&base)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.to_string_lossy().into_owned());
        let newline = self
            .current
            .as_ref()
            .map(|current| current.document.newline)
            .unwrap_or(NewlineStyle::Lf);
        let bom = self
            .current
            .as_ref()
            .map(|current| current.document.bom)
            .unwrap_or(false);
        let store = self.ensure_store()?;
        for attempt in 0..1000 {
            let name = if attempt == 0 {
                format!("{stem}.recovered.md")
            } else {
                format!("{stem}.recovered-{attempt}.md")
            };
            let relative = match &parent {
                Some(parent) => format!("{parent}/{name}"),
                None => name,
            };
            let outcome = store.create(NewDocument {
                title: format!("{stem}（恢复副本）"),
                relative_path: relative,
                snapshot: snapshot.clone(),
                newline,
                bom,
                source_file_digest: None,
            })?;
            if let CreateOutcome::Created(document) = outcome {
                self.refresh_documents()?;
                self.open_document(&document.id)?;
                return Ok(document);
            }
        }
        Err(WritingError::Unavailable("恢复副本命名已耗尽".into()))
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn opening_a_project_without_a_writing_db_does_not_create_one() {
        let dir = project();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        assert_eq!(workbench.status(), &Status::Empty);
        assert!(!WritingStore::exists(dir.path()));
    }

    #[test]
    fn create_save_and_reopen_preserve_text_and_identity() {
        let dir = project();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        let doc = workbench.create("第一章", "book/01.md").unwrap();
        let id = doc.id.clone();
        let (_, snapshot) = snapshot_for("# 新章\n");
        workbench.mark_dirty(1);
        let receipt = workbench.save(snapshot).unwrap();
        assert_eq!(receipt.revision, 2);
        assert_eq!(workbench.status(), &Status::Saved);

        let mut reopened = Workbench::new();
        reopened.open_project(dir.path().to_path_buf()).unwrap();
        let loaded = reopened.open_document(&id).unwrap();
        assert_eq!(loaded.snapshot.text, "# 新章\n");
        assert_eq!(loaded.document.revision, 2);
    }

    #[test]
    fn import_preserves_bom_crlf_and_duplicate_path_returns_existing() {
        let dir = project();
        std::fs::write(dir.path().join("a.md"), b"\xEF\xBB\xBFone\r\ntwo\r\n").unwrap();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        let doc = workbench.import("a.md").unwrap();
        assert_eq!(doc.newline, NewlineStyle::Crlf);
        assert!(doc.bom);
        assert_eq!(workbench.current().unwrap().snapshot.text, "one\ntwo\n");
        let again = workbench.import("a.md").unwrap();
        assert_eq!(again.id, doc.id);
    }

    #[test]
    fn a_stale_save_reports_the_actual_revision() {
        let dir = project();
        let mut first = Workbench::new();
        first.open_project(dir.path().to_path_buf()).unwrap();
        let doc = first.create("A", "a.md").unwrap();
        let (_, one) = snapshot_for("one");
        first.save(one).unwrap();

        let mut second = Workbench::new();
        second.open_project(dir.path().to_path_buf()).unwrap();
        second.open_document(&doc.id).unwrap();
        let (_, two) = snapshot_for("two");
        first.save(two).unwrap();

        let (_, three) = snapshot_for("three");
        let error = second.save(three).unwrap_err();
        assert!(matches!(
            error,
            WritingError::RevisionConflict {
                expected: 2,
                actual: 3
            }
        ));
        assert!(matches!(second.status(), Status::SaveFailed(_)));
        assert_eq!(second.current().unwrap().document.id, doc.id);
    }

    #[test]
    fn first_import_mirror_does_not_overwrite_a_file_changed_after_import() {
        let dir = project();
        std::fs::write(dir.path().join("a.md"), "source").unwrap();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        workbench.import("a.md").unwrap();
        std::fs::write(dir.path().join("a.md"), "EXTERNAL_NEW").unwrap();
        workbench.save(snapshot_for("APP_NEW").1).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "EXTERNAL_NEW"
        );
        assert!(
            workbench
                .mirror_error()
                .is_some_and(|error| error.contains("external_change")),
            "{:?}",
            workbench.mirror_error()
        );
    }

    #[test]
    fn external_file_change_blocks_the_next_mirror_without_rolling_back_the_save() {
        let dir = project();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        workbench.create("A", "a.md").unwrap();
        let (_, one) = snapshot_for("one");
        workbench.save(one).unwrap();
        std::fs::write(dir.path().join("a.md"), "external edit").unwrap();
        let (_, two) = snapshot_for("two");
        workbench.save(two).unwrap();
        assert_eq!(workbench.status(), &Status::Saved);
        assert!(
            workbench
                .mirror_error()
                .is_some_and(|error| error.contains("external_change")),
            "mirror error was {:?}",
            workbench.mirror_error()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "external edit",
            "mirror must not silently overwrite an external edit"
        );
    }

    #[test]
    fn a_recovered_session_uses_a_new_operation_id_and_can_save() {
        let dir = project();
        let mut first = Workbench::new();
        first.open_project(dir.path().to_path_buf()).unwrap();
        let doc = first.create("A", "a.md").unwrap();
        let (_, official) = snapshot_for("official");
        first.save(official).unwrap();
        let (state, _) = snapshot_for("draft");
        first
            .write_recovery("draft", &serde_json::to_string(&state).unwrap(), 1, 2)
            .unwrap();

        let mut reopened = Workbench::new();
        reopened.open_project(dir.path().to_path_buf()).unwrap();
        reopened.open_document(&doc.id).unwrap();
        let (state, text) = reopened.accepted_recovery().unwrap();
        let receipt = reopened
            .save(WritingSnapshot::new(
                text,
                serde_json::to_string(&state).unwrap(),
                1,
            ))
            .unwrap();
        assert_eq!(receipt.revision, 3);
        assert_eq!(reopened.current().unwrap().snapshot.text, "draft");
    }

    #[test]
    fn behind_recovery_cannot_overwrite_newer_official_state() {
        let dir = project();
        let mut first = Workbench::new();
        first.open_project(dir.path().to_path_buf()).unwrap();
        let doc = first.create("A", "a.md").unwrap();
        let (state, _) = snapshot_for("old-draft");
        first
            .write_recovery("old-draft", &serde_json::to_string(&state).unwrap(), 1, 2)
            .unwrap();
        let store = WritingStore::open(dir.path()).unwrap();
        store
            .save(SaveRequest {
                document: doc.id.clone(),
                expected_revision: 1,
                operation_id: "external-update".into(),
                snapshot: snapshot_for("NEW_OFFICIAL").1,
                expected_file_digest: None,
            })
            .unwrap();

        let mut reopened = Workbench::new();
        reopened.open_project(dir.path().to_path_buf()).unwrap();
        reopened.open_document(&doc.id).unwrap();
        assert_eq!(
            reopened.recovery_pending().unwrap().class,
            RecoveryClass::Behind
        );
        assert!(reopened.accepted_recovery().is_none());
        let copy = reopened.save_recovery_copy().unwrap();
        assert_ne!(copy.id, doc.id);
        assert_eq!(store.load(&doc.id).unwrap().snapshot.text, "NEW_OFFICIAL");
        assert_eq!(store.load(&copy.id).unwrap().snapshot.text, "old-draft");
    }

    #[test]
    fn recovery_round_trip_is_visible_until_discarded() {
        let dir = project();
        let mut workbench = Workbench::new();
        workbench.open_project(dir.path().to_path_buf()).unwrap();
        workbench.create("A", "a.md").unwrap();
        let (state, _) = snapshot_for("draft");
        workbench
            .write_recovery(&state.text, &serde_json::to_string(&state).unwrap(), 1, 1)
            .unwrap();
        workbench.find_recovery();
        assert!(matches!(workbench.status(), Status::RecoveryPending { .. }));
        workbench.discard_recovery().unwrap();
        assert_eq!(workbench.status(), &Status::Ready);
    }
}
