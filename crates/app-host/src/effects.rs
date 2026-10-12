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
    Install {
        ticket: RequestTicket,
        result: Result<(), AppApiError>,
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
            Outcome::Install { ticket, result } => f
                .debug_struct("Install")
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
    pub fn run_install(
        &self,
        scope: ScopeId,
        panel: PanelInstanceId,
        generation: u64,
        effect: InstallEffect,
    ) -> bool {
        match effect {
            InstallEffect::Probe => {
                let ticket = self.requests.dispatch(
                    scope,
                    panel,
                    generation,
                    RequestKey::Action(PROBE_ACTION),
                );
                self.spawn(move |api| async move {
                    // 探针结果的形状(运行时列表)由 Task 14 的安装流状态机消费;这里只保证
                    // 请求发出并保留身份——组合根拿到 `Install` 成功后再去取结果。
                    let _ = api.read().await.app_probe_runtimes().await;
                    Outcome::Install {
                        ticket,
                        result: Ok(()),
                    }
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
                    Outcome::Install { ticket, result }
                });
                true
            }
            InstallEffect::Stop(id) => match AppId::new(id) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(STOP_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_stop(app).await;
                        Outcome::Install { ticket, result }
                    });
                    true
                }
                Err(_) => true,
            },
            InstallEffect::Rollback(id) => match AppId::new(id) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(ROLLBACK_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_rollback(app).await;
                        Outcome::Install { ticket, result }
                    });
                    true
                }
                Err(_) => true,
            },
            InstallEffect::Uninstall(id, mode) => match AppId::new(id) {
                Ok(app) => {
                    let ticket = self.requests.dispatch(
                        scope,
                        panel,
                        generation,
                        RequestKey::Action(UNINSTALL_ACTION),
                    );
                    self.spawn(move |api| async move {
                        let result = api.read().await.app_uninstall(app, mode).await;
                        Outcome::Install { ticket, result }
                    });
                    true
                }
                Err(_) => true,
            },
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
        tokio::spawn(async move {
            let in_process = InProcess::start(&root).await;
            sink.send(Outcome::ServiceReady(ticket, Arc::new(in_process)));
        });
    }
}

const GENESIS_ACTION: u64 = u64::MAX;
const INSTALL_ACTION: u64 = u64::MAX - 1;
const UNINSTALL_ACTION: u64 = u64::MAX - 2;
const STOP_ACTION: u64 = u64::MAX - 3;
const ROLLBACK_ACTION: u64 = u64::MAX - 4;
const PROBE_ACTION: u64 = u64::MAX - 5;
