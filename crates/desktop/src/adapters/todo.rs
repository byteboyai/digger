//! 待办面板的 Digger 宿主适配层(S3b-1 Task 2)。
//!
//! 库 `dozer-panel-todo` 负责状态机、筛选、排序、视图与协议;这里只做产品侧:
//! 每项目一份 `WorkspaceState`、注入 `TodoService`、把异步结果按请求令牌路由、
//! 面板可见时的 1 秒轮询,以及库消息里少数"内核拦截"项的去向。
//!
//! 写失败没有 Toast(spec Q4):库把错误放进 outbox,产品排空后落 stderr,
//! 列表下一次刷新会纠正乐观更新。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dozer_model::AgentKind;
use dozer_panel_todo::{Message, WorkspaceState};
use dozer_services::{Services, TodoService};
use iced::Element;

use crate::product::Msg;
use iced_widget::core::{Border, Padding};

/// 面板可见时的刷新节奏,与 Dozer `TODO_POLL_INTERVAL` 同值。
pub const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// 面板需要产品落地的少数动作;其余都由库自己改状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoEffect {
    /// 收起 / 展开列表栏(骨架的 PaneStore 记账)。
    ToggleListCollapse,
    /// 把任务指派给某个 agent(只写归属,不执行)。
    AssignAgent { todo_id: i64, agent: AgentKind },
}

pub struct TodoPanel {
    states: HashMap<i64, WorkspaceState>,
    current: Option<i64>,
    service: Arc<dyn TodoService>,
    /// 指派 agent 走 `Client` 固有方法(Task 5);先保留,避免到 Task 5 再改构造签名。
    #[allow(dead_code)]
    client: dozer_client::Client,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Message>>,
    last_poll: Option<Instant>,
    pub native: crate::adapters::todo_native::Native,
}

impl TodoPanel {
    pub fn new(service: Arc<dyn TodoService>, client: dozer_client::Client) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            states: HashMap::new(),
            current: None,
            service,
            client,
            handle: None,
            tx,
            rx: Some(rx),
            last_poll: None,
            native: crate::adapters::todo_native::Native::new(),
        }
    }

    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Message>> {
        self.rx.take()
    }

    /// 切当前项目(不丢别的项目的状态)。
    pub fn set_project(&mut self, project_id: Option<i64>) {
        if self.current != project_id
            && let Some(old) = self.current
            && let Some(state) = self.states.get(&old)
        {
            // 切走后旧项目的在途结果作废:即便稍后切回来,也不能被上一轮
            // 的迟到 Loaded 覆盖。
            state.requests.invalidate("todos");
            state.requests.invalidate("categories");
        }
        self.current = project_id;
        self.native.set_project(project_id);
        if let Some(id) = project_id {
            self.states.entry(id).or_default();
        }
    }

    pub fn current_project(&self) -> Option<i64> {
        self.current
    }

    /// 取某个项目的状态(测试与产品视图用)。
    pub fn state(&self, project_id: i64) -> Option<&WorkspaceState> {
        self.states.get(&project_id)
    }

    pub fn current_state(&self) -> Option<&WorkspaceState> {
        self.current.and_then(|id| self.states.get(&id))
    }

    /// 面板切入:拉一次列表与分类树。没有当前项目或没有运行时句柄时什么都不做。
    pub fn activate(&mut self, project_path: Option<PathBuf>) {
        let (Some(project_id), Some(handle)) = (self.current, self.handle.clone()) else {
            return;
        };
        let services = self.services();
        let state = self.states.entry(project_id).or_default();
        let tx = self.tx.clone();
        let ctx = dozer_panel_host::ActivationCtx {
            project_id,
            project_path,
            io: dozer_panel_host::PanelIo::new(services, handle, move |message| {
                let _ = tx.send(message);
            }),
        };
        dozer_panel_todo::on_activate(state, &ctx);
    }

    /// 处理一条库消息,返回需要产品落地的效果。
    pub fn update(&mut self, msg: Message) -> Vec<TodoEffect> {
        // 库的少数变体由内核拦截,产品给出明确去向。
        match &msg {
            Message::ToggleListCollapse => return vec![TodoEffect::ToggleListCollapse],
            Message::TextInputMenuOpen(_)
            | Message::DetailClose
            | Message::DetailReplyInput(_)
            | Message::DetailReplySubmit
            | Message::CategoryContextMenuOpen(_)
            | Message::CategoryRowPress(_)
            | Message::Hover(_, _) => return Vec::new(),
            _ => {}
        }

        let project_id = match msg.result_project() {
            Some(pid) if Some(pid) != self.current => {
                // Review Focus 2:上一个项目的在途结果不能写进当前项目,也不能
                // 悄悄写回已切走的旧项目(请求令牌随切项目作废)。
                return Vec::new();
            }
            Some(pid) => pid,
            None => {
                let Some(pid) = self.current else {
                    return Vec::new();
                };
                pid
            }
        };

        let state = self.states.entry(project_id).or_default();
        let Some(handle) = self.handle.clone() else {
            // 尚未进入 iced 运行时(纯逻辑测试):只吸收纯状态结果,不发请求。
            match msg {
                Message::Loaded(items) => state.items = items,
                Message::CategoriesLoaded(categories) => state.categories = categories,
                Message::LoadFailed(_, _)
                | Message::Mutated(_)
                | Message::CategoryMutated(_)
                | Message::AsyncResult(..) => {}
                _ => {}
            }
            return Vec::new();
        };
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        dozer_panel_todo::update(state, msg, project_id, &service, &handle, move |message| {
            let _ = tx.send(message);
        });
        Vec::new()
    }

    /// 面板可见时的定时刷新。自限速,只刷新当前项目。
    pub fn poll(&mut self, now: Instant) {
        let (Some(project_id), Some(handle)) = (self.current, self.handle.clone()) else {
            return;
        };
        if self
            .last_poll
            .is_some_and(|last| now.duration_since(last) < POLL_INTERVAL)
        {
            return;
        }
        self.last_poll = Some(now);
        let state = self.states.entry(project_id).or_default();
        let tx = self.tx.clone();
        let emit = move |message| {
            let _ = tx.send(message);
        };
        dozer_panel_todo::request_todos_refresh(
            &state.requests,
            project_id,
            &self.service,
            &handle,
            emit.clone(),
        );
        dozer_panel_todo::request_categories_refresh(
            &state.requests,
            project_id,
            &self.service,
            &handle,
            emit,
        );
    }

    /// 页面事件 → 库的 `route_event` → 同一条 `Message` / 指派 / 详情路口。
    pub fn handle_webview_event(
        &mut self,
        event: dozer_panel_todo::TodoWebviewEvent,
    ) -> Vec<TodoEffect> {
        use dozer_panel_todo::Routed;
        let Some(project_id) = self.current else {
            return Vec::new();
        };
        let items = match self.states.get(&project_id) {
            Some(state) => state.items().to_vec(),
            None => return Vec::new(),
        };
        match dozer_panel_todo::route_event(&items, event) {
            Some(Routed::Message(message)) => self.update(message),
            Some(Routed::AssignAgent { idx, agent }) => items
                .get(idx)
                .map(|item| TodoEffect::AssignAgent {
                    todo_id: item.id,
                    agent,
                })
                .into_iter()
                .collect(),
            Some(Routed::OpenDetail { .. }) => {
                eprintln!("digger: todo detail not available in Digger yet"); // cli-output
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    /// 指派 agent:只写库里的归属字段,不触发执行。
    pub fn assign_agent(&self, todo_id: i64, agent: AgentKind) {
        let (Some(project_id), Some(handle)) = (self.current, self.handle.clone()) else {
            return;
        };
        let Some(state) = self.states.get(&project_id) else {
            return;
        };
        // 与 Dozer 同款:先让旧刷新作废,再取本次令牌;结果回来后走 Mutated,
        // 库会按既有约定重新拉一次列表。
        state.requests.invalidate("todos");
        let token = state.requests.begin(Some(project_id), "todos");
        let client = self.client.clone();
        let tx = self.tx.clone();
        handle.spawn(async move {
            let result = client
                .assign_todo_agent(todo_id, agent)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(Message::AsyncResult(
                token,
                Box::new(Message::Mutated(result)),
            ));
        });
    }

    /// 排空库的提示发件箱(产品落 stderr;没有 Toast 机制)。
    pub fn take_notices(&mut self) -> Vec<(String, String)> {
        let Some(state) = self.current.and_then(|id| self.states.get_mut(&id)) else {
            return Vec::new();
        };
        state
            .take_outbox()
            .into_iter()
            .map(|n| (format!("{:?}", n.level), n.text.fallback))
            .collect()
    }

    /// 排空只写日志的诊断(如 `diggerd` 不可用时的静默读失败)。
    pub fn take_diagnostics(&mut self) -> Vec<String> {
        let Some(state) = self.current.and_then(|id| self.states.get_mut(&id)) else {
            return Vec::new();
        };
        state.take_diagnostics()
    }

    /// 两栏视图(左分类树 | 右内容)。没有当前项目时给一块空态。
    pub fn panes(&self, collapsed: bool) -> (Element<'_, Msg>, Element<'_, Msg>) {
        let Some(state) = self.current_state() else {
            let empty: Element<'_, Msg> = iced::widget::text("请先打开一个 Project").into();
            return (empty, iced::widget::text("").into());
        };
        let display = TodoDisplay { collapsed };
        let (list, content) = dozer_panel_todo::view::view(
            &display,
            state,
            self.native.failed(),
            iced::Length::Fill,
            Border::default(),
            iced::Length::Fill,
            Border::default(),
        );
        (list.map(Msg::Todo), content.map(Msg::Todo))
    }

    pub fn list_view(&self, collapsed: bool) -> Element<'_, Msg> {
        self.panes(collapsed).0
    }

    pub fn content_view(&self, collapsed: bool) -> Element<'_, Msg> {
        self.panes(collapsed).1
    }

    pub fn sync_native(
        &mut self,
        window: iced::window::Id,
        visible: bool,
        rect: bytehost_core::LogicalRect,
    ) -> iced::Task<crate::adapters::todo_native::Event> {
        let empty = WorkspaceState::default();
        let state = self
            .current
            .and_then(|id| self.states.get(&id))
            .unwrap_or(&empty);
        self.native.sync(window, visible, rect, state)
    }

    /// 给库构造服务束:只有 `todo` 是真实的,其余全是 `UnavailableService`。
    fn services(&self) -> Services {
        use crate::adapters::project::UnavailableService;
        let unavailable =
            |reason: &str| -> Arc<UnavailableService> { Arc::new(UnavailableService::new(reason)) };
        Services {
            usage: unavailable("Todo 面板不用用量服务"),
            todo: Arc::clone(&self.service),
            project: unavailable("Todo 面板不用项目服务"),
            transcript: unavailable("Todo 面板不用转录服务"),
            memory: unavailable("Todo 面板不用记忆服务"),
            group_chat: unavailable("Todo 面板不用群聊服务"),
            conversation: unavailable("Todo 面板不用会话服务"),
            summary: unavailable("Todo 面板不用总结服务"),
            codehealth: unavailable("Todo 面板不用健康服务"),
            context: unavailable("Todo 面板不用上下文服务"),
            edit_history: unavailable("Todo 面板不用编辑历史服务"),
            bookmark: unavailable("Todo 面板不用书签服务"),
            locale: unavailable("Todo 面板不用语言服务"),
            daemon: unavailable("Todo 面板不用 daemon 服务"),
            session: unavailable("Todo 面板不用 session 服务"),
        }
    }
}

/// 待办的展示适配:面板头、折叠按钮、分隔线与内边距。悬停动画在 Digger
/// 一期不做(与 Usage 的显式最小口径一致),返回 0。
pub(crate) struct TodoDisplay {
    collapsed: bool,
}

impl dozer_panel_todo::adapter::PanelDisplay for TodoDisplay {
    fn hover_progress(&self, _slot: bytehost_desktop::HoverSlot) -> f32 {
        0.0
    }

    fn list_collapsed(&self) -> bool {
        self.collapsed
    }

    fn list_collapse_button<'a, M: Clone + 'a>(
        &self,
        collapsed: bool,
        tooltip_collapse: String,
        tooltip_expand: String,
        on_select: M,
        _on_hover: impl Fn(bool) -> M + 'a,
    ) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
        use byteui::interaction::icons::{self, IconKind};
        use iced::widget::{button, container, text, tooltip};
        let (icon, tip) = if collapsed {
            (IconKind::PanelRightOpen, tooltip_expand)
        } else {
            (IconKind::PanelRightClose, tooltip_collapse)
        };
        tooltip(
            button(icons::view(
                icon,
                byteui::theme::icon_size::row(),
                byteui::theme::color::current().dim,
            ))
            .on_press(on_select)
            .style(crate::adapters::collapse_button_style()),
            container(text(tip).size(byteui::theme::font::body()))
                .padding(6)
                .style(container::rounded_box),
            tooltip::Position::Bottom,
        )
        .into()
    }

    fn panel_head<'a, M: 'a>(
        &self,
        icon: byteui::interaction::icons::IconKind,
        title: String,
    ) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
        use iced::widget::{column, row, text};
        let tokens = byteui::theme::color::current();
        column![
            row![
                byteui::interaction::icons::view(
                    icon,
                    byteui::theme::icon_size::row(),
                    tokens.tab_active_border,
                ),
                text(title)
                    .size(byteui::theme::font::title())
                    .color(tokens.tab_active_border),
            ]
            .spacing(8)
            .height(iced::Length::Fixed(
                byteui::theme::geometry::tab_button_size()
            ))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(4)
        .into()
    }

    fn header_padding(&self) -> Padding {
        Padding::from([8.0, 20.0])
    }

    fn tab_divider<'a, M: 'a>(
        &self,
    ) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
        byteui::layout::divider::horizontal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::{CategoryInfo, TodoInfo, TodoStoredStatus};
    use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeData {
        items: HashMap<i64, Vec<TodoInfo>>,
        categories: HashMap<i64, Vec<CategoryInfo>>,
        next_id: i64,
    }

    struct FakeTodo {
        data: Mutex<FakeData>,
        fail: Mutex<Option<ServiceErrorKind>>,
        list_calls: AtomicUsize,
        category_calls: AtomicUsize,
        mutations: Mutex<Vec<String>>,
        /// 直接让下一次读失败。
        down: AtomicBool,
    }

    impl FakeTodo {
        fn new() -> Self {
            Self {
                data: Mutex::new(FakeData {
                    next_id: 1,
                    ..Default::default()
                }),
                fail: Mutex::new(None),
                list_calls: AtomicUsize::new(0),
                category_calls: AtomicUsize::new(0),
                mutations: Mutex::new(Vec::new()),
                down: AtomicBool::new(false),
            }
        }

        fn seed(&self, project_id: i64, items: Vec<TodoInfo>) {
            self.data.lock().unwrap().items.insert(project_id, items);
        }

        fn record(&self, call: impl Into<String>) {
            self.mutations.lock().unwrap().push(call.into());
        }

        fn error(&self) -> Option<ServiceError> {
            if self.down.load(Ordering::SeqCst) {
                return Some(ServiceError::new(
                    ServiceErrorKind::Unavailable,
                    "daemon 不可用",
                ));
            }
            self.fail
                .lock()
                .unwrap()
                .map(|kind| ServiceError::new(kind, "fake failure"))
        }
    }

    fn todo(id: i64, project_id: i64, text: &str) -> TodoInfo {
        TodoInfo {
            id,
            project_id,
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
        }
    }

    impl TodoService for FakeTodo {
        fn list_todos(
            &self,
            project_id: i64,
        ) -> BoxFuture<'_, Result<Vec<TodoInfo>, ServiceError>> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            let result = match self.error() {
                Some(e) => Err(e),
                None => Ok(self
                    .data
                    .lock()
                    .unwrap()
                    .items
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_default()),
            };
            Box::pin(async move { result })
        }

        fn list_categories(
            &self,
            project_id: i64,
        ) -> BoxFuture<'_, Result<Vec<CategoryInfo>, ServiceError>> {
            self.category_calls.fetch_add(1, Ordering::SeqCst);
            let result = match self.error() {
                Some(e) => Err(e),
                None => Ok(self
                    .data
                    .lock()
                    .unwrap()
                    .categories
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_default()),
            };
            Box::pin(async move { result })
        }

        fn add_todo(
            &self,
            project_id: i64,
            text: String,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("add:{project_id}:{text}"));
            let mut data = self.data.lock().unwrap();
            let id = data.next_id;
            data.next_id += 1;
            data.items
                .entry(project_id)
                .or_default()
                .push(todo(id, project_id, &text));
            Box::pin(async { Ok(()) })
        }

        fn toggle_todo(&self, id: i64, done: bool) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("toggle:{id}:{done}"));
            Box::pin(async { Ok(()) })
        }

        fn edit_todo_text(&self, id: i64, text: String) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("edit:{id}:{text}"));
            Box::pin(async { Ok(()) })
        }

        fn reorder_todo(
            &self,
            id: i64,
            after_id: Option<i64>,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("reorder:{id}:{after_id:?}"));
            Box::pin(async { Ok(()) })
        }

        fn set_todo_plan_date(
            &self,
            id: i64,
            plan_date: Option<String>,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("plan:{id}:{plan_date:?}"));
            Box::pin(async { Ok(()) })
        }

        fn set_todo_status(
            &self,
            id: i64,
            status: TodoStoredStatus,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("status:{id}:{status:?}"));
            Box::pin(async { Ok(()) })
        }

        fn set_todo_category(
            &self,
            id: i64,
            category_id: Option<i64>,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("category:{id}:{category_id:?}"));
            Box::pin(async { Ok(()) })
        }

        fn add_category(
            &self,
            project_id: i64,
            parent_id: Option<i64>,
            name: String,
        ) -> BoxFuture<'_, Result<CategoryInfo, ServiceError>> {
            self.record(format!("add_cat:{project_id}:{parent_id:?}:{name}"));
            Box::pin(async {
                Ok(CategoryInfo {
                    id: 1,
                    project_id: 1,
                    parent_id: None,
                    name: "新分类".into(),
                    rank: 0,
                    created_ms: 0,
                    auto_poll_enabled: false,
                })
            })
        }

        fn rename_category(
            &self,
            id: i64,
            name: String,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("rename_cat:{id}:{name}"));
            Box::pin(async { Ok(()) })
        }

        fn delete_category(&self, id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("delete_cat:{id}"));
            Box::pin(async { Ok(()) })
        }

        fn reparent_category(
            &self,
            id: i64,
            new_parent_id: Option<i64>,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("reparent_cat:{id}:{new_parent_id:?}"));
            Box::pin(async { Ok(()) })
        }

        fn move_category_sibling(
            &self,
            id: i64,
            direction: dozer_model::CategoryMoveDirection,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.record(format!("move_cat:{id}:{direction:?}"));
            Box::pin(async { Ok(()) })
        }
    }

    fn panel(fake: Arc<FakeTodo>) -> (TodoPanel, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let service: Arc<dyn TodoService> = fake;
        let mut p = TodoPanel::new(
            service,
            dozer_client::Client::new(PathBuf::from("/tmp/digger-todo-test.sock")),
        );
        p.attach_handle(rt.handle().clone());
        (p, rt)
    }

    fn drain(p: &mut TodoPanel, rt: &tokio::runtime::Runtime) {
        // 让 spawn 的任务先跑完再取结果。
        rt.block_on(async { tokio::task::yield_now().await });
        let messages = {
            let Some(rx) = p.rx.as_mut() else { return };
            let mut messages = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
            messages
        };
        for msg in messages {
            p.update(msg);
        }
    }

    #[test]
    fn activate_loads_todos_and_categories_for_the_current_project() {
        let fake = Arc::new(FakeTodo::new());
        fake.seed(7, vec![todo(1, 7, "买牛奶"), todo(2, 7, "写报告")]);
        let (mut p, rt) = panel(fake);
        p.set_project(Some(7));
        p.activate(None);
        drain(&mut p, &rt);
        let state = p.state(7).expect("state");
        assert_eq!(state.items().len(), 2);
    }

    #[test]
    fn results_for_a_previous_project_are_dropped() {
        let fake = Arc::new(FakeTodo::new());
        fake.seed(1, vec![todo(1, 1, "A 的任务")]);
        fake.seed(2, vec![todo(2, 2, "B 的任务")]);
        let (mut p, rt) = panel(fake);
        p.set_project(Some(1));
        p.activate(None);
        // A 的结果还没取走就切到 B。
        p.set_project(Some(2));
        drain(&mut p, &rt);
        assert!(
            p.state(2).unwrap().items().is_empty(),
            "B 不能被 A 的结果污染"
        );
        p.set_project(Some(1));
        assert!(p.state(1).unwrap().items().is_empty(), "A 的在途结果应作废");
    }

    #[test]
    fn unavailable_reads_keep_the_last_list_and_stay_silent() {
        let fake = Arc::new(FakeTodo::new());
        fake.seed(
            7,
            vec![todo(1, 7, "一"), todo(2, 7, "二"), todo(3, 7, "三")],
        );
        let (mut p, rt) = panel(fake.clone());
        p.set_project(Some(7));
        p.activate(None);
        drain(&mut p, &rt);
        assert_eq!(p.state(7).unwrap().items().len(), 3);

        fake.down.store(true, Ordering::SeqCst);
        p.update(Message::LoadFailed(
            ServiceErrorKind::Unavailable,
            "down".into(),
        ));
        assert_eq!(p.state(7).unwrap().items().len(), 3);
        assert!(p.take_notices().is_empty());

        fake.down.store(false, Ordering::SeqCst);
        p.activate(None);
        drain(&mut p, &rt);
        assert_eq!(p.state(7).unwrap().items().len(), 3);
    }

    #[test]
    fn non_unavailable_read_failures_produce_a_notice() {
        let (mut p, _rt) = panel(Arc::new(FakeTodo::new()));
        p.set_project(Some(7));
        p.update(Message::LoadFailed(
            ServiceErrorKind::Rejected,
            "boom".into(),
        ));
        let notices = p.take_notices();
        assert_eq!(notices.len(), 1);
        assert!(!notices[0].1.is_empty());
    }

    #[test]
    fn states_are_kept_per_project() {
        let (mut p, _rt) = panel(Arc::new(FakeTodo::new()));
        p.set_project(Some(7));
        p.states.get_mut(&7).unwrap().items = vec![todo(1, 7, "7 的任务")];
        p.set_project(Some(8));
        assert!(p.state(8).unwrap().items().is_empty());
        p.set_project(Some(7));
        assert_eq!(p.state(7).unwrap().items().len(), 1);
    }

    #[test]
    fn poll_is_rate_limited_and_only_refreshes_the_current_project() {
        let fake = Arc::new(FakeTodo::new());
        let (mut p, rt) = panel(fake.clone());
        p.set_project(Some(7));
        let t0 = Instant::now();
        p.poll(t0);
        p.poll(t0 + Duration::from_millis(200));
        rt.block_on(async { tokio::time::sleep(Duration::from_millis(50)).await });
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fake.category_calls.load(Ordering::SeqCst), 1);
        p.poll(t0 + POLL_INTERVAL);
        rt.block_on(async { tokio::time::sleep(Duration::from_millis(50)).await });
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 2);

        p.set_project(None);
        p.poll(t0 + POLL_INTERVAL * 3);
        rt.block_on(async { tokio::time::sleep(Duration::from_millis(50)).await });
        assert_eq!(fake.list_calls.load(Ordering::SeqCst), 2, "没有项目不轮询");
    }

    #[test]
    fn every_library_message_has_an_explicit_disposition() {
        enum D {
            Effect,
            Ignored,
            Handled,
        }
        fn disposition(m: &Message) -> D {
            match m {
                Message::ToggleListCollapse => D::Effect,
                Message::TextInputMenuOpen(_)
                | Message::DetailClose
                | Message::DetailReplyInput(_)
                | Message::DetailReplySubmit
                | Message::CategoryContextMenuOpen(_)
                | Message::CategoryRowPress(_)
                | Message::Hover(_, _) => D::Ignored,
                Message::AsyncResult(..)
                | Message::Toggle(_)
                | Message::StatusPick(..)
                | Message::CalendarPick(..)
                | Message::ClearListRequest
                | Message::ClearListCancel
                | Message::ClearListConfirm
                | Message::Loaded(_)
                | Message::Mutated(_)
                | Message::CategoriesLoaded(_)
                | Message::LoadFailed(..)
                | Message::CategoryMutated(_)
                | Message::CategoryToggleExpand(_)
                | Message::CategorySelect(_)
                | Message::SelectView(_)
                | Message::CategoryNewChild(_)
                | Message::CategoryNewSibling(_)
                | Message::CategoryDelete(_)
                | Message::CategoryRenameStart(_)
                | Message::CategoryRenameEdit(_)
                | Message::CategoryRenameSubmit
                | Message::CategoryMoveSibling(..)
                | Message::CategoryDragOver(_)
                | Message::CategoryDragRelease(_)
                | Message::AddText(_)
                | Message::EditText(..)
                | Message::ReorderTo { .. }
                | Message::SetCategory(..)
                | Message::AddHeight(_)
                | Message::ContentRetry => D::Handled,
            }
        }
        // 编译期穷尽性就是本测试的目的:库新增变体会让上面的 match 编译失败。
        assert!(matches!(
            disposition(&Message::ToggleListCollapse),
            D::Effect
        ));
    }

    fn page_panel() -> (TodoPanel, tokio::runtime::Runtime, Arc<FakeTodo>) {
        let fake = Arc::new(FakeTodo::new());
        fake.seed(7, vec![todo(1, 7, "一"), todo(2, 7, "二")]);
        let (mut p, rt) = panel(fake.clone());
        p.set_project(Some(7));
        p.activate(None);
        drain(&mut p, &rt);
        (p, rt, fake)
    }

    #[test]
    fn page_events_become_the_same_messages_as_dozer() {
        use dozer_panel_todo::{SetStatusTarget, TodoWebviewEvent as Ev};

        let (mut p, rt, fake) = page_panel();
        for (event, expected) in [
            (
                Ev::Add {
                    text: "新任务".into(),
                },
                "add:7:新任务",
            ),
            (Ev::Toggle { id: 1 }, "toggle:1:true"),
            (
                Ev::EditText {
                    id: 2,
                    text: "改".into(),
                },
                "edit:2:改",
            ),
            (
                Ev::Reorder {
                    id: 2,
                    after_id: None,
                },
                "reorder:2:None",
            ),
            (
                Ev::SetStatus {
                    id: 1,
                    state: SetStatusTarget::Done,
                },
                "status:1:Done",
            ),
            (
                Ev::SetPlanDate {
                    id: 1,
                    date: "08-10".into(),
                },
                "plan:1:Some(\"08-10\")",
            ),
            (
                Ev::SetCategory {
                    id: 1,
                    category_id: Some(9),
                },
                "category:1:Some(9)",
            ),
        ] {
            assert!(p.handle_webview_event(event).is_empty(), "no host effect");
            drain(&mut p, &rt);
            assert!(
                fake.mutations.lock().unwrap().iter().any(|m| m == expected),
                "missing {expected}; got {:?}",
                fake.mutations.lock().unwrap()
            );
        }
    }

    #[test]
    fn an_event_for_a_task_that_no_longer_exists_is_ignored() {
        use dozer_panel_todo::TodoWebviewEvent as Ev;
        let (mut p, rt, fake) = page_panel();
        let before = fake.mutations.lock().unwrap().len();
        for event in [
            Ev::Toggle { id: 99 },
            Ev::EditText {
                id: 99,
                text: "x".into(),
            },
        ] {
            assert!(p.handle_webview_event(event).is_empty());
            drain(&mut p, &rt);
        }
        assert_eq!(fake.mutations.lock().unwrap().len(), before);
    }

    #[test]
    fn events_for_optimistic_placeholder_ids_are_ignored() {
        use dozer_panel_todo::TodoWebviewEvent as Ev;
        let (mut p, rt, fake) = page_panel();
        let before = fake.mutations.lock().unwrap().len();
        assert!(p.handle_webview_event(Ev::Toggle { id: -1 }).is_empty());
        drain(&mut p, &rt);
        assert_eq!(fake.mutations.lock().unwrap().len(), before);
    }

    #[test]
    fn assigning_an_agent_calls_the_client_and_refreshes() {
        use dozer_panel_todo::TodoWebviewEvent as Ev;
        let (mut p, _rt, _fake) = page_panel();
        let effects = p.handle_webview_event(Ev::AssignAgent {
            id: 2,
            agent: AgentKind::Codebuddy,
        });
        assert_eq!(
            effects,
            vec![TodoEffect::AssignAgent {
                todo_id: 2,
                agent: AgentKind::Codebuddy,
            }]
        );
    }

    #[test]
    fn open_detail_is_a_documented_no_op() {
        use dozer_panel_todo::TodoWebviewEvent as Ev;
        let (mut p, _rt, fake) = page_panel();
        let before = fake.mutations.lock().unwrap().len();
        assert!(p.handle_webview_event(Ev::OpenDetail { id: 1 }).is_empty());
        assert_eq!(fake.mutations.lock().unwrap().len(), before);
    }

    #[test]
    fn add_height_events_resize_the_native_rect() {
        use dozer_panel_todo::TodoWebviewEvent as Ev;
        let (mut p, _rt, _fake) = page_panel();
        let before = p.state(7).unwrap().add_input_height();
        assert!(
            p.handle_webview_event(Ev::AddHeight { px: 200.0 })
                .is_empty()
        );
        assert!(p.state(7).unwrap().add_input_height() > before);
    }

    #[test]
    fn assigning_an_agent_calls_the_client_and_refreshes_via_mutated() {
        use dozer_core::protocol::{PROTOCOL_VERSION, Reply, Request, decode_line, encode_line};
        use std::path::PathBuf;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let path = PathBuf::from(format!(
            "/tmp/digger-todo-assign-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let _ = dozer_core::paths::install(digger_paths::product());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);

        let fake = Arc::new(FakeTodo::new());
        fake.seed(7, vec![todo(2, 7, "原任务")]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = rt.enter();
        let listener = tokio::net::UnixListener::bind(&path).unwrap();

        rt.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = tokio::io::BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let reply = match decode_line::<Request>(&line) {
                            Ok(Request::Hello { .. }) => Reply::Hello {
                                protocol: PROTOCOL_VERSION,
                                product: "digger".into(),
                            },
                            Ok(Request::AssignTodoAgent { id, agent }) => {
                                seen.lock().unwrap().push((id, agent));
                                Reply::Todo {
                                    todo: todo(id, 7, "原任务"),
                                }
                            }
                            _ => Reply::Error {
                                message: "unexpected".into(),
                            },
                        };
                        let _ = w.write_all(encode_line(&reply).as_bytes()).await;
                    }
                });
            }
        });

        let service: Arc<dyn TodoService> = fake.clone();
        let mut p = TodoPanel::new(service, dozer_client::Client::new(path.clone()));
        p.attach_handle(rt.handle().clone());
        p.set_project(Some(7));
        let before = fake.list_calls.load(Ordering::SeqCst);
        p.assign_agent(2, AgentKind::Claude);
        rt.block_on(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        drain(&mut p, &rt);
        rt.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        drain(&mut p, &rt);
        assert_eq!(calls.lock().unwrap().as_slice(), &[(2, AgentKind::Claude)]);
        assert!(
            fake.list_calls.load(Ordering::SeqCst) > before,
            "Mutated(Ok) must trigger a list refresh"
        );
        let _ = std::fs::remove_file(path);
    }
}
