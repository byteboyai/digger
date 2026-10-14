//! Digger 产品数据根:与 Dozer 完全独立,不共享注册表、端口或运行时目录。
//!
//! - 产品配置/布局:`<config>/digger/`(macOS: `~/Library/Application Support/ai.byteboy.digger/`
//!   由 `directories` 决定,与 Dozer 的 `ai.byteboy.dozer` 不同)。
//! - 应用宿主平台数据:`<config>/digger/bytehost/`(ADR-011 / 架构 §6):
//!   已安装应用、其数据与受管运行时都在这里,不进 Project 目录,也不与 Dozer 的
//!   `bytehost/` 根混用。

use directories::ProjectDirs;
use std::path::PathBuf;

/// Digger 的 per-user 目录。`ProjectDirs` 用不同的 qualifier/组织/应用名,保证
/// 与 Dozer 的目录不重叠(A15:两产品同时运行互不串扰)。
fn dirs() -> ProjectDirs {
    ProjectDirs::from("ai", "byteboy", "digger").expect("home directory must exist")
}

/// 产品配置目录(macOS: `~/Library/Application Support/ai.byteboy.digger` 由 directories 决定)。
pub fn config_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("DIGGER_DATA_DIR") {
        return PathBuf::from(root);
    }
    #[cfg(test)]
    return std::env::temp_dir().join(format!("digger-tests-{}", std::process::id()));
    #[cfg(not(test))]
    dirs().config_dir().to_path_buf()
}

/// 产品运行态目录(PID/锁/缓存)。
pub fn state_dir() -> PathBuf {
    if std::env::var_os("DIGGER_DATA_DIR").is_some() || cfg!(test) {
        return config_dir().join("state");
    }
    dirs()
        .state_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dirs().data_local_dir().to_path_buf())
}

/// 布局与偏好文件:全局 `layout.json`。
pub fn layout_path() -> PathBuf {
    config_dir().join("layout.json")
}

/// 应用宿主平台数据根:`<config>/digger/bytehost/`(ADR-011)。
pub fn bytehost_root() -> PathBuf {
    config_dir().join("bytehost")
}

/// 任务完成用量账本文件(Task 20):`<config>/digger/usage_ledger.json`。
pub fn usage_ledger_path() -> PathBuf {
    config_dir().join("usage_ledger.json")
}

/// 日志目录。
pub fn logs_dir() -> PathBuf {
    state_dir().join("logs")
}

/// 启动时确保产品数据根存在(配置 / 运行态 / 日志 / 应用宿主根)。
/// 只创建目录,不写任何业务状态。
pub fn ensure_data_root() -> std::io::Result<()> {
    for dir in [config_dir(), state_dir(), logs_dir(), bytehost_root()] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_ends_with_digger() {
        assert!(config_dir().to_string_lossy().contains("digger"));
    }

    #[test]
    fn config_dir_is_not_dozer() {
        // A15:与 Dozer 的数据根必须不同。
        assert!(!config_dir().to_string_lossy().contains("dozer"));
    }

    #[test]
    fn bytehost_root_is_under_config() {
        let r = bytehost_root();
        assert_eq!(r.file_name().unwrap(), "bytehost");
        assert!(r.starts_with(config_dir()));
    }

    #[test]
    fn layout_path_is_under_config() {
        let p = layout_path();
        assert_eq!(p.file_name().unwrap(), "layout.json");
        assert!(p.starts_with(config_dir()));
    }

    #[test]
    fn state_and_logs_dirs_are_under_digger_root() {
        assert!(state_dir().to_string_lossy().contains("digger"));
        let l = logs_dir();
        assert_eq!(l.file_name().unwrap(), "logs");
        assert!(l.starts_with(state_dir()));
    }
}
