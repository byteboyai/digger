//! 后台用量账本 worker(A9):单个工作线程拥有同步 SQLite 连接,窗口线程只投递命令。
//!
//! - 有界队列:满了立即返回明确的可重试结果,绝不阻塞窗口或静默丢弃。
//! - 只有事务提交成功后才向调用方报告 `persisted`;失败带回原文,同一完成事件可原样重试。
//! - 关闭门与准入栅栏保证所有已接受命令先处理完,再 checkpoint 并退出。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TrySendError, channel, sync_channel,
};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::ledger::{CompletionEvent, TaskUsageRecord};
use crate::usage_db::{ImportOutcome, UsageDb, UsageDbError};

/// 提交失败:队列满(可重试)或 worker 已关闭。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    QueueFull,
    Closed,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull => write!(f, "用量队列繁忙或已满,请稍后重试"),
            Self::Closed => write!(f, "用量后台服务已关闭"),
        }
    }
}

enum Command {
    Record {
        event: CompletionEvent,
        reply: oneshot::Sender<Result<bool, String>>,
    },
    Query {
        cwd: String,
        since_ts_ms: Option<u64>,
        reply: oneshot::Sender<Result<Vec<TaskUsageRecord>, String>>,
    },
    Import {
        source: PathBuf,
        reply: oneshot::Sender<Result<ImportOutcome, String>>,
    },
}

/// 可克隆的提交句柄(注入产品状态 / UsageService)。
#[derive(Clone)]
pub struct UsageHandle {
    tx: SyncSender<Command>,
    closed: Arc<AtomicBool>,
    /// Held only while checking admission and performing try_send, never during database work.
    admission: Arc<Mutex<()>>,
}

impl UsageHandle {
    fn submit(&self, command: Command) -> Result<(), SubmitError> {
        self.submit_with(command, || {})
    }

    fn submit_with(&self, command: Command, before_send: impl FnOnce()) -> Result<(), SubmitError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(SubmitError::Closed);
        }
        let _admission = match self.admission.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(SubmitError::QueueFull),
            Err(TryLockError::Poisoned(_)) => return Err(SubmitError::Closed),
        };
        if self.closed.load(Ordering::SeqCst) {
            return Err(SubmitError::Closed);
        }
        before_send();
        self.tx.try_send(command).map_err(|e| match e {
            TrySendError::Full(_) => SubmitError::QueueFull,
            TrySendError::Disconnected(_) => SubmitError::Closed,
        })
    }

    /// 非阻塞提交一次完成事件;返回后续结果的接收端。
    pub fn record(
        &self,
        event: CompletionEvent,
    ) -> Result<oneshot::Receiver<Result<bool, String>>, SubmitError> {
        let (reply, rx) = oneshot::channel();
        self.submit(Command::Record { event, reply })?;
        Ok(rx)
    }

    /// 非阻塞提交查询;结果在后台 SQLite 上执行。
    pub fn query(
        &self,
        cwd: impl Into<String>,
        since_ts_ms: Option<u64>,
    ) -> Result<oneshot::Receiver<Result<Vec<TaskUsageRecord>, String>>, SubmitError> {
        let (reply, rx) = oneshot::channel();
        self.submit(Command::Query {
            cwd: cwd.into(),
            since_ts_ms,
            reply,
        })?;
        Ok(rx)
    }

    /// 旧 JSON 导入(启动时一次)。
    pub fn import(
        &self,
        source: impl Into<PathBuf>,
    ) -> Result<oneshot::Receiver<Result<ImportOutcome, String>>, SubmitError> {
        let (reply, rx) = oneshot::channel();
        self.submit(Command::Import {
            source: source.into(),
            reply,
        })?;
        Ok(rx)
    }
}

/// worker 所有者:产品壳持有它,退出时 `shutdown` 排空并等待。
pub struct UsageWorker {
    handle: UsageHandle,
    join: Option<std::thread::JoinHandle<()>>,
    closed: Arc<AtomicBool>,
    done: Receiver<Result<(), String>>,
}

impl UsageWorker {
    pub fn handle(&self) -> UsageHandle {
        self.handle.clone()
    }

    /// 停止接受新提交,排空已接受的命令,checkpoint 并等待线程结束。
    pub fn shutdown(self) -> Result<(), String> {
        self.shutdown_with_deadline(Duration::from_secs(10))
    }

    /// 统一的关闭期限:关闭准入、排空、checkpoint、join 都在同一个 deadline 内。
    /// 超时后不再无条件 `join`(那会把 10 秒变成 20 秒),线程句柄直接丢弃;已确认记录仍由
    /// 事务保证,未确认事件由上层保留重试。
    pub fn shutdown_with_deadline(mut self, deadline: Duration) -> Result<(), String> {
        let start = Instant::now();
        // The worker waits for the admission fence before draining. A producer that read false
        // before this store can still enqueue, but its accepted command cannot be skipped.
        self.closed.store(true, Ordering::SeqCst);
        let remaining = deadline.saturating_sub(start.elapsed());
        let drained = self
            .done
            .recv_timeout(remaining)
            .map_err(|_| "用量后台服务未在限时内排空".to_string())?;
        while self.join.as_ref().is_some_and(|join| !join.is_finished()) {
            if start.elapsed() >= deadline {
                return Err("用量后台服务关闭超时".to_string());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            return Err("用量后台线程异常退出".to_string());
        }
        drained
    }
}

/// 启动后台 worker。打开数据库失败时直接返回错误,不建一个会静默丢写入的空服务。
pub fn spawn(path: impl Into<PathBuf>, capacity: usize) -> Result<UsageWorker, UsageDbError> {
    spawn_with(path.into(), capacity, |_| Ok(()))
}

fn spawn_with(
    path: PathBuf,
    capacity: usize,
    mut before_record: impl FnMut(&CompletionEvent) -> Result<(), String> + Send + 'static,
) -> Result<UsageWorker, UsageDbError> {
    // 先在当前线程验证能打开,失败立即上报;worker 自己再打开一次(Connection 不跨线程共享)。
    UsageDb::open(path.clone())?;
    let (tx, rx) = sync_channel::<Command>(capacity.max(1));
    let closed = Arc::new(AtomicBool::new(false));
    let admission = Arc::new(Mutex::new(()));
    let worker_closed = closed.clone();
    let worker_admission = admission.clone();
    let (finished, done) = channel();
    let join = std::thread::Builder::new()
        .name("digger-usage-db".into())
        .spawn(move || {
            let outcome = (|| -> Result<(), String> {
                let mut db = UsageDb::open(path).map_err(|e| e.to_string())?;
                loop {
                    let cmd = if worker_closed.load(Ordering::SeqCst) {
                        // Synchronize with every producer that passed admission before closing.
                        // Once this fence is crossed, no future submission can succeed.
                        let guard = worker_admission
                            .lock()
                            .map_err(|_| "用量准入锁被污染".to_string())?;
                        drop(guard);
                        match rx.try_recv() {
                            Ok(cmd) => cmd,
                            Err(_) => break,
                        }
                    } else {
                        match rx.recv_timeout(Duration::from_millis(10)) {
                            Ok(cmd) => cmd,
                            Err(RecvTimeoutError::Timeout) => continue,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    };
                    match cmd {
                        Command::Record { event, reply } => {
                            let result = before_record(&event)
                                .and_then(|()| db.record(event).map_err(|e| e.to_string()));
                            let _ = reply.send(result);
                        }
                        Command::Query {
                            cwd,
                            since_ts_ms,
                            reply,
                        } => {
                            let _ =
                                reply.send(db.query(&cwd, since_ts_ms).map_err(|e| e.to_string()));
                        }
                        Command::Import { source, reply } => {
                            let _ = reply
                                .send(db.import_legacy_json(&source).map_err(|e| e.to_string()));
                        }
                    }
                }
                db.checkpoint().map_err(|e| e.to_string())
            })(); // The connection is dropped before signalling completion.
            let _ = finished.send(outcome);
        })
        .map_err(|e| UsageDbError::Io(std::io::Error::other(e)))?;
    Ok(UsageWorker {
        handle: UsageHandle {
            tx,
            closed: closed.clone(),
            admission,
        },
        join: Some(join),
        closed,
        done,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::AgentKind;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "digger-usage-worker-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state/usage.sqlite3")
    }

    fn event(run_id: &str, cwd: &str, finished: u64) -> CompletionEvent {
        CompletionEvent {
            run_id: run_id.into(),
            project_id: 1,
            project_path: cwd.into(),
            agent: AgentKind::Claude,
            title: format!("run {run_id}"),
            started_ts_ms: finished.saturating_sub(10),
            finished_ts_ms: finished,
            turns: 1,
            tool_calls: 2,
            mutating_tool_calls: 1,
            files_touched: vec!["a".into()],
            tokens_in: 3,
            tokens_out: 4,
            tokens_cache_read: 5,
            tokens_cache_write: 6,
        }
    }

    #[test]
    fn writes_are_confirmed_only_after_commit_and_survive_shutdown() {
        let path = temp_path("drain");
        let worker = spawn(&path, 64).unwrap();
        let handle = worker.handle();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        for i in 0..50u64 {
            let rx = handle
                .record(event(&format!("r{i}"), "/p/a", 1_000 + i))
                .unwrap();
            assert!(rt.block_on(rx).unwrap().unwrap(), "each run is new");
        }
        worker.shutdown().unwrap();
        let db = UsageDb::open(&path).unwrap();
        assert_eq!(
            db.len().unwrap(),
            50,
            "shutdown drained every accepted record"
        );
    }

    #[test]
    fn an_admitted_write_finishes_before_shutdown_even_if_enqueued_after_close() {
        let path = temp_path("admission-fence");
        let worker = spawn(&path, 4).unwrap();
        let handle = worker.handle();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(handle.query("/p/a", None).unwrap())
            .unwrap()
            .unwrap();
        let (entered, admitted) = channel();
        let (release, resume) = channel();
        let (reply, result) = oneshot::channel();
        let writer = handle.clone();
        let producer = std::thread::spawn(move || {
            writer.submit_with(
                Command::Record {
                    event: event("held", "/p/a", 100),
                    reply,
                },
                || {
                    entered.send(()).unwrap();
                    resume.recv().unwrap();
                },
            )
        });
        admitted.recv_timeout(Duration::from_secs(2)).unwrap();
        let closing = std::thread::spawn(move || worker.shutdown());
        let started = Instant::now();
        while !handle.closed.load(Ordering::SeqCst) {
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        }
        assert_eq!(
            handle.record(event("late", "/p/a", 101)).unwrap_err(),
            SubmitError::Closed
        );
        assert_eq!(handle.query("/p/a", None).unwrap_err(), SubmitError::Closed);
        assert_eq!(
            handle.import(path.with_extension("json")).unwrap_err(),
            SubmitError::Closed
        );
        release.send(()).unwrap();
        producer.join().unwrap().unwrap();
        assert!(rt.block_on(result).unwrap().unwrap());
        closing.join().unwrap().unwrap();
        let db = UsageDb::open(path).unwrap();
        assert_eq!(db.len().unwrap(), 1);
        assert_eq!(db.query("/p/a", None).unwrap()[0].run_id, "held");
    }

    #[test]
    fn shutdown_has_one_deadline_even_when_the_database_is_locked() {
        let path = temp_path("deadline");
        let worker = spawn(&path, 4).unwrap();
        let handle = worker.handle();
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE;").unwrap();
        // The worker picks this up and blocks on the lock (busy_timeout is seconds long).
        let _rx = handle.record(event("r1", "/p/a", 1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(80));

        let started = std::time::Instant::now();
        let err = worker
            .shutdown_with_deadline(std::time::Duration::from_millis(120))
            .unwrap_err();
        assert!(err.contains("限时"), "{err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "shutdown took {:?}, not one bounded deadline",
            started.elapsed()
        );
        lock.execute_batch("ROLLBACK;").unwrap();
    }

    #[test]
    fn a_closed_worker_rejects_new_submissions_instead_of_dropping_them() {
        let worker = spawn(temp_path("closed"), 4).unwrap();
        let handle = worker.handle();
        worker.shutdown().unwrap();
        assert_eq!(
            handle.record(event("r1", "/p/a", 1)).unwrap_err(),
            SubmitError::Closed
        );
    }

    #[test]
    fn queries_run_on_the_worker_and_report_unavailable_after_close() {
        let path = temp_path("query");
        let worker = spawn(&path, 8).unwrap();
        let handle = worker.handle();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(handle.record(event("r1", "/p/a", 1_000)).unwrap())
            .unwrap()
            .unwrap();
        let rows = rt
            .block_on(handle.query("/p/a", None).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(rows.len(), 1);
        worker.shutdown().unwrap();
        assert_eq!(handle.query("/p/a", None).unwrap_err(), SubmitError::Closed);
    }
    #[test]
    fn a_blocked_writer_fills_the_queue_without_blocking_submit_and_retries_the_same_event() {
        let path = temp_path("slow-full-failed-retry");
        let (entered, waiting) = channel();
        let (release, resume) = channel();
        let mut first = true;
        let worker = spawn_with(path.clone(), 1, move |event| {
            if first {
                first = false;
                entered.send(()).unwrap();
                resume.recv().unwrap();
                return Err("injected disk commit failure".into());
            }
            assert!(!event.run_id.is_empty());
            Ok(())
        })
        .unwrap();
        let handle = worker.handle();
        let mut failed = handle.record(event("retry", "/p/a", 100)).unwrap();
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let queued = handle.record(event("queued", "/p/a", 101)).unwrap();
        // The receiver cannot advance past the barrier. These calls must return, without a
        // timing assertion or another thread releasing the writer.
        assert_eq!(
            handle.record(event("full", "/p/a", 102)).unwrap_err(),
            SubmitError::QueueFull
        );
        assert_eq!(
            handle.query("/p/a", None).unwrap_err(),
            SubmitError::QueueFull
        );
        assert!(matches!(
            failed.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(
            rt.block_on(failed)
                .unwrap()
                .unwrap_err()
                .contains("commit failure")
        );
        rt.block_on(queued).unwrap().unwrap();
        let db = UsageDb::open(&path).unwrap();
        assert_eq!(
            db.query("/p/a", None).unwrap().len(),
            1,
            "failed write is not a committed fact"
        );
        drop(db);
        rt.block_on(handle.record(event("retry", "/p/a", 100)).unwrap())
            .unwrap()
            .unwrap();
        worker.shutdown().unwrap();
        let db = UsageDb::open(path).unwrap();
        assert_eq!(db.len().unwrap(), 2);
        assert_eq!(db.query("/p/a", Some(100)).unwrap()[0].run_id, "retry");
    }
}
