//! Project-scoped host for the shared conversations state machine.
//! The library owns fetching, request tokens, search, paging and row activation.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytehost_chrome::{BodyArea, Divider, ShellMsg, pair_view};
use dozer_model::ProjectInfo;
use dozer_panel_conversations::adapter::{
    ConversationsCapabilities, ConversationsRequest, LinkedTodos,
};
use dozer_panel_conversations::{Message, WorkspaceState};
use dozer_services::{ConversationService, SummaryService, TranscriptService};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::agent::{AgentShell, Status};
use super::conversations_display::{ConversationsDisplay, relative_time};
use crate::product::Msg;

pub struct NoTodos;
impl LinkedTodos for NoTodos {
    fn linked_text(&self, _: &str) -> Option<String> {
        None
    }
}

pub struct ConversationsPanel {
    states: HashMap<i64, WorkspaceState>,
    // The library view borrows its display for the lifetime of the element.
    // Keep one beside each state so returned widgets never borrow a local adapter.
    displays: HashMap<i64, ConversationsDisplay>,
    conversation: Arc<dyn ConversationService>,
    summary: Arc<dyn SummaryService>,
    // Used by the explicit import action in Task 4; never called on activation.
    #[allow(dead_code)]
    pub(crate) transcript: Arc<dyn TranscriptService>,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<(i64, Message)>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>>,
    pub notice: Option<String>,
    pub(crate) dirty: HashSet<i64>,
    requests: Vec<(i64, ConversationsRequest)>,
    caps: ConversationsCapabilities,
}
impl ConversationsPanel {
    pub fn new(
        conversation: Arc<dyn ConversationService>,
        summary: Arc<dyn SummaryService>,
        transcript: Arc<dyn TranscriptService>,
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            states: HashMap::new(),
            displays: HashMap::new(),
            conversation,
            summary,
            transcript,
            handle: None,
            tx,
            rx: Some(rx),
            notice: None,
            dirty: HashSet::new(),
            requests: vec![],
            caps: ConversationsCapabilities::default(),
        }
    }
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>> {
        self.rx.take()
    }
    pub fn state(&self, pid: i64) -> Option<&WorkspaceState> {
        self.states.get(&pid)
    }
    pub fn ensure_state(&mut self, project: &ProjectInfo) -> &mut WorkspaceState {
        self.displays
            .entry(project.id)
            .or_insert_with(|| ConversationsDisplay::new(project.id));
        self.states.entry(project.id).or_default()
    }
    pub fn activate(&mut self, project: &ProjectInfo) -> Task<Msg> {
        self.ensure_state(project);
        self.dirty.remove(&project.id);
        let Some(handle) = self.handle.as_ref() else {
            return Task::none();
        };
        let ctx = dozer_panel_conversations::update::UpdateCtx {
            project_id: project.id,
            project_path: std::path::Path::new(&project.path),
            conversation: &self.conversation,
            summary: &self.summary,
            handle,
            caps: self.caps,
            locale: "zh-CN",
            now_ms: now_ms(),
            relative_time: &relative_time,
        };
        let tx = self.tx.clone();
        let pid = project.id;
        dozer_panel_conversations::update::on_activate(&self.states[&pid], &ctx, move |m| {
            let _ = tx.send((pid, m));
        });
        Task::none()
    }
    pub fn update(&mut self, pid: i64, msg: Message, project: Option<&ProjectInfo>) -> Task<Msg> {
        // A result's scope is authoritative, even when a host envelope names another project.
        let pid = msg.result_project().unwrap_or(pid);
        let Some(project) = project.filter(|p| p.id == pid) else {
            return Task::none();
        };
        let Some(state) = self.states.get_mut(&pid) else {
            return Task::none();
        };
        let Some(handle) = self.handle.as_ref() else {
            return Task::none();
        };
        // Unavailable list errors are intentionally debug-only in the library. Inspect
        // the same token guards before update so stale failures cannot create notices.
        let failure = accepted_service_failure(state, &msg);
        let ctx = dozer_panel_conversations::update::UpdateCtx {
            project_id: pid,
            project_path: std::path::Path::new(&project.path),
            conversation: &self.conversation,
            summary: &self.summary,
            handle,
            caps: self.caps,
            locale: "zh-CN",
            now_ms: now_ms(),
            relative_time: &relative_time,
        };
        let tx = self.tx.clone();
        dozer_panel_conversations::update::update(state, msg, &ctx, move |m| {
            let _ = tx.send((pid, m));
        });
        let notices = state.take_outbox();
        self.requests
            .extend(state.take_requests().into_iter().map(|r| (pid, r)));
        for notice in notices {
            let message = dozer_panel_conversations::i18n::render(&notice.text, "zh-CN");
            eprintln!("digger: conversations {:?}: {message}", notice.level);
            self.notice = Some(message);
        }
        if let Some(error) = failure {
            eprintln!("digger: conversations: {error}");
            self.notice = Some(error);
        }
        Task::none()
    }
    /// Native-menu requests are retained for the product's Task 3 menu dispatcher.
    pub fn take_requests(&mut self) -> Vec<(i64, ConversationsRequest)> {
        std::mem::take(&mut self.requests)
    }
    pub fn close_project(&mut self, pid: i64) {
        self.states.remove(&pid);
        self.displays.remove(&pid);
        self.dirty.remove(&pid);
        self.requests.retain(|(id, _)| *id != pid);
    }
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }
    pub fn view<'a>(
        &'a self,
        project: Option<&ProjectInfo>,
        area: &BodyArea,
        shell: &'a AgentShell,
    ) -> Element<'a, Msg> {
        let tokens = byteui::theme::color::current();
        let body = if let Some(project) = project {
            if shell.status != Status::Ready {
                shell.view(true).map(Msg::AgentShell)
            } else if let Some(state) = self.states.get(&project.id) {
                let display = &self.displays[&project.id];
                display.configure(area);
                let width = area.pair.map(|p| p.first.w).unwrap_or(area.rect.w).max(0.);
                let list = dozer_panel_conversations::view::conversations_list(
                    display,
                    state,
                    &NoTodos,
                    self.caps,
                    Length::Fixed(width),
                    Default::default(),
                );
                pair_view(
                    area,
                    list,
                    placeholder(),
                    Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
                    tokens.bg,
                    tokens.panel,
                )
            } else {
                placeholder()
            }
        } else {
            // Same unopened-project treatment as ProjectPanel.
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        match &self.notice {
            Some(notice) => {
                let line: Element<'_, Msg> = row![
                    text(notice.clone())
                        .size(byteui::theme::font::caption())
                        .color(tokens.red)
                        .width(Length::Fill),
                    button(text("×").color(tokens.dim)).on_press(Msg::ConversationsNoticeDismiss)
                ]
                .spacing(8)
                .padding([6, 12])
                .align_y(iced::Alignment::Center)
                .into();
                column![line, body].height(Length::Fill).into()
            }
            None => body,
        }
    }
}
fn placeholder<'a>() -> Element<'a, Msg> {
    container(text("审阅详情即将可用"))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn accepted_service_failure(state: &WorkspaceState, msg: &Message) -> Option<String> {
    let mut msg = msg;
    while let Message::AsyncResult(token, inner) = msg {
        if !state.requests.accepts(token) {
            return None;
        }
        msg = inner;
    }
    match msg {
        Message::SessionsRefreshed(_, Err(error))
            if error.kind == dozer_services::ServiceErrorKind::Unavailable =>
        {
            Some(format!("会话加载失败: {}", error.message))
        }
        Message::ReviewLoaded {
            result: Err(error),
            source,
            ..
        } if state.review.as_ref().is_some_and(|rv| &rv.source == source) => {
            Some(format!("会话加载失败: {error}"))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "conversations_tests.rs"]
pub(crate) mod tests;
