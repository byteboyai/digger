//! 待清除的浏览器数据存储队列(卸载"连数据一起删"时用)。
//!
//! 应用的 localStorage/IndexedDB 住在 WKWebsiteDataStore 里(不在应用的 `data/` 目录里),所以
//! "删数据卸载"除了删目录,还得**在宿主层**清掉它的数据存储。清除要求先让使用该存储的 webview 离开
//! 池,且系统版本 < macOS 14 上根本做不了——因此这是一件**可能失败、可能延后**的事,必须:
//!
//! - **在退出重启后仍可恢复**:失败或未完成的清除记在磁盘上,下次启动重新驱动,防止重新安装复活
//!   用户明确要求删除的数据;
//! - **失败可见**:最终清不掉(不支持的系统版本 / 反复被占用 / 其它错误)记成 `Failed`,供设置页展示,
//!   不静默丢弃。
//!
//! 读失败与"确实没有待办"必须分开:损坏 / I/O 失败保留原文件,禁止后续写入覆盖不可读台账;
//! 写失败保留内存里的 dirty 状态与 `last_error`,同一意图可以重试并真正再次落盘。
//!
//! 本模块**不碰** wry / 系统版本号:真正的 `remove_data_store` 调用在宿主的窗口层(desktop)完成。
//! 这一层只负责"哪些应用还欠一次清除"的持久事实与状态机,可脱离窗口单测。

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 一次待清除的浏览器数据存储的状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CleanupState {
    /// 清零待办:还会被驱动去清除(刚排队,或上次被占用待重试)。
    Pending,
    /// 清除失败且不再自动重试(`reason` 给用户看)。
    Failed { reason: String },
}

/// 台账读入失败。与 [`StoreCleanups::load`] 的 `Ok(空)` 区分:后者是"确实没有待办"。
#[derive(Debug)]
pub enum CleanupLoadError {
    /// 文件存在但解析不了。
    Corrupt { path: PathBuf, message: String },
    /// 文件读不了(权限 / I/O)。`NotFound` 不是错误。
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for CleanupLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt { path, message } => {
                write!(f, "清理台账损坏({}): {message}", path.display())
            }
            Self::Io { path, source } => {
                write!(f, "清理台账读取失败({}): {source}", path.display())
            }
        }
    }
}

impl std::error::Error for CleanupLoadError {}

/// 持久化的"待清除数据存储"台账。键是应用 id,值是当前状态。
///
/// 落盘为一个小 JSON:进程任何时刻崩溃/被杀,`Pending` 项下次启动继续清,`Failed` 项留在设置页可见。
#[derive(Debug)]
pub struct StoreCleanups {
    path: PathBuf,
    entries: BTreeMap<String, CleanupState>,
    /// `Some(reason)` = 台账读不出来:保留原文件,禁止任何会覆盖它的写入。
    unavailable: Option<String>,
    /// 上次写入失败后内存与磁盘不一致,下一次同一意图必须真正 persist。
    dirty: bool,
    last_error: Option<String>,
    #[cfg(test)]
    fail_persist: std::cell::Cell<Option<PersistStage>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistStage {
    Write,
    Sync,
    Rename,
}

/// 磁盘上的形状:`{ "apps": { "<id>": {"state": "pending"} | {"state":"failed","reason":"…"} } }`。
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    apps: BTreeMap<String, CleanupState>,
}

impl StoreCleanups {
    /// 从磁盘读入。文件不存在 → 空台账;损坏 / I/O 失败 → [`CleanupLoadError`](不是空台账)。
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, CleanupLoadError> {
        let path = path.into();
        match std::fs::read(&path) {
            Ok(bytes) => {
                let disk: OnDisk =
                    serde_json::from_slice(&bytes).map_err(|e| CleanupLoadError::Corrupt {
                        path: path.clone(),
                        message: e.to_string(),
                    })?;
                Ok(Self::with_entries(path, disk.apps))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Ok(Self::with_entries(path, BTreeMap::new()))
            }
            Err(e) => Err(CleanupLoadError::Io { path, source: e }),
        }
    }

    /// 读不出来时的只读占位:保留原文件,查询为空,但所有写入都返回错误。
    pub fn unavailable(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        let mut s = Self::with_entries(path.into(), BTreeMap::new());
        s.unavailable = Some(reason.into());
        s
    }

    fn with_entries(path: PathBuf, entries: BTreeMap<String, CleanupState>) -> Self {
        Self {
            path,
            entries,
            unavailable: None,
            dirty: false,
            last_error: None,
            #[cfg(test)]
            fail_persist: std::cell::Cell::new(None),
        }
    }

    /// 台账是否不可读(此时所有写入都会被拒绝,原文件保持不变)。
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    /// 内存状态是否尚未成功落盘。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 最近一次持久化失败的原文(供设置页展示 / 重试提示)。
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn writable(&self) -> io::Result<()> {
        match &self.unavailable {
            Some(reason) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("清理台账不可读,拒绝覆盖原文件: {reason}"),
            )),
            None => Ok(()),
        }
    }

    fn commit(&mut self, result: io::Result<()>) -> io::Result<()> {
        match result {
            Ok(()) => {
                self.dirty = false;
                self.last_error = None;
                Ok(())
            }
            Err(e) => {
                self.dirty = true;
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// 请求清除某应用的存储(幂等:已存在则不重复排队,也不改已有状态)。返回是否**新增**。
    ///
    /// 即使条目已存在也真正 persist 一次:上一次写入失败后,同一意图的重试必须能补上落盘。
    /// 落盘失败返回错误,但内存状态仍已更新(标记为 dirty),调用方据此提示并可重试。
    pub fn request(&mut self, app_id: &str) -> io::Result<bool> {
        self.writable()?;
        let added = if let std::collections::btree_map::Entry::Vacant(entry) =
            self.entries.entry(app_id.to_owned())
        {
            entry.insert(CleanupState::Pending);
            true
        } else {
            false
        };
        self.commit(self.persist())?;
        Ok(added)
    }

    /// 用户重试终态失败时重新持久化 Pending,重启后仍会驱动。
    pub fn retry(&mut self, app_id: &str) -> io::Result<()> {
        self.writable()?;
        self.entries
            .insert(app_id.to_owned(), CleanupState::Pending);
        self.commit(self.persist())
    }

    /// 清除成功:从台账移除并落盘。落盘失败时恢复内存条目,便于同一意图重试。
    pub fn finish(&mut self, app_id: &str) -> io::Result<()> {
        self.writable()?;
        let Some(previous) = self.entries.remove(app_id) else {
            return Ok(());
        };
        if let Err(e) = self.commit(self.persist()) {
            self.entries.insert(app_id.to_owned(), previous);
            return Err(e);
        }
        Ok(())
    }

    /// 记录最终失败(不再自动重试),保留给设置页展示;成功状态不覆盖失败。
    pub fn fail(&mut self, app_id: &str, reason: impl Into<String>) -> io::Result<()> {
        self.writable()?;
        let next = CleanupState::Failed {
            reason: reason.into(),
        };
        let previous = self.entries.insert(app_id.to_owned(), next.clone());
        if previous.as_ref() == Some(&next) && !self.dirty {
            return Ok(());
        }
        if let Err(e) = self.commit(self.persist()) {
            match previous {
                Some(old) => {
                    self.entries.insert(app_id.to_owned(), old);
                }
                None => {
                    self.entries.remove(app_id);
                }
            }
            return Err(e);
        }
        Ok(())
    }

    /// 该应用是否仍欠一次清除(任何状态,`Pending` 或 `Failed`)。
    pub fn is_pending(&self, app_id: &str) -> bool {
        self.entries.contains_key(app_id)
    }

    /// 该应用当前状态(`None` = 不欠清除)。
    pub fn state(&self, app_id: &str) -> Option<&CleanupState> {
        self.entries.get(app_id)
    }

    /// 所有仍是 `Pending` 的应用 id(重启后要重新驱动清除的集合)。
    pub fn pending(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, s)| matches!(s, CleanupState::Pending))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 所有 `Failed` 的应用及其原因(设置页展示)。
    pub fn failed(&self) -> Vec<(&str, &str)> {
        self.entries
            .iter()
            .filter_map(|(id, s)| match s {
                CleanupState::Failed { reason } => Some((id.as_str(), reason.as_str())),
                CleanupState::Pending => None,
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 台账文件路径(供启动时构造;测试可断言落在产品状态目录)。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 原子落盘:同目录唯一临时文件 + 文件同步 + rename。失败保留旧版本并清理本次临时文件。
    fn persist(&self) -> io::Result<()> {
        let disk = OnDisk {
            apps: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&disk).map_err(io::Error::other)?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "store_cleanups.json".to_string());
        let unique = format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let tmp = parent.join(unique);
        let result = (|| -> io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            #[cfg(test)]
            self.inject_failure(PersistStage::Write)?;
            file.write_all(&bytes)?;
            #[cfg(test)]
            self.inject_failure(PersistStage::Sync)?;
            file.sync_all()?;
            drop(file);
            #[cfg(test)]
            self.inject_failure(PersistStage::Rename)?;
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// Retry only persistence; retain every pending/failed intent exactly as it is.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writable()?;
        self.commit(self.persist())
    }

    #[cfg(test)]
    fn inject_failure(&self, stage: PersistStage) -> io::Result<()> {
        if self.fail_persist.get() == Some(stage) {
            self.fail_persist.set(None);
            return Err(io::Error::other(format!("injected {stage:?} failure")));
        }
        Ok(())
    }

    /// 测试故障注入:让下一次 persist 失败(不触碰磁盘)。
    #[cfg(test)]
    pub(crate) fn fail_next_persist(&self) {
        self.fail_persist.set(Some(PersistStage::Write));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-cleanups-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("store_cleanups.json")
    }

    #[test]
    fn a_request_is_persisted_and_survives_a_restart() {
        let path = temp_path("restart");
        let mut s = StoreCleanups::load(&path).unwrap();
        assert!(s.request("notes").unwrap());
        assert_eq!(s.pending(), vec!["notes".to_string()]);

        let reloaded = StoreCleanups::load(&path).unwrap();
        assert!(reloaded.is_pending("notes"));
        assert_eq!(reloaded.pending(), vec!["notes".to_string()]);
    }

    #[test]
    fn requesting_twice_is_idempotent() {
        let path = temp_path("idem");
        let mut s = StoreCleanups::load(&path).unwrap();
        assert!(s.request("notes").unwrap(), "首次新增");
        assert!(!s.request("notes").unwrap(), "重复请求不新增");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn finishing_removes_it_from_disk() {
        let path = temp_path("finish");
        let mut s = StoreCleanups::load(&path).unwrap();
        s.request("notes").unwrap();
        s.finish("notes").unwrap();
        assert!(s.is_empty());
        assert!(!StoreCleanups::load(&path).unwrap().is_pending("notes"));
    }

    #[test]
    fn a_failure_is_recorded_and_not_auto_retried_but_still_visible() {
        let path = temp_path("fail");
        let mut s = StoreCleanups::load(&path).unwrap();
        s.request("notes").unwrap();
        s.fail("notes", "macOS 14 以下无法清除").unwrap();
        assert!(s.pending().is_empty());
        assert!(s.is_pending("notes"));
        assert_eq!(s.failed(), vec![("notes", "macOS 14 以下无法清除")]);
        let reloaded = StoreCleanups::load(&path).unwrap();
        assert!(reloaded.pending().is_empty());
        assert_eq!(reloaded.failed().len(), 1);
    }

    #[test]
    fn a_corrupt_file_is_an_error_and_never_becomes_an_empty_table() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{ this is not json";
        std::fs::write(&path, original).unwrap();

        let err = StoreCleanups::load(&path).unwrap_err();
        assert!(matches!(err, CleanupLoadError::Corrupt { .. }));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn an_unreadable_ledger_is_read_only_and_keeps_the_original_bytes() {
        let path = temp_path("unreadable");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\"apps\":{\"notes\":{\"state\":\"pending\"}}}";
        std::fs::write(&path, original).unwrap();
        // Make the bytes unreadable by pointing load at a directory instead of a file: typed I/O failure.
        let as_dir = temp_path("unreadable-dir");
        std::fs::create_dir_all(&as_dir).unwrap();
        let err = StoreCleanups::load(&as_dir).unwrap_err();
        assert!(matches!(err, CleanupLoadError::Io { .. }));

        let mut s = StoreCleanups::unavailable(&path, err.to_string());
        assert!(s.unavailable_reason().is_some());
        assert!(
            s.request("other").is_err(),
            "must not overwrite an unreadable ledger"
        );
        assert!(s.finish("notes").is_err());
        assert!(s.fail("notes", "x").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn a_failed_write_is_visible_and_the_same_request_retries_it() {
        let path = temp_path("write-fail");
        let mut s = StoreCleanups::load(&path).unwrap();
        s.fail_next_persist();
        let err = s.request("notes").unwrap_err();
        assert!(err.to_string().contains("injected"));
        assert!(s.dirty());
        assert!(s.last_error().is_some());
        assert!(s.is_pending("notes"), "the in-memory intent is kept");
        assert!(!path.exists(), "nothing was written");

        // Same content, explicit retry: must really persist this time.
        assert!(!s.request("notes").unwrap(), "already in memory");
        assert!(!s.dirty());
        assert!(s.last_error().is_none());
        assert!(StoreCleanups::load(&path).unwrap().is_pending("notes"));
    }

    #[test]
    fn a_failed_finish_restores_the_entry_so_it_can_be_retried() {
        let path = temp_path("finish-fail");
        let mut s = StoreCleanups::load(&path).unwrap();
        s.request("notes").unwrap();
        s.fail_next_persist();
        assert!(s.finish("notes").is_err());
        assert!(
            s.is_pending("notes"),
            "the removal was rolled back in memory"
        );
        assert!(s.dirty());
        s.finish("notes").unwrap();
        assert!(s.is_empty());
        assert!(!StoreCleanups::load(&path).unwrap().is_pending("notes"));
    }

    #[test]
    fn a_failed_fail_restores_the_previous_state() {
        let path = temp_path("fail-fail");
        let mut s = StoreCleanups::load(&path).unwrap();
        s.request("notes").unwrap();
        s.fail_next_persist();
        assert!(s.fail("notes", "boom").is_err());
        assert!(
            s.pending() == vec!["notes".to_string()],
            "previous Pending restored"
        );
        assert!(s.failed().is_empty());
        s.fail("notes", "boom").unwrap();
        assert_eq!(s.failed().len(), 1);
    }
    #[test]
    fn each_atomic_write_stage_preserves_the_original_and_can_retry() {
        for stage in [
            PersistStage::Write,
            PersistStage::Sync,
            PersistStage::Rename,
        ] {
            let path = temp_path(&format!("stage-{stage:?}"));
            let mut ledger = StoreCleanups::load(&path).unwrap();
            ledger.request("first").unwrap();
            let original = std::fs::read(&path).unwrap();
            ledger.fail_persist.set(Some(stage));
            assert!(ledger.request("second").is_err());
            assert!(ledger.dirty());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            assert_eq!(
                std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
                1
            );
            ledger.flush().unwrap();
            assert!(!ledger.dirty());
            assert!(StoreCleanups::load(&path).unwrap().is_pending("second"));
        }
    }
    #[test]
    fn repeated_request_keeps_a_terminal_failure_until_explicit_retry() {
        let path = temp_path("request-failed");
        let mut ledger = StoreCleanups::load(path).unwrap();
        ledger.request("notes").unwrap();
        ledger.fail("notes", "unsupported").unwrap();
        assert!(!ledger.request("notes").unwrap());
        assert_eq!(ledger.failed(), vec![("notes", "unsupported")]);
        ledger.retry("notes").unwrap();
        assert_eq!(ledger.pending(), vec!["notes"]);
    }
}
