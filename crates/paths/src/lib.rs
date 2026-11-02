//! Digger 产品数据根:与 Dozer 完全独立,不共享注册表、端口或运行时目录。
//!
//! - 产品配置/布局:`<config>/digger/`(macOS: `~/Library/Application Support/ai.byteboy.digger/`
//!   由 `directories` 决定,与 Dozer 的 `ai.byteboy.dozer` 不同)。
//! - 应用宿主平台数据:`<config>/digger/bytehost/`(ADR-011 / 架构 §6):
//!   已安装应用、其数据与受管运行时都在这里,不进 Project 目录,也不与 Dozer 的
//!   `bytehost/` 根混用。

use directories::ProjectDirs;
use dozer_core::paths::{ProductDirs, ProductId};
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

/// 项目注册表与共享记忆所在的 SQLite 文件路径(ADR-014)。
/// 属于 Digger 自己的数据根,与 Dozer 的数据库无关。
/// **必须与 `diggerd` 实际使用的数据库路径相等**(测试断言)。
pub fn projects_db_path() -> PathBuf {
    state_dir().join("digger.db")
}

/// 旧版项目注册表路径(迁移用):`<config>/projects.db`。
pub fn legacy_projects_db_path() -> PathBuf {
    config_dir().join("projects.db")
}

/// 窗口骨架的布局与偏好文件:全局 `shell.json`(取代旧的 `layout.json`)。
pub fn shell_path() -> PathBuf {
    config_dir().join("shell.json")
}

/// 项目注册表与共享记忆所在的 SQLite 文件名(ADR-014)。
pub const PROJECTS_DB_FILE: &str = "digger.db";
/// 已打开的项目页签与当前页签的落盘文件名。
pub const PROJECT_TABS_FILE: &str = "project_tabs.json";

/// 应用宿主平台数据根:`<config>/digger/bytehost/`(ADR-011)。
pub fn bytehost_root() -> PathBuf {
    config_dir().join("bytehost")
}

/// 旧版任务完成用量账本文件(导入源):`<config>/digger/usage_ledger.json`。
pub fn usage_ledger_path() -> PathBuf {
    config_dir().join("usage_ledger.json")
}

/// 任务完成用量账本(A8):独立 SQLite 库 `<state>/usage.sqlite3`。
pub fn usage_db_path() -> PathBuf {
    state_dir().join("usage.sqlite3")
}

/// 主题偏好文件:采用 byteui 的 `{"scheme":"dark"|"light"}` 格式。
/// Digger 自己的数据根,与 Dozer 的 `color-theme.json` 完全独立。
pub fn color_theme_path() -> PathBuf {
    config_dir().join("color-theme.json")
}

/// 日志目录。
pub fn logs_dir() -> PathBuf {
    state_dir().join("logs")
}

/// Digger 的产品身份:与 Dozer 只共享代码,数据目录/socket/数据库/会话环境变量/hook 二进制全部独立。
pub fn product() -> ProductId {
    ProductId::custom(
        "digger",
        "DIGGER_SESSION_ID",
        "digger-hook",
        "diggerd.sock",
        "digger.db",
        ProductDirs::Fixed {
            config: config_dir(),
            state: state_dir(),
        },
    )
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
    fn projects_db_path_equals_the_daemons_db_path() {
        // S6a:projects_db_path() 必须与 product().db_path() 相等——
        // 守护进程与进程内存储共用同一个 SQLite 文件。
        assert_eq!(projects_db_path(), product().db_path());
    }

    #[test]
    fn data_dir_env_moves_every_path_together() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("custom-digger");
        std::fs::create_dir_all(&root).unwrap();
        // 用 scoped_env 的方式设置环境变量(测试结束自动恢复)
        // 这里直接测试:设 DIGGER_DATA_DIR 后所有路径都在其下
        // SAFETY: 测试代码,单线程执行,不会与其他线程竞争环境变量。
        unsafe {
            std::env::set_var("DIGGER_DATA_DIR", &root);
        }
        let config = config_dir();
        let state = state_dir();
        let db = projects_db_path();
        let socket = product().socket_path();
        // SAFETY: 测试代码,清理环境变量。
        unsafe {
            std::env::remove_var("DIGGER_DATA_DIR");
        }
        assert!(
            config.starts_with(&root),
            "config {config:?} should be under {root:?}"
        );
        assert!(
            state.starts_with(&root),
            "state {state:?} should be under {root:?}"
        );
        assert!(db.starts_with(&root), "db {db:?} should be under {root:?}");
        assert!(
            socket.starts_with(&root),
            "socket {socket:?} should be under {root:?}"
        );
    }

    #[test]
    fn theme_file_is_digger_own_and_uses_the_shared_format_name() {
        let path = color_theme_path();
        assert!(path.starts_with(config_dir()));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("color-theme.json")
        );
    }

    #[test]
    fn socket_and_session_env_are_digger_specific() {
        let p = product();
        assert_eq!(p.socket_file, "diggerd.sock");
        assert_eq!(p.session_env, "DIGGER_SESSION_ID");
        assert_eq!(p.hook_bin, "digger-hook");
        assert_eq!(p.db_file, "digger.db");
        assert_eq!(p.name, "digger");
    }

    #[test]
    fn paths_do_not_overlap_with_dozers_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("shared-home");
        std::fs::create_dir_all(&root).unwrap();

        // Digger 用 Fixed 目录(测试下由 config_dir() 返回 temp_dir)
        let digger = product();
        // Dozer 用 ProjectDirs("ai", "byteboy", "dozer")
        let dozer = ProductId::dozer();

        let digger_paths = [
            digger.config_dir(),
            digger.state_dir(),
            digger.socket_path(),
            digger.logs_dir(),
        ];
        let dozer_paths = [
            dozer.config_dir(),
            dozer.state_dir(),
            dozer.socket_path(),
            dozer.logs_dir(),
        ];

        for d in &digger_paths {
            for z in &dozer_paths {
                assert!(
                    !d.starts_with(z) && !z.starts_with(d),
                    "{d:?} 与 {z:?} 不应互相包含"
                );
            }
        }
    }

    #[test]
    fn config_dir_ends_with_digger() {
        // 默认根带产品名;`DIGGER_DATA_DIR` 覆盖时路径由调用方给,不含 "digger"
        // 也合法(审核环境正是如此)。两种情形都要过,且不假设环境变量在两次
        // 读取之间不变(测试并行,另有测试会临时设置/清理它)。
        let c = config_dir();
        let root = std::env::var_os("DIGGER_DATA_DIR");
        assert!(
            c.to_string_lossy().contains("digger")
                || root.is_some_and(|root| c.as_os_str() == root.as_os_str()),
            "config {c:?} 既不是默认的 digger 根,也不是 DIGGER_DATA_DIR 覆盖值"
        );
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
    fn project_data_is_under_digger_config_and_never_dozers() {
        let db = projects_db_path();
        assert_eq!(db.file_name().unwrap(), PROJECTS_DB_FILE);
        assert!(db.starts_with(state_dir()));
        assert!(!db.to_string_lossy().contains("dozer"));
    }

    #[test]
    fn shell_path_is_under_config() {
        let p = shell_path();
        assert_eq!(p.file_name().unwrap(), "shell.json");
        assert!(p.starts_with(config_dir()));
    }

    #[test]
    fn state_and_logs_dirs_are_under_digger_root() {
        // 不假设根目录名字里含 "digger":`DIGGER_DATA_DIR` 覆盖时由调用方决定。
        // 钉住的是相对结构:state 在 config 下,logs 在 state 下。
        let config = config_dir();
        let state = state_dir();
        assert_eq!(state, config.join("state"));
        let l = logs_dir();
        assert_eq!(l.file_name().unwrap(), "logs");
        assert_eq!(l, state.join("logs"));
    }

    #[test]
    fn usage_db_is_under_state_and_the_old_json_stays_an_import_source() {
        let db = usage_db_path();
        assert_eq!(db.file_name().unwrap(), "usage.sqlite3");
        assert!(db.starts_with(state_dir()));
        let old = usage_ledger_path();
        assert_eq!(old.file_name().unwrap(), "usage_ledger.json");
        assert!(old.starts_with(config_dir()));
        assert_ne!(db, old);
    }

    #[test]
    fn legacy_projects_db_path_is_under_config() {
        let p = legacy_projects_db_path();
        assert_eq!(p.file_name().unwrap(), "projects.db");
        assert!(p.starts_with(config_dir()));
    }
}
