//! 开发态资源根的集成测试。
//!
//! 放集成测试而不是 bin 的单测:bin 的 `#[cfg(test)]` 会把
//! `dozer_panel_preview::assets::from_lib_source()` 编译进 `target/debug/deps`
//! 下的测试二进制,那里的 `CARGO_MANIFEST_DIR` 不可靠;集成测试直接链接库,
//! 拿到的是库自己的源码树路径(与产品运行时同一条 `bundled_assets_root` 规则)。

#[test]
fn roots_in_dev_resolve_to_the_pinned_checkout() {
    let root = dozer_panel_preview::assets::from_lib_source();
    let parent = root.parent().expect("assets/flyfish 有父目录");
    assert_eq!(
        parent.file_name().and_then(|n| n.to_str()),
        Some("assets"),
        "开发态资源根应是库源码树的 assets/flyfish:{}",
        root.display()
    );
    assert!(
        root.join("host.html").is_file(),
        "锁定的检出缺 flyfish/host.html:{}",
        root.display()
    );
    for dir in ["editor", "image-annotate"] {
        assert!(
            parent.join(dir).is_dir(),
            "锁定的检出缺 {dir}/:{}",
            parent.display()
        );
    }
}
