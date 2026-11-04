//! 原生窗口 Settings:主窗保持现有外观;设置窗是覆盖父客户区的透明子窗口。
//!
//! 窗口本身由 iced 创建(单一事件循环),再用 `bytehost_desktop::dialog` 的
//! 公共 `window::run` 能力挂成主窗的 AppKit 子窗口。初始尺寸只是占位,
//! 产品打开后会按父窗口客户区重新对齐。

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

/// 设置 overlay:无标题栏、透明、不可拖动/缩放;frame 由父窗口决定。
///
/// `exit_on_close_request: false` 让系统关闭 / Esc / 关闭按钮都走统一生命周期。
pub fn settings_window() -> window::Settings {
    window::Settings {
        // 占位尺寸;绑定父窗口后立即被父客户区替换。
        size: Size::new(800.0, 600.0),
        min_size: Some(Size::new(1.0, 1.0)),
        resizable: false,
        minimizable: false,
        closeable: false,
        decorations: false,
        transparent: true,
        exit_on_close_request: false,
        position: window::Position::Centered,
        ..Default::default()
    }
}
