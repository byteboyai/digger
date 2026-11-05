//! Trusted Usage content adapter. Assets are embedded from the pinned shared panel,
//! and its UsageSurface owns readiness, generation checks and payload deduplication.
use bytehost_core::LogicalRect;
use dozer_panel_usage::{UsageSurface, WorkspaceState};
use iced::{Task, window};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use bytehost_desktop::native_child::{self, Host};

#[derive(Debug, Clone)]
pub enum Event {
    Ready(u64),
    Built(u64, Result<(), String>),
    Done,
    /// 生命周期自检读到的真实 view 状态。
    Probed(Probe),
}

pub use bytehost_desktop::native_child::Probe;

pub struct Native {
    surface: UsageSurface,
    host: Host,
    epoch: Arc<AtomicU64>,
    live: bool,
    theme: &'static str,
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
        let epoch = Arc::new(AtomicU64::new(0));
        Self {
            host: Host::new(epoch.clone()),
            surface: UsageSurface::new(2048, dispatch),
            epoch,
            live: false,
            theme: crate::settings::theme::code(),
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
        self.host.probe(id, generation, Event::Probed)
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }

    /// 作用域切换 / 关闭时立即把期望载荷置为空态。
    ///
    /// 只清 Rust 状态不够:若下一次 `sync` 之前 webview 仍持有旧载荷,旧项目的
    /// 图表会继续显示。这里把空载荷写进 Surface,后续同步会按正常推送语义送出去。
    pub fn clear_payload(&mut self) {
        self.surface.set_desired(dozer_model::Stamped::new(
            crate::settings::language::stamp(),
            dozer_panel_usage::current_view_payload(&WorkspaceState::default()),
        ));
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
    fn desired(&self, rect: LogicalRect) -> native_child::Desired {
        native_child::Desired {
            identity: "usage".into(),
            url: format!("digger-usage://localhost/host.html?theme={}", self.theme),
            rect,
            scale: f64::from(byteui::theme::icon_size::scale()),
            visible: true,
            loaded: true,
        }
    }

    pub fn sync(
        &mut self,
        id: window::Id,
        visible: bool,
        rect: LogicalRect,
        state: &WorkspaceState,
    ) -> Task<Event> {
        // 主题属于文档身份；重建后重新握手与推送，不能沿用旧页面的去重状态。
        let theme = crate::settings::theme::code();
        if self.live && self.theme != theme {
            self.theme = theme;
            let destroy = self.sync(id, false, rect, state);
            return destroy.chain(self.sync(id, visible, rect, state));
        }
        self.theme = theme;
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
            return self.host.destroy(id, generation, Event::Done);
        }
        self.surface.set_desired(dozer_model::Stamped::new(
            crate::settings::language::stamp(),
            dozer_panel_usage::current_view_payload(state),
        ));
        if !self.live {
            self.live = true;
            self.bounds = Some(rect);
            self.surface.observe_instance(true, false);
            let generation = self.epoch.load(Ordering::SeqCst);
            let tx = self.tx.clone();
            return self.host.create(
                id,
                generation,
                native_child::Document {
                    scheme: "digger-usage",
                    host: "localhost",
                    desired: self.desired(rect),
                },
                move |builder| {
                    builder
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
                },
                Event::Built,
                Event::Done,
            );
        }
        if self.error.is_some() {
            return Task::none();
        }
        self.bounds = Some(rect);
        let script = if self.ready {
            self.surface.take_pending_script().map(|(_, s)| s)
        } else {
            None
        };
        let generation = self.epoch.load(Ordering::SeqCst);
        self.host.sync(
            id,
            generation,
            self.desired(rect),
            script,
            Event::Built,
            Event::Done,
        )
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn clearing_the_payload_replaces_the_old_scope_rows() {
        let mut native = Native::new();
        native.surface.observe_instance(true, false);
        native.surface.apply_ready();
        let mut state = WorkspaceState::default();
        dozer_panel_usage::update(&mut state, crate::adapters::usage::sample_loaded_message());
        native.surface.set_desired(dozer_model::Stamped::new(
            crate::settings::language::stamp(),
            dozer_panel_usage::current_view_payload(&state),
        ));
        let (_, loaded) = native
            .surface
            .take_pending_script()
            .expect("loaded payload must be pending");
        assert!(loaded.contains(r#"\"conversation_count\":1"#), "{loaded}");

        native.clear_payload();
        let (_, cleared) = native
            .surface
            .take_pending_script()
            .expect("cleared payload must be pending");
        assert!(cleared.contains(r#"\"kind\":\"empty\""#), "{cleared}");
        assert!(!cleared.contains("conversation_count"), "{cleared}");
        assert_ne!(loaded, cleared);
    }

    #[test]
    fn a_theme_change_rebuilds_the_document_and_waits_for_a_new_handshake() {
        let mut native = Native::new();
        native.live = true;
        native.ready = true;
        native.theme = if crate::settings::theme::code() == "dark" {
            "light"
        } else {
            "dark"
        };
        let before = native.epoch.load(Ordering::SeqCst);
        let rect = LogicalRect {
            x: 0.,
            y: 0.,
            width: 300.,
            height: 400.,
        };
        let _ = native.sync(window::Id::unique(), true, rect, &WorkspaceState::default());
        assert!(native.epoch.load(Ordering::SeqCst) > before);
        assert!(native.live);
        assert!(!native.ready, "旧页面 ready 不能沿用到新主题文档");
        assert!(
            native
                .desired(rect)
                .url
                .ends_with(crate::settings::theme::code())
        );
        let old_epoch = native.epoch.load(Ordering::SeqCst);
        let _ = native.sync(window::Id::unique(), true, rect, &WorkspaceState::default());
        assert_eq!(
            native.epoch.load(Ordering::SeqCst),
            old_epoch,
            "未变主题不重复重建"
        );
    }
}
