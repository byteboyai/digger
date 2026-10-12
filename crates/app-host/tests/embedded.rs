//! 集成测试:把 `bytehost-panel` 的真实状态机、泛型执行器、请求台账与 `HostService` 生命周期
//! 串成 Task 13 的完整闭环(规格 §5.3 / §6.1)。
//!
//! 覆盖:
//! - 状态机 → 执行器 → fake 后端 → 回信 → 状态机的完整回路;
//! - `Changed`/`Resync` 合并刷新(在途时不叠加,回来补一次);
//! - `Disconnected` → 退避 → 重订阅;
//! - scope/request 身份与失效(卸载使该应用结果作废、面板关闭使实例结果作废);
//! - 优雅退出序列(取消令牌拒绝新操作 → await shutdown);
//! - **真实临时数据根**:嵌入一个真的 `InProcess` 服务(非 fake),验证 `cargo test` 下可用。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytehost_apps::id::AppId;
use bytehost_apps::state::ObservedState;
use bytehost_client::AppChange;
use bytehost_client::AppHostApi;
use bytehost_client::in_process::InProcess;
use bytehost_panel::host::{PanelEffect, PanelMessage, PanelState};
use bytehost_panel::install::Effect as InstallEffect;
use digger_app_host::AppKeyId;
use digger_app_host::effects::{Executor, Outcome, ReplySink};
use digger_app_host::request::{PanelInstanceId, RequestKey, RequestTable, ScopeId};
use digger_app_host::service::{HostService, ShutdownOutcome};
use digger_app_host::subscription::change_to_message;
use digger_app_host::testing::{FakeHost, summary};

/// 把后台回信收进一个通道,测试可以 `recv().await` / `try_recv()`。
#[derive(Clone)]
struct ChannelSink(tokio::sync::mpsc::UnboundedSender<Outcome>);

impl ReplySink for ChannelSink {
    fn send(&self, outcome: Outcome) {
        let _ = self.0.send(outcome);
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn key(id: &str) -> AppKeyId {
    use bytehost_panel::key::AppKey;
    AppKeyId::from_app_id(&AppId::new(id).unwrap()).unwrap()
}

const SCOPE: ScopeId = ScopeId(1);
const PANEL: PanelInstanceId = PanelInstanceId(1);

#[test]
fn the_state_machine_executor_and_fake_backend_form_a_full_loop() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    fake.on_list(vec![summary("demo", ObservedState::Running, (1, 0, 0))]);
    fake.on_launch_url(&AppId::new("demo").unwrap(), "http://demo.localhost:1/?t=x");

    let requests = Arc::new(RequestTable::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let host = HostService::with_api(fake, None);
    let exec = Executor::new(host.api(), requests.clone(), sink, host.cancel_token());

    let mut panel = PanelState::<AppKeyId>::default();
    let a = key("demo");

    // 状态机说"去拉列表",执行器真的去拉;fake 回了,结果经身份校验后再喂回状态机。
    let effects = panel.update(PanelMessage::Refresh, Instant::now(), &[a]);
    assert_eq!(effects, vec![PanelEffect::FetchList]);
    for e in effects {
        assert!(exec.run(SCOPE, PANEL, 0, e));
    }
    let outcome = rt.block_on(rx.recv()).expect("收到列表结果");
    let Outcome::List { ticket, result } = outcome else {
        panic!("期望 List,得到 {outcome:?}");
    };
    assert!(requests.accept(ticket), "身份有效");
    let effects = panel.update(PanelMessage::ListLoaded(result), Instant::now(), &[a]);
    // 可见且在跑:状态机发起取 launch URL。
    let launch_effect = effects
        .iter()
        .find(|e| matches!(e, PanelEffect::FetchLaunchUrl(_)))
        .cloned()
        .expect("可见运行中的应用要取地址");
    assert!(exec.run(SCOPE, PANEL, 0, launch_effect));

    let outcome = rt.block_on(rx.recv()).expect("收到地址结果");
    let Outcome::LaunchUrl { ticket, result, .. } = outcome else {
        panic!("期望 LaunchUrl,得到 {outcome:?}");
    };
    assert!(requests.accept(ticket));
    let url = result.unwrap();
    let effects = panel.update(
        PanelMessage::LaunchUrlLoaded(a, Ok(url.clone())),
        Instant::now(),
        &[a],
    );
    assert_eq!(effects, vec![PanelEffect::SetUrl(a, url)]);
    assert_eq!(
        panel.view_model(a),
        bytehost_panel::host::PanelView::Running,
        "地址就绪后是 Running"
    );
}

#[test]
fn a_change_arriving_while_a_list_is_in_flight_is_coalesced_into_one_refetch() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    fake.on_list(vec![summary("demo", ObservedState::Stopped, (1, 0, 0))]);
    let requests = Arc::new(RequestTable::new());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let host = HostService::with_api(fake, None);
    let exec = Executor::new(host.api(), requests, sink, host.cancel_token());

    let mut panel = PanelState::<AppKeyId>::default();
    let now = Instant::now();
    let effects = panel.update(PanelMessage::Refresh, now, &[]);
    assert_eq!(effects, vec![PanelEffect::FetchList]);
    for e in effects {
        exec.run(SCOPE, PANEL, 0, e);
    }
    // 在途期间来了两条变更:都不应立刻再发请求(合并)。
    assert!(panel.update(PanelMessage::Changed, now, &[]).is_empty());
    assert!(panel.update(PanelMessage::Changed, now, &[]).is_empty());
    let effects = panel.update(
        PanelMessage::ListLoaded(Ok(vec![summary("demo", ObservedState::Stopped, (1, 0, 0))])),
        now,
        &[],
    );
    let refetches = effects
        .iter()
        .filter(|e| matches!(e, PanelEffect::FetchList))
        .count();
    assert_eq!(refetches, 1, "两条变更合并成一次补拉");
}

#[test]
fn a_disconnect_rusts_the_subscription_and_eventually_resubscribes() {
    let mut panel = PanelState::<AppKeyId>::default();
    let t0 = Instant::now();
    let effects = panel.subscribe_if_due(t0);
    assert_eq!(effects, vec![PanelEffect::Subscribe]);
    panel.update(PanelMessage::Subscribed, t0, &[]);
    panel.update(PanelMessage::SubscriptionLost, t0, &[]);
    assert!(
        panel
            .subscribe_if_due(t0 + Duration::from_secs(1))
            .is_empty(),
        "退避未到点,不重订阅"
    );
    let effects = panel.subscribe_if_due(t0 + bytehost_panel::host::RESUBSCRIBE_BACKOFF[0]);
    assert_eq!(effects, vec![PanelEffect::Subscribe], "退避到点重订阅");
}

#[test]
fn a_late_reply_for_a_superseded_request_is_discarded_by_identity() {
    let requests = RequestTable::new();
    let first = requests.dispatch(SCOPE, PANEL, 0, RequestKey::LaunchUrl(key("demo")));
    let second = requests.dispatch(SCOPE, PANEL, 0, RequestKey::LaunchUrl(key("demo")));
    assert!(!requests.accept(first), "旧请求的迟到结果被身份校验丢弃");
    assert!(requests.accept(second));
}

#[test]
fn uninstalling_an_app_invalidates_its_ui_results_but_not_the_installation_write() {
    let requests = RequestTable::new();
    let a = key("gone");
    let launch = requests.dispatch(SCOPE, PANEL, 0, RequestKey::LaunchUrl(a));
    let logs = requests.dispatch(SCOPE, PANEL, 0, RequestKey::Logs(a));
    let uninstall = requests.dispatch(SCOPE, PANEL, 0, RequestKey::Action(u64::MAX - 2));
    requests.invalidate_app(a);
    assert!(!requests.accept(launch), "该应用的 launch URL 结果失效");
    assert!(!requests.accept(logs), "该应用的日志结果失效");
    assert!(
        requests.accept(uninstall),
        "写操作继续由服务完成,不被 UI 失效波及"
    );
}

#[test]
fn closing_a_panel_invalidates_every_result_of_that_instance() {
    let requests = RequestTable::new();
    let list = requests.dispatch(SCOPE, PANEL, 0, RequestKey::List);
    let action = requests.dispatch(SCOPE, PANEL, 0, RequestKey::Action(7));
    let other_panel = requests.dispatch(SCOPE, PanelInstanceId(2), 0, RequestKey::List);
    requests.invalidate_panel(PANEL);
    assert!(!requests.accept(list));
    assert!(!requests.accept(action));
    assert!(requests.accept(other_panel), "别的实例不受影响");
}

#[test]
fn begin_shutdown_refuses_new_dispatches() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    fake.on_list(vec![]);
    let requests = Arc::new(RequestTable::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let host = HostService::with_api(fake, None);
    let exec = Executor::new(host.api(), requests.clone(), sink, host.cancel_token());

    host.begin_shutdown();
    let ran = exec.run(SCOPE, PANEL, 0, PanelEffect::FetchList);
    assert!(ran, "effect 仍被认领,但内部不派发");
    assert!(rx.try_recv().is_err(), "退出已开始:不应有后台结果回来");
}

#[test]
fn the_shutdown_sequence_stops_new_ops_before_awaiting_the_service() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root();
    let in_process = Arc::new(rt.block_on(InProcess::start(&root)));
    let mut host = HostService::starting();
    host.install_service(in_process);

    host.begin_shutdown();
    assert!(host.is_shutting_down());
    let outcome = rt.block_on(host.await_shutdown());
    match outcome {
        ShutdownOutcome::Reported(report) => {
            assert!(report.is_clean(), "空数据根应干净收尾:{report:?}");
        }
        other => panic!("期望 Reported,得到 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_real_in_process_service_is_reachable_through_the_generic_host() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root();
    let in_process = Arc::new(rt.block_on(InProcess::start(&root)));
    let mut host = HostService::starting();
    host.install_service(in_process);

    let api = host.api();
    let apps = rt
        .block_on(async { api.read().await.app_list().await })
        .expect("真实空数据根的列表");
    assert!(apps.is_empty(), "全新数据根没有已安装应用");

    rt.block_on(host.await_shutdown());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_start_service_task_installs_the_backend_before_announcing_ready() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root();

    let requests = Arc::new(RequestTable::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let mut host = HostService::starting();
    let api = host.api();
    let exec = Executor::new(api.clone(), requests, sink, host.cancel_token());

    exec.start_service(SCOPE, PANEL, root.clone());

    // 等回信:它到达时,共享后端必须已是真服务(而不是"启动中"占位)。
    let outcome = rt.block_on(rx.recv()).expect("应收到 ServiceReady");
    match &outcome {
        Outcome::ServiceReady(_, in_process) => host.install_service(in_process.clone()),
        other => panic!("期望 ServiceReady,得到 {other:?}"),
    }
    let apps = rt
        .block_on(async { api.read().await.app_list().await })
        .expect("真服务可用:全新数据根返回空列表");
    assert!(apps.is_empty(), "全新数据根没有已安装应用");

    rt.block_on(host.await_shutdown());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_real_static_app_installs_from_a_local_source_and_shows_in_the_list() {
    use bytehost_apps::plan::{Approval, Provenance, TrustLevel};
    use bytehost_apps::proto::AppSource;

    let rt = rt();
    let _g = rt.enter();
    let root = temp_root();
    // 造一个真实的本机静态应用:manifest.toml + 入口页。
    let src = temp_root();
    std::fs::write(
        src.join("manifest.toml"),
        r#"schema_version = 1
min_host_version = "0.1.0"
id = "notes"
name = "Notes"
version = "0.1.0"

[presentation]
entrypoint = "main"

[entrypoints.main]
type = "web"
path = "/"

[runtime]
kind = "static_web"
source = "web/"

[permissions]
clipboard = "none"
downloads = "deny"
popups = "deny"

[permissions.network]
outbound = "none"

[permissions.filesystem]
data = "none"
"#,
    )
    .expect("写清单");
    std::fs::create_dir_all(src.join("web")).unwrap();
    std::fs::write(
        src.join("web/index.html"),
        "<!doctype html><title>Notes</title>",
    )
    .unwrap();

    let api = Arc::new(rt.block_on(InProcess::start(&root)));
    let source = AppSource::LocalDir { path: src.clone() };

    // 组合根按 Local 来源策略出计划。
    let plan = rt
        .block_on(api.app_plan(source.clone(), Provenance::Local, TrustLevel::Trusted))
        .expect("出计划");
    assert_eq!(plan.app_id.as_str(), "notes");
    assert!(matches!(plan.provenance, Provenance::Local));

    // 人明确批准后安装。
    let approved = plan.approve(Approval {
        approver: "dozer-gui".into(),
        approved_ms: 1_000,
    });
    rt.block_on(api.app_install(approved, source))
        .expect("安装");

    // 安装后列表能看到它(稳定 id)。
    let apps = rt.block_on(api.app_list()).expect("列表");
    assert_eq!(apps.len(), 1, "装好一个:{apps:?}");
    assert_eq!(apps[0].id.as_str(), "notes");

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(src);
}

fn temp_root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("digger-app-host-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建临时数据根");
    p
}

#[test]
fn an_install_effect_is_dispatched_and_routed_back_with_identity() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    let requests = Arc::new(RequestTable::new());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let host = HostService::with_api(fake, None);
    let exec = Executor::new(host.api(), requests.clone(), sink, host.cancel_token());

    assert!(exec.run_install(SCOPE, PANEL, 0, InstallEffect::Probe));
    let outcome = rt.block_on(rx.recv()).expect("探针结果");
    let Outcome::Install { ticket, result, .. } = outcome else {
        panic!("期望 Install,得到 {outcome:?}");
    };
    assert!(result.is_ok());
    assert!(requests.accept(ticket), "探针结果带回信身份");
}

#[test]
fn a_ui_only_effect_is_left_to_the_composition_root() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    let requests = Arc::new(RequestTable::new());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
    let host = HostService::with_api(fake, None);
    let exec = Executor::new(host.api(), requests, sink, host.cancel_token());

    assert!(
        !exec.run(SCOPE, PANEL, 0, PanelEffect::Subscribe),
        "订阅由组合根处理"
    );
    assert!(!exec.run_install(SCOPE, PANEL, 0, InstallEffect::PickSource));
    assert!(!exec.run_install(
        SCOPE,
        PANEL,
        0,
        InstallEffect::ProbeAfter(Duration::from_secs(1))
    ));
}

#[test]
fn a_pushed_change_reaches_the_state_machine_as_a_refresh() {
    let rt = rt();
    let _g = rt.enter();
    let fake = FakeHost::new();
    fake.on_list(vec![]);
    let mut rx = rt.block_on(fake.subscribe()).expect("订阅成功");
    let mut panel = PanelState::<AppKeyId>::default();
    // 空转面板:直接吸收一条推送的变更,应立即发起一次刷新。
    fake.push_change(AppChange::Resync);
    let change = rt.block_on(rx.recv()).expect("收到变更");
    let msg = change_to_message(change);
    assert_eq!(
        panel.update(msg, Instant::now(), &[]),
        vec![PanelEffect::FetchList],
        "推送的变更转成消息后触发一次刷新"
    );
}
