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
    key: RequestKey,
    scope: ScopeId,
    generation: u64,
    /// 该键是否仍存活(`invalidate_app` / `invalidate_panel` 会把它标死)。
    live: bool,
}

/// 在途请求台账。线程安全:执行器在后台任务里派发,窗口线程回信。
///
/// 内部用**单调递增的 sequence**(只在表内累加,永不因记录被消费而重置)作为每张
/// ticket 的精确身份,并给每个在途请求各留一条记录。latest-only 的键
/// ([`RequestKey::latest_only`]) 另有一张索引指向"该键当前有效的那一次";派发新的一次
/// 只把旧记录标记为作废,**不删除**,所以旧 ticket 的 `sequence` 仍能在表中查到并被
/// 正确拒绝——不会与之后某个恰好复用了旧序号的请求撞车。
///
/// 一次性写动作([`RequestKey::Action`])不进索引,每个动作独立留档:即便多个应用并发
/// 停止/卸载共用同一个动作 key,各自的回包也能被各自接收。
#[derive(Debug, Default)]
pub struct RequestTable {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// 下一个 identity;只增不减。
    next_sequence: u64,
    /// sequence → 该次派发。
    live: HashMap<u64, Live>,
    /// sequence → 归属面板(便于 `invalidate_panel` 只按面板整批作废)。
    panels: HashMap<u64, PanelInstanceId>,
    /// latest-only 键 → 该键当前有效的 sequence。
    latest: HashMap<(PanelInstanceId, RequestKey), u64>,
}

impl RequestTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一次派发;返回该请求的精确身份。latest-only 键若已有在途,后者取代前者。
    pub fn dispatch(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        key: RequestKey,
    ) -> RequestTicket {
        let mut inner = self.inner.lock().expect("request table poisoned");
        let sequence = inner.next_sequence;
        inner.next_sequence = inner.next_sequence.wrapping_add(1);
        inner.live.insert(
            sequence,
            Live {
                key,
                scope,
                generation,
                live: true,
            },
        );
        inner.panels.insert(sequence, panel);
        if key.latest_only()
            && let Some(old) = inner.latest.insert((panel, key), sequence)
            && let Some(entry) = inner.live.get_mut(&old)
        {
            // 旧的那次标记作废但保留记录,旧 ticket 仍会被拒且不会误配之后的新序号。
            entry.live = false;
        }
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
        let mut inner = self.inner.lock().expect("request table poisoned");
        let Some(entry) = inner.live.get(&ticket.sequence) else {
            return false;
        };
        // sequence 是精确身份,但还要防伪造 ticket:key/scope/generation 都必须一致。
        let ok = entry.live
            && entry.key == ticket.key
            && entry.scope == ticket.scope
            && entry.generation == ticket.generation;
        if ok {
            inner.live.remove(&ticket.sequence);
            inner.panels.remove(&ticket.sequence);
            if ticket.key.latest_only() {
                inner.latest.remove(&(ticket.panel, ticket.key));
            }
        }
        ok
    }

    /// 面板关闭:该实例的所有在途请求作废(迟到的结果一律丢弃)。
    pub fn invalidate_panel(&self, panel: PanelInstanceId) {
        let mut inner = self.inner.lock().expect("request table poisoned");
        inner.invalidate_panel(panel);
    }

    /// 应用被卸载:涉及该应用的在途请求作废(不同 key 都覆盖)。
    pub fn invalidate_app(&self, app: AppKeyId) {
        let mut inner = self.inner.lock().expect("request table poisoned");
        let doomed: Vec<u64> = inner
            .live
            .iter()
            .filter(|(_, entry)| match entry.key {
                RequestKey::LaunchUrl(k) | RequestKey::Logs(k) => k == app,
                RequestKey::List | RequestKey::Action(_) => false,
            })
            .map(|(seq, _)| *seq)
            .collect();
        for seq in doomed {
            if let Some(entry) = inner.live.remove(&seq) {
                inner.panels.remove(&seq);
                if entry.key.latest_only() {
                    inner.latest.retain(|_, s| *s != seq);
                }
            }
        }
    }

    /// Surface 重建 / 项目切换:旧 generation 的结果全部作废。
    pub fn bump_generation(&self, generation: u64) {
        let mut inner = self.inner.lock().expect("request table poisoned");
        inner.live.retain(|_, entry| entry.generation == generation);
        let keep: std::collections::HashSet<u64> = inner.live.keys().copied().collect();
        inner.panels.retain(|seq, _| keep.contains(seq));
        inner.latest.retain(|_, seq| keep.contains(seq));
    }

    /// 当前在途条数(测试用)。
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("request table poisoned")
            .live
            .len()
    }
}

impl Inner {
    /// 面板关闭:删掉该面板相关的所有记录与索引。
    fn invalidate_panel(&mut self, panel: PanelInstanceId) {
        let doomed: Vec<u64> = self
            .panels
            .iter()
            .filter(|(_, p)| **p == panel)
            .map(|(seq, _)| *seq)
            .collect();
        for seq in doomed {
            self.panels.remove(&seq);
            if let Some(entry) = self.live.remove(&seq)
                && entry.key.latest_only()
            {
                self.latest.retain(|_, s| *s != seq);
            }
        }
        // 兜底:latest 索引里若有指向已删 sequence 的项,一并清掉。
        self.latest.retain(|_, seq| self.live.contains_key(seq));
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

    /// 发现 1(回归):stale 的旧请求不得在"序号被消费后重用"时冒充新请求。
    ///
    /// A(seq0) → B(seq1) → 接收 B → C(seq2)。此时迟到的 A 必须被拒,且不得消费 C 的登记。
    #[test]
    fn a_stale_reply_cannot_impersonate_a_newer_request_after_reuse() {
        let t = RequestTable::new();
        let a = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        let b = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        assert!(!t.accept(a), "A 已被 B 取代");
        assert!(t.accept(b), "B 是最新,接收并消费");
        let c = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::List);
        assert_ne!(a.sequence, c.sequence, "序号不得因记录被消费而重置");
        assert!(!t.accept(a), "迟到的 A 仍被拒");
        assert!(t.accept(c), "C 的登记未被 A 消费,仍可接收");
    }

    /// 发现 2(回归):多个写动作即便共用同一个动作 key(不同应用),回包也必须各自接收。
    #[test]
    fn concurrent_actions_sharing_one_key_each_receive_their_reply() {
        let t = RequestTable::new();
        let stop_a = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::Action(7));
        let stop_b = t.dispatch(ScopeId(1), PanelInstanceId(1), 0, RequestKey::Action(7));
        assert_ne!(stop_a.sequence, stop_b.sequence);
        assert!(t.accept(stop_a), "先发的写动作回包不得被后发的覆盖丢弃");
        assert!(t.accept(stop_b), "后发的写动作回包也接收");
        assert_eq!(t.len(), 0);
    }
}
