//! S3b-2 Task 2:agent 经 MCP 改的任务在一个轮询周期内出现在待办面板。
//!
//! 不真等 1 秒:测试自己把 `Instant` 推到 `POLL_INTERVAL` 之后调
//! `TodoPanel::poll`,再把回流消息喂 `update`。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use digger_desktop::adapters::todo::{POLL_INTERVAL, TodoPanel};
use dozer_client::Client;
use dozer_core::paths::{ProductDirs, ProductId};
use dozer_core::protocol::AgentKind;
use dozer_mcp::McpFeatures;
use dozer_mcp::server::{AddTodoParams, DozerMcpServer, ToggleTodoParams};
use rmcp::handler::server::wrapper::Parameters;

struct Daemon {
    root: PathBuf,
    socket: PathBuf,
}

static DAEMON: tokio::sync::OnceCell<Daemon> = tokio::sync::OnceCell::const_new();

async fn daemon() -> &'static Daemon {
    DAEMON
        .get_or_init(|| async {
            let root = tempfile::tempdir().unwrap().keep();
            std::fs::create_dir_all(root.join("config")).unwrap();
            std::fs::create_dir_all(root.join("state")).unwrap();
            let product = ProductId::custom(
                "digger",
                "DIGGER_SESSION_ID",
                "digger-hook",
                "diggerd.sock",
                "digger.db",
                ProductDirs::Fixed {
                    config: root.join("config"),
                    state: root.join("state"),
                },
            );
            let socket = root.join("diggerd.sock");
            dozer_core::paths::install(product.clone()).expect("install digger product");
            let config = dozerd::daemon::DaemonConfig {
                product,
                socket: socket.clone(),
                features: dozerd::daemon::DaemonFeatures::minimal(),
            };
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.spawn(async move {
                let _ = dozerd::daemon::run(config).await;
            });
            std::mem::forget(rt);
            for _ in 0..300 {
                if socket.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Daemon { root, socket }
        })
        .await
}

fn features() -> McpFeatures {
    McpFeatures {
        memory: false,
        summary: false,
        todos: true,
        preview: false,
        file_edit: false,
    }
}

async fn project(d: &Daemon) -> i64 {
    let path = d.root.join("project");
    std::fs::create_dir_all(&path).unwrap();
    Client::new(d.socket.clone())
        .open_project(&path.to_string_lossy())
        .await
        .expect("open project")
        .expect("directory")
        .id
}

async fn session(sock: &Path, project_id: i64) -> String {
    Client::new(sock.to_path_buf())
        .create_for_agent(
            "todo-mcp-visible",
            "/bin/sh",
            &["-c".into(), "cat".into()],
            "/tmp",
            80,
            24,
            project_id,
            AgentKind::Claude,
        )
        .await
        .expect("session")
        .id
}

async fn drain(
    panel: &mut TodoPanel,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<dozer_panel_todo::Message>,
) {
    tokio::time::sleep(Duration::from_millis(30)).await;
    while let Ok(message) = rx.try_recv() {
        panel.update(message);
    }
}

#[tokio::test]
async fn agent_added_todo_shows_up_within_one_poll_interval() {
    let d = daemon().await;
    let project_id = project(d).await;
    let session_id = session(&d.socket, project_id).await;
    let server = DozerMcpServer::with_options(
        Client::new(d.socket.clone()),
        Some(session_id),
        features(),
        "digger-mcp".into(),
    );

    let client = Client::new(d.socket.clone());
    let mut panel = TodoPanel::new(Arc::new(client.clone()), client);
    panel.attach_handle(tokio::runtime::Handle::current());
    let mut rx = panel.take_receiver().expect("receiver");
    panel.set_project(Some(project_id));
    panel.activate(Some(d.root.clone()));
    drain(&mut panel, &mut rx).await;

    let added = server
        .add_todo(Parameters(AddTodoParams {
            text: "由 agent 新增".into(),
        }))
        .await
        .expect("add_todo");
    let added = added.structured_content.expect("structured");
    let id = added["id"].as_i64().expect("id");

    let t1 = Instant::now() + POLL_INTERVAL;
    panel.poll(t1);
    for _ in 0..100 {
        drain(&mut panel, &mut rx).await;
        if panel
            .state(project_id)
            .is_some_and(|s| s.items().iter().any(|t| t.id == id))
        {
            break;
        }
    }
    assert!(
        panel
            .state(project_id)
            .is_some_and(|s| s.items().iter().any(|t| t.id == id)),
        "agent-added task must appear within one poll interval"
    );

    server
        .toggle_todo(Parameters(ToggleTodoParams { id, done: true }))
        .await
        .expect("toggle_todo");
    let t2 = t1 + POLL_INTERVAL;
    panel.poll(t2);
    for _ in 0..100 {
        drain(&mut panel, &mut rx).await;
        if panel
            .state(project_id)
            .is_some_and(|s| s.items().iter().any(|t| t.id == id && t.done))
        {
            break;
        }
    }
    assert!(
        panel
            .state(project_id)
            .is_some_and(|s| s.items().iter().any(|t| t.id == id && t.done)),
        "agent toggled task must become done within one poll interval"
    );

    // 用独立 Client 再确认落库(测试主要看面板,这里顺手防假绿)。
    let direct = Client::new(d.socket.clone())
        .list_todos(project_id)
        .await
        .expect("direct list");
    assert!(direct.iter().any(|t| t.id == id && t.done));
}
