//! S6a Task 8:旧 `projects.db` 到 `digger.db` 的迁移。
//!
//! 迁移用 SQLite 的 `ATTACH` 在同一事务里把 legacy 的项目表与记忆表行按原 id
//! 拷入 target。数据行与 Digger 自己的完成标记(`digger_migration_runs`)在**同一事务**提交:
//! 重启后不会因为"已经复制成功、但改名失败"而重复插入主键冲突。
//!
//! 成功的迁移把 legacy 改名为 `projects.db.migrated`(保留不删除,D5)。改名失败是
//! "数据已迁移、归档待完成",返回 [`MigrationOutcome::ArchivePending`],不阻止项目服务启动。
//!
//! **不改 Dozer**:迁移逻辑是 Digger 自己的事,只在 digger-store 里实现;完成标记是本产品
//! 命名空间下的独立表,不占用共享 store 的 `PRAGMA user_version`,也不改它的表定义。

use dozer_services::{ServiceError, ServiceErrorKind};
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 迁移结果的格式版本。表结构或核对规则变化时递增。
const FORMAT_VERSION: i64 = 1;
/// Digger 自己的迁移记录表名(不碰共享 store 的 user_version)。
const MARKER_TABLE: &str = "digger_migration_runs";
const MIGRATION_KIND: &str = "projects";

/// 迁移结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// 旧文件不存在,无事可做。
    NothingToMigrate,
    /// 本文件此前已经迁移完成(标记在,旧文件已归档或不存在)。
    AlreadyMigrated,
    /// 迁移成功(本次复制,或此前已提交、本次仅完成归档)。
    Migrated { projects: usize, memories: usize },
    /// 数据已经提交,但旧文件归档改名失败。项目服务可以继续使用,提示用户归档待完成。
    ArchivePending {
        projects: usize,
        memories: usize,
        error: String,
    },
}

fn unavailable(what: impl std::fmt::Display, e: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, format!("{what}: {e}"))
}

fn conflict(what: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, format!("迁移冲突: {what}"))
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

/// 一次调用的规范化来源身份。文件存在时用真实路径,避免同一 legacy 经不同相对路径被认成两份。
fn source_identity(legacy: &Path) -> String {
    // 规范化**父目录**而不是文件本身:文件成功归档后,后续启动仍要能算出同一个身份。
    let parent = legacy.parent().filter(|p| !p.as_os_str().is_empty());
    let resolved_parent = parent
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| parent.map(Path::to_path_buf).unwrap_or_default());
    let path = match legacy.file_name() {
        Some(name) => resolved_parent.join(name),
        None => legacy.to_path_buf(),
    };
    path.to_string_lossy().into_owned()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn ensure_marker_table(conn: &Connection) -> Result<(), ServiceError> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS main.{MARKER_TABLE} (
            kind TEXT NOT NULL,
            format_version INTEGER NOT NULL,
            source_identity TEXT NOT NULL,
            completed_ms INTEGER NOT NULL,
            projects_copied INTEGER NOT NULL,
            memories_copied INTEGER NOT NULL,
            history_copied INTEGER NOT NULL,
            PRIMARY KEY (kind, format_version, source_identity)
         );"
    ))
    .map_err(|e| unavailable("创建 Digger 迁移记录表", e))
}

/// 读取完成标记的复制计数;`None` = 尚未标记。
fn marker_counts(
    conn: &Connection,
    identity: &str,
) -> Result<Option<(usize, usize)>, ServiceError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT projects_copied, memories_copied FROM main.{MARKER_TABLE}
             WHERE kind = ?1 AND format_version = ?2 AND source_identity = ?3"
        ))
        .map_err(|e| unavailable("查询迁移记录", e))?;
    match stmt.query_row(
        rusqlite::params![MIGRATION_KIND, FORMAT_VERSION, identity],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        Ok((p, m)) => Ok(Some((
            usize::try_from(p).map_err(|_| conflict("迁移记录中的 projects 计数越界"))?,
            usize::try_from(m).map_err(|_| conflict("迁移记录中的 memories 计数越界"))?,
        ))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(unavailable("查询迁移记录", e)),
    }
}

fn table_rows(conn: &Connection, schema: &str, table: &str) -> Result<usize, ServiceError> {
    let n: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {schema}.{table}"), [], |r| {
            r.get(0)
        })
        .map_err(|e| unavailable(format!("统计 {schema}.{table}"), e))?;
    usize::try_from(n).map_err(|_| conflict(format!("{schema}.{table} 行数越界")))
}

/// 旧版"已经复制、但没有新标记"的严格行级核对。
///
/// 用旧 schema 的规范化投影(缺列用默认值)逐表做 `EXCEPT`:来源每一行都必须在目标里出现。
/// 目标额外行不删除;任何来源行缺失或内容不同都停止自动恢复,不把"碰巧部分相同"当成功。
fn verify_old_copy(
    conn: &Connection,
    created: &str,
    desc: &str,
    has_memories: bool,
    has_history: bool,
) -> Result<(), ServiceError> {
    let mut checks = vec![(
        "projects",
        format!(
            "SELECT id, path, name, last_active_ms, {created} AS created_ms FROM legacy.projects
             EXCEPT
             SELECT id, path, name, last_active_ms, created_ms FROM main.projects"
        ),
    )];
    if has_memories {
        checks.push((
            "memories",
            format!(
                "SELECT id, project_id, title, kind, {desc} AS description, body, created_ms, created_by, updated_ms, updated_by
                 FROM legacy.memories
                 EXCEPT
                 SELECT id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by
                 FROM main.memories"
            ),
        ));
    }
    if has_history {
        checks.push((
            "memory_history",
            "SELECT id, memory_id, project_id, changed_ms, changed_by, change_kind,
                    title_snapshot, kind_snapshot, description_snapshot, body_snapshot
             FROM legacy.memory_history
             EXCEPT
             SELECT id, memory_id, project_id, changed_ms, changed_by, change_kind,
                    title_snapshot, kind_snapshot, description_snapshot, body_snapshot
             FROM main.memory_history"
                .to_string(),
        ));
    }
    for (table, query) in checks {
        let missing: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM ({query})"), [], |r| r.get(0))
            .map_err(|e| unavailable(format!("核对旧版 {table} 行"), e))?;
        if missing != 0 {
            return Err(conflict(format!(
                "目标库已有数据,但旧版 {table} 有 {missing} 行在目标中缺失或内容不同;未自动覆盖,请先备份后人工处理"
            )));
        }
    }
    Ok(())
}

struct Projection {
    created: &'static str,
    desc: &'static str,
    has_memories: bool,
    has_history: bool,
}

fn projection(conn: &Connection) -> Projection {
    Projection {
        created: if has_column(conn, "legacy", "projects", "created_ms") {
            "created_ms"
        } else {
            "last_active_ms"
        },
        desc: if has_column(conn, "legacy", "memories", "description") {
            "description"
        } else {
            "''"
        },
        has_memories: has_table(conn, "legacy", "memories"),
        has_history: has_table(conn, "legacy", "memory_history"),
    }
}

/// 在目标库事务里完成"核对 / 复制 + 完成标记",返回 `(projects, memories)`。
fn copy_and_mark(
    conn: &mut Connection,
    identity: &str,
    p: &Projection,
) -> Result<(usize, usize), ServiceError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| unavailable("BEGIN IMMEDIATE", e))?;

    // 锁内重新检查标记:两个迁移调用(线程 / 进程)只有一个会真正复制。
    if let Some(counts) = marker_counts(&tx, identity)? {
        #[cfg(test)]
        crate::crash_at("projects_before_commit");
        tx.commit().map_err(|e| unavailable("COMMIT", e))?;
        #[cfg(test)]
        crate::crash_at("projects_after_commit");
        return Ok(counts);
    }

    let target_rows = table_rows(&tx, "main", "projects")?
        + table_rows(&tx, "main", "memories").unwrap_or(0)
        + table_rows(&tx, "main", "memory_history").unwrap_or(0);
    let (projects, memories);
    if target_rows > 0 {
        // 旧版可能已经复制而未写标记:只有全部来源行逐行一致,才允许补标记。
        verify_old_copy(&tx, p.created, p.desc, p.has_memories, p.has_history)?;
        projects = table_rows(&tx, "legacy", "projects")?;
        memories = if p.has_memories {
            table_rows(&tx, "legacy", "memories")?
        } else {
            0
        };
    } else {
        projects = tx
            .execute(
                &format!(
                    "INSERT INTO main.projects (id, path, name, last_active_ms, created_ms)
                     SELECT id, path, name, last_active_ms, {} FROM legacy.projects",
                    p.created
                ),
                [],
            )
            .map_err(|e| unavailable("拷贝 projects(与 digger.db 已有行冲突?)", e))?;
        memories = if p.has_memories {
            tx.execute(
                &format!(
                    "INSERT INTO main.memories (id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by)
                     SELECT id, project_id, title, kind, {}, body, created_ms, created_by, updated_ms, updated_by FROM legacy.memories",
                    p.desc
                ),
                [],
            )
            .map_err(|e| unavailable("拷贝 memories(与 digger.db 已有行冲突?)", e))?
        } else {
            0
        };
    }
    let history = if p.has_history {
        if target_rows == 0 {
            tx.execute(
                "INSERT INTO main.memory_history (id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot)
                 SELECT id, memory_id, project_id, changed_ms, changed_by, change_kind, title_snapshot, kind_snapshot, description_snapshot, body_snapshot FROM legacy.memory_history",
                [],
            )
            .map_err(|e| unavailable("拷贝 memory_history", e))?
        } else {
            table_rows(&tx, "main", "memory_history")?
        }
    } else {
        0
    };
    tx.execute(
        &format!(
            "INSERT INTO main.{MARKER_TABLE}
             (kind, format_version, source_identity, completed_ms, projects_copied, memories_copied, history_copied)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ),
        rusqlite::params![
            MIGRATION_KIND,
            FORMAT_VERSION,
            identity,
            now_ms(),
            i64::try_from(projects).map_err(|_| conflict("projects 计数越界"))?,
            i64::try_from(memories).map_err(|_| conflict("memories 计数越界"))?,
            i64::try_from(history).map_err(|_| conflict("history 计数越界"))?,
        ],
    )
    .map_err(|e| unavailable("写迁移完成标记", e))?;
    #[cfg(test)]
    crate::crash_at("projects_before_commit");
    tx.commit().map_err(|e| unavailable("COMMIT", e))?;
    #[cfg(test)]
    crate::crash_at("projects_after_commit");
    Ok((projects, memories))
}

/// 把 legacy 改名为 `*.migrated`,不覆盖已有归档文件。
fn archive_legacy(legacy: &Path) -> Result<(), String> {
    crate::archive::legacy(legacy)
}

/// 把 `legacy`(`projects.db`)的项目与记忆行按原 id 拷入 `target`(`digger.db`)。
///
/// - 目标表结构由 `dozer-store-project` 自己建(`ProjectHost::open`),这里不抄 schema。
/// - 数据复制与 Digger 命名空间下的完成标记在同一事务;提交后崩溃 / 重启只补归档,不重复插入。
/// - 目标已有数据时只接受"旧版半完成"的逐行核对;真实冲突整体失败,不覆盖。
/// - 归档失败返回 [`MigrationOutcome::ArchivePending`](数据已提交,可重试归档),不阻止项目服务启动。
pub fn migrate_legacy_projects(
    legacy: &Path,
    target: &Path,
) -> Result<MigrationOutcome, ServiceError> {
    migrate_legacy_projects_with(legacy, target, || {})
}

/// Internal seam for the concurrency regression: `after_target_check` runs after the pre-flight
/// check and before any database write. Production passes a no-op.
fn migrate_legacy_projects_with(
    legacy: &Path,
    target: &Path,
    after_target_check: impl FnOnce(),
) -> Result<MigrationOutcome, ServiceError> {
    let identity = source_identity(legacy);
    if !legacy.exists() {
        if target.exists() {
            let conn = Connection::open(target).map_err(|e| unavailable("打开目标库", e))?;
            ensure_marker_table(&conn)?;
            if marker_counts(&conn, &identity)?.is_some() {
                return Ok(MigrationOutcome::AlreadyMigrated);
            }
        }
        return Ok(MigrationOutcome::NothingToMigrate);
    }

    let _target_existed = target.exists();
    after_target_check();
    // Never delete `target` or its sidecars here. The pre-flight `exists()` does not prove ownership:
    // another writer may have created and committed the file after that check. A rolled-back
    // transaction leaves the shared file in place; cleanup is the owner's business.
    migrate_into_target(legacy, target, &identity)
}

fn migrate_into_target(
    legacy: &Path,
    target: &Path,
    identity: &str,
) -> Result<MigrationOutcome, ServiceError> {
    // 让共享 store 建好(并升级)目标表结构。
    crate::ProjectHost::open(target)?;

    let mut conn = Connection::open(target).map_err(|e| unavailable("打开目标库", e))?;
    ensure_marker_table(&conn)?;
    conn.execute(
        "ATTACH DATABASE ?1 AS legacy",
        [legacy.to_string_lossy().as_ref()],
    )
    .map_err(|e| unavailable("ATTACH legacy", e))?;

    let p = projection(&conn);
    let copied = copy_and_mark(&mut conn, identity, &p);
    let _ = conn.execute_batch("DETACH DATABASE legacy");
    drop(conn);

    let (projects, memories) = copied?;
    match archive_legacy(legacy) {
        Ok(()) => Ok(MigrationOutcome::Migrated { projects, memories }),
        Err(error) => Ok(MigrationOutcome::ArchivePending {
            projects,
            memories,
            error,
        }),
    }
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

        // 第二次调用:legacy 已被改名为 .migrated,完成标记仍在,所以返回 AlreadyMigrated。
        let r2 = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(r2, MigrationOutcome::AlreadyMigrated);

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

        {
            let c = Connection::open(&target).unwrap();
            c.execute("UPDATE projects SET name = 'Other'", []).unwrap();
        }

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
        assert_eq!(names, vec!["Other".to_string()]);
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
    fn a_failed_migration_does_not_delete_the_target_file() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        // legacy 不是 SQLite 库:ATTACH 后读表失败。
        std::fs::write(&legacy, b"this is not a database at all, just text").unwrap();

        assert!(migrate_legacy_projects(&legacy, &target).is_err());
        // 失败的事务已经回滚;文件本身可能由共享 store 创建,不能凭旧 exists 删除。
        assert!(legacy.exists());
        if target.exists() {
            let c = Connection::open(&target).unwrap();
            let n: i64 = c
                .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "a failed migration leaves no committed rows");
        }
    }

    #[test]
    fn a_concurrent_writer_that_commits_after_the_exists_check_is_not_deleted() {
        use std::sync::{Arc, Barrier, mpsc};

        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        std::fs::write(&legacy, b"not a database").unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let (done_tx, done_rx) = mpsc::channel();
        let writer_target = target.clone();
        let writer_barrier = barrier.clone();
        let writer = std::thread::spawn(move || {
            // Wait until the migration has passed its pre-flight exists() check.
            writer_barrier.wait();
            let c = Connection::open(&writer_target).unwrap();
            c.execute_batch(
                "CREATE TABLE sentinel (value TEXT NOT NULL);
                 INSERT INTO sentinel VALUES ('committed by another writer');",
            )
            .unwrap();
            done_tx.send(()).unwrap();
        });

        let check_barrier = barrier.clone();
        let err = migrate_legacy_projects_with(&legacy, &target, move || {
            check_barrier.wait();
            done_rx.recv().unwrap(); // the other writer has committed
        })
        .unwrap_err();
        writer.join().unwrap();
        assert!(matches!(err.kind, ServiceErrorKind::Unavailable));

        let c = Connection::open(&target).unwrap();
        let value: String = c
            .query_row("SELECT value FROM sentinel", [], |r| r.get(0))
            .expect("the other writer's committed database must survive");
        assert_eq!(value, "committed by another writer");
        assert!(legacy.exists(), "a failed migration keeps the source");
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
    fn a_committed_migration_with_a_failed_archive_restarts_as_archive_only() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 2, 2);
        // 归档目标已存在:数据可以提交,但改名必须失败且不覆盖。
        std::fs::write(dir.path().join("projects.db.migrated"), b"keep me").unwrap();

        let first = migrate_legacy_projects(&legacy, &target).unwrap();
        match first {
            MigrationOutcome::ArchivePending {
                projects, memories, ..
            } => {
                assert_eq!((projects, memories), (2, 4));
            }
            other => panic!("expected ArchivePending, got {other:?}"),
        }
        assert!(legacy.exists(), "legacy stays for a later archive attempt");
        assert_eq!(
            std::fs::read(dir.path().join("projects.db.migrated")).unwrap(),
            b"keep me",
            "an existing archive is never overwritten"
        );

        // 重启前完成标记已经提交:第二次调用只做归档,不再 INSERT。
        std::fs::remove_file(dir.path().join("projects.db.migrated")).unwrap();
        let second = migrate_legacy_projects(&legacy, &target).unwrap();
        assert_eq!(
            second,
            MigrationOutcome::Migrated {
                projects: 2,
                memories: 4
            }
        );
        assert!(!legacy.exists());
        let c = Connection::open(&target).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "restart must not duplicate rows");
        let markers: i64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {MARKER_TABLE}"), [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(markers, 1);
    }

    #[test]
    fn an_old_half_completed_copy_is_marked_only_when_every_source_row_matches() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 3, 2);
        // 模拟旧版:已经复制了全部来源行,但没有完成标记,也没有归档。
        crate::ProjectHost::open(&target).unwrap();
        {
            let c = Connection::open(&target).unwrap();
            c.execute(
                "ATTACH DATABASE ?1 AS legacy",
                [legacy.to_string_lossy().as_ref()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO main.projects (id, path, name, last_active_ms, created_ms)
                 SELECT id, path, name, last_active_ms, created_ms FROM legacy.projects",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO main.memories (id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by)
                 SELECT id, project_id, title, kind, description, body, created_ms, created_by, updated_ms, updated_by FROM legacy.memories",
                [],
            )
            .unwrap();
        }

        let outcome = migrate_legacy_projects(&legacy, &target).unwrap();
        assert!(matches!(
            outcome,
            MigrationOutcome::Migrated {
                projects: 3,
                memories: 6
            }
        ));
        let c = Connection::open(&target).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3, "no duplicate insert during recovery");
        let markers: i64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {MARKER_TABLE}"), [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(markers, 1);
    }

    #[test]
    fn an_old_half_completed_copy_with_different_content_is_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 1, 1);
        crate::ProjectHost::open(&target).unwrap();
        {
            let c = Connection::open(&target).unwrap();
            c.execute(
                "ATTACH DATABASE ?1 AS legacy",
                [legacy.to_string_lossy().as_ref()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO main.projects (id, path, name, last_active_ms, created_ms)
                 SELECT id, path, 'different name', last_active_ms, created_ms FROM legacy.projects",
                [],
            )
            .unwrap();
        }

        let err = migrate_legacy_projects(&legacy, &target).unwrap_err();
        assert!(err.message.contains("projects"), "{err:?}");
        assert!(legacy.exists(), "a conflict never archives the source");
        let c = Connection::open(&target).unwrap();
        let name: String = c
            .query_row("SELECT name FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "different name", "the target is left untouched");
        let markers: i64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {MARKER_TABLE}"), [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(markers, 0, "a conflict does not write a success marker");
    }

    #[test]
    fn a_marker_without_the_legacy_file_reports_already_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let target = dir.path().join("digger.db");
        seed_legacy(&legacy, 1, 1);
        migrate_legacy_projects(&legacy, &target).unwrap();
        // 与重启等价:legacy 已被归档,标记仍在。
        assert_eq!(
            migrate_legacy_projects(&legacy, &target).unwrap(),
            MigrationOutcome::AlreadyMigrated
        );
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
    #[test]
    fn subprocess_project_migration() {
        let Ok(root) = std::env::var("DIGGER_TEST_PROJECT_CRASH_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        migrate_legacy_projects(&root.join("projects.db"), &root.join("digger.db")).unwrap();
        panic!("failpoint was not reached");
    }

    #[test]
    fn process_death_before_and_after_project_commit_recovers_without_duplicate_rows() {
        for point in ["projects_before_commit", "projects_after_commit"] {
            let root = std::env::temp_dir().join(format!(
                "digger-project-crash-{}-{point}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let legacy = root.join("projects.db");
            let target = root.join("digger.db");
            seed_legacy(&legacy, 2, 3);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "migrate::tests::subprocess_project_migration",
                    "--nocapture",
                ])
                .env("DIGGER_TEST_CRASH_AT", point)
                .env("DIGGER_TEST_PROJECT_CRASH_ROOT", &root)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(86));
            assert!(legacy.exists());
            let conn = Connection::open(&target).unwrap();
            let count: i64 = conn
                .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                count,
                if point.ends_with("before_commit") {
                    0
                } else {
                    2
                }
            );
            drop(conn);
            migrate_legacy_projects(&legacy, &target).unwrap();
            assert!(!legacy.exists());
            assert_eq!(
                migrate_legacy_projects(&legacy, &target).unwrap(),
                MigrationOutcome::AlreadyMigrated
            );
            let conn = Connection::open(&target).unwrap();
            assert_eq!(
                conn.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(
                conn.query_row("SELECT count(*) FROM memories", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                6
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn two_concurrent_migrations_commit_one_copy_and_keep_the_archive() {
        let root =
            std::env::temp_dir().join(format!("digger-migration-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("projects.db");
        let target = root.join("digger.db");
        seed_legacy(&source, 2, 3);
        let original = std::fs::read(&source).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let (source, target, barrier) = (source.clone(), target.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    migrate_legacy_projects(&source, &target)
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let conn = Connection::open(&target).unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM memories", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            std::fs::read(root.join("projects.db.migrated")).unwrap(),
            original
        );
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }
}
