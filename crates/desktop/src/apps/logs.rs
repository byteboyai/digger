//! 应用日志查看器的 iced 渲染(Task 15;规格 §6.2「日志」)。
//!
//! 复用共享的 [`bytehost_panel::logs`] 状态机:展开/收起、滚动跟随、刷新失败时保留旧内容并标注
//! `stale`、首次失败给原因。**本模块只画 [`LogsView`]**,不维护任何自己的日志缓冲——占用、
//! 迟到回包丢弃、跟随判定都在 [`bytehost_panel::logs::LogsState`] 里。
//!
//! `text` 用 `Shaping::Advanced` 做字体回退(日志里可能有非 ASCII),日志正文用等宽代码字体。

use bytehost_panel::logs::LogsView;
use byteui::theme::{color, font};

use iced::widget::{container, scrollable, text};
use iced::{Element, Length};

use super::{Message, log_scroll_id};

/// 一个应用的日志查看器区块(收起时为 `Hidden`,渲染空)。
pub fn log_viewer<'a>(view: LogsView, app_id: &str) -> Element<'a, Message> {
    let colors = color::current();
    let mut col = iced::widget::column![].spacing(4).width(Length::Fill);
    match view {
        LogsView::Hidden => {}
        LogsView::Loading => {
            col = col.push(text("读取日志…").size(font::label()).color(colors.dim))
        }
        LogsView::Failed(reason) => {
            col = col.push(text(reason).size(font::body()).color(colors.red))
        }
        LogsView::Loaded {
            text: log_text,
            truncated,
            stale,
        } => {
            if truncated {
                col = col.push(
                    text("仅显示末尾若干行")
                        .size(font::label())
                        .color(colors.dim),
                );
            }
            if stale {
                col = col.push(
                    text("刷新失败,显示的是上一次的内容")
                        .size(font::label())
                        .color(colors.orange),
                );
            }
            let scrolled_id = app_id.to_owned();
            let body = text(log_text)
                .size(font::body())
                .shaping(iced_core::text::Shaping::Advanced)
                .width(Length::Fill);
            col = col.push(
                container(
                    scrollable(body)
                        .id(log_scroll_id(app_id))
                        .direction(scrollable::Direction::Vertical(
                            byteui::interaction::scrollbar::scrollbar(),
                        ))
                        .style(|_t, _s| byteui::interaction::scrollbar::scrollbar_style())
                        .width(Length::Fill)
                        .on_scroll(move |viewport| {
                            Message::Install(bytehost_panel::install::Message::LogsScrolled(
                                scrolled_id.clone(),
                                at_bottom(&viewport),
                            ))
                        }),
                )
                .height(Length::Fixed(200.0))
                .width(Length::Fill)
                .padding(6),
            );
        }
    }
    container(col).width(Length::Fill).padding(6).into()
}

/// 视口是否贴底(留 4px 容差)——贴底时状态机才把"跟随"打开。
fn at_bottom(viewport: &scrollable::Viewport) -> bool {
    let abs = viewport.absolute_offset();
    let bottom = abs.y + viewport.bounds().height;
    bottom >= viewport.content_bounds().height - 4.0
}
