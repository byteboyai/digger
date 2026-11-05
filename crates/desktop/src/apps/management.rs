//! 「应用」管理视图的 iced 渲染(Task 14 / 共享设置迁移)。
//!
//! 列表、运行时、安装流程、审批、日志与忙碌 / 错误状态全部由共享
//! [`bytehost_settings_ui::apps`] 绘制;本模块只补 Digger 自己的:
//! - 服务启动状态与「重试」;
//! - 通知、浏览器数据清理进度 / 失败;
//! - 应用行的「打开」动作与宿主权威 issue 文本。
//!
//! 状态迁移仍然只在 `bytehost_panel::install::InstallFlowState`;本模块不重算策略。

use bytehost_panel::install::Message as InstallMessage;
use bytehost_panel::notice::NoticeLevel;
use byteui::theme::{color, font};
use iced::Element;
use iced::widget::{button, row, text};

use super::{AppsState, Message, Startup};

fn install(message: InstallMessage) -> Message {
    Message::Install(message)
}

fn notes<'a>(app: &bytehost_apps::proto::AppSummary) -> Vec<Element<'a, Message>> {
    match &app.issue {
        Some(issue) => {
            let (title, detail) = bytehost_panel::host::issue_texts(issue);
            vec![
                text(format!("{title}:{detail}"))
                    .size(font::label())
                    .color(color::current().red)
                    .into(),
            ]
        }
        None => Vec::new(),
    }
}

fn leading<'a>(app: &bytehost_apps::proto::AppSummary) -> Vec<Element<'a, Message>> {
    if app.observed.is_transient() {
        return Vec::new();
    }
    vec![
        button(text("打开").size(font::body()).color(color::current().gold))
            .padding([4, 12])
            .on_press(Message::OpenApp(app.id.as_str().to_owned()))
            .into(),
    ]
}

pub fn view(state: &AppsState) -> Element<'_, Message> {
    let tokens = color::current();
    let mut extra_top: Vec<Element<'_, Message>> = Vec::new();
    extra_top.push(
        row![
            iced::widget::Space::new().width(iced::Length::Fill),
            button(text("设置").size(font::body()).color(tokens.gold))
                .padding([4, 12])
                .on_press(Message::OpenSettings),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .width(iced::Length::Fill)
        .into(),
    );

    if let Startup::Unavailable(reason) = state.startup() {
        extra_top.push(
            text(format!("应用服务不可用:{reason}"))
                .size(font::body())
                .color(tokens.red)
                .into(),
        );
        extra_top.push(
            button(text("重试").size(font::body()).color(tokens.gold))
                .padding([4, 12])
                .on_press(Message::RetryService)
                .into(),
        );
        extra_top.push(
            text("修复端口占用/同根锁冲突后点「重试」,或重开此面板。")
                .size(font::label())
                .color(tokens.dim)
                .into(),
        );
    } else if matches!(state.startup(), Startup::Starting) {
        extra_top.push(
            text("应用服务正在启动…")
                .size(font::label())
                .color(tokens.dim)
                .into(),
        );
    }

    if let Some(notice) = state.notice() {
        let tint = match notice.level {
            NoticeLevel::Error => tokens.red,
            NoticeLevel::Warning => tokens.gold,
            NoticeLevel::Success => tokens.green,
            NoticeLevel::Info => tokens.cream,
        };
        extra_top.push(
            row![
                text(notice.text.clone()).size(font::body()).color(tint),
                button(text("关闭").size(font::body()).color(tokens.dim))
                    .padding([4, 12])
                    .on_press(Message::DismissNotice),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }

    if let Some(reason) = state.store_unavailable_reason() {
        extra_top.push(
            text(format!("浏览器数据清理台账不可读：{reason}"))
                .size(font::label())
                .color(tokens.red)
                .into(),
        );
        extra_top.push(
            text("为避免覆盖原文件，清理记录暂不可修改；请修复或恢复台账后重开应用面板。")
                .size(font::label())
                .color(tokens.dim)
                .into(),
        );
    }
    if state.store_dirty() {
        let detail = state.store_last_error().unwrap_or("未知写入错误");
        extra_top.push(
            text(format!("清理意图尚未落盘，重启后可能不会继续：{detail}"))
                .size(font::label())
                .color(tokens.gold)
                .into(),
        );
    }
    if state.store_pending_count() > 0 {
        extra_top.push(
            text(format!(
                "浏览器数据清理中：{} 项",
                state.store_pending_count()
            ))
            .size(font::label())
            .color(tokens.dim)
            .into(),
        );
    }
    for (id, reason) in state.store_failures() {
        extra_top.push(
            row![
                text(format!("{id} 浏览器数据未删除：{reason}"))
                    .size(font::label())
                    .color(tokens.red),
                button("重试清理").on_press(Message::RetryCleanup(id.to_owned())),
            ]
            .spacing(8)
            .into(),
        );
    }

    let mut view = bytehost_settings_ui::AppsView::new(
        state.flow(),
        bytehost_settings_ui::Text::new(&crate::settings::shared_text),
        &install,
        &super::log_scroll_id,
    );
    view.extra_top = extra_top;
    view.app_notes = Some(&notes);
    view.app_leading_actions = Some(&leading);
    bytehost_settings_ui::apps::view(view)
}
