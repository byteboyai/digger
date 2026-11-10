//! Digger 的宿主适配层:把产品特有语义注入共享面板库(Task 20)。
//!
//! 与 `dozer/crates/dozer-app/src/host_adapters/` 同型:共享库(`dozer-panel-usage`)
//! 只定义窄接缝,产品侧提供实现。这里有 Usage 与 Project 两个面板的适配。

pub mod agent;
pub mod agent_hooks;
pub mod agent_ime;
// 键位转换由 S6d-2 的终端输入接线消费(S6d-2 Task 5 待输入法探针结果)。
pub mod agent_display;
#[allow(dead_code)]
pub mod agent_keys;
pub mod agent_layout;
pub mod agent_tabs;
pub mod files;
#[cfg(target_os = "macos")]
mod files_secondary_click;
pub mod hover;
pub mod menu_overlay;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod project;
pub(crate) mod shell_pointer;
pub mod usage;

pub mod usage_native;

/// 预览原生宿主(S5b-2 P3):清单 / 协议 / 注入脚本 / IPC 全走共享库。
pub mod preview_native;

pub mod preview;
pub mod preview_bridge;

/// 预览页签栏部件(页签 / 溢出入口 / 下拉 / 模式按钮)。
pub mod tab_chrome;

/// 待办面板适配(S3b-1)。
pub mod todo;
/// 待办面板的 wry 宿主(S3b-1)。
pub mod todo_native;

/// 待办面板的嵌入 web 资源(S3b-1)。
pub mod todo_assets;

/// 预览面板的 `PanelDisplay<Msg>` 实现(库视图反过来要的展示能力)。
pub mod preview_display;

pub mod browser;
pub mod browser_addr_menu;
pub mod browser_display;
pub mod browser_native;
pub mod conversations;
pub mod conversations_display;
pub mod group_chat;
pub mod group_chat_display;
pub mod group_chat_geometry;
pub mod group_chat_native;
pub mod group_chat_overlay;
pub mod notice;
/// 会话面板 icon-scale 布局断言:单元测试与 `icon_scale_widget_probe` 共用。
pub mod scale_widget_checks;

pub mod conversations_import;
pub mod conversations_search_menu;

pub mod review_native;

pub mod review_geometry;

use iced_widget::button;

/// 收起 / 展开列表列等图标按钮:用中性深色底(`card`),去掉 iced 默认按钮的蓝紫底色。
/// 默认 `button()` 会吃到主题默认按钮样式(蓝紫填充),这类纯图标按钮应是透明的深色方块。
pub fn collapse_button_style() -> impl Fn(&iced_widget::Theme, button::Status) -> button::Style {
    move |_t, _s| button::Style {
        background: Some(byteui::theme::color::current().card.into()),
        ..button::Style::default()
    }
}
