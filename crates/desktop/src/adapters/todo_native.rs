//! 待办面板的 wry 子视图宿主(S3b-1 Task 3)。
//!
//! 结构照 `usage_native.rs`:epoch 防旧实例回传、`thread_local` 只放 wry 句柄
//! (仅主线程 `window::run` 闭包访问)、资源走 `digger-todo://` 自定义协议。
//! 页面回传的 `Ready` / 业务事件经 IPC 送回产品;Rust → 页面的推送由库的
//! `WebviewPushState` 去重,再用双重 JSON 编码的 `dispatch` 脚本下发。

use bytehost_core::LogicalRect;
#[cfg(test)]
use dozer_model::Locale;
use dozer_model::{AgentKind, Stamped};
use dozer_panel_todo::{
    TodoWebviewEvent, WebviewPushState, WorkspaceState, current_view_payload, encode_todo_push,
    parse_todo_event,
};
use iced::{Task, window};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

use bytehost_desktop::native_child::{self, Host};

#[derive(Debug, Clone)]
pub enum Event {
    Ready(u64),
    Built(u64, Result<(), String>),
    Todo(u64, TodoWebviewEvent),
    Done,
}

pub struct Native {
    push: WebviewPushState,
    host: Host,
    epoch: Arc<AtomicU64>,
    live: bool,
    theme: &'static str,
    ready: bool,
    bounds: Option<LogicalRect>,
    project_id: i64,
    error: Option<String>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Event>>,
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
            push: WebviewPushState::default(),
            epoch,
            live: false,
            theme: crate::settings::theme::code(),
            ready: false,
            bounds: None,
            project_id: 0,
            error: None,
            tx,
            rx: Some(rx),
        }
    }

    pub fn set_project(&mut self, project_id: Option<i64>) {
        self.project_id = project_id.unwrap_or(0);
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }

    pub fn update(&mut self, event: Event) -> Option<TodoWebviewEvent> {
        match event {
            Event::Ready(g) if self.live && g == self.epoch.load(Ordering::SeqCst) => {
                self.ready = true;
                self.error = None;
                self.push.set_ready(true);
                None
            }
            Event::Built(g, Err(error)) if self.live && g == self.epoch.load(Ordering::SeqCst) => {
                self.ready = false;
                self.error = Some(error);
                self.push.set_ready(false);
                None
            }
            Event::Todo(g, event) if self.live && g == self.epoch.load(Ordering::SeqCst) => {
                Some(event)
            }
            _ => None,
        }
    }

    fn desired(&self, rect: LogicalRect) -> native_child::Desired {
        native_child::Desired {
            identity: "todo".into(),
            url: format!("digger-todo://localhost/host.html?theme={}", self.theme),
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
            self.push.set_ready(false);
            self.push.observe_availability(false, Instant::now());
            return self.host.destroy(id, generation, Event::Done);
        }

        let payload = current_view_payload(
            state,
            &|_| false,
            self.project_id,
            dozer_panel_todo::today_ymd(),
            &agent_icon_data,
        );
        let desired = Stamped::new(crate::settings::language::stamp(), payload);
        self.push.observe_availability(self.live, Instant::now());

        if !self.live {
            self.live = true;
            self.bounds = Some(rect);
            self.error = None;
            self.push.set_ready(false);
            let generation = self.epoch.load(Ordering::SeqCst);
            let tx = self.tx.clone();
            return self.host.create(
                id,
                generation,
                native_child::Document {
                    scheme: "digger-todo",
                    host: "localhost",
                    desired: self.desired(rect),
                },
                move |builder| {
                    builder
                        .with_custom_protocol("digger-todo".into(), |_, request| {
                            match super::todo_assets::serve(request.uri().path()) {
                                Some((mime, body)) => wry::http::Response::builder()
                                    .header("Content-Type", mime)
                                    .body(body.to_vec().into())
                                    .unwrap(),
                                None => wry::http::Response::builder()
                                    .status(404)
                                    .body(Vec::new().into())
                                    .unwrap(),
                            }
                        })
                        .with_ipc_handler(move |request| match parse_todo_event(request.body()) {
                            Ok(TodoWebviewEvent::Ready) => {
                                let _ = tx.send(Event::Ready(generation));
                            }
                            Ok(event) => {
                                let _ = tx.send(Event::Todo(generation, event));
                            }
                            Err(error) => {
                                eprintln!("digger: 待办页面事件解析失败: {error}"); // cli-output
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
        let script = self.push.pending_push(&desired).map(|stamped| {
            let revision = self.push.mark_sent(stamped.clone());
            let (locale, payload) = stamped.into_parts();
            dispatch(&encode_todo_push(revision, Some(locale), payload))
        });
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

    pub fn failed(&self) -> Option<String> {
        self.error
            .clone()
            .or_else(|| self.push.failed().map(|text| text.fallback.clone()))
    }

    #[cfg(test)]
    pub(crate) fn is_live(&self) -> bool {
        self.live
    }

    #[cfg(test)]
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// 测试用:当前项目的新载荷是否还没推过(不消费)。
    #[cfg(test)]
    pub(crate) fn pending_project(&self, state: &WorkspaceState) -> Option<i64> {
        let desired = Stamped::new(
            crate::settings::language::stamp(),
            current_view_payload(
                state,
                &|_| false,
                self.project_id,
                dozer_panel_todo::today_ymd(),
                &agent_icon_data,
            ),
        );
        self.push
            .pending_push(&desired)
            .map(|p| p.stamp.locale_revision)
            .map(|_| self.project_id)
    }
}

/// 导航只放行本协议入口(严格 CSP 的一部分)。
pub fn allows_navigation(url: &str) -> bool {
    native_child::allows_navigation("digger-todo", "localhost", url)
}

/// 双重 JSON 编码:外层 `to_string` 生成 JS 字符串字面量,内层由页面
/// `JSON.parse` 取回对象。额外转义 `</` 与 U+2028/U+2029,防止脚本上下文
/// 提前结束或换行。
pub fn dispatch(json: &str) -> String {
    let encoded = serde_json::to_string(json).unwrap_or_else(|_| "\"null\"".into());
    let encoded = encoded
        .replace("</", "<\\/")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    format!("window.__dozer?.dispatch({encoded});")
}

fn agent_icon_data(agent: AgentKind) -> dozer_panel_todo::adapter::AgentIconData {
    let color = match agent {
        AgentKind::Claude => "#d97757",
        AgentKind::Codebuddy => "#4c9aff",
        AgentKind::Opencode => "#7bd88f",
        AgentKind::V8agent => "#c792ea",
        _ => "#9aa4b2",
    };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"><circle cx=\"8\" cy=\"8\" r=\"6\" fill=\"{color}\"/></svg>"
    );
    dozer_panel_todo::adapter::AgentIconData {
        bytes: svg.into_bytes(),
        preserves_color: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::LocaleStamp;
    use dozer_model::TodoInfo;

    fn state(text: &str) -> WorkspaceState {
        WorkspaceState {
            items: vec![TodoInfo {
                id: 1,
                project_id: 7,
                text: text.into(),
                done: false,
                paused: false,
                rank: 0,
                created_ms: 0,
                completed_at_ms: None,
                plan_date: None,
                dispatch_session_id: None,
                dispatch_at_ms: None,
                category_id: None,
                assigned_agent: None,
            }],
            ..Default::default()
        }
    }

    fn rect() -> LogicalRect {
        LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    }

    #[test]
    fn invisible_destroys_the_view_and_bumps_the_epoch() {
        let mut native = Native::new();
        native.set_project(Some(7));
        let _ = native.sync(window::Id::unique(), true, rect(), &state("一"));
        assert!(native.is_live());
        let old = native.epoch();
        let _ = native.sync(window::Id::unique(), false, rect(), &state("一"));
        assert!(!native.is_live());
        assert_eq!(native.epoch(), old + 1);
        assert!(native.update(Event::Ready(old)).is_none());
        assert!(
            native
                .update(Event::Todo(old, TodoWebviewEvent::Add { text: "x".into() }))
                .is_none(),
            "旧 epoch 的页面事件必须丢弃"
        );
    }

    #[test]
    fn ready_forces_a_full_repush() {
        let state = state("一");
        let desired = Stamped::new(
            LocaleStamp {
                ui_locale: Locale::default_locale(),
                locale_revision: 0,
            },
            current_view_payload(&state, &|_| false, 7, (2026, 10, 25), &agent_icon_data),
        );
        let mut push = WebviewPushState::default();
        push.set_ready(true);
        assert!(push.pending_push(&desired).is_some());
        let sent = push.mark_sent(desired.clone());
        assert_eq!(sent, 1);
        assert!(push.pending_push(&desired).is_none());
        push.set_ready(true);
        assert!(push.pending_push(&desired).is_some(), "重建后必须重发");
    }

    #[test]
    fn locale_change_alone_triggers_a_push() {
        let state = state("一");
        let payload = current_view_payload(&state, &|_| false, 7, (2026, 10, 25), &agent_icon_data);
        let a = Stamped::new(
            LocaleStamp {
                ui_locale: Locale::default_locale(),
                locale_revision: 0,
            },
            payload.clone(),
        );
        let b = Stamped::new(
            LocaleStamp {
                ui_locale: Locale::default_locale(),
                locale_revision: 1,
            },
            payload,
        );
        let mut push = WebviewPushState::default();
        push.set_ready(true);
        push.mark_sent(a);
        assert!(push.pending_push(&b).is_some());
    }

    #[test]
    fn dispatch_script_survives_hostile_task_text() {
        let hostile = "他说:\"hi\"\n</script><img src=x onerror=alert(1)>\u{2028}😀";
        let state = state(hostile);
        let payload = current_view_payload(&state, &|_| false, 7, (2026, 10, 25), &agent_icon_data);
        let json = encode_todo_push(
            1,
            Some(LocaleStamp {
                ui_locale: Locale::default_locale(),
                locale_revision: 0,
            }),
            payload,
        );
        let script = dispatch(&json);
        assert!(!script.contains("</script>"));
        assert!(!script.contains('\u{2028}'));
        let inner = script
            .strip_prefix("window.__dozer?.dispatch(")
            .and_then(|s| s.strip_suffix(");"))
            .unwrap();
        let decoded: String = serde_json::from_str(inner).unwrap();
        assert_eq!(decoded, json);
        let envelope: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(envelope["payload"]["items"][0]["text"], hostile);
    }

    #[test]
    fn switching_project_pushes_the_new_projects_payload_without_rebuilding_the_view() {
        let mut native = Native::new();
        native.set_project(Some(7));
        let _ = native.sync(window::Id::unique(), true, rect(), &state("一"));
        let epoch = native.epoch();
        assert!(native.is_live());
        native.update(Event::Ready(epoch));

        native.set_project(Some(8));
        assert!(native.is_live(), "切换项目不重建 webview");
        assert_eq!(native.epoch(), epoch, "重建才需要换 epoch");
        assert_eq!(
            native.pending_project(&state("一")),
            Some(8),
            "新项目的 payload 还没推过"
        );
    }

    #[test]
    fn navigation_is_limited_to_the_todo_protocol() {
        assert!(allows_navigation("digger-todo://localhost/host.html"));
        for bad in [
            "https://example.com",
            "digger-usage://localhost/host.html",
            "file:///etc/passwd",
        ] {
            assert!(!allows_navigation(bad), "{bad}");
        }
    }

    #[test]
    fn the_agent_icons_are_inline_svg() {
        for agent in dozer_panel_todo::DISPATCH_AGENTS {
            let icon = agent_icon_data(agent);
            let text = String::from_utf8(icon.bytes).unwrap();
            assert!(text.contains("<svg"), "{agent:?}");
        }
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;

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
