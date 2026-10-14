//! 已安装应用行上的「操作」按钮与行内二次确认(Task 15;规格 §6.2 启动/停止/回滚/卸载)。
//!
//! 每行按当前观测状态给**启动**/**停止**,带上一版时给**回滚**,并给**卸载**;点击卸载/回滚进
//! [`bytehost_panel::install::Flow`] 的 `ConfirmUninstall`/`ConfirmRollback`,由本模块就地渲染二次
//! 确认(而不是弹全局对话框),确认后状态机才吐 `Effect::Uninstall`/`Effect::Rollback`。
//!
//! 卸载的两种行为在这里被显式区分:「只删程序(保留数据)」与「连数据一起删」——两个按钮分别发
//! `UninstallConfirmed(Program)` / `UninstallConfirmed(ProgramAndData)`,用户动作与后果一一对应。
//!
//! 应用行是**只读展示**状态;运行中/失败原因/回滚说明都来自宿主权威状态(`observed_label` /
//! `rollback_note_label`),本模块不猜。

use bytehost_apps::proto::AppSummary;
use bytehost_apps::registry::UninstallMode;
use bytehost_panel::host::issue_texts;
use bytehost_panel::install::{
    ActKind, Flow, Message as InstallMessage, observed_label, rollback_note_label,
};
use byteui::theme::{color, font};

use iced::widget::text::IntoFragment;
use iced::widget::{button, column, row, text};
use iced::{Alignment, Element, Length};

use super::{AppsState, Message};

fn action_button<'a>(
    label: impl IntoFragment<'a>,
    msg: Message,
    tint: iced::Color,
) -> Element<'a, Message> {
    button(text(label).size(font::body()).color(tint))
        .padding([4, 12])
        .on_press(msg)
        .into()
}

fn dim<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into())
        .size(font::label())
        .color(color::current().dim)
}

/// 一行应用:名称 + 版本 + 观测状态(权威)+ 回滚说明 + 操作按钮 / 就地确认。
pub fn app_row<'a>(state: &'a AppsState, app: &'a AppSummary) -> Element<'a, Message> {
    let colors = color::current();
    let mut col = column![
        row![
            text(app.name.as_str())
                .size(font::body())
                .color(colors.cream),
            dim(format!("版本 {}", app.version)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        dim(format!(
            "{} · {}",
            observed_label(&app.observed),
            app.id.as_str()
        )),
    ]
    .spacing(4)
    .width(Length::Fill);

    // 运行出问题时的持续原因(不是一次性 Toast;标题 + 详情来自宿主权威文本)。
    if let Some(issue) = &app.issue {
        let (title, detail) = issue_texts(issue);
        col = col.push(
            text(format!("{title}:{detail}"))
                .size(font::label())
                .color(colors.red),
        );
    }
    // 回滚过的持久说明(自动回滚点出原因)。
    if let Some(note) = &app.rollback_note {
        col = col.push(dim(rollback_note_label(note)));
    }

    // 就地二次确认(该行处于确认步骤时才显示,替换按钮行)。
    match &state.flow().flow {
        Flow::ConfirmUninstall { id, name } if id == app.id.as_str() => {
            col = col.push(uninstall_confirm(name.as_str(), id.as_str()));
            return col.into();
        }
        Flow::ConfirmRollback { id, name, to } if id == app.id.as_str() => {
            col = col.push(rollback_confirm(
                name.as_str(),
                id.as_str(),
                &to.to_string(),
            ));
            return col.into();
        }
        _ => {}
    }

    col = col.push(action_row(state, app));

    // 该行展开着日志:就地渲染日志查看器(跟随/刷新失败保留旧内容由状态机决定)。
    if state.flow().is_log_open(app.id.as_str()) {
        col = col.push(super::logs::log_viewer(
            state.flow().logs_view(app.id.as_str()),
            app.id.as_str(),
        ));
    }
    col.into()
}

/// 一行操作按钮:停止(运行中/启动中才给;启动经宿主栏,不在此重复)+ 回滚(有上一版)+ 卸载。
fn action_row<'a>(state: &AppsState, app: &'a AppSummary) -> Element<'a, Message> {
    use bytehost_apps::state::ObservedState;

    let colors = color::current();
    let id = app.id.as_str().to_owned();
    let acting = state.flow().is_acting(&id);

    let mut buttons = row![].spacing(8);

    if acting.is_none() {
        if !app.observed.is_transient() {
            buttons = buttons.push(action_button(
                "打开",
                Message::OpenApp(id.clone()),
                colors.gold,
            ));
        }
        // 运行中/启动中 → 停止;启动走宿主栏(与应用面板一致,不在此重复)。
        if matches!(
            app.observed,
            ObservedState::Running | ObservedState::Starting
        ) {
            buttons = buttons.push(action_button(
                "停止",
                Message::Install(InstallMessage::StopClicked(id.clone())),
                colors.orange,
            ));
        }

        // 有上一版时才给回滚入口(否则点了也没目标)。
        if let Some(to) = app.previous_version {
            buttons = buttons.push(action_button(
                format!("回滚到 {to}"),
                Message::Install(InstallMessage::RollbackClicked(id.clone())),
                colors.cyan,
            ));
        }

        // 过渡态(启动中/停止中/卸载中)不给卸载,避免与进行中的生命周期操作打架。
        if !app.observed.is_transient() {
            buttons = buttons.push(action_button(
                "卸载",
                Message::Install(InstallMessage::UninstallClicked(id.clone())),
                colors.red,
            ));
        }
    }

    // 日志入口:任何状态(含失败)都能看日志;展开着时按钮变「收起日志」。
    let (log_label, log_msg) = if state.flow().is_log_open(&id) {
        ("收起日志", InstallMessage::HideLogs)
    } else {
        ("日志", InstallMessage::ShowLogs(id.clone()))
    };
    buttons = buttons.push(action_button(
        log_label,
        Message::Install(log_msg),
        colors.dim,
    ));

    let mut col = column![buttons].spacing(4).width(Length::Fill);
    if let Some(kind) = acting {
        let label = match kind {
            ActKind::Stop => "停止中…",
            ActKind::Uninstall => "卸载中…",
        };
        col = col.push(dim(label));
    }
    col.into()
}

/// 卸载应用的二次确认:明确区分「保留数据」与「删除数据」两种行为。
fn uninstall_confirm<'a>(name: &str, id: &str) -> Element<'a, Message> {
    let colors = color::current();
    column![
        text(format!("卸载 {name}?"))
            .size(font::body())
            .color(colors.cream),
        dim("「只删程序」保留 data/ 与 logs/,重装后数据还在;「连数据一起删」会清掉整个应用目录。"),
        row![
            action_button(
                "取消",
                Message::Install(InstallMessage::FlowDismissed),
                colors.dim,
            ),
            action_button(
                "只删程序(保留数据)",
                Message::Install(InstallMessage::UninstallConfirmed(UninstallMode::Program)),
                colors.orange,
            ),
            action_button(
                "连数据一起删",
                Message::Install(InstallMessage::UninstallConfirmed(
                    UninstallMode::ProgramAndData
                )),
                colors.red,
            ),
        ]
        .spacing(8),
        dim(id).size(font::label()),
    ]
    .spacing(6)
    .into()
}

/// 回滚应用的二次确认:说清落到哪一版。
fn rollback_confirm<'a>(name: &str, id: &str, to: &str) -> Element<'a, Message> {
    let colors = color::current();
    column![
        text(format!("把 {name} 回滚到 {to}?"))
            .size(font::body())
            .color(colors.cream),
        row![
            action_button(
                "取消",
                Message::Install(InstallMessage::RollbackCancelled),
                colors.dim,
            ),
            action_button(
                "回滚",
                Message::Install(InstallMessage::RollbackConfirmed(id.to_owned())),
                colors.cyan,
            ),
        ]
        .spacing(8),
    ]
    .spacing(6)
    .into()
}
