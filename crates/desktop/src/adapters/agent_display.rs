//! Agent 面板的 Digger 展示适配(S6d-2 Task 4)。
//!
//! 库 `dozer-panel-agent` 的视图只认 [`PanelDisplay`]:面板头、折叠按钮、页签
//! 部件、区域配色、终端样式与焦点都由产品注入。Digger 的取舍:
//! - **无悬停动画(D3):** `hover_progress` 恒 0、`hover_tooltip_ready` 恒假,
//!   `hover_message` 返回 `ShellMsg::Noop`。
//! - **朴素页签:** 状态点 + 标题 + 关闭 ×,选中加实底;**Dozer 同款页签、
//!   溢出与拖拽换位留给 S6d-3**。
//! - 列表在配对里是第二块(右栏时画在终端右边),收起按钮按实际左右画方向。

use bytehost_chrome::ShellMsg;
use dozer_panel_agent::display::{
    AgentHover, AgentPane, El, OverflowEntry, PaneStyle, PanelDisplay, TabArgs,
};
use dozer_panel_agent::picker::{PickerEntry, picker_entries};
use dozer_panel_agent::update::Message;
use dozer_panel_term::style::TermStyle;
use dozer_panel_term::view::TermEvent;
use iced_widget::core::{Length, Padding};
use iced_widget::{column, row, text};

use crate::adapters::agent::AgentPanel;
use crate::adapters::menu_overlay::{MenuAnchor, OverlayRow, view_rows};
use crate::product::Msg;
use crate::shell;

/// 列表块的内边距(与 Dozer `agent_list_pane` 同值,上窄下宽)。
fn list_padding() -> Padding {
    Padding {
        top: 6.0,
        right: 12.0,
        bottom: 12.0,
        left: 12.0,
    }
}

fn terminal_padding() -> f32 {
    8.0
}

/// 折叠/展开 Agent 列表的消息(按钮与测试共用一条构造路径)。
pub fn toggle_pane_message() -> Msg {
    Msg::Shell(ShellMsg::TogglePane(shell::agent_id()))
}

/// 终端块区域样式(列表块的另一份在 `pane_style` 里):`agent_layout::grid`
/// 与 `PanelDisplay` 必须用同一份内边距,否则网格与渲染差一圈。
pub fn terminal_pane_style() -> PaneStyle {
    PaneStyle {
        gap: 4.0,
        padding: Padding::from(terminal_padding()),
        background: Some(byteui::theme::color::current().term_bg),
    }
}

pub struct AgentDisplay<'a> {
    pub panel: &'a AgentPanel,
    pub pid: i64,
    pub term_style: &'a TermStyle,
    pub locale: &'a str,
    pub tab_bar_px: f32,
    pub list_on_left: bool,
}

impl<'a> AgentDisplay<'a> {}

impl<'a> PanelDisplay<'a, Msg> for AgentDisplay<'a> {
    fn wrap(&self, message: Message) -> Msg {
        Msg::Agent(self.pid, message)
    }
    fn locale(&self) -> &str {
        self.locale
    }
    fn hover_progress(&self, slot: AgentHover) -> f32 {
        self.panel.hover_progress(slot)
    }
    fn hover_message(&self, slot: AgentHover, hovered: bool) -> Msg {
        Msg::Agent(self.pid, Message::Hover(slot, hovered))
    }
    fn hover_tooltip_ready(&self, slot: AgentHover) -> bool {
        self.panel.hover_tooltip_ready(slot)
    }
    fn pane_style(&self, pane: AgentPane) -> PaneStyle {
        let tokens = byteui::theme::color::current();
        match pane {
            AgentPane::List => PaneStyle {
                gap: 8.0,
                padding: list_padding(),
                background: Some(tokens.bg),
            },
            AgentPane::Terminal => terminal_pane_style(),
        }
    }
    fn panel_head(
        &self,
        icon: byteui::interaction::icons::IconKind,
        title: &'a str,
        actions: Option<El<'a, Msg>>,
    ) -> El<'a, Msg> {
        let tokens = byteui::theme::color::current();
        let head = row![
            byteui::interaction::icons::view(
                icon,
                byteui::theme::icon_size::row(),
                tokens.tab_active_border
            ),
            text(title.to_string())
                .size(byteui::theme::font::title())
                .color(tokens.tab_active_border),
        ]
        .spacing(8)
        .height(Length::Fixed(byteui::theme::geometry::tab_button_size()))
        .align_y(iced::Alignment::Center);
        let head: El<'a, Msg> = match actions {
            Some(actions) => row![head, iced_widget::space::horizontal(), actions]
                .align_y(iced::Alignment::Center)
                .into(),
            None => head.into(),
        };
        column![head].spacing(4).into()
    }
    fn list_collapse_button(&self, collapse: String, expand: String) -> El<'a, Msg> {
        use byteui::interaction::icons::{self, IconKind};
        let collapsed = crate::product::pane_collapsed_of(&shell::agent_id());
        let (icon, tip) = match (collapsed, self.list_on_left) {
            (false, true) => (IconKind::PanelLeftClose, collapse),
            (true, true) => (IconKind::PanelLeftOpen, expand),
            (false, false) => (IconKind::PanelRightClose, collapse),
            (true, false) => (IconKind::PanelRightOpen, expand),
        };
        icons::icon_button_entry(
            icon,
            byteui::theme::icon_size::row(),
            false,
            false,
            0.0,
            false,
            byteui::theme::icon_size::row() + 6.0,
            true,
            Msg::Shell(ShellMsg::TogglePane(shell::agent_id())),
            |_| Msg::Shell(ShellMsg::Noop),
            tip,
        )
    }
    fn tab_divider(&self) -> El<'a, Msg> {
        byteui::layout::divider::horizontal()
    }
    fn panel_tab(&self, args: TabArgs<Msg>) -> El<'a, Msg> {
        let pid = self.pid;
        let hover = self.panel.hover_progress(args.item);
        let close_hover = self.panel.hover_progress(args.close);
        let show_tooltip = self.panel.hover_tooltip_ready(args.item);
        let item = args.item;
        let close = args.close;
        crate::adapters::agent_tabs::panel_tab(
            args,
            hover,
            close_hover,
            show_tooltip,
            move |hovered| Msg::Agent(pid, Message::Hover(item, hovered)),
            move |hovered| Msg::Agent(pid, Message::Hover(close, hovered)),
        )
    }
    fn overflow_button(&self, tab_count: usize, on_press: Msg) -> Option<El<'a, Msg>> {
        let pid = self.pid;
        crate::adapters::agent_tabs::overflow_button(
            tab_count,
            self.panel.hover_progress(AgentHover::TabOverflow),
            on_press,
            move |hovered| Msg::Agent(pid, Message::Hover(AgentHover::TabOverflow, hovered)),
        )
    }
    fn overflow_menu(
        &self,
        entries: Vec<OverflowEntry>,
        anchor: (f32, f32),
        on_select: fn(usize) -> Message,
        on_close: fn(usize) -> Message,
        on_dismiss: Message,
    ) -> El<'a, Msg> {
        let pid = self.pid;
        let panel = self.panel;
        crate::adapters::agent_tabs::overflow_menu(
            entries,
            anchor,
            crate::CURRENT_WINDOW
                .with(|w| w.get())
                .unwrap_or((1280.0, 800.0)),
            move |idx| Msg::Agent(pid, on_select(idx)),
            move |idx| Msg::Agent(pid, on_close(idx)),
            move |idx, hovered| {
                Msg::Agent(pid, Message::Hover(AgentHover::OverflowRow(idx), hovered))
            },
            move |slot| panel.hover_progress(slot),
            Msg::Agent(pid, on_dismiss),
        )
    }
    fn drag_surface(&self, content: El<'a, Msg>, index: usize) -> El<'a, Msg> {
        // 手势全在产品侧:surface 只上报"光标在第 index 个页签上",产品据此
        // 判断是否越过阈值并调库的 `reorder_term_tab`。
        let pid = self.pid;
        iced_widget::MouseArea::new(content)
            .on_move(move |_| Msg::AgentTabHover(pid, index))
            .into()
    }
    fn term_style(&self) -> &'a TermStyle {
        self.term_style
    }
    fn terminal_focused(&self) -> bool {
        self.panel.focus
    }
    fn ime_preedit(&self) -> Option<&'a str> {
        self.panel.preedit.as_deref()
    }
    fn term_event(&self, event: TermEvent) -> Msg {
        Msg::Agent(self.pid, dozer_panel_agent::term_message(event))
    }
    fn term_event_fn(&self) -> fn(TermEvent) -> Msg {
        // 终端 canvas 要一个不带捕获的映射函数;当前渲染的项目 id 由
        // `agent_view` 每帧写进 thread-local(同 `CURRENT_WINDOW` 的手法)。
        crate::adapters::agent::term_event_message
    }
    fn tab_bar_avail_px(&self) -> f32 {
        self.tab_bar_px
    }
}

/// Agent 选择器浮层:条目与图标颜色都取库的 `picker_entries()`。
pub fn picker_overlay(panel: &AgentPanel, window: (f32, f32)) -> Option<El<'_, Msg>> {
    let picker = panel.picker?;
    let state = panel.states.get(&picker.pid)?;
    if !state.agent_picker_open {
        return None;
    }
    let rows: Vec<OverlayRow> = picker_entries()
        .into_iter()
        .map(|entry| match entry {
            PickerEntry::Launch {
                launch,
                label,
                icon,
                icon_color,
            } => OverlayRow::Entry {
                icon: Some(icon),
                label,
                enabled: true,
                msg: Box::new(Msg::Agent(picker.pid, Message::PickerSelect(launch))),
                tint: Some(icon_color),
            },
            PickerEntry::Separator => OverlayRow::Separator,
        })
        .collect();
    let anchor = picker
        .at
        .map(|(x, y)| MenuAnchor { x, y })
        .unwrap_or(MenuAnchor {
            x: (window.0 - 260.0).max(0.0),
            y: 96.0,
        });
    Some(view_rows(rows, anchor, window, Msg::AgentPickerClose))
}

/// 供测试:选择器每一条都可点、分隔线在最后两个 shell 之前。
/// 溢出下拉浮层(窗口坐标,与选择器互斥)。
pub fn overflow_overlay(panel: &AgentPanel, window: (f32, f32)) -> Option<El<'_, Msg>> {
    let (pid, anchor) = panel.overflow?;
    let state = panel.states.get(&pid)?;
    if state.tabs.is_empty() {
        return None;
    }
    let entries: Vec<OverflowEntry> = state
        .tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| OverflowEntry {
            index: idx,
            title: dozer_panel_agent::tabs::tab_title(
                tab.agent,
                tab.cwd.as_deref(),
                &tab.info.name,
            ),
            active: idx == state.active,
            dot: dozer_panel_agent::style::dot_color(tab.agent_state, tab.alive),
            row: AgentHover::OverflowRow(idx),
        })
        .collect();
    Some(crate::adapters::agent_tabs::overflow_menu(
        entries,
        anchor,
        window,
        move |idx| Msg::Agent(pid, Message::SelectTab(idx)),
        move |idx| Msg::Agent(pid, Message::CloseTab(idx)),
        move |idx, hovered| Msg::Agent(pid, Message::Hover(AgentHover::OverflowRow(idx), hovered)),
        move |slot| panel.hover_progress(slot),
        Msg::AgentOverflowDismiss,
    ))
}

/// 供测试:选择器每一条都可点、分隔线在最后两个 shell 之前。
#[cfg(test)]
pub fn picker_row_labels() -> Vec<String> {
    picker_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            PickerEntry::Launch { label, .. } => Some(label),
            PickerEntry::Separator => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::agent::{AgentPanel, Status};
    use dozer_panel_agent::display::PanelDisplay;

    fn style() -> TermStyle {
        TermStyle {
            font: iced::Font::with_name("JetBrains Mono"),
            font_size: 14.0,
            line_height_factor: 1.2,
        }
    }

    fn display<'a>(panel: &'a AgentPanel, style: &'a TermStyle) -> AgentDisplay<'a> {
        AgentDisplay {
            panel,
            pid: 7,
            term_style: style,
            locale: "en",
            tab_bar_px: 400.0,
            list_on_left: false,
        }
    }

    #[test]
    fn list_collapse_button_toggles_the_pane_of_the_agent_panel() {
        assert!(matches!(
            toggle_pane_message(),
            Msg::Shell(ShellMsg::TogglePane(ref id)) if id == &shell::agent_id()
        ));
    }

    #[test]
    fn plain_tab_close_sends_close_for_that_tab() {
        let style = style();
        let panel = AgentPanel::new();
        let d = display(&panel, &style);
        // 页签的 × 由库用 `host.wrap(Message::CloseTab(idx))` 构造;这里钉住
        // 包装只加项目 id、不改页签下标,所以点第 2 个页签关的就是第 2 个。
        assert!(matches!(
            d.wrap(Message::CloseTab(2)),
            Msg::Agent(7, Message::CloseTab(2))
        ));
        assert!(matches!(
            d.wrap(Message::CloseTab(0)),
            Msg::Agent(7, Message::CloseTab(0))
        ));
    }

    #[test]
    fn hover_is_static_and_tooltips_never_show() {
        let style = style();
        let panel = AgentPanel::new();
        let d = display(&panel, &style);
        for slot in [
            AgentHover::PickerToggle,
            AgentHover::TabItem(0),
            AgentHover::TabClose(0),
            AgentHover::TabOverflow,
            AgentHover::OverflowRow(0),
        ] {
            assert_eq!(d.hover_progress(slot), 0.0);
            assert!(!d.hover_tooltip_ready(slot));
        }
    }

    #[test]
    fn term_event_maps_to_the_agent_message_of_this_project() {
        let style = style();
        let panel = AgentPanel::new();
        let d = display(&panel, &style);
        assert!(matches!(
            d.term_event(TermEvent::Scroll(3)),
            Msg::Agent(7, Message::Scroll(3))
        ));
        assert!(matches!(
            d.term_event(TermEvent::Input(vec![b'x'])),
            Msg::Agent(7, Message::Input(ref bytes)) if bytes == b"x"
        ));
        assert!(matches!(
            d.term_event(TermEvent::SelStart {
                col: 1,
                row: 2,
                right_half: true
            }),
            Msg::Agent(
                7,
                Message::SelStart {
                    col: 1,
                    row: 2,
                    right: true
                }
            )
        ));
    }

    #[test]
    fn picker_overlay_lists_every_known_agent_then_the_two_shells() {
        let labels = picker_row_labels();
        assert!(labels.iter().any(|l| l == "Git Shell"));
        assert!(labels.iter().any(|l| l == "OS Shell"));
        let agents = labels.len() - 2;
        assert_eq!(agents, dozer_model::AgentKind::known().count());
    }

    #[test]
    fn the_picker_is_not_drawn_without_a_pending_open() {
        let panel = AgentPanel::new();
        assert!(picker_overlay(&panel, (1000.0, 700.0)).is_none());
    }

    #[test]
    fn the_picker_is_drawn_only_while_the_state_says_it_is_open() {
        let mut panel = AgentPanel::new();
        panel.picker = Some(crate::adapters::agent::PickerMenu { pid: 7, at: None });
        assert!(picker_overlay(&panel, (1000.0, 700.0)).is_none());
        let mut state = dozer_panel_agent::state::AgentPanelState::new();
        state.agent_picker_open = true;
        panel.states.insert(7, state);
        assert!(picker_overlay(&panel, (1000.0, 700.0)).is_some());
    }

    #[test]
    fn an_unavailable_daemon_never_builds_a_pair() {
        let mut panel = AgentPanel::new();
        panel.shell.status = Status::Unavailable("x".into());
        assert!(!panel.pair_ready(Some(7)));
        panel.shell.status = Status::Ready;
        assert!(!panel.pair_ready(Some(7)), "no state for the project yet");
        panel.states.insert(7, Default::default());
        assert!(panel.pair_ready(Some(7)));
        assert!(!panel.pair_ready(None));
    }
}
