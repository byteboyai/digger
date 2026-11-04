//! Digger 桌面入口:在共享 bytehost 宿主上启动产品壳(Task 11)。
//!
//! 组合根委托:`product::boot` 建产品壳并设主题,`Product::update`/`view`
//! 经宿主泛型包装驱动(`HostMessage<M>` 证明产品消息不进入宿主 enum)。
//!
//! 使用 `iced::daemon` 而不是单窗 `application`:设置窗口需要独立的原生窗口,
//! 内容仍由共享 `bytehost-settings-ui` 绘制。主窗在 `product::boot` 里显式打开。

use digger_desktop::product::{self, Msg, ProductShell};
use digger_desktop::{daemon, fonts};

use bytehost_desktop::HostMessage;
use iced::{Element, window};

fn boot() -> (ProductShell, iced::Task<Msg>) {
    product::boot()
}

fn update(shell: &mut ProductShell, msg: Msg) -> iced::Task<Msg> {
    // 窗口消息经宿主泛型包装交给产品;产品不直接处理宿主 enum。
    product::update_host(shell, HostMessage::Product(msg))
}

fn view(shell: &ProductShell, id: window::Id) -> Element<'_, Msg> {
    if shell.is_settings_window(id) {
        shell.settings_overlay_view()
    } else {
        bytehost_desktop::Product::view(shell)
    }
}

fn title(shell: &ProductShell, id: window::Id) -> String {
    if shell.is_settings_window(id) {
        "Digger Settings".to_owned()
    } else {
        "Digger".to_owned()
    }
}

fn main() -> iced::Result {
    // 先于一切 `dozer_client::Client`:握手比对的是进程级产品身份。
    daemon::install_product();
    fonts::sanitize_font_db();
    iced::daemon(boot, update, view)
        .title(title)
        .subscription(product::subscription)
        // 设置 overlay 是透明子窗口:全局清屏必须透明,主窗口根容器自己铺底色。
        .style(
            |_shell: &ProductShell, _theme: &iced::Theme| iced::theme::Style {
                background_color: iced::Color::TRANSPARENT,
                text_color: iced::Color::WHITE,
            },
        )
        .font(fonts::JETBRAINS_MONO)
        .font(dozer_panel_term::symbols::SYMBOLS_FONT_DATA)
        .run()
}
