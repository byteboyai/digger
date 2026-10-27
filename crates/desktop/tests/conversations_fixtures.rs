#[path = "../examples/support/conversations.rs"]
#[allow(dead_code)]
mod support;
use dozer_model::AgentKind;
use dozer_panel_conversations::review::{ReviewEntry, review_entries_from_turns};

#[test]
fn tracked_fixtures_use_the_production_parser_and_review_fold() {
    for (name, expected) in [
        ("short", 6),
        ("long", 1000),
        ("huge", 10000),
        ("empty", 0),
        ("broken", 2),
        ("hostile", 2),
    ] {
        let turns = support::turns(name, &support::fixture(name));
        assert_eq!(turns.len(), expected, "{name}");
        assert_eq!(review_entries_from_turns(&turns).len(), expected, "{name}");
    }
    let turns = support::turns("tools", &support::fixture("tools"));
    let entries = review_entries_from_turns(&turns);
    assert!(entries.iter().any(|e| matches!(e, ReviewEntry::AiTurn { thinking_text: Some(_), tool_calls, tool_results, tokens_in: 17, tokens_out: 11, tokens_cache_read: 5, tokens_cache_write: 3, .. } if tool_calls.len() == 2 && tool_results.len() == 2 && tool_results.iter().any(|r| r.is_error))));
    for entry in &entries {
        if let ReviewEntry::AiTurn {
            tool_calls,
            tool_results,
            ..
        } = entry
        {
            for result in tool_results {
                assert!(result.call_id.is_some());
                assert!(tool_calls.iter().any(|call| call.id == result.call_id));
            }
        }
    }
    let hostile = support::turns("hostile", &support::fixture("hostile"));
    assert!(hostile[0].content.contains(&"x".repeat(100_000)));
    assert!(hostile[1].content.len() > 3_000_000);
}

#[test]
fn isolated_seed_is_found_by_exact_core_project_key_and_real_parser() {
    let root = tempfile::Builder::new()
        .prefix("s7c-seed-parser-")
        .tempdir_in(std::env::var_os("TMPDIR").expect("actual TMPDIR"))
        .unwrap();
    let home = root.path().join("home");
    let output = std::process::Command::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/seed-conversations-fixture.sh"),
    )
    .arg(&home)
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cwd = std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let dir = dozer_core::agent_paths::claude_project_dir_in(&home.canonicalize().unwrap(), &cwd);
    for name in support::FIXTURES {
        let text = std::fs::read_to_string(dir.join(format!("{name}.jsonl"))).unwrap();
        let parsed = dozerd::transcripts::parse::parse_chunk(AgentKind::Claude, &text, name, 0);
        assert_eq!(
            parsed.len(),
            support::turns(name, &support::fixture(name)).len(),
            "{name}"
        );
    }
}
