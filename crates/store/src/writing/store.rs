//! 文稿仓库:SQLite 短事务、版本/CAS、幂等提交与镜像 Outbox。
//!
//! 权威正文只有一份:`document_versions.text`。`documents.durable_revision`
//! 指向最新已提交版本;Commit、project revision、Outbox 与幂等结果在同一事务里
//! 一起落盘,任何一步失败都整体回滚。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::types::{
    CreateOutcome, Document, DocumentId, LoadedDocument, NewDocument, OutboxEntry, SaveReceipt,
    SaveRequest, StoreId, WritingError, WritingSnapshot, validate_relative_path,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const PROJECT_DB_RELATIVE: &str = ".digger/project.db";

#[derive(Debug)]
pub struct WritingStore {
    conn: Mutex<Connection>,
    root: PathBuf,
    store_id: StoreId,
    #[cfg(unix)]
    database_identity: (u64, u64),
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, WritingError> {
    mutex
        .lock()
        .map_err(|_| WritingError::Unavailable("文稿库锁已中毒".into()))
}

fn sql_error(error: rusqlite::Error) -> WritingError {
    WritingError::Unavailable(error.to_string())
}

fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

impl WritingStore {
    pub fn database_path(project_root: &Path) -> PathBuf {
        project_root.join(PROJECT_DB_RELATIVE)
    }

    /// 打开项目前先判断是否已有 Writing 库:打开项目本身不应创建空库。
    pub fn exists(project_root: &Path) -> bool {
        Self::database_path(project_root).is_file()
    }

    /// 打开(必要时初始化)项目内的 `.digger/project.db`。
    ///
    /// 已有库的 schema 更高时返回 [`WritingError::UnsupportedSchema`],调用方必须
    /// 以只读/报错处理,不能静默建空库或覆盖。
    pub fn open(project_root: &Path) -> Result<Self, WritingError> {
        if !project_root.is_dir() {
            return Err(WritingError::NotFound(format!(
                "项目目录 {}",
                project_root.display()
            )));
        }
        let dir = project_root.join(".digger");
        if std::fs::symlink_metadata(&dir)
            .is_ok_and(|meta| meta.file_type().is_symlink() || !meta.is_dir())
        {
            return Err(WritingError::InvalidPath(
                ".digger 必须是项目内真实目录".into(),
            ));
        }
        let database_path = project_root.join(PROJECT_DB_RELATIVE);
        if let Ok(meta) = std::fs::symlink_metadata(&database_path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(WritingError::InvalidPath("文稿数据库必须是常规文件".into()));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() != 1 {
                    return Err(WritingError::InvalidPath("文稿数据库不支持硬链接".into()));
                }
            }
        }
        std::fs::create_dir_all(&dir)
            .map_err(|error| WritingError::Unavailable(format!("创建 .digger 失败: {error}")))?;
        let path = project_root.join(PROJECT_DB_RELATIVE);
        let conn = Connection::open(&path).map_err(|error| {
            WritingError::Unavailable(format!("打开 {} 失败: {error}", path.display()))
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(sql_error)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
        )
        .map_err(sql_error)?;
        let store_id = ensure_schema(&conn)?;
        #[cfg(unix)]
        let database_identity = {
            use std::os::unix::fs::MetadataExt;
            let meta =
                std::fs::metadata(&path).map_err(|e| WritingError::Unavailable(e.to_string()))?;
            (meta.dev(), meta.ino())
        };
        Ok(Self {
            conn: Mutex::new(conn),
            root: project_root.to_path_buf(),
            store_id,
            #[cfg(unix)]
            database_identity,
        })
    }

    fn verify_instance(&self) -> Result<(), WritingError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::symlink_metadata(self.root.join(PROJECT_DB_RELATIVE))
                .map_err(|e| WritingError::Unavailable(format!("文稿数据库不可访问：{e}")))?;
            if meta.file_type().is_symlink() || (meta.dev(), meta.ino()) != self.database_identity {
                return Err(WritingError::Unavailable(
                    "文稿数据库已替换，请保留恢复草稿后重新载入项目".into(),
                ));
            }
        }
        let conn = lock(&self.conn)?;
        let current: String = conn
            .query_row("SELECT value FROM meta WHERE key='store_uuid'", [], |row| {
                row.get(0)
            })
            .map_err(sql_error)?;
        if current != self.store_id.as_str() {
            return Err(WritingError::Unavailable(
                "文稿库实例身份已变化，请重新载入项目".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn snapshot_database(&self, destination: &Path) -> Result<(), WritingError> {
        self.verify_instance()?;
        if destination.exists() {
            return Err(WritingError::InvalidPath("备份数据库目标已存在".into()));
        }
        let conn = lock(&self.conn)?;
        conn.execute("VACUUM INTO ?1", params![destination.to_string_lossy()])
            .map_err(sql_error)?;
        std::fs::File::open(destination)
            .map_err(|e| WritingError::Unavailable(e.to_string()))?
            .sync_all()
            .map_err(|e| WritingError::Unavailable(e.to_string()))
    }

    pub(super) fn renew_instance_identity(&self) -> Result<(), WritingError> {
        let conn = lock(&self.conn)?;
        conn.execute(
            "UPDATE meta SET value=?1 WHERE key='store_uuid'",
            params![Uuid::now_v7().to_string()],
        )
        .map_err(sql_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn inject_sql_fault(&self, full: bool) {
        let conn = lock(&self.conn).unwrap();
        if full {
            let pages: i64 = conn
                .query_row("PRAGMA page_count", [], |r| r.get(0))
                .unwrap();
            conn.execute_batch(&format!("PRAGMA max_page_count={pages};"))
                .unwrap();
        } else {
            conn.execute_batch("PRAGMA query_only=ON;").unwrap();
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub fn mirror_notice(&self) -> Result<Option<String>, WritingError> {
        let conn = lock(&self.conn)?;
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outbox WHERE state != 'synced'",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if pending == 0 {
            return Ok(None);
        }
        let error: Option<String> = conn.query_row("SELECT last_error FROM outbox WHERE state != 'synced' AND last_error IS NOT NULL ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).optional().map_err(sql_error)?;
        Ok(Some(
            error.unwrap_or_else(|| format!("{pending} 项文件同步待处理")),
        ))
    }

    pub fn project_revision(&self) -> Result<u64, WritingError> {
        let conn = lock(&self.conn)?;
        let value: i64 = conn
            .query_row("SELECT value FROM project_revision WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(sql_error)?;
        Ok(value.max(0) as u64)
    }

    /// 新建/导入文稿。相对路径已存在时不覆盖,返回已有文稿供调用方引导打开。
    pub fn create(&self, new: NewDocument) -> Result<CreateOutcome, WritingError> {
        self.verify_instance()?;
        let relative = validate_relative_path(&new.relative_path)?;
        if new.title.trim().is_empty() {
            return Err(WritingError::InvalidPath("标题不能为空".into()));
        }
        new.snapshot
            .validate(super::types::MAX_TEXT_BYTES, super::types::MAX_STATE_BYTES)?;
        let mut conn = lock(&self.conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        if let Some(existing) = document_by_path(&tx, &relative)? {
            tx.commit().map_err(sql_error)?;
            return Ok(CreateOutcome::Existing(existing));
        }

        let id = DocumentId(Uuid::now_v7().to_string());
        let ts = now_ms();
        let digest = new.snapshot.digest();
        // 导入:源文件摘要;新建:空串 sentinel 表示“目标必须不存在”。
        let mirror_baseline = new.source_file_digest.clone().unwrap_or_default();
        tx.execute(
            "INSERT INTO documents
                (id,title,relative_path,durable_revision,newline,bom,created_ms,updated_ms)
             VALUES (?1,?2,?3,1,?4,?5,?6,?6)",
            params![
                id.as_str(),
                new.title,
                relative,
                new.newline.as_str(),
                i64::from(new.bom),
                ts
            ],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO document_versions
                (document_id,revision,text,state_json,schema,digest,created_ms)
             VALUES (?1,1,?2,?3,?4,?5,?6)",
            params![
                id.as_str(),
                new.snapshot.text,
                new.snapshot.state_json,
                i64::from(new.snapshot.schema),
                digest,
                ts
            ],
        )
        .map_err(sql_error)?;
        let commit_id = insert_commit(
            &tx,
            id.as_str(),
            "create",
            &format!("create:{}", id.as_str()),
            0,
            1,
            &digest,
            ts,
        )?;
        bump_project_revision(&tx)?;
        insert_outbox(
            &tx,
            id.as_str(),
            1,
            &relative,
            Some(&mirror_baseline),
            &digest,
            ts,
        )?;
        let doc = Document {
            id,
            title: new.title,
            relative_path: relative,
            revision: 1,
            newline: new.newline,
            bom: new.bom,
            created_ms: ts,
            updated_ms: ts,
        };
        let _ = commit_id;
        tx.commit().map_err(sql_error)?;
        Ok(CreateOutcome::Created(doc))
    }

    pub fn load(&self, id: &DocumentId) -> Result<LoadedDocument, WritingError> {
        self.verify_instance()?;
        let conn = lock(&self.conn)?;
        let doc = document_by_id(&conn, id)?
            .ok_or_else(|| WritingError::NotFound(format!("文稿 {}", id.as_str())))?;
        let (text, state_json, schema, stored_digest): (String, String, i64, String) = conn
            .query_row(
                "SELECT text,state_json,schema,digest FROM document_versions
                 WHERE document_id=?1 AND revision=?2",
                params![doc.id.as_str(), doc.revision as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => WritingError::Corrupt(format!(
                    "文稿 {} 的 durable revision {} 没有对应版本",
                    doc.id.as_str(),
                    doc.revision
                )),
                other => sql_error(other),
            })?;
        let snapshot = WritingSnapshot::new(text, state_json, schema.max(0) as u32);
        if snapshot.digest() != stored_digest {
            return Err(WritingError::Corrupt(format!(
                "文稿 {} revision {} 的内容摘要不匹配",
                doc.id.as_str(),
                doc.revision
            )));
        }
        snapshot.validate(super::types::MAX_TEXT_BYTES, super::types::MAX_STATE_BYTES)?;
        Ok(LoadedDocument {
            document: doc,
            snapshot,
        })
    }

    /// 原子保存:版本、Commit、project revision、Outbox 与幂等结果同事务。
    pub fn save(&self, request: SaveRequest) -> Result<SaveReceipt, WritingError> {
        self.verify_instance()?;
        if request.operation_id.trim().is_empty() {
            return Err(WritingError::InvalidPath("operation id 不能为空".into()));
        }
        request
            .snapshot
            .validate(super::types::MAX_TEXT_BYTES, super::types::MAX_STATE_BYTES)?;
        let digest = request.snapshot.digest();
        let fp = fingerprint(&[
            request.document.as_str(),
            &request.expected_revision.to_string(),
            &digest,
            request.expected_file_digest.as_deref().unwrap_or(""),
        ]);
        let mut conn = lock(&self.conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        if let Some((stored_fp, receipt)) = replay(&tx, &request.operation_id)? {
            if stored_fp != fp {
                return Err(WritingError::OperationConflict {
                    operation_id: request.operation_id,
                });
            }
            tx.commit().map_err(sql_error)?;
            return Ok(SaveReceipt {
                replayed: true,
                ..receipt
            });
        }

        let doc = document_by_id(&tx, &request.document)?
            .ok_or_else(|| WritingError::NotFound(format!("文稿 {}", request.document.as_str())))?;
        if doc.revision != request.expected_revision {
            return Err(WritingError::RevisionConflict {
                expected: request.expected_revision,
                actual: doc.revision,
            });
        }
        let revision = doc
            .revision
            .checked_add(1)
            .ok_or_else(|| WritingError::Limit("文稿 revision 已耗尽".into()))?;
        let ts = now_ms();
        tx.execute(
            "INSERT INTO document_versions
                (document_id,revision,text,state_json,schema,digest,created_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                doc.id.as_str(),
                revision as i64,
                request.snapshot.text,
                request.snapshot.state_json,
                i64::from(request.snapshot.schema),
                digest,
                ts
            ],
        )
        .map_err(|error| WritingError::Corrupt(format!("写入版本失败: {error}")))?;
        let changed = tx
            .execute(
                "UPDATE documents SET durable_revision=?1, updated_ms=?2
                 WHERE id=?3 AND durable_revision=?4",
                params![revision as i64, ts, doc.id.as_str(), doc.revision as i64],
            )
            .map_err(sql_error)?;
        if changed != 1 {
            return Err(WritingError::RevisionConflict {
                expected: request.expected_revision,
                actual: doc.revision,
            });
        }
        let commit_id = insert_commit(
            &tx,
            doc.id.as_str(),
            "human",
            &request.operation_id,
            doc.revision,
            revision,
            &digest,
            ts,
        )?;
        bump_project_revision(&tx)?;
        insert_outbox(
            &tx,
            doc.id.as_str(),
            revision,
            &doc.relative_path,
            request.expected_file_digest.as_deref(),
            &digest,
            ts,
        )?;
        tx.execute(
            "INSERT INTO idempotency
                (operation_id,fingerprint,document_id,revision,commit_id,digest,created_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                request.operation_id,
                fp,
                doc.id.as_str(),
                revision as i64,
                commit_id,
                digest,
                ts
            ],
        )
        .map_err(sql_error)?;
        #[cfg(test)]
        crate::crash_at("writing-save-before-commit");
        tx.commit().map_err(sql_error)?;
        #[cfg(test)]
        crate::crash_at("writing-save-after-commit");
        Ok(SaveReceipt {
            document: doc.id,
            revision,
            commit_id,
            digest,
            replayed: false,
        })
    }

    pub fn retry_failed_mirrors(&self) -> Result<(), WritingError> {
        let conn = lock(&self.conn)?;
        conn.execute("UPDATE outbox SET attempts=0 WHERE state='retry'", [])
            .map_err(sql_error)?;
        Ok(())
    }

    pub fn pending_outbox(&self, limit: u32) -> Result<Vec<OutboxEntry>, WritingError> {
        let conn = lock(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id,document_id,revision,relative_path,expected_digest,target_digest,
                        state,attempts,last_error
                 FROM outbox AS candidate WHERE state IN ('pending','retry') AND attempts < 8
                   AND NOT EXISTS (SELECT 1 FROM outbox AS older WHERE older.document_id=candidate.document_id AND (older.state='blocked' OR (older.state='retry' AND older.attempts>=8)))
                 ORDER BY id LIMIT ?1",
            )
            .map_err(sql_error)?;
        let rows = stmt
            .query_map(params![limit.max(1) as i64], |row| {
                Ok(OutboxEntry {
                    id: row.get(0)?,
                    document: DocumentId(row.get(1)?),
                    revision: row.get::<_, i64>(2)?.max(0) as u64,
                    relative_path: row.get(3)?,
                    expected_file_digest: row.get(4)?,
                    target_digest: row.get(5)?,
                    state: row.get(6)?,
                    attempts: row.get::<_, i64>(7)?.max(0) as u32,
                    last_error: row.get(8)?,
                })
            })
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }

    pub fn mark_outbox_synced(&self, id: i64, file_digest: &str) -> Result<(), WritingError> {
        let mut conn = lock(&self.conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        tx.execute(
            "UPDATE outbox SET state='synced', attempts=attempts+1, last_error=NULL, updated_ms=?3
             WHERE id=?1",
            params![id, file_digest, now_ms()],
        )
        .map_err(sql_error)?;
        tx.execute(
            "UPDATE documents SET mirror_revision=(
                SELECT revision FROM outbox WHERE id=?1
             ), mirror_digest=?2
             WHERE id=(SELECT document_id FROM outbox WHERE id=?1)
               AND (SELECT revision FROM outbox WHERE id=?1) >= mirror_revision",
            params![id, file_digest],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)
    }

    /// 最近一次成功镜像的摘要。保存时拿它当基线,当前文件被外部改动时会冲突。
    pub fn mirror_digest(&self, id: &DocumentId) -> Result<Option<String>, WritingError> {
        let conn = lock(&self.conn)?;
        conn.query_row(
            "SELECT mirror_digest FROM documents WHERE id=?1",
            params![id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_error)
        .map(|value| value.flatten())
    }

    pub fn mark_outbox_failed(&self, id: i64, error: &str) -> Result<(), WritingError> {
        let conn = lock(&self.conn)?;
        conn.execute(
            "UPDATE outbox SET state='retry', attempts=attempts+1, last_error=?2, updated_ms=?3
             WHERE id=?1",
            params![id, error, now_ms()],
        )
        .map_err(sql_error)?;
        Ok(())
    }

    pub fn mark_outbox_blocked(&self, id: i64, reason: &str) -> Result<(), WritingError> {
        let conn = lock(&self.conn)?;
        conn.execute(
            "UPDATE outbox SET state='blocked', attempts=attempts+1, last_error=?2, updated_ms=?3
             WHERE id=?1",
            params![id, reason, now_ms()],
        )
        .map_err(sql_error)?;
        Ok(())
    }

    /// 读取指定历史版本(镜像 Outbox 重放用)。
    pub fn snapshot_at(
        &self,
        id: &DocumentId,
        revision: u64,
    ) -> Result<WritingSnapshot, WritingError> {
        let conn = lock(&self.conn)?;
        let (text, state_json, schema, stored_digest): (String, String, i64, String) = conn
            .query_row(
                "SELECT text,state_json,schema,digest FROM document_versions
                 WHERE document_id=?1 AND revision=?2",
                params![id.as_str(), revision as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(sql_error)?
            .ok_or_else(|| {
                WritingError::Corrupt(format!(
                    "文稿 {} 缺少 revision {revision} 的版本",
                    id.as_str()
                ))
            })?;
        let snapshot = WritingSnapshot::new(text, state_json, schema.max(0) as u32);
        if snapshot.digest() != stored_digest {
            return Err(WritingError::Corrupt(format!(
                "文稿 {} revision {revision} 的内容摘要不匹配",
                id.as_str()
            )));
        }
        snapshot.validate(super::types::MAX_TEXT_BYTES, super::types::MAX_STATE_BYTES)?;
        Ok(snapshot)
    }

    /// 处理当前 Outbox:按 revision 顺序写镜像。返回每个任务的结果。
    ///
    /// 外部文件变化标记为 `blocked`(不覆盖);写入失败标记为 `retry`。
    pub fn mirror_pending(
        &self,
        project_root: &Path,
        limit: u32,
    ) -> Result<Vec<(i64, Result<String, String>)>, WritingError> {
        self.verify_instance()?;
        let _worker_lock = dozer_file_io::lock::WriteLock::acquire(
            &self.root.join(".digger/locks"),
            &self.root.join(PROJECT_DB_RELATIVE),
            std::time::Duration::from_secs(5),
        )
        .map_err(|error| WritingError::Unavailable(error.to_string()))?;
        let root = dozer_file_io::ProjectRoot::open(project_root)
            .map_err(|error| WritingError::Unavailable(format!("打开项目根失败: {error}")))?;
        if root.canonical_path()
            != self
                .root
                .canonicalize()
                .map_err(|e| WritingError::Unavailable(e.to_string()))?
        {
            return Err(WritingError::InvalidPath(
                "镜像目标与文稿库项目不一致".into(),
            ));
        }
        let mut results = vec![];
        let mut blocked = std::collections::HashSet::new();
        for entry in self.pending_outbox(limit)? {
            if blocked.contains(entry.document.as_str()) {
                // 旧 revision 的外部冲突/失败未被解决时,不能继续写同文档的后续
                // revision(那会用旧基线覆盖外部内容)。
                continue;
            }
            let doc = self
                .load(&entry.document)
                .map_err(|error| WritingError::Corrupt(error.to_string()))?;
            let snapshot = self.snapshot_at(&entry.document, entry.revision)?;
            let (mirror_revision, mirror_digest): (u64, Option<String>) = {
                let conn = lock(&self.conn)?;
                conn.query_row(
                    "SELECT mirror_revision,mirror_digest FROM documents WHERE id=?1",
                    params![entry.document.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(sql_error)?
            };
            if mirror_revision >= entry.revision {
                self.mark_outbox_synced(entry.id, mirror_digest.as_deref().unwrap_or(""))?;
                continue;
            }
            let expected = mirror_digest
                .as_deref()
                .or(entry.expected_file_digest.as_deref());
            let baseline = match expected {
                Some("") => super::mirror::MirrorBaseline::Absent,
                Some(digest) => super::mirror::MirrorBaseline::Digest(digest.to_owned()),
                None => super::mirror::MirrorBaseline::Any,
            };
            let result = super::mirror::write_mirror(
                &root,
                &entry.relative_path,
                &snapshot.text,
                doc.document.newline,
                doc.document.bom,
                &baseline,
            );
            match result {
                Ok(outcome) => {
                    #[cfg(test)]
                    crate::crash_at("writing-mirror-after-rename");
                    self.mark_outbox_synced(entry.id, &outcome.file_digest)?;
                    results.push((entry.id, Ok(outcome.file_digest)));
                }
                Err(super::mirror::MirrorError::ExternalChange(reason)) => {
                    self.mark_outbox_blocked(entry.id, &reason)?;
                    blocked.insert(entry.document.as_str().to_owned());
                    results.push((entry.id, Err(format!("external_change: {reason}"))));
                }
                Err(error) => {
                    self.mark_outbox_failed(entry.id, &error.to_string())?;
                    blocked.insert(entry.document.as_str().to_owned());
                    results.push((entry.id, Err(error.to_string())));
                }
            }
        }
        Ok(results)
    }

    pub fn review_external(
        &self,
        id: &DocumentId,
    ) -> Result<super::mirror::ExternalReview, WritingError> {
        let loaded = self.load(id)?;
        let candidate = if self
            .current_file_digest(&loaded.document.relative_path)?
            .is_some()
        {
            Some(
                super::read_source(&self.root, &loaded.document.relative_path)
                    .map_err(|e| WritingError::Unavailable(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(super::mirror::ExternalReview {
            document: loaded.document,
            official: loaded.snapshot,
            candidate,
        })
    }

    /// Explicit user confirmation; a changed candidate or durable revision
    /// invalidates it. Never use an unconditioned overwrite for this action.
    pub fn overwrite_reviewed_mirror(
        &self,
        review: &super::mirror::ExternalReview,
    ) -> Result<(), WritingError> {
        let _worker_lock = dozer_file_io::lock::WriteLock::acquire(
            &self.root.join(".digger/locks"),
            &self.root.join(PROJECT_DB_RELATIVE),
            std::time::Duration::from_secs(5),
        )
        .map_err(|e| WritingError::Unavailable(e.to_string()))?;
        let current = self.load(&review.document.id)?;
        if current.document.revision != review.document.revision {
            return Err(WritingError::RevisionConflict {
                expected: review.document.revision,
                actual: current.document.revision,
            });
        }
        let baseline = match &review.candidate {
            Some(source) => super::MirrorBaseline::Digest(source.file_digest.clone()),
            None => super::MirrorBaseline::Absent,
        };
        let root = dozer_file_io::ProjectRoot::open(&self.root)
            .map_err(|e| WritingError::Unavailable(e.to_string()))?;
        // Check even if the replacement happens to equal the latest file: an
        // expired confirmation must remain visibly expired.
        let actual = super::current_file_digest(&root, &current.document.relative_path)
            .map_err(|e| WritingError::Unavailable(e.to_string()))?;
        if actual.as_deref()
            != review
                .candidate
                .as_ref()
                .map(|source| source.file_digest.as_str())
        {
            return Err(WritingError::Unavailable(
                "外部文件已再次变化，请重新预览后确认".into(),
            ));
        }
        let outcome = super::write_mirror(
            &root,
            &current.document.relative_path,
            &current.snapshot.text,
            current.document.newline,
            current.document.bom,
            &baseline,
        )
        .map_err(|e| WritingError::Unavailable(e.to_string()))?;
        let mut conn = lock(&self.conn)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        tx.execute("UPDATE outbox SET state='synced',last_error=NULL,updated_ms=?3 WHERE document_id=?1 AND revision<=?2", params![current.document.id.as_str(), current.document.revision as i64, now_ms()]).map_err(sql_error)?;
        tx.execute("UPDATE documents SET mirror_revision=?2,mirror_digest=?3 WHERE id=?1 AND mirror_revision<=?2", params![current.document.id.as_str(), current.document.revision as i64, outcome.file_digest]).map_err(sql_error)?;
        tx.commit().map_err(sql_error)
    }

    pub fn copy_reviewed_external(
        &self,
        review: &super::mirror::ExternalReview,
    ) -> Result<Document, WritingError> {
        let source = review
            .candidate
            .as_ref()
            .ok_or_else(|| WritingError::NotFound("外部文件已删除，没有可导入内容".into()))?;
        let mut doc = digger_editor::doc::Doc::new();
        doc.load(&source.text);
        let state = doc.export_state();
        let stem = Path::new(&review.document.relative_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("external");
        let path = Path::new(&review.document.relative_path)
            .with_file_name(format!("{stem}.external-{}.md", Uuid::now_v7()));
        let created = self.create(NewDocument {
            title: format!("{}（外部副本）", review.document.title),
            relative_path: path.to_string_lossy().into_owned(),
            snapshot: WritingSnapshot::new(
                state.text.clone(),
                serde_json::to_string(&state)
                    .map_err(|e| WritingError::Unavailable(e.to_string()))?,
                1,
            ),
            newline: source.newline,
            bom: source.bom,
            source_file_digest: None,
        })?;
        Ok(match created {
            CreateOutcome::Created(doc) | CreateOutcome::Existing(doc) => doc,
        })
    }

    /// 当前镜像文件摘要(保存前取基线,外部修改检测用)。
    pub fn current_file_digest(&self, relative_path: &str) -> Result<Option<String>, WritingError> {
        let root = dozer_file_io::ProjectRoot::open(&self.root)
            .map_err(|error| WritingError::Unavailable(format!("打开项目根失败: {error}")))?;
        super::mirror::current_file_digest(&root, relative_path)
            .map_err(|error| WritingError::Unavailable(error.to_string()))
    }

    /// 已提交内容摘要(外部冲突比较用)。
    pub fn current_digest(&self, id: &DocumentId) -> Result<String, WritingError> {
        let conn = lock(&self.conn)?;
        let doc = document_by_id(&conn, id)?
            .ok_or_else(|| WritingError::NotFound(format!("文稿 {}", id.as_str())))?;
        conn.query_row(
            "SELECT digest FROM document_versions WHERE document_id=?1 AND revision=?2",
            params![doc.id.as_str(), doc.revision as i64],
            |row| row.get(0),
        )
        .map_err(sql_error)
    }

    pub fn list(&self) -> Result<Vec<Document>, WritingError> {
        self.verify_instance()?;
        let conn = lock(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id,title,relative_path,durable_revision,newline,bom,created_ms,updated_ms
                 FROM documents ORDER BY created_ms, id",
            )
            .map_err(sql_error)?;
        let rows = stmt.query_map([], document_from_row).map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql_error)
    }
}

fn ensure_schema(conn: &Connection) -> Result<StoreId, WritingError> {
    let has_meta: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |row| row.get::<_, i64>(0).map(|n| n > 0),
        )
        .map_err(sql_error)?;
    if !has_meta {
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if tables != 0 {
            return Err(WritingError::Corrupt(
                "project.db 已有表但没有 Digger writing meta".into(),
            ));
        }
        create_schema(conn)?;
        let id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO meta(key,value) VALUES ('schema_version',?1),('store_uuid',?2)",
            params![SCHEMA_VERSION.to_string(), id],
        )
        .map_err(sql_error)?;
        return Ok(StoreId(id));
    }

    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    let found = version
        .as_deref()
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or_else(|| WritingError::Corrupt("meta.schema_version 缺失或非法".into()))?;
    if found > SCHEMA_VERSION {
        return Err(WritingError::UnsupportedSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    if found < SCHEMA_VERSION {
        return Err(WritingError::Corrupt(format!(
            "文稿库 schema {found} 没有迁移路径"
        )));
    }
    let id: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key='store_uuid'", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(sql_error)?;
    id.map(StoreId)
        .ok_or_else(|| WritingError::Corrupt("meta.store_uuid 缺失".into()))
}

fn create_schema(conn: &Connection) -> Result<(), WritingError> {
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE documents (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            relative_path TEXT NOT NULL UNIQUE,
            durable_revision INTEGER NOT NULL,
            newline TEXT NOT NULL CHECK(newline IN ('lf','crlf')),
            bom INTEGER NOT NULL,
            mirror_revision INTEGER NOT NULL DEFAULT 0,
            mirror_digest TEXT,
            created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL
         );
         CREATE TABLE document_versions (
            document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL,
            text TEXT NOT NULL,
            state_json TEXT NOT NULL,
            schema INTEGER NOT NULL,
            digest TEXT NOT NULL,
            created_ms INTEGER NOT NULL,
            PRIMARY KEY(document_id, revision)
         );
         CREATE TABLE commits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL REFERENCES documents(id),
            actor TEXT NOT NULL,
            operation_id TEXT NOT NULL,
            base_revision INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            digest TEXT NOT NULL,
            created_ms INTEGER NOT NULL,
            UNIQUE(document_id, operation_id)
         );
         CREATE TABLE outbox (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL REFERENCES documents(id),
            revision INTEGER NOT NULL,
            relative_path TEXT NOT NULL,
            expected_digest TEXT,
            target_digest TEXT NOT NULL,
            state TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_ms INTEGER NOT NULL,
            updated_ms INTEGER NOT NULL,
            UNIQUE(document_id, revision)
         );
         CREATE TABLE idempotency (
            operation_id TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            document_id TEXT NOT NULL REFERENCES documents(id),
            revision INTEGER NOT NULL,
            commit_id INTEGER NOT NULL,
            digest TEXT NOT NULL,
            created_ms INTEGER NOT NULL
         );
         CREATE TABLE project_revision (
            id INTEGER PRIMARY KEY CHECK(id=1),
            value INTEGER NOT NULL
         );
         INSERT INTO project_revision(id,value) VALUES(1,0);
         COMMIT;",
    )
    .map_err(sql_error)
}

fn document_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: DocumentId(row.get(0)?),
        title: row.get(1)?,
        relative_path: row.get(2)?,
        revision: row.get::<_, i64>(3)?.max(0) as u64,
        newline: super::types::NewlineStyle::parse(&row.get::<_, String>(4)?)
            .unwrap_or(super::types::NewlineStyle::Lf),
        bom: row.get::<_, i64>(5)? != 0,
        created_ms: row.get(6)?,
        updated_ms: row.get(7)?,
    })
}

fn document_by_id(conn: &Connection, id: &DocumentId) -> Result<Option<Document>, WritingError> {
    conn.query_row(
        "SELECT id,title,relative_path,durable_revision,newline,bom,created_ms,updated_ms
         FROM documents WHERE id=?1",
        params![id.as_str()],
        document_from_row,
    )
    .optional()
    .map_err(sql_error)
}

fn document_by_path(conn: &Connection, path: &str) -> Result<Option<Document>, WritingError> {
    conn.query_row(
        "SELECT id,title,relative_path,durable_revision,newline,bom,created_ms,updated_ms
         FROM documents WHERE relative_path=?1",
        params![path],
        document_from_row,
    )
    .optional()
    .map_err(sql_error)
}

fn insert_commit(
    conn: &Connection,
    document: &str,
    actor: &str,
    operation_id: &str,
    base: u64,
    revision: u64,
    digest: &str,
    ts: i64,
) -> Result<i64, WritingError> {
    conn.execute(
        "INSERT INTO commits
            (document_id,actor,operation_id,base_revision,revision,digest,created_ms)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            document,
            actor,
            operation_id,
            base as i64,
            revision as i64,
            digest,
            ts
        ],
    )
    .map_err(sql_error)?;
    Ok(conn.last_insert_rowid())
}

fn bump_project_revision(conn: &Connection) -> Result<(), WritingError> {
    conn.execute("UPDATE project_revision SET value=value+1 WHERE id=1", [])
        .map_err(sql_error)?;
    Ok(())
}

fn insert_outbox(
    conn: &Connection,
    document: &str,
    revision: u64,
    relative_path: &str,
    expected_digest: Option<&str>,
    target_digest: &str,
    ts: i64,
) -> Result<(), WritingError> {
    conn.execute(
        "INSERT INTO outbox
            (document_id,revision,relative_path,expected_digest,target_digest,state,created_ms,updated_ms)
         VALUES (?1,?2,?3,?4,?5,'pending',?6,?6)",
        params![
            document,
            revision as i64,
            relative_path,
            expected_digest,
            target_digest,
            ts
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn replay(
    conn: &Connection,
    operation_id: &str,
) -> Result<Option<(String, SaveReceipt)>, WritingError> {
    conn.query_row(
        "SELECT fingerprint,document_id,revision,commit_id,digest
         FROM idempotency WHERE operation_id=?1",
        params![operation_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                SaveReceipt {
                    document: DocumentId(row.get(1)?),
                    revision: row.get::<_, i64>(2)?.max(0) as u64,
                    commit_id: row.get(3)?,
                    digest: row.get(4)?,
                    replayed: true,
                },
            ))
        },
    )
    .optional()
    .map_err(sql_error)
}
