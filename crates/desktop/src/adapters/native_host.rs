//! Private main-thread child-webview executor shared by trusted product panels.
//! Only this module owns wry handles. Subscription-facing state stays in atomics/messages.
use bytehost_core::LogicalRect;
use iced::{Task, window};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Desired {
    /// Changing identity replaces the one child; changing URL navigates it in place.
    pub identity: String,
    pub url: String,
    pub rect: LogicalRect,
    pub scale: f64,
    pub visible: bool,
    pub loaded: bool,
}
/// Trusted document origin plus the first desired state of its child.
pub(super) struct Document {
    pub scheme: &'static str,
    pub host: &'static str,
    pub desired: Desired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Destroy,
    Create,
    Navigate,
    Zoom,
    SetBounds,
    Park,
    Show,
}

pub(super) fn plan(current: Option<&Desired>, desired: Option<&Desired>) -> Vec<Action> {
    let Some(d) = desired else {
        return current.map_or_else(Vec::new, |_| vec![Action::Destroy]);
    };
    if current.is_none_or(|c| c.identity != d.identity) {
        let mut actions = Vec::new();
        if current.is_some() {
            actions.push(Action::Destroy);
        }
        actions.extend([Action::Create, Action::Zoom]);
        actions.extend(if d.visible && d.loaded {
            vec![Action::SetBounds, Action::Show]
        } else {
            vec![Action::Park]
        });
        return actions;
    }
    let c = current.unwrap();
    let mut actions = Vec::new();
    let navigating = c.url != d.url;
    let showing = d.visible && d.loaded;
    let was_showing = c.visible && c.loaded;
    if (!showing && was_showing) || navigating {
        actions.push(Action::Park);
    }
    if navigating {
        actions.push(Action::Navigate);
    }
    if c.scale != d.scale {
        actions.push(Action::Zoom);
    }
    if showing && (!was_showing || c.rect != d.rect) {
        actions.push(Action::SetBounds);
    }
    if showing && !was_showing {
        actions.push(Action::Show);
    }
    actions
}

/// Compare the raw authority rather than a normalized/prefix hostname. This rejects
/// credentials, ports, percent-encoded authority, backslashes and control characters.
pub(super) fn allows_navigation(scheme: &str, host: &str, url: &str) -> bool {
    if url.chars().any(|c| c.is_control() || c == '\\') {
        return false;
    }
    let Some(rest) = url.strip_prefix(scheme).and_then(|s| s.strip_prefix("://")) else {
        return false;
    };
    rest.split_once('/')
        .is_some_and(|(authority, _)| authority == host)
}

struct Entry {
    generation: u64,
    view: wry::WebView,
}
thread_local! {
    static VIEWS: RefCell<BTreeMap<u64, Entry>> = const { RefCell::new(BTreeMap::new()) };
}
static NEXT_HOST: AtomicU64 = AtomicU64::new(1);
pub(super) struct Host {
    slot: u64,
    epoch: Arc<AtomicU64>,
    scheduled: Option<Desired>,
}
impl Host {
    pub fn new(epoch: Arc<AtomicU64>) -> Self {
        Self {
            slot: NEXT_HOST.fetch_add(1, Ordering::Relaxed),
            epoch,
            scheduled: None,
        }
    }
    pub fn destroy<E: Send + 'static>(
        &mut self,
        id: window::Id,
        generation: u64,
        done: E,
    ) -> Task<E> {
        self.scheduled = None;
        let slot = self.slot;
        window::run(id, move |_| {
            VIEWS.with(|views| {
                let mut views = views.borrow_mut();
                if views.get(&slot).is_some_and(|e| e.generation == generation)
                    && let Some(entry) = views.remove(&slot)
                {
                    restore_focus(&entry.view);
                }
            });
            done
        })
    }
    pub fn create<E, F>(
        &mut self,
        id: window::Id,
        generation: u64,
        document: Document,
        configure: F,
        built: fn(u64, Result<(), String>) -> E,
        done: E,
    ) -> Task<E>
    where
        E: Send + 'static,
        F: for<'a> FnOnce(wry::WebViewBuilder<'a>) -> wry::WebViewBuilder<'a> + Send + 'static,
    {
        let Document {
            scheme,
            host,
            desired,
        } = document;
        self.scheduled = Some(desired.clone());
        let slot = self.slot;
        let epoch = self.epoch.clone();
        window::run(id, move |window| {
            if epoch.load(Ordering::SeqCst) != generation {
                return done;
            }
            let result = VIEWS.with(|views| {
                let mut views = views.borrow_mut();
                if let Some(old) = views.remove(&slot) {
                    restore_focus(&old.view);
                }
                let builder = configure(wry::WebViewBuilder::new())
                    .with_url(&desired.url)
                    .with_bounds(if desired.loaded {
                        bounds(desired.rect)
                    } else {
                        parked_bounds(desired.rect)
                    })
                    .with_visible(desired.visible)
                    .with_devtools(false)
                    .with_download_started_handler(|_, _| false)
                    .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                    .with_navigation_handler(move |url| allows_navigation(scheme, host, &url));
                let view = builder.build_as_child(&window).map_err(|e| e.to_string())?;
                view.zoom(desired.scale).map_err(|e| e.to_string())?;
                views.insert(slot, Entry { generation, view });
                Ok(())
            });
            built(generation, result)
        })
    }
    pub fn sync<E: Send + 'static>(
        &mut self,
        id: window::Id,
        generation: u64,
        desired: Desired,
        script: Option<String>,
        done: E,
    ) -> Task<E> {
        // URL changes start a new document. A previous document's loaded bit must
        // never reveal old content while its replacement is loading.
        let mut desired = desired;
        if self
            .scheduled
            .as_ref()
            .is_some_and(|c| c.url != desired.url)
        {
            desired.loaded = false;
        }
        let actions = plan(self.scheduled.as_ref(), Some(&desired));
        if actions.is_empty() && script.is_none() {
            return Task::none();
        }
        self.scheduled = Some(desired.clone());
        let slot = self.slot;
        let epoch = self.epoch.clone();
        window::run(id, move |_| {
            if epoch.load(Ordering::SeqCst) != generation {
                return done;
            }
            VIEWS.with(|views| {
                let mut views = views.borrow_mut();
                if let Some(e) = views.get_mut(&slot).filter(|e| e.generation == generation) {
                    for action in actions {
                        match action {
                            Action::Navigate => {
                                let _ = e.view.load_url(&desired.url);
                            }
                            Action::Zoom => {
                                let _ = e.view.zoom(desired.scale);
                            }
                            Action::SetBounds => {
                                let _ = e.view.set_bounds(bounds(desired.rect));
                            }
                            Action::Park => {
                                restore_focus(&e.view);
                                let _ = e.view.set_bounds(parked_bounds(desired.rect));
                                // WKWebView must stay visible to run requestAnimationFrame
                                // and finish document bootstrap while parked offscreen.
                                let _ = e.view.set_visible(desired.visible);
                            }
                            Action::Show => {
                                let _ = e.view.set_visible(true);
                            }
                            Action::Create | Action::Destroy => {
                                unreachable!("identity replacement requires create")
                            }
                        }
                    }
                    if let Some(script) = script {
                        let _ = e.view.evaluate_script(&script);
                    }
                }
            });
            done
        })
    }
    pub fn probe<E: Send + 'static>(
        &self,
        id: window::Id,
        generation: u64,
        map: fn(Probe) -> E,
    ) -> Task<E> {
        let slot = self.slot;
        window::run(id, move |_| {
            map(VIEWS.with(|views| {
                let views = views.borrow();
                let entry = views.get(&slot).filter(|e| e.generation == generation);
                let mut probe = Probe {
                    exists: entry.is_some(),
                    visible: false,
                    bounds: None,
                    handles: usize::from(views.contains_key(&slot)),
                };
                if let Some(e) = entry {
                    inspect(&e.view, &mut probe);
                }
                probe
            }))
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    pub exists: bool,
    pub visible: bool,
    pub bounds: Option<(f64, f64, f64, f64)>,
    pub handles: usize,
}
#[cfg(target_os = "macos")]
fn inspect(view: &wry::WebView, probe: &mut Probe) {
    use wry::WebViewExtMacOS;
    let ns = view.webview();
    let frame = ns.frame();
    // SAFETY: live child and its parent are inspected only inside window::run.
    probe.bounds = unsafe { ns.superview() }.map(|parent| {
        let y = if parent.isFlipped() {
            frame.origin.y
        } else {
            parent.frame().size.height - frame.origin.y - frame.size.height
        };
        (frame.origin.x, y, frame.size.width, frame.size.height)
    });
    probe.visible = !ns.isHidden();
}
#[cfg(not(target_os = "macos"))]
fn inspect(view: &wry::WebView, probe: &mut Probe) {
    // wry exposes no cross-platform native visibility getter.
    probe.bounds = view.bounds().ok().map(|r| {
        let p = r.position.to_logical::<f64>(1.);
        let s = r.size.to_logical::<f64>(1.);
        (p.x, p.y, s.width, s.height)
    });
}
#[cfg(target_os = "macos")]
fn restore_focus(view: &wry::WebView) {
    use wry::WebViewExtMacOS;
    // window::run supplies main-thread execution; the child is still alive here.
    if let Some(window) = view.webview().window()
        && let Some(content) = window.contentView()
    {
        let _ = window.makeFirstResponder(Some(&content));
    }
}
#[cfg(not(target_os = "macos"))]
fn restore_focus(_: &wry::WebView) {}
fn parked_bounds(mut rect: LogicalRect) -> wry::Rect {
    rect.x = -100_000.;
    rect.y = -100_000.;
    bounds(rect)
}
fn bounds(rect: LogicalRect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(rect.x, rect.y).into(),
        size: wry::dpi::LogicalSize::new(rect.width, rect.height).into(),
    }
}
#[cfg(test)]
#[path = "native_host_tests.rs"]
mod tests;
