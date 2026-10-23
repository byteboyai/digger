//! Agent 面板的 hook 安装器(S6d-2 Task 2)。
//!
//! 库只定义 [`HookInstaller`] 接缝;Digger 的实现是 spawn 同目录的
//! `digger-hook install <agent>`(与 Dozer 的 `dozer-hook` 同型),不把 hook
//! 命令写进 agent 自己的配置文件以外的任何东西,也不碰 MCP(能力关闭,设计 D5)。
//! 安装是幂等且静默的:失败只 `eprintln!`,绝不拖慢或打断新会话。

use dozer_model::AgentKind;
use dozer_panel_agent::adapter::HookInstaller;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
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

/// 子进程的退出结果(带退出码,给 MCP 安装的退出码 3 用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildOutcome {
    Exited(i32),
    TimedOut,
    SpawnFailed,
}

/// 跑一个子进程:不继承 stdin、丢弃输出、有上限。
pub fn run_child_with_timeout(exe: &Path, args: &[&str], timeout: Duration) -> ChildOutcome {
    let mut child = match Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("digger: 启动 {exe:?} {args:?} 失败: {error}"); // cli-output
            return ChildOutcome::SpawnFailed;
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ChildOutcome::Exited(status.code().unwrap_or(-1)),
            Ok(None) => {}
            Err(error) => {
                eprintln!("digger: 等待 {exe:?} {args:?} 失败: {error}"); // cli-output
                let _ = child.kill();
                let _ = child.wait();
                return ChildOutcome::SpawnFailed;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("digger: {exe:?} {args:?} 超时,已放弃"); // cli-output
            return ChildOutcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// MCP 支持安装的 Agent(D5 名单;Aider/Goose/V8agent/Unknown 空操作)。
pub fn mcp_install_target(agent: AgentKind) -> Option<&'static str> {
    match agent {
        AgentKind::Claude
        | AgentKind::Codebuddy
        | AgentKind::Codex
        | AgentKind::Opencode
        | AgentKind::Qodercn => Some(agent.label()),
        AgentKind::Unknown | AgentKind::Aider | AgentKind::Goose | AgentKind::V8agent => None,
    }
}

/// 同目录的 `digger-mcp`;`exe` 没有父目录时退回裸名字(交给 PATH)。
pub fn mcp_exe_beside(exe: &Path) -> PathBuf {
    match exe.parent() {
        Some(dir) => dir.join("digger-mcp"),
        None => PathBuf::from("digger-mcp"),
    }
}

/// 产品注入的 hook / MCP 安装器。
pub struct DiggerHooks {
    hook_exe: PathBuf,
    mcp_exe: PathBuf,
    timeout: Duration,
    mcp_timeout: Duration,
    /// 本进程已经处理过的 Agent(成功/失败/保护拒绝都不再重试,D7)。
    mcp_attempted: Mutex<HashSet<&'static str>>,
    /// 待展示的 MCP 安装警告(由 `AgentPanel` 排空进通知条)。
    warnings: Arc<Mutex<Vec<String>>>,
}

impl DiggerHooks {
    /// 生产路径:`current_exe` 同目录的 `digger-hook` / `digger-mcp`;找不到退回裸名字。
    pub fn new() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("digger"));
        let mut hook_exe = hook_exe_beside(&exe);
        if !hook_exe.is_file() {
            hook_exe = PathBuf::from("digger-hook");
        }
        let mut mcp_exe = mcp_exe_beside(&exe);
        if !mcp_exe.is_file() {
            mcp_exe = PathBuf::from("digger-mcp");
        }
        Self {
            hook_exe,
            mcp_exe,
            timeout: INSTALL_TIMEOUT,
            mcp_timeout: INSTALL_TIMEOUT,
            mcp_attempted: Mutex::new(HashSet::new()),
            warnings: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 测试/嵌入用:显式给路径与上限。
    #[allow(dead_code)] // 测试用;产品路径走 `new`。
    pub fn with_exe(hook_exe: PathBuf, timeout: Duration) -> Self {
        let mcp_exe = hook_exe.with_file_name("digger-mcp");
        Self {
            hook_exe,
            mcp_exe,
            timeout,
            mcp_timeout: timeout,
            mcp_attempted: Mutex::new(HashSet::new()),
            warnings: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 测试用:显式给 hook / MCP 两个路径与上限。
    #[allow(dead_code)]
    pub fn with_exes(
        hook_exe: PathBuf,
        mcp_exe: PathBuf,
        timeout: Duration,
        mcp_timeout: Duration,
    ) -> Self {
        Self {
            hook_exe,
            mcp_exe,
            timeout,
            mcp_timeout,
            mcp_attempted: Mutex::new(HashSet::new()),
            warnings: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 与 `AgentPanel` 共享的警告队列。
    pub fn warnings_handle(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.warnings)
    }

    #[allow(dead_code)] // 测试直接读;产品路径用 `warnings_handle`。
    pub fn take_warnings(&self) -> Vec<String> {
        self.warnings
            .lock()
            .map(|mut warnings| std::mem::take(&mut *warnings))
            .unwrap_or_default()
    }

    fn warn(&self, text: String) {
        eprintln!("digger: {text}"); // cli-output
        if let Ok(mut warnings) = self.warnings.lock() {
            warnings.push(text);
        }
    }

    fn install_mcp(&self, label: &'static str) {
        let first = self
            .mcp_attempted
            .lock()
            .map(|mut attempted| attempted.insert(label))
            .unwrap_or(false);
        if !first {
            return;
        }
        match run_child_with_timeout(&self.mcp_exe, &["install", label], self.mcp_timeout) {
            ChildOutcome::Exited(0) => {}
            ChildOutcome::Exited(3) => {
                let path =
                    dozer_mcp::install::config_path_for_product(label, &digger_paths::product())
                        .map(|(path, _)| path.display().to_string())
                        .unwrap_or_else(|| "该 Agent 的 MCP 配置文件".into());
                self.warn(format!(
                    "未写入 Digger 的 MCP 条目:{path} 里已有一个名为 \"digger\" 的服务器"
                ));
            }
            ChildOutcome::Exited(code) => self.warn(format!(
                "未能为 {label} 注册 digger MCP(退出码 {code});会话仍会创建"
            )),
            ChildOutcome::TimedOut => self.warn(format!(
                "未能为 {label} 注册 digger MCP(安装超时);会话仍会创建"
            )),
            ChildOutcome::SpawnFailed => self.warn(format!(
                "未能为 {label} 注册 digger MCP(找不到 digger-mcp);会话仍会创建"
            )),
        }
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

    fn ensure_mcp(&self, agent: AgentKind) {
        if let Some(label) = mcp_install_target(agent) {
            self.install_mcp(label);
        }
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

    fn mcp_script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("digger-mcp");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn with_mcp_script(dir: &Path, body: &str) -> DiggerHooks {
        let hook = script(dir, "exit 0");
        let mcp = mcp_script(dir, body);
        DiggerHooks::with_exes(hook, mcp, Duration::from_secs(2), Duration::from_secs(3))
    }

    #[test]
    fn ensure_mcp_runs_install_with_the_agent_label_once_per_process() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("mcp.log");
        let hooks = with_mcp_script(dir.path(), &format!("echo \"$@\" >> \"{}\"", log.display()));
        for _ in 0..3 {
            hooks.ensure_mcp(AgentKind::Claude);
        }
        let got = fs::read_to_string(&log).unwrap();
        assert_eq!(got.lines().collect::<Vec<_>>(), ["install claude"]);
    }

    #[test]
    fn different_agents_each_get_one_install() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("mcp.log");
        let hooks = with_mcp_script(dir.path(), &format!("echo \"$@\" >> \"{}\"", log.display()));
        hooks.ensure_mcp(AgentKind::Claude);
        hooks.ensure_mcp(AgentKind::Codex);
        hooks.ensure_mcp(AgentKind::Claude);
        let got = fs::read_to_string(&log).unwrap();
        assert_eq!(
            got.lines().collect::<Vec<_>>(),
            ["install claude", "install codex"]
        );
    }

    #[test]
    fn unsupported_agents_do_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("mcp.log");
        let hooks = with_mcp_script(dir.path(), &format!("echo \"$@\" >> \"{}\"", log.display()));
        for agent in [
            AgentKind::Aider,
            AgentKind::Goose,
            AgentKind::V8agent,
            AgentKind::Unknown,
        ] {
            hooks.ensure_mcp(agent);
        }
        assert!(!log.exists());
    }

    #[test]
    fn exit_code_3_registers_one_readable_protection_warning_and_is_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("mcp.log");
        let hooks = with_mcp_script(
            dir.path(),
            &format!("echo \"$@\" >> \"{}\"; exit 3", log.display()),
        );
        hooks.ensure_mcp(AgentKind::Claude);
        hooks.ensure_mcp(AgentKind::Claude);
        let warnings = hooks.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("名为 \"digger\" 的服务器"),
            "{}",
            warnings[0]
        );
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    }

    #[test]
    fn missing_binary_registers_one_warning_and_does_not_block_or_panic() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = DiggerHooks::with_exes(
            dir.path().join("no-hook"),
            dir.path().join("no-mcp"),
            Duration::from_millis(200),
            Duration::from_millis(200),
        );
        let start = Instant::now();
        hooks.ensure_mcp(AgentKind::Claude);
        assert!(start.elapsed() < Duration::from_secs(2));
        let warnings = hooks.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("找不到 digger-mcp"));
    }

    #[test]
    fn hanging_install_is_abandoned_after_the_timeout_and_warns_once() {
        let dir = tempfile::tempdir().unwrap();
        let hook = script(dir.path(), "exit 0");
        let mcp = mcp_script(dir.path(), "sleep 60");
        let hooks = DiggerHooks::with_exes(
            hook,
            mcp,
            Duration::from_secs(2),
            Duration::from_millis(300),
        );
        let start = Instant::now();
        hooks.ensure_mcp(AgentKind::Claude);
        assert!(start.elapsed() < Duration::from_secs(3));
        let warnings = hooks.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("安装超时"));
    }

    #[test]
    fn install_does_not_inherit_stdin() {
        let dir = tempfile::tempdir().unwrap();
        // stdin 若被继承(终端/管道),read 会阻塞到超时;这里是 null,read 立刻 EOF。
        let hooks = with_mcp_script(dir.path(), "read -r line || exit 0; exit 9");
        hooks.ensure_mcp(AgentKind::Claude);
        assert!(hooks.take_warnings().is_empty());
    }

    #[test]
    fn warnings_are_taken_once_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = with_mcp_script(dir.path(), "exit 1");
        hooks.ensure_mcp(AgentKind::Claude);
        assert_eq!(hooks.take_warnings().len(), 1);
        assert!(hooks.take_warnings().is_empty());
    }

    #[test]
    fn the_notice_text_names_the_config_file_for_protection_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude-mcp.json");
        // SAFETY: 测试代码;其它 MCP 用例不走 exit 3 分支,不读这个变量。
        unsafe {
            std::env::set_var("DIGGER_CLAUDE_MCP_CONFIG", &path);
        }
        let hooks = with_mcp_script(dir.path(), "exit 3");
        hooks.ensure_mcp(AgentKind::Claude);
        let warnings = hooks.take_warnings();
        unsafe {
            std::env::remove_var("DIGGER_CLAUDE_MCP_CONFIG");
        }
        assert!(
            warnings[0].contains(&path.display().to_string()),
            "{}",
            warnings[0]
        );
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
