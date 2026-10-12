//! 最小真实任务运行的**生产者**:把一个真实的长操作(如应用安装/启动/停止)
//! 包成一次"任务运行",在结束时结算成 [`CompletionEvent`] 记入账本。
//!
//! 这是任务完成事件的**唯一来源**:调用方(组合根)在真实操作开始时 [`TaskRunner::begin`],
//! 在操作真正结束时 [`TaskRunner::finish`]。起止时刻由 [`Clock`] 提供(生产用系统时钟;
//! 测试可注入确定性时钟),因此记录的是**真实测量**的时长,不是占位数字。
//!
//! **诚实原则(闸门 2)**:Digger 当前尚无 agent 运行时,真实操作不产生 token。本模块
//! 如实把 token 计为 0(**不是伪造的数字**),`turns`/`tool_calls` 同理;`files_touched`
//! 由调用方按操作实际触及的文件填。将来接入 agent 运行时后,由它提供真实 token 计数,
//! 本模块与账本形状不变。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use dozer_model::AgentKind;

use crate::ledger::{CompletionEvent, UsageLedger, UsageLedgerError};

/// 时间源:生产用系统时钟;测试注入确定性时钟。
pub trait Clock: Send + Sync {
    /// 当前时刻(Unix 毫秒, UTC)。
    fn now_ms(&self) -> u64;
}

/// 系统时钟(真实生产时间源)。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 一次**正在进行**的任务运行:由 [`TaskRunner::begin`] 开启,携带运行身份与开始时刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTask {
    run_id: String,
    project_id: i64,
    project_path: String,
    agent: AgentKind,
    title: String,
    started_ts_ms: u64,
}

impl RunningTask {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn started_ts_ms(&self) -> u64 {
        self.started_ts_ms
    }
}

/// 结束一次运行时提供的用量事实。token 由真实生产者提供(当前恒 0,见模块文档);
/// `files_touched` 是这次运行**实际触及**的文件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskOutcome {
    pub turns: u32,
    pub tool_calls: u32,
    pub mutating_tool_calls: u32,
    pub files_touched: Vec<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tokens_cache_read: u64,
    pub tokens_cache_write: u64,
}

/// 任务运行的开关:开启运行、结束运行并记入账本。持有时间源与账本(单一实例共享)。
pub struct TaskRunner {
    clock: Box<dyn Clock>,
    ledger: Arc<Mutex<UsageLedger>>,
}

impl TaskRunner {
    /// 用系统时钟与已载入的账本构造(组合根从产品状态目录载入账本)。
    pub fn new(ledger: Arc<Mutex<UsageLedger>>) -> Self {
        Self {
            clock: Box::new(SystemClock),
            ledger,
        }
    }

    /// 注入时间源(测试用确定性时钟)。
    pub fn with_clock(ledger: Arc<Mutex<UsageLedger>>, clock: impl Clock + 'static) -> Self {
        Self {
            clock: Box::new(clock),
            ledger,
        }
    }

    /// 开始一次任务运行:记下开始时刻与运行身份(`run_id` 是幂等键,由调用方提供且产品内唯一)。
    pub fn begin(
        &self,
        run_id: impl Into<String>,
        project_id: i64,
        project_path: impl Into<String>,
        agent: AgentKind,
        title: impl Into<String>,
    ) -> RunningTask {
        RunningTask {
            run_id: run_id.into(),
            project_id,
            project_path: project_path.into(),
            agent,
            title: title.into(),
            started_ts_ms: self.clock.now_ms(),
        }
    }

    /// 结束一次运行:以**当前时刻**为结束时刻,结算成 [`CompletionEvent`] 并记入账本。
    ///
    /// 幂等:重复结束同一个 `run_id` 只是覆盖为最新累计值(账本以 `run_id` 为键 upsert),
    /// 不产生第二条记录。返回 [`UsageLedger::record`] 的结果(是否新增)。
    pub fn finish(
        &mut self,
        run: RunningTask,
        outcome: TaskOutcome,
    ) -> Result<bool, UsageLedgerError> {
        let event = CompletionEvent {
            run_id: run.run_id,
            project_id: run.project_id,
            project_path: run.project_path,
            agent: run.agent,
            title: run.title,
            started_ts_ms: run.started_ts_ms,
            finished_ts_ms: self.clock.now_ms(),
            turns: outcome.turns,
            tool_calls: outcome.tool_calls,
            mutating_tool_calls: outcome.mutating_tool_calls,
            files_touched: outcome.files_touched,
            tokens_in: outcome.tokens_in,
            tokens_out: outcome.tokens_out,
            tokens_cache_read: outcome.tokens_cache_read,
            tokens_cache_write: outcome.tokens_cache_write,
        };
        let mut ledger = self.ledger.lock().map_err(|_| UsageLedgerError::Corrupt {
            path: std::path::PathBuf::new(),
            message: "用量账本锁被污染".to_string(),
        })?;
        ledger.record(event)
    }

    /// 账本句柄(诊断/测试):与 [`crate::service::LedgerUsageService`] 共享同一实例。
    pub fn ledger(&self) -> Arc<Mutex<UsageLedger>> {
        Arc::clone(&self.ledger)
    }
}

/// 在途运行的登记簿:把"派发出去的真实操作"映射到它开启的 [`RunningTask`],以便操作
/// 真正结束时能结算。键是操作自己的身份(如 `RequestKey::Action(k)` 的动作键)。
///
/// 与账本分离:登记簿是进程内瞬时态(崩溃即丢,未结束的运行不算完成),账本才是持久事实。
#[derive(Debug, Default)]
pub struct RunningTasks {
    by_key: BTreeMap<u64, RunningTask>,
}

impl RunningTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一次已开启的运行(同一动作键重复登记会覆盖——最新一次为准)。
    pub fn insert(&mut self, key: u64, run: RunningTask) {
        self.by_key.insert(key, run);
    }

    /// 取走某动作键对应的运行(操作结束时调用;取走后不再在待结算集合里)。
    pub fn take(&mut self, key: u64) -> Option<RunningTask> {
        self.by_key.remove(&key)
    }

    /// 是否仍欠某动作键一次结算。
    pub fn contains(&self, key: u64) -> bool {
        self.by_key.contains_key(&key)
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::UsageLedger;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-run-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("usage_ledger.json")
    }

    fn ledger_at(path: &std::path::Path) -> Arc<Mutex<UsageLedger>> {
        Arc::new(Mutex::new(UsageLedger::load(path).unwrap()))
    }

    /// 确定性时钟:每次 `now_ms` 前进固定步长,便于断言真实测量时长。
    struct StepClock(AtomicU64);
    impl Clock for StepClock {
        fn now_ms(&self) -> u64 {
            self.0.fetch_add(100, Ordering::SeqCst)
        }
    }

    #[test]
    fn finish_records_measured_start_and_finish() {
        let mut runner = TaskRunner::with_clock(
            ledger_at(&temp_path("measure")),
            StepClock(AtomicU64::new(1_000)),
        );
        let run = runner.begin("r1", 1, "/p/a", AgentKind::Claude, "install notes");
        assert_eq!(run.started_ts_ms(), 1_000, "开始时刻来自时钟");
        let added = runner
            .finish(
                run,
                TaskOutcome {
                    files_touched: vec!["app/manifest.toml".into()],
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(added);
        let ledger = runner.ledger();
        let ledger = ledger.lock().unwrap();
        let hits = ledger.query("/p/a", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].started_ts_ms, 1_000);
        assert_eq!(
            hits[0].finished_ts_ms, 1_100,
            "结束时刻是新的一拍(真实测量)"
        );
        assert_eq!(hits[0].tokens_in, 0, "无 agent 运行时,如实为 0");
        assert_eq!(hits[0].files_touched, vec!["app/manifest.toml".to_string()]);
    }

    #[test]
    fn finishing_the_same_run_twice_does_not_double() {
        let mut runner = TaskRunner::with_clock(
            ledger_at(&temp_path("dedup")),
            StepClock(AtomicU64::new(5_000)),
        );
        let run = runner.begin("r1", 1, "/p/a", AgentKind::Claude, "op");
        assert!(runner.finish(run.clone(), TaskOutcome::default()).unwrap());
        assert!(
            !runner.finish(run, TaskOutcome::default()).unwrap(),
            "重复结束覆盖,不新增"
        );
        assert_eq!(runner.ledger().lock().unwrap().len(), 1);
    }

    #[test]
    fn a_run_survives_a_restart() {
        let path = temp_path("restart");
        {
            let mut runner =
                TaskRunner::with_clock(ledger_at(&path), StepClock(AtomicU64::new(1_000)));
            let run = runner.begin("r1", 1, "/p/a", AgentKind::Codex, "op");
            runner.finish(run, TaskOutcome::default()).unwrap();
        }
        // 重新载入账本(模拟重启)。
        let reloaded = UsageLedger::load(&path).unwrap();
        assert_eq!(reloaded.query("/p/a", None).len(), 1);
    }

    #[test]
    fn the_registry_tracks_in_flight_runs() {
        let runner =
            TaskRunner::with_clock(ledger_at(&temp_path("reg")), StepClock(AtomicU64::new(0)));
        let run = runner.begin("r1", 1, "/p/a", AgentKind::Claude, "op");
        let mut reg = RunningTasks::new();
        reg.insert(42, run);
        assert!(reg.contains(42));
        assert_eq!(reg.len(), 1);
        assert!(reg.take(42).is_some());
        assert!(!reg.contains(42));
        assert!(reg.take(42).is_none());
    }
}
