//! 原生窗口 Settings:主窗保持现有外观,设置窗是普通独立窗口(失焦不关闭)。

use iced::{Size, window};

/// 主窗口:透明标题栏 + 全尺寸内容视图(顶栏取代系统标题栏),关闭请求交给产品处理。
pub fn main_window(size: Size) -> window::Settings {
    window::Settings {
        size,
        exit_on_close_request: false,
        min_size: Some(Size::new(
            byteui::theme::geometry::min_window_width(),
            byteui::theme::geometry::min_window_height(),
        )),
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..Default::default()
    }
}

/// 设置窗口:独立普通窗口,居中;小窗口下仍可滚动,关闭按钮始终可达。
///
/// `exit_on_close_request: false` 让系统关闭 / Esc / 关闭按钮都走同一生命周期。
pub fn settings_window() -> window::Settings {
    window::Settings {
        size: Size::new(720.0, 640.0),
        min_size: Some(Size::new(520.0, 420.0)),
        resizable: true,
        exit_on_close_request: false,
        position: window::Position::Centered,
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: false,
            ..Default::default()
        },
        ..Default::default()
    }
}
