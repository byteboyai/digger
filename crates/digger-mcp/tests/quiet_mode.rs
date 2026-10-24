//! 并存安静模式:没有 `DIGGER_SESSION_ID`、但有别的产品会话变量时,服务初始化
//! 成功、零工具、不吐 stderr、不连 socket。

use std::io::Write;
use std::process::{Command, Stdio};

fn run_mcp(envs: &[(&str, String)], requests: &[&str]) -> (i32, String, String) {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_digger-mcp"))
        .arg("serve")
        .env("HOME", home.path())
        .env("DIGGER_DATA_DIR", data.path())
        .env_remove("DIGGER_SESSION_ID")
        .env_remove("DOZER_SESSION_ID")
        .envs(envs.iter().map(|(k, v)| (*k, v.clone())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn digger-mcp");
    {
        let stdin = child.stdin.as_mut().unwrap();
        for line in requests {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn initialize() -> &'static str {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"digger-test","version":"0"}}}"#
}

fn tools_list() -> &'static str {
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
}

#[test]
fn with_only_dozer_session_env_the_server_initializes_and_lists_nothing() {
    let (code, stdout, stderr) = run_mcp(
        &[("DOZER_SESSION_ID", "dozer-session".into())],
        &[
            initialize(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            tools_list(),
        ],
    );
    assert_eq!(code, 0);
    assert!(stderr.is_empty(), "安静模式不应写 stderr: {stderr}");
    let responses: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(
        responses
            .iter()
            .any(|v| v["id"] == 1 && v.get("result").is_some()),
        "initialize 应成功: {stdout}"
    );
    let list = responses
        .iter()
        .find(|v| v["id"] == 2)
        .expect("tools/list 应答");
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 0);
}

#[test]
fn with_no_session_env_it_fails_with_the_digger_message() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_digger-mcp"))
        .arg("serve")
        .env("HOME", home.path())
        .env("DIGGER_DATA_DIR", data.path())
        .env_remove("DIGGER_SESSION_ID")
        .env_remove("DOZER_SESSION_ID")
        .stdin(Stdio::null())
        .output()
        .expect("spawn digger-mcp");
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("缺少 DIGGER_SESSION_ID"),
        "stderr: {stderr}"
    );
}
