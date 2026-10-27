//! Group-chat list panel host: per-project library state, routing, polling and
//! the temporary native content placeholder (S8c owns the chat webview).
use std::collections::HashMap;
use std::time::Instant;

use bytehost_chrome::{BodyArea, Divider, ShellMsg, pair_view};
use dozer_model::ProjectInfo;
use dozer_panel_group_chat::{
    Message, WorkspaceState, host, on_activate, poll_if_due, run_effect, update,
};
use dozer_panel_host::PanelIo;
use dozer_services::Services;
use iced::widget::{column, container, text};
use iced::{Element, Length, Task};

use super::group_chat_display::GroupChatDisplay;
use super::notice::NoticeBar;
use crate::product::Msg;

pub struct GroupChatPanel {
    states: HashMap<i64, WorkspaceState>,
    displays: HashMap<i64, GroupChatDisplay>,
    services: Services,
    handle: Option<tokio::runtime::Handle>,
    focus_project: Option<i64>,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Message>>,
    pub notice: NoticeBar,
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
            .or_insert_with(|| GroupChatDisplay::new(project.id));
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

    pub(crate) fn collapse_message(&self) -> Msg {
        Msg::Shell(ShellMsg::TogglePane(crate::shell::group_chat_id()))
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
        Some(super::group_chat_overlay::dialog_overlay(
            card.map(Msg::GroupChat),
            Msg::GroupChat(dozer_panel_group_chat::Message::NewGroupDialogCancel(
                project.id,
            )),
        ))
    }
    fn content_placeholder(&self, pid: i64) -> Element<'_, Msg> {
        let Some(state) = self.states.get(&pid) else {
            return placeholder("选择一个群聊,或点击＋新建", "聊天内容即将可用", true);
        };
        let Some(group) = state
            .selected()
            .and_then(|id| state.groups().iter().find(|g| g.id == id))
        else {
            return placeholder("选择一个群聊,或点击＋新建", "聊天内容即将可用", true);
        };
        let tokens = byteui::theme::color::current();
        let mut body = column![
            text(group.topic.clone())
                .size(byteui::theme::font::subtitle())
                .color(tokens.cream),
        ]
        .spacing(6);
        for member in &group.members {
            body = body.push(
                text(format!(
                    "@{} · {}",
                    member.handle,
                    member.agent.display_label()
                ))
                .size(byteui::theme::font::body())
                .color(tokens.body),
            );
        }
        body = body.push(
            text("聊天内容即将可用")
                .size(byteui::theme::font::body())
                .color(tokens.dim),
        );
        container(body)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    pub fn view<'a>(&'a self, project: Option<&ProjectInfo>, area: &BodyArea) -> Element<'a, Msg> {
        let tokens = byteui::theme::color::current();
        let Some(project) = project else {
            return placeholder("请先打开一个 Project", "聊天内容即将可用", false);
        };
        let Some(state) = self.states.get(&project.id) else {
            return placeholder("选择一个群聊,或点击＋新建", "聊天内容即将可用", false);
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
        let collapse = iced::widget::tooltip(
            iced::widget::button(byteui::interaction::icons::view(
                display.collapse_icon(),
                byteui::theme::icon_size::row(),
                byteui::theme::color::current().dim,
            ))
            .on_press(self.collapse_message()),
            iced::widget::container(
                iced::widget::text(display.collapse_label()).size(byteui::theme::font::body()),
            )
            .padding(6)
            .style(iced::widget::container::rounded_box),
            iced::widget::tooltip::Position::Bottom,
        )
        .delay(super::hover::TOOLTIP_DELAY);
        let list: Element<'_, Msg> = column![
            iced::widget::row![iced::widget::space::horizontal(), collapse].padding([0, 8]),
            list
        ]
        .height(Length::Fill)
        .into();
        let list = if let Some(notice) = self.notice.view(Msg::GroupChatNoticeDismiss) {
            column![notice, list].height(Length::Fill).into()
        } else {
            list
        };
        let content = self.content_placeholder(project.id);
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

fn placeholder(first: &str, second: &str, with_notice: bool) -> Element<'static, Msg> {
    let tokens = byteui::theme::color::current();
    let _ = with_notice;
    container(
        column![
            text(first.to_owned())
                .size(byteui::theme::font::body())
                .color(tokens.body),
            text(second.to_owned())
                .size(byteui::theme::font::body())
                .color(tokens.dim),
        ]
        .spacing(8)
        .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::GroupInfo;
    use dozer_panel_group_chat::Message as GcMessage;
    use dozer_services::{GroupChatService, ServiceError, ServiceErrorKind};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        groups: Mutex<Vec<GroupInfo>>,
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
            _gid: i64,
        ) -> dozer_services::BoxFuture<'_, Result<(), ServiceError>> {
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
            self.calls.lock().unwrap().push("retry".into());
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
        fn push_group_message_to_todo(
            &self,
            _m: i64,
            _t: String,
        ) -> dozer_services::BoxFuture<'_, Result<dozer_model::TodoInfo, ServiceError>> {
            Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "no")) })
        }
    }

    fn services(fake: Arc<dyn GroupChatService>) -> Services {
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

    fn project(id: i64) -> ProjectInfo {
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
    fn no_todo_text_anywhere_in_the_view() {
        // S8b content is a placeholder; no todo entry exists until S8c.
        let fake = Arc::new(Fake::default());
        let mut panel = GroupChatPanel::new(services(fake));
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(GcMessage::GroupsLoaded(1, Ok(vec![])), |_| true);
        let _ = panel.content_placeholder(1);
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
