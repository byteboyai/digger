//! 预览宿主的资源根(S5b-2 P3 遗留,Task 9)。
//!
//! 宿主的资源规则只有一条(库 `assets::bundled_assets_root`):开发态读源码树
//! 里的 `assets/flyfish`,打包态(`.app`)自动换成 `Contents/Resources/flyfish`。
//! 产品与示例都从这里拿开发态的同一个值,不再各自拼 `../dozer/...`。
//!
//! "开发态"以 `Cargo.toml` 里 `dozer-panel-preview` 的 `git` + `tag` 为准:
//! `assets::from_lib_source()` 的 `CARGO_MANIFEST_DIR` 就是 Cargo 的 git 检出。

use std::path::PathBuf;

/// 预览宿主的**开发态**资源根(= `Host::new` 的入参,打包态由它换算成
/// `Contents/Resources/flyfish`)。
///
/// 反例:进程在 `.app` 里时返回的已经是包内路径,此时再交给 `Host::new`
/// 会再换一次(等价无害);不在 `.app` 里时就是源码树里的 `assets/flyfish`。
/// 认 `DIGGER_PREVIEW_ASSETS`(打包后的 selftest 也用它):指向一份缺目录 /
/// 缺文件的资源树时,`missing_in` 必须报出来,而不是静默白屏。
pub fn roots() -> PathBuf {
    if let Some(root) = std::env::var_os("DIGGER_PREVIEW_ASSETS") {
        return PathBuf::from(root);
    }
    dozer_panel_preview::assets::from_lib_source()
}

/// 宿主按名字读的目录(`assets/` 的直接子目录):这一批只有这三个会被创建,
/// 其余(json-editor / tabular-host / plantuml-viewer)属于能力关闭的类型。
const REQUIRED_DIRS: [&str; 3] = ["editor", "flyfish", "image-annotate"];

/// 命名空间的入口。`dozer://flyfish/host.html` 读 `flyfish/host.html`;
/// `dozer://image-annotate/host.html` 是**编译期内嵌**的(`assets.rs` 的
/// `image_annotate_host_html`),不读磁盘——所以只点名 flyfish 那个文件,
/// image-annotate 只需目录在(它要服务 vendor 静态资源)。4xx 的静态资源
/// 在宿主里不会报错,只会白屏。
const REQUIRED_FILES: [&str; 1] = ["flyfish/host.html"];

/// `assets/` 目录里缺什么。空 = 齐全。只做存在性检查,不读内容、不碰网络。
pub(crate) fn missing_in(assets: &std::path::Path) -> Vec<String> {
    let dirs = REQUIRED_DIRS
        .iter()
        .filter(|dir| !assets.join(dir).is_dir())
        .map(|dir| format!("{dir}/"));
    let files = REQUIRED_FILES
        .iter()
        .filter(|file| !assets.join(file).is_file())
        .map(|file| (*file).to_string());
    dirs.chain(files).collect()
}

/// `roots()` 对应的 `assets/` 目录(即 `flyfish` 的父目录)。打包态解析的是
/// 进程自己的包内路径,不依赖 `roots()` 的形态。启动时 `missing_in` 用。
pub fn assets_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("DIGGER_PREVIEW_ASSETS") {
        return PathBuf::from(root);
    }
    match dozer_panel_preview::assets::bundle_resources_root(
        &std::env::current_exe().unwrap_or_default(),
    ) {
        Some(resources) => resources,
        None => dozer_panel_preview::assets::from_lib_source()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 开发态必须是锁定的 dozer 检出,不能是本仓库或 `target/` 里的副本。
    ///
    /// 只断言路径形状:bin 测试二进制的 `CARGO_MANIFEST_DIR` 不足以证明资源
    /// 目录存在(存在性由集成测试 `tests/preview_assets_smoke.rs` 检查)。
    #[test]
    fn roots_in_dev_resolve_to_the_pinned_checkout() {
        let root = roots();
        assert!(
            root.ends_with("dozer-panel-preview/assets/flyfish"),
            "开发态资源根应指向库源码树:{}",
            root.display()
        );
        assert!(
            root.to_string_lossy().contains("dozer-"),
            "开发态资源根应落在 Cargo 的 dozer git 检出里:{}",
            root.display()
        );
        assert!(
            !root.starts_with(env!("CARGO_MANIFEST_DIR")),
            "开发态资源根不能是 Digger 自己的源码树:{}",
            root.display()
        );
    }

    /// 产品里 `Host::new(roots())` 之前必须先叫这一句;缺资源时宿主只会整块白屏。
    /// (原名 `selftest_fails_when_a_fixture_is_missing`,名不副实:它测的是
    /// `missing_in` 对**资源目录**的检查,与 selftest 夹具无关。G-M)
    #[test]
    fn missing_in_reports_each_absent_required_dir() {
        let dir = std::env::temp_dir().join(format!(
            "digger-preview-missing-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 只造一半:flyfish 入口在,editor 与 image-annotate 都没造。
        std::fs::create_dir_all(dir.join("flyfish")).unwrap();
        std::fs::write(dir.join("flyfish/host.html"), b"<html>").unwrap();
        let missing = missing_in(&dir);
        assert!(missing.contains(&"editor/".to_string()), "{missing:?}");
        assert!(
            missing.contains(&"image-annotate/".to_string()),
            "{missing:?}"
        );
        assert!(
            !missing.contains(&"flyfish/host.html".to_string()),
            "已存在的入口文件不该报缺:{missing:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 计划里的 `bundled_layout_has_the_three_first_batch_dirs` 被挪进了
    /// `package-macos.sh` 的 shell 检查;这里补一个真正的单元测试:开发态资源根
    /// 必须三个目录齐全,否则产品启动就白屏(G-M)。
    #[test]
    fn dev_roots_have_every_required_dir() {
        let root = roots();
        let assets = root.parent().expect("flyfish 有父目录");
        let missing = missing_in(assets);
        assert!(
            missing.is_empty(),
            "开发态资源根 {} 缺:{missing:?}",
            assets.display()
        );
    }

    /// 缺一个入口文件也必须报出来:宿主的 `dozer://` 应答会把 404 静默落成白屏。
    #[test]
    fn missing_in_names_every_absent_dir_and_entry_file() {
        let dir = std::env::temp_dir().join(format!(
            "digger-preview-assets-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for name in REQUIRED_DIRS {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        std::fs::write(dir.join("flyfish/host.html"), b"<html>").unwrap();
        assert!(missing_in(&dir).is_empty(), "{:?}", missing_in(&dir));

        std::fs::remove_dir_all(dir.join("editor")).unwrap();
        std::fs::remove_file(dir.join("flyfish/host.html")).unwrap();
        let missing = missing_in(&dir);
        assert!(missing.contains(&"editor/".to_string()), "{missing:?}");
        assert!(
            missing.contains(&"flyfish/host.html".to_string()),
            "{missing:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
