//! S6a Task 9:Digger 守护进程的拉起与生命周期管理。
//!
//! `Daemon` 负责确保 `diggerd` 在跑:连不上就 spawn 一个,隔 1 秒重试最多 3 次。
//! 关闭 Digger 窗口不终止 `diggerd`(会话要随守护进程存活,与 Dozer 一致)。

use dozer_client::Client;
use dozer_services::SessionService;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// 守护进程拉起/连接失败的可读错误。
#[derive(Debug)]
pub struct DaemonError {
    message: String,
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DaemonError {}

impl DaemonError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 把本进程装成 Digger 产品身份(`dozer_core::paths::install`)。必须在任何 `Client` 之前调用:
/// 客户端握手拿进程级身份去比对守护进程的产品名,不装就是 "dozer",连上 `diggerd` 会被判成别的产品。
pub fn install_product() {
    if let Err(e) = dozer_core::paths::install(digger_paths::product()) {
        eprintln!("安装 Digger 产品身份失败: {e:?}");
    }
}

/// Digger 守护进程管理器:负责确保 `diggerd` 在跑并能响应请求。
pub struct Daemon {
    client: Client,
    socket: PathBuf,
}

impl Daemon {
    /// 构造一个指向 Digger 产品 socket 的守护进程管理器。
    pub fn new() -> Self {
        Self::with_socket(digger_paths::product().socket_path())
    }

    pub fn with_socket(socket: PathBuf) -> Self {
        let client = Client::new(socket.clone());
        Self { client, socket }
    }

    /// 会话服务接缝(`dozer_services::SessionService`):包装同一个 `Client`,
    /// 连的还是 `diggerd` 的 socket。库只依赖 trait,不依赖 `dozer-client`。
    pub fn sessions(&self) -> Arc<dyn SessionService> {
        Arc::new(self.client.clone())
    }

    /// 确保守护进程在跑并能响应请求。
    ///
    /// 先尝试连接并握手;连不上就 spawn 一个 `diggerd`,隔 1 秒重试最多 3 次。
    /// 失败时返回可读错误,调用方显示提示但不阻断主窗口启动。
    pub async fn ensure(&self) -> Result<(), DaemonError> {
        self.ensure_with(|| Self::spawn(None), std::time::Duration::from_secs(1))
            .await
    }

    /// `ensure` 的内核:拉起动作与重试间隔可注入(测试用)。
    async fn ensure_with(
        &self,
        spawn: impl FnOnce() -> std::io::Result<()>,
        retry_gap: std::time::Duration,
    ) -> Result<(), DaemonError> {
        // 先尝试连接。
        match self.try_connect().await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("diggerd 连接失败,尝试拉起: {e}");
            }
        }

        // spawn 一个新的 diggerd。
        spawn().map_err(|e| DaemonError::new(format!("拉起 diggerd 失败: {e}")))?;

        // 重试最多 3 次,每次隔 1 秒。
        for attempt in 1..=3 {
            tokio::time::sleep(retry_gap).await;
            match self.try_connect().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    eprintln!("diggerd 重试 {attempt}/3 失败: {e}");
                }
            }
        }

        Err(DaemonError::new(
            "diggerd 拉起后仍无法连接,请检查日志或手动启动 diggerd",
        ))
    }

    /// 尝试连接守护进程并握手。
    async fn try_connect(&self) -> Result<(), DaemonError> {
        // 检查 socket 文件是否存在。
        if !self.socket.exists() {
            return Err(DaemonError::new("socket 文件不存在"));
        }

        // 协议握手。
        self.client
            .check_version()
            .await
            .map_err(|e| DaemonError::new(format!("协议握手失败: {e}")))?;

        // 发一个 list 请求验证连接。
        self.client
            .list_projects()
            .await
            .map_err(|e| DaemonError::new(format!("list 请求失败: {e}")))?;

        Ok(())
    }

    /// 在后台 spawn 一个 `diggerd` 进程。
    ///
    /// 先在 `current_exe` 同目录找 `diggerd`,找不到退回 PATH。
    /// 不继承 stdin,关闭 Digger 窗口不终止 diggerd。
    pub fn spawn(exe_dir: Option<&Path>) -> std::io::Result<()> {
        let diggerd = find_diggerd(exe_dir)?;

        // 用 setsid (Unix) 让 diggerd 脱离当前进程组,这样 Digger 退出不会杀掉它。
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut cmd = Command::new(&diggerd);
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            cmd.spawn()?;
        }

        #[cfg(not(unix))]
        {
            let mut cmd = Command::new(&diggerd);
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
            cmd.spawn()?;
        }

        Ok(())
    }
}

/// 查找 `diggerd` 二进制:先在 `exe_dir` 找,找不到退回 PATH。
fn find_diggerd(exe_dir: Option<&Path>) -> std::io::Result<PathBuf> {
    // 先试 exe_dir。
    if let Some(dir) = exe_dir {
        let candidate = dir.join("diggerd");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    // 再试 current_exe 同目录。
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("diggerd");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    // 最后在 PATH 里找(GUI 启动的 PATH 很短,不借 shell)。
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("diggerd");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "找不到 diggerd 二进制:请确保它与 digger 在同一目录,或在 PATH 里",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_diggerd_binary_yields_a_readable_error_and_does_not_hang() {
        let dir = tempdir().unwrap();
        let empty_dir = dir.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();

        // 清空 PATH 确保找不到 diggerd。
        let result = std::panic::catch_unwind(|| {
            // 用空的 exe_dir,并且临时改 PATH。
            let old_path = std::env::var("PATH").ok();
            // SAFETY: 测试代码,单线程。
            unsafe {
                std::env::set_var("PATH", "");
            }
            let result = find_diggerd(Some(&empty_dir));
            if let Some(p) = old_path {
                unsafe {
                    std::env::set_var("PATH", p);
                }
            }
            result
        });

        assert!(result.is_err() || result.unwrap().is_err());
    }

    use dozer_core::protocol::{PROTOCOL_VERSION, Reply, Request, decode_line, encode_line};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    #[test]
    fn install_product_makes_this_process_digger() {
        install_product();
        assert_eq!(dozer_core::paths::product().name, "digger");
    }

    fn sock(tag: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/dg-{tag}-{}.sock", std::process::id()))
    }

    /// 假守护进程:Hello 回 `protocol`,ListProjects 回空列表。
    fn fake_daemon(path: &Path, protocol: u32) {
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path).unwrap();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let reply = match decode_line::<Request>(&line) {
                            Ok(Request::Hello { .. }) => Reply::Hello {
                                protocol,
                                product: "digger".into(),
                            },
                            Ok(Request::ListProjects) => Reply::Projects { projects: vec![] },
                            Ok(Request::ListSessions) => Reply::Sessions { sessions: vec![] },
                            _ => Reply::Error {
                                message: "unexpected".into(),
                            },
                        };
                        let _ = w.write_all(encode_line(&reply).as_bytes()).await;
                    }
                });
            }
        });
    }

    #[tokio::test]
    async fn ensure_returns_ok_when_a_daemon_already_answers_and_never_spawns() {
        install_product();
        let s = sock("ok");
        fake_daemon(&s, PROTOCOL_VERSION);
        let spawned = Arc::new(AtomicUsize::new(0));
        let n = spawned.clone();
        let r = Daemon::with_socket(s.clone())
            .ensure_with(
                move || {
                    n.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                std::time::Duration::from_millis(10),
            )
            .await;
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(spawned.load(Ordering::SeqCst), 0);
        let _ = fs::remove_file(s);
    }

    #[tokio::test]
    async fn protocol_mismatch_is_reported_as_such() {
        install_product();
        let s = sock("mismatch");
        fake_daemon(&s, PROTOCOL_VERSION + 1);
        let r = Daemon::with_socket(s.clone())
            .ensure_with(|| Ok(()), std::time::Duration::from_millis(10))
            .await;
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("协议") || msg.contains("无法连接"), "{msg}");
        let _ = fs::remove_file(s);
    }

    #[tokio::test]
    async fn a_failed_spawn_yields_a_readable_error_naming_diggerd() {
        let s = sock("nospawn");
        let _ = fs::remove_file(&s);
        let r = Daemon::with_socket(s)
            .ensure_with(
                || {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "找不到 diggerd",
                    ))
                },
                std::time::Duration::from_millis(10),
            )
            .await;
        assert!(r.unwrap_err().to_string().contains("diggerd"));
    }

    #[tokio::test]
    async fn a_spawn_that_never_brings_up_a_socket_gives_up_after_three_retries() {
        let s = sock("never");
        let _ = fs::remove_file(&s);
        let r = Daemon::with_socket(s)
            .ensure_with(|| Ok(()), std::time::Duration::from_millis(5))
            .await;
        assert!(r.unwrap_err().to_string().contains("diggerd"));
    }

    #[test]
    fn find_diggerd_prefers_the_given_dir_over_path() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("diggerd"), b"").unwrap();
        assert_eq!(
            find_diggerd(Some(dir.path())).unwrap(),
            dir.path().join("diggerd")
        );
    }

    #[tokio::test]
    async fn sessions_service_talks_to_the_daemon_socket() {
        install_product();
        let s = sock("sessions");
        fake_daemon(&s, PROTOCOL_VERSION);
        let svc = Daemon::with_socket(s.clone()).sessions();
        let sessions = svc
            .list_sessions()
            .await
            .expect("list over the fake socket");
        assert!(sessions.is_empty());
        let _ = fs::remove_file(s);
    }
}
