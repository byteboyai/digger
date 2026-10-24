//! 库菜单 → macOS 原生菜单(`byteui::native_menu`)的转换。
//!
//! 原生 NSMenu 是真窗口层级,天然浮在 wry 原生页面之上,所以菜单弹出时不需要把文件内容
//! 藏起来。iced 自绘的浮层盖不住原生页面,才需要隐藏页面。

#![cfg(target_os = "macos")]

use byteui::native_menu::Item;
use dozer_panel_files::menu::MenuItem;

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
