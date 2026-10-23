//! Agent 面板画廊(S6d-3 Task 4):不连 `diggerd`,用预置假会话摆固定状态,
//! 供与 Dozer 逐状态对照截图。
//!
//! 用法:`cargo run -p digger-desktop --example agent_gallery -- --state grouped`
//! (也接受 `--state=grouped`)。状态:empty / one-shell / grouped / active-card /
//! many-tabs / narrow / overflow-open / picker-open / confirm-open / notice /
//! daemon-unavailable。

use byteui::theme::color::ColorTokens;
use digger_desktop::adapters::agent::{AgentPanel, PickerMenu};
use digger_desktop::adapters::agent_display::AgentDisplay;
use digger_desktop::adapters::agent_tabs;
use digger_desktop::fonts;
use digger_desktop::product::Msg;
use dozer_model::{AgentKind, AgentState, SessionInfo};
use dozer_panel_agent::adapter::TaskTitleSource;
use dozer_panel_agent::card::CardCtx;
use dozer_panel_agent::display::TerminalSlots;
use dozer_panel_agent::state::AgentPanelState;
use dozer_panel_agent::tabs::{SessionTab, TabBackend};
use dozer_panel_term::model::TerminalModel;
use dozer_panel_term::osc::OscScanner;
use dozer_panel_term::style::TermStyle;
use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind, SessionAttach, SessionSpec};
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task};

const PID: i64 = 7;
const LIST_W: f32 = 300.0;

struct NoTasks;
impl TaskTitleSource for NoTasks {
    fn task_title_for_session(&self, _session_id: &str) -> Option<String> {
        None
    }
}
static NO_TASKS: NoTasks = NoTasks;

/// 假会话服务:画廊从不创建/attach,调用到就报错,保证不碰真实 `diggerd`。
#[derive(Clone)]
struct FakeSessions;
impl dozer_services::SessionService for FakeSessions {
    fn list_sessions(&self) -> BoxFuture<'_, Result<Vec<SessionInfo>, ServiceError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn create(&self, _: SessionSpec) -> BoxFuture<'_, Result<SessionInfo, ServiceError>> {
        Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "gallery")) })
    }
    fn attach(&self, _: String, _: u64) -> BoxFuture<'_, Result<SessionAttach, ServiceError>> {
        Box::pin(async { Err(ServiceError::new(ServiceErrorKind::Unavailable, "gallery")) })
    }
    fn write(&self, _: String, _: Vec<u8>) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
    fn resize(&self, _: String, _: u16, _: u16) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
    fn kill(&self, _: String) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
    fn close_with_summary(&self, _: String) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
}

fn tab(
    tab_id: usize,
    name: &str,
    agent: AgentKind,
    state: AgentState,
    alive: bool,
    screen: &str,
) -> SessionTab {
    let mut model = TerminalModel::new(100, 30);
    let _ = model.feed(screen.as_bytes());
    SessionTab {
        info: SessionInfo {
            id: format!("gallery-{tab_id}"),
            name: name.into(),
            command: "sh".into(),
            cwd: "/tmp/gallery".into(),
            alive,
            created_ms: 0,
            agent_state: state,
            transcript_path: None,
            project_id: Some(PID),
            agent,
        },
        model,
        alive,
        agent_state: state,
        agent,
        transcript_path: None,
        osc: OscScanner::new(),
        cwd: None,
        last_exit: (!alive).then_some(1),
        llm_model: Some("claude-sonnet-5".into()),
        permission_mode: Some("default".into()),
        last_activity: Some("写 S6d-3 对照画廊".into()),
        workspace_override: None,
        tab_id,
        forwarder: tokio::runtime::Handle::current().spawn(async {}),
        backend: TabBackend::Daemon,
    }
}

fn state_for(name: &str) -> (AgentPanel, AgentPanelState, (f32, f32)) {
    let mut panel = AgentPanel::with_services(
        std::sync::Arc::new(FakeSessions),
        std::sync::Arc::new(digger_desktop::adapters::agent_hooks::DiggerHooks::new()),
    );
    panel.shell.status = digger_desktop::adapters::agent::Status::Ready;
    let mut state = AgentPanelState::new();
    let mut window = (1200.0, 800.0);
    match name {
        "empty" => {}
        "one-shell" => {
            state.tabs.push(tab(
                0,
                "shell",
                AgentKind::Unknown,
                AgentState::Idle,
                true,
                "$ echo hi\nhi\n",
            ));
        }
        "grouped" => {
            state.tabs.push(tab(
                0,
                "claude-1",
                AgentKind::Claude,
                AgentState::Running,
                true,
                "claude: running\n",
            ));
            state.tabs.push(tab(
                1,
                "claude-2",
                AgentKind::Claude,
                AgentState::AwaitingInput,
                true,
                "claude: awaiting\n",
            ));
            state.tabs.push(tab(
                2,
                "buddy",
                AgentKind::Codebuddy,
                AgentState::TurnEnded,
                true,
                "buddy: done\n",
            ));
            state.tabs.push(tab(
                3,
                "shell",
                AgentKind::Unknown,
                AgentState::Idle,
                false,
                "exit 1\n",
            ));
            state.active = 1;
        }
        "active-card" => {
            state.tabs.push(tab(
                0,
                "claude",
                AgentKind::Claude,
                AgentState::Running,
                true,
                "claude: working\n",
            ));
            state.active = 0;
        }
        "many-tabs" => {
            for i in 0..12 {
                state.tabs.push(tab(
                    i,
                    &format!("tab-{i}"),
                    AgentKind::Unknown,
                    AgentState::Idle,
                    true,
                    "shell\n",
                ));
            }
            state.active = 11;
        }
        "narrow" => {
            window = (760.0, 600.0);
            state.tabs.push(tab(
                0,
                "claude",
                AgentKind::Claude,
                AgentState::Running,
                true,
                "claude\n",
            ));
            state.tabs.push(tab(
                1,
                "shell",
                AgentKind::Unknown,
                AgentState::Idle,
                true,
                "shell\n",
            ));
        }
        "overflow-open" => {
            for i in 0..8 {
                state.tabs.push(tab(
                    i,
                    &format!("tab-{i}"),
                    AgentKind::Unknown,
                    AgentState::Idle,
                    true,
                    "shell\n",
                ));
            }
            state.active = 3;
            panel.open_overflow(PID, (820.0, 120.0));
        }
        "picker-open" => {
            state.tabs.push(tab(
                0,
                "shell",
                AgentKind::Unknown,
                AgentState::Idle,
                true,
                "shell\n",
            ));
            panel.open_picker(PickerMenu {
                pid: PID,
                at: Some((760.0, 120.0)),
            });
        }
        "confirm-open" => {
            state.tabs.push(tab(
                0,
                "claude",
                AgentKind::Claude,
                AgentState::Running,
                true,
                "claude\n",
            ));
            state.pending_close_tab = Some("gallery-0".into());
        }
        "notice" => {
            state.tabs.push(tab(
                0,
                "shell",
                AgentKind::Unknown,
                AgentState::Idle,
                true,
                "shell\n",
            ));
            panel.notice = Some("Could not create a new session: diggerd 拒绝了请求".into());
        }
        "daemon-unavailable" => {
            panel.shell.status =
                digger_desktop::adapters::agent::Status::Unavailable("diggerd 没起来".into());
        }
        other => eprintln!("未知状态 {other},用 empty"),
    }
    if !state.tabs.is_empty() {
        state.next_tab_id = state.tabs.len();
    }
    (panel, state, window)
}

struct Gallery {
    panel: AgentPanel,
    state: AgentPanelState,
    window: (f32, f32),
    style: TermStyle,
}

impl Gallery {
    fn new(state_name: &str) -> Self {
        let (panel, state, window) = state_for(state_name);
        Self {
            panel,
            state,
            window,
            style: fonts::terminal_style(),
        }
    }

    fn body(&self) -> Element<'_, Msg> {
        if matches!(
            self.panel.shell.status,
            digger_desktop::adapters::agent::Status::Unavailable(_)
                | digger_desktop::adapters::agent::Status::Checking
                | digger_desktop::adapters::agent::Status::Unknown
        ) {
            return self.panel.shell.view(true).map(Msg::AgentShell);
        }
        let term_block_w = (self.window.0 - LIST_W - 8.0).max(1.0);
        let display = AgentDisplay {
            panel: &self.panel,
            pid: PID,
            term_style: &self.style,
            locale: "en",
            tab_bar_px: agent_tabs::tab_bar_avail_px(
                term_block_w,
                byteui::theme::icon_size::scale(),
            ),
            list_on_left: true,
        };
        let list: Element<'_, Msg> = dozer_panel_agent::view::list::agent_list(
            &self.state,
            &display,
            &CardCtx {
                titles: &NO_TASKS,
                project_branch: Some("main"),
                project_dirty: true,
            },
            Length::Fixed(LIST_W),
            iced_widget::core::Border::default(),
        );
        let term: Element<'_, Msg> = dozer_panel_agent::view::terminal::terminal_pane(
            &self.state,
            &display,
            TerminalSlots {
                floating: None,
                footer: None,
            },
            Length::Fill,
            iced_widget::core::Border::default(),
        );
        let body: Element<'_, Msg> = row![list, term].height(Length::Fill).into();
        let body: Element<'_, Msg> = match &self.panel.notice {
            Some(note) => column![
                text(note.clone())
                    .size(13.0)
                    .color(byteui::theme::color::current().red),
                body
            ]
            .height(Length::Fill)
            .into(),
            None => body,
        };
        // 覆盖层顺序:确认框 > 选择器 > 溢出下拉(画廊一次只摆一个)。
        if let Some(dialog) =
            dozer_panel_agent::picker::close_confirm_spec(&self.state, "en", |m| Msg::Agent(PID, m))
        {
            let card: Element<'_, Msg> = container(byteui::feedback::dialog::confirm(dialog))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into();
            return iced::widget::stack![body, card].into();
        }
        if let Some(overlay) =
            digger_desktop::adapters::agent_display::picker_overlay(&self.panel, self.window)
        {
            return iced::widget::stack![body, overlay].into();
        }
        if let Some(overlay) =
            digger_desktop::adapters::agent_display::overflow_overlay(&self.panel, self.window)
        {
            return iced::widget::stack![body, overlay].into();
        }
        body
    }
}

fn window_size(name: &str) -> iced::Size {
    match name {
        "narrow" => iced::Size::new(760.0, 600.0),
        _ => iced::Size::new(1200.0, 800.0),
    }
}

fn gallery_update(_: &mut Gallery, _: Msg) -> Task<Msg> {
    Task::none()
}

fn gallery_view(gallery: &Gallery) -> Element<'_, Msg> {
    container(gallery.body()).padding(8).into()
}

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    let state_name = args
        .iter()
        .position(|a| a == "--state")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--state=").map(str::to_string))
        })
        .unwrap_or_else(|| "empty".into());
    byteui::theme::color::set_theme(ColorTokens::byteboy2077());
    byteui::theme::font::set_theme(byteui::theme::font::FontTokens::byteboy2077());
    let size = window_size(&state_name);
    let boot_state = state_name.clone();
    iced::application(
        move || (Gallery::new(&boot_state), Task::none()),
        gallery_update,
        gallery_view,
    )
    .font(fonts::JETBRAINS_MONO)
    .title(move |_: &Gallery| format!("Agent gallery · {state_name}"))
    .window_size(size)
    .subscription(|_: &Gallery| Subscription::none())
    .run()
}
