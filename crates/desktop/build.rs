fn main() {
    copy_usage();
    copy_todo();
}

fn copy_usage() {
    let source = dozer_panel_usage::web_assets_root();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for name in ["host.html", "usage-content.js", "usage-content.css"] {
        let input = source.join(name);
        println!("cargo:rerun-if-changed={}", input.display());
        std::fs::copy(input, out.join(name))
            .expect("shared Usage assets must be built and present");
    }
}

/// 待办资源与用量同名(`host.html`),拷到 `OUT_DIR` 时必须加 `todo-` 前缀,
/// 否则两份构建脚本会互相覆盖。
fn copy_todo() {
    let source = dozer_panel_todo::web_assets_root();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for (name, dest) in [
        ("host.html", "todo-host.html"),
        ("todo-content.js", "todo-content.js"),
        ("todo-content.css", "todo-content.css"),
    ] {
        let input = source.join(name);
        println!("cargo:rerun-if-changed={}", input.display());
        std::fs::copy(input, out.join(dest))
            .expect("shared Todo assets must be built and present");
    }
}
