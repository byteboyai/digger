//! 待办面板的嵌入式 web 资源(S3b-1 Task 1)。
//!
//! 资源唯一来源是 `dozer-panel-todo`;`build.rs` 拷到 `OUT_DIR` 时统一加
//! `todo-` 前缀,避免与用量面板同名的 `host.html` 互相覆盖。
//! 自定义协议只服务这三个文件,其余路径一律 404。

const HOST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/todo-host.html"));
const JS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/todo-content.js"));
const CSS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/todo-content.css"));

/// 按 URI 的 path 部分取资源;只认三个白名单路径。
pub fn serve(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/host.html" => Some(("text/html; charset=utf-8", HOST)),
        "/todo-content.js" => Some(("text/javascript; charset=utf-8", JS)),
        "/todo-content.css" => Some(("text/css; charset=utf-8", CSS)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_the_three_files_with_their_mime_types() {
        for (path, mime) in [
            ("/host.html", "text/html; charset=utf-8"),
            ("/todo-content.js", "text/javascript; charset=utf-8"),
            ("/todo-content.css", "text/css; charset=utf-8"),
        ] {
            let (got, body) = serve(path).unwrap_or_else(|| panic!("{path} must be served"));
            assert_eq!(got, mime);
            assert!(!body.is_empty(), "{path} must not be empty");
        }
    }

    #[test]
    fn unknown_paths_are_not_served() {
        for path in ["/nope", "/../usage-content.js", "/"] {
            assert!(serve(path).is_none(), "{path} must not be served");
        }
    }

    #[test]
    fn host_page_has_a_strict_csp_and_no_external_refs() {
        let html = std::str::from_utf8(HOST).unwrap();
        assert!(html.contains("default-src 'none'"));
        for bad in ["http://", "https://", "//cdn"] {
            assert!(!html.contains(bad), "host.html must not reference {bad}");
        }
    }

    #[test]
    fn todo_and_usage_host_pages_are_different_files() {
        let usage = include_bytes!(concat!(env!("OUT_DIR"), "/host.html"));
        assert_ne!(HOST, usage);
        assert!(
            std::str::from_utf8(HOST)
                .unwrap()
                .contains("todo-content.js")
        );
    }
}
