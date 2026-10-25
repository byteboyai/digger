//! S3b-1 Task 6:待办面板对真实 `diggerd` 的端到端。
//!
//! 与 `digger-mcp/tests/memory_roundtrip.rs` 同型:在进程内拉起真实守护
//! 进程(独立临时数据根),各测试用不同项目隔离。`dozer_core::paths::install`
//! 每进程只能装一次,所以三个测试共用一个 daemon;重启测试用自己的产品实例
//! 关闭再拉起,因此测试之间用一把互斥锁串行。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use digger_desktop::adapters::todo::TodoPanel;
use dozer_client::Client;
use dozer_core::paths::{ProductDirs, ProductId};
use dozer_panel_todo::TodoWebviewEvent;

struct Daemon {
    root: PathBuf,
    socket: PathBuf,
    product: ProductId,
}

static DAEMON: tokio::sync::OnceCell<Daemon> = tokio::sync::OnceCell::const_new();
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RUNNING: std::sync::OnceLock<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>> =
    std::sync::OnceLock::new();
static DAEMON_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn running() -> &'static tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>> {
    RUNNING.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn daemon_rt() -> &'static tokio::runtime::Runtime {
    DAEMON_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("daemon runtime")
    })
}

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
            Daemon {
                root,
                socket,
                product,
            }
        })
        .await
}

async fn start(d: &Daemon) {
    let config = dozerd::daemon::DaemonConfig {
        product: d.product.clone(),
        socket: d.socket.clone(),
        features: dozerd::daemon::DaemonFeatures::minimal(),
    };
    let handle = daemon_rt().spawn(async move {
        let _ = dozerd::daemon::run(config).await;
    });
    *running().lock().await = Some(handle);
    for _ in 0..300 {
        if d.socket.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("diggerd socket never appeared at {}", d.socket.display());
}

async fn ensure_started(d: &Daemon) {
    if running().lock().await.is_none() {
        start(d).await;
    }
}

async fn stop(d: &Daemon) {
    let client = Client::new(d.socket.clone());
    let _ = client.shutdown_daemon().await;
    if let Some(handle) = running().lock().await.take() {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }
    let _ = std::fs::remove_file(&d.socket);
}

async fn project(d: &Daemon, name: &str) -> i64 {
    let path = d.root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    Client::new(d.socket.clone())
        .open_project(&path.to_string_lossy())
        .await
        .expect("open project")
        .expect("path is a directory")
        .id
}

fn make_panel(
    socket: &Path,
) -> (
    TodoPanel,
    tokio::sync::mpsc::UnboundedReceiver<dozer_panel_todo::Message>,
) {
    let client = Client::new(socket.to_path_buf());
    let mut panel = TodoPanel::new(Arc::new(client.clone()), client);
    panel.attach_handle(tokio::runtime::Handle::current());
    let rx = panel.take_receiver().expect("fresh receiver");
    (panel, rx)
}

async fn pump_until(
    panel: &mut TodoPanel,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<dozer_panel_todo::Message>,
    project_id: i64,
    needle: &str,
) {
    for _ in 0..100 {
        while let Ok(message) = rx.try_recv() {
            panel.update(message);
        }
        if panel
            .state(project_id)
            .is_some_and(|s| s.items().iter().any(|t| t.text == needle))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "never saw {needle:?}; items={:?}",
        panel.state(project_id).map(|s| s.items())
    );
}

#[tokio::test]
async fn panel_round_trips_against_a_real_diggerd() {
    let _guard = LOCK.lock().await;
    let d = daemon().await;
    ensure_started(d).await;
    let pid = project(d, "roundtrip").await;
    let (mut panel, mut rx) = make_panel(&d.socket);
    panel.set_project(Some(pid));
    panel.activate(Some(d.root.clone()));
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        panel
            .handle_webview_event(TodoWebviewEvent::Add {
                text: "买牛奶".into()
            })
            .is_empty()
    );
    pump_until(&mut panel, &mut rx, pid, "买牛奶").await;
    assert!(
        panel
            .state(pid)
            .unwrap()
            .items()
            .iter()
            .any(|t| t.text == "买牛奶"),
        "panel state sees the new task"
    );
    let direct = Client::new(d.socket.clone())
        .list_todos(pid)
        .await
        .expect("直接读库");
    assert!(
        direct.iter().any(|t| t.text == "买牛奶"),
        "the task really landed in digger.db, not only in memory"
    );
}

#[tokio::test]
async fn todos_of_two_projects_do_not_mix() {
    let _guard = LOCK.lock().await;
    let d = daemon().await;
    ensure_started(d).await;
    let a = project(d, "proj-a").await;
    let b = project(d, "proj-b").await;
    let (mut panel, mut rx) = make_panel(&d.socket);

    panel.set_project(Some(a));
    panel.activate(Some(d.root.join("proj-a")));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        panel
            .handle_webview_event(TodoWebviewEvent::Add {
                text: "A 的任务".into()
            })
            .is_empty()
    );
    pump_until(&mut panel, &mut rx, a, "A 的任务").await;

    panel.set_project(Some(b));
    panel.activate(Some(d.root.join("proj-b")));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        panel
            .handle_webview_event(TodoWebviewEvent::Add {
                text: "B 的任务".into()
            })
            .is_empty()
    );
    pump_until(&mut panel, &mut rx, b, "B 的任务").await;

    assert!(
        panel
            .state(a)
            .unwrap()
            .items()
            .iter()
            .all(|t| t.text != "B 的任务")
    );
    assert!(
        panel
            .state(b)
            .unwrap()
            .items()
            .iter()
            .all(|t| t.text != "A 的任务")
    );
}

#[tokio::test]
async fn daemon_restart_keeps_the_todos() {
    let _guard = LOCK.lock().await;
    let d = daemon().await;
    ensure_started(d).await;
    let pid = project(d, "restart").await;
    let (mut panel, mut rx) = make_panel(&d.socket);
    panel.set_project(Some(pid));
    panel.activate(Some(d.root.join("restart")));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        panel
            .handle_webview_event(TodoWebviewEvent::Add {
                text: "重启后还在".into()
            })
            .is_empty()
    );
    pump_until(&mut panel, &mut rx, pid, "重启后还在").await;

    stop(d).await;
    start(d).await;

    let (mut again, mut rx2) = make_panel(&d.socket);
    again.set_project(Some(pid));
    again.activate(Some(d.root.join("restart")));
    pump_until(&mut again, &mut rx2, pid, "重启后还在").await;
}
