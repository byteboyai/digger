//! Group-chat's single trusted native child. The shared executor alone owns wry
//! handles; this module owns the page-specific part: protocol answers, IPC origin
//! checks, readiness and generation bookkeeping.
use bytehost_core::LogicalRect;
pub use bytehost_desktop::native_child::Probe;
use bytehost_desktop::native_child::{self, Action, Host};
use dozer_panel_group_chat::{
    GroupChatWebviewEvent,
    assets::{group_chat_webview_spec, serve_group_chat_content},
    parse_group_chat_event,
};
use iced::{Task, window};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const SCHEME: &str = "digger-groupchat";
pub const HOST: &str = "group-chat-content";
const THEME: &str = "dark";
/// Page URLs are short; reject anything longer before looking at it.
const MAX_PAGE_URL_BYTES: usize = 2048;
/// Larger than any post the library would accept (20 000 chars) plus JSON framing;
/// only tightens the library's own limits, never relaxes them.
const MAX_EVENT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct Desired {
    /// Panel selected, project present, not closing, not failed.
    pub shown: bool,
    /// An iced overlay is open: keep the page alive but parked and hidden.
    pub overlay_open: bool,
    pub rect: LogicalRect,
    pub project_id: Option<i64>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Built(u64, Result<(), String>),
    /// An event the page sent, already origin-checked and parsed by the library.
    Page(u64, GroupChatWebviewEvent),
    /// A page event that failed the library's parser (reported, never executed).
    Rejected(u64, String),
    Failed(u64, String),
    Probed(u64, Probe),
    Done,
}

struct Live {
    project_id: i64,
    desired: native_child::Desired,
    generation: u64,
    built: bool,
    ready: bool,
}

fn host_desired(d: &Desired, ready: bool) -> Option<native_child::Desired> {
    if !d.shown {
        return None;
    }
    let pid = d.project_id?;
    let spec = group_chat_webview_spec(false, ready, SCHEME, THEME)?;
    Some(native_child::Desired {
        identity: format!("group-chat:{pid}"),
        url: spec.url,
        rect: LogicalRect {
            x: d.rect.x.max(0.),
            y: d.rect.y.max(0.),
            width: d.rect.width.max(0.),
            height: d.rect.height.max(0.),
        },
        scale: f64::from(byteui::theme::icon_size::scale()),
        // Geometry clamps unusable blocks to a 1px sentinel: never show those.
        visible: !d.overlay_open && d.rect.width > 1. && d.rect.height > 1.,
        loaded: ready,
    })
}

pub fn allows_page(url: &str) -> bool {
    url.len() <= MAX_PAGE_URL_BYTES && native_child::allows_navigation(SCHEME, HOST, url)
}

/// Classify one IPC message from the page. `None` means "drop silently".
fn classify(page_url: &str, body: &str) -> Option<Result<GroupChatWebviewEvent, String>> {
    if !allows_page(page_url) {
        return None;
    }
    if body.len() > MAX_EVENT_BYTES {
        return Some(Err("群聊页面事件过大".into()));
    }
    Some(parse_group_chat_event(body))
}

fn serve(path: &str) -> (u16, &'static str, &'static [u8]) {
    match serve_group_chat_content(path) {
        Some((mime, body)) => (200, mime, body),
        None => (404, "text/plain", &[]),
    }
}

pub struct GroupChatNative {
    #[cfg(test)]
    pub(crate) scheduled_actions: Vec<String>,
    /// Rect and visibility of the most recent desired state (planned, not live).
    #[cfg(test)]
    pub(crate) scheduled_target: Option<(LogicalRect, bool)>,
    last_probe: Option<Probe>,
    host: Host,
    epoch: Arc<AtomicU64>,
    live: Option<Live>,
    error: Option<String>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Event>>,
}

impl Default for GroupChatNative {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupChatNative {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let epoch = Arc::new(AtomicU64::new(0));
        Self {
            #[cfg(test)]
            scheduled_actions: vec![],
            #[cfg(test)]
            scheduled_target: None,
            last_probe: None,
            host: Host::new(epoch.clone()),
            epoch,
            live: None,
            error: None,
            tx,
            rx: Some(rx),
        }
    }
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }
    pub fn failed(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    /// The project whose page is alive: events belong to it, not to whatever
    /// project happens to be focused now.
    pub fn project_id(&self) -> Option<i64> {
        self.live
            .as_ref()
            .filter(|l| l.generation == self.epoch.load(Ordering::SeqCst))
            .map(|l| l.project_id)
    }
    #[cfg(test)]
    pub(crate) fn generation(&self) -> Option<u64> {
        self.live.as_ref().map(|l| l.generation)
    }
    pub fn last_probe(&self) -> Option<Probe> {
        self.last_probe
    }
    pub fn probe(&self, id: window::Id) -> Task<Event> {
        let generation = self.epoch.load(Ordering::SeqCst);
        self.host.probe(id, generation, move |probe| {
            Event::Probed(generation, probe)
        })
    }
    /// A live, built child of the current generation that `desired` would keep
    /// (same project, still shown). Scripts may only be injected into such a child.
    pub fn is_available_for(&self, desired: &Desired) -> bool {
        let Some(live) = self.live.as_ref() else {
            return false;
        };
        live.built
            && live.generation == self.epoch.load(Ordering::SeqCst)
            && desired.shown
            && desired.project_id == Some(live.project_id)
    }
    pub fn is_ready(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|l| l.ready && l.generation == self.epoch.load(Ordering::SeqCst))
    }
    /// Invalidate every in-flight result of the current generation.
    pub fn invalidate(&mut self) {
        self.last_probe = None;
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
    /// Validate an event against the current generation. Returns the page event
    /// only when it belongs to the live child.
    pub fn update(&mut self, event: Event) -> Option<GroupChatWebviewEvent> {
        if let Event::Probed(generation, probe) = event {
            if generation == self.epoch.load(Ordering::SeqCst) {
                self.last_probe = Some(probe);
            }
            return None;
        }
        let live = self.live.as_mut()?;
        if live.generation != self.epoch.load(Ordering::SeqCst) {
            return None;
        }
        match event {
            Event::Built(g, Ok(())) if g == live.generation => {
                live.built = true;
                None
            }
            Event::Page(g, page) if g == live.generation => {
                if matches!(page, GroupChatWebviewEvent::Ready) {
                    live.ready = true;
                }
                Some(page)
            }
            Event::Rejected(g, error) | Event::Built(g, Err(error)) | Event::Failed(g, error)
                if g == live.generation =>
            {
                eprintln!("digger: group-chat: {error}");
                self.error = Some(error);
                None
            }
            _ => None,
        }
    }

    fn plan(&self, desired: &Desired) -> Vec<Action> {
        let current = self
            .live
            .as_ref()
            .filter(|l| l.generation == self.epoch.load(Ordering::SeqCst));
        // Readiness belongs to the page that said it: a page for another project is a
        // new document and starts parked.
        let ready = current.is_some_and(|l| l.ready && desired.project_id == Some(l.project_id));
        let target = host_desired(desired, ready);
        match (self.live.as_ref(), current) {
            // A stale child must be replaced, never reused.
            (Some(_), None) => {
                let mut actions = vec![Action::Destroy];
                actions.extend(native_child::plan(None, target.as_ref()));
                actions
            }
            _ => native_child::plan(current.map(|l| &l.desired), target.as_ref()),
        }
    }

    pub fn sync(
        &mut self,
        id: window::Id,
        desired: Desired,
        script: Option<String>,
    ) -> Task<Event> {
        let actions = self.plan(&desired);
        #[cfg(test)]
        {
            self.scheduled_actions
                .extend(actions.iter().map(|a| format!("{a:?}")));
            self.scheduled_target = host_desired(
                &desired,
                self.live
                    .as_ref()
                    .is_some_and(|l| l.ready && desired.project_id == Some(l.project_id)),
            )
            .map(|d| (d.rect, d.visible));
        }
        let mut tasks = Vec::new();
        if actions.contains(&Action::Destroy)
            && let Some(old) = self.live.take()
        {
            self.last_probe = None;
            self.epoch.fetch_add(1, Ordering::SeqCst);
            self.error = None;
            tasks.push(self.host.destroy(id, old.generation, Event::Done));
        }
        let Some(mut target) = host_desired(&desired, false) else {
            return Task::batch(tasks);
        };
        if self.live.is_none() {
            let tx = self.tx.clone();
            self.last_probe = None;
            let generation = self.epoch.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
            target.loaded = false;
            self.live = Some(Live {
                project_id: desired.project_id.expect("validated host project"),
                desired: target.clone(),
                generation,
                built: false,
                ready: false,
            });
            self.error = None;
            tasks.push(self.host.create(
                id,
                generation,
                native_child::Document {
                    scheme: SCHEME,
                    host: HOST,
                    desired: target,
                },
                move |builder| {
                    let pages = tx.clone();
                    builder
                        .with_custom_protocol(SCHEME.into(), move |_, request| {
                            let (status, mime, body) = serve(request.uri().path());
                            wry::http::Response::builder()
                                .status(status)
                                .header("Content-Type", mime)
                                .body(std::borrow::Cow::Borrowed(body))
                                .expect("fixed group-chat response")
                        })
                        .with_ipc_handler(move |request| {
                            let page_url = request.uri().to_string();
                            match classify(&page_url, request.body()) {
                                None => eprintln!(
                                    "digger: group-chat: IPC from a foreign origin dropped"
                                ),
                                Some(Ok(event)) => {
                                    let _ = pages.send(Event::Page(generation, event));
                                }
                                Some(Err(error)) => {
                                    let _ = pages.send(Event::Rejected(
                                        generation,
                                        format!("群聊页面事件解析失败: {error}"),
                                    ));
                                }
                            }
                        })
                },
                Event::Built,
                Event::Done,
            ));
        } else if let Some(live) = self.live.as_mut() {
            target.loaded = live.ready;
            live.desired = target.clone();
            let generation = live.generation;
            tasks.push(self.host.sync(
                id,
                generation,
                target,
                script,
                |generation, result| match result {
                    Err(error) => Event::Failed(generation, error),
                    Ok(()) => Event::Done,
                },
                Event::Done,
            ));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
#[path = "group_chat_native_tests.rs"]
mod tests;
