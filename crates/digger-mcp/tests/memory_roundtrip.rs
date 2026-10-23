//! 三个记忆工具的端到端:真实 `diggerd`(进程内)+ 真实 SQLite。

use dozer_client::Client;
use dozer_core::paths::{ProductDirs, ProductId};
use dozer_core::protocol::AgentKind;
use dozer_mcp::McpFeatures;
use dozer_mcp::server::{DozerMcpServer, GetMemoryParams, NoParams, WriteMemoryParams};
use rmcp::handler::server::wrapper::Parameters;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn features() -> McpFeatures {
    McpFeatures {
        memory: true,
        summary: false,
        todos: false,
        preview: false,
        file_edit: false,
    }
}

struct Daemon {
    socket: PathBuf,
    db: PathBuf,
}

static DAEMON: tokio::sync::OnceCell<Daemon> = tokio::sync::OnceCell::const_new();

/// 全进程只起一个真实 `diggerd`:`dozer_core::paths::install` 每进程只能装一次
/// 产品身份,多个测试各起一个会被它拒绝。各测试用不同的 `project_id` 隔离数据。
async fn start_daemon() -> &'static Daemon {
    DAEMON
        .get_or_init(|| async {
            let root = tempfile::tempdir().unwrap().keep();
            let config = root.join("config");
            let state = root.join("state");
            std::fs::create_dir_all(&config).unwrap();
            std::fs::create_dir_all(&state).unwrap();
            let product = ProductId::custom(
                "digger",
                "DIGGER_SESSION_ID",
                "digger-hook",
                "diggerd.sock",
                "digger.db",
                ProductDirs::Fixed {
                    config: config.clone(),
                    state: state.clone(),
                },
            );
            let sock = root.join("diggerd.sock");
            dozer_core::paths::install(product.clone()).expect("install digger product");
            let daemon_config = dozerd::daemon::DaemonConfig {
                product: product.clone(),
                socket: sock.clone(),
                features: dozerd::daemon::DaemonFeatures::minimal(),
            };
            // 守护进程必须活得比每个 `#[tokio::test]` 自己的运行时更久:
            // 泄漏一个独立的多线程运行时来承载它。
            let daemon_rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            daemon_rt.spawn(async move {
                let _ = dozerd::daemon::run(daemon_config).await;
            });
            std::mem::forget(daemon_rt);
            for _ in 0..200 {
                if sock.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Daemon {
                socket: sock,
                db: state.join("digger.db"),
            }
        })
        .await
}

async fn session(sock: &Path, project_id: i64, agent: AgentKind) -> String {
    Client::new(sock.to_path_buf())
        .create_for_agent(
            "mcp-test",
            "/bin/sh",
            &["-c".into(), "cat".into()],
            "/tmp",
            80,
            24,
            project_id,
            agent,
        )
        .await
        .expect("建会话")
        .id
}

async fn server(sock: &Path, session_id: String) -> DozerMcpServer {
    DozerMcpServer::with_options(
        Client::new(sock.to_path_buf()),
        Some(session_id),
        features(),
        "digger-mcp".into(),
    )
}

#[tokio::test]
async fn write_then_list_then_get_roundtrip() {
    let daemon = start_daemon().await;
    let s = server(
        &daemon.socket,
        session(&daemon.socket, 1, AgentKind::Claude).await,
    )
    .await;
    let written = s
        .write_memory(Parameters(WriteMemoryParams {
            title: "用户偏好".into(),
            body: "喜欢简短回复".into(),
            kind: "user".into(),
            description: "沟通风格".into(),
        }))
        .await
        .expect("write_memory");
    assert!(written.structured_content.is_some());

    let list = s
        .list_memories(Parameters(NoParams {}))
        .await
        .expect("list_memories");
    let json = list.structured_content.expect("structured");
    assert_eq!(json["memories"].as_array().unwrap().len(), 1);
    assert_eq!(json["memories"][0]["title"], "用户偏好");

    let got = s
        .get_memory(Parameters(GetMemoryParams {
            title_or_id: "用户偏好".into(),
        }))
        .await
        .expect("get_memory");
    assert_eq!(got.structured_content.unwrap()["body"], "喜欢简短回复");
}

#[tokio::test]
async fn write_with_the_same_title_updates_instead_of_duplicating() {
    let daemon = start_daemon().await;
    let s = server(
        &daemon.socket,
        session(&daemon.socket, 2, AgentKind::Claude).await,
    )
    .await;
    for body in ["v1", "v2"] {
        s.write_memory(Parameters(WriteMemoryParams {
            title: "同名记忆".into(),
            body: body.into(),
            kind: "project".into(),
            description: "".into(),
        }))
        .await
        .expect("write_memory");
    }
    let list = s
        .list_memories(Parameters(NoParams {}))
        .await
        .expect("list_memories");
    assert_eq!(
        list.structured_content.unwrap()["memories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn session_without_a_project_gets_a_readable_error() {
    let daemon = start_daemon().await;
    let s = server(&daemon.socket, "does-not-exist".into()).await;
    let err = s
        .list_memories(Parameters(NoParams {}))
        .await
        .expect_err("不存在的会话应报错");
    assert!(format!("{err}").contains("不存在于"), "{err}");
}

#[tokio::test]
async fn memory_written_through_mcp_is_visible_through_the_project_store() {
    let daemon = start_daemon().await;
    let s = server(
        &daemon.socket,
        session(&daemon.socket, 4, AgentKind::Claude).await,
    )
    .await;
    s.write_memory(Parameters(WriteMemoryParams {
        title: "面板可见".into(),
        body: "正文".into(),
        kind: "project".into(),
        description: "".into(),
    }))
    .await
    .expect("write_memory");
    let host = digger_store::ProjectHost::open(&daemon.db).expect("open project db");
    let memories = host.memory.list_memories(4).await.expect("list via store");
    assert!(memories.iter().any(|m| m.title == "面板可见"));
}

#[tokio::test]
async fn updated_by_is_the_sessions_agent_label() {
    let daemon = start_daemon().await;
    let s = server(
        &daemon.socket,
        session(&daemon.socket, 5, AgentKind::Claude).await,
    )
    .await;
    let written = s
        .write_memory(Parameters(WriteMemoryParams {
            title: "归属".into(),
            body: "x".into(),
            kind: "project".into(),
            description: "".into(),
        }))
        .await
        .expect("write_memory");
    assert_eq!(
        written.structured_content.unwrap()["updated_by"],
        AgentKind::Claude.label()
    );
}

#[tokio::test]
async fn empty_title_and_oversized_body_are_handled_like_dozer() {
    let daemon = start_daemon().await;
    let s = server(
        &daemon.socket,
        session(&daemon.socket, 6, AgentKind::Claude).await,
    )
    .await;
    // 与 Dozer 的 memory 工具同一份实现:空标题不 panic,超长正文可写入。
    let _ = s
        .write_memory(Parameters(WriteMemoryParams {
            title: "".into(),
            body: "b".into(),
            kind: "project".into(),
            description: "".into(),
        }))
        .await;
    let oversized = "长".repeat(200_000);
    s.write_memory(Parameters(WriteMemoryParams {
        title: "超长".into(),
        body: oversized.clone(),
        kind: "project".into(),
        description: "".into(),
    }))
    .await
    .expect("超长正文应可写入");
    let got = s
        .get_memory(Parameters(GetMemoryParams {
            title_or_id: "超长".into(),
        }))
        .await
        .expect("get_memory");
    assert_eq!(got.structured_content.unwrap()["body"], oversized);
}
