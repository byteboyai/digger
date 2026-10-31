//! 库菜单 → macOS 原生菜单(`byteui::native_menu`)的转换。
//!
//! 原生 NSMenu 是真窗口层级,天然浮在 wry 原生页面之上,所以菜单弹出时不需要把文件内容
//! 藏起来。iced 自绘的浮层盖不住原生页面,才需要隐藏页面。

#![cfg(target_os = "macos")]

use byteui::native_menu::Item;
use dozer_panel_files::menu::MenuItem;

/// 在指定 iced 窗口上弹原生菜单，选中项作为 `Task` 投回产品 `update`。
///
/// `window::run` 把闭包调度到主线程并借出真实窗口句柄，菜单只在该闭包里同步
/// 存活；窗口已关闭/不存在时返回 `None`，不会挂到其它窗口。`window = None`
/// (窗口还没就绪)时什么也不做，迟到的选择不会串到别的窗口。
pub fn show_task<Msg: Clone + Send + 'static>(
    window: Option<iced::window::Id>,
    items: Vec<Item<Msg>>,
    at: (f32, f32),
    align_no_icon_left: bool,
) -> iced::Task<Msg> {
    let Some(window) = window else {
        return iced::Task::none();
    };
    iced::window::run(window, move |handle| {
        if align_no_icon_left {
            byteui::native_menu::show_align_no_icon_left_for_window(handle, items, at)
        } else {
            byteui::native_menu::show_for_window(handle, items, at)
        }
    })
    .then(|selection| match selection {
        Some(message) => iced::Task::done(message),
        None => iced::Task::none(),
    })
}

/// 文件库的菜单项 → 原生菜单项。顺序、`enabled`、`msg` 原样;文字色取主题 `body`
/// (与 Dozer `app/update.rs::run_files_requests` 的转换一致)。
pub fn to_native_items<Msg>(items: Vec<MenuItem<Msg>>) -> Vec<Item<Msg>> {
    let body = byteui::theme::color::current().body;
    items
        .into_iter()
        .map(|item| match item {
            MenuItem::Entry {
                icon,
                label,
                enabled,
                msg,
            } => Item::Entry {
                icon,
                icon_color: None,
                label,
                color: body,
                enabled,
                msg,
            },
            MenuItem::Separator => Item::Separator,
        })
        .collect()
}

/// 页签溢出下拉的原生菜单版本——纯选择列表,与 Dozer `tab_widget::tab_overflow_items` 同口径。
/// `entries` 是 `(下标, 标题, 是否选中)`。选中行用 CREAM 文字加标题前的 `>` 图标(CREAM),
/// 其余行 BODY 且不画 `>`;原生渲染端为本菜单无条件预留固定图标列,不画箭头的行文字也不错位。
/// 原生 NSMenu 是整行单击模型,不含关闭按钮 / 状态点 / 悬停。
pub fn tab_overflow_items<Msg: Clone>(
    entries: &[(usize, String, bool)],
    on_select: impl Fn(usize) -> Msg,
) -> Vec<Item<Msg>> {
    let cream = byteui::theme::color::current().cream;
    let body = byteui::theme::color::current().body;
    entries
        .iter()
        .map(|(idx, title, active)| {
            let (icon, icon_color) = if *active {
                (
                    Some(byteui::interaction::icons::IconKind::ChevronRight),
                    Some(cream),
                )
            } else {
                (None, None)
            };
            Item::Entry {
                icon,
                icon_color,
                label: title.clone(),
                color: if *active { cream } else { body },
                enabled: true,
                msg: on_select(*idx),
            }
        })
        .collect()
}

/// Conversations retain their library filter state; a cancelled native menu emits nothing.
pub fn conversation_filter_items(
    pid: i64,
    current: Option<dozer_model::AgentKind>,
    present: &[dozer_model::AgentKind],
) -> Vec<Item<crate::product::Msg>> {
    use crate::product::Msg;
    use dozer_panel_conversations::Message;
    let colors = byteui::theme::color::current();
    std::iter::once(None)
        .chain(present.iter().copied().map(Some))
        .map(|agent| {
            let selected = agent == current;
            Item::Entry {
                icon: agent.map(dozer_panel_agent::style::agent_icon),
                icon_color: agent.map(dozer_panel_agent::style::agent_dot_color),
                label: agent.map_or_else(|| "全部".into(), |a| a.label().into()),
                color: if selected { colors.gold } else { colors.body },
                enabled: !selected,
                msg: Msg::Conversations(pid, Message::AgentFilterSelect(agent)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteui::interaction::icons::IconKind;

    #[test]
    fn to_native_items_preserves_order_and_fields() {
        let native = to_native_items(vec![
            MenuItem::Entry {
                icon: Some(IconKind::CircleX),
                label: "Close".into(),
                enabled: true,
                msg: 1u8,
            },
            MenuItem::Separator,
            MenuItem::Entry {
                icon: None,
                label: "Reload".into(),
                enabled: false,
                msg: 2u8,
            },
        ]);
        assert_eq!(native.len(), 3);
        let body = byteui::theme::color::current().body;
        match &native[0] {
            Item::Entry {
                icon,
                icon_color,
                label,
                color,
                enabled,
                msg,
            } => {
                assert_eq!(*icon, Some(IconKind::CircleX));
                assert!(icon_color.is_none());
                assert_eq!(label, "Close");
                assert_eq!(*color, body);
                assert!(*enabled);
                assert_eq!(*msg, 1);
            }
            Item::Separator => panic!("第 0 项应是条目"),
        }
        assert!(matches!(native[1], Item::Separator));
        match &native[2] {
            Item::Entry {
                label,
                enabled,
                msg,
                ..
            } => {
                assert_eq!(label, "Reload");
                assert!(!*enabled, "禁用状态要保留");
                assert_eq!(*msg, 2);
            }
            Item::Separator => panic!("第 2 项应是条目"),
        }
    }

    #[test]
    fn an_empty_menu_converts_to_an_empty_list() {
        assert!(to_native_items::<u8>(Vec::new()).is_empty());
    }

    #[test]
    fn tab_overflow_items_marks_only_the_active_row_with_a_chevron() {
        let items = tab_overflow_items(
            &[
                (0, "a.rs".to_string(), false),
                (1, "b.rs".to_string(), true),
                (2, "c.rs".to_string(), false),
            ],
            |i| i,
        );
        let chevrons: Vec<bool> = items
            .iter()
            .map(|item| match item {
                Item::Entry { icon, .. } => *icon == Some(IconKind::ChevronRight),
                Item::Separator => false,
            })
            .collect();
        assert_eq!(chevrons, vec![false, true, false]);
    }

    #[test]
    fn tab_overflow_items_dispatch_the_row_index_not_the_position() {
        // 页签关过之后下标与位置会错开:消息必须带条目自己的下标。
        let items = tab_overflow_items(
            &[(3, "x".to_string(), false), (7, "y".to_string(), true)],
            |i| i * 10,
        );
        let msgs: Vec<usize> = items
            .into_iter()
            .map(|item| match item {
                Item::Entry { msg, .. } => msg,
                Item::Separator => usize::MAX,
            })
            .collect();
        assert_eq!(msgs, vec![30, 70]);
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::*;
    use crate::product::Msg;
    use dozer_model::AgentKind;
    use dozer_panel_conversations::Message;

    #[test]
    fn filter_menu_lists_all_plus_present_agents_and_locks_the_current_one() {
        let items = conversation_filter_items(
            17,
            Some(AgentKind::Claude),
            &[AgentKind::Claude, AgentKind::Codex],
        );
        assert_eq!(items.len(), 3);
        let colors = byteui::theme::color::current();
        for (i, item) in items.iter().enumerate() {
            let Item::Entry {
                icon,
                icon_color,
                label,
                color,
                enabled,
                msg,
            } = item
            else {
                panic!("entry")
            };
            let expected = [None, Some(AgentKind::Claude), Some(AgentKind::Codex)][i];
            assert_eq!(*enabled, i != 1);
            assert_eq!(*color, if i == 1 { colors.gold } else { colors.body });
            assert!(
                matches!(msg, Msg::Conversations(17, Message::AgentFilterSelect(agent)) if *agent == expected)
            );
            if let Some(agent) = expected {
                assert_eq!(label, agent.label());
                assert_eq!(*icon, Some(dozer_panel_agent::style::agent_icon(agent)));
                assert_eq!(
                    *icon_color,
                    Some(dozer_panel_agent::style::agent_dot_color(agent))
                );
            } else {
                assert_eq!(label, "全部");
                assert!(icon.is_none());
            }
        }
        let all = conversation_filter_items(17, None, &[]);
        assert!(matches!(&all[0], Item::Entry { enabled: false, .. }));
    }
}
