//! 测试替身:[`FakeHost`] 是一个可编程的 [`AppHostApi`]。
//!
//! 要覆盖的场景(规格 §5.2 / Task 13 第 1 条):
//! - **成功**:预设某个请求的应答;
//! - **Host 失败**:带类别的宿主拒绝(如 `Unavailable`);
//! - **Transport 失败**:连不上/协议不符;
//! - **断订阅**:订阅流立刻以 `Disconnected` 收尾(模拟服务消失);
//! - **Resync / 迟到回包**:手动往订阅流里推 `Resync`;迟到的应答由调用方在 `AppHostApi`
//!   之上用 [`crate::request::RequestTable`] 核对身份;
//! - **未预设的调用显式失败**:任何没有预设的请求返回 `Transport("fake 未预设: …")`,
//!   而不是静默成功——测试里"意外调用"必须看见。
//!
//! 生产**绝不**用本模块:它只为 `cargo test` 存在。

use std::collections::HashMap;
use std::sync::Mutex;

use bytehost_apps::id::AppId;
use bytehost_apps::proto::{AppErrorKind, AppFailure, AppReply, AppRequest, AppSummary};
use bytehost_client::{AppApiError, AppChange, AppHostApi};
use tokio::sync::mpsc;

/// 预设的动作:一次请求得到什么结果。
#[derive(Debug, Clone)]
enum Canned {
    Reply(AppReply),
    HostFail(AppErrorKind, String),
    TransportFail(String),
}

/// 可编程的 [`AppHostApi`]。
pub struct FakeHost {
    canned: Mutex<HashMap<String, Canned>>,
    /// 订阅行为:默认"成功但永不发事件"(测试自己会推);可设成"立刻断开"。
    subscribe_mode: Mutex<SubscribeMode>,
    /// 已发出的订阅流(测试往里面推事件)。
    subscriber_tx: Mutex<Option<mpsc::UnboundedSender<AppChange>>>,
    /// 所有收到过的请求(断言"发了什么")。
    requests: Mutex<Vec<AppRequest>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscribeMode {
    /// 订阅成功,流挂着等测试推事件。
    Live,
    /// 订阅成功但立刻发 `Disconnected` 收尾。
    Disconnected,
    /// 订阅本身失败(`Host(Unavailable)`)。
    Unavailable,
}

impl FakeHost {
    pub fn new() -> Self {
        Self {
            canned: Mutex::new(HashMap::new()),
            subscribe_mode: Mutex::new(SubscribeMode::Live),
            subscriber_tx: Mutex::new(None),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// 预设"列表"的应答。
    pub fn on_list(&self, apps: Vec<AppSummary>) {
        self.canned
            .lock()
            .unwrap()
            .insert("list".into(), Canned::Reply(AppReply::Apps { apps }));
    }

    /// 预设"列表"返回宿主失败(带类别)。
    pub fn on_list_host_fail(&self, kind: AppErrorKind, message: impl Into<String>) {
        self.canned
            .lock()
            .unwrap()
            .insert("list".into(), Canned::HostFail(kind, message.into()));
    }

    /// 预设"列表"返回传输失败。
    pub fn on_list_transport_fail(&self, message: impl Into<String>) {
        self.canned
            .lock()
            .unwrap()
            .insert("list".into(), Canned::TransportFail(message.into()));
    }

    /// 预设某个应用的启动地址。
    pub fn on_launch_url(&self, id: &AppId, url: impl Into<String>) {
        self.canned.lock().unwrap().insert(
            format!("launch:{}", id.as_str()),
            Canned::Reply(AppReply::LaunchUrl { url: url.into() }),
        );
    }

    /// 预设某个应用的启动结果(成功时给一个站点地址)。
    pub fn on_start(&self, id: &AppId, result: Result<String, AppApiError>) {
        let canned = match result {
            Ok(url) => Canned::Reply(AppReply::Started { url }),
            Err(AppApiError::Host(f)) => Canned::HostFail(f.kind, f.message),
            Err(AppApiError::Transport(t)) => Canned::TransportFail(t),
        };
        self.canned
            .lock()
            .unwrap()
            .insert(format!("start:{}", id.as_str()), canned);
    }

    pub fn on_stop(&self, id: &AppId, result: Result<(), AppApiError>) {
        self.canned.lock().unwrap().insert(
            format!("stop:{}", id.as_str()),
            canned_from_result(result, AppReply::Done),
        );
    }

    /// 预设某应用的日志末尾。
    pub fn on_logs(&self, id: &AppId, text: impl Into<String>, truncated: bool) {
        self.canned.lock().unwrap().insert(
            format!("logs:{}", id.as_str()),
            Canned::Reply(AppReply::Logs {
                text: text.into(),
                truncated,
            }),
        );
    }

    /// 预设"出安装计划"的结果。
    pub fn on_plan(&self, result: Result<bytehost_apps::plan::InstallPlan, AppApiError>) {
        let canned = match result {
            Ok(plan) => Canned::Reply(AppReply::Plan {
                plan: Box::new(plan),
            }),
            Err(AppApiError::Host(f)) => Canned::HostFail(f.kind, f.message),
            Err(AppApiError::Transport(t)) => Canned::TransportFail(t),
        };
        self.canned.lock().unwrap().insert("plan".into(), canned);
    }

    /// 预设"安装已批准计划"的结果。
    pub fn on_install(&self, result: Result<(), AppApiError>) {
        self.canned
            .lock()
            .unwrap()
            .insert("install".into(), canned_from_result(result, AppReply::Done));
    }

    /// 预设"卸载"的结果。
    pub fn on_uninstall(&self, result: Result<(), AppApiError>) {
        self.canned.lock().unwrap().insert(
            "uninstall".into(),
            canned_from_result(result, AppReply::Done),
        );
    }

    /// 预设"探测运行时"的结果。
    pub fn on_probe(&self, runtimes: Vec<bytehost_apps::proto::RuntimeProbe>) {
        self.canned.lock().unwrap().insert(
            "probe".into(),
            Canned::Reply(AppReply::Runtimes { runtimes }),
        );
    }

    /// 预设"探测运行时"返回宿主失败(带类别)。
    pub fn on_probe_host_fail(&self, kind: AppErrorKind, message: impl Into<String>) {
        self.canned
            .lock()
            .unwrap()
            .insert("probe".into(), Canned::HostFail(kind, message.into()));
    }

    /// 预设某应用的回滚结果。
    pub fn on_rollback(&self, id: &AppId, result: Result<(), AppApiError>) {
        self.canned.lock().unwrap().insert(
            format!("rollback:{}", id.as_str()),
            canned_from_result(result, AppReply::Done),
        );
    }

    /// 预设"出运行时安装计划"的结果。
    pub fn on_runtime_plan(
        &self,
        result: Result<bytehost_apps::proto::RuntimeInstallPlan, AppApiError>,
    ) {
        let canned = match result {
            Ok(plan) => Canned::Reply(AppReply::RuntimePlan {
                plan: Box::new(plan),
            }),
            Err(AppApiError::Host(f)) => Canned::HostFail(f.kind, f.message),
            Err(AppApiError::Transport(t)) => Canned::TransportFail(t),
        };
        self.canned
            .lock()
            .unwrap()
            .insert("runtime_plan".into(), canned);
    }

    /// 预设"安装已批准运行时计划"的结果。
    pub fn on_runtime_install(&self, result: Result<(), AppApiError>) {
        self.canned.lock().unwrap().insert(
            "runtime_install".into(),
            canned_from_result(result, AppReply::Done),
        );
    }

    /// 预设"卸载运行时"的结果。
    pub fn on_runtime_uninstall(&self, result: Result<(), AppApiError>) {
        self.canned.lock().unwrap().insert(
            "runtime_uninstall".into(),
            canned_from_result(result, AppReply::Done),
        );
    }

    /// 让订阅一建立就断开(模拟服务消失)。
    pub fn subscribe_disconnects(&self) {
        *self.subscribe_mode.lock().unwrap() = SubscribeMode::Disconnected;
    }

    /// 让订阅本身失败(宿主不可用)。
    pub fn subscribe_unavailable(&self) {
        *self.subscribe_mode.lock().unwrap() = SubscribeMode::Unavailable;
    }

    /// 往当前订阅流推一条变更信号(测试注入 `Changed`/`Resync`)。
    pub fn push_change(&self, change: AppChange) {
        if let Some(tx) = self.subscriber_tx.lock().unwrap().as_ref() {
            let _ = tx.send(change);
        }
    }

    /// 已收到过的请求快照。
    pub fn requests(&self) -> Vec<AppRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// 某个 key 被请求过几次。
    pub fn request_count(&self, key: impl Fn(&AppRequest) -> bool) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| key(r))
            .count()
    }

    fn canned_for(&self, request: &AppRequest) -> Option<Canned> {
        let key = match request {
            AppRequest::List => "list".to_string(),
            AppRequest::LaunchUrl { id } => format!("launch:{}", id.as_str()),
            AppRequest::Start { id } => format!("start:{}", id.as_str()),
            AppRequest::Stop { id } => format!("stop:{}", id.as_str()),
            AppRequest::Logs { id, .. } => format!("logs:{}", id.as_str()),
            AppRequest::Plan { .. } => "plan".to_string(),
            AppRequest::Install { .. } => "install".to_string(),
            AppRequest::Uninstall { .. } => "uninstall".to_string(),
            AppRequest::Rollback { id } => format!("rollback:{}", id.as_str()),
            AppRequest::ProbeRuntimes => "probe".to_string(),
            AppRequest::RuntimePlan { .. } => "runtime_plan".to_string(),
            AppRequest::InstallRuntime { .. } => "runtime_install".to_string(),
            AppRequest::UninstallRuntime { .. } => "runtime_uninstall".to_string(),
            _ => return None,
        };
        self.canned.lock().unwrap().get(&key).cloned()
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

fn canned_from_result(result: Result<(), AppApiError>, done: AppReply) -> Canned {
    match result {
        Ok(()) => Canned::Reply(done),
        Err(AppApiError::Host(f)) => Canned::HostFail(f.kind, f.message),
        Err(AppApiError::Transport(t)) => Canned::TransportFail(t),
    }
}

fn unexpected(request: &AppRequest) -> AppApiError {
    AppApiError::Transport(format!("fake 未预设:{request:?}"))
}

impl AppHostApi for FakeHost {
    async fn request(&self, request: AppRequest) -> Result<AppReply, AppApiError> {
        self.requests.lock().unwrap().push(request.clone());
        match self.canned_for(&request) {
            Some(Canned::Reply(reply)) => Ok(reply),
            Some(Canned::HostFail(kind, message)) => {
                Err(AppApiError::Host(AppFailure::new(kind, message)))
            }
            Some(Canned::TransportFail(message)) => Err(AppApiError::Transport(message)),
            // 未预设:显式失败,不静默成功。
            None => Err(unexpected(&request)),
        }
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<AppChange>, AppApiError> {
        match *self.subscribe_mode.lock().unwrap() {
            SubscribeMode::Unavailable => Err(AppApiError::Host(AppFailure::new(
                AppErrorKind::Unavailable,
                "fake 订阅不可用",
            ))),
            SubscribeMode::Disconnected => {
                let (tx, rx) = mpsc::unbounded_channel();
                let _ = tx.send(AppChange::Disconnected);
                drop(tx);
                Ok(rx)
            }
            SubscribeMode::Live => {
                let (tx, rx) = mpsc::unbounded_channel();
                *self.subscriber_tx.lock().unwrap() = Some(tx);
                Ok(rx)
            }
        }
    }
}

/// 便捷:造一份 [`InstallPlan`](测试用;不经过服务端重新核对,仅用于驱动 UI 状态机)。
///
/// `kind` 是运行方式(`static_web`/`node`/`python`);`trust`/`provenance` 决定产品策略门的结果。
pub fn plan(
    id: &str,
    version: (u32, u32, u32),
    kind: &str,
    trust: bytehost_apps::plan::TrustLevel,
    provenance: bytehost_apps::plan::Provenance,
) -> bytehost_apps::plan::InstallPlan {
    use bytehost_apps::id::Version;
    let version = Version::new(version.0, version.1, version.2);
    serde_json::from_value(serde_json::json!({
        "app_id": id,
        "name": format!("name-{id}"),
        "version": version.to_string(),
        "upgrading_from": null,
        "provenance": provenance,
        "trust": trust,
        "runtime_kind": kind,
        "manifest_digest": "manifest-digest",
        "source_digest": "source-digest",
        "requested": {},
        "enforcement": [],
        "permission_diff": [],
        "will_run": ["nothing"],
        "source_info": { "kind": "local_dir", "display": format!("/tmp/{id}") }
    }))
    .expect("valid InstallPlan json")
}

/// 便捷:造一份最小的 [`AppSummary`](测试用)。
pub fn summary(
    id: &str,
    observed: bytehost_apps::state::ObservedState,
    version: (u32, u32, u32),
) -> AppSummary {
    use bytehost_apps::id::Version;
    use bytehost_apps::state::DesiredState;
    AppSummary {
        id: AppId::new(id).unwrap(),
        name: format!("name-{id}"),
        version: Version::new(version.0, version.1, version.2),
        desired: DesiredState::Running,
        observed,
        url: None,
        issue: None,
        previous_version: None,
        rollback_note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_apps::state::ObservedState;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn an_unstubbed_call_fails_explicitly() {
        let fake = FakeHost::new();
        let err = rt().block_on(fake.app_list()).unwrap_err();
        assert!(matches!(err, AppApiError::Transport(_)));
        assert!(err.text().contains("未预设"), "{err}");
    }

    #[test]
    fn a_stubbed_list_succeeds() {
        let fake = FakeHost::new();
        fake.on_list(vec![summary("a", ObservedState::Running, (1, 0, 0))]);
        let apps = rt().block_on(fake.app_list()).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id.as_str(), "a");
    }

    #[test]
    fn a_host_failure_keeps_its_kind() {
        let fake = FakeHost::new();
        fake.on_list_host_fail(AppErrorKind::Unavailable, "端口被占用");
        let err = rt().block_on(fake.app_list()).unwrap_err();
        assert_eq!(err.kind(), Some(AppErrorKind::Unavailable));
        assert_eq!(err.text(), "端口被占用");
    }

    #[test]
    fn a_transport_failure_has_no_kind() {
        let fake = FakeHost::new();
        fake.on_list_transport_fail("连不上");
        let err = rt().block_on(fake.app_list()).unwrap_err();
        assert_eq!(err.kind(), None);
        assert_eq!(err.text(), "连不上");
    }

    #[test]
    fn a_disconnecting_subscription_ends_with_disconnected() {
        let fake = FakeHost::new();
        fake.subscribe_disconnects();
        let mut rx = rt().block_on(fake.subscribe()).unwrap();
        assert_eq!(rx.try_recv().unwrap(), AppChange::Disconnected);
    }

    #[test]
    fn an_unavailable_subscription_fails_like_a_dead_host() {
        let fake = FakeHost::new();
        fake.subscribe_unavailable();
        let err = rt().block_on(fake.subscribe()).unwrap_err();
        assert_eq!(err.kind(), Some(AppErrorKind::Unavailable));
    }

    #[test]
    fn a_live_subscription_delivers_pushed_changes() {
        let fake = FakeHost::new();
        let mut rx = rt().block_on(fake.subscribe()).unwrap();
        fake.push_change(AppChange::Resync);
        fake.push_change(AppChange::Changed(AppId::new("a").unwrap()));
        assert_eq!(rx.try_recv().unwrap(), AppChange::Resync);
        assert_eq!(
            rx.try_recv().unwrap(),
            AppChange::Changed(AppId::new("a").unwrap())
        );
    }

    #[test]
    fn requests_are_recorded_for_assertions() {
        let fake = FakeHost::new();
        let id = AppId::new("a").unwrap();
        fake.on_stop(&id, Ok(()));
        rt().block_on(fake.app_stop(id.clone())).unwrap();
        assert_eq!(
            fake.request_count(|r| matches!(r, AppRequest::Stop { .. })),
            1
        );
    }
}
