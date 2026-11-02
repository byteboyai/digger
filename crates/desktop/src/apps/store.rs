//! 浏览器数据存储清除的宿主适配(Task 17)。
//!
//! 卸载"连数据一起删"([`UninstallMode::ProgramAndData`])时,应用的 localStorage/IndexedDB 住在
//! WKWebsiteDataStore 里(不在应用 `data/` 目录内),删目录之外还得**清它的数据存储**。清除要求:
//!
//! - 该应用的 webview **先离开池**(销毁 Surface),否则 wry 返回 `DataStoreInUse`;
//! - macOS 14+ 才行([`bytehost_webview::supports_store_removal`]),更老的系统 WKWebView 没有按
//!   identifier 的存储,只能记成失败;
//! - 清除是**异步**的,还可能被占用——所以"刚丢掉句柄就清"通常要隔一会儿重试。
//!
//! 本模块把三件事接起来(每件本身都已经单测):
//! - [`digger_app_host::cleanups::StoreCleanups`] —— **持久台账**(重启后继续清、失败留存可见);
//! - [`bytehost_webview::StoreRemovals`] —— 纯策略的**内存重试队列**;
//! - 组合根里的 wry 调用([`bytehost_desktop::app_webview::remove_app_data_store`])。
//!
//! 与 Dozer 不同:Dozer 的队列只在内存里(它的重启不复活已删数据这一条靠运行时目录语义),Digger
//! 明确要求"退出重启后可恢复",所以这里在内存队列外再加一层落盘台账。
//!
//! **为什么带 `#![allow(dead_code)]`**:一期 Digger 尚无应用视口,`remove_app_data_store` 的真实调用
//! 需要窗口线程 + 事件循环(与 Task 16 Surface 创建同一约束),归入 Task 17 的真人验收项;本模块的
//! 决策逻辑(排队/完成/重试/失败/重启恢复)全部有表格测试,等在窗口线程里驱动。

use std::path::PathBuf;

use bytehost_apps::registry::UninstallMode;
use bytehost_webview::{StoreRemovalOutcome, StoreRemovals, data_store_identifier};

use digger_app_host::cleanups::{CleanupState, StoreCleanups};

/// 浏览器数据存储清除的宿主控制器:持久台账 + 内存重试队列。
pub struct StoreCleanup {
    /// 落盘台账(重启恢复、失败可见的唯一真相)。
    ledger: StoreCleanups,
    /// 纯策略内存队列(负责"隔一会儿再试"的节流与"webview 是否已离池"的判断)。
    queue: StoreRemovals,
}

impl StoreCleanup {
    /// 从产品状态目录下的台账文件载入,**并把上次未完成的清除重新排进内存队列**(重启恢复)。
    pub fn load(path: PathBuf) -> Self {
        let ledger = match StoreCleanups::load(path.clone()) {
            Ok(ledger) => ledger,
            Err(e) => {
                // 读不出来 ≠ 空表:保留原文件,禁止写入覆盖;设置页会显示这条原因。
                StoreCleanups::unavailable(path, e.to_string())
            }
        };
        let mut queue = StoreRemovals::default();
        // 上次仍是 `Pending` 的(被占用待重试、或退出时还没轮到的):重启后继续驱动。
        // `Failed` 的不重新排队——它已经是终态,留在台账里给设置页展示。
        for id in ledger.pending() {
            queue.request(&id);
        }
        Self { ledger, queue }
    }

    /// 记账文件路径(供组合根构造;测试可断言落在产品状态目录)。
    pub fn ledger_path(&self) -> &std::path::Path {
        self.ledger.path()
    }

    /// 请求清除一个应用的数据存储(卸载"连数据一起删"成功后调用)。
    ///
    /// **调用方必须先把该应用的 Surface 销毁**(webview 离池),否则只会得到 `DataStoreInUse` 再重试。
    /// 把它排进持久台账与内存队列:落盘失败不影响内存记账(下次仍会尝试),但会让"重启恢复"这一保证
    /// 打折扣——调用方据此可提示,这里不 panic。
    pub fn request(&mut self, app_id: &str) -> Result<(), String> {
        let result = if matches!(self.ledger.state(app_id), Some(CleanupState::Failed { .. })) {
            self.ledger.retry(app_id)
        } else {
            self.ledger.request(app_id).map(|_| ())
        };
        // 落盘失败仍会尝试清除,但调用方必须把"重启后不保证继续"告诉用户,并允许同一意图重试。
        self.queue.request(app_id);
        result.map_err(|e| e.to_string())
    }

    /// 一个结果回来:更新持久台账与内存队列。
    ///
    /// - `Done` → 台账移除 + 队列完成;
    /// - `InUse` → 队列记为待重试(隔 [`STORE_RETRY_DELAY`](bytehost_webview) 再试),台账保持 `Pending`;
    /// - `Unsupported`/`Failed` → 记成终态失败(不再自动重试),台账保留供设置页展示。
    pub fn settle(
        &mut self,
        app_id: &str,
        outcome: StoreRemovalOutcome,
        now: std::time::Instant,
    ) -> Result<(), String> {
        let result = match outcome {
            StoreRemovalOutcome::Done => {
                let r = self.ledger.finish(app_id);
                self.queue.finish(app_id);
                r
            }
            StoreRemovalOutcome::InUse => {
                // 队列自己按退避排下一拍;若已超次数上限,`retry` 返回 false → 记成终态失败。
                if !self.queue.retry(app_id, now) {
                    self.ledger.fail(app_id, "数据存储仍被占用,已停止自动重试")
                } else {
                    Ok(())
                }
            }
            StoreRemovalOutcome::Unsupported => {
                let r = self
                    .ledger
                    .fail(app_id, "系统版本过低(macOS 14 以下),无法清除浏览器数据");
                self.queue.finish(app_id);
                r
            }
            StoreRemovalOutcome::Failed(reason) => {
                let r = self.ledger.fail(app_id, reason);
                self.queue.finish(app_id);
                r
            }
        };
        result.map_err(|e| e.to_string())
    }

    /// 取出**现在就该清**的应用及其存储标识(webview 必须已离池)。
    ///
    /// `webview_in_pool` 由组合根按"该应用是否还有 Surface"回答;只有离池的才会被取出。
    /// 返回 `Vec<(app_id, [u8;16])>`:存储标识由 [`data_store_identifier`] 确定性派生(与创建时一致)。
    #[allow(dead_code)]
    pub fn take_ready(
        &mut self,
        now: std::time::Instant,
        webview_in_pool: impl Fn(&str) -> bool,
    ) -> Vec<(String, [u8; 16])> {
        self.queue
            .take_ready(now, webview_in_pool)
            .into_iter()
            .map(|(id, _)| {
                let store = data_store_identifier(&id);
                (id, store)
            })
            .collect()
    }

    /// 下一次该醒来重试的时刻(没有待重试则 `None`);组合根据此挂一个延后任务。
    #[allow(dead_code)]
    pub fn next_wake(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        self.queue.next_wake(now)
    }

    /// 是否仍欠某应用的清除(任何状态)。
    #[allow(dead_code)]
    pub fn is_pending(&self, app_id: &str) -> bool {
        self.ledger.is_pending(app_id)
    }

    /// 当前状态(`None` = 不欠清除)。
    #[allow(dead_code)]
    pub fn state(&self, app_id: &str) -> Option<&CleanupState> {
        self.ledger.state(app_id)
    }

    /// 终态失败的应用及原因(设置页展示:重装后可能仍看到旧数据)。
    #[allow(dead_code)]
    pub fn failures(&self) -> Vec<(&str, &str)> {
        self.ledger.failed()
    }

    /// 仍在自动重试/待驱动的应用数(诊断/测试)。
    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.ledger.pending().len()
    }

    /// 台账读不出来的原因(`None` = 正常)。设置页展示,并禁用覆盖写入。
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.ledger.unavailable_reason()
    }

    /// 上次写入失败后仍有内存意图未落盘。
    pub fn dirty(&self) -> bool {
        self.ledger.dirty()
    }

    /// 最近一次持久化失败原文。
    pub fn last_error(&self) -> Option<&str> {
        self.ledger.last_error()
    }
}

/// 卸载模式是否要求清除浏览器数据存储。
pub fn purges_browser_data(mode: UninstallMode) -> bool {
    matches!(mode, UninstallMode::ProgramAndData)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn temp_ledger(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "digger-store-cleanup-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("store_cleanups.json")
    }

    #[test]
    fn only_program_and_data_purges_the_store() {
        assert!(purges_browser_data(UninstallMode::ProgramAndData));
        assert!(!purges_browser_data(UninstallMode::Program));
    }

    #[test]
    fn a_requested_purge_is_only_ready_after_the_webview_leaves_the_pool() {
        let mut c = StoreCleanup::load(temp_ledger("in-pool"));
        c.request("notes").unwrap();
        let now = Instant::now();
        // 仍在池里 → 不取。
        assert!(c.take_ready(now, |_| true).is_empty());
        // 离池 → 取出,带确定性存储标识。
        let ready = c.take_ready(now, |_| false);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, "notes");
        assert_eq!(ready[0].1, data_store_identifier("notes"));
    }

    #[test]
    fn a_done_result_clears_the_ledger() {
        let mut c = StoreCleanup::load(temp_ledger("done"));
        c.request("notes").unwrap();
        c.settle("notes", StoreRemovalOutcome::Done, Instant::now())
            .unwrap();
        assert!(!c.is_pending("notes"));
        assert!(c.failures().is_empty());
    }

    #[test]
    fn an_in_use_result_keeps_it_pending_for_retry() {
        let mut c = StoreCleanup::load(temp_ledger("inuse"));
        c.request("notes").unwrap();
        c.settle("notes", StoreRemovalOutcome::InUse, Instant::now())
            .unwrap();
        assert!(c.is_pending("notes"), "被占用仍待重试");
        assert_eq!(c.pending_count(), 1);
    }

    #[test]
    fn repeated_in_use_gives_up_into_a_visible_failure() {
        let mut c = StoreCleanup::load(temp_ledger("giveup"));
        c.request("notes").unwrap();
        let mut now = Instant::now();
        // 一直"被占用":队列按退避最多重试若干次,超上限后记终态失败、不再自动重试。
        for _ in 0..20 {
            c.settle("notes", StoreRemovalOutcome::InUse, now).unwrap();
            now += Duration::from_secs(5);
        }
        assert_eq!(c.pending_count(), 0, "不再自动重试");
        assert!(c.is_pending("notes"), "失败仍留在台账");
        assert_eq!(c.failures().len(), 1);
    }

    #[test]
    fn unsupported_and_failed_are_terminal_but_visible() {
        let mut c = StoreCleanup::load(temp_ledger("terminal"));
        c.request("a").unwrap();
        c.settle("a", StoreRemovalOutcome::Unsupported, Instant::now())
            .unwrap();
        assert!(
            c.state("a")
                .is_some_and(|s| matches!(s, CleanupState::Failed { .. }))
        );
        assert_eq!(c.pending_count(), 0);

        c.request("b").unwrap();
        c.settle(
            "b",
            StoreRemovalOutcome::Failed("boom".into()),
            Instant::now(),
        )
        .unwrap();
        assert!(c.failures().iter().any(|(id, _)| *id == "b"));
    }

    #[test]
    fn a_pending_purge_survives_a_restart_and_is_redriven() {
        let path = temp_ledger("restart");
        let mut c = StoreCleanup::load(path.clone());
        c.request("notes").unwrap();
        c.settle("notes", StoreRemovalOutcome::InUse, Instant::now())
            .unwrap();

        // 模拟重启:重新载入,待办仍在,并已重新排进内存队列,可被取出去清。
        let mut reloaded = StoreCleanup::load(path);
        assert!(reloaded.is_pending("notes"));
        let ready = reloaded.take_ready(Instant::now(), |_| false);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, "notes");
    }

    #[test]
    fn a_failed_purge_is_not_redriven_on_restart() {
        let path = temp_ledger("no-redrive");
        let mut c = StoreCleanup::load(path.clone());
        c.request("notes").unwrap();
        c.settle(
            "notes",
            StoreRemovalOutcome::Failed("boom".into()),
            Instant::now(),
        )
        .unwrap();

        let mut reloaded = StoreCleanup::load(path);
        assert!(reloaded.is_pending("notes"), "失败仍可见");
        assert!(
            reloaded.take_ready(Instant::now(), |_| false).is_empty(),
            "终态失败不重新驱动"
        );
    }

    #[test]
    fn manual_retry_is_persisted_across_restart() {
        let path = temp_ledger("manual-retry");
        let mut c = StoreCleanup::load(path.clone());
        c.request("notes").unwrap();
        c.settle(
            "notes",
            StoreRemovalOutcome::Failed("busy".into()),
            Instant::now(),
        )
        .unwrap();
        c.request("notes").unwrap();
        let mut reloaded = StoreCleanup::load(path);
        assert_eq!(reloaded.take_ready(Instant::now(), |_| false).len(), 1);
    }

    #[test]
    fn a_corrupt_ledger_is_visible_and_read_only() {
        let path = temp_ledger("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{ not json";
        std::fs::write(&path, original).unwrap();
        let mut c = StoreCleanup::load(path.clone());
        assert!(c.unavailable_reason().is_some());
        assert!(
            c.request("notes").is_err(),
            "must not overwrite the unreadable ledger"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!c.is_pending("notes"), "no in-memory success is claimed");
    }

    #[test]
    fn requesting_a_purge_twice_is_idempotent() {
        let mut c = StoreCleanup::load(temp_ledger("idem"));
        c.request("notes").unwrap();
        c.request("notes").unwrap();
        assert_eq!(c.take_ready(Instant::now(), |_| false).len(), 1);
    }
}
