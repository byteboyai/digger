//! Agent 预览桥接(S6e-3a Task 5/6):把 Digger 的预览上下文推给 `diggerd`,
//! 并轮询/回报 Agent 发来的预览命令。
//!
//! 两条循环都只经 `dozer_client::Client` 走 UDS;共享库 `dozer-panel-preview`
//! 不依赖 client,这里也不复制库逻辑。失败只记日志、退避,不 panic。

use dozer_client::Client;
use dozer_model::{PreviewCommand, PreviewContext};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

/// 轮询的基准间隔与退避上限(Review Focus 3)。
const POLL_BASE_MS: u64 = 400;
const POLL_MAX_MS: u64 = 5_000;

/// 预览上下文推送 + 命令轮询的共享状态。
#[derive(Clone)]
pub struct PreviewBridge {
    client: Client,
    handle: tokio::runtime::Handle,
    /// 上下文推送的防抖代次(按项目):同一项目后一次调用让前一次放弃;
    /// 不同项目互不影响(切项目时旧项目还要推 `None`)。
    nonce: Arc<std::sync::Mutex<std::collections::HashMap<i64, u64>>>,
}

impl PreviewBridge {
    pub fn new(client: Client, handle: tokio::runtime::Handle) -> Self {
        Self {
            client,
            handle,
            nonce: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// 推送一次上下文。`delay_ms > 0` 时防抖:在等待期间若有更晚的调用,
    /// 这次就放弃。失败只记日志(Review Focus 3)。
    pub fn push(&self, project_id: i64, context: Option<PreviewContext>, delay_ms: u64) {
        let nonce = {
            let mut map = self
                .nonce
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let slot = map.entry(project_id).or_default();
            *slot += 1;
            *slot
        };
        let flag = Arc::clone(&self.nonce);
        let client = self.client.clone();
        self.handle.spawn(async move {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            let current = flag
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&project_id)
                .copied();
            if current != Some(nonce) {
                return; // 同项目有更晚的一次变化取代了这次
            }
            if let Err(error) = client.update_preview_context(project_id, context).await {
                eprintln!("digger: 推送预览上下文失败: {error}"); // cli-output
            }
        });
    }

    /// 启动命令轮询(幂等)。`project` 是当前项目 id(≤0 跳过);失败时退避,
    /// 连续失败翻倍、上限 5 秒,成功即复位。只在拿到命令时回调。
    pub fn spawn_poller(
        &self,
        project: Arc<AtomicI64>,
        started: Arc<AtomicBool>,
        proxy: impl Fn(i64, Vec<PreviewCommand>) + Send + 'static,
    ) {
        if started.swap(true, Ordering::SeqCst) {
            return;
        }
        let client = self.client.clone();
        self.handle.spawn(async move {
            let mut wait = POLL_BASE_MS;
            loop {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                let pid = project.load(Ordering::SeqCst);
                if pid <= 0 {
                    wait = POLL_BASE_MS;
                    continue;
                }
                match client.take_pending_preview_commands(pid).await {
                    Ok(commands) => {
                        wait = POLL_BASE_MS;
                        if !commands.is_empty() {
                            proxy(pid, commands);
                        }
                    }
                    Err(error) => {
                        wait = (wait * 2).min(POLL_MAX_MS);
                        eprintln!("digger: 轮询预览命令失败(下次 {wait}ms): {error}"); // cli-output
                    }
                }
            }
        });
    }

    /// 回报一条命令的终态(后台任务;失败只记日志)。
    pub fn report_outcome(&self, outcome: dozer_model::PreviewCommandOutcome) {
        let client = self.client.clone();
        self.handle.spawn(async move {
            if let Err(error) = client.report_preview_command_outcome(outcome).await {
                eprintln!("digger: 回报预览命令结果失败: {error}"); // cli-output
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_core::protocol::{PROTOCOL_VERSION, Reply, Request, decode_line, encode_line};
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Seen {
        contexts: Vec<Option<PreviewContext>>,
        pending_calls: usize,
        /// 前 n 次 TakePendingPreviewCommands 返回 Error。
        fail_first: usize,
        pending: Vec<PreviewCommand>,
    }

    fn sock(tag: &str) -> PathBuf {
        PathBuf::from(format!(
            "/tmp/prev-bridge-{tag}-{}.sock",
            std::process::id()
        ))
    }

    fn serve(path: &std::path::Path, seen: Arc<Mutex<Seen>>) {
        let _ = std::fs::remove_file(path);
        let listener = tokio::net::UnixListener::bind(path).unwrap();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = tokio::io::BufReader::new(r).lines();
                    use tokio::io::AsyncBufReadExt;
                    while let Ok(Some(line)) = lines.next_line().await {
                        let reply = match decode_line::<Request>(&line) {
                            Ok(Request::Hello { .. }) => Reply::Hello {
                                protocol: PROTOCOL_VERSION,
                                product: "digger".into(),
                            },
                            Ok(Request::UpdatePreviewContext { context, .. }) => {
                                seen.lock().unwrap().contexts.push(context);
                                Reply::Ok
                            }
                            Ok(Request::TakePendingPreviewCommands { .. }) => {
                                let mut s = seen.lock().unwrap();
                                s.pending_calls += 1;
                                if s.pending_calls <= s.fail_first {
                                    Reply::Error {
                                        message: "unavailable".into(),
                                    }
                                } else {
                                    Reply::PendingPreviewCommands {
                                        commands: std::mem::take(&mut s.pending),
                                    }
                                }
                            }
                            Ok(Request::ReportPreviewCommandOutcome { .. }) => Reply::Ok,
                            _ => Reply::Error {
                                message: "unexpected".into(),
                            },
                        };
                        use tokio::io::AsyncWriteExt;
                        let _ = w.write_all(encode_line(&reply).as_bytes()).await;
                    }
                });
            }
        });
    }

    fn client(path: &std::path::Path) -> Client {
        dozer_core::paths::install(digger_paths::product()).ok();
        Client::new(path.to_path_buf())
    }

    fn context(path: &str, line: u32) -> PreviewContext {
        PreviewContext {
            path: path.into(),
            start_line: line,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn push_debounces_so_only_the_last_context_is_sent() {
        let path = sock("debounce");
        let seen = Arc::new(Mutex::new(Seen::default()));
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        for i in 1..=5 {
            bridge.push(7, Some(context(&format!("/tmp/{i}.rs"), i)), 50);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let contexts = seen.lock().unwrap().contexts.clone();
        assert_eq!(contexts.len(), 1, "只应发送最后一次");
        assert_eq!(contexts[0].as_ref().unwrap().start_line, 5);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn an_immediate_push_supersedes_a_queued_debounced_one() {
        let path = sock("immediate");
        let seen = Arc::new(Mutex::new(Seen::default()));
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        bridge.push(7, Some(context("/tmp/slow.rs", 1)), 200);
        bridge.push(7, Some(context("/tmp/fast.rs", 2)), 0);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let contexts = seen.lock().unwrap().contexts.clone();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].as_ref().unwrap().path, "/tmp/fast.rs");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_push_for_another_project_does_not_supersede_the_first_project() {
        let path = sock("per-project");
        let seen = Arc::new(Mutex::new(Seen::default()));
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        // 切项目:旧项目推 None、新项目推真实上下文,两者都要送达。
        bridge.push(1, None, 100);
        bridge.push(2, Some(context("/tmp/new.rs", 3)), 0);
        tokio::time::sleep(Duration::from_millis(250)).await;
        let contexts = seen.lock().unwrap().contexts.clone();
        assert_eq!(contexts.len(), 2, "不同项目的推送互不取代: {contexts:?}");
        assert!(contexts.iter().any(Option::is_none));
        assert!(contexts.iter().flatten().any(|c| c.path == "/tmp/new.rs"));
        let _ = std::fs::remove_file(path);
    }

    /// Review Focus 4:关光文本页签时上下文是 `None`,且立即发送。
    #[tokio::test]
    async fn closing_the_last_text_tab_pushes_none_immediately() {
        let path = sock("close-none");
        let seen = Arc::new(Mutex::new(Seen::default()));
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        bridge.push(7, None, 0);
        tokio::time::sleep(Duration::from_millis(120)).await;
        let contexts = seen.lock().unwrap().contexts.clone();
        assert_eq!(contexts, vec![None]);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_failing_daemon_does_not_panic_and_a_later_push_still_works() {
        // 先指向不存在的 socket:失败只记日志。
        let missing = sock("missing");
        let bridge = PreviewBridge::new(client(&missing), tokio::runtime::Handle::current());
        bridge.push(7, Some(context("/tmp/x.rs", 1)), 0);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 再换一个真实服务端:同一个 bridge 的下一次推送要成功。
        let path = sock("recover");
        let seen = Arc::new(Mutex::new(Seen::default()));
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        bridge.push(7, Some(context("/tmp/y.rs", 2)), 0);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(seen.lock().unwrap().contexts.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn the_poller_backs_off_while_the_daemon_is_unreachable_and_recovers() {
        let path = sock("backoff");
        let seen = Arc::new(Mutex::new(Seen::default()));
        seen.lock().unwrap().fail_first = 2;
        seen.lock().unwrap().pending = vec![PreviewCommand {
            request_id: "r1".into(),
            project_id: 7,
            target: dozer_model::PreviewCommandTarget::Path {
                path: "/tmp/a.rs".into(),
            },
            action: dozer_model::PreviewCommandAction::Reveal { line: 1, column: 1 },
            expected_revision: None,
        }];
        serve(&path, seen.clone());
        let bridge = PreviewBridge::new(client(&path), tokio::runtime::Handle::current());
        let project = Arc::new(AtomicI64::new(7));
        let started = Arc::new(AtomicBool::new(false));
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        bridge.spawn_poller(project, started.clone(), move |pid, commands| {
            sink.lock().unwrap().push((pid, commands));
        });
        tokio::time::sleep(Duration::from_millis(3_400)).await;
        assert_eq!(got.lock().unwrap().len(), 1, "退避后恢复并拿到命令");
        assert!(seen.lock().unwrap().pending_calls >= 3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_poller_is_idempotent() {
        let started = Arc::new(AtomicBool::new(false));
        assert!(!started.swap(true, Ordering::SeqCst));
        assert!(started.swap(true, Ordering::SeqCst), "第二次 swap 应为真");
    }
}
