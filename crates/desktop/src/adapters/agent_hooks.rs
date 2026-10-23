//! Agent 面板的 hook 安装器(S6d-2 Task 2)。
//!
//! 库只定义 [`HookInstaller`] 接缝;Digger 的实现是 spawn 同目录的
//! `digger-hook install <agent>`(与 Dozer 的 `dozer-hook` 同型),不把 hook
//! 命令写进 agent 自己的配置文件以外的任何东西,也不碰 MCP(能力关闭,设计 D5)。
//! 安装是幂等且静默的:失败只 `eprintln!`,绝不拖慢或打断新会话。

use dozer_model::AgentKind;
use dozer_panel_agent::adapter::HookInstaller;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 安装进程的上限:agent CLI 不能因为 hook 安装卡住而迟迟不启动。
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(10);

/// 一次 install 的结果(测试用;产品路径只关心是否完成)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// 进程在时限内退出(退出码非零也算完成,不重试)。
    Finished,
    /// 超过时限,已 kill 并收尸。
    TimedOut,
    /// spawn 失败(二进制不存在 / 无权限)。
    SpawnFailed,
}

/// 同目录的 `digger-hook`;`exe` 没有父目录时退回裸名字(交给 PATH)。
pub fn hook_exe_beside(exe: &Path) -> PathBuf {
    match exe.parent() {
        Some(dir) => dir.join("digger-hook"),
        None => PathBuf::from("digger-hook"),
    }
}

/// 这个 agent 有没有 `digger-hook install` 目标。名单与 Dozer
/// `hook_install_target` 的 `Some` 一侧一致:Aider 走 launcher、V8agent 走
/// 自己的 UDS 上报、Unknown 不装。刻意穷尽 match,新增枚举变体时不会静默落进
/// 默认分支。
pub fn install_target(agent: AgentKind) -> Option<&'static str> {
    match agent {
        AgentKind::Claude
        | AgentKind::Codebuddy
        | AgentKind::Codex
        | AgentKind::Qodercn
        | AgentKind::Opencode
        | AgentKind::Goose => Some(agent.label()),
        AgentKind::Unknown | AgentKind::Aider | AgentKind::V8agent => None,
    }
}

/// 跑一次 `digger-hook install <label>`,不继承 stdin、丢弃输出、有上限。
pub fn install_with_timeout(exe: &Path, label: &str, timeout: Duration) -> InstallOutcome {
    let mut child = match Command::new(exe)
        .arg("install")
        .arg(label)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("digger: 启动 {exe:?} install {label} 失败: {error}"); // cli-output
            return InstallOutcome::SpawnFailed;
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return InstallOutcome::Finished,
            Ok(None) => {}
            Err(error) => {
                eprintln!("digger: 等待 {exe:?} install {label} 失败: {error}"); // cli-output
                let _ = child.kill();
                let _ = child.wait();
                return InstallOutcome::Finished;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("digger: {exe:?} install {label} 超时,已放弃"); // cli-output
            return InstallOutcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 产品注入的 hook 安装器。
pub struct DiggerHooks {
    hook_exe: PathBuf,
    timeout: Duration,
}

impl DiggerHooks {
    /// 生产路径:`current_exe` 同目录的 `digger-hook`;找不到就退回裸名字(交给 PATH)。
    pub fn new() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("digger"));
        let mut hook_exe = hook_exe_beside(&exe);
        if !hook_exe.is_file() {
            hook_exe = PathBuf::from("digger-hook");
        }
        Self {
            hook_exe,
            timeout: INSTALL_TIMEOUT,
        }
    }

    /// 测试/嵌入用:显式给路径与上限。
    #[allow(dead_code)] // 测试用;产品路径走 `new`。
    pub fn with_exe(hook_exe: PathBuf, timeout: Duration) -> Self {
        Self { hook_exe, timeout }
    }
}

impl Default for DiggerHooks {
    fn default() -> Self {
        Self::new()
    }
}

impl HookInstaller for DiggerHooks {
    fn ensure_hook(&self, agent: AgentKind) {
        let Some(label) = install_target(agent) else {
            return;
        };
        let _ = install_with_timeout(&self.hook_exe, label, self.timeout);
    }

    fn ensure_mcp(&self, _agent: AgentKind) {
        // 能力关闭(设计 D5):Digger 首发不注册 MCP。
    }

    fn hook_exe(&self) -> String {
        self.hook_exe.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("digger-hook");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn hook_exe_is_the_sibling_digger_hook() {
        assert_eq!(
            hook_exe_beside(Path::new("/Applications/Digger.app/Contents/MacOS/digger")),
            PathBuf::from("/Applications/Digger.app/Contents/MacOS/digger-hook")
        );
        assert_eq!(
            hook_exe_beside(Path::new("digger")),
            PathBuf::from("digger-hook")
        );
    }

    #[test]
    fn hooks_run_install_for_known_agents_and_skip_aider_unknown_v8agent() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("args.log");
        let exe = script(dir.path(), &format!("echo \"$@\" >> '{}'", log.display()));
        let hooks = DiggerHooks::with_exe(exe, Duration::from_secs(5));
        for agent in [
            AgentKind::Claude,
            AgentKind::Codebuddy,
            AgentKind::Codex,
            AgentKind::Qodercn,
            AgentKind::Opencode,
            AgentKind::Goose,
        ] {
            hooks.ensure_hook(agent);
        }
        let got = fs::read_to_string(&log).unwrap();
        for agent in [
            AgentKind::Claude,
            AgentKind::Codebuddy,
            AgentKind::Codex,
            AgentKind::Qodercn,
            AgentKind::Opencode,
            AgentKind::Goose,
        ] {
            assert!(
                got.lines()
                    .any(|l| l == format!("install {}", agent.label())),
                "missing install for {agent:?}: {got}"
            );
        }
        assert_eq!(
            got.lines().count(),
            6,
            "only the six hook-capable agents run install"
        );
        for agent in [AgentKind::Aider, AgentKind::V8agent, AgentKind::Unknown] {
            hooks.ensure_hook(agent);
        }
        assert_eq!(fs::read_to_string(&log).unwrap(), got);
    }

    #[test]
    fn a_missing_hook_binary_does_not_panic_or_block() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = DiggerHooks::with_exe(dir.path().join("nope"), Duration::from_millis(100));
        let start = Instant::now();
        hooks.ensure_hook(AgentKind::Claude);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_hanging_hook_is_abandoned_after_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), "sleep 60");
        let start = Instant::now();
        let outcome = install_with_timeout(&exe, "claude", Duration::from_millis(150));
        assert_eq!(outcome, InstallOutcome::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "the timeout must not wait for the sleeping child"
        );
    }

    #[test]
    fn ensure_mcp_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("args.log");
        let exe = script(dir.path(), &format!("echo \"$@\" >> '{}'", log.display()));
        let hooks = DiggerHooks::with_exe(exe, Duration::from_secs(5));
        hooks.ensure_mcp(AgentKind::Claude);
        assert!(!log.exists(), "MCP installation is disabled for Digger");
    }
}
