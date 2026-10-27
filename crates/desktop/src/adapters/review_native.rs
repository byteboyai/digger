//! Review's single trusted native child. The shared executor alone owns wry handles.
use super::native_host::{self, Action, Host};
use bytehost_core::LogicalRect;
use dozer_panel_conversations::{
    assets::{parse_review_trace_event, serve_review_trace},
    snapshot::ReviewWebviewSpec,
};
use iced::{Task, window};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
type Snapshot = Arc<Mutex<Option<String>>>;
#[derive(Clone, Debug)]
pub struct Desired {
    pub visible: bool,
    pub rect: LogicalRect,
    pub spec: Option<ReviewWebviewSpec>,
    pub project_id: Option<i64>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Built(u64, Result<(), String>),
    Loaded {
        generation: u64,
        nonce: u64,
        page_url: String,
    },
    Failed(u64, String),
    DocumentFailed {
        generation: u64,
        page_url: String,
        error: String,
    },
    Done,
}
pub struct Live {
    project_id: i64,
    desired: native_host::Desired,
    generation: u64,
    nonce: u64,
    loaded: bool,
}
fn host_desired(d: &Desired, loaded: bool) -> Option<native_host::Desired> {
    if !d.visible {
        return None;
    }
    let spec = d.spec.as_ref()?;
    let pid = d.project_id?;
    Some(native_host::Desired {
        identity: format!("review:{pid}"),
        url: spec.url.clone(),
        rect: LogicalRect {
            x: d.rect.x.max(0.),
            y: d.rect.y.max(0.),
            width: d.rect.width.max(0.),
            height: d.rect.height.max(0.),
        },
        scale: f64::from(byteui::theme::icon_size::scale()),
        visible: true,
        loaded,
    })
}
pub(super) fn plan(current: Option<&Live>, desired: &Desired) -> Vec<Action> {
    let target = host_desired(
        desired,
        current.is_some_and(|c| {
            c.loaded
                && desired.project_id == Some(c.project_id)
                && desired
                    .spec
                    .as_ref()
                    .is_some_and(|s| s.url == c.desired.url)
        }),
    );
    native_host::plan(current.map(|c| &c.desired), target.as_ref())
}
pub fn allows_navigation(url: &str) -> bool {
    native_host::allows_navigation("digger-review", "review-trace", url)
}
// Read the raw query: permissive percent decoding and duplicate keys must never
// turn an unrelated page into the current document. Decimal u64 includes wrapped 0.
// Library document URLs are under 128 bytes; cap untrusted IPC URLs before parsing.
const MAX_PAGE_URL_BYTES: usize = 2048;
fn nonce(url: &str) -> Option<u64> {
    if url.len() > MAX_PAGE_URL_BYTES || !allows_navigation(url) || url.contains(['#', '%']) {
        return None;
    }
    let (_, query) = url.split_once('?')?;
    let mut found = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "_r" {
            if found.is_some()
                || value.is_empty()
                || value.len() > 20
                || !value.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            found = Some(value.parse().ok()?);
        } else if key.contains('%') || value.contains('%') {
            return None;
        }
    }
    found
}
pub fn loaded_matches(page_url: &str, expected_nonce: u64) -> bool {
    nonce(page_url) == Some(expected_nonce)
}
/// A generation owns its own indirection. Unlink it before assigning the next
/// project's handle; never clear the old WorkspaceState's cached snapshot itself.
#[derive(Clone, Default)]
struct Serving(Arc<Mutex<Option<Snapshot>>>);
impl Serving {
    fn set(&self, snapshot: Option<Snapshot>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = snapshot;
    }
    fn serve(&self, path: &str) -> Result<(u16, &'static str, Vec<u8>), String> {
        let slot = self.0.lock().map_err(|e| e.to_string())?;
        let json = slot
            .as_ref()
            .map(|s| s.lock().map(|v| v.clone()).map_err(|e| e.to_string()))
            .transpose()?
            .flatten();
        Ok(
            match serve_review_trace(path.trim_start_matches('/'), json.as_deref()) {
                Some((mime, body)) => (200, mime, body),
                None => (404, "text/plain", vec![]),
            },
        )
    }
}
pub struct ReviewNative {
    host: Host,
    epoch: Arc<AtomicU64>,
    live: Option<Live>,
    serving: Serving,
    error: Option<String>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Event>>,
}
impl Default for ReviewNative {
    fn default() -> Self {
        Self::new()
    }
}
impl ReviewNative {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let epoch = Arc::new(AtomicU64::new(0));
        Self {
            host: Host::new(epoch.clone()),
            epoch,
            live: None,
            serving: Serving::default(),
            error: None,
            tx,
            rx: Some(rx),
        }
    }
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }
    pub fn invalidate_project(&mut self, project_id: i64) {
        if self
            .live
            .as_ref()
            .is_some_and(|live| live.project_id == project_id)
        {
            self.invalidate();
        }
    }
    pub fn invalidate(&mut self) {
        self.serving.set(None);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
    pub fn failed(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn update(&mut self, event: Event) -> Option<i64> {
        let live = self.live.as_mut()?;
        if live.generation != self.epoch.load(Ordering::SeqCst) {
            return None;
        }
        match event {
            Event::Loaded {
                generation,
                nonce,
                page_url,
            } if generation == live.generation
                && nonce == live.nonce
                && page_url == live.desired.url
                && loaded_matches(&page_url, live.nonce) =>
            {
                live.loaded = true;
                self.error = None;
                Some(live.project_id)
            }
            Event::DocumentFailed {
                generation,
                page_url,
                error,
            } if generation == live.generation
                && page_url == live.desired.url
                && loaded_matches(&page_url, live.nonce) =>
            {
                live.loaded = false;
                eprintln!("digger: review: {error}");
                self.error = Some(error);
                None
            }
            Event::Built(g, Err(error)) | Event::Failed(g, error) if g == live.generation => {
                live.loaded = false;
                eprintln!("digger: review: {error}");
                self.error = Some(error);
                None
            }
            _ => None,
        }
    }
    pub fn sync(
        &mut self,
        id: window::Id,
        desired: Desired,
        snapshot: Option<Snapshot>,
    ) -> Task<Event> {
        let actions = if self
            .live
            .as_ref()
            .is_some_and(|l| l.generation != self.epoch.load(Ordering::SeqCst))
        {
            let mut a = vec![Action::Destroy];
            a.extend(plan(None, &desired));
            a
        } else {
            plan(self.live.as_ref(), &desired)
        };
        let replace = actions.contains(&Action::Destroy);
        let mut tasks = Vec::new();
        if replace {
            self.serving.set(None);
            let old = self.live.take().unwrap();
            self.epoch.fetch_add(1, Ordering::SeqCst);
            self.error = None;
            tasks.push(self.host.destroy(id, old.generation, Event::Done));
        }
        let Some(mut target) = host_desired(&desired, false) else {
            return Task::batch(tasks);
        };
        if self.live.is_none() {
            self.serving = Serving::default();
            self.serving.set(snapshot);
            let serving = self.serving.clone();
            let tx = self.tx.clone();
            let generation = self.epoch.load(Ordering::SeqCst);
            let Some(document_nonce) = nonce(&target.url) else {
                self.error = Some("审阅页面地址无效".into());
                eprintln!("digger: review: invalid document URL");
                return Task::batch(tasks);
            };
            self.live = Some(Live {
                project_id: desired.project_id.expect("validated host project"),
                desired: target.clone(),
                generation,
                nonce: document_nonce,
                loaded: false,
            });
            self.error = None;
            tasks.push(self.host.create(
                id,
                generation,
                native_host::Document {
                    scheme: "digger-review",
                    host: "review-trace",
                    desired: target,
                },
                move |builder| {
                    let errors = tx.clone();
                    builder
                        .with_custom_protocol("digger-review".into(), move |_, request| {
                            let response = match serving.serve(request.uri().path()) {
                                Ok(r) => r,
                                Err(error) => {
                                    let _ = errors.send(Event::Failed(
                                        generation,
                                        format!("审阅资源读取失败: {error}"),
                                    ));
                                    (500, "text/plain", vec![])
                                }
                            };
                            wry::http::Response::builder()
                                .status(response.0)
                                .header("Content-Type", response.1)
                                .body(response.2.into())
                                .expect("fixed review response")
                        })
                        .with_ipc_handler(move |request| {
                            let url = request.uri().to_string();
                            match parse_review_trace_event(request.body()) {
                                Ok(_) => {
                                    if let Some(nonce) = nonce(&url) {
                                        let _ = tx.send(Event::Loaded {
                                            generation,
                                            nonce,
                                            page_url: url,
                                        });
                                    }
                                }
                                Err(error) => {
                                    let _ = tx.send(Event::DocumentFailed {
                                        generation,
                                        page_url: url,
                                        error: format!("审阅页面事件解析失败: {error}"),
                                    });
                                }
                            }
                        })
                },
                Event::Built,
                Event::Done,
            ));
        } else if let Some(live) = self.live.as_mut() {
            self.serving.set(snapshot);
            if live.desired.url != target.url {
                live.loaded = false;
                live.nonce = nonce(&target.url).unwrap_or(0);
                self.error = None;
            }
            target.loaded = live.loaded;
            live.desired = target.clone();
            if self.error.is_none() {
                let page_url = target.url.clone();
                tasks.push(self.host.sync(
                    id,
                    live.generation,
                    target,
                    None,
                    move |generation, result| match result {
                        Err(error) => Event::DocumentFailed {
                            generation,
                            page_url,
                            error,
                        },
                        Ok(()) => Event::Done,
                    },
                    Event::Done,
                ));
            }
        }
        Task::batch(tasks)
    }
}
#[cfg(test)]
#[path = "review_native_tests.rs"]
mod tests;
