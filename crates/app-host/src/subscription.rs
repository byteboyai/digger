//! 订阅闭环(规格 §5.3 / Task 13 第 3 条)。
//!
//! 把 [`bytehost_client::AppChange`] 的失效信号翻译成面板状态机认的消息,并驱动重订阅:
//!
//! - `Changed` / `Resync` → [`PanelMessage::Changed`]:状态机内部**合并刷新**——若正有一次 `List`
//!   在途,只置一次 `dirty`(不叠加请求),等它回来补发一次;否则立刻刷一次。
//! - `Disconnected` → [`PanelMessage::SubscriptionLost`]:状态机按共享退避序列
//!   [`bytehost_panel::host::RESUBSCRIBE_BACKOFF`] 排下一次重订阅,并把轮询从兜底档
//!   ([`SAFETY_POLL_INTERVAL`]) 拉回快速档([`POLL_INTERVAL`]),直到重订阅成功。
//! - 重订阅成功 → [`PanelMessage::Subscribed`]:状态机切回 `Live` 并补拉一次(退避期间可能漏了变更)。
//!
//! 退避与轮询的**规则只有一份**(定义在 `bytehost-panel`),本模块不复制常量;它只负责
//! "拿到事件 → 转消息"与"按状态机说的去重订阅"。这样 Digger 与 Dozer 的时序行为一致。
//!
//! 订阅的连接本身由 [`Executor`](crate::effects::Executor) 之外的任务持有:拿到
//! `mpsc::UnboundedReceiver<AppChange>`,逐条经 `proxy` 转成 [`PanelMessage`] 回喂窗口;
//! 接收端被丢弃时(见 `InProcess::subscribe` 的实现)消息流以 `Disconnected` 收尾。

use bytehost_client::AppChange;
use bytehost_panel::host::PanelMessage;

use crate::AppKeyId;

/// 把一条 [`AppChange`] 失效信号翻成面板状态机消息。
///
/// `Changed` 与 `Resync` **不做区分**——两者都只是"去重拉一次列表"的失效信号,合并逻辑在状态机里
/// (有在途则置 `dirty`),所以这里统一映射成 [`PanelMessage::Changed`]。`Disconnected` 映射成
/// [`PanelMessage::SubscriptionLost`],由状态机排退避重订阅。
pub fn change_to_message(change: AppChange) -> PanelMessage<AppKeyId> {
    match change {
        AppChange::Changed(_) | AppChange::Resync => PanelMessage::Changed,
        AppChange::Disconnected => PanelMessage::SubscriptionLost,
    }
}

/// 订阅已建立(dozerd/InProcess 回了 `Subscribed`)时回喂的消息。
pub const SUBSCRIBED: PanelMessage<AppKeyId> = PanelMessage::Subscribed;

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_apps::id::AppId;

    #[test]
    fn changed_and_resync_both_ask_for_a_coalesced_refresh() {
        assert!(matches!(
            change_to_message(AppChange::Changed(AppId::new("a").unwrap())),
            PanelMessage::Changed
        ));
        assert!(
            matches!(change_to_message(AppChange::Resync), PanelMessage::Changed),
            "Resync 与 Changed 同义:都只是去重拉一次"
        );
    }

    #[test]
    fn disconnect_asks_for_backoff_resubscribe() {
        assert!(matches!(
            change_to_message(AppChange::Disconnected),
            PanelMessage::SubscriptionLost
        ));
    }
}
