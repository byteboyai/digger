//! Digger 产品数据根:委托给 `digger-paths`(S6a)。
//!
//! 本模块保留对外函数名(`config_dir` / `state_dir` / `shell_path` / `bytehost_root` 等),
//! 实际实现全部由 `digger-paths` 提供。`PROJECTS_DB_FILE` 在 Task 8 改为 `digger.db`。

pub use digger_paths::{
    PROJECT_TABS_FILE, PROJECTS_DB_FILE, bytehost_root, config_dir, ensure_data_root, shell_path,
    state_dir, usage_ledger_path,
};
