//! 任务完成用量的落盘账本:幂等记录 + 区间查询的存储层。
//!
//! 键是**任务运行 id**(`run_id`,产品内稳定且唯一);值是这一次运行结算出的用量事实。
//! 落盘为一个小 JSON(`{ "runs": { "<run_id>": { ...TaskUsageRecord } } }`),原子写
//! (临时文件 + rename,见 [`UsageLedger::persist`])。载入时文件不存在视为空账本,
//! **损坏则报错**(读不出 ≠ 没有记录);写调用返回 [`UsageLedgerError`] 供上层提示。

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use dozer_model::AgentKind;
use serde::{Deserialize, Serialize};

/// 一次任务运行的**完成事件**:生产者(业务运行时)在任务结束结算时投递。
///
/// `run_id` 是幂等键:同一个完成事件重复投递(重试/重放/崩溃后重放)不会产生第二条记录。
/// 所有 token 字段是这一次运行的累计值(不是增量)——重复事件带同样的累计值,upsert 即幂等。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionEvent {
    /// 任务运行 id(幂等键,产品内稳定唯一)。
    pub run_id: String,
    /// 该运行所属项目 id(面板按项目隔离/关联用)。
    pub project_id: i64,
    /// 该运行所属项目的工作目录(绝对路径;**区间查询按它做 scope 隔离**)。
    pub project_path: String,
    /// 产出该用量的 agent。
    pub agent: AgentKind,
    /// 展示标题(会话面板副行同款)。
    pub title: String,
    /// 运行开始时刻(Unix 毫秒, UTC)。
    pub started_ts_ms: u64,
    /// 运行结束时刻(Unix 毫秒, UTC)。
    pub finished_ts_ms: u64,
    /// 回合数。
    pub turns: u32,
    /// 工具调用次数。
    pub tool_calls: u32,
    /// 会改动文件的工具调用次数。
    pub mutating_tool_calls: u32,
    /// 本次运行触及的文件(仓库相对路径或绝对路径,由生产者决定口径)。
    pub files_touched: Vec<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tokens_cache_read: u64,
    pub tokens_cache_write: u64,
}

/// 账本里存的一次运行用量事实(由 [`CompletionEvent`] 结算而来,是落盘形状)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUsageRecord {
    pub run_id: String,
    pub project_id: i64,
    pub project_path: String,
    pub agent: AgentKind,
    pub title: String,
    pub started_ts_ms: u64,
    pub finished_ts_ms: u64,
    pub turns: u32,
    pub tool_calls: u32,
    pub mutating_tool_calls: u32,
    pub files_touched: Vec<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tokens_cache_read: u64,
    pub tokens_cache_write: u64,
}

impl From<CompletionEvent> for TaskUsageRecord {
    fn from(e: CompletionEvent) -> Self {
        Self {
            run_id: e.run_id,
            project_id: e.project_id,
            project_path: e.project_path,
            agent: e.agent,
            title: e.title,
            started_ts_ms: e.started_ts_ms,
            finished_ts_ms: e.finished_ts_ms,
            turns: e.turns,
            tool_calls: e.tool_calls,
            mutating_tool_calls: e.mutating_tool_calls,
            files_touched: e.files_touched,
            tokens_in: e.tokens_in,
            tokens_out: e.tokens_out,
            tokens_cache_read: e.tokens_cache_read,
            tokens_cache_write: e.tokens_cache_write,
        }
    }
}

/// 账本读写错误。故意细分"损坏"(读)与"Io"(读写):
///
/// - `Corrupt`:账本文件在但解析不了——**不能**当作空账本静默吞掉,
///   否则会假装"没有用量记录"(与"确实为空"混淆,违反闸门 2 的"非恒空假实现");
/// - `Io`:文件读写失败(权限/磁盘)。
#[derive(Debug)]
pub enum UsageLedgerError {
    Corrupt { path: PathBuf, message: String },
    Io(io::Error),
}

impl std::fmt::Display for UsageLedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageLedgerError::Corrupt { path, message } => {
                write!(f, "用量账本损坏({}): {message}", path.display())
            }
            UsageLedgerError::Io(e) => write!(f, "用量账本 I/O 失败: {e}"),
        }
    }
}

impl std::error::Error for UsageLedgerError {}

impl From<io::Error> for UsageLedgerError {
    fn from(e: io::Error) -> Self {
        UsageLedgerError::Io(e)
    }
}

/// 任务完成用量的落盘账本。键是 `run_id`。
#[derive(Debug)]
pub struct UsageLedger {
    path: PathBuf,
    runs: BTreeMap<String, TaskUsageRecord>,
}

/// 磁盘形状:`{ "runs": { "<run_id>": { ... } } }`。
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    runs: BTreeMap<String, TaskUsageRecord>,
}

impl UsageLedger {
    /// 从磁盘载入账本。文件不存在 → 空账本(首次运行);文件在但解析不了 → [`UsageLedgerError::Corrupt`]
    /// (区分"没有记录"与"读不出来");I/O 失败 → [`UsageLedgerError::Io`]。
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, UsageLedgerError> {
        let path = path.into();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let disk: OnDisk =
                    serde_json::from_slice(&bytes).map_err(|e| UsageLedgerError::Corrupt {
                        path: path.clone(),
                        message: e.to_string(),
                    })?;
                Ok(Self {
                    path,
                    runs: disk.runs,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self {
                path,
                runs: BTreeMap::new(),
            }),
            Err(e) => Err(UsageLedgerError::Io(e)),
        }
    }

    /// 记一次任务完成事件(幂等 upsert:同 `run_id` 覆盖为最新累计值,不新增第二条)。
    ///
    /// 返回是否**新增**(`true`)还是覆盖已有(`false`)。落盘失败返回 [`UsageLedgerError::Io`],
    /// 但内存已更新——调用方据此提示"记录可能没落盘,重启后不保证"。
    pub fn record(&mut self, event: CompletionEvent) -> Result<bool, UsageLedgerError> {
        let run_id = event.run_id.clone();
        let record: TaskUsageRecord = event.into();
        let added = self.runs.insert(run_id, record).is_none();
        self.persist()?;
        Ok(added)
    }

    /// 按项目路径 + 起始时刻(含)过滤记录,按结束时刻升序返回。
    ///
    /// `cwd` 精确匹配 `project_path`(scope 隔离:两个项目的记录互不相串);`since_ts_ms`
    /// 为 `None` 时不过滤时间,为 `Some(t)` 时只保留 `finished_ts_ms >= t` 的记录。
    pub fn query(&self, cwd: &str, since_ts_ms: Option<u64>) -> Vec<&TaskUsageRecord> {
        let mut hits: Vec<&TaskUsageRecord> = self
            .runs
            .values()
            .filter(|r| r.project_path == cwd)
            .filter(|r| since_ts_ms.is_none_or(|t| r.finished_ts_ms >= t))
            .collect();
        hits.sort_by_key(|r| r.finished_ts_ms);
        hits
    }

    /// 全部记录数(诊断/测试)。
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// 账本文件路径(供启动时构造;测试可断言落在产品状态目录)。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 原子落盘(先写临时文件再 rename),避免半截 JSON 让下次启动误判。
    fn persist(&self) -> Result<(), UsageLedgerError> {
        let disk = OnDisk {
            runs: self.runs.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&disk)
            .map_err(|e| UsageLedgerError::Io(io::Error::other(e)))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("usage_ledger.json")
    }

    fn event(run_id: &str, cwd: &str, finished_ts_ms: u64) -> CompletionEvent {
        CompletionEvent {
            run_id: run_id.into(),
            project_id: 1,
            project_path: cwd.into(),
            agent: AgentKind::Claude,
            title: format!("run {run_id}"),
            started_ts_ms: finished_ts_ms.saturating_sub(1_000),
            finished_ts_ms,
            turns: 3,
            tool_calls: 5,
            mutating_tool_calls: 2,
            files_touched: vec!["src/main.rs".into()],
            tokens_in: 100,
            tokens_out: 50,
            tokens_cache_read: 10,
            tokens_cache_write: 5,
        }
    }

    #[test]
    fn a_completion_survives_a_restart() {
        let path = temp_path("restart");
        let mut ledger = UsageLedger::load(&path).unwrap();
        assert!(ledger.record(event("r1", "/p/a", 1_000)).unwrap());

        // 模拟退出重启:重新从磁盘载入,记录仍在且可查。
        let reloaded = UsageLedger::load(&path).unwrap();
        let hits = reloaded.query("/p/a", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].run_id, "r1");
        assert_eq!(hits[0].tokens_in, 100);
    }

    #[test]
    fn a_duplicate_event_does_not_double() {
        let path = temp_path("dedup");
        let mut ledger = UsageLedger::load(&path).unwrap();
        assert!(
            ledger.record(event("r1", "/p/a", 1_000)).unwrap(),
            "首次新增"
        );
        assert!(
            !ledger.record(event("r1", "/p/a", 1_000)).unwrap(),
            "重复事件覆盖,不新增"
        );
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.query("/p/a", None).len(), 1, "查询也只有一条");
    }

    #[test]
    fn two_scopes_do_not_cross() {
        let path = temp_path("scopes");
        let mut ledger = UsageLedger::load(&path).unwrap();
        ledger.record(event("a1", "/p/a", 1_000)).unwrap();
        ledger.record(event("b1", "/p/b", 2_000)).unwrap();
        assert_eq!(ledger.query("/p/a", None).len(), 1);
        assert_eq!(ledger.query("/p/b", None).len(), 1);
        assert_eq!(ledger.query("/p/a", None)[0].run_id, "a1");
        assert_eq!(ledger.query("/p/b", None)[0].run_id, "b1");
    }

    #[test]
    fn since_filters_by_finish_time() {
        let path = temp_path("since");
        let mut ledger = UsageLedger::load(&path).unwrap();
        ledger.record(event("r1", "/p/a", 1_000)).unwrap();
        ledger.record(event("r2", "/p/a", 5_000)).unwrap();
        let hits = ledger.query("/p/a", Some(2_000));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].run_id, "r2");
    }

    #[test]
    fn a_corrupt_ledger_is_an_error_not_an_empty_result() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        let err = UsageLedger::load(&path).unwrap_err();
        assert!(matches!(err, UsageLedgerError::Corrupt { .. }));
    }

    #[test]
    fn a_missing_ledger_is_an_empty_ledger_not_an_error() {
        let path = temp_path("missing");
        let ledger = UsageLedger::load(&path).unwrap();
        assert!(ledger.is_empty());
    }
}
