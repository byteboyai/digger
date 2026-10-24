//! `digger-mcp install/uninstall <agent>`:只碰自己的 `digger` 条目,与 `dozer` 并存。

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(args: &[&str], envs: &[(&str, &Path)]) -> i32 {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_digger-mcp"));
    cmd.args(args)
        .env("HOME", tempfile::tempdir().unwrap().path());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.status().expect("run digger-mcp").code().unwrap_or(-1)
}

fn env_name(agent: &str) -> &'static str {
    match agent {
        "claude" => "DIGGER_CLAUDE_MCP_CONFIG",
        "codebuddy" => "DIGGER_CODEBUDDY_MCP_CONFIG",
        "qodercn" => "DIGGER_QODERCN_MCP_CONFIG",
        "codex" => "DIGGER_CODEX_MCP_CONFIG",
        _ => "DIGGER_OPENCODE_MCP_CONFIG",
    }
}

#[test]
fn install_writes_a_digger_entry_for_each_supported_agent() {
    let dir = tempfile::tempdir().unwrap();
    for agent in ["claude", "codebuddy", "qodercn", "codex", "opencode"] {
        let path = dir.path().join(format!("{agent}.conf"));
        std::fs::write(&path, if agent == "codex" { "" } else { "{}" }).unwrap();
        assert_eq!(
            run(&["install", agent], &[(env_name(agent), &path)]),
            0,
            "{agent}"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"digger\"")
                || text.contains("[mcp_servers.digger")
                || text.contains("\"digger\":")
                || text.contains("digger"),
            "{agent} 未写入 digger 条目: {text}"
        );
    }
}

#[test]
fn install_next_to_an_existing_dozer_entry_keeps_it_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claude.json");
    let original = serde_json::json!({
        "mcpServers": {
            "dozer": {"type": "stdio", "command": "/Applications/Dozer.app/Contents/MacOS/dozer-mcp", "args": ["serve"], "env": {}}
        },
        "other": {"keep": true}
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&original).unwrap()).unwrap();
    assert_eq!(
        run(
            &["install", "claude"],
            &[("DIGGER_CLAUDE_MCP_CONFIG", &path)]
        ),
        0
    );
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        after["mcpServers"]["dozer"],
        original["mcpServers"]["dozer"]
    );
    assert_eq!(after["other"], original["other"]);
    assert_eq!(
        after["mcpServers"]["digger"]["args"],
        serde_json::json!(["serve"])
    );
}

#[test]
fn uninstall_removes_only_the_digger_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claude.json");
    let doc = serde_json::json!({
        "mcpServers": {
            "dozer": {"type": "stdio", "command": "/x/dozer-mcp", "args": ["serve"]},
            "digger": {"type": "stdio", "command": "/x/digger-mcp", "args": ["serve"]}
        }
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    assert_eq!(
        run(
            &["uninstall", "claude"],
            &[("DIGGER_CLAUDE_MCP_CONFIG", &path)]
        ),
        0
    );
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(after["mcpServers"].get("digger").is_none());
    assert_eq!(after["mcpServers"]["dozer"]["command"], "/x/dozer-mcp");
}

#[test]
fn a_same_named_user_entry_is_protected_and_exits_3() {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("claude.json");
    let doc = serde_json::json!({
        "mcpServers": {"digger": {"type": "stdio", "command": "/usr/local/bin/other-tool"}}
    });
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        run(
            &["install", "claude"],
            &[("DIGGER_CLAUDE_MCP_CONFIG", &path)]
        ),
        3
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "同名条目不得被覆盖");
}
