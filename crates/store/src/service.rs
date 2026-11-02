//! [`dozer_services::UsageService`] 的账本实现:把 [`UsageLedger`] 里的任务完成记录
//! 映射成面板既有 DTO(`ConversationSummary` + `UsagePayload`)。
//!
//! 与 dozerd 的语义对齐(`dozerd/transcripts/mod.rs:1038`):`since_ts` 是 **Unix 毫秒**,
//! 过滤 `finished_ts_ms >= since_ts`;`cwd` 精确匹配 `project_path` 做 scope 隔离。
//! 读账本失败(损坏/I/O)返回 [`ServiceErrorKind::Unavailable`]——**不是**空结果,避免
//! 把"读不出来"伪装成"确实没用量"(闸门 2 要求:不得恒空假实现)。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dozer_model::{ConversationSummary, UsagePayload};
use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind, UsageRows, UsageService};

use crate::ledger::{TaskUsageRecord, UsageLedger, UsageLedgerError};

/// 以落盘账本为唯一真相的用量服务。
#[derive(Debug)]
pub struct LedgerUsageService {
    ledger: Arc<Mutex<UsageLedger>>,
}

impl LedgerUsageService {
    /// 用已载入的账本构造(账本由组合根从产品状态目录载入,单一实例共享)。
    pub fn new(ledger: Arc<Mutex<UsageLedger>>) -> Self {
        Self { ledger }
    }

    /// 从磁盘载入账本并构造服务。载入失败(损坏/I/O)在此显式返回,不静默当空账本。
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, UsageLedgerError> {
        let ledger = UsageLedger::load(path)?;
        Ok(Self::new(Arc::new(Mutex::new(ledger))))
    }
}

impl UsageService for LedgerUsageService {
    fn usage_summary(
        &self,
        cwd: String,
        since_ts: Option<u64>,
    ) -> BoxFuture<'_, Result<UsageRows, ServiceError>> {
        Box::pin(async move {
            let guard = self.ledger.lock().map_err(|_| {
                ServiceError::new(ServiceErrorKind::Unavailable, "用量账本锁被污染")
            })?;
            let rows = guard
                .query(&cwd, since_ts)
                .into_iter()
                .map(to_row)
                .collect();
            Ok(rows)
        })
    }
}

/// 账本损坏时的占位用量服务(复审 #9):查询恒返回 [`ServiceErrorKind::Unavailable`],
/// **不**退回一次性临时空账本。
///
/// 与"确实没有记录"(`LedgerUsageService` 读空文件 → `Ok(空)`)区分:后者是合法空态,
/// 前者是"读不出来"。这样产品层既不会把损坏伪装成空表,也不会让新记录悄悄写进一个下次
/// 启动就不再引用的临时文件——损坏的原文件保持原样,留待用户/后续修复。
#[derive(Debug)]
pub struct UnavailableUsageService {
    reason: String,
}

impl UnavailableUsageService {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl UsageService for UnavailableUsageService {
    fn usage_summary(
        &self,
        _cwd: String,
        _since_ts: Option<u64>,
    ) -> BoxFuture<'_, Result<UsageRows, ServiceError>> {
        let reason = self.reason.clone();
        Box::pin(async move { Err(ServiceError::new(ServiceErrorKind::Unavailable, reason)) })
    }
}

/// 单条落盘记录 → 面板 DTO。`first_ts`/`last_ts` 取运行起止(ms),`turn_count` 取回合数。
fn to_row(r: &TaskUsageRecord) -> (ConversationSummary, UsagePayload) {
    let summary = ConversationSummary {
        conversation_id: r.run_id.clone(),
        agent: r.agent,
        file_path: r.project_path.clone(),
        title: r.title.clone(),
        first_ts: r.started_ts_ms,
        last_ts: r.finished_ts_ms,
        turn_count: r.turns,
    };
    let payload = UsagePayload {
        turns: r.turns,
        tool_calls: r.tool_calls,
        mutating_tool_calls: r.mutating_tool_calls,
        files_touched: r.files_touched.iter().cloned().collect::<BTreeSet<_>>(),
        tokens_in: r.tokens_in,
        tokens_out: r.tokens_out,
        tokens_cache_read: r.tokens_cache_read,
        tokens_cache_write: r.tokens_cache_write,
    };
    (summary, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::CompletionEvent;
    use dozer_model::AgentKind;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-svc-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("usage_ledger.json")
    }

    fn event(run_id: &str, cwd: &str, finished_ts_ms: u64) -> CompletionEvent {
        CompletionEvent {
            run_id: run_id.into(),
            project_id: 7,
            project_path: cwd.into(),
            agent: AgentKind::Claude,
            title: format!("run {run_id}"),
            started_ts_ms: finished_ts_ms.saturating_sub(1_000),
            finished_ts_ms,
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

    #[tokio::test]
    async fn rows_map_from_the_ledger() {
        let path = temp_path("map");
        let service = LedgerUsageService::load(&path).unwrap();
        {
            let mut ledger = service.ledger.lock().unwrap();
            ledger.record(event("r1", "/p/a", 1_000)).unwrap();
        }
        let rows = service.usage_summary("/p/a".into(), None).await.unwrap();
        assert_eq!(rows.len(), 1);
        let (summary, payload) = &rows[0];
        assert_eq!(summary.conversation_id, "r1");
        assert_eq!(summary.agent, AgentKind::Claude);
        assert_eq!(summary.first_ts, 0);
        assert_eq!(summary.last_ts, 1_000);
        assert_eq!(summary.turn_count, 4);
        assert_eq!(payload.tokens_in, 100);
        assert_eq!(payload.files_touched.len(), 2);
    }

    #[tokio::test]
    async fn a_different_cwd_yields_no_rows() {
        let path = temp_path("isolation");
        let service = LedgerUsageService::load(&path).unwrap();
        {
            let mut ledger = service.ledger.lock().unwrap();
            ledger.record(event("a1", "/p/a", 1_000)).unwrap();
            ledger.record(event("b1", "/p/b", 1_000)).unwrap();
        }
        let a = service.usage_summary("/p/a".into(), None).await.unwrap();
        let b = service.usage_summary("/p/b".into(), None).await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].0.conversation_id, "a1");
        assert_eq!(b[0].0.conversation_id, "b1");
    }

    #[tokio::test]
    async fn since_ts_filters_by_finish_time_in_millis() {
        let path = temp_path("since");
        let service = LedgerUsageService::load(&path).unwrap();
        {
            let mut ledger = service.ledger.lock().unwrap();
            ledger.record(event("r1", "/p/a", 1_000)).unwrap();
            ledger.record(event("r2", "/p/a", 5_000)).unwrap();
        }
        let rows = service
            .usage_summary("/p/a".into(), Some(2_000))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.conversation_id, "r2");
    }

    #[tokio::test]
    async fn a_corrupt_ledger_is_unavailable_not_an_empty_success() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        // 载入阶段就应显式失败,而不是把损坏账本当空账本。
        let err = LedgerUsageService::load(&path).unwrap_err();
        assert!(matches!(err, UsageLedgerError::Corrupt { .. }));
    }

    #[tokio::test]
    async fn an_empty_ledger_returns_an_empty_success() {
        let path = temp_path("empty-ok");
        let service = LedgerUsageService::load(&path).unwrap();
        let rows = service.usage_summary("/p/a".into(), None).await.unwrap();
        assert!(rows.is_empty(), "确实为空是 Ok(空),与读失败区分");
    }

    #[tokio::test]
    async fn unavailable_service_reports_unavailable_not_empty() {
        // 复审 #9:损坏账本要用占位服务返回 Unavailable,而不是伪装成 Ok(空)。
        let service = UnavailableUsageService::new("账本损坏");
        let err = service
            .usage_summary("/p/a".into(), None)
            .await
            .unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::Unavailable);
    }
}

/// SQLite 后台账本的 [`UsageService`] 实现:查询经 worker 线程,不在窗口 / Tokio worker 上长时间执行 SQLite。
#[derive(Clone)]
pub struct UsageDbService {
    handle: crate::usage_worker::UsageHandle,
}

impl UsageDbService {
    pub fn new(handle: crate::usage_worker::UsageHandle) -> Self {
        Self { handle }
    }
}

impl std::fmt::Debug for UsageDbService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageDbService").finish()
    }
}

impl UsageService for UsageDbService {
    fn usage_summary(
        &self,
        cwd: String,
        since_ts: Option<u64>,
    ) -> BoxFuture<'_, Result<UsageRows, ServiceError>> {
        Box::pin(async move {
            let rx = self
                .handle
                .query(cwd, since_ts)
                .map_err(|e| ServiceError::new(ServiceErrorKind::Unavailable, e.to_string()))?;
            let rows = rx
                .await
                .map_err(|_| {
                    ServiceError::new(ServiceErrorKind::Unavailable, "用量查询 worker 已停止")
                })?
                .map_err(|e| ServiceError::new(ServiceErrorKind::Unavailable, e))?;
            Ok(rows.iter().map(to_row).collect())
        })
    }
}
