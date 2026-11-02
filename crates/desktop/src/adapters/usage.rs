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

use bytehost_chrome::ShellMsg;
use dozer_model::AgentKind;
use dozer_panel_usage::adapter::{NoGitStats, PanelDisplay, PanelHeadKind};
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
    ///
    /// 库的文案是 [`dozer_model::LocalizedText`]:Digger 一期无 i18n 解析器,
    /// 取 `fallback`(spec §6.1 保证非空的官方英文兜底)当纯文本,不引入 locale。
    pub fn take_notices(&mut self) -> Vec<(String, String)> {
        self.state
            .take_outbox()
            .into_iter()
            .map(|n| (format!("{:?}", n.level), n.text.fallback))
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
        if let Some(error) = &self.native.error {
            return iced::widget::text(format!("用量视图创建失败：{error}；切换页签可重试")).into();
        }
        // 区域外框已由骨架画,这里不再加边框。Digger 一期无 locale 切换,传 "en"。
        dozer_panel_usage::view::content_pane(
            self,
            collapsed,
            &self.state,
            Length::Fill,
            Border::default(),
            "en",
        )
    }

    /// agent 筛选列表。
    pub fn list_view(&self) -> Element<'_, Msg> {
        dozer_panel_usage::view::list_pane(self, &self.state, Length::Fill, Border::default(), "en")
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

    fn panel() -> UsagePanel {
        let svc: Arc<dyn UsageService> = Arc::new(digger_store::UnavailableUsageService::new(
            "test".to_string(),
        ));
        UsagePanel::new(svc, PathBuf::from("/tmp/digger-usage-test"))
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
        p.update(sample_loaded_message());
        assert!(p.has_agent_filter());
    }

    #[test]
    fn both_panes_build_in_both_collapse_states() {
        let mut p = panel();
        p.update(sample_loaded_message());
        let _ = p.content_view(false);
        let _ = p.content_view(true);
        let _ = p.list_view();
    }
}
