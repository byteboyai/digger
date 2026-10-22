//! S6a Task 8:旧 `projects.db` 到 `digger.db` 的迁移。
//!
//! 迁移用 SQLite 的 `ATTACH` 在同一事务里把 legacy 的项目表与记忆表行按原 id
//! 拷入 target。成功后把 legacy 改名为 `projects.db.migrated`(保留不删除,D5)。
//!
//! **不改 Dozer**:迁移逻辑是 Digger 自己的事,只在 digger-store 里实现。

use dozer_services::{ServiceError, ServiceErrorKind};
use rusqlite::Connection;
use std::path::Path;

/// 迁移结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// 旧文件不存在,无事可做。
    NothingToMigrate,
    /// 迁移成功:`projects` 与 `memories` 各搬了多少行。
    Migrated { projects: usize, memories: usize },
    /// 旧文件存在但目标库已经有数据(说明迁移过),跳过。
    AlreadyMigrated,
}

/// 把 `legacy`(`projects.db`)的项目与记忆行按原 id 拷入 `target`(`digger.db`)。
///
/// 失败时 target 要么不存在要么与迁移前字节一致(事务回滚),legacy 仍在原位。
/// 成功后 legacy 改名为 `legacy.migrated`。
pub fn migrate_legacy_projects(
    legacy: &Path,
    target: &Path,
) -> Result<MigrationOutcome, ServiceError> {
    if !legacy.exists() {
        return Ok(MigrationOutcome::NothingToMigrate);
    }

    // 打开目标库(不存在则创建),先建表结构。
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ServiceError::new(ServiceErrorKind::Unavailable, format!("建目标库目录: {e}"))
        })?;
    }
    let target_conn = Connection::open(target).map_err(|e| {
        ServiceError::new(ServiceErrorKind::Unavailable, format!("打开目标库: {e}"))
    })?;

    // 建表结构(与 dozer-store-project 一致)。
    target_conn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                last_active_ms INTEGER NOT NULL,
                created_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                kind TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL,
                created_ms INTEGER NOT NULL,
                created_by TEXT NOT NULL,
                updated_ms INTEGER NOT NULL,
                updated_by TEXT NOT NULL
             );
             CREATE UNIQUE INDEX IF NOT EXISTS memories_project_title
                ON memories(project_id, title);
             CREATE TABLE IF NOT EXISTS memory_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_id INTEGER NOT NULL,
                project_id INTEGER NOT NULL,
                changed_ms INTEGER NOT NULL,
                changed_by TEXT NOT NULL,
                change_kind TEXT NOT NULL,
                title_snapshot TEXT NOT NULL,
                kind_snapshot TEXT NOT NULL,
                description_snapshot TEXT NOT NULL,
                body_snapshot TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS memory_history_memory
                ON memory_history(memory_id, changed_ms);",
        )
        .map_err(|e| ServiceError::new(ServiceErrorKind::Unavailable, format!("建目标表: {e}")))?;

    // 检查目标库是否已有数据(迁移过的标志)。
    let target_projects: i64 = target_conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap_or(0);
    let target_memories: i64 = target_conn
        .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
        .unwrap_or(0);
    if target_projects > 0 || target_memories > 0 {
        return Ok(MigrationOutcome::AlreadyMigrated);
    }

    // ATTACH legacy 库,在事务里拷贝数据。
    let legacy_str = legacy.to_string_lossy();
    target_conn
        .execute_batch(&format!("ATTACH DATABASE '{legacy_str}' AS legacy"))
        .map_err(|e| {
            ServiceError::new(ServiceErrorKind::Unavailable, format!("ATTACH legacy: {e}"))
        })?;

    let result = (|| -> Result<MigrationOutcome, ServiceError> {
        target_conn
            .execute_batch("BEGIN TRANSACTION")
            .map_err(|e| ServiceError::new(ServiceErrorKind::Unavailable, format!("BEGIN: {e}")))?;

        // 拷贝 projects(按原 id)。
        let projects_copied: i64 = target_conn
            .execute(
                "INSERT INTO projects (id, path, name, last_active_ms, created_ms)
                 SELECT id, path, name, last_active_ms, created_ms FROM legacy.projects",
                [],
            )
            .map_err(|e| {
                ServiceError::new(ServiceErrorKind::Unavailable, format!("拷贝 projects: {e}"))
            })? as i64;

        // 拷贝 memories(按原 id)。
        let memories_copied: i64 = target_conn
            .execute(
                "INSERT INTO memories (id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by)
                 SELECT id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by FROM legacy.memories",
                [],
            )
            .map_err(|e| {
                ServiceError::new(ServiceErrorKind::Unavailable, format!("拷贝 memories: {e}"))
            })? as i64;

        // 拷贝 memory_history(按原 id)。
        let _ = target_conn
            .execute(
                "INSERT INTO memory_history (id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot)
                 SELECT id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot FROM legacy.memory_history",
                [],
            )
            .map_err(|e| {
                ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    format!("拷贝 memory_history: {e}"),
                )
            })?;

        target_conn.execute_batch("COMMIT").map_err(|e| {
            ServiceError::new(ServiceErrorKind::Unavailable, format!("COMMIT: {e}"))
        })?;

        Ok(MigrationOutcome::Migrated {
            projects: projects_copied as usize,
            memories: memories_copied as usize,
        })
    })();

    // DETACH(无论成功失败)。
    let _ = target_conn.execute_batch("DETACH DATABASE legacy");

    match result {
        Ok(MigrationOutcome::Migrated { .. }) => {
            // 迁移成功:把 legacy 改名为 .migrated。
            let migrated_path = legacy.with_extension("db.migrated");
            std::fs::rename(legacy, &migrated_path).map_err(|e| {
                ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    format!("改名 legacy 为 .migrated: {e}"),
                )
            })?;
            result
        }
        other => other,
    }
}

/// 把 `digger.db` 切换到 WAL 模式(尽力而为,失败只返回 Err 不阻断)。
/// Digger 在 `open_in` 里调用,打开各 store 之前执行。
pub fn set_wal_mode(db_path: &Path) -> Result<(), ServiceError> {
    if !db_path.exists() {
        return Ok(());
    }
    let conn = Connection::open(db_path).map_err(|e| {
        ServiceError::new(
            ServiceErrorKind::Unavailable,
            format!("打开 db 设 WAL: {e}"),
        )
    })?;
    conn.execute_batch("PRAGMA journal_mode=WAL").map_err(|e| {
        ServiceError::new(ServiceErrorKind::Unavailable, format!("设 WAL 模式: {e}"))
    })?;
    Ok(())
}

/// 查询当前 db 的 journal_mode(测试用)。
pub fn journal_mode(db_path: &Path) -> Result<String, ServiceError> {
    let conn = Connection::open(db_path).map_err(|e| {
        ServiceError::new(
            ServiceErrorKind::Unavailable,
            format!("打开 db 查 journal_mode: {e}"),
        )
    })?;
    conn.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
        .map_err(|e| {
            ServiceError::new(
                ServiceErrorKind::Unavailable,
                format!("查 journal_mode: {e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 在 legacy 库里建表并插入测试数据。
    fn seed_legacy(legacy: &Path, projects: usize, memories_per_project: usize) {
        let conn = Connection::open(legacy).unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                last_active_ms INTEGER NOT NULL,
                created_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                kind TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL,
                created_ms INTEGER NOT NULL,
                created_by TEXT NOT NULL,
                updated_ms INTEGER NOT NULL,
                updated_by TEXT NOT NULL
             );
             CREATE TABLE memory_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_id INTEGER NOT NULL,
                project_id INTEGER NOT NULL,
                changed_ms INTEGER NOT NULL,
                changed_by TEXT NOT NULL,
                change_kind TEXT NOT NULL,
                title_snapshot TEXT NOT NULL,
                kind_snapshot TEXT NOT NULL,
                description_snapshot TEXT NOT NULL,
                body_snapshot TEXT NOT NULL
             );",
        )
        .unwrap();

        for i in 0..projects {
            conn.execute(
                "INSERT INTO projects (id, path, name, last_active_ms, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    (i + 1) as i64,
                    format!("/path/to/project{i}"),
                    format!("Project {i}"),
                    1000 + i as i64,
                    500 + i as i64,
                ],
            )
            .unwrap();
            for j in 0..memories_per_project {
                let mem_id = i * memories_per_project + j + 1;
                conn.execute(
                    "INSERT INTO memories (id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    rusqlite::params![
                        mem_id as i64,
                        (i + 1) as i64,
                        format!("Memory {mem_id}"),
                        "feedback",
                        "",
                        format!("Body of memory {mem_id}"),
                        2000 + mem_id as i64,
                        "user",
                        2000 + mem_id as i64,
                        "user",
                    ],
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn no_legacy_file_means_nothing_to_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        let result = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(result, MigrationOutcome::NothingToMigrate);
        assert!(!target.exists());
    }

    #[test]
    fn legacy_rows_are_copied_with_their_original_ids() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 3, 2);

        let result = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(
            result,
            MigrationOutcome::Migrated {
                projects: 3,
                memories: 6
            }
        );

        // 验证目标库的数据。
        let conn = Connection::open(&target).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);

        // 验证 id 保持不变。
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM projects ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);

        // 验证记忆也搬过去了。
        let mem_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mem_count, 6);
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 2, 1);

        let r1 = migrate_legacy_projects(&legacy, &target).unwrap();
        assert!(matches!(r1, MigrationOutcome::Migrated { .. }));

        // 第二次调用:legacy 已被改名为 .migrated,所以返回 NothingToMigrate。
        let r2 = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(r2, MigrationOutcome::NothingToMigrate);

        // 目标库行数不变。
        let conn = Connection::open(&target).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn migrated_legacy_file_is_renamed_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 1, 1);

        let _ = migrate_legacy_projects(&legacy, &target).unwrap();

        assert!(!legacy.exists(), "legacy should be renamed");
        let migrated = dir.path().join("projects.db.migrated");
        assert!(migrated.exists(), "legacy should be renamed to .migrated");
    }

    #[test]
    fn already_migrated_when_target_has_data() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");

        // 先在目标库里放数据。
        seed_legacy(&target, 1, 1);
        // 再建 legacy(模拟旧文件还在)。
        seed_legacy(&legacy, 1, 1);

        let result = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(result, MigrationOutcome::AlreadyMigrated);

        // legacy 文件应该还在(没被改名)。
        assert!(legacy.exists());
    }

    #[test]
    fn open_in_switches_digger_db_to_wal() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("digger.db");
        // 先建一个普通 journal 模式的 db。
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("CREATE TABLE t (x)").unwrap();
        }
        let mode_before = journal_mode(&db).unwrap();
        assert_eq!(mode_before, "delete", "baseline should be delete mode");

        set_wal_mode(&db).unwrap();
        let mode_after = journal_mode(&db).unwrap();
        assert_eq!(mode_after, "wal");
    }
}
