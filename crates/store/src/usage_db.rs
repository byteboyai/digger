//! SQLite 用量账本(A8):独立 `state/usage.sqlite3`,以 `run_id` 为主键的事务 upsert 与索引查询。
//!
//! 与旧 JSON 账本的关系:
//! - 旧 `config/usage_ledger.json` 只是**导入源**;导入的事务提交完成标记后才归档,不双写。
//! - 完成标记与记录在同一个事务;进程在提交后中止,重启只补归档,不重复计数。
//! - 相同 `run_id` 相同内容可复用;不同内容明确冲突,不覆盖。
//!
//! 完整 DTO 保存在版本化 payload 里,索引列与 payload 在同一事务更新。SQLite 只有有符号 64 位整数,
//! 所有 `u64` 在写入前显式校验,绝不 `as i64` 静默截断。

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::ledger::{CompletionEvent, OnDisk, TaskUsageRecord};

const SCHEMA_VERSION: i64 = 1;
const PAYLOAD_VERSION: i64 = 1;
const META_IMPORT_PREFIX: &str = "legacy_json_imported:";

/// SQLite 用量账本的错误。读不出来 / 冲突都不能伪装成空账本。
#[derive(Debug)]
pub enum UsageDbError {
    Io(io::Error),
    Sql(rusqlite::Error),
    /// payload 或旧 JSON 解析不了。
    Corrupt(String),
    /// 同一 run_id 已有不同内容。
    Conflict(String),
    /// 超出 SQLite 有符号 64 位范围。
    OutOfRange(&'static str),
}

impl std::fmt::Display for UsageDbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "用量库 I/O 失败: {e}"),
            Self::Sql(e) => write!(f, "用量库 SQL 失败: {e}"),
            Self::Corrupt(m) => write!(f, "用量库数据损坏: {m}"),
            Self::Conflict(m) => write!(f, "用量库冲突: {m}"),
            Self::OutOfRange(field) => write!(f, "用量库范围越界: {field} 超出 SQLite INTEGER"),
        }
    }
}

impl std::error::Error for UsageDbError {}

impl From<io::Error> for UsageDbError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<rusqlite::Error> for UsageDbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

/// 旧 JSON 导入结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// 旧文件不存在。
    NoLegacy,
    /// 此前已导入完成(标记在);没有重复写入。
    AlreadyImported,
    /// 本次导入并提交完成。
    Imported { runs: usize },
    /// 记录已提交,但旧文件归档失败(下次启动只补归档)。
    ArchivePending { runs: usize, error: String },
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, UsageDbError> {
    i64::try_from(value).map_err(|_| UsageDbError::OutOfRange(field))
}

/// 独立 SQLite 用量库。同步 API 只允许由后台 worker 线程持有(A9)。
pub struct UsageDb {
    path: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for UsageDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageDb").field("path", &self.path).finish()
    }
}

impl UsageDb {
    /// 打开 / 创建用量库并配置 WAL、有限 busy timeout、synchronous=FULL。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, UsageDbError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS usage_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS usage_runs (
                run_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                project_path TEXT NOT NULL,
                agent TEXT NOT NULL,
                title TEXT NOT NULL,
                started_ts_ms INTEGER NOT NULL,
                finished_ts_ms INTEGER NOT NULL,
                turns INTEGER NOT NULL,
                tool_calls INTEGER NOT NULL,
                mutating_tool_calls INTEGER NOT NULL,
                files_touched TEXT NOT NULL,
                tokens_in INTEGER NOT NULL,
                tokens_out INTEGER NOT NULL,
                tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL,
                payload_version INTEGER NOT NULL,
                payload TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_usage_runs_project
                ON usage_runs(project_path, finished_ts_ms, run_id);
             INSERT OR IGNORE INTO usage_meta(key, value) VALUES ('schema_version', '{SCHEMA_VERSION}');"
        ))?;
        Ok(Self { path, conn })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 记录一次完成事件。返回是否新增;同一 run_id 覆盖为新的累计值。
    pub fn record(&mut self, event: CompletionEvent) -> Result<bool, UsageDbError> {
        let record: TaskUsageRecord = event.into();
        let payload = serde_json::to_string(&record)
            .map_err(|e| UsageDbError::Corrupt(format!("序列化用量记录: {e}")))?;
        let added = self
            .conn
            .query_row(
                "SELECT 1 FROM usage_runs WHERE run_id = ?1",
                [&record.run_id],
                |_| Ok(()),
            )
            .optional()?
            .is_none();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO usage_runs (
                run_id, project_id, project_path, agent, title, started_ts_ms, finished_ts_ms,
                turns, tool_calls, mutating_tool_calls, files_touched,
                tokens_in, tokens_out, tokens_cache_read, tokens_cache_write,
                payload_version, payload
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(run_id) DO UPDATE SET
                project_id=excluded.project_id, project_path=excluded.project_path,
                agent=excluded.agent, title=excluded.title,
                started_ts_ms=excluded.started_ts_ms, finished_ts_ms=excluded.finished_ts_ms,
                turns=excluded.turns, tool_calls=excluded.tool_calls,
                mutating_tool_calls=excluded.mutating_tool_calls,
                files_touched=excluded.files_touched,
                tokens_in=excluded.tokens_in, tokens_out=excluded.tokens_out,
                tokens_cache_read=excluded.tokens_cache_read,
                tokens_cache_write=excluded.tokens_cache_write,
                payload_version=excluded.payload_version, payload=excluded.payload",
            params![
                record.run_id,
                record.project_id,
                record.project_path,
                serde_json::to_string(&record.agent)
                    .map_err(|e| UsageDbError::Corrupt(format!("序列化 agent: {e}")))?,
                record.title,
                to_i64(record.started_ts_ms, "started_ts_ms")?,
                to_i64(record.finished_ts_ms, "finished_ts_ms")?,
                i64::from(record.turns),
                i64::from(record.tool_calls),
                i64::from(record.mutating_tool_calls),
                serde_json::to_string(&record.files_touched)
                    .map_err(|e| UsageDbError::Corrupt(format!("序列化 files_touched: {e}")))?,
                to_i64(record.tokens_in, "tokens_in")?,
                to_i64(record.tokens_out, "tokens_out")?,
                to_i64(record.tokens_cache_read, "tokens_cache_read")?,
                to_i64(record.tokens_cache_write, "tokens_cache_write")?,
                PAYLOAD_VERSION,
                payload,
            ],
        )?;
        tx.commit()?;
        Ok(added)
    }

    /// 按项目路径精确过滤 + `finished_ts_ms >= since`(毫秒),按结束时刻升序。
    pub fn query(
        &self,
        cwd: &str,
        since_ts_ms: Option<u64>,
    ) -> Result<Vec<TaskUsageRecord>, UsageDbError> {
        let since = match since_ts_ms {
            Some(t) => to_i64(t, "since_ts_ms")?,
            None => i64::MIN,
        };
        let mut stmt = self.conn.prepare(
            "SELECT payload FROM usage_runs
             WHERE project_path = ?1 AND finished_ts_ms >= ?2
             ORDER BY finished_ts_ms ASC, run_id ASC",
        )?;
        let rows = stmt.query_map(params![cwd, since], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row?;
            let record: TaskUsageRecord = serde_json::from_str(&payload)
                .map_err(|e| UsageDbError::Corrupt(format!("解析用量 payload: {e}")))?;
            out.push(record);
        }
        Ok(out)
    }

    pub fn len(&self) -> Result<usize, UsageDbError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM usage_runs", [], |r| r.get(0))?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    pub fn is_empty(&self) -> Result<bool, UsageDbError> {
        Ok(self.len()? == 0)
    }

    /// 当前 schema 版本。
    pub fn schema_version(&self) -> Result<i64, UsageDbError> {
        let v: String = self.conn.query_row(
            "SELECT value FROM usage_meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )?;
        v.parse()
            .map_err(|_| UsageDbError::Corrupt(format!("schema_version={v}")))
    }

    /// 显式 checkpoint(worker 关闭前调用)。
    pub fn checkpoint(&self) -> Result<(), UsageDbError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// 旧 JSON 导入。解析、写入与完成标记在同一事务;提交后才归档来源。
    pub fn import_legacy_json(&mut self, source: &Path) -> Result<ImportOutcome, UsageDbError> {
        let identity = source_identity(source);
        if !source.exists() {
            return Ok(if self.import_marker(&identity)? {
                ImportOutcome::AlreadyImported
            } else {
                ImportOutcome::NoLegacy
            });
        }
        let bytes = std::fs::read(source)?;
        let disk: OnDisk = serde_json::from_slice(&bytes)
            .map_err(|e| UsageDbError::Corrupt(format!("旧 JSON 账本解析失败: {e}")))?;

        if self.import_marker(&identity)? {
            let runs = disk.runs.len();
            return self.finish_import(source, runs, true);
        }

        let tx = self.conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT value FROM usage_meta WHERE key = ?1",
                [format!("{META_IMPORT_PREFIX}{identity}")],
                |r| r.get(0),
            )
            .optional()?;
        if existing.is_some() {
            tx.commit()?;
            return self.finish_import(source, disk.runs.len(), true);
        }
        let mut imported = 0usize;
        for (run_id, record) in &disk.runs {
            if &record.run_id != run_id {
                return Err(UsageDbError::Corrupt(format!(
                    "旧账本键 {run_id} 与记录内的 run_id {} 不一致",
                    record.run_id
                )));
            }
            let payload = serde_json::to_string(record)
                .map_err(|e| UsageDbError::Corrupt(format!("序列化旧记录: {e}")))?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT payload FROM usage_runs WHERE run_id = ?1",
                    [run_id],
                    |r| r.get(0),
                )
                .optional()?;
            match old {
                Some(old) if old == payload => continue, // 同 ID 同内容:复用
                Some(_) => {
                    return Err(UsageDbError::Conflict(format!(
                        "run_id {run_id} 在目标库已有不同内容;未覆盖来源"
                    )));
                }
                None => {
                    insert_record(&tx, record, &payload)?;
                    imported += 1;
                }
            }
        }
        tx.execute(
            "INSERT INTO usage_meta(key, value) VALUES (?1, ?2)",
            params![
                format!("{META_IMPORT_PREFIX}{identity}"),
                now_ms().to_string()
            ],
        )?;
        tx.commit()?;
        let runs = imported.max(disk.runs.len());
        self.finish_import(source, runs, false)
    }

    fn import_marker(&self, identity: &str) -> Result<bool, UsageDbError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM usage_meta WHERE key = ?1",
                [format!("{META_IMPORT_PREFIX}{identity}")],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    fn finish_import(
        &self,
        source: &Path,
        runs: usize,
        already: bool,
    ) -> Result<ImportOutcome, UsageDbError> {
        match archive_source(source) {
            Ok(()) if already => Ok(ImportOutcome::AlreadyImported),
            Ok(()) => Ok(ImportOutcome::Imported { runs }),
            Err(error) => Ok(ImportOutcome::ArchivePending { runs, error }),
        }
    }

    /// 停写后把完整新库导出为旧 JSON 格式(回退工具);原子写,不修改 SQLite。
    pub fn export_legacy_json(&self, target: &Path) -> Result<(), UsageDbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT run_id, payload FROM usage_runs ORDER BY run_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut runs: BTreeMap<String, TaskUsageRecord> = BTreeMap::new();
        for row in rows {
            let (run_id, payload) = row?;
            let record: TaskUsageRecord = serde_json::from_str(&payload)
                .map_err(|e| UsageDbError::Corrupt(format!("导出时解析 payload: {e}")))?;
            runs.insert(run_id, record);
        }
        write_json_atomic(target, &OnDisk { runs })
    }
}

fn insert_record(
    tx: &rusqlite::Transaction<'_>,
    record: &TaskUsageRecord,
    payload: &str,
) -> Result<(), UsageDbError> {
    tx.execute(
        "INSERT INTO usage_runs (
            run_id, project_id, project_path, agent, title, started_ts_ms, finished_ts_ms,
            turns, tool_calls, mutating_tool_calls, files_touched,
            tokens_in, tokens_out, tokens_cache_read, tokens_cache_write,
            payload_version, payload
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            record.run_id,
            record.project_id,
            record.project_path,
            serde_json::to_string(&record.agent)
                .map_err(|e| UsageDbError::Corrupt(format!("序列化 agent: {e}")))?,
            record.title,
            to_i64(record.started_ts_ms, "started_ts_ms")?,
            to_i64(record.finished_ts_ms, "finished_ts_ms")?,
            i64::from(record.turns),
            i64::from(record.tool_calls),
            i64::from(record.mutating_tool_calls),
            serde_json::to_string(&record.files_touched)
                .map_err(|e| UsageDbError::Corrupt(format!("序列化 files_touched: {e}")))?,
            to_i64(record.tokens_in, "tokens_in")?,
            to_i64(record.tokens_out, "tokens_out")?,
            to_i64(record.tokens_cache_read, "tokens_cache_read")?,
            to_i64(record.tokens_cache_write, "tokens_cache_write")?,
            PAYLOAD_VERSION,
            payload,
        ],
    )?;
    Ok(())
}

fn source_identity(source: &Path) -> String {
    let parent = source.parent().filter(|p| !p.as_os_str().is_empty());
    let resolved = parent
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| parent.map(Path::to_path_buf).unwrap_or_default());
    match source.file_name() {
        Some(name) => resolved.join(name),
        None => source.to_path_buf(),
    }
    .to_string_lossy()
    .into_owned()
}

fn archive_source(source: &Path) -> Result<(), String> {
    let mut dest = source.as_os_str().to_owned();
    dest.push(".migrated");
    let dest = PathBuf::from(dest);
    if dest.exists() {
        return Err(format!("归档目标已存在,未覆盖: {}", dest.display()));
    }
    std::fs::rename(source, &dest).map_err(|e| format!("归档旧 JSON: {e}"))
}

fn write_json_atomic(target: &Path, disk: &OnDisk) -> Result<(), UsageDbError> {
    let bytes = serde_json::to_vec_pretty(disk)
        .map_err(|e| UsageDbError::Corrupt(format!("序列化导出: {e}")))?;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let unique = format!(
        ".{}.{}.tmp",
        target
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "usage.json".into()),
        std::process::id()
    );
    let tmp = parent.join(unique);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::AgentKind;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-db-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state/usage.sqlite3")
    }

    fn event(run_id: &str, cwd: &str, finished: u64) -> CompletionEvent {
        CompletionEvent {
            run_id: run_id.into(),
            project_id: 7,
            project_path: cwd.into(),
            agent: AgentKind::Claude,
            title: format!("run {run_id}"),
            started_ts_ms: finished.saturating_sub(1_000),
            finished_ts_ms: finished,
            turns: 4,
            tool_calls: 6,
            mutating_tool_calls: 3,
            files_touched: vec!["a.rs".into(), "b.rs".into()],
            tokens_in: 100,
            tokens_out: 50,
            tokens_cache_read: 10,
            tokens_cache_write: 5,
        }
    }

    #[test]
    fn records_round_trip_with_all_fields_and_survive_reopening() {
        let path = temp_path("roundtrip");
        {
            let mut db = UsageDb::open(&path).unwrap();
            assert!(db.record(event("r1", "/p/a", 1_000)).unwrap());
        }
        let db = UsageDb::open(&path).unwrap();
        let rows = db.query("/p/a", None).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.run_id, "r1");
        assert_eq!(r.tokens_in, 100);
        assert_eq!(
            r.files_touched,
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
        assert_eq!(r.agent, AgentKind::Claude);
    }

    #[test]
    fn query_filters_by_project_and_finish_time_and_uses_the_index() {
        let path = temp_path("query");
        let mut db = UsageDb::open(&path).unwrap();
        db.record(event("a1", "/p/a", 1_000)).unwrap();
        db.record(event("a2", "/p/a", 5_000)).unwrap();
        db.record(event("b1", "/p/b", 5_000)).unwrap();
        assert_eq!(db.query("/p/a", None).unwrap().len(), 2);
        let rows = db.query("/p/a", Some(2_000)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, "a2");
        let plan: String = db
            .conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT payload FROM usage_runs WHERE project_path='/p/a' AND finished_ts_ms>=0 ORDER BY finished_ts_ms, run_id",
                [],
                |r| r.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("idx_usage_runs_project"),
            "query plan must use the index: {plan}"
        );
    }

    #[test]
    fn the_same_run_id_with_new_cumulative_values_overwrites() {
        let path = temp_path("upsert");
        let mut db = UsageDb::open(&path).unwrap();
        assert!(db.record(event("r1", "/p/a", 1_000)).unwrap());
        let mut again = event("r1", "/p/a", 1_000);
        again.tokens_in = 999;
        assert!(!db.record(again).unwrap(), "upsert does not add a row");
        assert_eq!(db.len().unwrap(), 1);
        assert_eq!(db.query("/p/a", None).unwrap()[0].tokens_in, 999);
    }

    #[test]
    fn out_of_range_u64_is_rejected_not_truncated() {
        let path = temp_path("range");
        let mut db = UsageDb::open(&path).unwrap();
        let mut e = event("r1", "/p/a", 1_000);
        e.tokens_in = u64::MAX;
        assert!(matches!(
            db.record(e),
            Err(UsageDbError::OutOfRange("tokens_in"))
        ));
        assert_eq!(db.len().unwrap(), 0);
    }

    fn write_legacy(path: &Path, runs: &[(&str, u64)]) {
        let mut map = BTreeMap::new();
        for (id, finished) in runs {
            let r: TaskUsageRecord = event(id, "/p/a", *finished).into();
            map.insert((*id).to_string(), r);
        }
        write_json_atomic(path, &OnDisk { runs: map }).unwrap();
    }

    #[test]
    fn legacy_json_imports_once_and_archives_after_commit() {
        let dir = std::env::temp_dir().join(format!("digger-usage-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("usage_ledger.json");
        write_legacy(&source, &[("r1", 1_000), ("r2", 2_000)]);
        let path = dir.join("state/usage.sqlite3");
        let mut db = UsageDb::open(&path).unwrap();

        assert_eq!(
            db.import_legacy_json(&source).unwrap(),
            ImportOutcome::Imported { runs: 2 }
        );
        assert!(!source.exists(), "source archived after the commit");
        assert!(dir.join("usage_ledger.json.migrated").exists());
        assert_eq!(
            db.import_legacy_json(&source).unwrap(),
            ImportOutcome::AlreadyImported
        );
        assert_eq!(db.len().unwrap(), 2, "re-import does not double count");
    }

    #[test]
    fn a_conflicting_legacy_record_is_not_overwritten() {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-conflict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("usage_ledger.json");
        write_legacy(&source, &[("r1", 1_000)]);
        let path = dir.join("state/usage.sqlite3");
        let mut db = UsageDb::open(&path).unwrap();
        let mut existing = event("r1", "/p/a", 1_000);
        existing.tokens_in = 42;
        db.record(existing).unwrap();
        let original = std::fs::read(&source).unwrap();

        let err = db.import_legacy_json(&source).unwrap_err();
        assert!(matches!(err, UsageDbError::Conflict(_)));
        assert!(source.exists(), "a conflict keeps the source untouched");
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(db.query("/p/a", None).unwrap()[0].tokens_in, 42);
    }

    #[test]
    fn a_corrupt_legacy_json_is_an_error_and_is_left_in_place() {
        let dir = std::env::temp_dir().join(format!("digger-usage-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("usage_ledger.json");
        std::fs::write(&source, b"{ nope").unwrap();
        let mut db = UsageDb::open(dir.join("state/usage.sqlite3")).unwrap();
        assert!(matches!(
            db.import_legacy_json(&source),
            Err(UsageDbError::Corrupt(_))
        ));
        assert!(source.exists());
        assert_eq!(db.len().unwrap(), 0);
    }

    #[test]
    fn export_writes_a_verified_legacy_snapshot() {
        let path = temp_path("export");
        let mut db = UsageDb::open(&path).unwrap();
        db.record(event("r1", "/p/a", 1_000)).unwrap();
        let out = path.with_file_name("export.json");
        db.export_legacy_json(&out).unwrap();
        let disk: OnDisk = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(disk.runs.len(), 1);
        assert_eq!(disk.runs["r1"].tokens_in, 100);
    }
}
