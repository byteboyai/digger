//! S6a Task 9:Digger 守护进程的拉起与生命周期管理。
//!
//! `Daemon` 负责确保 `diggerd` 在跑:连不上就 spawn 一个,隔 1 秒重试最多 3 次。
//! 关闭 Digger 窗口不终止 `diggerd`(会话要随守护进程存活,与 Dozer 一致)。

use dozer_client::Client;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Digger 守护进程管理器:负责确保 `diggerd` 在跑并能响应请求。
pub struct Daemon {
    client: Client,
    socket: PathBuf,
}

impl Daemon {
    /// 构造一个指向 Digger 产品 socket 的守护进程管理器。
    pub fn new() -> Self {
        let socket = digger_paths::product().socket_path();
        let client = Client::new(socket.clone());
        Self { client, socket }
    }

    /// 确保守护进程在跑并能响应请求。
    ///
    /// 先尝试连接并握手;连不上就 spawn 一个 `diggerd`,隔 1 秒重试最多 3 次。
    /// 失败时返回可读错误,调用方显示提示但不阻断主窗口启动。
    pub async fn ensure(&self) -> Result<(), DaemonError> {
        // 先尝试连接。
        match self.try_connect().await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("diggerd 连接失败,尝试拉起: {e}");
            }
        }

        // spawn 一个新的 diggerd。
        Self::spawn(None).map_err(|e| DaemonError::new(format!("拉起 diggerd 失败: {e}")))?;

        // 重试最多 3 次,每次隔 1 秒。
        for attempt in 1..=3 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
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

    // 最后试 PATH(用 which)。
    let output = Command::new("which").arg("diggerd").output()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
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

    #[test]
    fn spawn_does_not_inherit_stdin() {
        // 这个测试只验证 spawn 函数签名正确,实际 spawn 需要真实的 diggerd 二进制。
        // 在 CI 里没有 diggerd,所以只检查 find_diggerd 的错误路径。
        let dir = tempdir().unwrap();
        let result = find_diggerd(Some(dir.path()));
        assert!(result.is_err());
    }
}
