//! 异步结果的身份信封(规格 §5.3)。
//!
//! 面板状态机是纯的:它只认"结果回来了",不认"这个结果是不是还给当前这个实例的"。把一次在途请求
//! 包上身份([`RequestTicket`]:scope + panel instance + generation + request key + sequence),
//! 回信时由 [`RequestTable`] 核对:**实例仍存活、scope 一致、且属于当前有效请求**才交给状态机,
//! 否则静默丢弃。
//!
//! 取消是资源优化,**身份校验才是正确性保障**:即使请求没能及时取消,迟到的结果也不会污染新的实例。
//! 同一 key 采 latest-only(后发起的取代在途的同 key 请求);不同 key 各自独立,不相互覆盖。
//!
//! 使结果失效的三种情形(规格 §5.3 / Task 13 第 4 条):
//! - **面板关闭**:[`RequestTable::invalidate_panel`] —— 该实例的旧结果全部作废;
//! - **卸载应用**:[`RequestTable::invalidate_app`] —— 涉及该应用的旧结果作废;
//! - **Surface 重建 / 项目切换**:[`RequestTable::bump_generation`] —— 旧 generation 的结果作废。
//!
//! 失效**只影响 UI 结果的接收**,不影响后台的写操作:安装/卸载/启动一旦交给服务就继续完成。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::AppKeyId;

/// 产品提供的不透明作用域:宿主不理解业务数据,只把它当身份的一部分原样带回。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u64);

/// 区分同一面板的不同实例(一次销毁重建 = 一次新的 instance)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelInstanceId(pub u64);

/// 在途请求的精确身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestTicket {
    pub scope: ScopeId,
    pub panel: PanelInstanceId,
    /// Surface/实例的 generation:重建后旧 generation 的结果一律丢弃。
    pub generation: u64,
    /// 同一个 key 采 latest-only(如"列表刷新"、"取某个应用的 launch URL")。
    pub key: RequestKey,
    /// 每次派发递增,识别同一 key 上更晚的那一次。
    pub sequence: u64,
}

/// 一次在途请求的逻辑名(latest-only 的去重单位)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKey {
    List,
    LaunchUrl(AppKeyId),
    Logs(AppKeyId),
    /// 其他一次性动作(启停、安装…):不参与 latest-only 去重,但同样带身份。
    Action(u64),
}

impl RequestKey {
    /// 是否按 latest-only 处理(同 key 只有最近一次有效)。
    pub fn latest_only(&self) -> bool {
        !matches!(self, RequestKey::Action(_))
    }
}

/// 一个活着且当前有效的请求:派发时登记,回信时核对。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Live {
    scope: ScopeId,
    generation: u64,
    sequence: u64,
    /// 该键是否仍存活(`invalidate_app` / `invalidate_panel` 会把它标死)。
    live: bool,
}

/// 在途请求台账。线程安全:执行器在后台任务里派发,窗口线程回信。
#[derive(Debug, Default)]
pub struct RequestTable {
    /// (panel, key) → 当前有效的那一次。
    live: Mutex<HashMap<(PanelInstanceId, RequestKey), Live>>,
}

impl RequestTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一次派发;返回该请求的精确身份。同 key 若已有在途,后者取代前者(latest-only)。
    pub fn dispatch(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        key: RequestKey,
    ) -> RequestTicket {
        let mut live = self.live.lock().expect("request table poisoned");
        let sequence = live
            .get(&(panel, key))
            .map_or(0, |previous| previous.sequence.wrapping_add(1));
        live.insert(
            (panel, key),
            Live {
                scope,
                generation,
                sequence,
                live: true,
            },
        );
        RequestTicket {
            scope,
            panel,
            generation,
            key,
            sequence,
        }
    }

    /// 回信时核对:实例存活、scope/generation/sequence 都匹配当前有效请求,才算数。
    /// 通过则消费掉这条在途记录(一次性)。
    pub fn accept(&self, ticket: RequestTicket) -> bool {
        let mut live = self.live.lock().expect("request table poisoned");
        let Some(current) = live.get(&(ticket.panel, ticket.key)) else {
            return false;
        };
        let ok = current.live
            && current.scope == ticket.scope
            && current.generation == ticket.generation
            && current.sequence == ticket.sequence;
        if ok {
            live.remove(&(ticket.panel, ticket.key));
        }
        ok
    }

    /// 面板关闭:该实例的所有在途请求作废(迟到的结果一律丢弃)。
    pub fn invalidate_panel(&self, panel: PanelInstanceId) {
        let mut live = self.live.lock().expect("request table poisoned");
        live.retain(|(p, _), _| *p != panel);
    }

    /// 应用被卸载:涉及该应用的在途请求作废(不同 key 都覆盖)。
    pub fn invalidate_app(&self, app: AppKeyId) {
        let mut live = self.live.lock().expect("request table poisoned");
        live.retain(|(_, key), _| match key {
            RequestKey::LaunchUrl(k) | RequestKey::Logs(k) => *k != app,
            RequestKey::List | RequestKey::Action(_) => true,
        });
    }

    /// Surface 重建 / 项目切换:旧 generation 的结果全部作废。
    pub fn bump_generation(&self, generation: u64) {
        let mut live = self.live.lock().expect("request table poisoned");
        live.retain(|_, entry| entry.generation == generation || !entry.live);
        // 新 generation 下没有任何在途记录:旧的全丢。
        live.retain(|_, entry| entry.generation == generation);
    }

    /// 当前在途条数(测试用)。
    #[cfg(test)]
    fn len(&self) -> usize {
        self.live.lock().expect("request table poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> AppKeyId {
        use bytehost_panel::key::AppKey;
        AppKeyId::from_app_id(&bytehost_apps::id::AppId::new(id).unwrap()).unwrap()
    }

    #[test]
    fn a_matching_reply_is_accepted_once() {
        let t = RequestTable::new();
        let ticket = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        assert!(t.accept(ticket));
        assert!(!t.accept(ticket), "一次性:重复回信被丢弃");
    }

    #[test]
    fn a_later_request_on_the_same_key_supersedes_the_earlier_one() {
        let t = RequestTable::new();
        let first = t.dispatch(
            ScopeId(1),
            PanelInstanceId(1),
            0,
            RequestKey::LaunchUrl(key("a")),
        );
        let second = t.dispatch(
            ScopeId(1),
            PanelInstanceId(1),
            0,
            RequestKey::LaunchUrl(key("a")),
        );
        assert_ne!(first.sequence, second.sequence);
        assert!(!t.accept(first), "旧的一次被取代,迟到结果丢弃");
        assert!(t.accept(second), "只有最近一次算数");
    }

    #[test]
    fn scope_mismatch_is_rejected() {
        let t = RequestTable::new();
        let ticket = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        let forged = RequestTicket {
            scope: ScopeId(2),
            ..ticket
        };
        assert!(!t.accept(forged));
    }

    #[test]
    fn a_stale_generation_is_rejected() {
        let t = RequestTable::new();
        let old = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        t.bump_generation(1);
        assert!(!t.accept(old), "旧 generation 的结果作废");
        let fresh = t.dispatch(ScopeId(1), PanelInstanceId(1), 1, RequestKey::List);
        assert!(t.accept(fresh));
    }

    #[test]
    fn closing_a_panel_invalidates_all_its_requests() {
        let t = RequestTable::new();
        let a = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        let b = t.dispatch(
            ScopeId(1),
            PanelInstanceId(1),
            0,
            RequestKey::Logs(key("a")),
        );
        let other = t.dispatch(ScopeId(1), PanelInstanceId(2), 0, RequestKey::List);
        t.invalidate_panel(PanelInstanceId(1));
        assert!(!t.accept(a));
        assert!(!t.accept(b));
        assert!(t.accept(other), "别的实例不受影响");
    }

    #[test]
    fn uninstalling_an_app_invalidates_only_that_apps_requests() {
        let t = RequestTable::new();
        let app = key("gone");
        let target = t.dispatch(
            ScopeId(1),
            PanelInstanceId(1),
            0,
            RequestKey::LaunchUrl(app),
        );
        let logs = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::Logs(app));
        let list = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        let other = t.dispatch(
            ScopeId(1),
            PanelInstanceId(1),
            0,
            RequestKey::LaunchUrl(key("kept")),
        );
        t.invalidate_app(app);
        assert!(!t.accept(target));
        assert!(!t.accept(logs));
        assert!(t.accept(list), "列表不是针对某个应用的,不受卸载影响");
        assert!(t.accept(other));
    }

    #[test]
    fn actions_are_not_latest_only_but_still_carry_identity() {
        let t = RequestTable::new();
        let a = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::Action(1));
        let b = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::Action(2));
        assert!(t.accept(a), "不同动作 key 互不覆盖");
        assert!(t.accept(b));
        assert_eq!(t.len(), 0);
    }
}
