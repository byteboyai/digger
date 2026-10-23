//! Digger 桌面入口:在共享 bytehost 宿主上启动产品壳(Task 11)。
//!
//! 组合根委托:`product::boot` 建产品壳并设主题,`Product::update`/`view`
//! 经宿主泛型包装驱动(`HostMessage<M>` 证明产品消息不进入宿主 enum)。

mod adapters;
mod apps;
// `code_font`/`terminal_style` 由 S6d-2 的终端视图消费;本期只交付并测试。
#[allow(dead_code)]
mod fonts;
mod lifecycle_selftest;
mod markdown;
mod paths;
mod preview_assets;
mod product;
mod projects;
mod shell;
// S6a:守护进程拉起与生命周期。
mod daemon;

use bytehost_desktop::HostMessage;
use iced::{Element, Task};

use product::{Msg, ProductShell};

// 下面这个 thread local(以及 `product::CURRENT_PANES`)是给 `PanelDisplay`
// 适配器**取外部状态**的绕法:库的 `PanelDisplay` 方法签名(`fn background(&self)`
// 等)没有地方传窗口尺寸 / 分栏状态,而适配器本身是 `&self` 的零状态展示对象。
// 因此由产品每帧 `view` 前刷新一份快照,适配器只读。约束:**只读、每帧覆盖、
// 不跨帧累积**——见 `pane_snapshot_does_not_outlive_the_frame`。
thread_local! {
    /// 最近一次已知的窗口逻辑尺寸:只给 `PanelDisplay` 这类拿不到 `ProductShell` 的展示适配器用。
    pub(crate) static CURRENT_WINDOW: std::cell::Cell<Option<(f32, f32)>> =
        const { std::cell::Cell::new(None) };
}

fn boot() -> (ProductShell, Task<Msg>) {
    product::boot()
}

fn update(shell: &mut ProductShell, msg: Msg) -> Task<Msg> {
    // 窗口消息经宿主泛型包装交给产品;产品不直接处理宿主 enum。
    product::update_host(shell, HostMessage::Product(msg))
}

fn view(shell: &ProductShell) -> Element<'_, Msg> {
    bytehost_desktop::Product::view(shell)
}

fn main() -> iced::Result {
    fonts::sanitize_font_db();
    iced::application(boot, update, view)
        .font(fonts::JETBRAINS_MONO)
        .title("Digger")
        .window(iced::window::Settings {
            exit_on_close_request: false,
            min_size: Some(iced::Size::new(
                byteui::theme::geometry::min_window_width(),
                byteui::theme::geometry::min_window_height(),
            )),
            // 与 Dozer 一致:透明标题栏 + 全尺寸内容视图,顶栏取代系统标题栏(红绿灯由 `bytehost-chrome` 摆进顶栏里)。
            platform_specific: iced::window::settings::PlatformSpecific {
                title_hidden: true,
                titlebar_transparent: true,
                fullsize_content_view: true,
            },
            ..Default::default()
        })
        .subscription(product::subscription)
        .window_size(product::default_window_size_for_boot())
        .run()
}
