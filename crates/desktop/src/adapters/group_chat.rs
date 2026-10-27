//! Group-chat panel host: per-project library state, routing, polling, the list
//! column and the single native chat page (`group_chat_native`).
use std::collections::HashMap;
use std::time::Instant;

use bytehost_chrome::{BodyArea, Divider, ShellMsg, pair_view};
use dozer_model::{Locale, LocaleStamp, LocalizedText, ProjectInfo};
use dozer_panel_group_chat::{
    Message, WebviewPushState, WorkspaceState, host, on_activate, poll_if_due, run_effect, update,
};
use dozer_panel_host::PanelIo;
use dozer_services::Services;
use iced::widget::{column, container, text};
use iced::{Element, Length, Task};

use super::group_chat_display::GroupChatDisplay;
use super::group_chat_native::{self, GroupChatNative};
use super::notice::NoticeBar;
use crate::product::Msg;

/// 新建群对话框里常驻的提示:群聊里 agent 的发言在执行层只读,且目前只有这两家能发言。
pub(crate) const MENTION_READ_ONLY_NOTE: &str =
    "被 @ 的成员只读地读取项目文件,目前仅 Claude、Codex、Qodercn 可以发言";

pub struct GroupChatPanel {
    states: HashMap<i64, WorkspaceState>,
    displays: HashMap<i64, GroupChatDisplay>,
    services: Services,
    handle: Option<tokio::runtime::Handle>,
    focus_project: Option<i64>,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Message>>,
    pub notice: NoticeBar,
    pub native: GroupChatNative,
    /// Declarative push + readiness for the one chat page (fixed single slot).
    push: WebviewPushState,
}

impl GroupChatPanel {
    pub fn new(services: Services) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            states: HashMap::new(),
            displays: HashMap::new(),
            services,
            handle: None,
            focus_project: None,
            tx,
            rx: Some(rx),
            notice: NoticeBar::default(),
            native: GroupChatNative::new(),
            push: WebviewPushState::default(),
        }
    }
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Message>> {
        self.rx.take()
    }
    fn io(&self) -> Option<PanelIo<Message>> {
        let handle = self.handle.clone()?;
        let tx = self.tx.clone();
        Some(PanelIo::new(self.services.clone(), handle, move |m| {
            let _ = tx.send(m);
        }))
    }
    pub fn ensure_state(&mut self, project: &ProjectInfo) -> &mut WorkspaceState {
        self.displays
            .entry(project.id)
            .or_insert_with(|| GroupChatDisplay::new());
        self.states.entry(project.id).or_default()
    }
    pub fn state(&self, pid: i64) -> Option<&WorkspaceState> {
        self.states.get(&pid)
    }
    pub fn activate(&mut self, project: &ProjectInfo, now: Instant) -> Task<Msg> {
        self.ensure_state(project);
        self.focus_project = Some(project.id);
        let Some(io) = self.io() else {
            return Task::none();
        };
        let Some(state) = self.states.get_mut(&project.id) else {
            return Task::none();
        };
        on_activate(state);
        poll_if_due(state, project.id, now, &io);
        Task::none()
    }
    pub fn update(&mut self, message: Message, project_open: impl Fn(i64) -> bool) -> Task<Msg> {
        let pid = message.project_id();
        if !project_open(pid) {
            return Task::none();
        }
        let Some(io) = self.io() else {
            return Task::none();
        };
        let Some(state) = self.states.get_mut(&pid) else {
            return Task::none();
        };
        for effect in update(state, message) {
            run_effect(state.requests(), effect, pid, &io);
        }
        for notice in state.take_outbox() {
            let text = dozer_panel_group_chat::i18n::render(&notice.text, "zh-CN");
            self.notice.push(text);
        }
        Task::none()
    }
    /// Why the chat page is replaced by the failure shell, if it is.
    pub fn page_failure(&self) -> Option<String> {
        self.push
            .failed()
            .map(|reason| dozer_panel_group_chat::i18n::render(reason, "zh-CN"))
    }
    /// Retry after a load failure: forget it; the next sync recreates the page.
    pub fn retry_page(&mut self) {
        self.push.clear_failed();
        self.native.take_error();
    }
    pub(crate) fn chat_desired(
        &self,
        shown: bool,
        overlay_open: bool,
        rect: bytehost_core::LogicalRect,
        project_id: Option<i64>,
    ) -> group_chat_native::Desired {
        group_chat_native::Desired {
            shown: shown
                && self.push.failed().is_none()
                && project_id.is_some_and(|pid| self.states.contains_key(&pid)),
            overlay_open,
            rect,
            project_id,
        }
    }
    /// Reconcile the chat page with the shell: create/park/show/destroy it and inject
    /// whatever the library says is pending. Scripts are only produced for a live,
    /// built child of the same project, so nothing is marked sent that cannot arrive.
    pub fn sync_native(
        &mut self,
        id: iced::window::Id,
        shown: bool,
        overlay_open: bool,
        rect: bytehost_core::LogicalRect,
        project_id: Option<i64>,
        now: Instant,
    ) -> Task<group_chat_native::Event> {
        if let Some(error) = self.native.take_error() {
            self.notice.push(error.clone());
            self.push.set_failed(LocalizedText::literal(
                dozer_panel_group_chat::i18n::HOST_OWNER,
                error,
            ));
        }
        let desired = self.chat_desired(shown, overlay_open, rect, project_id);
        let available = self.native.is_available_for(&desired);
        let scripts = match project_id.and_then(|pid| self.states.get(&pid).map(|s| (pid, s))) {
            Some((pid, state)) if desired.shown => host::take_content_script(
                state,
                &mut self.push,
                pid,
                available,
                now,
                LocaleStamp {
                    ui_locale: Locale::default_locale(),
                    locale_revision: 0,
                },
                "zh-CN",
            ),
            _ => {
                self.push.observe_availability(false, now);
                Vec::new()
            }
        };
        let script = (!scripts.is_empty()).then(|| scripts.join("\n"));
        // A load timeout is decided while taking the scripts: re-derive the target so
        // the failed page is destroyed in this very pass, not one sync later.
        let desired = self.chat_desired(shown, overlay_open, rect, project_id);
        self.native.sync(id, desired, script)
    }
    /// Returns `true` when the page asked to show the Todo panel.
    pub fn update_native(&mut self, event: group_chat_native::Event) -> (Task<Msg>, bool) {
        let project = self.native.project_id();
        let Some(page) = self.native.update(event) else {
            return (Task::none(), false);
        };
        let (Some(pid), Some(io)) = (project, self.io()) else {
            return (Task::none(), false);
        };
        let Some(state) = self.states.get_mut(&pid) else {
            return (Task::none(), false);
        };
        let action = host::handle_webview_event(state, &mut self.push, pid, page, &io);
        for notice in state.take_outbox() {
            let text = dozer_panel_group_chat::i18n::render(&notice.text, "zh-CN");
            self.notice.push(text);
        }
        (
            Task::none(),
            action == dozer_panel_group_chat::host::HostAction::ShowTodoPanel,
        )
    }
    pub fn close_project(&mut self, pid: i64) {
        self.states.remove(&pid);
        self.displays.remove(&pid);
        if self.focus_project == Some(pid) {
            self.focus_project = None;
        }
    }
    pub fn set_focus(&mut self, pid: Option<i64>) {
        self.focus_project = pid;
    }
    pub fn poll_wanted(&self, panel_visible: bool) -> bool {
        self.focus_project
            .and_then(|pid| self.states.get(&pid))
            .is_some_and(|state| host::poll_wanted(state, panel_visible))
    }
    pub fn tick(&mut self, now: Instant, panel_visible: bool) {
        if !panel_visible {
            return;
        }
        let Some(pid) = self.focus_project else {
            return;
        };
        let Some(io) = self.io() else {
            return;
        };
        let Some(state) = self.states.get_mut(&pid) else {
            return;
        };
        poll_if_due(state, pid, now, &io);
    }
    pub fn dismiss_notice(&mut self) {
        self.notice.dismiss();
    }

    pub(crate) fn cancel_dialog(&mut self, pid: i64) {
        if self.states.contains_key(&pid) {
            let _ = self.update(
                dozer_panel_group_chat::Message::NewGroupDialogCancel(pid),
                |_| true,
            );
        }
    }
    pub fn group_create_overlay(&self, project: Option<&ProjectInfo>) -> Option<Element<'_, Msg>> {
        let project = project?;
        let state = self.states.get(&project.id)?;
        if !state.new_group_dialog_open() {
            return None;
        }
        let display = self.displays.get(&project.id)?;
        let card =
            dozer_panel_group_chat::dialog::new_group_dialog_card(state, display, project.id)?;
        let tokens = byteui::theme::color::current();
        let card = column![
            card.map(Msg::GroupChat),
            text(MENTION_READ_ONLY_NOTE)
                .size(byteui::theme::font::body())
                .color(tokens.dim),
        ]
        .spacing(8);
        Some(super::group_chat_overlay::dialog_overlay(
            card.into(),
            Msg::GroupChat(dozer_panel_group_chat::Message::NewGroupDialogCancel(
                project.id,
            )),
        ))
    }
    /// The chat page itself is a native child above this area. What iced draws here
    /// is only what is visible without it: the failure shell or the
    /// "no project / no state yet" text.
    fn content_view(&self) -> Element<'_, Msg> {
        let tokens = byteui::theme::color::current();
        if let Some(reason) = self.page_failure() {
            let body = column![
                text("群聊内容页加载失败")
                    .size(byteui::theme::font::subtitle())
                    .color(tokens.cream),
                text(reason)
                    .size(byteui::theme::font::body())
                    .color(tokens.dim),
                iced::widget::button(text("重试").size(byteui::theme::font::body()))
                    .on_press(Msg::GroupChatRetry),
            ]
            .spacing(8);
            return container(body)
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }
        container(column![])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    pub fn view<'a>(&'a self, project: Option<&ProjectInfo>, area: &BodyArea) -> Element<'a, Msg> {
        let tokens = byteui::theme::color::current();
        let Some(project) = project else {
            return placeholder("请先打开一个 Project", "");
        };
        let Some(state) = self.states.get(&project.id) else {
            return placeholder("选择一个群聊,或点击＋新建", "");
        };
        let display = &self.displays[&project.id];
        display.configure(area);
        let list_width = area.pair.map(|p| p.first.w).unwrap_or(area.rect.w).max(0.0);
        let list = dozer_panel_group_chat::view::list_pane(
            state,
            display,
            project.id,
            Length::Fixed(list_width),
            iced_widget::core::Border::default(),
        )
        .map(Msg::GroupChat);
        let list = if let Some(notice) = self.notice.view(Msg::GroupChatNoticeDismiss) {
            column![notice, list].height(Length::Fill).into()
        } else {
            list
        };
        let content = self.content_view();
        pair_view(
            area,
            list,
            content,
            Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
            tokens.bg,
            tokens.panel,
        )
    }
}

fn placeholder(first: &str, second: &str) -> Element<'static, Msg> {
    let tokens = byteui::theme::color::current();
    let body = column![
        text(first.to_owned())
            .size(byteui::theme::font::body())
            .color(tokens.body),
        text(second.to_owned())
            .size(byteui::theme::font::body())
            .color(tokens.dim),
    ]
    .spacing(8);
    container(body)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dozer_model::GroupInfo;
    use dozer_panel_group_chat::Message as GcMessage;
    use dozer_services::{GroupChatService, ServiceError, ServiceErrorKind};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) calls: Mutex<Vec<String>>,
        pub(crate) groups: Mutex<Vec<GroupInfo>>,
    }
    impl GroupChatService for Fake {
        fn list_groups(
            &self,
            _pid: i64,
        ) -> dozer_services::BoxFuture<'_, Result<Vec<GroupInfo>, ServiceError>> {
            self.calls.lock().unwrap().push("list_groups".into());
            let groups = self.groups.lock().unwrap().clone();
            Box::pin(async move { Ok(groups) })
        }
        fn list_group_messages(
            &self,
            _gid: i64,
            _rev: i64,
            _limit: u32,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<(Vec<dozer_model::GroupMessageInfo>, i64), ServiceError>,
        > {
            self.calls
                .lock()
                .unwrap()
                .push("list_group_messages".into());
            Box::pin(async { Ok((vec![], 0)) })
        }
        fn create_group(
            &self,
            _pid: i64,
            topic: String,
        ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
            self.calls.lock().unwrap().push(format!("create:{topic}"));
            let g = GroupInfo {
                id: 1,
                project_id: 1,
                topic,
                created_ms: 0,
                members: vec![],
            };
            Box::pin(async move { Ok(g) })
        }
        fn delete_group(
            &self,
            gid: i64,
        ) -> dozer_services::BoxFuture<'_, Result<(), ServiceError>> {
            self.calls.lock().unwrap().push(format!("delete:{gid}"));
            Box::pin(async { Ok(()) })
        }
        fn add_group_member(
            &self,
            _gid: i64,
            agent: dozer_model::AgentKind,
            _h: String,
            _r: String,
        ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
            self.calls.lock().unwrap().push(format!("add:{agent:?}"));
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn update_group_member(
            &self,
            _m: i64,
            _h: String,
            _r: String,
        ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn remove_group_member(
            &self,
            _m: i64,
        ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn post_group_message(
            &self,
            _g: i64,
            _t: String,
        ) -> dozer_services::BoxFuture<'_, Result<dozer_services::PostedMessages, ServiceError>>
        {
            self.calls.lock().unwrap().push("post".into());
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn cancel_group(
            &self,
            g: i64,
            _s: dozer_model::GroupCancelScope,
        ) -> dozer_services::BoxFuture<'_, Result<(), ServiceError>> {
            self.calls.lock().unwrap().push(format!("cancel:{g}"));
            Box::pin(async { Ok(()) })
        }
        fn retry_group_message(
            &self,
            _m: i64,
        ) -> dozer_services::BoxFuture<'_, Result<dozer_model::GroupMessageInfo, ServiceError>>
        {
            self.calls.lock().unwrap().push("retry".into());
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn push_group_message_to_todo(
            &self,
            m: i64,
            _t: String,
        ) -> dozer_services::BoxFuture<'_, Result<dozer_model::TodoInfo, ServiceError>> {
            self.calls.lock().unwrap().push(format!("push_todo:{m}"));
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
    }

    pub(crate) fn services(fake: Arc<dyn GroupChatService>) -> Services {
        use crate::adapters::project::UnavailableService;
        let un = |r: &str| Arc::new(UnavailableService::new(r));
        Services {
            usage: un("x"),
            todo: un("x"),
            project: un("x"),
            transcript: un("x"),
            memory: un("x"),
            group_chat: fake,
            conversation: un("x"),
            summary: un("x"),
            codehealth: un("x"),
            context: un("x"),
            edit_history: un("x"),
            bookmark: un("x"),
            locale: un("x"),
            daemon: un("x"),
            session: un("x"),
        }
    }

    pub(crate) fn project(id: i64) -> ProjectInfo {
        ProjectInfo {
            id,
            path: format!("/tmp/p{id}"),
            name: format!("p{id}"),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    #[test]
    fn messages_for_one_project_never_touch_another() {
        let fake = Arc::new(Fake::default());
        let mut panel = GroupChatPanel::new(services(fake));
        let p1 = project(1);
        let p2 = project(2);
        panel.ensure_state(&p1);
        panel.ensure_state(&p2);
        let _ = panel.update(GcMessage::GroupsLoaded(1, Ok(vec![])), |_| true);
        assert!(panel.state(2).unwrap().groups().is_empty());
    }

    #[test]
    fn late_messages_after_close_project_are_ignored() {
        let fake = Arc::new(Fake::default());
        let mut panel = GroupChatPanel::new(services(fake));
        let p = project(1);
        panel.ensure_state(&p);
        panel.close_project(1);
        let _ = panel.update(GcMessage::GroupsLoaded(1, Ok(vec![])), |_| true);
        assert!(panel.state(1).is_none());
    }

    #[test]
    fn the_new_group_dialog_always_carries_the_read_only_note() {
        assert_eq!(
            MENTION_READ_ONLY_NOTE,
            "被 @ 的成员只读地读取项目文件,目前仅 Claude、Codex、Qodercn 可以发言"
        );
        let fake = Arc::new(Fake::default());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        panel.ensure_state(&p);
        assert!(panel.group_create_overlay(Some(&p)).is_none());
        let _ = panel.update(GcMessage::NewGroupDialogToggle(1), |_| true);
        assert!(panel.group_create_overlay(Some(&p)).is_some());
    }

    #[test]
    fn the_failure_shell_names_the_reason_has_no_todo_text_and_retry_clears_it() {
        let fake = Arc::new(Fake::default());
        let mut panel = GroupChatPanel::new(services(fake));
        assert!(panel.page_failure().is_none());
        panel.push.set_failed(LocalizedText::literal(
            dozer_panel_group_chat::i18n::HOST_OWNER,
            "页面加载超时",
        ));
        let reason = panel.page_failure().expect("failed");
        assert!(reason.contains("页面加载超时"));
        assert!(!reason.contains("待办"));
        let _ = panel.content_view();
        panel.retry_page();
        assert!(panel.page_failure().is_none());
        let _ = panel.content_view();
    }

    #[test]
    fn first_visibility_creates_state_and_loads_groups_once() {
        let fake = Arc::new(Fake::default());
        fake.groups.lock().unwrap().push(GroupInfo {
            id: 1,
            project_id: 1,
            topic: "g".into(),
            created_ms: 0,
            members: vec![],
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake.clone()));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        let _ = panel.activate(&p, std::time::Instant::now());
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
        let mut rx = panel.rx.take().unwrap();
        while let Ok(m) = rx.try_recv() {
            let _ = panel.update(m, |_| true);
        }
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(50)).await });
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| *c == "list_groups")
                .count(),
            1
        );
        assert_eq!(panel.state(1).unwrap().groups().len(), 1);
        assert!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c == "list_group_messages")
        );
    }

    #[test]
    fn service_failure_surfaces_as_a_dismissable_notice() {
        struct Failing;
        impl GroupChatService for Failing {
            fn list_groups(
                &self,
                _pid: i64,
            ) -> dozer_services::BoxFuture<'_, Result<Vec<GroupInfo>, ServiceError>> {
                Box::pin(async {
                    Err(ServiceError::new(
                        ServiceErrorKind::Unavailable,
                        "daemon down",
                    ))
                })
            }
            fn list_group_messages(
                &self,
                _gid: i64,
                _rev: i64,
                _limit: u32,
            ) -> dozer_services::BoxFuture<
                '_,
                Result<(Vec<dozer_model::GroupMessageInfo>, i64), ServiceError>,
            > {
                Box::pin(async {
                    Err(ServiceError::new(
                        ServiceErrorKind::Unavailable,
                        "daemon down",
                    ))
                })
            }
            fn create_group(
                &self,
                _pid: i64,
                _t: String,
            ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
                Box::pin(async {
                    Err(ServiceError::new(
                        ServiceErrorKind::Unavailable,
                        "daemon down",
                    ))
                })
            }
            fn delete_group(
                &self,
                _gid: i64,
            ) -> dozer_services::BoxFuture<'_, Result<(), ServiceError>> {
                Box::pin(async { Ok(()) })
            }
            fn add_group_member(
                &self,
                _g: i64,
                _a: dozer_model::AgentKind,
                _h: String,
                _r: String,
            ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
            fn update_group_member(
                &self,
                _m: i64,
                _h: String,
                _r: String,
            ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
            fn remove_group_member(
                &self,
                _m: i64,
            ) -> dozer_services::BoxFuture<'_, Result<GroupInfo, ServiceError>> {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
            fn post_group_message(
                &self,
                _g: i64,
                _t: String,
            ) -> dozer_services::BoxFuture<'_, Result<dozer_services::PostedMessages, ServiceError>>
            {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
            fn cancel_group(
                &self,
                _g: i64,
                _s: dozer_model::GroupCancelScope,
            ) -> dozer_services::BoxFuture<'_, Result<(), ServiceError>> {
                Box::pin(async { Ok(()) })
            }
            fn retry_group_message(
                &self,
                _m: i64,
            ) -> dozer_services::BoxFuture<'_, Result<dozer_model::GroupMessageInfo, ServiceError>>
            {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
            fn push_group_message_to_todo(
                &self,
                _m: i64,
                _t: String,
            ) -> dozer_services::BoxFuture<'_, Result<dozer_model::TodoInfo, ServiceError>>
            {
                Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "x")) })
            }
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(Arc::new(Failing)));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        let _ = panel.activate(&p, std::time::Instant::now());
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
        let mut rx = panel.rx.take().unwrap();
        while let Ok(m) = rx.try_recv() {
            let _ = panel.update(m, |_| true);
        }
        assert!(panel.notice.text().is_some());
        panel.dismiss_notice();
        assert!(panel.notice.text().is_none());
    }

    #[test]
    fn dialog_opens_and_cancel_closes_without_creating_a_group() {
        let fake = Arc::new(Fake::default());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake.clone()));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(GcMessage::NewGroupDialogToggle(1), |_| true);
        assert!(panel.state(1).unwrap().new_group_dialog_open());
        assert!(panel.group_create_overlay(Some(&p)).is_some());
        let _ = panel.update(GcMessage::NewGroupDialogCancel(1), |_| true);
        assert!(!panel.state(1).unwrap().new_group_dialog_open());
        assert!(panel.group_create_overlay(Some(&p)).is_none());
        assert!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| !c.starts_with("create:"))
        );
    }

    #[test]
    fn creating_groups_never_posts_or_retries() {
        let fake = Arc::new(Fake::default());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake.clone()));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(GcMessage::NewGroupDialogToggle(1), |_| true);
        let _ = panel.update(
            GcMessage::NewGroupTopicAction(
                1,
                iced_widget::text_editor::Action::Edit(iced_widget::text_editor::Edit::Insert('x')),
            ),
            |_| true,
        );
        let _ = panel.update(GcMessage::NewGroupSubmit(1), |_| true);
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(80)).await });
        let mut rx = panel.rx.take().unwrap();
        while let Ok(m) = rx.try_recv() {
            let _ = panel.update(m, |_| true);
        }
        let calls = fake.calls.lock().unwrap();
        assert!(
            calls.iter().all(|c| c != "post" && c != "retry"),
            "{calls:?}"
        );
    }

    #[test]
    fn hidden_tick_never_starts_a_request() {
        let fake = Arc::new(Fake::default());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake.clone()));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        panel.ensure_state(&p);
        panel.set_focus(Some(1));
        panel.tick(std::time::Instant::now(), false);
        rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(30)).await });
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn polling_is_gated_by_the_focused_project() {
        let fake = Arc::new(Fake::default());
        let mut panel = GroupChatPanel::new(services(fake));
        panel.ensure_state(&project(1));
        panel.ensure_state(&project(2));
        assert!(!panel.poll_wanted(true));
        panel.set_focus(Some(1));
        assert!(panel.poll_wanted(true));
        panel.set_focus(None);
        assert!(!panel.poll_wanted(true));
    }

    #[test]
    fn empty_topic_submit_keeps_the_dialog_open() {
        let fake = Arc::new(Fake::default());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut panel = GroupChatPanel::new(services(fake.clone()));
        panel.attach_handle(rt.handle().clone());
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(GcMessage::NewGroupDialogToggle(1), |_| true);
        let _ = panel.update(GcMessage::NewGroupSubmit(1), |_| true);
        assert!(panel.state(1).unwrap().new_group_dialog_open());
        assert!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| !c.starts_with("create:"))
        );
    }
}
