//! `digger-mcp` 的工具集:记忆 + 预览上下文/导航/定位 + 精确编辑,与 Dozer 的同名项共用代码。

use dozer_mcp::McpFeatures;

fn digger_features() -> McpFeatures {
    McpFeatures {
        memory: true,
        summary: false,
        todos: false,
        preview: true,
        file_edit: true,
    }
}

#[test]
fn tools_list_is_memory_plus_the_preview_and_edit_bridge() {
    let names = digger_features().tool_names();
    assert_eq!(
        names,
        [
            "write_memory",
            "list_memories",
            "get_memory",
            "get_preview_context",
            "preview_navigate",
            "locate_in_file",
            "apply_precise_edit",
        ]
    );
    // 描述与参数 schema 来自 dozer-mcp 的同一份 `#[tool]` 路由代码;这里额外
    // 核对"全量工具集里同名项落在同一组",防止将来分组改名漂移。
    let all = McpFeatures::all_tool_names();
    for name in &names {
        assert!(all.contains(name), "{name} 不在 dozer-mcp 的同一份路由表里");
    }
}

#[test]
fn todo_and_summary_tools_stay_off() {
    let names = digger_features().tool_names();
    for forbidden in [
        "list_todos",
        "add_todo",
        "toggle_todo",
        "edit_todo_text",
        "submit_session_summary",
    ] {
        assert!(!names.contains(&forbidden), "{forbidden} 不该出现");
    }
}
