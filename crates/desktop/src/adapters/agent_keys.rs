//! iced 键位到共享终端输入表的薄适配。
//!
//! 对照 Dozer `platform/window_events.rs`: Command 组合键先由应用快捷键
//! 处理，绝不能写入 PTY；其余按键仅映射 `dozer-panel-term` 的旧表。

use dozer_panel_term::input::{TermKey, TermMods, TermNamedKey, key_to_bytes};
use iced::keyboard::{Key, Modifiers, key::Named};

pub fn to_term_key(key: &Key) -> Option<TermKey> {
    match key {
        Key::Character(text) => Some(TermKey::Char(text.to_string())),
        Key::Named(named) => Some(TermKey::Named(match named {
            Named::Space => TermNamedKey::Space,
            Named::Enter => TermNamedKey::Enter,
            Named::Backspace => TermNamedKey::Backspace,
            Named::Tab => TermNamedKey::Tab,
            Named::Escape => TermNamedKey::Escape,
            Named::ArrowUp => TermNamedKey::ArrowUp,
            Named::ArrowDown => TermNamedKey::ArrowDown,
            Named::ArrowRight => TermNamedKey::ArrowRight,
            Named::ArrowLeft => TermNamedKey::ArrowLeft,
            _ => return None,
        })),
        _ => None,
    }
}

pub fn to_term_mods(modifiers: Modifiers) -> TermMods {
    TermMods {
        ctrl: modifiers.control(),
        alt: modifiers.alt(),
        shift: modifiers.shift(),
        super_key: modifiers.logo(),
    }
}

pub fn key_bytes(key: &Key, modifiers: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    (!modifiers.logo())
        .then(|| key_to_bytes(&to_term_key(key)?, to_term_mods(modifiers), app_cursor))?
}

/// iced 键盘事件 → PTY 字节。只处理 `KeyPressed`:字符键用 `modified_key`
/// (带 Shift 的结果),具名键用 `key`;⌘ 组合键一律不进 PTY(同 [`key_bytes`])。
pub fn event_bytes(event: &iced::keyboard::Event, app_cursor: bool) -> Option<Vec<u8>> {
    let iced::keyboard::Event::KeyPressed {
        key,
        modified_key,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    let key = match key {
        Key::Character(_) => modified_key,
        _ => key,
    };
    key_bytes(key, *modifiers, app_cursor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppShortcut {
    Copy,
    Paste,
}

pub fn shortcut(key: &Key, modifiers: Modifiers) -> Option<AppShortcut> {
    if !modifiers.logo() || modifiers.shift() || modifiers.control() || modifiers.alt() {
        return None;
    }
    let Key::Character(text) = key else {
        return None;
    };
    match text.as_str() {
        "c" | "C" => Some(AppShortcut::Copy),
        "v" | "V" => Some(AppShortcut::Paste),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(key: Named) -> Key {
        Key::Named(key)
    }
    fn character(key: &str) -> Key {
        Key::Character(key.into())
    }

    #[test]
    fn shifted_characters_use_the_modified_key() {
        // 值来源:iced 的 `KeyPressed` 里 `modified_key` 是应用了修饰键后的结果;
        // 字符键必须用它,否则 Shift+a 会变成小写 a、Shift+1 变成 1。
        let press = |key: &str, modified: &str, modifiers| iced::keyboard::Event::KeyPressed {
            key: Key::Character(key.into()),
            modified_key: Key::Character(modified.into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: Some(modified.into()),
            repeat: false,
        };
        assert_eq!(
            event_bytes(&press("a", "A", Modifiers::SHIFT), false),
            Some(b"A".to_vec())
        );
        assert_eq!(
            event_bytes(&press("1", "!", Modifiers::SHIFT), false),
            Some(b"!".to_vec())
        );
        // 具名键仍用 `key`(方向键不受 modified_key 影响)。
        let arrow = iced::keyboard::Event::KeyPressed {
            key: named(Named::ArrowUp),
            modified_key: named(Named::ArrowUp),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::default(),
            text: None,
            repeat: false,
        };
        assert_eq!(event_bytes(&arrow, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(event_bytes(&arrow, true), Some(b"\x1bOA".to_vec()));
        // ⌘ 恒不进 PTY;KeyReleased 不产生字节。
        assert_eq!(event_bytes(&press("a", "a", Modifiers::LOGO), false), None);
        if let iced::keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
            ..
        } = press("a", "a", Modifiers::default())
        {
            let released = iced::keyboard::Event::KeyReleased {
                key,
                modified_key,
                physical_key,
                location,
                modifiers,
            };
            assert_eq!(event_bytes(&released, false), None);
        }
    }

    #[test]
    fn byte_matrix_matches_dozers_known_outputs() {
        // 值逐项抄自 dozer-panel-term/src/input.rs 的 key_to_bytes 测试。
        let cases = [
            (
                character("a"),
                Modifiers::default(),
                false,
                Some(b"a".to_vec()),
            ),
            (character("A"), Modifiers::SHIFT, false, Some(b"A".to_vec())),
            (
                named(Named::Space),
                Modifiers::default(),
                false,
                Some(vec![b' ']),
            ),
            (
                named(Named::Enter),
                Modifiers::default(),
                false,
                Some(vec![b'\r']),
            ),
            (
                named(Named::Backspace),
                Modifiers::default(),
                false,
                Some(vec![0x7f]),
            ),
            (
                named(Named::Tab),
                Modifiers::default(),
                false,
                Some(vec![b'\t']),
            ),
            (
                named(Named::Escape),
                Modifiers::default(),
                false,
                Some(vec![0x1b]),
            ),
            (
                named(Named::ArrowUp),
                Modifiers::default(),
                false,
                Some(b"\x1b[A".to_vec()),
            ),
            (
                named(Named::ArrowUp),
                Modifiers::default(),
                true,
                Some(b"\x1bOA".to_vec()),
            ),
        ];
        for (key, mods, app_cursor, expected) in cases {
            assert_eq!(key_bytes(&key, mods, app_cursor), expected);
        }
        for (letter, code) in ('a'..='z').zip(1_u8..=26) {
            assert_eq!(
                key_bytes(&character(&letter.to_string()), Modifiers::CTRL, false),
                Some(vec![code])
            );
        }
        assert_eq!(
            key_bytes(&character("c"), Modifiers::CTRL, false),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(&character("x"), Modifiers::ALT, false),
            Some(b"x".to_vec())
        );
    }

    #[test]
    fn command_combinations_never_reach_the_pty() {
        for key in [character("x"), named(Named::Enter), named(Named::ArrowLeft)] {
            assert_eq!(key_bytes(&key, Modifiers::LOGO, false), None);
        }
    }

    #[test]
    fn keys_outside_the_legacy_table_produce_nothing() {
        // 新增支持须先扩展 dozer-panel-term，保证 Dozer/Digger 一致。
        for key in [
            Named::Home,
            Named::End,
            Named::PageUp,
            Named::PageDown,
            Named::Delete,
            Named::Insert,
            Named::F1,
            Named::F12,
        ] {
            assert_eq!(key_bytes(&named(key), Modifiers::default(), false), None);
        }
    }

    #[test]
    fn modifier_only_and_dead_keys_produce_nothing() {
        // iced 0.14 的 `Key` 未暴露 dead-key 专用变体；其他修饰键仍不可输入。
        for key in [
            Named::Shift,
            Named::Control,
            Named::Alt,
            Named::Super,
            Named::CapsLock,
        ] {
            assert_eq!(key_bytes(&named(key), Modifiers::default(), false), None);
        }
        assert_eq!(
            key_bytes(&Key::Unidentified, Modifiers::default(), false),
            None
        );
    }

    #[test]
    fn every_term_named_key_is_reachable_from_an_iced_named_key() {
        let mapped = [
            Named::Space,
            Named::Enter,
            Named::Backspace,
            Named::Tab,
            Named::Escape,
            Named::ArrowUp,
            Named::ArrowDown,
            Named::ArrowRight,
            Named::ArrowLeft,
        ];
        for term in [
            TermNamedKey::Space,
            TermNamedKey::Enter,
            TermNamedKey::Backspace,
            TermNamedKey::Tab,
            TermNamedKey::Escape,
            TermNamedKey::ArrowUp,
            TermNamedKey::ArrowDown,
            TermNamedKey::ArrowRight,
            TermNamedKey::ArrowLeft,
        ] {
            assert!(
                mapped
                    .into_iter()
                    .any(|key| to_term_key(&named(key)) == Some(TermKey::Named(term)))
            );
        }
    }

    #[test]
    fn shortcut_recognizes_command_c_and_v_only() {
        assert_eq!(
            shortcut(&character("C"), Modifiers::LOGO),
            Some(AppShortcut::Copy)
        );
        assert_eq!(
            shortcut(&character("v"), Modifiers::LOGO),
            Some(AppShortcut::Paste)
        );
        assert_eq!(
            shortcut(&character("c"), Modifiers::LOGO | Modifiers::SHIFT),
            None
        );
    }

    #[test]
    fn multi_char_character_keys_are_passed_through_unchanged() {
        assert_eq!(
            to_term_key(&character("你好")),
            Some(TermKey::Char("你好".into()))
        );
    }
}
