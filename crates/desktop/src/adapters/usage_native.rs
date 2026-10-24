//! Trusted Usage content adapter. Assets are embedded from the pinned shared panel,
//! and its UsageSurface owns readiness, generation checks and payload deduplication.
use bytehost_core::LogicalRect;
use dozer_panel_usage::{UsageSurface, WorkspaceState};
use iced::{Task, window};
use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

thread_local! {
    static VIEW: RefCell<Option<(u64, wry::WebView)>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(u64),
    Built(u64, Result<(), String>),
    Done,
    /// 生命周期自检读到的真实 view 状态。
    Probed(Probe),
}

/// 真实用量 webview 此刻的样子(生命周期自检读它:读的是 NSView 本身,不是我们自己的记账)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    pub exists: bool,
    pub visible: bool,
    /// NSView 的实际位置与大小(逻辑像素:x, y, 宽, 高;y 从顶部算)
    pub bounds: Option<(f64, f64, f64, f64)>,
    pub handles: usize,
}

fn probe_view(generation: u64) -> Probe {
    use wry::WebViewExtMacOS;
    let handles = VIEW.with(|v| usize::from(v.borrow().is_some()));
    VIEW.with(|v| match v.borrow().as_ref() {
        Some((g, view)) if *g == generation => {
            // wry 的 `bounds()` 读取时不处理翻转坐标系的父视图,读出来的 y 是错的:直接读 NSView 的 frame 自己换算。
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
                visible: !ns.isHidden(),
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

pub struct Native {
    surface: UsageSurface,
    epoch: Arc<AtomicU64>,
    live: bool,
    ready: bool,
    last_probe: Option<Probe>,
    bounds: Option<LogicalRect>,
    pub error: Option<String>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Event>>,
}

fn dispatch(json: &str) -> String {
    format!(
        "window.__dozer?.dispatch({});",
        serde_json::to_string(json).unwrap()
    )
}

impl Default for Native {
    fn default() -> Self {
        Self::new()
    }
}

impl Native {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            surface: UsageSurface::new(2048, dispatch),
            epoch: Arc::new(AtomicU64::new(0)),
            live: false,
            ready: false,
            last_probe: None,
            bounds: None,
            error: None,
            tx,
            rx: Some(rx),
        }
    }
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn last_probe(&self) -> Option<Probe> {
        self.last_probe
    }

    /// 读真实 webview 的状态(生命周期自检用),结果经 [`Event::Probed`] 回来。
    pub fn probe(&self, id: window::Id) -> Task<Event> {
        let generation = self.epoch.load(Ordering::SeqCst);
        window::run(id, move |_| Event::Probed(probe_view(generation)))
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }
    pub fn update(&mut self, event: Event) {
        match event {
            Event::Ready(g) if self.live && g == self.epoch.load(Ordering::SeqCst) => {
                self.ready = true;
                self.surface.apply_ready();
            }
            Event::Built(g, Err(error)) if self.live && g == self.epoch.load(Ordering::SeqCst) => {
                self.error = Some(error);
                self.surface.observe_instance(false, false);
            }
            Event::Probed(probe) => self.last_probe = Some(probe),
            _ => {}
        }
    }
    pub fn sync(
        &mut self,
        id: window::Id,
        visible: bool,
        rect: LogicalRect,
        state: &WorkspaceState,
    ) -> Task<Event> {
        if !visible {
            if !self.live {
                return Task::none();
            }
            let generation = self.epoch.fetch_add(1, Ordering::SeqCst);
            self.live = false;
            self.ready = false;
            self.error = None;
            self.bounds = None;
            self.surface.observe_instance(false, false);
            return window::run(id, move |_| {
                VIEW.with(|v| {
                    let mut v = v.borrow_mut();
                    if v.as_ref().is_some_and(|(g, _)| *g == generation) {
                        v.take();
                    }
                });
                Event::Done
            });
        }
        self.surface.set_desired(dozer_model::Stamped::new(
            // Digger 一期无语言切换:固定 `en` + 本地显示代 0。
            dozer_model::LocaleStamp {
                ui_locale: dozer_model::Locale::default_locale(),
                locale_revision: 0,
            },
            dozer_panel_usage::current_view_payload(state),
        ));
        if !self.live {
            self.live = true;
            self.bounds = Some(rect);
            self.surface.observe_instance(true, false);
            let generation = self.epoch.load(Ordering::SeqCst);
            let current = self.epoch.clone();
            let tx = self.tx.clone();
            return window::run(id, move |w| {
                if current.load(Ordering::SeqCst) != generation {
                    return Event::Done;
                }
                let result = wry::WebViewBuilder::new()
                    .with_url("digger-usage://localhost/host.html")
                    .with_bounds(bounds(rect))
                    .with_devtools(false)
                    .with_download_started_handler(|_, _| false)
                    .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                    .with_navigation_handler(|url| url.starts_with("digger-usage://localhost/"))
                    .with_custom_protocol("digger-usage".into(), |_, request| {
                        let (mime, body): (&str, &[u8]) = match request.uri().path() {
                            "/host.html" => (
                                "text/html; charset=utf-8",
                                include_bytes!(concat!(env!("OUT_DIR"), "/host.html")),
                            ),
                            "/usage-content.js" => (
                                "text/javascript; charset=utf-8",
                                include_bytes!(concat!(env!("OUT_DIR"), "/usage-content.js")),
                            ),
                            "/usage-content.css" => (
                                "text/css; charset=utf-8",
                                include_bytes!(concat!(env!("OUT_DIR"), "/usage-content.css")),
                            ),
                            _ => {
                                return wry::http::Response::builder()
                                    .status(404)
                                    .body(Vec::new().into())
                                    .unwrap();
                            }
                        };
                        wry::http::Response::builder()
                            .header("Content-Type", mime)
                            .body(body.to_vec().into())
                            .unwrap()
                    })
                    .with_ipc_handler(move |request| {
                        if dozer_panel_usage::parse_usage_event(request.body()).is_ok() {
                            let _ = tx.send(Event::Ready(generation));
                        }
                    })
                    .build_as_child(&w)
                    .map(|view| VIEW.with(|v| *v.borrow_mut() = Some((generation, view))))
                    .map_err(|e| e.to_string());
                Event::Built(generation, result)
            });
        }
        if self.error.is_some() {
            return Task::none();
        }
        let changed = self.bounds != Some(rect);
        self.bounds = Some(rect);
        let script = if self.ready {
            self.surface.take_pending_script().map(|(_, s)| s)
        } else {
            None
        };
        if !changed && script.is_none() {
            return Task::none();
        }
        let generation = self.epoch.load(Ordering::SeqCst);
        window::run(id, move |_| {
            VIEW.with(|v| {
                if let Some((g, view)) = v.borrow().as_ref()
                    && *g == generation
                {
                    if changed {
                        let _ = view.set_bounds(bounds(rect));
                    }
                    if let Some(script) = script {
                        let _ = view.evaluate_script(&script);
                    }
                }
            });
            Event::Done
        })
    }
}

fn bounds(rect: LogicalRect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(rect.x, rect.y).into(),
        size: wry::dpi::LogicalSize::new(rect.width, rect.height).into(),
    }
}
