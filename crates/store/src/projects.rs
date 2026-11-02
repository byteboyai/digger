//! Project 注册表与 Project 级共享记忆的进程内装配。
//!
//! 实现来自 Dozer 与 Digger 共用的 `dozer-store-project`;这里只决定 Digger 把它放在哪个数据库文件里。
//! 两个存储用同一个 SQLite 文件(各持一个连接,表互不相交),与 `dozerd` 把所有 store 放进 `digger.db` 同型。
//! 文件属于 Digger 自己的数据根,与 Dozer 的数据库没有任何关系。
//!
//! S6a:`open_in` 在打开 store 之前先迁移旧 `projects.db`(若存在),然后把 `digger.db`
//! 切到 WAL 模式(尽力而为,失败只记日志不阻断)。

use crate::migrate::{migrate_legacy_projects, set_wal_mode};
use dozer_services::{MemoryService, ProjectService, ServiceError, ServiceErrorKind};
use dozer_store_project::{LocalMemoryService, LocalProjectService, MemoryStore, ProjectStore};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProjectHost {
    pub projects: Arc<dyn ProjectService>,
    pub memory: Arc<dyn MemoryService>,
}

/// `anyhow::Error` 的 `{:#}` 带出整条原因链;用泛型 `Display` 接,Digger 不必直接依赖 anyhow。
fn unavailable<E: std::fmt::Display>(e: E) -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, format!("{e:#}"))
}

impl ProjectHost {
    /// 打开(必要时创建)`db_path`。打不开是 `Unavailable`,调用方据此显示"项目功能不可用",不静默退回临时库。
    pub fn open(db_path: &Path) -> Result<Self, ServiceError> {
        let projects = ProjectStore::new(db_path).map_err(unavailable)?;
        let memories = MemoryStore::new(db_path).map_err(unavailable)?;
        Ok(Self {
            projects: Arc::new(LocalProjectService::new(Arc::new(projects))),
            memory: Arc::new(LocalMemoryService::new(Arc::new(memories))),
        })
    }

    /// 在 `db_path` 里打开注册表,先迁移旧 `legacy_db`(若存在)到 `db_path`,再设 WAL。
    ///
    /// 迁移失败时返回 `Unavailable`,调用方显示"项目功能不可用:迁移失败 …",不静默回退。
    /// WAL 设置失败只记日志、不算迁移失败(尽力而为)。
    pub fn open_in(db_path: &Path, legacy_db: &Path) -> Result<Self, ServiceError> {
        Self::open_in_with_outcome(db_path, legacy_db).map(|(host, _)| host)
    }

    /// 同 [`ProjectHost::open_in`],但把迁移结果一起交给调用方展示(归档待完成仍算成功)。
    pub fn open_in_with_outcome(
        db_path: &Path,
        legacy_db: &Path,
    ) -> Result<(Self, crate::MigrationOutcome), ServiceError> {
        let outcome = migrate_legacy_projects(legacy_db, db_path)?;

        // 先于任何 store 连接设 WAL(切换需要独占);库文件还不存在时由它建出空库。
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = set_wal_mode(db_path) {
            eprintln!("digger-store: 设 WAL 模式失败(已忽略): {e}");
        }

        Ok((Self::open(db_path)?, outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn projects_and_memories_survive_reopening_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("digger.db");
        let proj = dir.path().join("book");
        std::fs::create_dir(&proj).unwrap();
        let id = {
            let host = ProjectHost::open(&db).unwrap();
            let info = block_on(
                host.projects
                    .open_project(proj.to_string_lossy().into_owned()),
            )
            .unwrap()
            .unwrap();
            block_on(host.memory.write_memory(
                info.id,
                "风格".into(),
                "feedback".into(),
                "".into(),
                "短句".into(),
                "user".into(),
            ))
            .unwrap();
            info.id
        };
        let host = ProjectHost::open(&db).unwrap();
        let listed = block_on(host.projects.list_projects()).unwrap();
        assert_eq!(listed.iter().map(|p| p.id).collect::<Vec<_>>(), vec![id]);
        assert_eq!(block_on(host.memory.list_memories(id)).unwrap().len(), 1);
    }

    #[test]
    fn an_unopenable_database_is_unavailable_not_an_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        // 把"文件"当目录用,建库目录必然失败。
        let e = ProjectHost::open(&blocker.join("digger.db"))
            .err()
            .expect("must fail");
        assert_eq!(e.kind, ServiceErrorKind::Unavailable);
    }

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
    fn open_in_migrates_legacy_and_sets_wal() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("projects.db");
        let db = dir.path().join("digger.db");

        // 建旧库并插数据。
        seed_legacy(&legacy, 2, 1);

        let host = ProjectHost::open_in(&db, &legacy).unwrap();

        // 验证数据搬过来了。
        let listed = block_on(host.projects.list_projects()).unwrap();
        assert_eq!(listed.len(), 2);

        // 验证 WAL 模式。
        let mode = crate::journal_mode(&db).unwrap();
        assert_eq!(mode, "wal");

        // 验证旧文件被改名。
        assert!(!legacy.exists());
        assert!(dir.path().join("projects.db.migrated").exists());
    }

    /// S6a Task 8 Step 2 的并发测试:两个线程各持一个 ProjectStore 连接,
    /// 打开同一文件,各做多次交错的写入,最后总行数等于两边写入之和。
    ///
    /// 连接的 busy_timeout 由 rusqlite 默认 5 秒提供;WAL 由 open_in 先设。
    /// 若在这套配置下仍出现 `database is locked`,停下报告,不要自己改 Dozer。
    #[test]
    fn two_writers_on_one_digger_db_never_lose_writes_or_hit_locked() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("digger.db");

        // 创建测试用的项目目录。
        let proj_a_dir = dir.path().join("proj_a");
        let proj_b_dir = dir.path().join("proj_b");
        std::fs::create_dir_all(&proj_a_dir).unwrap();
        std::fs::create_dir_all(&proj_b_dir).unwrap();

        // 先经 open_in 建好 schema 并设 WAL(避免并发建表的 race)。
        let legacy = dir.path().join("projects.db");
        {
            let host = ProjectHost::open_in(&db, &legacy).unwrap();
            // 先写一个 project 验证基本功能。
            let result = block_on(
                host.projects
                    .open_project(proj_a_dir.to_string_lossy().into_owned()),
            );
            assert!(result.is_ok(), "initial write failed: {:?}", result.err());
            assert!(result.unwrap().is_some(), "should create project");
            let projects = block_on(host.projects.list_projects()).unwrap();
            assert_eq!(
                projects.len(),
                1,
                "should have 1 project after initial write"
            );
        }
        assert_eq!(crate::journal_mode(&db).unwrap(), "wal");

        let iterations = 300;
        let db_clone = db.clone();

        // 线程 A:写 projects(不同的子目录)。
        let handle_a = std::thread::spawn({
            let db = db_clone.clone();
            let base = proj_a_dir.clone();
            move || -> Result<(), String> {
                let host = ProjectHost::open(&db).map_err(|e| e.to_string())?;
                for i in 0..iterations {
                    let subdir = base.join(format!("a_{i}"));
                    std::fs::create_dir_all(&subdir).map_err(|e| e.to_string())?;
                    block_on(
                        host.projects
                            .open_project(subdir.to_string_lossy().into_owned()),
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        });

        // 线程 B:也写 projects(不同的子目录)。
        let handle_b = std::thread::spawn({
            let db = db_clone.clone();
            let base = proj_b_dir.clone();
            move || -> Result<(), String> {
                let host = ProjectHost::open(&db).map_err(|e| e.to_string())?;
                for i in 0..iterations {
                    let subdir = base.join(format!("b_{i}"));
                    std::fs::create_dir_all(&subdir).map_err(|e| e.to_string())?;
                    block_on(
                        host.projects
                            .open_project(subdir.to_string_lossy().into_owned()),
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        });

        handle_a.join().unwrap().unwrap();
        handle_b.join().unwrap().unwrap();

        // 验证总行数。
        let host = ProjectHost::open(&db).unwrap();
        let projects = block_on(host.projects.list_projects()).unwrap();
        // 初始 1 个 + 线程 A iterations 个 + 线程 B iterations 个。
        assert_eq!(projects.len(), 1 + iterations * 2);
    }
}
