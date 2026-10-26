//! 四个待办 MCP 工具对真实 `diggerd` 的往返,以及拒绝空白文字、拒绝跨项目 id 的测试。

use std::path::{Path, PathBuf};
use std::time::Duration;

use dozer_client::Client;
use dozer_core::paths::{ProductDirs, ProductId};
use dozer_core::protocol::AgentKind;
use dozer_mcp::McpFeatures;
use dozer_mcp::server::{
    AddTodoParams, DozerMcpServer, EditTodoTextParams, NoParams, ToggleTodoParams,
};
use rmcp::handler::server::wrapper::Parameters;

struct Daemon {
    socket: PathBuf,
    db: PathBuf,
}

static DAEMON: tokio::sync::OnceCell<Daemon> = tokio::sync::OnceCell::const_new();

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

fn features() -> McpFeatures {
    McpFeatures {
        memory: true,
        summary: false,
        todos: true,
        preview: false,
        file_edit: false,
    }
}

async fn project(d: &Daemon, name: &str) -> dozer_core::protocol::ProjectInfo {
    let path = d.db.parent().unwrap().join(format!("todo-{name}"));
    std::fs::create_dir_all(&path).unwrap();
    Client::new(d.socket.clone())
        .open_project(&path.to_string_lossy())
        .await
        .expect("open project")
        .expect("path is a directory")
}

async fn session(sock: &Path, project_id: i64) -> String {
    Client::new(sock.to_path_buf())
        .create_for_agent(
            "todo-mcp-test",
            "/bin/sh",
            &["-c".into(), "cat".into()],
            "/tmp",
            80,
            24,
            project_id,
            AgentKind::Claude,
        )
        .await
        .expect("create session")
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

fn todos(result: rmcp::model::CallToolResult) -> Vec<serde_json::Value> {
    result.structured_content.expect("structured")["todos"]
        .as_array()
        .expect("todos array")
        .clone()
}

async fn list(s: &DozerMcpServer) -> Vec<serde_json::Value> {
    todos(
        s.list_todos(Parameters(NoParams {}))
            .await
            .expect("list_todos"),
    )
}

#[tokio::test]
async fn add_then_list_then_toggle_then_edit() {
    let daemon = start_daemon().await;
    let project = project(daemon, "roundtrip").await;
    let s = server(&daemon.socket, session(&daemon.socket, project.id).await).await;

    let added = s
        .add_todo(Parameters(AddTodoParams {
            text: "买牛奶".into(),
        }))
        .await
        .expect("add_todo");
    let added = added.structured_content.expect("structured");
    let id = added["id"].as_i64().expect("id");
    assert_eq!(added["text"], "买牛奶");
    assert_eq!(added["done"], false);

    let items = list(&s).await;
    let item = items
        .iter()
        .find(|t| t["id"].as_i64() == Some(id))
        .expect("added task in list");
    assert_eq!(item["done"], false);

    s.toggle_todo(Parameters(ToggleTodoParams { id, done: true }))
        .await
        .expect("toggle_todo");
    let items = list(&s).await;
    let item = items
        .iter()
        .find(|t| t["id"].as_i64() == Some(id))
        .expect("toggled task");
    assert_eq!(item["done"], true);

    s.edit_todo_text(Parameters(EditTodoTextParams {
        id,
        text: "买燕麦奶".into(),
    }))
    .await
    .expect("edit_todo_text");
    let items = list(&s).await;
    assert!(
        items
            .iter()
            .any(|t| t["id"].as_i64() == Some(id) && t["text"] == "买燕麦奶")
    );

    // 独立 Client 直接读库,证明不是只改了内存。
    let direct = Client::new(daemon.socket.clone())
        .list_todos(project.id)
        .await
        .expect("direct list");
    assert!(
        direct
            .iter()
            .any(|t| t.id == id && t.text == "买燕麦奶" && t.done)
    );
}

#[tokio::test]
async fn blank_todo_text_is_rejected_and_creates_no_task() {
    // dozer-mcp(libs-2026-10-26a 起)在转给 Client 之前拒绝空白文字。
    let daemon = start_daemon().await;
    let project = project(daemon, "blank").await;
    let s = server(&daemon.socket, session(&daemon.socket, project.id).await).await;
    for text in ["", "   ", "\n\t"] {
        let err = s
            .add_todo(Parameters(AddTodoParams { text: text.into() }))
            .await
            .expect_err("blank text must be rejected");
        assert!(
            err.message.contains("不能为空"),
            "{text:?}: {}",
            err.message
        );
    }
    assert!(list(&s).await.is_empty());
}

#[tokio::test]
async fn a_session_cannot_toggle_or_edit_another_projects_todo() {
    // toggle/edit 只允许本会话所在项目里的任务 id。
    let daemon = start_daemon().await;
    let a = project(daemon, "scope-a").await;
    let b = project(daemon, "scope-b").await;
    let s = server(&daemon.socket, session(&daemon.socket, a.id).await).await;
    let client = Client::new(daemon.socket.clone());
    let foreign = client
        .add_todo(b.id, "B 的任务")
        .await
        .expect("add to B directly");
    let err = s
        .toggle_todo(Parameters(ToggleTodoParams {
            id: foreign.id,
            done: true,
        }))
        .await
        .expect_err("toggling B's task from A must fail");
    assert!(err.message.contains("不属于当前项目"), "{}", err.message);
    s.edit_todo_text(Parameters(EditTodoTextParams {
        id: foreign.id,
        text: "改写".into(),
    }))
    .await
    .expect_err("editing B's task from A must fail");
    let after = client
        .list_todos(b.id)
        .await
        .expect("read B")
        .into_iter()
        .find(|t| t.id == foreign.id)
        .expect("B task");
    assert!(!after.done);
    assert_eq!(after.text, "B 的任务");
}

/// Review Focus 2:会话没有归属项目时,工具返回可读错误而不是 panic / 误写。
/// 真实 daemon 创建会话时必须带 project_id,不能造出 None;用一个只应答
/// Hello/ListSessions 的本地假服务端直接喂 `SessionInfo.project_id = None`。
#[tokio::test]
async fn a_session_outside_any_registered_project_gets_a_readable_error() {
    use dozer_core::protocol::{Reply, Request, SessionInfo, decode_line, encode_line};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let path = PathBuf::from(format!(
        "/tmp/digger-todo-orphan-{}-{:?}.sock",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let mut lines = tokio::io::BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let reply = match decode_line::<Request>(&line) {
                        // 回 Error 模拟旧守护进程:Client 的兼容策略会放行,
                        // 不必在这里读 `paths::product()`(那会抢先初始化产品身份,
                        // 与同二进制里真实 daemon 的 install 冲突)。
                        Ok(Request::Hello { .. }) => Reply::Error {
                            message: "old daemon".into(),
                        },
                        Ok(Request::ListSessions) => Reply::Sessions {
                            sessions: vec![SessionInfo {
                                id: "orphan".into(),
                                name: "orphan".into(),
                                command: "/bin/sh".into(),
                                cwd: "/tmp".into(),
                                alive: true,
                                created_ms: 0,
                                agent_state: Default::default(),
                                transcript_path: None,
                                project_id: None,
                                agent: AgentKind::Claude,
                            }],
                        },
                        _ => Reply::Error {
                            message: "unexpected".into(),
                        },
                    };
                    let _ = w.write_all(encode_line(&reply).as_bytes()).await;
                }
            });
        }
    });

    let s = DozerMcpServer::with_options(
        Client::new(path.clone()),
        Some("orphan".into()),
        features(),
        "digger-mcp".into(),
    );
    let err = s
        .add_todo(Parameters(AddTodoParams {
            text: "不该写入".into(),
        }))
        .await
        .expect_err("orphan session must fail");
    let text = format!("{err}");
    assert!(!text.is_empty());
    assert!(text.contains("会话尚未归属任何项目"), "{text}");
    assert!(!text.contains("panic"), "{text}");
    let _ = std::fs::remove_file(path);
}
