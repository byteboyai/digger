//! Usage 面板的 **Digger 宿主适配层**(Task 20)。
//!
//! 面板本体(数据层 / 视图层 / web 资源)是共享库 `dozer-panel-usage`,Dozer 与
//! Digger 共用同一实现(计划 Task 19 第 1 条)。本文件是 Digger 侧唯一的产品适配:
//!
//! - **展示语义**:[`PanelDisplay<Msg>`] 实现(agent 图标/颜色、面板头、列表折叠
//!   按钮、折叠/悬停事件映射)。Digger 尚无品牌资产,agent 一律回落公共 `Bot`
//!   图标 + 主题色——与库的 `NoGitStats` 同样的"显式最小"口径,不伪造。
//! - **数据**:[`dozer_services::UsageService`] 由 [`digger_store::LedgerUsageService`]
//!   提供,数据来自真实的**应用操作**完成事件(见 [`crate::apps`] 的 producer hook),
//!   不是测试插入的假数据。
//! - **Git 统计**:Digger 一期无 Git 统计,注入 [`NoGitStats`](库内)恒 0 / 空。
//!
//! 数据接缝遵循计划 Task 20 第 3 条:"业务 crate 实现,宿主适配层注入"——
//! [`UsagePanel::new`] 收一个 `Arc<dyn UsageService>`,由组合根传入真实实现。

use std::path::PathBuf;
use std::sync::Arc;

use dozer_model::AgentKind;
use dozer_panel_usage::adapter::{NoGitStats, PanelDisplay};
use dozer_panel_usage::{Message as UsageMessage, WorkspaceState, activation, on_activate};
use dozer_services::UsageService;
use iced::{Element, Length};
use iced_widget::core::{Border, Color};

use crate::product::Msg;

/// Digger 的 Usage 面板状态:库的 [`WorkspaceState`] + 注入的数据接缝。
pub struct UsagePanel {
    /// 库的数据/视图状态。
    state: WorkspaceState,
    pub native: super::usage_native::Native,
    /// 注入的用量服务(真实实现:`LedgerUsageService`)。
    service: Arc<dyn UsageService>,
    /// 本面板查询的**作用域路径**。Digger 一期无 Project,用产品数据根当唯一
    /// 作用域——应用操作完成事件也记在同一路径下,二者对齐才能被查到。
    scope_path: PathBuf,
    /// iced 运行时句柄(库的 `spawn_refresh` 要它去后台取数)。`None` = 尚未在
    /// 运行时内挂上(`ProductShell::new` 可在无运行时处被测试调用);此时 `activate`
    /// 只置"统计中",不起刷新——与库对"无 ctx"的处理同型。
    handle: Option<tokio::runtime::Handle>,
    /// 后台取数结果的回流发送端:库的 `emit` 把 [`UsageMessage`] 投到这里,组合根在
    /// `boot` 里把它接成 iced 流(与 [`crate::apps`] 的结果回流同型)。
    tx: tokio::sync::mpsc::UnboundedSender<UsageMessage>,
    /// 回流接收端;供 [`UsagePanel::take_receiver`] 取走接成 iced 流。
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<UsageMessage>>,
}

impl UsagePanel {
    /// 构造:注入真实用量服务 + 作用域路径。运行时句柄用 [`UsagePanel::attach_handle`]
    /// 在进入 iced 运行时后补挂(组合根 `boot` 内)。
    pub fn new(service: Arc<dyn UsageService>, scope_path: PathBuf) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state: WorkspaceState::default(),
            native: super::usage_native::Native::new(),
            service,
            scope_path,
            handle: None,
            tx,
            rx: Some(rx),
        }
    }

    /// 挂上 iced 运行时句柄(仅可在运行时上下文内调用)。
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 取走后台结果接收端(组合根在 `boot` 里接成 iced 流;只该取一次)。
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<UsageMessage>> {
        self.rx.take()
    }

    /// 面板切入:构造库的 [`activation::ActivationCtx`] 并调 `on_activate`。
    ///
    /// 与 Dozer 侧逐字段一致:注入 Git 统计(这里 [`NoGitStats`])、注入用量服务、
    /// 用运行时句柄与发射器。`emit` 把库消息投回组合根的回流通道(而不是丢弃)。
    pub fn activate(&mut self) {
        // 还没挂运行时句柄(如测试环境):只置"统计中",不起刷新。
        let Some(handle) = self.handle.clone() else {
            self.state.set_loading(true);
            return;
        };
        let tx = self.tx.clone();
        let ctx = activation::ActivationCtx::new(
            // 库的 `project_id` 仅用于日志/路由归属,Digger 一期用固定实例 id。
            1,
            Some(self.scope_path.clone()),
            Arc::clone(&self.service),
            Arc::new(NoGitStats),
            handle,
            move |message| {
                // 通道关闭 = 组合根已退出;静默丢弃(后台任务随后自然结束)。
                let _ = tx.send(message);
            },
        );
        on_activate(&mut self.state, Some(&ctx));
    }

    /// 处理一条库消息(面板已激活后的异步结果 / 交互)。
    pub fn update(&mut self, message: UsageMessage) {
        dozer_panel_usage::update(&mut self.state, message);
    }

    /// 排空待发提示(库的 outbox)。Digger 尚无 Toast 机制,一期由调用方落地(见
    /// [`UsagePanel::take_notices`] 的说明);返回文本供调用方处理。
    pub fn take_notices(&mut self) -> Vec<(String, String)> {
        self.state
            .take_outbox()
            .into_iter()
            .map(|n| (format!("{:?}", n.level), n.text))
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

    /// 原生标题与加载态；图表由同源 UsageSurface 和嵌入资源驱动。
    pub fn view(&self) -> Element<'_, Msg> {
        if let Some(error) = &self.native.error {
            return iced::widget::text(format!("用量视图创建失败：{error}；切换页签可重试")).into();
        }
        dozer_panel_usage::view::content_pane(
            self,
            false,
            &self.state,
            Length::Fill,
            Border::default(),
        )
    }
}

/// 把用量面板的后台结果接收端接成一条持续到进程结束的 iced 流(与 [`crate::apps`] 同型)。
pub fn receiver_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<UsageMessage>,
) -> impl iced::futures::Stream<Item = UsageMessage> {
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
        title: &'a str,
        _actions: Option<Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer>>,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        use iced::widget::{column, container, row, text};
        let tokens = byteui::theme::color::current();
        let icon = match title {
            "Agent" => byteui::interaction::icons::IconKind::Bot,
            _ => byteui::interaction::icons::IconKind::BarChart3,
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
        _collapsed: bool,
        _tooltip_collapse: &'a str,
        _tooltip_expand: &'a str,
        on_select: Msg,
        _on_hover: impl Fn(bool) -> Msg + 'a,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        // 一期无分栏折叠 UI:给一个触发 `ToggleListCollapse` 的普通按钮,功能可见即可。
        iced::widget::button(byteui::interaction::icons::view(
            byteui::interaction::icons::IconKind::BarChart3,
            byteui::theme::icon_size::row(),
            byteui::theme::color::current().dim,
        ))
        .on_press(on_select)
        .into()
    }

    fn collapse_hover(&self, _hovered: bool) -> Msg {
        // 库内 `CollapseHover` 在产品 update 里是 no-op(match 穷尽用);Digger 无悬停键。
        Msg::Usage(UsageMessage::CollapseHover(false))
    }

    fn toggle_list_collapse(&self) -> Msg {
        Msg::Usage(UsageMessage::ToggleListCollapse)
    }
}
