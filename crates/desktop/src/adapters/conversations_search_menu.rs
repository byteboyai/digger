//! 会话搜索框的右键菜单(收尾计划 R5 / G1)。
//!
//! Dozer 的做法是弹原生菜单后合成 ⌘X/C/V/A 键盘事件回灌给输入框;Digger 是标准 iced 应用,
//! 没有那套自定义事件循环,而 iced 0.14 的输入框操作读不到「选中的是哪段文字」。所以这里的
//! 语义是**整框**的(用户已选定):剪切/复制作用于整个搜索词,粘贴追加到末尾,全选是精确的。
//! ⌘C/V/X/A 快捷键仍是 iced 原生的精确选区语义,不受影响。
//!
//! 搜索词存在库的 `WorkspaceState.search_draft`(crate 私有),宿主读不到,所以由
//! [`ConversationsPanel`](super::conversations::ConversationsPanel) 在 `SearchInput` 经过时缓存一份。

/// 菜单动作。`Pasted` 是 `Paste` 读完剪贴板后的回流(剪贴板读取是异步 Task)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEdit {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Pasted(Option<String>),
}

/// 把剪贴板文本追加到搜索词末尾。搜索框是单行的:换行折成空格,多个换行不产生空段;
/// 剪贴板里没有可见内容时原样返回。
pub fn pasted_draft(draft: &str, clipboard: &str) -> String {
    let joined = clipboard
        .split(['\r', '\n'])
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{draft}{joined}")
}

/// 右键弹出的原生菜单项:剪切、复制、粘贴、全选。搜索词为空时剪切/复制置灰。
#[cfg(target_os = "macos")]
pub fn menu_items(
    pid: i64,
    draft_is_empty: bool,
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
        msg: crate::product::Msg::ConversationsSearchEdit(pid, edit),
    };
    vec![
        entry(IconKind::Scissors, "剪切", !draft_is_empty, SearchEdit::Cut),
        entry(IconKind::Copy, "复制", !draft_is_empty, SearchEdit::Copy),
        entry(IconKind::ClipboardPaste, "粘贴", true, SearchEdit::Paste),
        entry(IconKind::SelectAll, "全选", true, SearchEdit::SelectAll),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use byteui::native_menu::Item;

    #[test]
    fn pasted_text_joins_lines_and_appends_to_the_draft() {
        assert_eq!(pasted_draft("ab", "cd"), "abcd");
        assert_eq!(pasted_draft("", "x\r\ny\n\nz"), "x y z");
        assert_eq!(
            pasted_draft("ab", "\n"),
            "ab",
            "blank clipboard changes nothing"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn menu_has_four_entries_and_locks_cut_copy_on_an_empty_draft() {
        for (empty, cut_copy_enabled) in [(true, false), (false, true)] {
            let items = menu_items(9, empty);
            assert_eq!(items.len(), 4);
            let expected = [
                SearchEdit::Cut,
                SearchEdit::Copy,
                SearchEdit::Paste,
                SearchEdit::SelectAll,
            ];
            for (i, item) in items.iter().enumerate() {
                let Item::Entry { enabled, msg, .. } = item else {
                    panic!("entry")
                };
                assert!(
                    matches!(msg, crate::product::Msg::ConversationsSearchEdit(9, e) if *e == expected[i]),
                    "entry {i} carries the wrong message"
                );
                let locked_by_empty = i < 2;
                assert_eq!(*enabled, !locked_by_empty || cut_copy_enabled, "entry {i}");
            }
        }
    }
}
