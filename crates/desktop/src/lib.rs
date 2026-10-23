//! Digger 桌面组合根的库一面:模块入口 + 给展示适配器取外部状态的 thread-local。
//!
//! `main.rs` 只保留 iced 应用的 boot/update/view 三件事;拆出 lib 是为了让
//! `examples/agent_gallery.rs` 能复用产品里的适配层(与 S6d-3 的对照画廊)。

pub mod adapters;
pub mod apps;
pub mod daemon;
pub mod fonts;
pub mod lifecycle_selftest;
pub mod markdown;
pub mod paths;
pub mod preview_assets;
pub mod product;
pub mod projects;
pub mod shell;

// 下面这些 thread-local 是给 `PanelDisplay` / 事件订阅这类**拿不到
// `ProductShell`** 的代码取外部状态的绕法:产品每帧 `view` 前刷新一份快照,
// 订阅闭包 / 展示适配器只读。约束:**只读、每帧覆盖、不跨帧累积**。
thread_local! {
    /// 最近一次已知的窗口逻辑尺寸:只给 `PanelDisplay` 这类拿不到 `ProductShell` 的展示适配器用。
    pub(crate) static CURRENT_WINDOW: std::cell::Cell<Option<(f32, f32)>> =
        const { std::cell::Cell::new(None) };
    /// 最近一次光标位置:订阅里的鼠标事件不带坐标,只能由 `view` 每帧写入。
    pub(crate) static CURRENT_CURSOR: std::cell::Cell<iced::Point> =
        const { std::cell::Cell::new(iced::Point { x: 0.0, y: 0.0 }) };
    /// Agent 自己的浮层(选择器 / 溢出下拉 / 关页签确认)是否打开:打开时键盘不投终端。
    pub(crate) static AGENT_KEYS_BLOCKED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}
