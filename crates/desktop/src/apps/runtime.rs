//! 「运行时」区块的 iced 视图(Task 15;规格 §6.2 运行时探测/安装/卸载)。
//!
//! 只画 [`bytehost_panel::install`] 里的纯展示函数(`runtime_line` / `progress_text` /
//! `plan_lines` / `managed_runtime_of`),**不重算**任何探测或计划内容——运行时探测与安装计划的
//! 真相全在字节宿主侧。
//!
//! 计划 Task 15 第 1 条的落点:展示每个运行时的探测状态、受管版本、安装/卸载按钮与进行中的进度;
//! 运行时安装计划经审批卡由用户确认;卸载运行时走行内二次确认。探针结果里带 `job.failed` 的
//! **失败原因**在行内如实显示(不弹 Notice),受管运行时在 `probes` 加载失败时显示**持续问题页**
//! (可重试),区别于一次性的 Toast。

use bytehost_apps::proto::{ManagedRuntime, RuntimeProbe};
use bytehost_panel::install::{
    Flow, Load, Message as InstallMessage, managed_runtime_label, managed_runtime_of, plan_lines,
    probe_is_installing, progress_text, runtime_line,
};
use byteui::theme::{color, font};

use iced::widget::text::IntoFragment;
use iced::widget::{button, column, container, row, text};
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

/// 「运行时」区块:探测行 + 运行时计划审批/卸载确认(处于这些步骤时覆盖探测行)。
pub fn runtime_section(state: &super::AppsState) -> Element<'_, Message> {
    let colors = color::current();
    let flow = state.flow();
    let mut col = column![text("运行时").size(font::subtitle()).color(colors.cream)]
        .spacing(6)
        .width(Length::Fill);

    // 运行时流程处于审批/确认/规划时,优先展示该步骤(与安装流程互斥,同一台状态机)。
    match &flow.flow {
        Flow::RuntimePlanning { runtime } => {
            col = col.push(dim(format!(
                "正在生成 {} 的安装计划…",
                managed_runtime_label(*runtime)
            )));
            return col.into();
        }
        Flow::RuntimeReviewing { plan } => {
            col = col.push(runtime_review_view(&plan_lines(plan), plan.will_do.clone()));
            return col.into();
        }
        Flow::ConfirmRuntimeUninstall { runtime, version } => {
            col = col.push(dim(format!(
                "卸载 {} {version}?受管运行时被卸载后,依赖它的应用会起不来。",
                managed_runtime_label(*runtime)
            )));
            col = col.push(
                row![
                    action_button(
                        "取消",
                        Message::Install(InstallMessage::FlowDismissed),
                        colors.dim,
                    ),
                    action_button(
                        "卸载",
                        Message::Install(InstallMessage::RuntimeUninstallConfirmed),
                        colors.red,
                    ),
                ]
                .spacing(8),
            );
            return col.into();
        }
        _ => {}
    }

    // 探测结果:加载中 / 失败(持续问题页,可重试) / 行。
    match &flow.probes {
        Load::Loading => col = col.push(dim("读取运行时…")),
        Load::Failed(why) => {
            col = col.push(
                text(format!("运行时探测失败:{why}"))
                    .size(font::body())
                    .color(colors.red),
            );
            col = col.push(dim("重试会再探测一次。"));
            col = col.push(action_button(
                "重试",
                Message::Install(InstallMessage::PollProbes),
                colors.gold,
            ));
        }
        Load::Loaded(probes) => {
            if probes.is_empty() {
                col = col.push(dim("没有探测到任何运行时。"));
            }
            for probe in probes {
                col = col.push(probe_row(probe));
            }
        }
    }
    col.into()
}

/// 一行运行时:名称 + 探测状态;有安装/卸载入口时给按钮;进行中时给进度。
fn probe_row<'a>(probe: &'a RuntimeProbe) -> Element<'a, Message> {
    let colors = color::current();
    let (_key, status, _available) = runtime_line(probe);
    let name = probe_name(probe);
    let mut col = column![
        row![
            text(name).size(font::body()).color(colors.cream),
            dim(status),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(4)
    .width(Length::Fill);

    // 进行中的安装任务:进度 + 失败原因(行内,不弹 Notice)。
    if let Some(job) = &probe.job {
        if !job.finished {
            col = col.push(dim(progress_text(job)));
        }
        if let Some(why) = &job.failed {
            col = col.push(
                text(format!("安装失败:{why}"))
                    .size(font::label())
                    .color(colors.red),
            );
        }
    }

    // 受管版本行 + 卸载按钮(仅受管运行时;版本新→旧)。
    for version in &probe.managed {
        col = col.push(
            row![
                dim(format!("受管 {version}")),
                action_button(
                    "卸载",
                    Message::Install(InstallMessage::RuntimeUninstallClicked(
                        runtime_of(probe).unwrap_or(ManagedRuntime::Node),
                        version.clone(),
                    )),
                    colors.red,
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }

    // 安装入口:仅"此平台可自动安装"且当前没有未完成任务时给出(避免重复安装)。
    if probe.installable
        && let Some(runtime) = runtime_of(probe)
    {
        let busy = probe_is_installing(probe);
        let label = format!("安装 {}", managed_runtime_label(runtime));
        if busy {
            col = col.push(dim("安装中…"));
        } else {
            col = col.push(action_button(
                label,
                Message::Install(InstallMessage::RuntimeInstallClicked(runtime)),
                colors.gold,
            ));
        }
    }

    // 未受管但仍不可用的运行时(docker):不误报"可安装",只说明现状。
    if !probe.installable && runtime_of(probe).is_none() {
        col = col.push(dim("此平台不支持由 Digger 自动安装。"));
    }

    container(col).padding([4, 0]).width(Length::Fill).into()
}

/// 运行时行名(探测键 → 中文名;非受管用原始键)。
fn probe_name(probe: &RuntimeProbe) -> String {
    let (name, _, _) = runtime_line(probe);
    name
}

/// 探测键 → 受管运行时(仅 node/python)。
fn runtime_of(probe: &RuntimeProbe) -> Option<ManagedRuntime> {
    managed_runtime_of(probe.runtime.as_str())
}

/// 运行时安装计划审批卡:`(标签, 值)` 事实行 + 将要做的事 + 批准/取消。
fn runtime_review_view<'a>(
    lines: &[(String, String)],
    will_do: Vec<String>,
) -> Element<'a, Message> {
    let colors = color::current();
    let mut col = column![text("安装运行时").size(font::body()).color(colors.cream)].spacing(6);

    for (k, v) in lines {
        col = col.push(
            row![
                dim(k.clone()),
                text(v.clone()).size(font::label()).color(colors.cream),
            ]
            .spacing(10),
        );
    }
    if !will_do.is_empty() {
        col = col.push(dim("安装与运行时会执行:"));
        for line in will_do {
            col = col.push(text(line).size(font::label()).color(colors.cream));
        }
    }
    col = col.push(
        row![
            action_button(
                "取消",
                Message::Install(InstallMessage::FlowDismissed),
                colors.dim,
            ),
            action_button(
                "批准并安装",
                Message::Install(InstallMessage::RuntimeApproveClicked),
                colors.gold,
            ),
        ]
        .spacing(8),
    );
    container(col).padding(10).width(Length::Fill).into()
}
