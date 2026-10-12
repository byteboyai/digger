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
//! 本模块**不碰** wry / 系统版本号:真正的 `remove_data_store` 调用在宿主的窗口层(desktop)完成。
//! 这一层只负责"哪些应用还欠一次清除"的持久事实与状态机,可脱离窗口单测。

use std::collections::BTreeMap;
use std::io;
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

/// 持久化的"待清除数据存储"台账。键是应用 id,值是当前状态。
///
/// 落盘为一个小 JSON:进程任何时刻崩溃/被杀,`Pending` 项下次启动继续清,`Failed` 项留在设置页可见。
#[derive(Debug)]
pub struct StoreCleanups {
    path: PathBuf,
    entries: BTreeMap<String, CleanupState>,
}

/// 磁盘上的形状:`{ "apps": { "<id>": {"state": "pending"} | {"state":"failed","reason":"…"} } }`。
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    apps: BTreeMap<String, CleanupState>,
}

impl StoreCleanups {
    /// 从磁盘读入(文件不存在 / 损坏都当空表——这是清除待办,读不出来不能让启动失败)。
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OnDisk>(&bytes).ok())
            .map(|d| d.apps)
            .unwrap_or_default();
        Self { path, entries }
    }

    /// 请求清除某应用的存储(幂等:已存在则不重复排队,也不改已有状态)。返回是否**新增**。
    ///
    /// 落盘失败会返回错误,但内存状态仍已更新——调用方据此提示"清除待办可能没被记下,重启后不保证继续"。
    pub fn request(&mut self, app_id: &str) -> io::Result<bool> {
        if self.entries.contains_key(app_id) {
            return Ok(false);
        }
        self.entries
            .insert(app_id.to_owned(), CleanupState::Pending);
        self.persist()?;
        Ok(true)
    }

    /// 清除成功:从台账移除并落盘。
    pub fn finish(&mut self, app_id: &str) -> io::Result<()> {
        if self.entries.remove(app_id).is_some() {
            self.persist()?;
        }
        Ok(())
    }

    /// 记录最终失败(不再自动重试),保留给设置页展示;成功状态不覆盖失败。
    pub fn fail(&mut self, app_id: &str, reason: impl Into<String>) -> io::Result<()> {
        let reason = reason.into();
        let next = CleanupState::Failed { reason };
        if self.entries.get(app_id) == Some(&next) {
            return Ok(());
        }
        self.entries.insert(app_id.to_owned(), next);
        self.persist()
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

    /// 原子落盘(先写临时文件再 rename),避免半截 JSON 让下次启动误判。
    fn persist(&self) -> io::Result<()> {
        let disk = OnDisk {
            apps: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&disk).map_err(io::Error::other)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)
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
        let mut s = StoreCleanups::load(&path);
        assert!(s.request("notes").unwrap());
        assert_eq!(s.pending(), vec!["notes".to_string()]);

        // 模拟退出重启:重新从磁盘读入,待办仍在。
        let reloaded = StoreCleanups::load(&path);
        assert!(reloaded.is_pending("notes"));
        assert_eq!(reloaded.pending(), vec!["notes".to_string()]);
    }

    #[test]
    fn requesting_twice_is_idempotent() {
        let path = temp_path("idem");
        let mut s = StoreCleanups::load(&path);
        assert!(s.request("notes").unwrap(), "首次新增");
        assert!(!s.request("notes").unwrap(), "重复请求不新增");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn finishing_removes_it_from_disk() {
        let path = temp_path("finish");
        let mut s = StoreCleanups::load(&path);
        s.request("notes").unwrap();
        s.finish("notes").unwrap();
        assert!(s.is_empty());
        assert!(
            !StoreCleanups::load(&path).is_pending("notes"),
            "落盘也清了"
        );
    }

    #[test]
    fn a_failure_is_recorded_and_not_auto_retried_but_still_visible() {
        let path = temp_path("fail");
        let mut s = StoreCleanups::load(&path);
        s.request("notes").unwrap();
        s.fail("notes", "macOS 14 以下无法清除").unwrap();
        // 不再出现在"要驱动"的待办里,但仍在台账里(设置页可见)。
        assert!(s.pending().is_empty());
        assert!(s.is_pending("notes"));
        assert_eq!(s.failed(), vec![("notes", "macOS 14 以下无法清除")]);
        // 重启后失败仍在,不会被当成待办重新清。
        let reloaded = StoreCleanups::load(&path);
        assert!(reloaded.pending().is_empty());
        assert_eq!(reloaded.failed().len(), 1);
    }

    #[test]
    fn a_corrupt_file_is_treated_as_empty_instead_of_failing_startup() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ this is not json").unwrap();
        let s = StoreCleanups::load(&path);
        assert!(s.is_empty(), "读不出就当空表,不能让启动失败");
    }
}
