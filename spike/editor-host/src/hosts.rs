//! 所有 wry 子视图的构建都经过这里(ADR-001 §2、ADR-011 §5)。
//!
//! 一张表(`pinned`)显式决定每一类宿主的每一项 wry 设置——wry 的默认值不是安全默认
//! (下载放行、debug 构建开 devtools、媒体权限无条件批准……)。`builder` 把表里的每一项都显式写进
//! `WebViewBuilder`,并返回"已设置项"清单;测试断言清单覆盖 `DECIDED`,因此新增/遗漏一项会被发现,
//! 而行为测试(`SELFTEST=policy`)在真实 WKWebView 里验证这些设置确实生效。

use bytehost_webview::{AppIpc, AppOrigin, app_init_script, data_store_identifier};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use wry::{NewWindowResponse, WebViewBuilder, WebViewBuilderExtDarwin};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Class {
    Editor,
    Preview,
    App,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Nav {
    /// 只允许宿主自己的初始文档(`about:blank`);任何顶层导航都拒绝。
    InitialDocumentOnly,
    /// 只允许应用自己的 origin(`bytehost-webview::AppOrigin::allows_navigation`)。
    OwnOriginOnly,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ipc {
    /// Digger 的 `HostEnvelope` 桥接(绑定校验在 Rust 侧)。
    Bridge,
    /// 只认带每 webview 随机 nonce 的 5 个白名单动词(`AppIpc`)。
    NonceWhitelist,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pinned {
    pub devtools: bool,
    pub clipboard: bool,
    pub downloads_allowed: bool,
    pub new_windows_allowed: bool,
    pub custom_protocols: &'static [&'static str],
    pub navigation: Nav,
    pub ipc: Ipc,
    pub autoplay: bool,
    pub hotkeys_zoom: bool,
    /// 非持久数据存储(编辑器/预览不应跨会话留下 cookie、localStorage)。
    pub incognito: bool,
    /// 每应用独立的 WKWebsiteDataStore(macOS 14+)。
    pub per_app_data_store: bool,
    pub accept_first_mouse: bool,
    pub back_forward_gestures: bool,
}

/// 已经"做过决定"的 wry 选项。`builder` 必须对每一项都显式设置一次。
pub const DECIDED: &[&str] = &[
    "devtools",
    "clipboard",
    "download_started_handler",
    "new_window_req_handler",
    "custom_protocols",
    "navigation_handler",
    "ipc_handler",
    "autoplay",
    "hotkeys_zoom",
    "incognito",
    "data_store",
    "accept_first_mouse",
    "back_forward_gestures",
];

pub fn pinned(c: Class) -> Pinned {
    match c {
        Class::Editor | Class::Preview => Pinned {
            // 一方代码,开发期可用检查器;发布版一律关。
            devtools: cfg!(debug_assertions),
            clipboard: true,
            downloads_allowed: false,
            new_windows_allowed: false,
            // 宿主资源走作用域受限的自定义协议是 ADR-001 的目标;spike 尚未注册,所以为空。
            // 应用 webview 永远为空(下方 App)。
            custom_protocols: &[],
            navigation: Nav::InitialDocumentOnly,
            ipc: Ipc::Bridge,
            autoplay: false,
            hotkeys_zoom: false,
            incognito: true,
            per_app_data_store: false,
            accept_first_mouse: true,
            back_forward_gestures: false,
        },
        Class::App => Pinned {
            devtools: false, // 无论 debug 还是 release(bytehost A4b1:wry 在 debug 构建默认开)
            clipboard: true, // 由 manifest 授权决定;授权闸门未做,spike 先放行(见 ADR-011)
            downloads_allowed: false,
            new_windows_allowed: false,
            custom_protocols: &[], // 应用 webview 不得拿到 Digger 的任何自定义协议
            navigation: Nav::OwnOriginOnly,
            ipc: Ipc::NonceWhitelist,
            autoplay: false,
            hotkeys_zoom: false,
            incognito: false,
            per_app_data_store: true,
            accept_first_mouse: false,
            back_forward_gestures: false,
        },
    }
}

/// 纯函数:这条导航是否放行。`builder` 里的导航回调只是对它的薄封装。
pub fn nav_allowed(p: &Pinned, origin: Option<&AppOrigin>, url: &str) -> bool {
    match p.navigation {
        Nav::InitialDocumentOnly => url == "about:blank",
        Nav::OwnOriginOnly => origin.is_some_and(|o| o.allows_navigation(url)),
    }
}

/// 行为测试用的记录器:宿主拒绝了什么。
#[derive(Clone, Default)]
pub struct Recorder {
    pub navigations: Arc<Mutex<Vec<(String, bool)>>>,
    pub downloads_denied: Arc<AtomicUsize>,
    pub new_windows_denied: Arc<Mutex<Vec<String>>>,
    pub ipc_rejected: Arc<AtomicUsize>,
    pub ipc_accepted: Arc<Mutex<Vec<String>>>,
}

pub struct Build {
    pub class: Class,
    pub bounds: wry::Rect,
    /// 应用 webview 的 origin 与 id;其余类别为 None。
    pub app: Option<(AppOrigin, String)>,
    /// None:原样转发 IPC 正文(生产路径);Some(tag):加前缀 `tag|ipc|`(探针)。
    pub tag: Option<String>,
    pub forward: UnboundedSender<String>,
    pub rec: Recorder,
}

/// 构建一个已钉死全部设置的 `WebViewBuilder`,并返回已设置项清单。内容(html/url)由调用方接着设置。
pub fn builder(b: &Build) -> (WebViewBuilder<'static>, Vec<&'static str>) {
    let p = pinned(b.class);
    let mut applied = Vec::new();
    let mut wb = WebViewBuilder::new().with_bounds(b.bounds);

    wb = wb.with_devtools(p.devtools);
    applied.push("devtools");
    wb = wb.with_clipboard(p.clipboard);
    applied.push("clipboard");
    wb = wb.with_autoplay(p.autoplay);
    applied.push("autoplay");
    wb = wb.with_hotkeys_zoom(p.hotkeys_zoom);
    applied.push("hotkeys_zoom");
    wb = wb.with_incognito(p.incognito);
    applied.push("incognito");
    wb = wb.with_accept_first_mouse(p.accept_first_mouse);
    applied.push("accept_first_mouse");
    wb = wb.with_back_forward_navigation_gestures(p.back_forward_gestures);
    applied.push("back_forward_gestures");

    // 自定义协议:只注册表里列出的(目前没有任何一类需要)。
    for name in p.custom_protocols {
        let _ = name; // 注册点:wb = wb.with_custom_protocol(...)
    }
    applied.push("custom_protocols");

    // 数据存储:应用各用各的标识(算法在 bytehost-webview,不得抄)。
    if p.per_app_data_store {
        let (_, id) = b.app.as_ref().expect("an app webview needs an app id");
        wb = wb.with_data_store_identifier(data_store_identifier(id));
    }
    applied.push("data_store");

    // 导航:每次导航都问(wry 的回调拿不到主框架/子框架之分,所以对子框架同样适用)。
    let (origin, rec, pn) = (b.app.as_ref().map(|(o, _)| o.clone()), b.rec.clone(), p.clone());
    wb = wb.with_navigation_handler(move |url| {
        let ok = nav_allowed(&pn, origin.as_ref(), &url);
        rec.navigations.lock().unwrap().push((url, ok));
        ok
    });
    applied.push("navigation_handler");

    // window.open:拒绝并记下。
    let (rec, allowed) = (b.rec.clone(), p.new_windows_allowed);
    wb = wb.with_new_window_req_handler(move |url, _| {
        if allowed {
            NewWindowResponse::Allow
        } else {
            rec.new_windows_denied.lock().unwrap().push(url);
            NewWindowResponse::Deny
        }
    });
    applied.push("new_window_req_handler");

    // 下载:拒绝并计数(wry 默认放行)。
    let (rec, allowed) = (b.rec.clone(), p.downloads_allowed);
    wb = wb.with_download_started_handler(move |_url, _path| {
        if !allowed {
            rec.downloads_denied.fetch_add(1, Ordering::SeqCst);
        }
        allowed
    });
    applied.push("download_started_handler");

    // IPC。
    let (tx, tag, rec) = (b.forward.clone(), b.tag.clone(), b.rec.clone());
    match p.ipc {
        Ipc::Bridge => {
            wb = wb.with_ipc_handler(move |req| {
                let body = req.into_body();
                let _ = tx.send(match &tag {
                    Some(t) => format!("{t}|ipc|{body}"),
                    None => body,
                });
            });
        }
        Ipc::NonceWhitelist => {
            let nonce = bytehost_webview::new_ipc_nonce();
            wb = wb.with_initialization_script(app_init_script(&nonce));
            wb = wb.with_ipc_handler(move |req| {
                let body = req.into_body();
                match AppIpc::parse(&body, &nonce) {
                    Some(m) => rec.ipc_accepted.lock().unwrap().push(format!("{m:?}")),
                    None => {
                        rec.ipc_rejected.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
        }
    }
    applied.push("ipc_handler");

    // 运行时守卫:漏掉任何一项"决定"在开发期就会 panic,而不只是在测试里。
    debug_assert!(DECIDED.iter().all(|d| applied.contains(d)), "builder skipped a decided option");
    (wb, applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    fn bounds() -> wry::Rect {
        use wry::dpi::{LogicalPosition, LogicalSize};
        wry::Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(10.0, 10.0).into() }
    }
    fn build(class: Class) -> Build {
        let (tx, _rx) = unbounded_channel();
        Build {
            class,
            bounds: bounds(),
            app: (class == Class::App).then(|| (AppOrigin::from_url("http://notes.localhost:4321/").unwrap(), "notes".to_string())),
            tag: None,
            forward: tx,
            rec: Recorder::default(),
        }
    }

    #[test]
    fn every_class_sets_every_decided_option() {
        for class in [Class::Editor, Class::Preview, Class::App] {
            let (_, applied) = builder(&build(class));
            for d in DECIDED {
                assert!(applied.contains(d), "{class:?} builder does not decide `{d}`");
            }
            for a in &applied {
                assert!(DECIDED.contains(a), "{class:?} builder sets `{a}` which is not in DECIDED (add it, with a decision)");
            }
        }
    }

    #[test]
    fn app_webview_is_the_most_restricted_class() {
        let app = pinned(Class::App);
        assert!(!app.devtools, "devtools are never available to an app, in any build");
        assert!(!app.downloads_allowed && !app.new_windows_allowed && !app.autoplay);
        assert!(app.custom_protocols.is_empty(), "an app webview never gets a Digger custom protocol");
        assert_eq!(app.navigation, Nav::OwnOriginOnly);
        assert_eq!(app.ipc, Ipc::NonceWhitelist);
        assert!(app.per_app_data_store && !app.incognito);
    }

    #[test]
    fn editor_and_preview_pin_the_safe_values() {
        for c in [Class::Editor, Class::Preview] {
            let p = pinned(c);
            assert!(!p.downloads_allowed && !p.new_windows_allowed && !p.autoplay && !p.hotkeys_zoom, "{c:?}");
            assert_eq!(p.navigation, Nav::InitialDocumentOnly, "{c:?}");
            assert_eq!(p.ipc, Ipc::Bridge, "{c:?}");
            assert!(p.incognito && !p.per_app_data_store, "{c:?}: no persistent web data");
            #[cfg(not(debug_assertions))]
            assert!(!p.devtools, "{c:?}: developer tools must be off in release builds");
        }
    }

    #[test]
    fn host_navigation_allows_only_the_initial_document() {
        let p = pinned(Class::Editor);
        assert!(nav_allowed(&p, None, "about:blank"));
        for url in ["https://example.com/", "http://127.0.0.1:1/", "file:///etc/hosts", "data:text/html,x", "javascript:alert(1)", "blob:null/x", "about:srcdoc", ""] {
            assert!(!nav_allowed(&p, None, url), "{url}");
        }
    }

    #[test]
    fn app_navigation_is_confined_to_its_own_origin() {
        let p = pinned(Class::App);
        let o = AppOrigin::from_url("http://notes.localhost:4321/?token=1").unwrap();
        for url in ["http://notes.localhost:4321/", "http://NOTES.localhost:4321/a?b=1#c", "about:blank", "about:srcdoc", "blob:http://notes.localhost:4321/uuid"] {
            assert!(nav_allowed(&p, Some(&o), url), "{url}");
        }
        for url in [
            "http://other.localhost:4321/",
            "http://notes.localhost:4322/",
            "https://notes.localhost:4321/",
            "http://notes.localhost.evil.com:4321/",
            "http://user:pw@notes.localhost:4321/",
            "http://localhost:4321/",
            "http://127.0.0.1:4321/",
            "file:///etc/hosts",
            "data:text/html,x",
            "javascript:alert(1)",
            "blob:http://other.localhost:4321/uuid",
            "dozer://html/x",
            "digger://x/",
        ] {
            assert!(!nav_allowed(&p, Some(&o), url), "{url}");
        }
        assert!(!nav_allowed(&p, None, "http://notes.localhost:4321/"), "without an origin nothing is allowed");
    }
}
