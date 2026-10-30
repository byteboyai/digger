//! Markdown 编辑器的 webview 构建器(ADR-013 §3):Digger 自己的第一方宿主构建器。
//!
//! - 不用 `bytehost_desktop::panel::build_child`(没有 IPC / 初始化脚本),也不用
//!   `build_app_child`(应用类:`<id>.localhost` 来源、AppIpc 白名单)——编辑器是可信的第一方代码。
//! - 每一项 wry 设置都显式钉死,并有测试读本文件的生产源码断言(见 `tests`)。
//! - 内容只来自编译进二进制的页面:不走 `file://`、不走 URL、不注册自定义协议。
//! - **为什么不并入 `bytehost_desktop::native_child`(X1 阶段 B 评估,2026-10-28):** 那个执行器的模型是
//!   「按 URL 载入、`allows_navigation` 按 scheme/host 严格比对、`Desired`/`plan` 状态机」,而这里是
//!   `with_html` 载入编译进二进制的页面、导航只放行 `about:blank`、带 IPC 处理器与初始化脚本、剪贴板故意开着,
//!   生命周期是一组直接操作单槽的函数。设置与模型都不同,硬并只会把差异塞进 `configure` 闭包里。
//! - 句柄表是本模块私有的 `thread_local`,与应用面板的句柄表**永不共用**。
//! - 剪贴板**故意不关**(与 `build_app_child` 相反):粘贴是写作的核心动作,而这里跑的是第一方代码。
//!   wry 0.55 在 macOS 上没有剪贴板开关(恒开),`with_clipboard(true)` 只是把意图写明。
//!   `allow_link_preview(false)` 在 wry 0.55.1 的构建器里不存在,无法钉死(见 ADR-013 更新)。

use std::cell::RefCell;

use bytehost_core::LogicalRect;
use digger_editor::bridge::Mode;

use super::Message;

/// 编译进二进制的单文件页面(`web/dist/cm.html`,由 `scripts/build-editor-web.sh` 生成并提交)。
const HTML: &str = include_str!("../../web/dist/cm.html");

thread_local! {
    /// (generation, webview)。只在窗口线程上访问,经 `iced::window::run` 到达。
    static VIEW: RefCell<Option<(u64, wry::WebView)>> = const { RefCell::new(None) };
}

fn bounds(rect: LogicalRect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(rect.x, rect.y).into(),
        size: wry::dpi::LogicalSize::new(rect.width, rect.height).into(),
    }
}

/// 初始化脚本:在页面脚本之前冻结握手 nonce 与运行模式。
fn init_script(nonce: &str, mode: Mode) -> String {
    let mode = if mode == Mode::SelfTest {
        "selftest"
    } else {
        "app"
    };
    format!("window.__DIGGER__ = Object.freeze({{ nonce: {nonce:?}, mode: {mode:?} }});")
}

/// 建 webview 并登记到句柄表。必须在窗口线程上调用。
pub fn build(
    window: &impl wry::raw_window_handle::HasWindowHandle,
    generation: u64,
    rect: LogicalRect,
    nonce: &str,
    mode: Mode,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
) -> Result<(), String> {
    let view = wry::WebViewBuilder::new()
        .with_bounds(bounds(rect))
        .with_devtools(false)
        .with_clipboard(true)
        .with_download_started_handler(|_, _| false)
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_navigation_handler(|url| url == "about:blank")
        .with_initialization_script(init_script(nonce, mode))
        .with_initialization_script(format!(
            "window.__DIGGER_THEME__ = {:?};",
            crate::settings::theme::code()
        ))
        .with_ipc_handler(move |request| {
            let _ = tx.send(Message::Page(generation, request.into_body()));
        })
        .with_html(HTML)
        .build_as_child(window)
        .map_err(|e| e.to_string())?;
    VIEW.with(|v| *v.borrow_mut() = Some((generation, view)));
    Ok(())
}

/// Rust → 页面:`window.__md.receive(<json>)`。过期的 generation 不执行。
pub fn send(generation: u64, json: &str) {
    let js = format!(
        "window.__md && window.__md.receive({})",
        serde_json::to_string(json).expect("string serialises")
    );
    with(generation, |view| {
        let _ = view.evaluate_script(&js);
    });
}

pub fn set_bounds(generation: u64, rect: LogicalRect) {
    with(generation, |view| {
        let _ = view.set_bounds(bounds(rect));
    });
}

pub fn set_visible(generation: u64, visible: bool) {
    with(generation, |view| {
        let _ = view.set_visible(visible);
    });
}

/// 仅更新样式，不重建文档或撤销栈；隐藏的编辑器也保留最新主题。
pub fn set_theme(generation: u64, theme: &str) {
    let theme = serde_json::to_string(theme).expect("theme serializes");
    with(generation, |view| {
        let _ = view.evaluate_script(&format!(
            "window.__DIGGER_THEME__ = {theme}; document.documentElement?.setAttribute('data-theme', {theme});"
        ));
    });
}

/// 真实 webview 此刻的样子(生命周期自检读它:读的是 NSView 本身,不是我们自己的记账)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    /// 该 generation 的 webview 还在句柄表里
    pub exists: bool,
    /// NSView 未被隐藏
    pub visible: bool,
    /// NSView 的实际位置与大小(逻辑像素:x, y, 宽, 高;y 从顶部算)
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// 句柄表里的 webview 总数
    pub handles: usize,
}

pub fn probe(generation: u64) -> Probe {
    use wry::WebViewExtMacOS;
    let handles = VIEW.with(|v| usize::from(v.borrow().is_some()));
    VIEW.with(|v| match v.borrow().as_ref() {
        Some((g, view)) if *g == generation => {
            // wry 的 `bounds()` 读取时不处理翻转坐标系的父视图(`set_bounds` 处理了),读出来的 y 是错的:
            // 这里直接读 NSView 的 frame,按父视图是否翻转自己换算成「从顶部算」的 y。
            let ns = view.webview();
            let frame = ns.frame();
            // SAFETY: 在窗口线程上读一个仍在句柄表里的、存活的 NSView 及其父视图的几何,只读。
            let bounds = unsafe { ns.superview() }.map(|parent| {
                let y = if parent.isFlipped() {
                    frame.origin.y
                } else {
                    parent.frame().size.height - frame.origin.y - frame.size.height
                };
                (frame.origin.x, y, frame.size.width, frame.size.height)
            });
            Probe {
                exists: true,
                visible: !view.webview().isHidden(),
                bounds,
                handles,
            }
        }
        _ => Probe {
            exists: false,
            visible: false,
            bounds: None,
            handles,
        },
    })
}

/// 销毁该 generation 的 webview(其他 generation 的不动)。
pub fn destroy(generation: u64) {
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        if v.as_ref().is_some_and(|(g, _)| *g == generation) {
            v.take();
        }
    });
}

fn with(generation: u64, f: impl FnOnce(&wry::WebView)) {
    VIEW.with(|v| {
        if let Some((g, view)) = v.borrow().as_ref()
            && *g == generation
        {
            f(view);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉死的设置:读本文件**生产源码**(`#[cfg(test)]` 之前),测的是真实构建器而不是副本。
    const REQUIRED: &[&str] = &[
        ".with_devtools(false)",
        ".with_download_started_handler(|_, _| false)",
        ".with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)",
        ".with_navigation_handler(|url| url == \"about:blank\")",
        ".with_clipboard(true)",
        ".with_initialization_script(init_script(nonce, mode))",
        ".with_ipc_handler(",
        ".with_html(HTML)",
    ];
    const FORBIDDEN: &[&str] = &[
        ".with_clipboard(false)",
        ".with_devtools(true)",
        ".with_url(",
        ".with_custom_protocol(",
        "file://",
        "build_app_child",
        "panel::build_child",
        "bytehost_desktop::",
        "bytehost_webview",
        "bytehost_apps",
    ];

    fn production_source() -> &'static str {
        include_str!("host.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap()
    }

    fn check_pins(src: &str) -> Result<(), String> {
        for needle in REQUIRED {
            if !src.contains(needle) {
                return Err(format!("缺少钉死的设置:{needle}"));
            }
        }
        for needle in FORBIDDEN {
            // 文档注释里提到被禁用的名字是允许的:只查代码行
            if src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(needle))
            {
                return Err(format!("出现了被禁用的东西:{needle}"));
            }
        }
        Ok(())
    }

    #[test]
    fn the_builder_pins_every_setting() {
        check_pins(production_source()).unwrap();
    }

    #[test]
    fn relaxing_any_one_pin_fails_the_check() {
        let src = production_source();
        for needle in REQUIRED {
            assert!(
                check_pins(&src.replace(needle, "")).is_err(),
                "removing {needle} must be caught"
            );
        }
        for (from, to) in [
            (".with_devtools(false)", ".with_devtools(true)"),
            (
                ".with_download_started_handler(|_, _| false)",
                ".with_download_started_handler(|_, _| true)",
            ),
            (
                "wry::NewWindowResponse::Deny",
                "wry::NewWindowResponse::Allow",
            ),
            (
                ".with_navigation_handler(|url| url == \"about:blank\")",
                ".with_navigation_handler(|_| true)",
            ),
        ] {
            assert!(
                check_pins(&src.replace(from, to)).is_err(),
                "relaxing {from} must be caught"
            );
        }
    }

    #[test]
    fn pinning_the_clipboard_off_fails_the_check_too() {
        let src = production_source().replace(".with_clipboard(true)", ".with_clipboard(false)");
        let err = check_pins(&src).unwrap_err();
        assert!(err.contains("with_clipboard"), "{err}");
    }

    #[test]
    fn the_init_script_freezes_the_nonce_and_mode() {
        let js = init_script("abc", Mode::Normal);
        assert_eq!(
            js,
            r#"window.__DIGGER__ = Object.freeze({ nonce: "abc", mode: "app" });"#
        );
        assert!(init_script("abc", Mode::SelfTest).contains(r#"mode: "selftest""#));
    }

    #[test]
    fn the_embedded_page_reads_the_nonce_the_host_injects() {
        assert!(
            HTML.contains("__DIGGER__"),
            "the bundle must read window.__DIGGER__"
        );
        assert!(!HTML.contains("localhost"), "no network origin in the page");
    }
}
