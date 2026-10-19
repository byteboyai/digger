//! 预览宿主示例(S5b-2 P3 Task 8):`cargo run -p digger-desktop --example preview_smoke`
//!
//! 不接产品壳——只把 P3 的宿主适配器拉到真实窗口里:左边文件列表,右边预览,
//! 顶部三个按钮(关闭当前页签 / 放大(藏起 webview)/ 全部销毁),底部状态行
//! 显示存活句柄数。
//!
//! `--selftest`:无交互地逐项打开夹具,等 host 就绪,断言存活句柄数与期望一致,
//! 末尾销毁全部并再次断言;任何一步不符即非零退出。用于在无人盯屏时回归。
//!
//! 面板区域 = 整块 webview 矩形(顶栏高度留给产品壳,这里不画)。

#[path = "../src/adapters/preview_native/mod.rs"]
mod preview_native;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytehost_core::LogicalRect;
use dozer_panel_preview::host_ipc::IpcMessage;
use dozer_panel_preview::{PreviewPane, PreviewPanelState, TabKind, ViewerPanel};
use iced::{Element, Task, window};

const TOP_BAR: f64 = 48.0;
const STATUS_BAR: f64 = 26.0;
const WINDOW: iced::Size = iced::Size::new(1100.0, 760.0);

/// 自检用的夹具。`.py`/`.md`/`.html` 是真正会建 webview 的类型;`.json`/`.csv`
/// 在 Digger 的能力下走原生降级页,不应产生句柄。
fn fixtures() -> Vec<(PathBuf, &'static str)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let dozer = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../dozer");
    [
        (
            "docs/superpowers/specs/2026-10-18-preview-engine-s5b2-design.md",
            "md",
        ),
        ("README.md", "md"),
        (
            "../dozer/crates/dozer-panel-preview/assets/html_host.html",
            "html",
        ),
    ]
    .iter()
    .map(|(p, kind)| (root.join(p), *kind))
    .chain(std::iter::once((
        dozer.join("crates/dozer-panel-preview/assets/flyfish/host.html"),
        "html",
    )))
    .filter(|(p, _)| p.exists())
    .collect()
}

/// 名字里带空格、`#`、中日文的夹具:走 `file URL → dozer:// → 白名单` 全程,
/// 钉住 Task 8 Step 3 的那一条(临时文件,进程退出后由调用方清理)。
fn awkward_fixture() -> Option<(PathBuf, &'static str)> {
    let path = std::env::temp_dir().join("digger preview 空格 #1 中文.md");
    std::fs::write(&path, "# 标题\n\n带空格、井号与中文。\n").ok()?;
    Some((path, "md"))
}

#[derive(Debug, Clone)]
enum Msg {
    Ready(window::Id),
    Resized(iced::Size),
    ListPicked(PathBuf),
    CloseCurrent,
    Maximize,
    DestroyAll,
    Ipc(preview_native::Event),
    /// 自检心跳:状态机只在收到消息时推进,而"句柄已建但还没发 Ready"
    /// 这类中间态不会有事件,靠它把检查再跑一遍。
    Tick,
}

struct App {
    window: Option<window::Id>,
    size: (f64, f64),
    host: Arc<preview_native::Host>,
    state: PreviewPanelState,
    list: Vec<(PathBuf, &'static str)>,
    selected: Option<PathBuf>,
    /// 右侧是否让位给"放大"——这是一个纯 UI 开关(真产品里由配对布局决定),
    /// 宿主侧只负责把对应 webview 藏起。
    maximized: bool,
}

fn build() -> (App, Task<Msg>) {
    let mut list = fixtures();
    if let Some(awkward) = awkward_fixture() {
        list.push(awkward);
    }
    let mut pane = PreviewPane::default();
    for (path, _) in &list {
        pane.open_path_provisional(path.clone());
    }
    let host = preview_native::Host::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../dozer/crates/dozer-panel-preview/assets/flyfish"
    ));
    // 预览里的编辑器要能读回原文件(保存/冲突检测走同一白名单)。
    for tab in pane.tabs() {
        if let TabKind::File(path) = &tab.kind {
            host.allow_file(path);
        }
    }
    *IPC_RX.lock().expect("rx 锁") = host.take_receiver();
    let host = Arc::new(host);
    let selected = list.first().map(|(p, _)| p.clone());
    let app = App {
        window: None,
        size: (WINDOW.width as f64, WINDOW.height as f64),
        host,
        state: PreviewPanelState::new(pane),
        list,
        selected,
        maximized: false,
    };
    let task = window::open(window::Settings {
        size: WINDOW,
        ..Default::default()
    })
    .1
    .map(Msg::Ready);
    (app, task)
}

impl App {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ready(id) => self.window = Some(id),
            Msg::Resized(size) => {
                self.size = (size.width as f64, size.height as f64);
            }
            Msg::ListPicked(path) => self.open_file(path),
            Msg::CloseCurrent => {
                let active = self.state.pane.active_idx();
                if let Some(id) = self.state.pane.tabs().get(active).map(|t| t.id) {
                    self.state.pane.close_by_id(id);
                }
            }
            Msg::Maximize => self.maximized = !self.maximized,
            Msg::DestroyAll => self.host.destroy_all(),
            Msg::Tick => {}
            Msg::Ipc(preview_native::Event::Ipc { ref message, .. })
                if !matches!(message, IpcMessage::ZoomIn | IpcMessage::ZoomOut) =>
            {
                eprintln!("preview_smoke: {msg:?}"); // cli-output
            }
            Msg::Ipc(_) => {}
        }
        self.sync()
    }

    fn open_file(&mut self, path: PathBuf) {
        self.host.allow_file(&path);
        self.selected = Some(path.clone());
        self.state.pane.open_path_provisional(path);
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
        // 放大时整块被 iced 内容盖住,宿主把 webview 全部藏起(不是销毁)。
        let visible = !self.maximized;
        let desired =
            preview_native::desired(&self.state.pane, 0, ViewerPanel::Files, "en", |_| {
                Some(rect)
            });
        let desired = if visible {
            desired
        } else {
            desired
                .into_iter()
                .map(|(spec, rect)| {
                    let mut spec = spec;
                    spec.visible = false;
                    (spec, rect)
                })
                .collect()
        };
        let host = Arc::clone(&self.host);
        window::run(id, move |w| host.sync(w, &desired)).discard()
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::{button, column, container, row, scrollable, text};
        use iced::{Length, Theme};

        let title = format!(
            "preview_smoke  live={}  tabs={}  maximized={}",
            self.host.live_count(),
            self.state.pane.tabs().len(),
            self.maximized,
        );

        let toolbar = row![
            text(title).width(Length::Fill),
            button("关闭当前页签").on_press(Msg::CloseCurrent),
            button(if self.maximized {
                "还原"
            } else {
                "放大(藏起)"
            })
            .on_press(Msg::Maximize),
            button("全部销毁").on_press(Msg::DestroyAll),
        ]
        .spacing(8)
        .padding(8);

        let mut files = column![].spacing(2).padding(8);
        for (path, kind) in &self.list {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let label = format!("[{kind}] {name}");
            let mut b = button(text(label)).width(Length::Fill);
            if self.selected.as_deref() == Some(path.as_path()) {
                b = b.style(button::primary);
            }
            files = files.push(b.on_press(Msg::ListPicked(path.clone())));
        }

        let body: Element<'_, Msg> = if self.maximized {
            container(
                column![
                    text("放大:预览 webview 已被藏起(仍存活)"),
                    text(format!("live={}", self.host.live_count())),
                ]
                .spacing(6)
                .padding(16),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(text(format!(
                "预览区 … (webview 由宿主绘制,live={})",
                self.host.live_count()
            )))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        let content = row![
            container(scrollable(files)).width(Length::FillPortion(1)),
            container(body).width(Length::FillPortion(3)),
        ]
        .spacing(1)
        .height(Length::Fill);

        container(column![
            toolbar,
            content,
            container(text(format!("宿主存活句柄 {}", self.host.live_count())))
                .height(iced::Length::Fixed(STATUS_BAR as f32)),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Theme::Dark.palette().background.into()),
            ..Default::default()
        })
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

fn subscription(_: &App) -> iced::Subscription<Msg> {
    iced::Subscription::batch([
        iced::Subscription::run(ipc_stream),
        window::resize_events().map(|(_, size)| Msg::Resized(size)),
    ])
}

fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--selftest") {
        return selftest();
    }
    iced::application(build, update, view)
        .title("Digger preview smoke")
        .window(window::Settings {
            size: WINDOW,
            ..Default::default()
        })
        .subscription(subscription)
        .run()
}

// ---------------------------------------------------------------------------
// `--selftest`
// ---------------------------------------------------------------------------

/// 无 iced 事件循环的自检:`sync` 依赖窗口线程,所以这里用 `wry` 之外的裸窗口
/// 起不了;改为**每个夹具单独打开 + 在真实事件循环里推进若干帧**,以 IPC 的
/// `Ready` 作为"建好了"的信号。
///
/// 实现上跑一个最小 iced 应用:进入事件循环后按状态机推进,结束时以退出码
/// 汇报。这样 `window::run` 与产品同路径。
fn selftest() -> iced::Result {
    eprintln!("selftest: 启动"); // cli-output
    let mut list = fixtures();
    if let Some(awkward) = awkward_fixture() {
        list.push(awkward);
    }
    assert!(!list.is_empty(), "selftest: 没有可用夹具");

    let mut pane = PreviewPane::default();
    let host = preview_native::Host::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../dozer/crates/dozer-panel-preview/assets/flyfish"
    ));
    for (path, _) in &list {
        host.allow_file(path);
        pane.open_path_provisional(path.clone());
    }
    *IPC_RX.lock().expect("rx 锁") = host.take_receiver();
    let host = Arc::new(host);

    let app = SelfTest {
        step: 0,
        window: None,
        size: (WINDOW.width as f64, WINDOW.height as f64),
        host,
        state: PreviewPanelState::new(pane),
        list,
        ready: std::collections::BTreeSet::new(),
        deadline: Instant::now() + Duration::from_secs(30),
        failure: None,
    };

    eprintln!("selftest: 夹具 {} 个,进入事件循环", app.list.len()); // cli-output
    let slot = Mutex::new(Some(app));
    iced::application(
        move || {
            eprintln!("selftest: boot"); // cli-output
            let app = slot
                .lock()
                .expect("selftest 锁")
                .take()
                .expect("只 boot 一次");
            (
                app,
                window::open(window::Settings {
                    size: WINDOW,
                    ..Default::default()
                })
                .1
                .map(Msg::Ready),
            )
        },
        selftest_update,
        selftest_view,
    )
    .title("Digger preview selftest")
    .subscription(|_: &SelfTest| {
        iced::Subscription::batch([
            iced::Subscription::run(ipc_stream),
            window::resize_events().map(|(_, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(100)).map(|_| Msg::Tick),
        ])
    })
    .run()
}

fn selftest_update(app: &mut SelfTest, msg: Msg) -> Task<Msg> {
    app.update(msg)
}

fn selftest_view(app: &SelfTest) -> Element<'_, Msg> {
    app.view()
}

struct SelfTest {
    step: usize,
    window: Option<window::Id>,
    size: (f64, f64),
    host: Arc<preview_native::Host>,
    state: PreviewPanelState,
    list: Vec<(PathBuf, &'static str)>,
    ready: std::collections::BTreeSet<usize>,
    deadline: Instant,
    failure: Option<String>,
}

impl SelfTest {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ready(id) => self.window = Some(id),
            Msg::Resized(size) => self.size = (size.width as f64, size.height as f64),
            Msg::Ipc(preview_native::Event::Ipc {
                webview_id,
                message: IpcMessage::Host(_) | IpcMessage::FocusedEnvelope,
            }) => {
                self.ready.insert(webview_id);
            }
            Msg::Ipc(preview_native::Event::Ipc { .. }) => {}
            Msg::Ipc(preview_native::Event::BuildFailed { error, .. }) => {
                self.failure = Some(format!("host 建失败:{error}"));
            }
            Msg::Tick => {}
            _ => {}
        }
        let sync = self.sync();
        Task::batch([sync, self.advance()])
    }

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

    /// 每个"拍"检查一次期望;不满足就等下一拍(由 IPC/窗口事件驱动)。
    fn advance(&mut self) -> Task<Msg> {
        if let Some(reason) = &self.failure {
            eprintln!("selftest: FAIL {reason}"); // cli-output
            std::process::exit(1);
        }
        if Instant::now() > self.deadline {
            let step = self.step;
            self.failure = Some(format!(
                "第 {step} 步超时(live={} expected={} ready={})",
                self.host.live_count(),
                self.expected_handles(),
                self.ready.len()
            ));
            return Task::none();
        }
        let expected = self.expected_handles();
        let actual = self.host.live_count();
        if actual != expected {
            return Task::none();
        }
        if self.step == 0 {
            // 句柄数相等只证明 Create 执行了;还要至少收到一条 host IPC,
            // 才证明协议应答 + 注入脚本 + classify_ipc 整条通路是通的。
            if self.ready.is_empty() {
                return Task::none();
            }
            eprintln!("selftest: 第 0 步 ok(live={actual})"); // cli-output
            self.step = 1;
            self.host.destroy_all();
            return Task::done(Msg::Resized(iced::Size::new(
                self.size.0 as f32,
                self.size.1 as f32,
            )));
        }
        if self.step == 1 {
            eprintln!("selftest: 第 1 步 ok(全部销毁后 live=0)"); // cli-output
            self.step = 2;
            return Task::none();
        }
        eprintln!("selftest: PASS"); // cli-output
        std::process::exit(0);
    }

    fn expected_handles(&self) -> usize {
        self.state
            .pane
            .desired_webviews(0, ViewerPanel::Files, "en")
            .len()
    }

    fn view(&self) -> Element<'_, Msg> {
        iced::widget::text(format!(
            "selftest step {} live={}",
            self.step,
            self.host.live_count()
        ))
        .into()
    }
}
