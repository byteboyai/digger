//! 安装审批卡的 iced 视图(Task 14;规格 §6.3 / §6.4)。
//!
//! 只画 [`bytehost_panel::install`] 里的纯展示函数(`plan_view` / `source_disclosure`),**不重算**
//! 任何信任或权限策略——产品信任决策只在 [`crate::apps`] 组合根经 [`digger_app_host::trust`] 做出。
//!
//! 计划 Task 14 第 2 条的落点:展示实际计划、来源、摘要、权限与 enforcement;本机目录来源保留
//! "整目录复制"提示(由 `plan_view` 的 `notices`/`will_run` 带出)。审批按钮只接受**当前这一份计划**
//! 的身份——按钮回调固定发 `InstallMessage::ApproveClicked`,状态机在 `Reviewing` 之外一律忽略。

use bytehost_apps::plan::InstallPlan;
use bytehost_panel::install::{Message as InstallMessage, plan_view};
use byteui::theme::{color, font};

use iced::widget::text::IntoFragment;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};

use super::Message;

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

fn fact<'a>(k: &'static str, v: String) -> Element<'a, Message> {
    row![
        dim(k),
        text(v).size(font::label()).color(color::current().cream)
    ]
    .spacing(10)
    .into()
}

/// 审批卡:把 `plan_view` 的每一行如实画出来;升级(申请比已授予更多)的权限用金色标 `↑`。
///
/// `running` = 被升级/安装的应用当前是否在运行(决定要不要提醒"会短暂中断")。
pub fn review_view<'a>(plan: &'a InstallPlan, running: bool) -> Element<'a, Message> {
    let colors = color::current();
    let view = plan_view(plan, running);

    let mut col = column![
        text(format!("安装 {}", view.title))
            .size(font::body())
            .color(colors.cream),
    ]
    .spacing(6);

    // 升级提示(A6g):自动重启、起不来会回滚。
    for notice in view.notices {
        col = col.push(text(notice).size(font::label()).color(colors.gold));
    }

    for (k, v) in view.facts {
        col = col.push(fact(k, v));
    }

    // 来源披露(A6h):事实行原样画;来自网络时先给醒目的"不可信"标注。
    col = col.push(dim("来源:"));
    if view.source.from_network {
        col = col.push(
            text("来自网络,不可信")
                .size(font::label())
                .color(colors.gold),
        );
        col = col.push(dim("只允许静态应用,运行在严格 CSP 下"));
    }
    for (k, v) in view.source.rows {
        col = col.push(fact(k, v));
    }
    for warning in view.source.warnings {
        col = col.push(text(warning).size(font::label()).color(colors.gold));
    }

    // 安装与运行时会执行什么(原样展示;本机目录来源的"整目录复制"在这里)。
    col = col.push(dim("安装与运行时会执行:"));
    for line in view.will_run {
        col = col.push(text(line).size(font::label()).color(colors.cream));
    }

    // 权限与 enforcement。
    col = col.push(dim("申请的权限:"));
    if view.permissions.is_empty() {
        col = col.push(
            text("不申请任何权限")
                .size(font::label())
                .color(colors.cream),
        );
    }
    for p in view.permissions {
        col = col.push(
            row![
                text(if p.escalation { "↑" } else { " " })
                    .size(font::label())
                    .color(colors.gold),
                text(p.label).size(font::label()).color(colors.cream),
                text(p.change).size(font::label()).color(colors.cream),
                Space::new().width(Length::Fill),
                dim(p.enforcement),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }

    // 人类确认:批准只针对**当前这一份计划**(状态机身份绑定);取消回到 Idle。
    col = col.push(
        row![
            action_button(
                "取消",
                Message::Install(InstallMessage::FlowDismissed),
                colors.dim
            ),
            action_button(
                "批准并安装",
                Message::Install(InstallMessage::ApproveClicked),
                colors.gold,
            ),
        ]
        .spacing(8),
    );

    container(col).padding(10).width(Length::Fill).into()
}
