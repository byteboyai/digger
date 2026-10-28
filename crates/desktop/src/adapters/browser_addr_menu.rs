//! 浏览器地址栏的右键菜单(G1 的整框语义,同 [`conversations_search_menu`](super::conversations_search_menu))。
//!
//! iced 0.14 的输入框操作读不到「选中的是哪段文字」,所以剪切 / 复制作用于整个地址栏文本,粘贴追加到末尾,
//! 全选是精确的。⌘C/V/X/A 快捷键仍是 iced 原生的精确选区语义,不受影响。
//!
//! 地址栏文本在库的 `State` 里:聚焦(编辑)时是草稿 `addr_text()`,没聚焦时显示的是当前页签网址。
//! 菜单动作作用于**屏幕上看到的那一份**,所以由 [`BrowserPanel`](super::browser::BrowserPanel) 先算出显示文本再来这里。

/// 菜单动作。`Pasted` 是 `Paste` 读完剪贴板后的回流(剪贴板读取是异步 Task)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrEdit {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Pasted(Option<String>),
}

/// 粘贴后的地址栏文本。地址栏是单行的:换行折成空格。
///
/// `replace_all`:粘贴时地址栏**没有聚焦**,等价于「先聚焦(聚焦会全选)再粘贴」,所以整条替换;
/// 已经在编辑时追加到末尾(与 G1 整框语义一致)。剪贴板没有可见内容时原样返回当前文本。
pub fn pasted_addr(displayed: &str, clipboard: &str, replace_all: bool) -> String {
    let joined = clipboard
        .split(['\r', '\n'])
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return displayed.to_owned();
    }
    if replace_all {
        joined
    } else {
        format!("{displayed}{joined}")
    }
}

/// 右键弹出的原生菜单项:剪切、复制、粘贴、全选。地址栏显示文本为空时剪切 / 复制置灰。
#[cfg(target_os = "macos")]
pub fn menu_items(
    pid: i64,
    displayed_is_empty: bool,
) -> Vec<byteui::native_menu::Item<crate::product::Msg>> {
    use byteui::interaction::icons::IconKind;
    use byteui::native_menu::Item;
    let colors = byteui::theme::color::current();
    let entry = |icon, label: &str, enabled: bool, edit| Item::Entry {
        icon: Some(icon),
        icon_color: None,
        label: label.into(),
        color: if enabled { colors.body } else { colors.dim },
        enabled,
        msg: crate::product::Msg::BrowserAddrEdit(pid, edit),
    };
    vec![
        entry(
            IconKind::Scissors,
            "剪切",
            !displayed_is_empty,
            AddrEdit::Cut,
        ),
        entry(IconKind::Copy, "复制", !displayed_is_empty, AddrEdit::Copy),
        entry(IconKind::ClipboardPaste, "粘贴", true, AddrEdit::Paste),
        entry(IconKind::SelectAll, "全选", true, AddrEdit::SelectAll),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use byteui::native_menu::Item;

    #[test]
    fn pasted_text_joins_lines_and_replaces_or_appends() {
        assert_eq!(pasted_addr("https://a.com/", "x\r\ny\n\nz", true), "x y z");
        assert_eq!(pasted_addr("https://a.com/", "b", false), "https://a.com/b");
        assert_eq!(
            pasted_addr("https://a.com/", "\n", true),
            "https://a.com/",
            "blank clipboard changes nothing, even when replacing"
        );
        assert_eq!(pasted_addr("", "a b", false), "a b");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn menu_has_four_entries_and_locks_cut_copy_on_an_empty_address() {
        for (empty, cut_copy_enabled) in [(true, false), (false, true)] {
            let items = menu_items(9, empty);
            assert_eq!(items.len(), 4);
            let expected = [
                AddrEdit::Cut,
                AddrEdit::Copy,
                AddrEdit::Paste,
                AddrEdit::SelectAll,
            ];
            for (i, item) in items.iter().enumerate() {
                let Item::Entry { enabled, msg, .. } = item else {
                    panic!("entry")
                };
                assert!(
                    matches!(msg, crate::product::Msg::BrowserAddrEdit(9, e) if *e == expected[i]),
                    "entry {i} carries the wrong message"
                );
                let locked_by_empty = i < 2;
                assert_eq!(*enabled, !locked_by_empty || cut_copy_enabled, "entry {i}");
            }
        }
    }
}
