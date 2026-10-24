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
}

// 下面两个是给**订阅回调**读的快照。订阅回调跑在 tokio 工作线程,`view` 跑在主线程,
// `thread_local` 每线程各一份,订阅线程读到的永远是初始值(光标 `(0, 0)`、浮层"未打开"),
// 终端因此从未获得焦点。所以这两个必须是进程级的原子量,不能是 `thread_local`。

/// 一个 `iced::Point` 的跨线程快照(x、y 的位模式各占一半,一次原子读写,不会读到撕裂值)。
pub(crate) struct SharedPoint(std::sync::atomic::AtomicU64);

impl SharedPoint {
    pub(crate) const fn new() -> Self {
        Self(std::sync::atomic::AtomicU64::new(0))
    }

    pub(crate) fn set(&self, point: iced::Point) {
        let bits = (u64::from(point.x.to_bits()) << 32) | u64::from(point.y.to_bits());
        self.0.store(bits, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn get(&self) -> iced::Point {
        let bits = self.0.load(std::sync::atomic::Ordering::Relaxed);
        iced::Point::new(
            f32::from_bits((bits >> 32) as u32),
            f32::from_bits(bits as u32),
        )
    }
}

/// 最近一次光标位置:订阅里的鼠标事件不带坐标,只能由 `view` 每帧写入。
pub(crate) static CURRENT_CURSOR: SharedPoint = SharedPoint::new();

/// Agent 自己的浮层(选择器 / 溢出下拉 / 关页签确认)是否打开:打开时键盘不投终端。
pub(crate) static AGENT_KEYS_BLOCKED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;

    /// 订阅回调跑在 tokio 工作线程,`view` 跑在主线程:快照必须跨线程可见。
    /// (原来用 `thread_local`,另一线程读到的永远是初始值 `(0, 0)`,终端因此从未获得焦点。)
    #[test]
    fn a_cursor_written_on_one_thread_is_read_on_another() {
        let cell = SharedPoint::new();
        cell.set(iced::Point::new(123.5, 45.25));
        let seen = std::thread::scope(|s| s.spawn(|| cell.get()).join().unwrap());
        assert_eq!(seen, iced::Point::new(123.5, 45.25));
    }

    #[test]
    fn a_fresh_shared_point_is_the_origin() {
        assert_eq!(SharedPoint::new().get(), iced::Point::new(0.0, 0.0));
    }

    #[test]
    fn negative_and_fractional_coordinates_round_trip() {
        let cell = SharedPoint::new();
        cell.set(iced::Point::new(-3.75, 1e-3));
        assert_eq!(cell.get(), iced::Point::new(-3.75, 1e-3));
    }
}
