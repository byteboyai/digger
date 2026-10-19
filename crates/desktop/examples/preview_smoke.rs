//! 预览宿主冒烟示例:`cargo run -p digger-desktop --example preview_smoke`
//!
//! 不接产品壳——只把 P3 的宿主适配器拉到真实窗口里,用仓库里的 Markdown /
//! HTML / 源码各一验证:清单 → 句柄、`dozer://` 协议应答、注入脚本、IPC 分类。
//!
//! 面板区域 = 整块 webview 矩形(顶栏高度留给产品壳,这里不画)。

#[path = "../src/adapters/preview_native/mod.rs"]
mod preview_native;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bytehost_core::LogicalRect;
use dozer_panel_preview::{PreviewPane, PreviewPanelState, TabKind, ViewerPanel};
use iced::{Element, Task, window};

const TOP_BAR: f64 = 48.0;
const WINDOW: iced::Size = iced::Size::new(1100.0, 760.0);

fn samples() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    [
        "docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md",
        "README.md",
        "../dozer/crates/dozer-panel-preview/assets/html_host.html",
    ]
    .iter()
    .map(|p| root.join(p))
    .filter(|p| p.exists())
    .collect()
}

#[derive(Debug, Clone)]
enum Msg {
    Ready(window::Id, iced::Size),
    Ipc(preview_native::Event),
}

struct App {
    window: Option<window::Id>,
    size: (f64, f64),
    host: Arc<preview_native::Host>,
    state: PreviewPanelState,
}

fn build() -> (App, Task<Msg>) {
    let mut pane = PreviewPane::default();
    for path in samples() {
        pane.open_path_provisional(path);
    }
    let host = preview_native::Host::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../dozer/crates/dozer-panel-preview/assets/flyfish"
    ));
    // 预览里的编辑器要能读回原文件(保存/冲突检测走同一白名单)。
    for tab in pane.tabs() {
        if let TabKind::File(path) = &tab.kind {
            host.allow_file(path.clone());
        }
    }
    *IPC_RX.lock().expect("rx 锁") = host.take_receiver();
    let host = Arc::new(host);
    let app = App {
        window: None,
        size: (WINDOW.width as f64, WINDOW.height as f64),
        host,
        state: PreviewPanelState::new(pane),
    };
    let task = window::open(window::Settings {
        size: WINDOW,
        ..Default::default()
    })
    .1
    .map(|id| Msg::Ready(id, WINDOW));
    (app, task)
}

impl App {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ready(id, size) => {
                self.window = Some(id);
                self.size = (size.width as f64, size.height as f64);
            }
            Msg::Ipc(event) => eprintln!("preview_smoke: {event:?}"), // cli-output
        }
        self.sync()
    }

    /// 把库的期望清单对齐到预览自己的句柄表。`desired` 先在这里(宿主线程之外)
    /// 算好,再送进 `window::run` 的 `'static` 闭包。
    fn sync(&mut self) -> Task<Msg> {
        let Some(id) = self.window else {
            return Task::none();
        };
        let rect = LogicalRect {
            x: 0.0,
            y: TOP_BAR,
            width: self.size.0,
            height: (self.size.1 - TOP_BAR).max(1.0),
        };
        let desired =
            preview_native::desired(&self.state.pane, 0, ViewerPanel::Files, "en", |_| {
                Some(rect)
            });
        let host = Arc::clone(&self.host);
        window::run(id, move |w| host.sync(w, &desired)).discard()
    }

    fn view(&self) -> Element<'_, Msg> {
        iced::widget::container(iced::widget::text("Digger preview smoke"))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    app.update(msg)
}

fn view(app: &App) -> Element<'_, Msg> {
    app.view()
}

/// 示例是单进程单窗口,IPC receiver 放进进程级静态槽,使订阅可以是无捕获
/// `fn() -> Stream`(`Subscription::run` 的唯一形态)。产品里应走自己的消息通道。
static IPC_RX: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<preview_native::Event>>> =
    Mutex::new(None);

fn ipc_stream() -> impl iced::futures::Stream<Item = Msg> {
    let rx = IPC_RX.lock().expect("rx 锁").take();
    iced::futures::stream::unfold(rx, |rx| async move {
        let mut rx = rx?;
        rx.recv().await.map(|event| (Msg::Ipc(event), Some(rx)))
    })
}

fn main() -> iced::Result {
    iced::application(build, update, view)
        .title("Digger preview smoke")
        .window(window::Settings {
            size: WINDOW,
            ..Default::default()
        })
        .subscription(|_| iced::Subscription::run(ipc_stream))
        .run()
}
