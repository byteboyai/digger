fn main() {
    let source = dozer_panel_usage::web_assets_root();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for name in ["host.html", "usage-content.js", "usage-content.css"] {
        let input = source.join(name);
        println!("cargo:rerun-if-changed={}", input.display());
        std::fs::copy(input, out.join(name))
            .expect("shared Usage assets must be built and present");
    }
}
