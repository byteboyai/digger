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
    /// 迁移成功:`projects` 与 `memories` 各搬了多少行(历史行随记忆一起搬,不单独计数)。
    Migrated { projects: usize, memories: usize },
}

fn unavailable(what: &str, e: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, format!("{what}: {e}"))
}

/// `conn` 的 `schema` 库里 `table` 是否有 `column` 列(`schema` 为 `legacy` 等已 ATTACH 的库名)。
fn has_column(conn: &Connection, schema: &str, table: &str, column: &str) -> bool {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{table}', '{schema}') WHERE name = ?1"),
        [column],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

fn has_table(conn: &Connection, schema: &str, table: &str) -> bool {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {schema}.sqlite_master WHERE type='table' AND name = ?1"),
        [table],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// 把 `legacy`(`projects.db`)的项目与记忆行按原 id 拷入 `target`(`digger.db`)。
///
/// - 目标表结构由 `dozer-store-project` 自己建(`ProjectHost::open`),这里不抄 schema。
/// - 一个事务;与目标库已有行的 id/路径/标题冲突时整体回滚并返回错误(不覆盖、不部分写入),legacy 保持原位。
/// - 失败时若 `target` 是本次调用才创建的,会被删除;已存在的 `target` 回滚后内容不变。
/// - 成功后 legacy 改名为 `projects.db.migrated`(保留不删除)。
pub fn migrate_legacy_projects(
    legacy: &Path,
    target: &Path,
) -> Result<MigrationOutcome, ServiceError> {
    if !legacy.exists() {
        return Ok(MigrationOutcome::NothingToMigrate);
    }
    let target_existed = target.exists();
    let result = copy_rows(legacy, target);
    match result {
        Ok(outcome) => {
            let mut migrated = legacy.as_os_str().to_owned();
            migrated.push(".migrated");
            std::fs::rename(legacy, std::path::PathBuf::from(migrated))
                .map_err(|e| unavailable("改名 legacy 为 .migrated", e))?;
            Ok(outcome)
        }
        Err(e) => {
            if !target_existed {
                for suffix in ["", "-wal", "-shm", "-journal"] {
                    let mut p = target.as_os_str().to_owned();
                    p.push(suffix);
                    let _ = std::fs::remove_file(std::path::PathBuf::from(p));
                }
            }
            Err(e)
        }
    }
}

fn copy_rows(legacy: &Path, target: &Path) -> Result<MigrationOutcome, ServiceError> {
    // 让两个 store 建好(并升级)目标表结构。
    crate::ProjectHost::open(target)?;

    let mut conn = Connection::open(target).map_err(|e| unavailable("打开目标库", e))?;
    conn.execute(
        "ATTACH DATABASE ?1 AS legacy",
        [legacy.to_string_lossy().as_ref()],
    )
    .map_err(|e| unavailable("ATTACH legacy", e))?;

    let created = if has_column(&conn, "legacy", "projects", "created_ms") {
        "created_ms"
    } else {
        "last_active_ms"
    };
    let has_desc = has_column(&conn, "legacy", "memories", "description");
    let desc = if has_desc { "description" } else { "''" };
    let has_memories = has_table(&conn, "legacy", "memories");
    let has_history = has_table(&conn, "legacy", "memory_history");

    let tx = conn.transaction().map_err(|e| unavailable("BEGIN", e))?;
    let projects = tx
        .execute(
            &format!(
                "INSERT INTO main.projects (id, path, name, last_active_ms, created_ms)
                 SELECT id, path, name, last_active_ms, {created} FROM legacy.projects"
            ),
            [],
        )
        .map_err(|e| unavailable("拷贝 projects(与 digger.db 已有行冲突?)", e))?;
    let mut memories = 0;
    if has_memories {
        memories = tx
            .execute(
                &format!(
                    "INSERT INTO main.memories (id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by)
                     SELECT id, project_id, title, kind, {desc}, body, created_ms, created_by, updated_ms, updated_by FROM legacy.memories"
                ),
                [],
            )
            .map_err(|e| unavailable("拷贝 memories(与 digger.db 已有行冲突?)", e))?;
    }
    if has_history {
        tx.execute(
            "INSERT INTO main.memory_history (id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot)
             SELECT id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot FROM legacy.memory_history",
            [],
        )
        .map_err(|e| unavailable("拷贝 memory_history", e))?;
    }
    tx.commit().map_err(|e| unavailable("COMMIT", e))?;
    let _ = conn.execute_batch("DETACH DATABASE legacy");
    Ok(MigrationOutcome::Migrated { projects, memories })
}

/// 把 `digger.db` 切换到 WAL 模式(尽力而为,失败只返回 Err 不阻断)。
/// Digger 在 `open_in` 里调用,打开各 store 之前执行。
pub fn set_wal_mode(db_path: &Path) -> Result<(), ServiceError> {
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
    fn conflicting_target_rows_fail_without_overwriting_and_keep_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&target, 1, 1);
        seed_legacy(&legacy, 1, 1);

        let err = migrate_legacy_projects(&legacy, &target).unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::Unavailable);
        assert!(legacy.exists(), "legacy must stay in place");
        let c = Connection::open(&target).unwrap();
        let names: Vec<String> = c
            .prepare("SELECT name FROM projects")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(names, vec!["Project 0".to_string()]);
    }

    #[test]
    fn an_id_collision_with_an_existing_target_project_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 2, 1);
        {
            let host = crate::ProjectHost::open(&target).unwrap();
            let p = dir.path().join("existing");
            std::fs::create_dir(&p).unwrap();
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(host.projects.open_project(p.to_string_lossy().into_owned()))
                .unwrap()
                .unwrap();
        }
        // 已有项目占了 id 1,与 legacy 的 id 1 冲突 → 报错,legacy 留在原位。
        assert!(migrate_legacy_projects(&legacy, &target).is_err());
        assert!(legacy.exists());
    }

    #[test]
    fn a_failed_migration_leaves_no_new_digger_db() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        // legacy 不是 SQLite 库:ATTACH 后读表失败。
        std::fs::write(&legacy, b"this is not a database at all, just text").unwrap();

        assert!(migrate_legacy_projects(&legacy, &target).is_err());
        assert!(!target.exists(), "a partial digger.db must not remain");
        assert!(legacy.exists());
    }

    #[test]
    fn legacy_without_created_ms_and_description_columns_still_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        {
            let c = Connection::open(&legacy).unwrap();
            c.execute_batch(
                "CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, last_active_ms INTEGER NOT NULL);
                 INSERT INTO projects VALUES (7, '/p', 'P', 1234);",
            )
            .unwrap();
        }
        let r = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(
            r,
            MigrationOutcome::Migrated {
                projects: 1,
                memories: 0
            }
        );
        let c = Connection::open(&target).unwrap();
        let (id, created): (i64, i64) = c
            .query_row("SELECT id, created_ms FROM projects", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((id, created), (7, 1234));
    }

    #[test]
    fn a_legacy_path_with_a_quote_is_attached_safely() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("it's here");
        std::fs::create_dir(&sub).unwrap();
        let legacy = sub.join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 1, 1);
        assert!(matches!(
            migrate_legacy_projects(&legacy, &target).unwrap(),
            MigrationOutcome::Migrated {
                projects: 1,
                memories: 1
            }
        ));
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
