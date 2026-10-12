//! 泛型 effect 执行器:把 [`bytehost_panel`] 状态机吐出的 `Effect` 翻成对 `AppHostApi` 的异步调用。
//!
//! **泛型于后端** `A: AppHostApi`(`AppHostApi` 用 RPITIT 返回 `impl Future`,不是 dyn-safe)。
//! 同一个 [`Executor`] 既能驱动进程内嵌入的 [`InProcess`](bytehost_client::in_process::InProcess)
//! (生产,`A = InProcess`),也能驱动 [`crate::testing::FakeHost`](测试)。它是纯的:给定一个 effect
//! 与一个资源句柄,派发出一个"带回信身份的后台任务",结果包成 [`Outcome`] 交回调用方(窗口线程)。
//! **不在执行器里碰状态机。**
//!
//! 启动应用服务也走这条路: [`Executor::start_service`] 异步建 [`InProcess`],不阻塞窗口;结果
//! [`Outcome::ServiceReady`] 回喂组合根。派发前核对取消令牌:退出开始后一律拒绝新操作。

use std::sync::Arc;

use bytehost_apps::id::AppId;
use bytehost_client::in_process::InProcess;
use bytehost_client::{AppApiError, AppHostApi};
use bytehost_panel::host::PanelEffect;
use bytehost_panel::install::Effect as InstallEffect;
use bytehost_panel::key::AppKey;
use tokio::sync::RwLock;

use crate::AppKeyId;
use crate::request::{PanelInstanceId, RequestKey, RequestTable, RequestTicket, ScopeId};
use crate::service::CancelToken;

/// 一次后台调用完成后回喂给状态机的消息(带身份)。
pub enum Outcome {
    /// `FetchList` 的结果。
    List {
        ticket: RequestTicket,
        result: Result<Vec<bytehost_apps::proto::AppSummary>, AppApiError>,
    },
    /// 取 launch URL 的结果(地址含令牌,秘密:不进日志/不落盘)。
    LaunchUrl {
        ticket: RequestTicket,
        app: AppId,
        result: Result<String, AppApiError>,
    },
    /// 启动 / 停止的结果。
    Action {
        ticket: RequestTicket,
        app: AppId,
        act: bytehost_panel::host::Act,
        result: Result<(), AppApiError>,
    },
    /// 读日志末尾的结果,`(text, truncated)`。
    Logs {
        ticket: RequestTicket,
        app: AppId,
        result: Result<(String, bool), AppApiError>,
    },
    /// 安装 / 卸载 / 回滚 / 探针等"完成型"结果。
    ///
    /// `detail` 只在需要正文或区分的动作上带出(出计划返回计划正文;探针返回运行时列表;
    /// 停止/回滚/卸载带上目标应用 id 以便翻回正确的状态机消息);其余为 [`InstallDetail::None`]。
    Install {
        ticket: RequestTicket,
        result: Result<(), AppApiError>,
        detail: InstallDetail,
    },
    /// 运行时探针(`Effect::Probe`)的结果:探测列表成功时不带 `Install`。
    ///
    /// 探针是唯一"成功要带回正文、失败也要区分"的运行时动作;单列一个变体,避免把
    /// `Vec<RuntimeProbe>` 塞进只表达 `Result<(), AppApiError>` 的 `Install`。
    Runtimes {
        ticket: RequestTicket,
        result: Result<Vec<bytehost_apps::proto::RuntimeProbe>, AppApiError>,
    },
    /// 应用服务就绪(异步启动完成):组合根据此把服务装进 `HostService`。
    ServiceReady(RequestTicket, Arc<InProcess>),
    /// 应用服务启动失败(迟到的失败,不阻塞窗口)。
    ServiceFailed(RequestTicket, AppApiError),
}

/// 后台任务回信的统一出口(通常是 `proxy.send(...)`)。
pub trait ReplySink: Send + Sync + 'static {
    fn send(&self, outcome: Outcome);
}

/// `Outcome::Install` 的正文/区分:出计划带回计划,停止/回滚/卸载带回目标应用 id 与动作种类,
/// 其余完成型动作为 [`InstallDetail::None`]。
///
/// 状态机把"安装/停止/回滚/卸载"都表现为一次 `Result<(), Failure>`,但翻回消息所需的上下文不同
/// (停止/卸载要 `ActionDone(id, kind, ..)`、回滚要 `RolledBack(id, ..)`、安装要 `InstallDone`);
/// 这些上下文在这里**只编码一次**,组合根不再各自复刻。
#[derive(Debug, Clone)]
pub enum InstallDetail {
    /// 无正文,动作种类由 [`InstallAction::from_key`] 决定。
    None,
    /// 出计划:`Effect::Plan` 返回的计划正文。
    Plan(Box<bytehost_apps::plan::InstallPlan>),
    /// 出运行时计划:`Effect::RuntimePlan` 返回的计划正文。
    RuntimePlan(Box<bytehost_apps::proto::RuntimeInstallPlan>),
    /// 停止/回滚/卸载:目标应用的稳定 id(翻回 `ActionDone`/`RolledBack` 用)。
    App(String),
}

// `AppHostApi` 的实现(如 `InProcess`)未必 `Debug`;手写 `Debug` 不打印句柄本身。
impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::List { ticket, result } => f
                .debug_struct("List")
                .field("ticket", ticket)
                .field("result", result)
                .finish(),
            Outcome::LaunchUrl {
                ticket,
                app,
                result,
            } => f
                .debug_struct("LaunchUrl")
                .field("ticket", ticket)
                .field("app", app)
                .field("result", result)
                .finish(),
            Outcome::Action {
                ticket,
                app,
                act,
                result,
            } => f
                .debug_struct("Action")
                .field("ticket", ticket)
                .field("app", app)
                .field("act", act)
                .field("result", result)
                .finish(),
            Outcome::Logs {
                ticket,
                app,
                result,
            } => f
                .debug_struct("Logs")
                .field("ticket", ticket)
                .field("app", app)
                .field("result", result)
                .finish(),
            Outcome::Install { ticket, result, .. } => f
                .debug_struct("Install")
                .field("ticket", ticket)
                .field("result", result)
                .finish(),
            Outcome::Runtimes { ticket, result } => f
                .debug_struct("Runtimes")
                .field("ticket", ticket)
                .field("result", result)
                .finish(),
            Outcome::ServiceReady(ticket, _) => f
                .debug_struct("ServiceReady")
                .field("ticket", ticket)
                .field("service", &"<InProcess>")
                .finish(),
            Outcome::ServiceFailed(ticket, err) => f
                .debug_struct("ServiceFailed")
                .field("ticket", ticket)
                .field("error", err)
                .finish(),
        }
    }
}

/// 泛型执行器:`AppHostApi` 句柄 + 请求台账 + 回信出口 + 取消令牌。
pub struct Executor<A: AppHostApi + 'static> {
    api: Arc<RwLock<A>>,
    requests: Arc<RequestTable>,
    sink: Arc<dyn ReplySink>,
    cancel: CancelToken,
}

impl<A: AppHostApi + Sync + 'static> Executor<A> {
    pub fn new(
        api: Arc<RwLock<A>>,
        requests: Arc<RequestTable>,
        sink: Arc<dyn ReplySink>,
        cancel: CancelToken,
    ) -> Self {
        Self {
            api,
            requests,
            sink,
            cancel,
        }
    }

    /// 后台调用网关:退出已开始时拒绝派发(`None`);否则派发一个任务把 `fut` 的结果包成 `Outcome`。
    ///
    /// `fut` 是一个接收后端引用的异步闭包,读锁在任务内部短持有。
    fn spawn<F, Fut>(&self, f: F)
    where
        F: FnOnce(Arc<RwLock<A>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Outcome> + Send,
    {
        if self.cancel.is_cancelled() {
            return;
        }
        let api = self.api.clone();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            sink.send(f(api).await);
        });
    }

    /// 执行一条应用面板 effect。返回 `false` = 由组合根处理(SetUrl/ClearUrl/Notice/SyncRail/
    /// OpenSettingsApps/Subscribe)。
    pub fn run(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        effect: PanelEffect<AppKeyId>,
    ) -> bool {
        match effect {
            PanelEffect::FetchList => {
                let ticket = self
                    .requests
                    .dispatch(scope, panel, generation, RequestKey::List);
                self.spawn(move |api| async move {
                    let result = api.read().await.app_list().await;
                    Outcome::List { ticket, result }
                });
                true
            }
            PanelEffect::FetchLaunchUrl(slot) => {
                let ticket =
                    self.requests
                        .dispatch(scope, panel, generation, RequestKey::LaunchUrl(slot));
                let app = slot.into_app_id();
                self.spawn(move |api| async move {
                    let result = api.read().await.app_launch_url(app.clone()).await;
                    Outcome::LaunchUrl {
                        ticket,
                        app,
                        result,
                    }
                });
                true
            }
            PanelEffect::StartApp(slot) => self.run_act(
                scope,
                panel,
                generation,
                slot,
                bytehost_panel::host::Act::Start,
            ),
            PanelEffect::StopApp(slot) => self.run_act(
                scope,
                panel,
                generation,
                slot,
                bytehost_panel::host::Act::Stop,
            ),
            PanelEffect::FetchLogs(slot, max_lines) => {
                let ticket =
                    self.requests
                        .dispatch(scope, panel, generation, RequestKey::Logs(slot));
                let app = slot.into_app_id();
                self.spawn(move |api| async move {
                    let result = api.read().await.app_logs(app.clone(), max_lines).await;
                    Outcome::Logs {
                        ticket,
                        app,
                        result,
                    }
                });
                true
            }
            PanelEffect::Subscribe
            | PanelEffect::SyncRail(_)
            | PanelEffect::SetUrl(_, _)
            | PanelEffect::ClearUrl(_)
            | PanelEffect::OpenSettingsApps
            | PanelEffect::Notice { .. } => false,
        }
    }

    fn run_act(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        slot: AppKeyId,
        act: bytehost_panel::host::Act,
    ) -> bool {
        let ticket = self.requests.dispatch(
            scope,
            panel,
            generation,
            RequestKey::Action(u64::from(slot.0)),
        );
        let app = slot.into_app_id();
        self.spawn(move |api| async move {
            let result = match act {
                bytehost_panel::host::Act::Start => {
                    api.read().await.app_start(app.clone()).await.map(|_url| ())
                }
                bytehost_panel::host::Act::Stop => api.read().await.app_stop(app.clone()).await,
            };
            Outcome::Action {
                ticket,
                app,
                act,
                result,
            }
        });
        true
    }

    /// 执行一条安装流 effect。返回 `false` = 由组合根处理(原生对话框/排期/跨面板刷新/通知)。
    ///
    /// 只覆盖安装流里"要发请求"的那部分;**审批门(`install.rs` 的状态机)仍由组合根把守**——
    /// 执行器只负责把**已批准**的请求发出去并带回信身份。
    ///
    /// `plan_policy` 只在 `Effect::Plan` 时需要:状态机的 `Effect::Plan(AppSource)` 不带产品信任级别,
    /// 组合根按用户选中的来源算出 [`SourcePolicy`](crate::trust::SourcePolicy) 传进来;其余 effect 忽略它。
    pub fn run_install(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        effect: InstallEffect,
    ) -> bool {
        self.run_install_with_policy(scope, panel, generation, effect, None)
    }

    /// 同 [`Executor::run_install`],但可带出计划请求的产品策略(见上)。
    pub fn run_install_with_policy(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        effect: InstallEffect,
        plan_policy: Option<crate::trust::SourcePolicy>,
    ) -> bool {
        match effect {
            InstallEffect::Plan(source) => {
                let policy = plan_policy.unwrap_or(crate::trust::SourcePolicy::LOCAL_TRUSTED);
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(PLAN_ACTION),
                );
                self.spawn(move |api| async move {
                    let plan = api
                        .read()
                        .await
                        .app_plan(source, policy.provenance, policy.trust)
                        .await;
                    match plan {
                        Ok(plan) => Outcome::Install {
                            ticket,
                            result: Ok(()),
                            detail: InstallDetail::Plan(Box::new(plan)),
                        },
                        Err(err) => Outcome::Install {
                            ticket,
                            result: Err(err),
                            detail: InstallDetail::None,
                        },
                    }
                });
                true
            }
            InstallEffect::Probe => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(PROBE_ACTION),
                );
                self.spawn(move |api| async move {
                    let result = api.read().await.app_probe_runtimes().await;
                    Outcome::Runtimes { ticket, result }
                });
                true
            }
            InstallEffect::List => {
                let ticket = self
                    .requests
                    .dispatch(scope, panel, generation, RequestKey::List);
                self.spawn(move |api| async move {
                    let result = api.read().await.app_list().await;
                    Outcome::List { ticket, result }
                });
                true
            }
            InstallEffect::Install { approved, source } => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(INSTALL_ACTION),
                );
                self.spawn(move |api| async move {
                    let result = api.read().await.app_install(*approved, source).await;
                    Outcome::Install {
                        ticket,
                        result,
                        detail: InstallDetail::None,
                    }
                });
                true
            }
            InstallEffect::Stop(id) => match AppId::new(id.clone()) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(STOP_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_stop(app).await;
                        Outcome::Install {
                            ticket,
                            result,
                            detail: InstallDetail::App(id),
                        }
                    });
                    true
                }
                Err(_) => true,
            },
            InstallEffect::Rollback(id) => match AppId::new(id.clone()) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(ROLLBACK_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_rollback(app).await;
                        Outcome::Install {
                            ticket,
                            result,
                            detail: InstallDetail::App(id),
                        }
                    });
                    true
                }
                Err(_) => true,
            },
            InstallEffect::Uninstall(id, mode) => match AppId::new(id.clone()) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(UNINSTALL_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_uninstall(app, mode).await;
                        Outcome::Install {
                            ticket,
                            result,
                            detail: InstallDetail::App(id),
                        }
                    });
                    true
                }
                Err(_) => true,
            },
            InstallEffect::RuntimePlan(runtime) => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(RUNTIME_PLAN_ACTION),
                );
                self.spawn(move |api| async move {
                    let plan = api.read().await.app_runtime_plan(runtime).await;
                    match plan {
                        Ok(plan) => Outcome::Install {
                            ticket,
                            result: Ok(()),
                            detail: InstallDetail::RuntimePlan(Box::new(plan)),
                        },
                        Err(err) => Outcome::Install {
                            ticket,
                            result: Err(err),
                            detail: InstallDetail::None,
                        },
                    }
                });
                true
            }
            InstallEffect::InstallRuntime(plan) => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(RUNTIME_INSTALL_ACTION),
                );
                self.spawn(move |api| async move {
                    let result = api.read().await.app_install_runtime(*plan).await;
                    Outcome::Install {
                        ticket,
                        result,
                        detail: InstallDetail::None,
                    }
                });
                true
            }
            InstallEffect::UninstallRuntime(runtime, version) => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(RUNTIME_UNINSTALL_ACTION),
                );
                self.spawn(move |api| async move {
                    let result = api
                        .read()
                        .await
                        .app_uninstall_runtime(runtime, &version)
                        .await;
                    Outcome::Install {
                        ticket,
                        result,
                        detail: InstallDetail::None,
                    }
                });
                true
            }
            InstallEffect::FetchLogs(id, max_lines) => match AppId::new(id) {
                Ok(app) => {
                    let slot = AppKeyId::from_app_id(&app).expect("valid id interns");
                    let ticket =
                        self.requests
                            .dispatch(scope, panel, generation, RequestKey::Logs(slot));
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_logs(app.clone(), max_lines).await;
                        Outcome::Logs {
                            ticket,
                            app,
                            result,
                        }
                    });
                    true
                }
                Err(_) => true,
            },
            _ => false,
        }
    }
}

impl Executor<InProcess> {
    /// 异步建一个进程内应用服务并回喂 [`Outcome::ServiceReady`]。**不阻塞窗口**:
    /// 立即返回一个被 `tokio::spawn` 的后台任务;窗口照常先画出来。
    ///
    /// `root` 是产品独立的数据根(调用方给 `paths::bytehost_root()`)。`AppService::start`
    /// 本身**永不失败**——端口被占 / 目录不可用都会变成 `State::Unavailable`,经 `request` 以
    /// `Host(Unavailable)` 回,与"连不上"同形(`ServiceReady` 因此只在极端内部错误时失败)。
    pub fn start_service(&self, scope: ScopeId, panel: PanelInstanceId, root: std::path::PathBuf) {
        if self.cancel.is_cancelled() {
            return;
        }
        let ticket = self
            .requests
            .dispatch(scope, panel, 0, RequestKey::Action(GENESIS_ACTION));
        let sink = self.sink.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            let in_process = Arc::new(InProcess::start(&root).await);
            // 在 tokio 工作线程里换后端:窗口线程不能阻塞等锁。`InProcess` 不可 Clone,拿内部
            // 的 `Arc<AppService>` 重建等价实例后写入共享锁;此后所有请求走真服务。
            let rebuilt = InProcess::new(in_process.service().clone());
            *api.write().await = rebuilt;
            sink.send(Outcome::ServiceReady(ticket, in_process));
        });
    }
}

const GENESIS_ACTION: u64 = u64::MAX;
const INSTALL_ACTION: u64 = u64::MAX - 1;
const UNINSTALL_ACTION: u64 = u64::MAX - 2;
const STOP_ACTION: u64 = u64::MAX - 3;
const ROLLBACK_ACTION: u64 = u64::MAX - 4;
const PROBE_ACTION: u64 = u64::MAX - 5;
const PLAN_ACTION: u64 = u64::MAX - 6;
const RUNTIME_PLAN_ACTION: u64 = u64::MAX - 7;
const RUNTIME_INSTALL_ACTION: u64 = u64::MAX - 8;
const RUNTIME_UNINSTALL_ACTION: u64 = u64::MAX - 9;

/// 一次"完成型" `Outcome::Install` 对应的安装流请求种类。
///
/// `Outcome::Install` 不带请求种类标签(安装/停止/回滚/卸载/出计划/运行时动作共用它),组合根要把它
/// 翻回正确的状态机消息就得知道是哪种;种类编码在 [`RequestTicket::key`](crate::request::RequestKey)
/// 的 action id 里。这个分类就是那条映射,**只有一份**,组合根不再各自复刻 action id 常量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallAction {
    /// 出安装计划(`Effect::Plan`)。
    Plan,
    /// 安装 / 升级(`Effect::Install`)。
    Install,
    /// 停止(`Effect::Stop`)。
    Stop,
    /// 回滚(`Effect::Rollback`)。
    Rollback,
    /// 卸载(`Effect::Uninstall`)。
    Uninstall,
    /// 运行时探针(`Effect::Probe`;结果走 `Outcome::Runtimes`)。
    Probe,
    /// 出运行时安装计划(`Effect::RuntimePlan`)。
    RuntimePlan,
    /// 安装运行时(`Effect::InstallRuntime`)。
    RuntimeInstall,
    /// 卸载运行时(`Effect::UninstallRuntime`)。
    RuntimeUninstall,
    /// 应用服务启动(`Executor::start_service`)。
    Genesis,
}

impl InstallAction {
    /// 由请求键解出完成型动作种类;`List`/`LaunchUrl`/`Logs` 不属于这类,返回 `None`。
    pub fn from_key(key: crate::request::RequestKey) -> Option<Self> {
        let crate::request::RequestKey::Action(id) = key else {
            return None;
        };
        Some(match id {
            PLAN_ACTION => Self::Plan,
            INSTALL_ACTION => Self::Install,
            STOP_ACTION => Self::Stop,
            ROLLBACK_ACTION => Self::Rollback,
            UNINSTALL_ACTION => Self::Uninstall,
            PROBE_ACTION => Self::Probe,
            RUNTIME_PLAN_ACTION => Self::RuntimePlan,
            RUNTIME_INSTALL_ACTION => Self::RuntimeInstall,
            RUNTIME_UNINSTALL_ACTION => Self::RuntimeUninstall,
            GENESIS_ACTION => Self::Genesis,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::request::RequestKey;

    #[test]
    fn every_completion_action_round_trips_through_its_key() {
        for (id, expected) in [
            (PLAN_ACTION, InstallAction::Plan),
            (INSTALL_ACTION, InstallAction::Install),
            (STOP_ACTION, InstallAction::Stop),
            (ROLLBACK_ACTION, InstallAction::Rollback),
            (UNINSTALL_ACTION, InstallAction::Uninstall),
            (PROBE_ACTION, InstallAction::Probe),
            (RUNTIME_PLAN_ACTION, InstallAction::RuntimePlan),
            (RUNTIME_INSTALL_ACTION, InstallAction::RuntimeInstall),
            (RUNTIME_UNINSTALL_ACTION, InstallAction::RuntimeUninstall),
            (GENESIS_ACTION, InstallAction::Genesis),
        ] {
            assert_eq!(
                InstallAction::from_key(RequestKey::Action(id)),
                Some(expected)
            );
        }
    }

    #[test]
    fn list_and_per_app_keys_are_not_completion_actions() {
        assert_eq!(InstallAction::from_key(RequestKey::List), None);
        assert_eq!(InstallAction::from_key(RequestKey::Action(7)), None);
    }
}
