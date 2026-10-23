//! `digger-mcp` 的工具集:恰为三个记忆工具,与 Dozer 的同名项共用代码。

use dozer_mcp::McpFeatures;

fn digger_features() -> McpFeatures {
    McpFeatures {
        memory: true,
        summary: false,
        todos: false,
        preview: false,
        file_edit: false,
    }
}

#[test]
fn tools_list_is_exactly_the_three_memory_tools() {
    let names = digger_features().tool_names();
    assert_eq!(names, ["write_memory", "list_memories", "get_memory"]);
    // 描述与参数 schema 来自 dozer-mcp 的同一份 `#[tool]` 路由代码;这里额外
    // 核对"全量工具集里同名项落在同一组",防止将来分组改名漂移。
    let all = McpFeatures::all_tool_names();
    for name in &names {
        assert!(all.contains(name), "{name} 不在 dozer-mcp 的同一份路由表里");
    }
}

#[test]
fn no_todo_preview_edit_or_summary_tool_is_listed() {
    let names = digger_features().tool_names();
    for forbidden in [
        "list_todos",
        "add_todo",
        "toggle_todo",
        "edit_todo_text",
        "get_preview_context",
        "preview_navigate",
        "locate_in_file",
        "apply_precise_edit",
        "submit_session_summary",
    ] {
        assert!(!names.contains(&forbidden), "{forbidden} 不该出现");
    }
}
