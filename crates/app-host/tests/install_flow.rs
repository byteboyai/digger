//! 集成测试:安装审批流程的真实闭环(Task 14;规格 §6.3)。
//!
//! 覆盖计划 Task 14 的验证要求:fake 测试覆盖**拒绝 / 取消 / 过期 / 重复批准**,并保证
//! "审批的就是展示的"、审批绑定当前这一份计划、重复点击防重、来源/内容变了要重新出计划。
//!
//! 计划请求经**产品信任策略**([`digger_app_host::trust`])注入来源与信任级别——策略是组合根在派发
//! `Effect::Plan` 时算出来的(`run_install_with_policy`),`AppHostApi` 只负责把请求发出去;结果经
//! [`RequestTable`] 身份核验后回喂状态机。这条路径与 `digger-desktop` 组合根一致。

use std::path::PathBuf;
use std::sync::Arc;

use bytehost_apps::plan::{Provenance, TrustLevel};
use bytehost_apps::proto::{AppErrorKind, AppFailure};
use bytehost_client::AppApiError;
use bytehost_panel::install::{
    ActKind, Effect as InstallEffect, Flow, InstallFlowState, Message as InstallMessage,
    SourceChoice,
};
use digger_app_host::effects::{Executor, InstallDetail, Outcome, ReplySink};
use digger_app_host::request::{PanelInstanceId, RequestTable, ScopeId};
use digger_app_host::service::HostService;
use digger_app_host::testing::{FakeHost, plan};
use digger_app_host::trust;

#[derive(Clone)]
struct ChannelSink(tokio::sync::mpsc::UnboundedSender<Outcome>);

impl ReplySink for ChannelSink {
    fn send(&self, outcome: Outcome) {
        let _ = self.0.send(outcome);
    }
}

const SCOPE: ScopeId = ScopeId(1);
const PANEL: PanelInstanceId = PanelInstanceId(1);
const NOW: u64 = 1_000;

/// 把状态机 + 执行器 + fake 后端串起来的极小组合根(与 desktop 组合根同构)。
struct Rig {
    state: InstallFlowState,
    exec: Executor<FakeHost>,
    requests: Arc<RequestTable>,
    rx: tokio::sync::mpsc::UnboundedReceiver<Outcome>,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    fn new(fake: FakeHost) -> Self {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let requests = Arc::new(RequestTable::new());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
        let host = HostService::with_api(fake, None);
        let exec = Executor::new(host.api(), requests.clone(), sink, host.cancel_token());
        Self {
            state: InstallFlowState::default(),
            exec,
            requests,
            rx,
            rt,
        }
    }

    /// 派发一条 effect,回收其结果,并**按产品策略**把它翻成回喂状态机的消息。
    ///
    /// 生产组合根也走这条:`Effect::Plan` 用 [`trust::policy_for`] 算出的策略发请求,拿回计划后
    /// 再经 [`trust::review`] 复核——拒绝就当成流程内失败,不展示审批卡。
    fn dispatch(&mut self, effect: InstallEffect) {
        let _g = self.rt.enter();
        match effect {
            InstallEffect::Plan(_) => {
                let policy = trust::policy_for(self.state.source_choice);
                if !self
                    .exec
                    .run_install_with_policy(SCOPE, PANEL, 0, effect, Some(policy))
                {
                    return;
                }
                let outcome = self.rt.block_on(self.rx.recv()).expect("计划结果");
                let Outcome::Install {
                    ticket,
                    result,
                    detail,
                } = outcome
                else {
                    panic!("期望 Install,得到 {outcome:?}");
                };
                assert!(self.requests.accept(ticket), "计划结果带回信身份");
                let msg = match (result, detail) {
                    (Ok(()), InstallDetail::Plan(p)) => match trust::review(&p) {
                        trust::Review::Allowed => InstallMessage::PlanLoaded(Ok(p)),
                        trust::Review::Rejected(reason) => {
                            InstallMessage::PlanLoaded(Err(rejected(reason)))
                        }
                    },
                    (Err(e), _) => InstallMessage::PlanLoaded(Err(e)),
                    (Ok(()), _) => panic!("计划成功却没有计划正文"),
                };
                let _ = self.state.update(msg, NOW);
            }
            InstallEffect::Probe => {
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("探针结果");
                    let Outcome::Runtimes { ticket, result } = outcome else {
                        panic!("期望 Runtimes,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self.state.update(InstallMessage::ProbesLoaded(result), NOW);
                }
            }
            InstallEffect::List => {
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("列表结果");
                    let Outcome::List { ticket, result } = outcome else {
                        panic!("期望 List,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self.state.update(InstallMessage::ListLoaded(result), NOW);
                }
            }
            InstallEffect::Install { .. } => {
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("安装结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket), "安装结果带回信身份");
                    let _ = self.state.update(InstallMessage::InstallDone(result), NOW);
                }
            }
            // 运行时计划:成功进审批卡,失败流程内报错(与生产组合根的 `install_outcome` 一致)。
            InstallEffect::RuntimePlan(_) => {
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("运行时计划结果");
                    let Outcome::Install {
                        ticket,
                        result,
                        detail,
                    } = outcome
                    else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let msg = match (result, detail) {
                        (Ok(()), InstallDetail::RuntimePlan(p)) => {
                            InstallMessage::RuntimePlanLoaded(Ok(p))
                        }
                        (Err(e), _) => InstallMessage::RuntimePlanLoaded(Err(e)),
                        (Ok(()), _) => panic!("运行时计划成功却没有正文"),
                    };
                    let _ = self.state.update(msg, NOW);
                }
            }
            // 运行时安装:结果回喂 `RuntimeInstallStarted`(成功/失败都由状态机转 Notice/失败卡)。
            InstallEffect::InstallRuntime(_) => {
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("运行时安装结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self
                        .state
                        .update(InstallMessage::RuntimeInstallStarted(result), NOW);
                }
            }
            // 回滚 / 卸载运行时:把结果回喂对应完成消息。
            InstallEffect::UninstallRuntime(runtime, version) => {
                let effect = InstallEffect::UninstallRuntime(runtime, version);
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("卸载运行时结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self
                        .state
                        .update(InstallMessage::RuntimeUninstallDone(result), NOW);
                }
            }
            // 停止 / 回滚 / 卸载应用:回喂带目标 id 的 `ActionDone` / `RolledBack`。
            InstallEffect::Stop(id) => {
                let effect = InstallEffect::Stop(id.clone());
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("停止结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self
                        .state
                        .update(InstallMessage::ActionDone(id, ActKind::Stop, result), NOW);
                }
            }
            InstallEffect::Rollback(id) => {
                let effect = InstallEffect::Rollback(id.clone());
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("回滚结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self
                        .state
                        .update(InstallMessage::RolledBack(id, result), NOW);
                }
            }
            InstallEffect::Uninstall(id, mode) => {
                let effect = InstallEffect::Uninstall(id.clone(), mode);
                if self.exec.run_install(SCOPE, PANEL, 0, effect) {
                    let outcome = self.rt.block_on(self.rx.recv()).expect("卸载结果");
                    let Outcome::Install { ticket, result, .. } = outcome else {
                        panic!("期望 Install,得到 {outcome:?}");
                    };
                    assert!(self.requests.accept(ticket));
                    let _ = self.state.update(
                        InstallMessage::ActionDone(id, ActKind::Uninstall, result),
                        NOW,
                    );
                }
            }
            // 其余(选对话框、通知、跨面板刷新、排期)由组合根处理,测试不驱动。
            _ => {
                let _ = self.exec.run_install(SCOPE, PANEL, 0, effect);
            }
        }
    }

    /// 喂一条消息给状态机,不派发 effect(用于纯 UI 意图)。
    fn send(&mut self, msg: InstallMessage) {
        let _ = self.state.update(msg, NOW);
    }

    /// 喂一条消息,并把它吐出的 effect 逐条 dispatch。
    fn step(&mut self, msg: InstallMessage) {
        let effects = self.state.update(msg, NOW);
        for effect in effects {
            self.dispatch(effect);
        }
    }
}

fn rejected(reason: String) -> AppApiError {
    AppApiError::Host(AppFailure::new(AppErrorKind::Unsupported, reason))
}

fn local_source() -> PathBuf {
    PathBuf::from("/tmp/local-app")
}

/// 本机目录来源选好路径后,计划请求会带上 Local/Trusted 策略(经 `run_install_with_policy`)。
#[test]
fn a_local_process_app_reaches_review_with_the_plans_identity_bound() {
    let fake = FakeHost::new();
    fake.on_plan(Ok(plan(
        "notes",
        (1, 0, 0),
        "python",
        TrustLevel::Trusted,
        Provenance::Local,
    )));
    let mut rig = Rig::new(fake);
    assert_eq!(rig.state.source_choice, SourceChoice::Dir);

    rig.send(InstallMessage::InstallClicked);
    assert!(matches!(rig.state.flow, Flow::Picking { .. }));
    rig.step(InstallMessage::SourcePicked(Some(local_source())));

    match &rig.state.flow {
        Flow::Reviewing { plan, .. } => {
            assert_eq!(plan.app_id.as_str(), "notes");
            assert_eq!(plan.trust, TrustLevel::Trusted);
        }
        other => panic!("期望 Reviewing,得到 {other:?}"),
    }
}

/// 重复单击"批准":只应发出一次安装,第二次不再发(防重)。
#[test]
fn duplicate_approval_only_installs_once() {
    let fake = FakeHost::new();
    fake.on_plan(Ok(plan(
        "notes",
        (1, 0, 0),
        "static_web",
        TrustLevel::Trusted,
        Provenance::Local,
    )));
    fake.on_install(Ok(()));
    let mut rig = Rig::new(fake);
    rig.send(InstallMessage::InstallClicked);
    rig.step(InstallMessage::SourcePicked(Some(local_source())));
    assert!(matches!(rig.state.flow, Flow::Reviewing { .. }));

    // 第一次批准:进入 Installing 并发一次安装请求。
    rig.step(InstallMessage::ApproveClicked);
    match &rig.state.flow {
        Flow::Idle => {}
        // 成功安装后 `InstallDone(Ok)` 把流程收成 Idle。
        other => panic!("安装成功后应回到 Idle,得到 {other:?}"),
    }
    // 第二次点批准:此时不在 Reviewing,状态机不再吐 Install effect。
    let effects = rig.state.update(InstallMessage::ApproveClicked, NOW);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, InstallEffect::Install { .. })),
        "重复批准不得再发安装:{effects:?}"
    );
}

/// 取消(关闭对话步骤):从 Reviewing 回到 Idle,不发任何安装。
#[test]
fn dismissing_a_reviewed_plan_cancels_without_installing() {
    let fake = FakeHost::new();
    fake.on_plan(Ok(plan(
        "notes",
        (1, 0, 0),
        "static_web",
        TrustLevel::Trusted,
        Provenance::Local,
    )));
    let mut rig = Rig::new(fake);
    rig.send(InstallMessage::InstallClicked);
    rig.step(InstallMessage::SourcePicked(Some(local_source())));
    assert!(matches!(rig.state.flow, Flow::Reviewing { .. }));

    let effects = rig.state.update(InstallMessage::FlowDismissed, NOW);
    assert!(effects.is_empty(), "取消不发任何 effect");
    assert_eq!(rig.state.flow, Flow::Idle);
}

/// 过期(来源已变):展示中的审批卡在切换来源后立刻作废;随后到达的旧计划结果被按当前 flow 丢弃。
#[test]
fn switching_source_expires_the_previous_review() {
    let fake = FakeHost::new();
    fake.on_plan(Ok(plan(
        "notes",
        (1, 0, 0),
        "static_web",
        TrustLevel::Trusted,
        Provenance::Local,
    )));
    let mut rig = Rig::new(fake);
    rig.send(InstallMessage::InstallClicked);
    rig.step(InstallMessage::SourcePicked(Some(local_source())));
    assert!(matches!(rig.state.flow, Flow::Reviewing { .. }));

    // 用户切换来源:审批卡清回 Idle(不能"审批的是 A 来源、装的是 B 来源")。
    rig.send(InstallMessage::SourceChoiceChanged(SourceChoice::Url));
    assert_eq!(rig.state.flow, Flow::Idle);

    // 旧计划的迟到结果:此时 flow 不是 Planning,状态机丢弃,不产生 Reviewing。
    let late = rig.state.update(
        InstallMessage::PlanLoaded(Ok(Box::new(plan(
            "notes",
            (1, 0, 0),
            "static_web",
            TrustLevel::Trusted,
            Provenance::Local,
        )))),
        NOW,
    );
    assert!(late.is_empty(), "过期的计划结果不得复活审批卡");
    assert_eq!(rig.state.flow, Flow::Idle, "流程保持 Idle");
}

/// 策略拒绝:不受信任来源的进程型应用不会进入 Reviewing,而是流程内失败。
#[test]
fn a_rejected_source_becomes_a_flow_failure_not_a_review() {
    let fake = FakeHost::new();
    fake.on_plan(Ok(plan(
        "notes",
        (1, 0, 0),
        "python",
        TrustLevel::Untrusted,
        Provenance::ThirdParty,
    )));
    let mut rig = Rig::new(fake);
    rig.send(InstallMessage::SourceChoiceChanged(SourceChoice::Url));
    rig.send(InstallMessage::UrlChanged(
        "https://example.com/a.zip".into(),
    ));
    rig.send(InstallMessage::UrlPlanClicked);
    assert!(matches!(rig.state.flow, Flow::Planning { .. }));

    // 组合根按 URL 策略发请求,拿回 Untrusted/python 计划 -> review 拒绝 -> 流程内失败。
    rig.dispatch(InstallEffect::Plan(bytehost_apps::proto::AppSource::Url {
        url: "https://example.com/a.zip".into(),
        sha256: None,
    }));

    match &rig.state.flow {
        Flow::Failed { message } => assert!(
            message.contains("static_web"),
            "拒绝原因应说明只允许 static_web:{message}"
        ),
        other => panic!("期望 Failed,得到 {other:?}"),
    }
}

/// fake 后端对计划请求失败(如清单不合法):流程内失败,带出宿主原因。
#[test]
fn a_plan_request_failure_is_shown_in_the_flow() {
    let fake = FakeHost::new();
    fake.on_plan(Err(rejected("清单不合法".into())));
    let mut rig = Rig::new(fake);
    rig.send(InstallMessage::InstallClicked);
    rig.send(InstallMessage::SourcePicked(Some(local_source())));
    assert!(matches!(rig.state.flow, Flow::Planning { .. }));
    rig.dispatch(InstallEffect::Plan(
        bytehost_apps::proto::AppSource::LocalDir {
            path: local_source(),
        },
    ));
    match &rig.state.flow {
        Flow::Failed { message } => assert!(message.contains("清单不合法"), "{message}"),
        other => panic!("期望 Failed,得到 {other:?}"),
    }
}

// ---- Task 15:运行时、回滚、卸载 ----

fn runtime_plan() -> bytehost_apps::proto::RuntimeInstallPlan {
    bytehost_apps::proto::RuntimeInstallPlan {
        runtime: bytehost_apps::proto::ManagedRuntime::Node,
        versions: vec![("node".into(), "24.21.0".into())],
        downloads: Vec::new(),
        dest: "/tmp/rt".into(),
        will_do: vec!["装 node".into()],
    }
}

/// 运行时安装:点击 → 规划 → 审批卡 → 批准 → 安装完成,全链路经执行器与 fake 后端。
#[test]
fn a_runtime_install_flows_from_click_through_approval_to_done() {
    let fake = FakeHost::new();
    fake.on_runtime_plan(Ok(runtime_plan()));
    fake.on_runtime_install(Ok(()));
    let mut rig = Rig::new(fake);

    // 点安装 → 发出 RuntimePlan effect(经执行器发请求,拿回计划进审批卡)。
    rig.step(InstallMessage::RuntimeInstallClicked(
        bytehost_apps::proto::ManagedRuntime::Node,
    ));
    match &rig.state.flow {
        Flow::RuntimeReviewing { plan } => {
            assert_eq!(plan.runtime, bytehost_apps::proto::ManagedRuntime::Node);
        }
        other => panic!("期望 RuntimeReviewing,得到 {other:?}"),
    }

    // 批准 → 发 InstallRuntime → fake 成功 → 收成 Idle。
    rig.step(InstallMessage::RuntimeApproveClicked);
    assert_eq!(rig.state.flow, Flow::Idle, "运行时安装成功后回到 Idle");
}

/// 运行时计划请求失败:不进审批卡,流程内失败(展示原因)。
#[test]
fn a_runtime_plan_failure_is_shown_in_the_flow() {
    let fake = FakeHost::new();
    fake.on_runtime_plan(Err(rejected("此平台不支持".into())));
    let mut rig = Rig::new(fake);
    rig.step(InstallMessage::RuntimeInstallClicked(
        bytehost_apps::proto::ManagedRuntime::Python,
    ));
    match &rig.state.flow {
        Flow::Failed { message } => assert!(message.contains("此平台不支持"), "{message}"),
        other => panic!("期望 Failed,得到 {other:?}"),
    }
}

/// 卸载运行时:点击 → 二次确认 → 确认,发出卸载 effect 并回 Idle。
#[test]
fn uninstalling_a_runtime_requires_confirmation() {
    let fake = FakeHost::new();
    fake.on_runtime_uninstall(Ok(()));
    let mut rig = Rig::new(fake);

    rig.send(InstallMessage::RuntimeUninstallClicked(
        bytehost_apps::proto::ManagedRuntime::Node,
        "24.21.0".into(),
    ));
    match &rig.state.flow {
        Flow::ConfirmRuntimeUninstall { runtime, version } => {
            assert_eq!(*runtime, bytehost_apps::proto::ManagedRuntime::Node);
            assert_eq!(version, "24.21.0");
        }
        other => panic!("期望 ConfirmRuntimeUninstall,得到 {other:?}"),
    }

    rig.step(InstallMessage::RuntimeUninstallConfirmed);
    assert_eq!(rig.state.flow, Flow::Idle, "卸载完成后回到 Idle");
}

/// 回滚:点击 → 二次确认 → 确认,发出回滚 effect 并回 Idle(成功后列表刷新)。
#[test]
fn rolling_back_an_app_requires_confirmation() {
    use bytehost_apps::id::Version;
    use bytehost_apps::state::{DesiredState, ObservedState};

    let fake = FakeHost::new();
    let id = bytehost_apps::id::AppId::new("notes").unwrap();
    let mut row = digger_app_host::testing::summary("notes", ObservedState::Running, (1, 1, 0));
    row.previous_version = Some(Version::new(1, 0, 0));
    row.desired = DesiredState::Running;
    fake.on_rollback(&id, Ok(()));
    let mut rig = Rig::new(fake);

    // 先加载列表,状态机才知道"上一版"是谁(否则回滚按钮无目标)。
    rig.step(InstallMessage::ListLoaded(Ok(vec![row])));

    rig.send(InstallMessage::RollbackClicked("notes".into()));
    match &rig.state.flow {
        Flow::ConfirmRollback { id, .. } => assert_eq!(id, "notes"),
        other => panic!("期望 ConfirmRollback,得到 {other:?}"),
    }

    rig.step(InstallMessage::RollbackConfirmed("notes".into()));
    assert_eq!(rig.state.flow, Flow::Idle, "回滚完成后回到 Idle");
}

/// 卸载应用:确认后发 `Uninstall`,成功后 `ActionDone` 收尾并刷新列表。
#[test]
fn uninstalling_an_app_goes_through_confirmation_and_reports_done() {
    let fake = FakeHost::new();
    fake.on_uninstall(Ok(()));
    let mut rig = Rig::new(fake);

    rig.send(InstallMessage::UninstallClicked("notes".into()));
    match &rig.state.flow {
        Flow::ConfirmUninstall { id, .. } => assert_eq!(id, "notes"),
        other => panic!("期望 ConfirmUninstall,得到 {other:?}"),
    }

    let effects = rig.state.update(
        InstallMessage::UninstallConfirmed(bytehost_apps::registry::UninstallMode::Program),
        NOW,
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, InstallEffect::Uninstall(_, _))),
        "确认卸载应发出 Uninstall effect:{effects:?}"
    );
    rig.dispatch(InstallEffect::Uninstall(
        "notes".into(),
        bytehost_apps::registry::UninstallMode::Program,
    ));
    assert_eq!(rig.state.flow, Flow::Idle);
}

/// 停止应用:发出停止 effect 并带 `ActKind::Stop` 收尾。
#[test]
fn stopping_an_app_reports_action_done() {
    let fake = FakeHost::new();
    let id = bytehost_apps::id::AppId::new("notes").unwrap();
    fake.on_stop(&id, Ok(()));
    let mut rig = Rig::new(fake);

    rig.step(InstallMessage::StopClicked("notes".into()));
    assert_eq!(
        rig.state.is_acting("notes"),
        None,
        "停止完成后不再有在途动作"
    );
}
