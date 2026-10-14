//! 「应用」管理视图的 iced 渲染(Task 14)。
//!
//! 只读 [`crate::apps::AppsState`] 的状态并产生 [`Message`];所有状态迁移在
//! [`bytehost_panel::install::InstallFlowState`] 里。本模块**不重算**任何策略。
//!
//! 覆盖计划 Task 14 第 1 条的三类可区分状态:
//! - **服务不可用**:`startup == Unavailable` 时置顶显示可操作原因,不显示安装表单;
//! - **空列表**:列表为空且服务可用时显示"还没有安装任何应用";
//! - **计划生成失败**:状态机进入 `Flow::Failed`,显示原因与「知道了」。

use bytehost_panel::install::{Flow, Load, Message as InstallMessage, PickTarget, SourceChoice};
use bytehost_panel::notice::NoticeLevel;
use byteui::theme::{color, font};

use iced::widget::text::IntoFragment;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};

use super::{AppsState, Message, Startup, actions, runtime};
use crate::apps::approval::review_view;

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

fn heading<'a>(label: &'static str) -> Element<'a, Message> {
    text(label)
        .size(font::subtitle())
        .color(color::current().cream)
        .into()
}

pub fn view(state: &AppsState) -> Element<'_, Message> {
    let mut body = column![heading("应用")].spacing(12).width(Length::Fill);

    // 0) 状态机瞬时消息(复审 #8):回滚失败、运行时卸载失败等此前被静默丢弃,这里顶部可见、可关闭。
    if let Some(notice) = state.notice() {
        let tint = match notice.level {
            NoticeLevel::Error => color::current().red,
            NoticeLevel::Warning => color::current().gold,
            NoticeLevel::Success => color::current().green,
            NoticeLevel::Info => color::current().cream,
        };
        body = body.push(
            row![
                text(notice.text.clone()).size(font::body()).color(tint),
                action_button("关闭", Message::DismissNotice, color::current().dim),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        );
    }

    if state.store.pending_count() > 0 {
        body = body.push(dim(format!(
            "浏览器数据清理中：{} 项",
            state.store.pending_count()
        )));
    }
    for (id, reason) in state.store.failures() {
        body = body.push(
            row![
                text(format!("{id} 浏览器数据未删除：{reason}")),
                button("重试清理").on_press(Message::RetryCleanup(id.to_owned()))
            ]
            .spacing(8),
        );
    }

    // 1) 服务不可用:置顶显示、可操作;不显示安装表单(伪造能力)。
    if let Startup::Unavailable(reason) = state.startup() {
        body = body.push(
            text(format!("应用服务不可用:{reason}"))
                .size(font::body())
                .color(color::current().red),
        );
        body = body.push(action_button(
            "重试",
            Message::RetryService,
            color::current().gold,
        ));
        body = body.push(dim("修复端口占用/同根锁冲突后点「重试」,或重开此面板。"));
        body = body.push(installed_section(state));
        return scroll_view(body);
    }

    if matches!(state.startup(), Startup::Starting) {
        body = body.push(dim("应用服务正在启动…"));
    }

    // 2) 已安装列表(空列表单独一句)。
    body = body.push(installed_section(state));

    // 2b) 运行时(探测/安装/卸载)。
    body = body.push(runtime::runtime_section(state));

    // 3) 安装流程(空闲时是来源选择表单;有流程时是当前步骤)。
    body = body.push(heading("安装"));
    body = body.push(flow_view(state));

    scroll_view(body)
}

fn scroll_view(body: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
    scrollable(body)
        .direction(scrollable::Direction::Vertical(
            byteui::interaction::scrollbar::scrollbar(),
        ))
        .style(|_t, _s| byteui::interaction::scrollbar::scrollbar_style())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// 「已安装」区块:加载中 / 失败 / 空 / 列表(稳定 id 展示,不绑定当前 Project)。
fn installed_section(state: &AppsState) -> Element<'_, Message> {
    let flow = state.flow();
    let mut col = column![heading("已安装")].spacing(6).width(Length::Fill);
    match &flow.apps {
        Load::Loading => col = col.push(dim("读取中…")),
        Load::Failed(why) => col = col.push(dim(format!("读取失败:{why}"))),
        Load::Loaded(apps) if apps.is_empty() => {
            col = col.push(dim("还没有安装任何应用"));
        }
        Load::Loaded(apps) => {
            for app in apps {
                col = col.push(actions::app_row(state, app));
            }
        }
    }
    col.into()
}

/// 来源单选行:三颗互斥按钮;当前选中那颗用金色。
fn source_choice_row(current: SourceChoice) -> Element<'static, Message> {
    let colors = color::current();
    let mk = |label: &'static str, choice: SourceChoice| -> Element<'static, Message> {
        let selected = current == choice;
        action_button(
            label,
            Message::Install(InstallMessage::SourceChoiceChanged(choice)),
            if selected { colors.gold } else { colors.dim },
        )
    };
    row![
        mk("本机目录", SourceChoice::Dir),
        mk("本机压缩包", SourceChoice::Archive),
        mk("网络 URL", SourceChoice::Url),
    ]
    .spacing(8)
    .into()
}

/// 空闲时的安装表单:来源切换 + (视来源)各色输入框与主按钮。
fn install_form(state: &AppsState) -> Element<'_, Message> {
    let colors = color::current();
    let flow = state.flow();
    let mut col = column![source_choice_row(flow.source_choice)]
        .spacing(8)
        .width(Length::Fill);
    match flow.source_choice {
        SourceChoice::Dir => {
            col = col.push(action_button(
                "安装应用…",
                Message::Install(InstallMessage::InstallClicked),
                colors.gold,
            ));
        }
        SourceChoice::Archive => {
            col = col.push(action_button(
                "安装应用…",
                Message::Install(InstallMessage::PickArchive),
                colors.gold,
            ));
        }
        SourceChoice::Url => {
            col = col.push(dim("https 压缩包地址(只允许静态应用)"));
            col = col.push(byteui::form::input_text::view(
                "https://example.com/app.zip",
                flow.url_input.as_str(),
                false,
                None,
                false,
                None,
                false,
                |s| Message::Install(InstallMessage::UrlChanged(s)),
            ));
            col = col.push(dim(
                "期望 sha256(可选;填了就钉死,不填则审批卡展示实际算出的哈希)",
            ));
            col = col.push(byteui::form::input_text::view(
                "留空 = 不钉死",
                flow.sha_input.as_str(),
                false,
                None,
                false,
                None,
                false,
                |s| Message::Install(InstallMessage::Sha256Changed(s)),
            ));
            if let Some(err) = &flow.url_error {
                col = col.push(text(err.as_str()).size(font::label()).color(colors.red));
            }
            col = col.push(action_button(
                "获取计划",
                Message::Install(InstallMessage::UrlPlanClicked),
                colors.gold,
            ));
        }
    }
    container(col).width(Length::Fill).into()
}

fn flow_view(state: &AppsState) -> Element<'_, Message> {
    let colors = color::current();
    match &state.flow().flow {
        Flow::Idle => install_form(state),
        Flow::Picking { target } => match target {
            PickTarget::Dir => {
                dim("请在弹出的对话框里选择应用目录(目录里要有 manifest.toml)…").into()
            }
            PickTarget::Archive => {
                dim("请在弹出的对话框里选择压缩包(.zip / .tar.gz / .tgz)…").into()
            }
        },
        Flow::Planning { .. } => dim("正在检查应用…").into(),
        Flow::Installing { .. } => dim("正在安装…").into(),
        // 计划生成失败:显示原因 + 关闭(回 Idle 后可重试)。
        Flow::Failed { message } => column![
            text(message.as_str()).size(font::body()).color(colors.red),
            action_button(
                "知道了",
                Message::Install(InstallMessage::FlowDismissed),
                colors.dim,
            ),
        ]
        .spacing(8)
        .into(),
        Flow::Reviewing { plan, .. } => {
            let running = false;
            review_view(plan, running)
        }
        // 运行时流程与卸载/回滚确认在各自区块呈现(运行时在 `runtime_section`;
        // 卸载/回滚确认在已装列表行的 `actions::row_actions`),这里只留一句指向性说明。
        other => dim(flow_placeholder(other).to_string()).into(),
    }
}

/// 由其他区块承载的流程分支在「安装」区块给出的指向性说明。
fn flow_placeholder(flow: &Flow) -> &'static str {
    match flow {
        Flow::RuntimePlanning { .. }
        | Flow::RuntimeReviewing { .. }
        | Flow::ConfirmRuntimeUninstall { .. } => "运行时操作在上方「运行时」区块进行。",
        Flow::ConfirmUninstall { .. } | Flow::ConfirmRollback { .. } => {
            "请在已安装列表的应用行上确认该操作。"
        }
        _ => "",
    }
}
