//! Project 注册表与 Project 级共享记忆的进程内装配。
//!
//! 实现来自 Dozer 与 Digger 共用的 `dozer-store-project`;这里只决定 Digger 把它放在哪个数据库文件里。
//! 两个存储用同一个 SQLite 文件(各持一个连接,表互不相交),与 `dozerd` 把所有 store 放进 `dozer.db` 同型。
//! 文件属于 Digger 自己的数据根,与 Dozer 的数据库没有任何关系。

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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn projects_and_memories_survive_reopening_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projects.db");
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
        let e = ProjectHost::open(&blocker.join("projects.db"))
            .err()
            .expect("must fail");
        assert_eq!(e.kind, ServiceErrorKind::Unavailable);
    }
}
