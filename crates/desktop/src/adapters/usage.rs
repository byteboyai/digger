//! Usage 面板的 **Digger 宿主适配层**。
//!
//! 面板本体(数据层 / 视图层 / web 资源)是共享库 `dozer-panel-usage`,Dozer 与
//! Digger 共用同一实现(计划 Task 19 第 1 条)。本文件是 Digger 侧唯一的产品适配:
//!
//! - **展示语义**:[`PanelDisplay<Msg>`] 实现(agent 图标/颜色、面板头、列表折叠
//!   按钮、折叠/悬停事件映射)。Digger 尚无品牌资产,agent 一律回落公共 `Bot`
//!   图标 + 主题色——与库的 `NoGitStats` 同样的"显式最小"口径,不伪造。
//! - **数据**:[`dozer_services::UsageService`] 由组合根注入 `dozer_client::Client`
//!   实现,经守护进程 `GetUsageSummary` 读**当前项目**的会话用量(transcripts 存储),
//!   与 Dozer 同口径;不再是本机应用操作账本。
//! - **Git 统计**:Digger 一期无 Git 统计,注入 [`NoGitStats`](库内)恒 0 / 空。
//!
//! 数据接缝遵循"业务实现,宿主适配层注入"——[`UsagePanel::new`] 收一个
//! `Arc<dyn UsageService>`,由组合根传入真实实现。

use std::path::PathBuf;
use std::sync::Arc;

use bytehost_chrome::ShellMsg;
use dozer_model::AgentKind;
use dozer_panel_usage::adapter::{NoGitStats, PanelDisplay, PanelHeadKind};
use dozer_panel_usage::{Message as UsageMessage, WorkspaceState, activation, on_activate};
use dozer_services::UsageService;
use iced::{Element, Length};
use iced_widget::core::{Border, Color};

use crate::product::Msg;

/// 一条后台用量结果及其作用域身份。
///
/// `epoch` 是 [`UsagePanel`] 在每次作用域生命周期变化时推进的独立标识;它不随
/// 共享 `WorkspaceState` 的 `refresh_generation` 重置而复用。后台结果必须先通过
/// [`UsagePanel::apply_result`] 的身份检查,才能进入共享 `update`。
#[derive(Debug, Clone)]
pub struct UsageResult {
    pub scope: PathBuf,
    pub epoch: u64,
    pub message: UsageMessage,
}

/// Digger 的 Usage 面板状态:库的 [`WorkspaceState`] + 注入的数据接缝。
pub struct UsagePanel {
    /// 库的数据/视图状态。
    state: WorkspaceState,
    pub native: super::usage_native::Native,
    /// 注入的用量服务(真实实现:`dozer_client::Client`,走守护进程 transcripts 存储)。
    service: Arc<dyn UsageService>,
    /// 本面板查询的**作用域路径**(当前项目路径)。`None` = 尚未打开 Project,
    /// 面板保持空态,不发起查询。
    scope: Option<PathBuf>,
    /// 跨生命周期的作用域标识。每次作用域实际变化(含关闭全部项目)都推进一次,
    /// 与共享 `WorkspaceState::refresh_generation` 各管一层:epoch 隔离生命周期,
    /// generation 只排序同一生命周期内的刷新。
    scope_epoch: u64,
    /// epoch 用尽后拒绝新查询,避免 checked 递增回绕到旧身份。
    epoch_exhausted: bool,
    /// iced 运行时句柄(库的 `spawn_refresh` 要它去后台取数)。`None` = 尚未在
    /// 运行时内挂上(`ProductShell::new` 可在无运行时处被测试调用);此时 `activate`
    /// 只置"统计中",不起刷新——与库对"无 ctx"的处理同型。
    handle: Option<tokio::runtime::Handle>,
    /// 后台取数结果的回流发送端:库的 `emit` 把带 scope / epoch 的
    /// [`UsageResult`] 投到这里,组合根在 `boot` 里把它接成 iced 流
    /// (与 [`crate::apps`] 的结果回流同型)。
    tx: tokio::sync::mpsc::UnboundedSender<UsageResult>,
    /// 回流接收端;供 [`UsagePanel::take_receiver`] 取走接成 iced 流。
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<UsageResult>>,
}

impl UsagePanel {
    /// 构造:注入真实用量服务;作用域由组合根在打开/切换项目时经
    /// [`UsagePanel::set_scope`] 更新。运行时句柄用 [`UsagePanel::attach_handle`]
    /// 在进入 iced 运行时后补挂(组合根 `boot` 内)。
    pub fn new(service: Arc<dyn UsageService>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state: WorkspaceState::default(),
            native: super::usage_native::Native::new(),
            service,
            scope: None,
            scope_epoch: 0,
            epoch_exhausted: false,
            handle: None,
            tx,
            rx: Some(rx),
        }
    }

    /// 更新查询作用域(打开/切换/关闭项目时由组合根调用)。
    ///
    /// 作用域实际变化时:**先**推进 epoch 使旧回包失效,再清空共享状态、loading、
    /// 筛选、通知与原生展示。相同作用域重复同步是幂等的,不清空当前数据。
    pub fn set_scope(&mut self, scope: Option<PathBuf>) {
        if self.scope == scope {
            return;
        }
        self.invalidate_scope();
        if self.epoch_exhausted {
            return;
        }
        self.scope = scope;
        self.state = WorkspaceState::default();
        self.native.clear_payload();
    }

    /// 清空面板数据(作用域没了——关闭全部项目):旧项目的行不该残留。
    ///
    /// 即使作用域已经是 `None`,也推进 epoch,确保关闭前发出的查询全部失效。
    pub fn reset(&mut self) {
        self.scope = None;
        self.invalidate_scope();
        self.state = WorkspaceState::default();
        self.native.clear_payload();
    }

    /// 使当前生命周期的所有后台回包失效。checked 递增;耗尽时进入不可恢复的
    /// 拒绝状态,绝不回绕复用旧 epoch。
    fn invalidate_scope(&mut self) {
        if self.epoch_exhausted {
            return;
        }
        match self.scope_epoch.checked_add(1) {
            Some(epoch) => self.scope_epoch = epoch,
            None => {
                self.epoch_exhausted = true;
                self.scope = None;
                self.state = WorkspaceState::default();
                self.native.clear_payload();
                eprintln!(
                    "digger: usage 面板/error: 作用域 epoch 已耗尽，已拒绝新查询；请重启应用" // cli-output
                );
            }
        }
    }

    /// 挂上 iced 运行时句柄(仅可在运行时上下文内调用)。
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 取走后台结果接收端(组合根在 `boot` 里接成 iced 流;只该取一次)。
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<UsageResult>> {
        self.rx.take()
    }

    /// 面板切入:构造库的 [`activation::ActivationCtx`] 并调 `on_activate`。
    ///
    /// 与 Dozer 侧逐字段一致:注入 Git 统计(这里 [`NoGitStats`])、注入用量服务、
    /// 用运行时句柄与发射器。`emit` 把库消息投回组合根的回流通道(而不是丢弃)。
    pub fn activate(&mut self) {
        if self.epoch_exhausted {
            return;
        }
        // 尚未打开 Project:保持空态,不向守护进程发查询。
        let Some(scope) = self.scope.clone() else {
            return;
        };
        // 还没挂运行时句柄(如测试环境):只置"统计中",不起刷新。
        let Some(handle) = self.handle.clone() else {
            self.state.set_loading(true);
            return;
        };
        let epoch = self.scope_epoch;
        let tx = self.tx.clone();
        let ctx = activation::ActivationCtx::new(
            // 库的 `project_id` 仅用于日志/路由归属,Digger 一期用固定实例 id。
            1,
            Some(scope.clone()),
            Arc::clone(&self.service),
            Arc::new(NoGitStats),
            handle,
            move |message| {
                // 通道关闭 = 组合根已退出;静默丢弃(后台任务随后自然结束)。
                let _ = tx.send(UsageResult {
                    scope: scope.clone(),
                    epoch,
                    message,
                });
            },
        );
        on_activate(&mut self.state, Some(&ctx));
    }

    /// 处理一条**本地交互**库消息(筛选 / 折叠悬停等)。
    ///
    /// 后台 `Loaded` / `LoadFailed` 必须走 [`UsagePanel::apply_result`];这里拒绝,
    /// 保留一条裸回包即可绕过身份检查的旁路是不允许的。
    pub fn update(&mut self, message: UsageMessage) {
        if matches!(
            message,
            UsageMessage::Loaded(..) | UsageMessage::LoadFailed(..)
        ) {
            eprintln!(
                "digger: usage 面板/error: 后台回包走了交互入口，已拒绝" // cli-output
            );
            return;
        }
        dozer_panel_usage::update(&mut self.state, message);
    }

    /// 后台结果入口:先校验 scope 与 epoch,再交给共享 `update`。
    /// 返回 `false` 表示旧生命周期的回包被丢弃,不应触碰 rows / loading / 通知。
    pub fn apply_result(&mut self, result: UsageResult) -> bool {
        if self.epoch_exhausted
            || self.scope.as_ref() != Some(&result.scope)
            || self.scope_epoch != result.epoch
        {
            return false;
        }
        dozer_panel_usage::update(&mut self.state, result.message);
        true
    }

    /// 测试 / 生命周期自检用:在当前作用域直接放入一行样例数据。
    /// 这是一条显式的非产品消息入口,不参与后台路由。
    pub(crate) fn seed_sample(&mut self) {
        dozer_panel_usage::update(&mut self.state, sample_loaded_message());
    }

    /// 测试用:当前作用域身份。
    #[cfg(test)]
    pub(crate) fn scope_identity(&self) -> Option<(PathBuf, u64)> {
        self.scope.clone().map(|scope| (scope, self.scope_epoch))
    }

    #[cfg(test)]
    pub(crate) fn row_titles(&self) -> Vec<String> {
        self.state
            .rows()
            .iter()
            .map(|(meta, _)| meta.title.clone())
            .collect()
    }

    /// 测试用:epoch 是否已耗尽。
    #[cfg(test)]
    pub(crate) fn epoch_exhausted(&self) -> bool {
        self.epoch_exhausted
    }

    /// 测试用:强制推进到指定 epoch,用于覆盖 checked 溢出分支。
    #[cfg(test)]
    pub(crate) fn force_epoch(&mut self, epoch: u64) {
        self.scope_epoch = epoch;
    }

    /// 排空待发提示(库的 outbox)。Digger 尚无 Toast 机制,一期由调用方落地(见
    /// [`UsagePanel::take_notices`] 的说明);返回文本供调用方处理。
    ///
    pub fn take_notices(&mut self) -> Vec<(String, String)> {
        self.state
            .take_outbox()
            .into_iter()
            .map(|n| {
                (
                    format!("{:?}", n.level),
                    dozer_panel_usage::i18n::resolve(&n.text, crate::settings::language::code()),
                )
            })
            .collect()
    }

    pub fn sync_native(
        &mut self,
        window: iced::window::Id,
        visible: bool,
        rect: bytehost_core::LogicalRect,
    ) -> iced::Task<super::usage_native::Event> {
        self.native.sync(window, visible, rect, &self.state)
    }

    /// 库的 agent 筛选栏只在有数据时才拼:加载中 / 还没数据时调用方只显示内容栏(同 Dozer)。
    pub fn has_agent_filter(&self) -> bool {
        self.state.has_agent_filter()
    }

    /// 内容栏:标题 + 加载态;图表由同源 UsageSurface 和嵌入资源驱动的原生 webview 盖在它下面。
    /// `collapsed` 只决定标题行末尾那颗按钮显示"展开"还是"收起"(列表是否收起由骨架的 `PaneStore` 记账)。
    pub fn content_view(&self, collapsed: bool) -> Element<'_, Msg> {
        if self.epoch_exhausted {
            return iced::widget::text("用量作用域标识已耗尽，已停止查询；请重启应用后再试").into();
        }
        if let Some(error) = &self.native.error {
            return iced::widget::text(format!("用量视图创建失败：{error}；切换页签可重试")).into();
        }
        // 区域外框已由骨架绘制，语言取已保存的产品偏好。
        dozer_panel_usage::view::content_pane(
            self,
            collapsed,
            &self.state,
            Length::Fill,
            Border::default(),
            crate::settings::language::code(),
        )
    }

    /// agent 筛选列表。
    pub fn list_view(&self) -> Element<'_, Msg> {
        dozer_panel_usage::view::list_pane(
            self,
            &self.state,
            Length::Fill,
            Border::default(),
            crate::settings::language::code(),
        )
    }
}

/// 一条带一行样例数据的 `Loaded`:让面板进入"有数据"态(真实窗口自检与单元测试用)。
/// 代数取 `u64::MAX`,保证不被库当成"被更新的刷新取代"而丢弃。
pub fn sample_loaded_message() -> UsageMessage {
    let meta = dozer_model::ConversationMeta {
        path: PathBuf::from("/selftest/session.jsonl"),
        title: "selftest".to_owned(),
        modified_ms: 1,
        agent: AgentKind::Claude,
    };
    let usage = dozer_panel_usage::aggregate::ConversationUsage {
        turns: 1,
        tokens_in: 100,
        tokens_out: 50,
        ..Default::default()
    };
    UsageMessage::Loaded(
        1,
        u64::MAX,
        vec![(meta, usage)],
        0,
        std::collections::BTreeMap::new(),
    )
}

/// 把用量面板的后台结果接收端接成一条持续到进程结束的 iced 流(与 [`crate::apps`] 同型)。
pub fn receiver_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<UsageResult>,
) -> impl iced::futures::Stream<Item = UsageResult> {
    iced::futures::stream::unfold(Some(rx), |maybe| async move {
        let mut rx = maybe?;
        let next = rx.recv().await?;
        Some((next, Some(rx)))
    })
}

/// Digger 侧的展示适配:agent 一律用公共 `Bot` 图标 + 主题色(无品牌资产,显式回落,
/// 不伪造)。面板头/折叠按钮用 byteui 原语直接拼(minimal)。
impl PanelDisplay<Msg> for UsagePanel {
    fn wrap(&self, message: dozer_panel_usage::Message) -> Msg {
        Msg::Usage(message)
    }

    fn agent_icon_color(&self, _agent: AgentKind) -> Color {
        // 无品牌配色:统一用主题 dim 色,避免借用甲方动作专属金。
        byteui::theme::color::current().dim
    }

    fn agent_icon(
        &self,
        _agent: AgentKind,
        color: Color,
    ) -> Element<'static, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        byteui::interaction::icons::view(
            byteui::interaction::icons::IconKind::Bot,
            byteui::theme::icon_size::row(),
            color,
        )
    }

    fn panel_head<'a>(
        &self,
        kind: PanelHeadKind,
        title: &'a str,
        _actions: Option<Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer>>,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        use iced::widget::{column, container, row, text};
        let tokens = byteui::theme::color::current();
        // 图标按稳定身份选(不解析标题——i18n 后标题会变),与 Dozer 侧一致。
        let icon = match kind {
            PanelHeadKind::AgentFilter => byteui::interaction::icons::IconKind::Bot,
            PanelHeadKind::Content => byteui::interaction::icons::IconKind::BarChart3,
        };
        let head = row![
            byteui::interaction::icons::view(icon, byteui::theme::icon_size::row(), tokens.gold),
            text(title)
                .size(byteui::theme::font::title())
                .color(tokens.cream),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        container(column![head]).width(Length::Fill).into()
    }

    fn list_collapse_button<'a>(
        &self,
        collapsed: bool,
        tooltip_collapse: impl Into<std::borrow::Cow<'a, str>>,
        tooltip_expand: impl Into<std::borrow::Cow<'a, str>>,
        on_select: Msg,
        _on_hover: impl Fn(bool) -> Msg + 'a,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        use byteui::interaction::icons::{self, IconKind};
        use iced::widget::{button, container, text, tooltip};
        // 列表在右侧(默认位置):收起 = 把右边的列表收进去。
        let (icon, tip) = if collapsed {
            (IconKind::PanelRightOpen, tooltip_expand.into())
        } else {
            (IconKind::PanelRightClose, tooltip_collapse.into())
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

    fn collapse_hover(&self, _hovered: bool) -> Msg {
        // 悬停只是库为按钮高亮预留的接缝;Digger 的按钮自带悬停样式,这里什么都不做。
        Msg::Shell(ShellMsg::Noop)
    }

    fn toggle_list_collapse(&self) -> Msg {
        // 收起状态只存在骨架的 `PaneStore` 里(落盘、可拖拽收起),不在用量自己的状态里重复一份。
        Msg::Shell(ShellMsg::TogglePane(crate::shell::usage_id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::ShellMsg;
    use dozer_model::{AgentKind, ConversationSummary, UsagePayload};
    use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind, UsageRows};
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};

    const TIMEOUT: Duration = Duration::from_secs(5);

    struct PendingCall {
        reply: oneshot::Sender<Result<UsageRows, ServiceError>>,
    }

    /// 每次 `usage_summary` 都挂起,直到测试显式回复;`started` 让测试确定
    /// 查询已经开始,而不是用 sleep 猜调度。
    struct ControlledUsage {
        calls: Arc<Mutex<Vec<Option<PendingCall>>>>,
        started: mpsc::UnboundedSender<String>,
    }

    impl UsageService for ControlledUsage {
        fn usage_summary(
            &self,
            cwd: String,
            _since_ts: Option<u64>,
        ) -> BoxFuture<'_, Result<UsageRows, ServiceError>> {
            let (reply, rx) = oneshot::channel();
            self.calls.lock().unwrap().push(Some(PendingCall { reply }));
            let _ = self.started.send(cwd);
            Box::pin(async move {
                rx.await.unwrap_or_else(|_| {
                    Err(ServiceError::new(
                        ServiceErrorKind::Unavailable,
                        "test reply dropped",
                    ))
                })
            })
        }
    }

    struct Harness {
        panel: UsagePanel,
        calls: Arc<Mutex<Vec<Option<PendingCall>>>>,
        started: mpsc::UnboundedReceiver<String>,
        results: tokio::sync::mpsc::UnboundedReceiver<UsageResult>,
    }

    impl Harness {
        fn new() -> Self {
            let (started_tx, started) = mpsc::unbounded_channel();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let service = ControlledUsage {
                calls: Arc::clone(&calls),
                started: started_tx,
            };
            let mut panel = UsagePanel::new(Arc::new(service));
            panel.attach_handle(tokio::runtime::Handle::current());
            let results = panel.take_receiver().expect("result receiver");
            Self {
                panel,
                calls,
                started,
                results,
            }
        }

        fn set_scope(&mut self, path: &str) {
            self.panel.set_scope(Some(PathBuf::from(path)));
        }

        async fn next_start(&mut self) -> String {
            tokio::time::timeout(TIMEOUT, self.started.recv())
                .await
                .expect("query did not start")
                .expect("started channel closed")
        }

        fn reply(&self, index: usize, result: Result<UsageRows, ServiceError>) {
            let call = self.calls.lock().unwrap()[index]
                .take()
                .expect("query reply already used");
            call.reply.send(result).expect("query receiver dropped");
        }

        async fn next_result(&mut self) -> UsageResult {
            tokio::time::timeout(TIMEOUT, self.results.recv())
                .await
                .expect("no background result")
                .expect("result channel closed")
        }
    }

    fn rows(title: &str) -> UsageRows {
        vec![(
            ConversationSummary {
                conversation_id: title.into(),
                agent: AgentKind::Claude,
                file_path: format!("/{title}.jsonl"),
                title: title.into(),
                first_ts: 0,
                last_ts: 0,
                turn_count: 1,
            },
            UsagePayload {
                turns: 1,
                tokens_in: 1,
                tokens_out: 1,
                ..Default::default()
            },
        )]
    }

    fn failure(message: &str) -> ServiceError {
        ServiceError::new(ServiceErrorKind::Unavailable, message)
    }

    fn titles(panel: &UsagePanel) -> Vec<String> {
        panel
            .state
            .rows()
            .iter()
            .map(|(meta, _)| meta.title.clone())
            .collect()
    }

    fn panel() -> UsagePanel {
        let svc: Arc<dyn UsageService> = Arc::new(digger_store::UnavailableUsageService::new(
            "test".to_string(),
        ));
        let mut panel = UsagePanel::new(svc);
        panel.set_scope(Some(PathBuf::from("/tmp/digger-usage-test")));
        panel
    }

    #[test]
    fn the_list_collapse_button_talks_to_the_shell_not_to_the_library() {
        let p = panel();
        assert!(
            matches!(p.toggle_list_collapse(), Msg::Shell(ShellMsg::TogglePane(id)) if id == crate::shell::usage_id())
        );
        assert!(matches!(p.collapse_hover(true), Msg::Shell(ShellMsg::Noop)));
        assert!(matches!(
            p.collapse_hover(false),
            Msg::Shell(ShellMsg::Noop)
        ));
    }

    #[test]
    fn the_agent_filter_exists_only_once_there_is_data() {
        let mut p = panel();
        assert!(!p.has_agent_filter(), "nothing loaded yet");
        p.seed_sample();
        assert!(p.has_agent_filter());
    }

    #[test]
    fn both_panes_build_in_both_collapse_states() {
        let mut p = panel();
        p.seed_sample();
        let _ = p.content_view(false);
        let _ = p.content_view(true);
        let _ = p.list_view();
    }

    #[test]
    fn bare_background_messages_cannot_bypass_the_identity_check() {
        let mut p = panel();
        p.update(sample_loaded_message());
        assert!(titles(&p).is_empty());
        assert!(!p.has_agent_filter());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_project_never_queries_and_close_all_drops_a_late_success() {
        let mut h = Harness::new();
        h.panel.activate();
        assert!(h.calls.lock().unwrap().is_empty());
        assert!(!h.panel.state.loading());

        h.set_scope("/a");
        h.panel.activate();
        assert_eq!(h.next_start().await, "/a");
        h.panel.set_scope(None);
        assert!(!h.panel.state.loading());

        h.reply(0, Ok(rows("a")));
        let late = h.next_result().await;
        assert!(!h.panel.apply_result(late));
        assert!(titles(&h.panel).is_empty());
        assert!(!h.panel.state.loading());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reopening_the_same_path_gets_a_new_epoch_and_rejects_the_old_success() {
        let mut h = Harness::new();
        h.set_scope("/a");
        let first_epoch = h.panel.scope_identity().unwrap().1;
        h.panel.activate();
        h.next_start().await;

        h.panel.set_scope(None);
        h.set_scope("/a");
        let second_epoch = h.panel.scope_identity().unwrap().1;
        assert_ne!(first_epoch, second_epoch, "reset must not reuse the epoch");

        h.reply(0, Ok(rows("old-a")));
        let late = h.next_result().await;
        assert!(!h.panel.apply_result(late));
        assert!(titles(&h.panel).is_empty());

        h.panel.activate();
        assert_eq!(h.next_start().await, "/a");
        h.reply(1, Ok(rows("new-a")));
        let fresh = h.next_result().await;
        assert!(h.panel.apply_result(fresh));
        assert_eq!(titles(&h.panel), vec!["new-a"]);
        assert!(!h.panel.state.loading());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_late_a_success_after_b_succeeds_does_not_replace_b() {
        let mut h = Harness::new();
        h.set_scope("/a");
        h.panel.activate();
        h.next_start().await;

        h.set_scope("/b");
        h.panel.activate();
        h.next_start().await;

        h.reply(1, Ok(rows("b")));
        let b = h.next_result().await;
        assert!(h.panel.apply_result(b));
        assert_eq!(titles(&h.panel), vec!["b"]);

        h.reply(0, Ok(rows("a")));
        let late_a = h.next_result().await;
        assert!(!h.panel.apply_result(late_a));
        assert_eq!(titles(&h.panel), vec!["b"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_late_a_failure_does_not_touch_b_loading_or_notices() {
        let mut h = Harness::new();
        h.set_scope("/a");
        h.panel.activate();
        h.next_start().await;

        h.set_scope("/b");
        h.panel.activate();
        h.next_start().await;
        assert!(h.panel.state.loading());

        h.reply(0, Err(failure("old a failed")));
        let late_a = h.next_result().await;
        assert!(!h.panel.apply_result(late_a));
        assert!(h.panel.state.loading(), "b must keep loading");
        assert!(
            h.panel.take_notices().is_empty(),
            "old failure must be silent"
        );

        h.reply(1, Ok(rows("b")));
        let b = h.next_result().await;
        assert!(h.panel.apply_result(b));
        assert!(!h.panel.state.loading());
        assert_eq!(titles(&h.panel), vec!["b"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switching_while_hidden_rejects_the_old_result_and_requeries_on_show() {
        let mut h = Harness::new();
        h.set_scope("/a");
        h.panel.activate();
        h.next_start().await;

        // 面板隐藏时不 activate;作用域变化仍然立即失效旧 epoch 并清空状态。
        h.set_scope("/b");
        h.reply(0, Ok(rows("a")));
        let late_a = h.next_result().await;
        assert!(!h.panel.apply_result(late_a));
        assert!(titles(&h.panel).is_empty());

        h.panel.activate();
        assert_eq!(h.next_start().await, "/b");
        h.reply(1, Ok(rows("b")));
        let b = h.next_result().await;
        assert!(h.panel.apply_result(b));
        assert_eq!(titles(&h.panel), vec!["b"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_scope_refreshes_keep_the_latest_and_resync_is_idempotent() {
        let mut h = Harness::new();
        h.set_scope("/a");
        let epoch = h.panel.scope_identity().unwrap().1;
        h.panel.activate();
        h.next_start().await;
        h.panel.activate();
        h.next_start().await;

        h.reply(1, Ok(rows("new")));
        let new = h.next_result().await;
        assert!(h.panel.apply_result(new));
        assert_eq!(titles(&h.panel), vec!["new"]);

        // 相同 scope 重复同步不清数据、不换 epoch。
        h.set_scope("/a");
        assert_eq!(h.panel.scope_identity().unwrap().1, epoch);
        assert_eq!(titles(&h.panel), vec!["new"]);

        // 旧刷新仍在同一生命周期内,由共享 refresh_generation 丢弃。
        h.reply(0, Ok(rows("old")));
        let old = h.next_result().await;
        assert!(h.panel.apply_result(old));
        assert_eq!(titles(&h.panel), vec!["new"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn changing_scope_clears_rows_filter_and_loading() {
        let mut h = Harness::new();
        h.set_scope("/a");
        h.panel.activate();
        h.next_start().await;
        h.reply(0, Ok(rows("a")));
        let a = h.next_result().await;
        assert!(h.panel.apply_result(a));
        h.panel
            .update(UsageMessage::AgentFilterSet(Some(AgentKind::Claude)));
        assert!(h.panel.state.agent_filter().is_some());

        h.set_scope("/b");
        assert!(titles(&h.panel).is_empty());
        assert!(h.panel.state.agent_filter().is_none());
        assert!(!h.panel.state.loading());
        assert!(h.panel.take_notices().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exhausted_epoch_refuses_new_queries_without_wrapping() {
        let mut h = Harness::new();
        h.set_scope("/a");
        h.panel.force_epoch(u64::MAX);
        h.set_scope("/b");

        assert!(h.panel.epoch_exhausted());
        assert!(h.panel.scope_identity().is_none());
        h.panel.activate();
        assert!(h.calls.lock().unwrap().is_empty());
        assert!(!h.panel.state.loading());
    }
}
