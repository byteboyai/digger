//! Digger 桌面入口:在共享 bytehost 宿主上启动产品壳(Task 11)。
//!
//! 组合根委托:`product::boot` 建产品壳并设主题,`Product::update`/`view`
//! 经宿主泛型包装驱动(`HostMessage<M>` 证明产品消息不进入宿主 enum)。

mod apps;
mod paths;
mod product;

use bytehost_desktop::HostMessage;
use iced::{Element, Task};

use product::{Msg, ProductShell};

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
    iced::application(boot, update, view)
        .title("Digger")
        .subscription(product::subscription)
        .window_size(product::default_window_size_for_boot())
        .run()
}
