//! digger-mcp 薄入口:传 Digger 的产品身份与“只开记忆工具”的特性给
//! `dozer_mcp::run_main`。工具集与 Dozer 的同名工具共用同一份代码。

fn main() {
    dozer_mcp::server::run_main(
        digger_paths::product(),
        dozer_mcp::McpFeatures {
            memory: true,
            summary: false,
            todos: false,
            preview: false,
            file_edit: false,
        },
    );
}
