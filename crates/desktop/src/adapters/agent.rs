//! Agent 面板(S6d-1 壳 + S6d-2 会话):每项目一份库状态、懒启动、恢复、关闭与退出。
//!
//! 库 `dozer-panel-agent` 只持有**一个项目**的会话/页签状态;项目路由、恢复时机、
//! 关项目页签杀会话、关窗口只 detach 都在这里。所有守护进程调用都在后台任务里,
//! 结果经 `tx` 回到 iced 流(见 `product::boot`),UI 线程只做状态装配。
//!
//! **本期范围:** Task 2–4、6。键盘/输入法(Task 5)等 S6d-1 的输入法探针结果
//! 由用户填写后再做;`focus` 恒假、不接收按键(报告里记入"待用户")。

use dozer_model::ProjectInfo;
use dozer_panel_agent::adapter::{AgentCapabilities, HookInstaller};
use dozer_panel_agent::launch::PickerLaunch;
use dozer_panel_agent::restore::RestoredSession;
use dozer_panel_agent::state::AgentPanelState;
use dozer_panel_agent::tabs::TabBackend;
use dozer_panel_agent::update::{Message, UpdateCtx};
use dozer_services::SessionService;
use iced::{Element, Task};
use iced_widget::core::Rectangle;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentShellMsg {
    Retry,
    // 由 4b 的 `Daemon::ensure` 结果构造。
    Ensured(Result<(), String>),
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    // S6a:启动序列开始时置 Checking,收到 Ensured 后 Ready/Unavailable。
    #[default]
    Unknown,
    Checking,
    Ready,
    Unavailable(String),
}
#[derive(Debug, Default)]
pub struct AgentShell {
    pub status: Status,
}
/// 壳当前该显示哪一种内容(视图据此分派,也让分派可测)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body<'a> {
    NoProject,
    Unavailable(&'a str),
    Checking,
    Placeholder,
}
impl AgentShell {
    pub fn body(&self, has_project: bool) -> Body<'_> {
        match (&self.status, has_project) {
            (_, false) => Body::NoProject,
            (Status::Unavailable(reason), _) => Body::Unavailable(reason),
            (Status::Checking | Status::Unknown, _) => Body::Checking,
            (Status::Ready, _) => Body::Placeholder,
        }
    }
    pub fn view(&self, has_project: bool) -> Element<'_, AgentShellMsg> {
        match self.body(has_project) {
            Body::NoProject => column![text("请先打开一个 Project，再使用 Agent 面板。")]
                .padding(20)
                .into(),
            Body::Unavailable(reason) => {
                column![text(reason), button("重试").on_press(AgentShellMsg::Retry)]
                    .spacing(12)
                    .padding(20)
                    .into()
            }
            Body::Checking => column![text("正在连接 Agent 服务…")].padding(20).into(),
            Body::Placeholder => column![text("Agent 面板即将可用")].padding(20).into(),
        }
    }
    pub fn update(
        &mut self,
        msg: AgentShellMsg,
        ensure: impl FnOnce() -> Task<AgentShellMsg>,
    ) -> Task<AgentShellMsg> {
        match msg {
            AgentShellMsg::Retry if self.status == Status::Checking => Task::none(),
            AgentShellMsg::Retry => {
                self.status = Status::Checking;
                ensure()
            }
            AgentShellMsg::Ensured(Ok(())) => {
                self.status = Status::Ready;
                Task::none()
            }
            AgentShellMsg::Ensured(Err(error)) => {
                self.status = Status::Unavailable(error);
                Task::none()
            }
        }
    }
}

use iced::widget::{button, column, text};

/// 恢复结果经 iced 的 `Clone` 约束回流:接收端不可 `Clone`,包一层只 `take` 一次。
pub type RestoredResult = Arc<Mutex<Option<Vec<RestoredSession>>>>;

/// 选择器浮层的位置与所属项目。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickerMenu {
    pub pid: i64,
    pub at: Option<(f32, f32)>,
}

/// 退出前等待 kill 的总预算。
pub const EXIT_KILL_BUDGET: Duration = Duration::from_secs(3);

/// 每项目状态的 Agent 面板。
pub struct AgentPanel {
    pub shell: AgentShell,
    pub states: HashMap<i64, AgentPanelState>,
    /// 恢复在途的项目。
    starting: HashSet<i64>,
    svc: Arc<dyn SessionService>,
    hooks: Arc<dyn HookInstaller>,
    caps: AgentCapabilities,
    handle: Option<tokio::runtime::Handle>,
    tx: UnboundedSender<(i64, Message)>,
    rx: Mutex<Option<UnboundedReceiver<(i64, Message)>>>,
    /// 顶部一行错误提示(红字,可关闭)。Digger 一期没有 Toast。
    pub notice: Option<String>,
    /// 退出前必须等完的 kill 任务。
    critical: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// 每个项目的关闭代次:关项目页签时 +1。新建会话/恢复的 emit 闭包捕获当时的
    /// 代次,代次对不上就丢弃消息;若迟到的是 `Attached`,立刻 kill 那个会话
    /// (create 已在守护进程里成功,不能留孤儿)。
    epochs: Arc<Mutex<HashMap<i64, u64>>>,
    pub focus: bool,
    /// 终端块矩形:Task 5(指针焦点)消费;输入法探针结果出来前恒 `None`。
    #[allow(dead_code)]
    pub terminal_rect: Option<Rectangle>,
    last_grid: HashMap<i64, (u16, u16)>,
    /// 恢复开始时那一刻的网格:新会话按它创建,恢复完成前窗口又变了则用完成时的。
    start_grid: HashMap<i64, (u16, u16)>,
    pub picker: Option<PickerMenu>,
}

impl Default for AgentPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentPanel {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            shell: AgentShell::default(),
            states: HashMap::new(),
            starting: HashSet::new(),
            svc: crate::daemon::Daemon::new().sessions(),
            hooks: Arc::new(crate::adapters::agent_hooks::DiggerHooks::new()),
            // 能力全关(设计 D5):不总结、不装 MCP。
            caps: AgentCapabilities::default(),
            handle: tokio::runtime::Handle::try_current().ok(),
            tx,
            rx: Mutex::new(Some(rx)),
            notice: None,
            critical: Arc::new(Mutex::new(Vec::new())),
            epochs: Arc::new(Mutex::new(HashMap::new())),
            focus: false,
            terminal_rect: None,
            last_grid: HashMap::new(),
            start_grid: HashMap::new(),
            picker: None,
        }
    }

    /// 测试/嵌入:显式注入服务与 hook 安装器。
    #[allow(dead_code)] // S6d-2 测试注入;产品路径走 `new`。
    pub fn with_services(svc: Arc<dyn SessionService>, hooks: Arc<dyn HookInstaller>) -> Self {
        let mut panel = Self::new();
        panel.svc = svc;
        panel.hooks = hooks;
        panel
    }

    /// `boot` 在 iced 运行时里补挂(同 usage/project 面板)。
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 取走后台回投通道(只能取一次;`boot` 接成 iced 流)。
    pub fn take_receiver(&self) -> Option<UnboundedReceiver<(i64, Message)>> {
        self.rx.lock().ok().and_then(|mut r| r.take())
    }

    fn handle(&self) -> Option<tokio::runtime::Handle> {
        self.handle.clone()
    }

    fn track(&self, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut tasks) = self.critical.lock() {
            tasks.push(task);
        }
    }

    fn epoch(&self, pid: i64) -> u64 {
        self.epochs
            .lock()
            .ok()
            .and_then(|epochs| epochs.get(&pid).copied())
            .unwrap_or(0)
    }

    fn bump_epoch(&self, pid: i64) {
        if let Ok(mut epochs) = self.epochs.lock() {
            *epochs.entry(pid).or_insert(0) += 1;
        }
    }

    /// 后台回投闭包:带上项目 id 与**创建时**的关闭代次。项目页签关掉后,
    /// 旧任务再发消息一律丢弃;`Attached` 说明 create 已在守护进程里成功,
    /// 这里补一次 kill,避免留下孤儿会话。
    fn emitter(&self, pid: i64) -> impl Fn(Message) + Clone + Send + 'static {
        let tx = self.tx.clone();
        let epochs = Arc::clone(&self.epochs);
        let svc = Arc::clone(&self.svc);
        let handle = self.handle();
        let critical = Arc::clone(&self.critical);
        let captured = self.epoch(pid);
        move |message: Message| {
            let current = epochs
                .lock()
                .ok()
                .and_then(|epochs| epochs.get(&pid).copied())
                .unwrap_or(0);
            if current != captured {
                if let Message::Attached { info, .. } = message
                    && let Some(handle) = handle.clone()
                {
                    let svc = Arc::clone(&svc);
                    let task = handle.spawn(async move {
                        if let Err(error) = svc.kill(info.id).await {
                            eprintln!(
                                "digger: 关闭项目后回收迟到会话失败: {error}" // cli-output
                            );
                        }
                    });
                    if let Ok(mut tasks) = critical.lock() {
                        tasks.push(task);
                    }
                }
                return;
            }
            let _ = tx.send((pid, message));
        }
    }

    /// 面板首次可见(或切回时仍可见):恢复该项目的存活会话;没有就新开 shell。
    ///
    /// 同一项目只会启动一次:已有状态或恢复在途时什么都不做(Review Focus 2)。
    pub fn ensure_started(
        &mut self,
        project: &ProjectInfo,
        grid: (u16, u16),
    ) -> Task<crate::product::Msg> {
        if self.shell.status != Status::Ready
            || self.states.contains_key(&project.id)
            || self.starting.contains(&project.id)
            || self.handle().is_none()
        {
            return Task::none();
        }
        self.starting.insert(project.id);
        self.start_grid.insert(project.id, grid);
        let svc = Arc::clone(&self.svc);
        let pid = project.id;
        Task::perform(
            async move { dozer_panel_agent::restore::fetch_restored(&*svc, pid).await },
            move |sessions| {
                crate::product::Msg::AgentRestored(pid, Arc::new(Mutex::new(Some(sessions))))
            },
        )
    }

    /// 恢复结果到达:项目页签已关则丢弃并让 attach 接收端随 drop 关闭。
    pub fn on_restored(
        &mut self,
        pid: i64,
        project: Option<&ProjectInfo>,
        sessions: Vec<RestoredSession>,
        grid: (u16, u16),
    ) -> Task<crate::product::Msg> {
        self.starting.remove(&pid);
        let grid = self.start_grid.remove(&pid).unwrap_or(grid);
        let Some(project) = project.filter(|p| p.id == pid) else {
            return Task::none();
        };
        let Some(handle) = self.handle() else {
            return Task::none();
        };
        let mut state = AgentPanelState::new();
        let emit = self.emitter(pid);
        state.adopt_restored(sessions, &handle, emit);
        let empty = state.tabs.is_empty();
        self.states.insert(pid, state);
        if empty {
            // 库不自作主张:没有会话由调用方决定新开 shell(根在项目路径)。
            return self.update(
                pid,
                Message::PickerSelect(PickerLaunch::Agent(None)),
                Some(project),
                grid,
            );
        }
        Task::none()
    }

    /// 处理一条库消息。没有该项目状态的迟到消息直接忽略。
    pub fn update(
        &mut self,
        pid: i64,
        message: Message,
        project: Option<&ProjectInfo>,
        grid: (u16, u16),
    ) -> Task<crate::product::Msg> {
        let Some(project) = project.filter(|p| p.id == pid) else {
            return Task::none();
        };
        let Some(handle) = self.handle() else {
            return Task::none();
        };
        let emit = self.emitter(pid);
        let Some(state) = self.states.get_mut(&pid) else {
            return Task::none();
        };
        let critical = Arc::clone(&self.critical);
        let track = move |task: tokio::task::JoinHandle<()>| {
            if let Ok(mut tasks) = critical.lock() {
                tasks.push(task);
            }
        };
        let ctx = UpdateCtx {
            project,
            svc: &self.svc,
            handle: &handle,
            cols: grid.0,
            rows: grid.1,
            term_tab_bar_avail_px: 0.0,
            caps: self.caps,
            hooks: &self.hooks,
            track_exit_critical: &track,
        };
        dozer_panel_agent::update::update(state, message, &ctx, emit);
        let notices = state.take_outbox();
        let requests = state.take_requests();
        // Digger 一期忽略跨面板联动(没有会话列表 / git / 磁盘占用面板)。
        let _ = state.take_host_events();
        let picker_open = state.agent_picker_open;
        for notice in notices {
            let text = dozer_panel_agent::i18n::resolver()
                .resolve(&notice.text, "en")
                .text;
            eprintln!(
                "digger: agent 面板/{:?}: {text}", // cli-output
                notice.level
            );
            self.notice = Some(text);
        }
        for request in requests {
            let dozer_panel_agent::adapter::AgentRequest::ShowPicker { at } = request;
            self.picker = Some(PickerMenu { pid, at });
        }
        if !picker_open && self.picker.is_some_and(|p| p.pid == pid) {
            self.picker = None;
        }
        Task::none()
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// 关闭选择器浮层(点空白 / Esc):不改会话,只清掉库与产品的打开态。
    pub fn close_picker(&mut self) {
        if let Some(picker) = self.picker.take()
            && let Some(state) = self.states.get_mut(&picker.pid)
        {
            state.agent_picker_open = false;
        }
    }

    /// 关项目页签:作废该项目的在途回投、abort 已有转发、kill 全部存活会话、
    /// 移除状态。**不 abort 还在 create 的任务**:create 可能已经在守护进程里
    /// 成功,任务随后发来的 `Attached` 由 `emitter` 回收(kill),不留孤儿。
    pub fn close_project(&mut self, pid: i64) -> Task<crate::product::Msg> {
        self.bump_epoch(pid);
        self.starting.remove(&pid);
        self.start_grid.remove(&pid);
        self.last_grid.remove(&pid);
        if self.picker.is_some_and(|p| p.pid == pid) {
            self.picker = None;
        }
        let Some(state) = self.states.remove(&pid) else {
            return Task::none();
        };
        for tab in &state.tabs {
            tab.forwarder.abort();
        }
        // `state.pending` 里的任务随 `state` 在本函数结束时 drop 而 detach
        // (不 abort):它们完成时由 `emitter` 的代次检查决定丢弃还是 kill。
        let Some(handle) = self.handle() else {
            return Task::none();
        };
        for id in state
            .tabs
            .iter()
            .filter(|tab| tab.alive && matches!(tab.backend, TabBackend::Daemon))
            .map(|tab| tab.info.id.clone())
        {
            let svc = Arc::clone(&self.svc);
            let task = handle.spawn(async move {
                if let Err(error) = svc.kill(id).await {
                    eprintln!("digger: 关闭项目时结束会话失败: {error}"); // cli-output
                }
            });
            self.track(task);
        }
        Task::none()
    }

    /// 关窗口:只 detach,不 kill(会话随 `diggerd` 存活)。
    pub fn prepare_shutdown(&mut self) {
        for state in self.states.values() {
            for tab in &state.tabs {
                tab.forwarder.abort();
            }
            for task in state.pending.values() {
                task.abort();
            }
        }
    }

    /// 并行等退出前的 kill 任务,整体有上限;超时放弃,不卡窗口。
    pub fn drain_critical(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        self.drain_critical_with(EXIT_KILL_BUDGET)
    }

    pub fn drain_critical_with(
        &self,
        budget: Duration,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        let tasks = self
            .critical
            .lock()
            .map(|mut tasks| std::mem::take(&mut *tasks))
            .unwrap_or_default();
        async move {
            let all = async move {
                for task in tasks {
                    let _ = task.await;
                }
            };
            let _ = tokio::time::timeout(budget, all).await;
        }
    }

    /// 窗口/分隔线尺寸变化:网格变了才发一次 `Resize`;0 尺寸不发。
    pub fn sync_grid(
        &mut self,
        project: &ProjectInfo,
        grid: (u16, u16),
    ) -> Task<crate::product::Msg> {
        if grid.0 == 0 || grid.1 == 0 {
            return Task::none();
        }
        if self.last_grid.get(&project.id) == Some(&grid) {
            return Task::none();
        }
        self.last_grid.insert(project.id, grid);
        self.update(
            project.id,
            Message::Resize {
                cols: grid.0,
                rows: grid.1,
            },
            Some(project),
            grid,
        )
    }

    /// 配对视图是否该出现:守护进程可用且当前项目已有状态。
    pub fn pair_ready(&self, pid: Option<i64>) -> bool {
        self.shell.status == Status::Ready && pid.is_some_and(|id| self.states.contains_key(&id))
    }
}

thread_local! {
    /// 当前正在渲染的 Agent 项目 id:终端的 canvas 只接受不带捕获的
    /// `fn(TermEvent) -> Msg`,项目路由取这份每帧覆盖的快照(同
    /// `CURRENT_WINDOW` 的手法:只读、每帧覆盖、不跨帧累积)。
    static CURRENT_AGENT_PROJECT: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// 由 `agent_view` 每帧写入当前 Agent 项目 id。
pub fn set_current_agent_project(pid: i64) {
    CURRENT_AGENT_PROJECT.with(|cell| cell.set(pid));
}

/// 终端 canvas 的静态事件映射(项目 id 取本帧写入的快照)。
pub fn term_event_message(event: dozer_panel_term::view::TermEvent) -> crate::product::Msg {
    let pid = CURRENT_AGENT_PROJECT.with(std::cell::Cell::get);
    crate::product::Msg::Agent(pid, dozer_panel_agent::term_message(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::{AgentKind, AgentState, SessionEvent, SessionInfo};
    use dozer_panel_agent::adapter::HookInstaller;
    use dozer_panel_agent::tabs::SessionTab;
    use dozer_panel_term::model::TerminalModel;
    use dozer_panel_term::osc::OscScanner;
    use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind, SessionAttach, SessionSpec};
    use std::time::Instant;

    struct NoHooks;
    impl HookInstaller for NoHooks {
        fn ensure_hook(&self, _agent: AgentKind) {}
        fn ensure_mcp(&self, _agent: AgentKind) {}
        fn hook_exe(&self) -> String {
            "/tmp/digger-hook".into()
        }
    }

    #[derive(Default)]
    struct FakeState {
        sessions: Vec<SessionInfo>,
        creates: Vec<SessionSpec>,
        kills: Vec<String>,
        resizes: Vec<(String, u16, u16)>,
        writes: Vec<(String, Vec<u8>)>,
        list_calls: usize,
        create_error: Option<String>,
        create_delay: Duration,
        attach_tx: HashMap<String, tokio::sync::mpsc::UnboundedSender<SessionEvent>>,
        attach_snapshot: Vec<u8>,
    }

    #[derive(Clone, Default)]
    struct FakeSessions(Arc<Mutex<FakeState>>);

    impl FakeSessions {
        fn get(&self) -> std::sync::MutexGuard<'_, FakeState> {
            self.0.lock().unwrap()
        }
        fn push_session(&self, info: SessionInfo) {
            self.get().sessions.push(info);
        }
        fn fail_create(&self, message: &str) {
            self.get().create_error = Some(message.into());
        }
        fn slow_create(&self, delay: Duration) {
            self.get().create_delay = delay;
        }
    }

    impl SessionService for FakeSessions {
        fn list_sessions(&self) -> BoxFuture<'_, Result<Vec<SessionInfo>, ServiceError>> {
            let mut state = self.get();
            state.list_calls += 1;
            let sessions = state.sessions.clone();
            Box::pin(async move { Ok(sessions) })
        }
        fn create(&self, spec: SessionSpec) -> BoxFuture<'_, Result<SessionInfo, ServiceError>> {
            let mut state = self.get();
            state.creates.push(spec.clone());
            let error = state.create_error.clone();
            let delay = state.create_delay;
            let info = SessionInfo {
                id: format!("created-{}", state.creates.len()),
                name: spec.name,
                command: spec.command,
                cwd: spec.cwd,
                alive: true,
                created_ms: 0,
                agent_state: AgentState::Idle,
                transcript_path: None,
                project_id: Some(spec.project_id),
                agent: spec.agent,
            };
            let id = info.id.clone();
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            state.attach_tx.insert(id, tx);
            drop(state);
            Box::pin(async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                match error {
                    Some(message) => Err(ServiceError::new(ServiceErrorKind::Rejected, message)),
                    None => Ok(info),
                }
            })
        }
        fn attach(
            &self,
            id: String,
            _from: u64,
        ) -> BoxFuture<'_, Result<SessionAttach, ServiceError>> {
            let mut state = self.get();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            state.attach_tx.insert(id, tx);
            let snapshot = state.attach_snapshot.clone();
            Box::pin(async move { Ok((snapshot, 0, rx)) })
        }
        fn write(&self, id: String, data: Vec<u8>) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.get().writes.push((id, data));
            Box::pin(async { Ok(()) })
        }
        fn resize(
            &self,
            id: String,
            cols: u16,
            rows: u16,
        ) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.get().resizes.push((id, cols, rows));
            Box::pin(async { Ok(()) })
        }
        fn kill(&self, id: String) -> BoxFuture<'_, Result<(), ServiceError>> {
            self.get().kills.push(id);
            Box::pin(async { Ok(()) })
        }
        fn close_with_summary(&self, _id: String) -> BoxFuture<'_, Result<(), ServiceError>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn session(id: &str, pid: i64, alive: bool) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: id.into(),
            command: "sh".into(),
            cwd: "/tmp".into(),
            alive,
            created_ms: 0,
            agent_state: AgentState::Idle,
            transcript_path: None,
            project_id: Some(pid),
            agent: AgentKind::Unknown,
        }
    }

    fn project(id: i64, path: &str) -> ProjectInfo {
        ProjectInfo {
            id,
            name: format!("p{id}"),
            path: path.into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    fn make_panel(fake: &FakeSessions) -> AgentPanel {
        let mut panel = AgentPanel::with_services(Arc::new(fake.clone()), Arc::new(NoHooks));
        panel.shell.status = Status::Ready;
        panel
    }

    /// 驱动一个 iced Task 到完成,收集它发出的产品消息(与运行时同一条路径)。
    async fn drain(task: Task<crate::product::Msg>) -> Vec<crate::product::Msg> {
        use iced::futures::StreamExt;
        let Some(mut stream) = iced_runtime::task::into_stream(task) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(Some(action)) =
            tokio::time::timeout(Duration::from_secs(2), stream.next()).await
        {
            if let iced_runtime::Action::Output(message) = action {
                out.push(message);
            }
        }
        out
    }

    fn restored_of(messages: Vec<crate::product::Msg>) -> Option<(i64, Vec<RestoredSession>)> {
        let message = messages
            .into_iter()
            .find(|m| matches!(m, crate::product::Msg::AgentRestored(..)))?;
        match &message {
            crate::product::Msg::AgentRestored(pid, payload) => {
                let sessions = payload.lock().unwrap().take().unwrap_or_default();
                Some((*pid, sessions))
            }
            _ => None,
        }
    }

    fn insert_tab(state: &mut AgentPanelState, id: &str, agent: AgentKind) {
        let runtime = tokio::runtime::Handle::current();
        let tab_id = state.next_tab_id;
        state.next_tab_id += 1;
        state.tabs.push(SessionTab {
            info: SessionInfo {
                id: id.into(),
                name: id.into(),
                command: "sh".into(),
                cwd: "/tmp".into(),
                alive: true,
                created_ms: 0,
                agent_state: AgentState::Idle,
                transcript_path: None,
                project_id: Some(7),
                agent,
            },
            model: TerminalModel::new(20, 5),
            alive: true,
            agent_state: AgentState::Idle,
            agent,
            transcript_path: None,
            osc: OscScanner::new(),
            cwd: None,
            last_exit: None,
            llm_model: None,
            permission_mode: None,
            last_activity: None,
            workspace_override: None,
            tab_id,
            forwarder: runtime.spawn(async {}),
            backend: TabBackend::Daemon,
        });
    }

    fn flush(panel: &mut AgentPanel, pid: i64, project: &ProjectInfo) {
        let mut received = Vec::new();
        if let Some(rx) = panel.rx.lock().unwrap().as_mut() {
            while let Ok(message) = rx.try_recv() {
                received.push(message);
            }
        }
        for (message_pid, message) in received {
            if message_pid == pid {
                let _ = panel.update(pid, message, Some(project), (80, 24));
            }
        }
    }

    async fn settle() {
        // hook 安装走 `spawn_blocking`;给多线程池与转发任务足够时间落地。
        for _ in 0..50 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    #[tokio::test]
    async fn close_confirm_appears_only_for_running_or_awaiting_input_tabs_and_confirming_kills_that_session()
     {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        let mut state = AgentPanelState::new();
        insert_tab(&mut state, "running", AgentKind::Claude);
        insert_tab(&mut state, "idle", AgentKind::Claude);
        state.tabs[0].agent_state = AgentState::Running;
        state.tabs[1].agent_state = AgentState::Idle;
        panel.states.insert(7, state);

        // 运行中:只挂起确认,不 kill。
        let _ = panel.update(7, Message::CloseTab(0), Some(&project), (80, 24));
        assert_eq!(
            panel.states[&7].pending_close_tab.as_deref(),
            Some("running")
        );
        assert!(fake.get().kills.is_empty());
        assert!(
            dozer_panel_agent::picker::close_confirm_spec(&panel.states[&7], "en", |m| m).is_some()
        );
        // 取消:什么都不变。
        let _ = panel.update(7, Message::CloseCancel, Some(&project), (80, 24));
        assert!(panel.states[&7].pending_close_tab.is_none());
        assert_eq!(panel.states[&7].tabs.len(), 2);

        // 空闲页签:直接关闭并 kill。
        let _ = panel.update(7, Message::CloseTab(1), Some(&project), (80, 24));
        settle().await;
        assert!(panel.states[&7].pending_close_tab.is_none());
        assert_eq!(panel.states[&7].tabs.len(), 1);
        assert_eq!(fake.get().kills, ["idle"]);
    }

    #[tokio::test]
    async fn selecting_a_picker_entry_creates_a_session_in_the_project_directory() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        panel.states.insert(7, AgentPanelState::new());
        let _ = panel.update(
            7,
            Message::PickerSelect(PickerLaunch::Agent(Some(AgentKind::Claude))),
            Some(&project),
            (90, 30),
        );
        settle().await;
        flush(&mut panel, 7, &project);
        let state = fake.get();
        assert_eq!(state.creates.len(), 1);
        assert_eq!(
            state.creates[0].command,
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()),
            "create 起的是用户 shell,agent CLI 由后续键入启动"
        );
        assert_eq!(state.creates[0].cwd, "/tmp/p7");
        assert_eq!(state.creates[0].agent, AgentKind::Claude);
        assert_eq!((state.creates[0].cols, state.creates[0].rows), (90, 30));
        assert_eq!(
            state.writes[0].1, b"claude\n",
            "attach 之后把选择器的启动命令写进 PTY"
        );
    }

    #[tokio::test]
    async fn unavailable_daemon_starts_nothing_and_keeps_the_panel_usable_after_retry() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        panel.shell.status = Status::Unavailable("diggerd 没起来".into());
        let project = project(7, "/tmp/p7");
        assert!(
            iced_runtime::task::into_stream(panel.ensure_started(&project, (80, 24))).is_none()
        );
        assert_eq!(fake.get().list_calls, 0);
        assert!(panel.states.is_empty());
        // 重试成功后同一项目可以正常启动。
        panel.shell.status = Status::Ready;
        assert!(
            iced_runtime::task::into_stream(panel.ensure_started(&project, (80, 24))).is_some()
        );
        assert!(panel.starting.contains(&7));
    }

    #[tokio::test]
    async fn switching_away_and_back_during_restore_does_not_double_start() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let task = panel.ensure_started(&project, (80, 24));
        // 恢复在途时切走(面板不可见)又切回(再次可见):入口仍是 ensure_started。
        assert!(
            iced_runtime::task::into_stream(panel.ensure_started(&project, (80, 24))).is_none(),
            "回到同一项目时不得再启动一次"
        );
        let restored = drain(task).await;
        assert_eq!(fake.get().list_calls, 1, "list_sessions 只调一次");
        let (pid, sessions) = restored_of(restored).expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        assert_eq!(panel.states[&7].tabs.len(), 1);
        assert!(fake.get().creates.is_empty(), "没有第二个 shell");
    }

    #[tokio::test]
    async fn a_new_project_starts_with_the_current_grid_so_its_first_session_is_created_at_the_right_size()
     {
        let fake = FakeSessions::default();
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        // 恢复开始时窗口是 100×40;恢复结果到达时窗口已经变成 80×24。
        let task = panel.ensure_started(&project, (100, 40));
        let (pid, sessions) = restored_of(drain(task).await).expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        settle().await;
        flush(&mut panel, pid, &project);
        let state = fake.get();
        assert_eq!(state.creates.len(), 1);
        assert_eq!(
            (state.creates[0].cols, state.creates[0].rows),
            (100, 40),
            "create 用的是恢复开始时的网格(start_grid),不是完成时的 80×24"
        );
    }

    #[tokio::test]
    async fn a_hundred_resizes_to_the_same_grid_send_once() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        let mut state = AgentPanelState::new();
        insert_tab(&mut state, "s1", AgentKind::Unknown);
        panel.states.insert(7, state);
        let _ = panel.sync_grid(&project, (100, 40));
        settle().await;
        assert_eq!(fake.get().resizes.len(), 1, "首次网格要发");
        for _ in 0..100 {
            let _ = panel.sync_grid(&project, (100, 40));
        }
        settle().await;
        assert_eq!(fake.get().resizes.len(), 1, "之后 100 次同网格只发过那一次");
    }

    #[tokio::test]
    async fn ensure_started_twice_starts_once() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let task = panel.ensure_started(&project, (80, 24));
        assert!(
            iced_runtime::task::into_stream(panel.ensure_started(&project, (80, 24))).is_none(),
            "the second call while restore is in flight starts nothing"
        );
        let restored = drain(task).await;
        assert_eq!(fake.get().list_calls, 1);
        assert_eq!(restored.len(), 1);
    }

    #[tokio::test]
    async fn first_visibility_restores_live_sessions_and_does_not_open_a_shell() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        fake.push_session(session("s2", 7, true));
        fake.push_session(session("other", 8, true));
        fake.get().attach_snapshot = b"hello".to_vec();
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (80, 24))).await)
            .expect("restore result");
        assert_eq!(sessions.len(), 2, "only this project's live sessions");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        assert_eq!(panel.states[&7].tabs.len(), 2);
        assert!(fake.get().creates.is_empty(), "no new shell when restored");
    }

    #[tokio::test]
    async fn first_visibility_without_sessions_opens_one_shell_in_the_project_directory() {
        let fake = FakeSessions::default();
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (100, 40))).await)
            .expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (100, 40));
        settle().await;
        flush(&mut panel, pid, &project);
        let state = fake.get();
        assert_eq!(state.creates.len(), 1);
        assert_eq!(state.creates[0].cwd, "/tmp/p7");
        assert_eq!(state.creates[0].project_id, 7);
        assert_eq!((state.creates[0].cols, state.creates[0].rows), (100, 40));
    }

    #[tokio::test]
    async fn restore_result_for_a_closed_project_is_discarded_and_detached() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (80, 24))).await)
            .expect("restore result");
        let _ = panel.on_restored(pid, None, sessions, (80, 24));
        assert!(panel.states.is_empty());
        assert!(!panel.starting.contains(&pid));
    }

    #[tokio::test]
    async fn messages_for_one_project_never_touch_another() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let a = project(7, "/tmp/a");
        let mut sa = AgentPanelState::new();
        insert_tab(&mut sa, "a1", AgentKind::Claude);
        let mut sb = AgentPanelState::new();
        insert_tab(&mut sb, "b1", AgentKind::Claude);
        panel.states.insert(7, sa);
        panel.states.insert(8, sb);
        let b_before = panel.states[&8].tabs[0].model.visible_lines().clone();
        let _ = panel.update(
            7,
            Message::Output {
                tab_id: 0,
                bytes: b"only-a".to_vec(),
            },
            Some(&a),
            (80, 24),
        );
        let _ = panel.update(
            7,
            Message::AgentState {
                tab_id: 0,
                agent: AgentKind::Claude,
                state: AgentState::Running,
                transcript_path: None,
            },
            Some(&a),
            (80, 24),
        );
        let _ = panel.update(7, Message::Exited { tab_id: 0 }, Some(&a), (80, 24));
        assert_eq!(panel.states[&8].tabs[0].model.visible_lines(), b_before);
        assert_eq!(panel.states[&8].tabs[0].agent_state, AgentState::Idle);
        assert!(panel.states[&8].tabs[0].alive);
    }

    #[tokio::test]
    async fn a_message_for_a_project_without_state_is_ignored() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let a = project(7, "/tmp/a");
        let _ = panel.update(
            7,
            Message::Output {
                tab_id: 0,
                bytes: b"x".to_vec(),
            },
            Some(&a),
            (80, 24),
        );
        assert!(panel.states.is_empty());
    }

    #[tokio::test]
    async fn late_messages_after_close_project_are_ignored() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let a = project(7, "/tmp/a");
        let mut sa = AgentPanelState::new();
        insert_tab(&mut sa, "a1", AgentKind::Claude);
        panel.states.insert(7, sa);
        let _ = panel.close_project(7);
        assert!(!panel.states.contains_key(&7));
        let _ = panel.update(7, Message::Exited { tab_id: 0 }, Some(&a), (80, 24));
        assert!(!panel.states.contains_key(&7));
    }

    #[tokio::test]
    async fn restore_snapshot_and_following_events_are_contiguous() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        fake.get().attach_snapshot = b"AB".to_vec();
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (80, 24))).await)
            .expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        let sender = fake.get().attach_tx[&String::from("s1")].clone();
        sender
            .send(SessionEvent::Output(b"C".to_vec()))
            .expect("forwarder still alive");
        settle().await;
        flush(&mut panel, pid, &project);
        let lines = panel.states[&7].tabs[0].model.visible_lines();
        let row: String = lines[0].iter().map(|c| c.ch).collect();
        assert!(row.starts_with("ABC"), "snapshot then events: {row:?}");
    }

    #[tokio::test]
    async fn disconnected_event_marks_the_tab_exited_and_does_not_reconnect() {
        let fake = FakeSessions::default();
        fake.push_session(session("s1", 7, true));
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (80, 24))).await)
            .expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        let sender = fake.get().attach_tx[&String::from("s1")].clone();
        sender.send(SessionEvent::Disconnected).unwrap();
        settle().await;
        flush(&mut panel, pid, &project);
        assert!(!panel.states[&7].tabs[0].alive);
        assert!(fake.get().creates.is_empty(), "no reconnect / new shell");
    }

    #[tokio::test]
    async fn closing_a_project_while_create_is_in_flight_kills_the_session_when_it_finishes() {
        // 审核 C:create 已发出、应答还没回来时关项目页签。不能 abort 任务
        // (否则会话已在守护进程里建好却没人 kill);代次作废旧回投,等
        // `Attached` 到达时补一次 kill。
        let fake = FakeSessions::default();
        fake.slow_create(Duration::from_millis(500));
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        panel.states.insert(7, AgentPanelState::new());
        let _ = panel.update(
            7,
            Message::PickerSelect(PickerLaunch::Agent(None)),
            Some(&project),
            (80, 24),
        );
        // 等后台任务走过 hook/ensure 并发出 create。
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(fake.get().creates.len(), 1, "create 请求已经发出");
        let _ = panel.close_project(7);
        // create 500ms 后返回;随后 attach 成功,迟到的 `Attached` 触发 kill。
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(
            fake.get().kills,
            ["created-1"],
            "在途 create 完成后必须被回收,不能留下孤儿会话"
        );
        assert!(!panel.states.contains_key(&7));
    }

    #[tokio::test]
    async fn close_project_kills_every_session_of_that_project_only() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let mut sa = AgentPanelState::new();
        insert_tab(&mut sa, "a1", AgentKind::Claude);
        insert_tab(&mut sa, "a2", AgentKind::Claude);
        let mut sb = AgentPanelState::new();
        insert_tab(&mut sb, "b1", AgentKind::Claude);
        panel.states.insert(7, sa);
        panel.states.insert(8, sb);
        let _ = panel.close_project(7);
        settle().await;
        let mut kills = fake.get().kills.clone();
        kills.sort();
        assert_eq!(kills, ["a1", "a2"]);
        assert!(panel.states.contains_key(&8));
    }

    #[tokio::test]
    async fn prepare_shutdown_detaches_without_killing() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let mut sa = AgentPanelState::new();
        insert_tab(&mut sa, "a1", AgentKind::Claude);
        panel.states.insert(7, sa);
        panel.prepare_shutdown();
        settle().await;
        assert!(fake.get().kills.is_empty());
        assert!(panel.states.contains_key(&7));
    }

    #[tokio::test]
    async fn drain_critical_waits_for_inflight_kills_but_gives_up_after_the_limit() {
        let fake = FakeSessions::default();
        let panel = make_panel(&fake);
        let handle = tokio::runtime::Handle::current();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&done);
        panel.track(handle.spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        let start = Instant::now();
        panel.drain_critical_with(Duration::from_millis(200)).await;
        assert!(done.load(std::sync::atomic::Ordering::SeqCst));
        assert!(start.elapsed() < Duration::from_secs(1));

        // 永不结束的任务:整体在上限内返回。
        let panel = make_panel(&fake);
        panel.track(handle.spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }));
        let start = Instant::now();
        panel.drain_critical_with(Duration::from_millis(50)).await;
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "a hanging kill must not block exit"
        );
    }

    #[tokio::test]
    async fn service_failures_surface_as_a_notice_and_can_be_dismissed() {
        let fake = FakeSessions::default();
        fake.fail_create("diggerd 拒绝创建");
        let project = project(7, "/tmp/p7");
        let mut panel = make_panel(&fake);
        let (pid, sessions) = restored_of(drain(panel.ensure_started(&project, (80, 24))).await)
            .expect("restore result");
        let _ = panel.on_restored(pid, Some(&project), sessions, (80, 24));
        settle().await;
        flush(&mut panel, pid, &project);
        assert!(
            panel.notice.as_deref().unwrap_or("").contains("拒绝创建"),
            "{:?}",
            panel.notice
        );
        panel.dismiss_notice();
        assert!(panel.notice.is_none());
    }

    #[tokio::test]
    async fn repeated_sync_with_the_same_grid_sends_nothing() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        let mut state = AgentPanelState::new();
        insert_tab(&mut state, "s1", AgentKind::Unknown);
        panel.states.insert(7, state);
        for _ in 0..100 {
            let _ = panel.sync_grid(&project, (100, 40));
        }
        settle().await;
        assert_eq!(fake.get().resizes.len(), 1, "只发一次相同网格");
        assert_eq!(fake.get().resizes[0].1, 100);
        let _ = panel.sync_grid(&project, (120, 50));
        settle().await;
        assert_eq!(fake.get().resizes.len(), 2);
    }

    #[tokio::test]
    async fn zero_grid_is_never_sent() {
        let fake = FakeSessions::default();
        let mut panel = make_panel(&fake);
        let project = project(7, "/tmp/p7");
        let mut state = AgentPanelState::new();
        insert_tab(&mut state, "s1", AgentKind::Unknown);
        panel.states.insert(7, state);
        let _ = panel.sync_grid(&project, (0, 40));
        let _ = panel.sync_grid(&project, (100, 0));
        settle().await;
        assert!(fake.get().resizes.is_empty());
    }

    #[test]
    fn unavailable_shows_the_reason_and_a_retry_that_checks_once() {
        let mut shell = AgentShell {
            status: Status::Unavailable("daemon 没起来".into()),
        };
        assert_eq!(shell.body(true), Body::Unavailable("daemon 没起来"));
        let calls = std::cell::Cell::new(0);
        let retry = |shell: &mut AgentShell| {
            let _ = shell.update(AgentShellMsg::Retry, || {
                calls.set(calls.get() + 1);
                Task::none()
            });
        };
        retry(&mut shell);
        retry(&mut shell);
        assert_eq!(calls.get(), 1, "the second Retry while Checking is ignored");
        assert_eq!(shell.body(true), Body::Checking);
    }

    #[test]
    fn a_successful_retry_turns_unavailable_into_ready() {
        let mut shell = AgentShell {
            status: Status::Unavailable("x".into()),
        };
        let _ = shell.update(AgentShellMsg::Retry, Task::none);
        let _ = shell.update(AgentShellMsg::Ensured(Ok(())), Task::none);
        assert_eq!(shell.body(true), Body::Placeholder);
        let _ = shell.update(AgentShellMsg::Ensured(Err("又失败了".into())), Task::none);
        assert_eq!(shell.body(true), Body::Unavailable("又失败了"));
    }

    #[test]
    fn no_project_shows_the_empty_state_whatever_the_status() {
        for status in [
            Status::Unknown,
            Status::Checking,
            Status::Ready,
            Status::Unavailable("x".into()),
        ] {
            assert_eq!(AgentShell { status }.body(false), Body::NoProject);
        }
    }

    #[test]
    fn the_default_shell_is_unknown_and_shows_checking() {
        // S6a:默认状态改为 Unknown(启动序列开始时置 Checking)。
        assert_eq!(AgentShell::default().body(true), Body::Checking);
    }
}
