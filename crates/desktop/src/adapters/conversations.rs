//! Project-scoped host for the shared conversations state machine.
//! The library owns fetching, request tokens, search, paging and row activation.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytehost_chrome::{BodyArea, Divider, ShellMsg, pair_view};
use dozer_model::ProjectInfo;
use dozer_panel_conversations::adapter::{
    ConversationsCapabilities, ConversationsRequest, LinkedTodos, PanelDisplay,
};
use dozer_panel_conversations::{Message, WorkspaceState};
use dozer_services::{ConversationService, SummaryService, TranscriptService};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::agent::{AgentShell, AgentShellMsg, Status};
use super::conversations_display::{ConversationsDisplay, relative_time};
use super::conversations_import::{
    ImportEvent, ImportMarker, ImportPrompt, ImportState, LOG_DISCLOSURE, READ_DISCLOSURE,
};
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
    pub(crate) transcript: Arc<dyn TranscriptService>,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<(i64, Message)>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>>,
    pub notice: Option<String>,
    pub(crate) dirty: HashSet<i64>,
    requests: Vec<(i64, ConversationsRequest)>,
    caps: ConversationsCapabilities,
    imports: HashMap<i64, ImportState>,
    import_marker: ImportMarker,
    marker_path: std::path::PathBuf,
    import_tx: tokio::sync::mpsc::UnboundedSender<(i64, ImportEvent)>,
    import_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(i64, ImportEvent)>>,
}
impl ConversationsPanel {
    pub fn new(
        conversation: Arc<dyn ConversationService>,
        summary: Arc<dyn SummaryService>,
        transcript: Arc<dyn TranscriptService>,
    ) -> Self {
        Self::new_with_marker_path(
            conversation,
            summary,
            transcript,
            digger_paths::config_dir().join("conversations-imported.json"),
        )
    }
    pub(crate) fn new_with_marker_path(
        conversation: Arc<dyn ConversationService>,
        summary: Arc<dyn SummaryService>,
        transcript: Arc<dyn TranscriptService>,
        marker_path: std::path::PathBuf,
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (import_tx, import_rx) = tokio::sync::mpsc::unbounded_channel();
        let import_marker = ImportMarker::load(&marker_path);
        Self {
            imports: HashMap::new(),
            import_marker,
            marker_path,
            import_tx,
            import_rx: Some(import_rx),
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
    pub fn take_import_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<(i64, ImportEvent)>> {
        self.import_rx.take()
    }
    pub fn import_prompt(&self, pid: i64, daemon_ready: bool) -> Option<ImportPrompt> {
        (daemon_ready
            && !self.import_marker.is_marked(pid)
            && self
                .states
                .get(&pid)
                .and_then(|s| s.sessions())
                .is_some_and(|s| s.is_empty()))
        .then_some(ImportPrompt)
    }
    pub fn update_import(
        &mut self,
        pid: i64,
        event: ImportEvent,
        project: Option<&ProjectInfo>,
        daemon_ready: bool,
        closing: bool,
    ) -> Task<Msg> {
        let Some(project) =
            project.filter(|p| p.id == pid && !closing && self.states.contains_key(&pid))
        else {
            return Task::none();
        };
        match event {
            ImportEvent::Start => {
                if self.import_prompt(pid, daemon_ready).is_none()
                    || !matches!(
                        self.imports.get(&pid),
                        None | Some(ImportState::Idle | ImportState::Failed(_))
                    )
                {
                    return Task::none();
                }
                let Some(handle) = self.handle.as_ref() else {
                    return Task::none();
                };
                let token = self.states[&pid]
                    .requests
                    .begin(Some(pid), "host-history-import");
                self.imports.insert(pid, ImportState::Importing);
                let service = self.transcript.clone();
                let cwd = project.path.clone();
                let tx = self.import_tx.clone();
                // Construct the service future inside the spawned task, too:
                // an implementation can block before returning its BoxFuture.
                handle.spawn(async move {
                    let result = service
                        .backfill_project_transcripts(cwd)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send((pid, ImportEvent::Finished(token, result)));
                });
            }
            ImportEvent::Finished(token, result) => {
                if !matches!(self.imports.get(&pid), Some(ImportState::Importing))
                    || !self.states[&pid].requests.accepts(&token)
                {
                    return Task::none();
                }
                match result {
                    Ok(count) => {
                        self.imports.insert(pid, ImportState::Done(count));
                        self.import_marker.mark(pid, count, now_ms());
                        if let Err(error) = self.import_marker.save(&self.marker_path) {
                            let message = format!("导入已完成,但无法保存导入提示标记: {error}");
                            eprintln!("digger: conversations: {message}");
                            self.notice = Some(message);
                        }
                        return self.activate(project);
                    }
                    Err(error) => {
                        eprintln!("digger: conversations import: {error}");
                        self.notice = Some(format!("历史会话导入失败: {error}"));
                        self.imports.insert(pid, ImportState::Failed(error));
                    }
                }
            }
        }
        Task::none()
    }
    fn import_view(&self, pid: i64) -> Option<Element<'_, Msg>> {
        let state = self.imports.get(&pid).cloned().unwrap_or_default();
        // Completion outlives prompt eligibility (marker set / list now nonempty).
        let completion = match state {
            ImportState::Done(0) => Some("没有找到可导入的会话".to_owned()),
            ImportState::Done(count) => Some(format!("已导入 {count} 条会话")),
            _ => None,
        };
        let logs = digger_paths::logs_dir().display().to_string();
        let disclosure = column![text(LOG_DISCLOSURE), text(logs)].spacing(4);
        if let Some(completion) = completion {
            return Some(
                column![text(completion), disclosure]
                    .spacing(6)
                    .padding(12)
                    .into(),
            );
        }
        self.import_prompt(pid, true)?;
        let mut content = column![text(READ_DISCLOSURE)].spacing(6);
        let label = match &state {
            ImportState::Importing => "正在导入…",
            ImportState::Failed(error) => {
                content =
                    content.push(text(error.clone()).color(byteui::theme::color::current().red));
                "重试"
            }
            _ => "导入此项目的历史会话",
        };
        let action = button(text(label)).on_press_maybe(
            (state != ImportState::Importing)
                .then_some(Msg::ConversationsImport(pid, ImportEvent::Start)),
        );
        Some(content.push(action).push(disclosure).padding(12).into())
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
    /// Only existing, correctly scoped projects participate in turn refreshes.
    pub fn on_turn_ended(
        &mut self,
        pid: i64,
        project: Option<&ProjectInfo>,
        visible: bool,
    ) -> Task<Msg> {
        let Some(project) = project.filter(|p| p.id == pid && self.states.contains_key(&pid))
        else {
            return Task::none();
        };
        if visible {
            self.activate(project)
        } else {
            self.dirty.insert(pid);
            Task::none()
        }
    }
    pub fn set_search_focused(&mut self, pid: i64, focused: bool) {
        if let Some(state) = self.states.get_mut(&pid) {
            state.set_search_focused(focused);
        }
    }
    /// Focus and fallback menu flags must never survive a hidden/inactive project.
    pub fn reset_inactive_focus(&mut self, active: Option<i64>) {
        for (&pid, state) in &mut self.states {
            if Some(pid) != active {
                state.set_search_focused(false);
                state.set_agent_picker_open(false);
                // Invalidate even a not-yet-delivered focus query across hide/show.
                state.requests.begin(Some(pid), "host-search-focus");
            }
        }
    }
    pub fn close_agent_filters(&mut self) {
        for state in self.states.values_mut() {
            state.set_agent_picker_open(false);
        }
    }
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn open_agent_filter(&mut self, pid: i64) {
        if let Some(state) = self.states.get_mut(&pid) {
            state.set_agent_picker_open(true);
        }
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
        if let Some((error, daemon_unavailable)) = failure {
            eprintln!("digger: conversations: {error}");
            self.notice = Some(error.clone());
            if daemon_unavailable {
                // Reuse the shared status/retry lifecycle. Only a library-accepted
                // result reaches here; stale or closed-project errors cannot mark
                // the daemon down. No additional ensure task is started.
                return Task::done(Msg::AgentShell(AgentShellMsg::Ensured(Err(error))));
            }
        }
        Task::none()
    }
    /// Host requests are consumed by the product after each library update.
    pub fn take_requests(&mut self) -> Vec<(i64, ConversationsRequest)> {
        std::mem::take(&mut self.requests)
    }
    pub fn close_project(&mut self, pid: i64) {
        self.imports.remove(&pid);
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
                let list: Element<'_, Msg> = if let Some(import) = self.import_view(project.id) {
                    column![import, list].height(Length::Fill).into()
                } else {
                    list
                };
                pair_view(
                    area,
                    list,
                    placeholder(Some(display)),
                    Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
                    tokens.bg,
                    tokens.panel,
                )
            } else {
                placeholder(None)
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
fn placeholder<'a>(display: Option<&ConversationsDisplay>) -> Element<'a, Msg> {
    let mut header = row![text("审阅详情即将可用").width(Length::Fill)]
        .spacing(8)
        .align_y(iced::Alignment::Center);
    if let Some(display) = display {
        header =
            header.push(display.list_collapse_button("收起会话列表".into(), "展开会话列表".into()));
    }
    container(header)
        .padding(8)
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
fn accepted_service_failure(state: &WorkspaceState, msg: &Message) -> Option<(String, bool)> {
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
            Some((format!("会话加载失败: {}", error.message), true))
        }
        Message::ReviewLoaded {
            result: Err(error),
            source,
            ..
        } if state.review.as_ref().is_some_and(|rv| &rv.source == source) => {
            Some((format!("会话加载失败: {error}"), false))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "conversations_tests.rs"]
pub(crate) mod tests;
