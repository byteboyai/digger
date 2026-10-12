//! 「应用」面板的组合根(Task 14)。
//!
//! 把与界面框架无关的 [`bytehost_panel::install::InstallFlowState`] 状态机、[`Executor`] 执行器、
//! [`RequestTable`] 身份台账,以及进程内嵌入的应用服务([`HostService<InProcess>`])串成一个 iced
//! 可驱动的产品面板。
//!
//! 职责边界(规格 §4 / §6.3):
//! - **状态机**决定"下一步发什么请求";本模块只负责把 effect 派发出去、把结果按回信身份喂回去。
//! - **产品信任策略只在 Digger 适配层**([`digger_app_host::trust`]):派发 `Effect::Plan` 时按用户选中的
//!   来源算出 [`SourcePolicy`](digger_app_host::trust::SourcePolicy) 注入请求;计划回来后经
//!   [`trust::review`](digger_app_host::trust::review) 复核,拒绝就当成流程内失败,不展示审批卡。
//! - **原生对话框**交给 iced `Task`(rfd 异步对话框,在既有 AppKit run loop 里弹);其余跨面板刷新
//!   交给宿主层。
//!
//! 后台结果经 [`ReplySink`] 投到一条 `tokio::sync::mpsc::UnboundedSender`,再由 [`subscription`]
//! 把接收端接成 iced 订阅流回到窗口线程——与 `bytehost-desktop::dispatch` 的口径一致。

mod actions;
mod approval;
mod logs;
mod management;
mod runtime;
mod store;
mod surface;

pub use store::StoreCleanup;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use bytehost_apps::proto::{AppErrorKind, AppFailure};
use bytehost_apps::registry::UninstallMode;
use bytehost_client::AppApiError;
use bytehost_client::in_process::InProcess;
use bytehost_core::LogicalRect;
use bytehost_panel::install::{
    ActKind, Effect as InstallEffect, InstallFlowState, Message as InstallMessage,
};
use bytehost_webview::StoreRemovalOutcome;

use digger_app_host::effects::{Executor, InstallAction, InstallDetail, Outcome, ReplySink};
use digger_app_host::request::{PanelInstanceId, RequestKey, RequestTable, ScopeId};
use digger_app_host::service::HostService;
use digger_app_host::trust;

use crate::paths;
use surface::{AppSurfaces, SurfaceAction};

/// 任务完成用量的**生产者运行时**(Task 20):把真实的应用操作(安装/启动/停止/卸载…)
/// 包成一次任务运行,结束时结算进落盘账本,供共享 Usage 面板查询。
///
/// - `runner` 持有账本(与面板的 `LedgerUsageService` 共享同一 `Arc<Mutex<UsageLedger>>`)。
/// - `running` 是在途运行的登记簿,键是**派发请求的 ticket sequence**(同一动作的并发实例
///   各自唯一,不撞车)。
/// - `scope_path` 是这些记录归属的作用域(Digger 一期用产品数据根,与面板查询路径对齐)。
pub struct UsageRuntime {
    runner: digger_store::TaskRunner,
    running: digger_store::RunningTasks,
    scope_path: String,
}

impl UsageRuntime {
    pub fn new(
        ledger: std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>,
        scope_path: String,
    ) -> Self {
        Self {
            runner: digger_store::TaskRunner::new(ledger),
            running: digger_store::RunningTasks::new(),
            scope_path,
        }
    }
}

/// 应用服务的启动状态:窗口先画出来,服务后台起。
#[derive(Debug, Clone, PartialEq)]
pub enum Startup {
    /// 服务正在后台启动。
    Starting,
    /// 服务就绪(此后请求一律走真实 `InProcess`)。
    Ready,
    /// 服务不可用(启动失败,或产品决定不启用);带上可操作原因。
    Unavailable(String),
}

/// 产品消息:窗口线程收到的一条「应用」面板消息。
#[derive(Clone, Debug)]
pub enum Message {
    /// 面板被激活(首次进入):触发一次探测 + 列表。
    Opened,
    /// 状态机自己的消息(经视图层转来)。
    Install(InstallMessage),
    /// 后台一次调用的结果(由回流任务投回)。`Outcome` 自带手写 `Debug`,不是 `Clone`,故包 `Arc`。
    Outcome(Arc<Outcome>),
    /// 原生"选目录/选压缩包"对话框返回;`None` = 用户取消。
    SourcePicked(Option<PathBuf>),
    /// 应用 Surface 创建结果(`Ok` = 拿到原生句柄)。
    SurfaceOpened(String, Result<(), String>),
    /// 一次浏览器数据存储清除的结果(窗口线程 `remove_data_store` 回调回传)。
    StoreRemovalResult(String, StoreRemovalOutcome),
    /// 一次清除请求已提交到窗口线程(真实结果随后经 [`Message::StoreRemovalResult`] 回);无需处理。
    StoreRemovalSubmitted,
}

/// 「应用」面板的组合根状态。
pub struct AppsState {
    flow: InstallFlowState,
    startup: Startup,
    service: HostService<InProcess>,
    exec: Executor<InProcess>,
    requests: Arc<RequestTable>,
    /// 订阅流的接收端;`subscription` 首次建立时取走,之后为 `None`。
    reply_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Outcome>>>>,
    scope: ScopeId,
    panel: PanelInstanceId,
    generation: u64,
    /// `Opened` 是否已触发过(防止同帧多次触发)。
    opened: bool,
    /// 每个应用的 launch URL → 内存 Surface 登记(Task 16);地址不落任何持久状态。
    surfaces: AppSurfaces,
    /// 卸载"连数据一起删"的浏览器数据存储清除台账 + 内存重试(Task 17);跨重启恢复。
    store: StoreCleanup,
    /// 在途卸载的应用 → 卸载模式,用于决定是否要在卸载成功后清浏览器数据。
    pending_uninstall: std::collections::HashMap<String, UninstallMode>,
    /// 任务完成用量生产者(Task 20);`None` = 未接(测试/未接线时不产生用量记录)。
    usage: Option<UsageRuntime>,
}

impl AppsState {
    /// 在组合根启动时构造:服务以不可用占位开始,后台异步起真服务。
    pub fn new() -> Self {
        Self::with_ledger_path(paths::state_dir().join("store_cleanups.json"))
    }

    /// 构造并指定清除台账文件路径(生产传产品状态目录;测试传临时路径,避免碰真实用户数据)。
    fn with_ledger_path(ledger: PathBuf) -> Self {
        let requests = Arc::new(RequestTable::new());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: Arc<dyn ReplySink> = Arc::new(ChannelSink(tx));
        let service = HostService::starting();
        let exec = Executor::new(
            service.api(),
            requests.clone(),
            sink,
            service.cancel_token(),
        );
        Self {
            flow: InstallFlowState::default(),
            startup: Startup::Starting,
            service,
            exec,
            requests,
            reply_rx: Arc::new(Mutex::new(Some(rx))),
            scope: ScopeId(1),
            panel: PanelInstanceId(1),
            generation: 0,
            opened: false,
            surfaces: AppSurfaces::new(),
            store: StoreCleanup::load(ledger),
            pending_uninstall: std::collections::HashMap::new(),
            usage: None,
        }
    }

    /// 接入任务用量生产者(Task 20):组合根传入与面板共享的账本 + 作用域路径。
    pub fn attach_usage(
        &mut self,
        ledger: std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>,
        scope_path: String,
    ) {
        self.usage = Some(UsageRuntime::new(ledger, scope_path));
    }

    /// 在派发一条**用户发起**的真实操作时登记一次任务运行(以请求 ticket sequence 为键)。
    /// 只对会产生真实副作用、且用户可感知的动作计数(见 [`is_user_invoked_action`])。
    fn begin_usage_for(&mut self, action: InstallAction, key: u64, title: String) {
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        if !is_user_invoked_action(action) {
            return;
        }
        let run_id = format!("digger:{}:{}", action_run_label(action), key);
        let run = usage.runner.begin(
            run_id,
            1,
            usage.scope_path.clone(),
            dozer_model::AgentKind::Unknown,
            title,
        );
        usage.running.insert(key, run);
    }

    /// 一次真实操作完成:结算成用量记录(写账本)。无登记的键(非用户动作/未接线)则不动。
    fn settle_usage_for(&mut self, key: u64) {
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        let Some(run) = usage.running.take(key) else {
            return;
        };
        // 诚实原则:Digger 当前无 agent 运行时,真实操作不产生 token / 回合,如实记 0;
        // `files_touched` 一期无从得知(操作由 app-host 执行,不回传文件清单),留空不伪造。
        let outcome = digger_store::TaskOutcome::default();
        if let Err(e) = usage.runner.finish(run, outcome) {
            // 写账本失败(损坏/IO):不 panic,后进入面板时会以 Unavailable 呈现。
            eprintln!("digger: 用量账本写入失败: {e}"); // cli-output
        }
    }

    /// 供测试/诊断:当前在途的运行数(不接线时恒 0)。
    #[allow(dead_code)]
    pub fn usage_in_flight(&self) -> usize {
        self.usage.as_ref().map(|u| u.running.len()).unwrap_or(0)
    }

    /// 后台异步起一个进程内应用服务(不阻塞窗口)。
    fn start_service(&mut self) {
        self.exec
            .start_service(self.scope, self.panel, paths::bytehost_root());
    }
    /// 订阅流的接收端句柄(交给 [`take_reply_receiver`] / boot 建立 iced 流)。
    pub fn reply_receiver(
        &self,
    ) -> Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Outcome>>>> {
        self.reply_rx.clone()
    }

    /// 处理一条产品消息;返回要 spawned 的 iced `Task`。
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        let task = match msg {
            Message::Opened => {
                if self.opened {
                    iced::Task::none()
                } else {
                    self.opened = true;
                    // 首次打开:异步起真服务(在 iced 运行时内 spawn);`ServiceReady` 到达后
                    // 会再喂一次 `Opened` 触发真正的探测 + 拉列表。
                    self.start_service();
                    self.step(InstallMessage::Opened)
                }
            }
            Message::Install(m) => self.step(m),
            Message::Outcome(outcome) => self.deliver(outcome),
            Message::SourcePicked(path) => self.step(InstallMessage::SourcePicked(path)),
            // 应用 Surface 的上行事件:创建结果/导航完成/IPC。三者都不改状态机。
            // 创建失败记进控制器(持久状态,留给未来视口内联显示),不是一次性 Toast。
            Message::SurfaceOpened(app, result) => {
                self.surfaces.report_open_result(&app, result);
                iced::Task::none()
            }
            // 一次浏览器数据存储清除的结果:更新持久台账与内存重试队列;仍待重试的靠 `store_wake_task`
            // 到点再驱动,终态失败的留在台账里(设置页可见)。
            Message::StoreRemovalResult(app, outcome) => {
                self.store.settle(&app, outcome, std::time::Instant::now());
                iced::Task::none()
            }
            // 清除请求已提交(真实结果异步回);无操作。
            Message::StoreRemovalSubmitted => iced::Task::none(),
        };
        // 日志查看器展开/收到新内容后:排一拍刷新,并把新内容钉到底(一次性,消费即复位)。
        iced::Task::batch([task, self.log_scroll_task(), self.log_tick_task()])
    }

    /// 喂一条消息给状态机,并把吐出的 effect 逐条派发。
    ///
    /// 各 effect 产生的 iced `Task`(原生对话框 / 延后轮询)会**合并**成一条返回;没有则 `none`。
    fn step(&mut self, msg: InstallMessage) -> iced::Task<Message> {
        let effects = self.flow.update(msg, now_ms());
        let mut tasks = Vec::new();
        for effect in effects {
            if let Some(task) = self.dispatch(effect) {
                tasks.push(task);
            }
        }
        if tasks.is_empty() {
            iced::Task::none()
        } else {
            iced::Task::batch(tasks)
        }
    }

    /// 派发一条安装流 effect。返回 `Some(task)` = 需要 iced 任务(原生对话框 / 延后轮询)。
    fn dispatch(&mut self, effect: InstallEffect) -> Option<iced::Task<Message>> {
        match effect {
            // 原生对话框:目录 / 压缩包。
            InstallEffect::PickSource => Some(Self::pick_task(true)),
            InstallEffect::PickArchive => Some(Self::pick_task(false)),
            // `Effect::Plan` 需要产品策略:按用户当前选择的来源注入来源/信任级别。
            InstallEffect::Plan(_) => {
                let policy = trust::policy_for(self.flow.source_choice);
                let _ = self.exec.run_install_with_policy(
                    self.scope,
                    self.panel,
                    self.generation,
                    effect,
                    Some(policy),
                );
                None
            }
            // 运行时安装进行中的进度轮询:延后一拍再探一次(不阻塞窗口线程)。
            InstallEffect::ProbeAfter(after) => Some(iced::Task::perform(
                async move {
                    tokio::time::sleep(after).await;
                },
                |()| Message::Install(InstallMessage::PollProbes),
            )),
            // 卸载:先记下卸载模式——状态机回来的 `ActionDone` 不带模式,而"连数据一起删"需要在
            // 卸载成功后清浏览器数据存储(Task 17)。随后照常交给执行器。
            InstallEffect::Uninstall(id, mode) => {
                self.pending_uninstall.insert(id.clone(), mode);
                let effect = InstallEffect::Uninstall(id.clone(), mode);
                let ticket = self.exec.run_install_traced(
                    self.scope,
                    self.panel,
                    self.generation,
                    effect,
                    None,
                );
                if let Some(ticket) = ticket {
                    self.begin_usage_for(
                        InstallAction::Uninstall,
                        ticket.sequence,
                        format!("卸载应用 {id}"),
                    );
                }
                None
            }
            // 其余可派发的 effect 走执行器;执行器认不了的一律忽略(通知/跨面板刷新由组合根后续接)。
            other => {
                let action = dispatched_action(&other);
                let title = dispatched_title(&other);
                let ticket = self.exec.run_install_traced(
                    self.scope,
                    self.panel,
                    self.generation,
                    other,
                    None,
                );
                if let (Some(action), Some(ticket)) = (action, ticket) {
                    self.begin_usage_for(action, ticket.sequence, title);
                }
                None
            }
        }
    }

    /// 后台结果回来:核对回信身份,按结果翻译成状态机消息喂回去。
    ///
    /// 以 `Arc<Outcome>` 传入(消息要 `Clone`),这里只借用并按需克隆携带的数据;唯一句柄
    /// (`Arc<InProcess>`)直接取出装入。
    fn deliver(&mut self, outcome: Arc<Outcome>) -> iced::Task<Message> {
        match outcome.as_ref() {
            Outcome::ServiceReady(ticket, in_process) => {
                let in_process = in_process.clone();
                if self.requests.accept(*ticket) {
                    self.service_ready(in_process)
                } else {
                    self.service.install_service(in_process);
                    self.startup = Startup::Ready;
                    iced::Task::none()
                }
            }
            Outcome::ServiceFailed(_ticket, err) => {
                self.startup = Startup::Unavailable(err.text().to_owned());
                iced::Task::none()
            }
            Outcome::List { ticket, result } => {
                if self.requests.accept(*ticket) {
                    self.step(InstallMessage::ListLoaded(
                        result.clone().map_err(|e| failure(&e)),
                    ))
                } else {
                    iced::Task::none()
                }
            }
            Outcome::Runtimes { ticket, result } => {
                if self.requests.accept(*ticket) {
                    self.step(InstallMessage::ProbesLoaded(
                        result.clone().map_err(|e| failure(&e)),
                    ))
                } else {
                    iced::Task::none()
                }
            }
            Outcome::Install {
                ticket,
                result,
                detail,
            } => {
                if self.requests.accept(*ticket) {
                    // 一次真实操作完成:结算它登记的任务运行(写用量账本),再翻成状态机消息。
                    self.settle_usage_for(ticket.sequence);
                    self.install_outcome(ticket.key, result.clone(), detail.clone())
                } else {
                    iced::Task::none()
                }
            }
            Outcome::Logs {
                ticket,
                app,
                result,
            } => {
                if self.requests.accept(*ticket) {
                    let id = app.as_str().to_owned();
                    let task = self.step(InstallMessage::LogsLoaded(
                        id.clone(),
                        result.clone().map_err(|e| failure(&e)),
                    ));
                    iced::Task::batch([task, self.log_scroll_task()])
                } else {
                    iced::Task::none()
                }
            }
            // launch URL 到手:地址是秘密,只进内存 Surface 登记,不落日志/状态。
            // 尚无应用视口时先存为"待创建";有几何时由产品调 `open_pending` 创建(见 `surface` 模块)。
            Outcome::LaunchUrl {
                ticket,
                app,
                result,
            } => {
                if self.requests.accept(*ticket) {
                    match result {
                        // 地址到手只登记为"待创建":一期尚无应用视口几何,创建由产品在拿到视口时
                        // 调 `surface_open_pending` 触发。地址不入日志、不入任何持久状态。
                        Ok(url) => self.surfaces.set_pending_url(app.as_str(), url.clone()),
                        // 取地址失败:不残留任何凭证。
                        Err(_) => {
                            let _ = self.surfaces.close(app.as_str());
                        }
                    }
                }
                iced::Task::none()
            }
            // 其余结果(动作)由后续组接。
            _ => iced::Task::none(),
        }
    }

    /// 取走"把日志钉到底部"的一次性请求,生成一条滚动任务(没有则 `none`)。
    fn log_scroll_task(&mut self) -> iced::Task<Message> {
        match self.flow.take_log_scroll() {
            Some(id) => iced::widget::operation::scroll_to(
                crate::apps::log_scroll_id(&id),
                iced::widget::operation::AbsoluteOffset {
                    x: 0.0,
                    y: f32::MAX,
                },
            ),
            None => iced::Task::none(),
        }
    }

    /// 若函数已排一拍日志刷新 `Tick`(有展开的查看器且尚无定时器),返回对应的延时任务。
    fn log_tick_task(&mut self) -> iced::Task<Message> {
        if self.flow.arm_tick() {
            iced::Task::perform(
                async {
                    tokio::time::sleep(bytehost_panel::logs::REFRESH_INTERVAL).await;
                },
                |()| Message::Install(InstallMessage::Tick),
            )
        } else {
            iced::Task::none()
        }
    }

    fn service_ready(&mut self, in_process: Arc<InProcess>) -> iced::Task<Message> {
        self.service.install_service(in_process);
        self.startup = Startup::Ready;
        if self.opened {
            self.step(InstallMessage::Opened)
        } else {
            iced::Task::none()
        }
    }

    /// 把一条完成型 `Outcome::Install` 翻成状态机消息。
    fn install_outcome(
        &mut self,
        key: RequestKey,
        result: Result<(), AppApiError>,
        detail: InstallDetail,
    ) -> iced::Task<Message> {
        match InstallAction::from_key(key) {
            Some(InstallAction::Plan) => {
                let msg = match (result, detail) {
                    (Ok(()), InstallDetail::Plan(p)) => match trust::review(&p) {
                        trust::Review::Allowed => InstallMessage::PlanLoaded(Ok(p)),
                        trust::Review::Rejected(reason) => {
                            InstallMessage::PlanLoaded(Err(failure(&rejected(reason))))
                        }
                    },
                    (Err(e), _) => InstallMessage::PlanLoaded(Err(failure(&e))),
                    (Ok(()), _) => InstallMessage::PlanLoaded(Err(failure(&internal(
                        "计划请求成功却没有计划正文",
                    )))),
                };
                self.step(msg)
            }
            Some(InstallAction::RuntimePlan) => {
                let msg = match (result, detail) {
                    (Ok(()), InstallDetail::RuntimePlan(p)) => {
                        InstallMessage::RuntimePlanLoaded(Ok(p))
                    }
                    (Err(e), _) => InstallMessage::RuntimePlanLoaded(Err(failure(&e))),
                    (Ok(()), _) => InstallMessage::RuntimePlanLoaded(Err(failure(&internal(
                        "运行时计划请求成功却没有计划正文",
                    )))),
                };
                self.step(msg)
            }
            Some(InstallAction::Install) => {
                self.step(InstallMessage::InstallDone(result.map_err(|e| failure(&e))))
            }
            Some(InstallAction::RuntimeInstall) => self.step(
                InstallMessage::RuntimeInstallStarted(result.map_err(|e| failure(&e))),
            ),
            Some(InstallAction::RuntimeUninstall) => self.step(
                InstallMessage::RuntimeUninstallDone(result.map_err(|e| failure(&e))),
            ),
            Some(InstallAction::Rollback) => {
                let InstallDetail::App(id) = detail else {
                    return iced::Task::none();
                };
                self.step(InstallMessage::RolledBack(
                    id,
                    result.map_err(|e| failure(&e)),
                ))
            }
            Some(InstallAction::Stop) => {
                let InstallDetail::App(id) = detail else {
                    return iced::Task::none();
                };
                self.step(InstallMessage::ActionDone(
                    id,
                    ActKind::Stop,
                    result.map_err(|e| failure(&e)),
                ))
            }
            Some(InstallAction::Uninstall) => {
                let InstallDetail::App(id) = detail else {
                    return iced::Task::none();
                };
                let mode = self.pending_uninstall.remove(id.as_str());
                if result.is_ok() {
                    // 卸载成功且是"连数据一起删":先撤该应用所有 Surface(webview 离池,wry 才允许清存储),
                    // 再排一次浏览器数据存储清除(落盘台账 + 内存重试;失败留可见)。
                    if mode.is_some_and(store::purges_browser_data) {
                        let _ = self.surfaces.close(id.as_str());
                        self.store.request(id.as_str());
                    }
                }
                // 卸载失败:模式已在上面取出,自然丢弃,不残留误导性的待清。
                self.step(InstallMessage::ActionDone(
                    id,
                    ActKind::Uninstall,
                    result.map_err(|e| failure(&e)),
                ))
            }
            // 探针走 `Outcome::Runtimes`;genesis 由 `deliver` 处理。
            Some(InstallAction::Probe) | Some(InstallAction::Genesis) | None => iced::Task::none(),
        }
    }

    /// 视图渲染入口。
    pub fn view(&self) -> iced::Element<'_, Message> {
        management::view(self)
    }

    /// 应用 Surface 登记(只读):产品视口据此决定渲染哪些应用、显示哪些创建失败。
    #[allow(dead_code)]
    pub fn surfaces(&self) -> &AppSurfaces {
        &self.surfaces
    }

    /// 应用停止/移除/切走:销毁其 Surface 并丢弃地址。返回产品要执行的窗口动作。
    #[allow(dead_code)]
    pub fn surface_close(&mut self, app: &str) -> Vec<SurfaceAction> {
        self.surfaces.close(app)
    }

    /// 视口几何就绪/变化:把待创建地址转成真实创建动作(换地址会先销毁旧 Surface)。
    #[allow(dead_code)]
    pub fn surface_open_pending(&mut self, app: &str, rect: LogicalRect) -> Vec<SurfaceAction> {
        self.surfaces.open_pending(app, rect)
    }

    /// 视口尺寸变化。
    #[allow(dead_code)]
    pub fn surface_resize(&mut self, app: &str, rect: LogicalRect) -> Vec<SurfaceAction> {
        self.surfaces.resize(app, rect)
    }

    /// 退出/停服务/端口重置:撤掉**所有**应用 Surface,不保留任何地址。
    #[allow(dead_code)]
    pub fn surface_close_all(&mut self) -> Vec<SurfaceAction> {
        self.surfaces.close_all()
    }

    /// 取出**现在就该清**的应用数据存储(webview 必须已离池)——纯决策,不碰窗口。
    ///
    /// 组合根在窗口线程调 [`store_removal_task`] 把它翻成真实的 `remove_data_store` 调用。返回空表示
    /// 没有可清的(要么仍在池里,要么已到重试次数上限)。公开给集成测试断言"排队/离池/取回"。
    #[allow(dead_code)]
    pub fn store_take_ready(&mut self) -> Vec<(String, [u8; 16])> {
        let now = std::time::Instant::now();
        let surfaces = &self.surfaces;
        self.store.take_ready(now, |id| surfaces.is_open(id))
    }

    /// 仍欠清除的应用(任何状态:待重试或终态失败);诊断/设置页展示。
    #[allow(dead_code)]
    pub fn store_is_pending(&self, app: &str) -> bool {
        self.store.is_pending(app)
    }

    /// 终态失败的应用数据存储清除(`(app, reason)`);设置页据此提示"重装后可能仍看到旧数据"。
    #[allow(dead_code)]
    pub fn store_failures(&self) -> Vec<(&str, &str)> {
        self.store.failures()
    }

    /// 台账落到产品状态目录下(测试断言路径用)。
    #[allow(dead_code)]
    pub fn store_ledger_path(&self) -> &std::path::Path {
        self.store.ledger_path()
    }

    /// 下一次该醒来重试清除的时刻(没有待重试则 `None`)。
    ///
    /// 窗口层据此挂一个延后任务,到点再调 [`store_removal_task`] 驱动;清除是异步的且可能被占用,
    /// 所以"排一拍再试"由窗口层负责(本层不持有窗口 id,不能自己排原生任务)。
    #[allow(dead_code)]
    pub fn store_next_wake(&self) -> Option<std::time::Duration> {
        self.store.next_wake(std::time::Instant::now())
    }

    /// 取走"地址到手、等待视口几何"的应用(产品在渲染视口时调用)。
    #[allow(dead_code)]
    pub fn take_pending_surfaces(&mut self) -> Vec<(String, String)> {
        self.surfaces.take_pending()
    }

    /// 面板查询接口(视图用)。
    pub(crate) fn flow(&self) -> &InstallFlowState {
        &self.flow
    }

    pub(crate) fn startup(&self) -> &Startup {
        &self.startup
    }

    /// 产生一条原生对话框任务(选目录 / 选压缩包),结果回 [`Message::SourcePicked`]。
    pub(crate) fn pick_task(dir: bool) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let picked = if dir {
                    rfd::AsyncFileDialog::new()
                        .set_title("选择应用目录(目录里要有 manifest.toml)")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                } else {
                    rfd::AsyncFileDialog::new()
                        .set_title("选择应用压缩包")
                        .add_filter("压缩包", &["zip", "tar.gz", "tgz"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                };
                Message::SourcePicked(picked)
            },
            |m| m,
        )
    }
}

impl Default for AppsState {
    fn default() -> Self {
        Self::new()
    }
}

/// 取出后台结果回流接收端(只在 `boot` 里取一次,交给 [`reply_stream`])。
pub fn take_reply_receiver(
    rx: &Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Outcome>>>>,
) -> Option<tokio::sync::mpsc::UnboundedReceiver<Outcome>> {
    rx.lock().ok().and_then(|mut g| g.take())
}

/// 把后台结果接收端接成一条持续到进程结束的 iced 流(经 [`iced::Task::stream`] 挂到窗口)。
///
/// 用 `Task::stream` 而非 `Subscription`:接收端是唯一的、不该被反复重建;`Task::stream` 只在
/// `boot` 里挂一次,天然没有"订阅身份必须 `Hash`"的约束。
pub fn reply_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<Outcome>,
) -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::unfold(Some(rx), |maybe| async move {
        let mut rx = maybe?;
        let next = rx.recv().await?;
        Some((Message::Outcome(Arc::new(next)), Some(rx)))
    })
}

/// 日志滚动容器在 iced 里的稳定 id(每个应用的日志查看器一个)。
pub(crate) fn log_scroll_id(app_id: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("digger.apps.logs.{app_id}"))
}

/// 把一个 [`SurfaceAction`] 翻成窗口线程的原生任务(经共享受限 builder)。
///
/// 这是 Digger 侧**唯一**碰 `bytehost_desktop::app_webview` 的地方:创建/销毁/改几何都在这里,
/// 安全策略(下载/新窗口/devtools/nonce/origin)全在共享 builder 里,Digger 不复制。
/// `Destroy`/`Resize` 在主线程同步执行;`Create` 走 `iced::window::run`。
///
/// 一期无应用视口(计划 Task 16 只做机制),`on_ipc`/`on_ready` 暂为 no-op;接入视口后改为经
/// 一条 `UnboundedSender<Message>` 把上行事件投回窗口线程(与后台结果回流同型)。
#[allow(dead_code)]
pub fn surface_task(window: iced::window::Id, action: SurfaceAction) -> iced::Task<Message> {
    match action {
        SurfaceAction::Create {
            app,
            slot,
            url,
            rect,
            ..
        } => {
            let spec = bytehost_desktop::AppSurfaceSpec {
                slot,
                url,
                rect,
                visible: true,
            };
            bytehost_desktop::build_app_child(
                window,
                spec,
                |_slot, _ipc| {},
                |_slot| {},
                move |built| match built.result {
                    Ok(()) => Message::SurfaceOpened(app.clone(), Ok(())),
                    Err(e) => Message::SurfaceOpened(app.clone(), Err(e)),
                },
            )
        }
        SurfaceAction::Resize { slot, rect } => {
            let _ = bytehost_desktop::set_app_bounds(slot, rect);
            iced::Task::none()
        }
        SurfaceAction::Destroy { slot } => {
            let _ = bytehost_desktop::destroy_app(slot);
            iced::Task::none()
        }
    }
}

/// 把一组"现在就该清"的应用数据存储翻成窗口线程的原生清除任务(Task 17)。
///
/// 组合根在窗口线程这样用:先 [`AppsState::store_take_ready`] 取出已离池的 `(app, store)`,再对本函数
/// 的返回值 batch 到当前 `Task`。每条清除经共享
/// [`bytehost_desktop::app_webview::remove_app_data_store`] 提交;真实结果由 WebKit 在事件循环里异步回,
/// 这里用一条**一次性** `futures` 通道把它接回窗口线程,产出 [`Message::StoreRemovalResult`]。
///
/// 一期 Digger 尚无应用视口,`store_take_ready` 一般返回空;真实调用归 Task 17 真人验收项。
#[allow(dead_code)]
pub fn store_removal_task(
    window: iced::window::Id,
    ready: Vec<(String, [u8; 16])>,
) -> iced::Task<Message> {
    let mut tasks = Vec::new();
    for (app, store) in ready {
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        let tx = std::sync::Mutex::new(tx);
        // 提交清除:回调里把结果送进通道(回调是 `Fn`,通道句柄可克隆发送)。
        let submit = bytehost_desktop::app_webview::remove_app_data_store(
            window,
            app,
            store,
            move |app, outcome| {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.unbounded_send(Message::StoreRemovalResult(app, outcome));
                }
            },
            || Message::StoreRemovalSubmitted,
        );
        // 把通道接成一条流:清除完成后可能还有重试,产品会在下一拍再调本函数,故这条流处理一条即结束
        // (每条清除一张通道,结果一一对应,不会串台)。
        let drain = iced::Task::stream(rx);
        tasks.push(iced::Task::batch([submit, drain]));
    }
    if tasks.is_empty() {
        iced::Task::none()
    } else {
        iced::Task::batch(tasks)
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 该动作是否算一次**用户发起的真实任务运行**(计入用量)。
///
/// 只算会产生真实副作用、用户可感知的写操作:安装/启动(安装后启动)/停止/卸载/回滚,
/// 以及运行时安装/卸载。**出计划/探针/genesis 不算**——它们是只读规划/服务启动,
/// 不对应用产生改动,也不是用户"跑的一次任务"。
fn is_user_invoked_action(action: InstallAction) -> bool {
    matches!(
        action,
        InstallAction::Install
            | InstallAction::Stop
            | InstallAction::Uninstall
            | InstallAction::Rollback
            | InstallAction::RuntimeInstall
            | InstallAction::RuntimeUninstall
    )
}

/// 动作 → `run_id` 里的稳定标签(仅用于构造幂等键,不含用户数据)。
fn action_run_label(action: InstallAction) -> &'static str {
    match action {
        InstallAction::Install => "install",
        InstallAction::Stop => "stop",
        InstallAction::Uninstall => "uninstall",
        InstallAction::Rollback => "rollback",
        InstallAction::RuntimeInstall => "runtime-install",
        InstallAction::RuntimeUninstall => "runtime-uninstall",
        InstallAction::Plan => "plan",
        InstallAction::Probe => "probe",
        InstallAction::RuntimePlan => "runtime-plan",
        InstallAction::Genesis => "genesis",
    }
}

/// 由一条 effect 判断它会成为哪种完成型动作(用于登记任务运行)。
/// `None` = 不由执行器发请求(原生对话框/排期/刷新)。
fn dispatched_action(effect: &InstallEffect) -> Option<InstallAction> {
    match effect {
        InstallEffect::Install { .. } => Some(InstallAction::Install),
        InstallEffect::Stop(_) => Some(InstallAction::Stop),
        InstallEffect::Rollback(_) => Some(InstallAction::Rollback),
        InstallEffect::Uninstall(_, _) => Some(InstallAction::Uninstall),
        InstallEffect::InstallRuntime(_) => Some(InstallAction::RuntimeInstall),
        InstallEffect::UninstallRuntime(_, _) => Some(InstallAction::RuntimeUninstall),
        // 计划/探针/genesis 是只读规划或服务启动,不计入用量。
        InstallEffect::Plan(_)
        | InstallEffect::Probe
        | InstallEffect::RuntimePlan(_)
        | InstallEffect::List => None,
        _ => None,
    }
}

/// 动作的展示标题(用量面板副行)。不含用户数据之外的秘密。
fn dispatched_title(effect: &InstallEffect) -> String {
    match effect {
        InstallEffect::Install { source, .. } => {
            format!("安装应用({})", source_label(source))
        }
        InstallEffect::Stop(id) => format!("启动/停止应用 {id}"),
        InstallEffect::Rollback(id) => format!("回滚应用 {id}"),
        InstallEffect::Uninstall(id, _) => format!("卸载应用 {id}"),
        InstallEffect::InstallRuntime(_) => "安装运行时".to_string(),
        InstallEffect::UninstallRuntime(runtime, version) => {
            format!("卸载运行时 {runtime:?} {version}")
        }
        _ => "应用操作".to_string(),
    }
}

/// 安装来源的极简标签(只有种类,不带路径/URL——URL 可能含查询串等敏感内容)。
fn source_label(source: &bytehost_apps::proto::AppSource) -> &'static str {
    use bytehost_apps::proto::AppSource;
    match source {
        AppSource::LocalDir { .. } => "本机目录",
        AppSource::Archive { .. } => "本机压缩包",
        AppSource::Url { .. } => "远程 URL",
    }
}

/// 统一失败文本(面板状态机接收 `AppApiError` 别名 `Failure`)。
fn failure(err: &AppApiError) -> AppApiError {
    AppApiError::Host(AppFailure::new(
        err.kind().unwrap_or(AppErrorKind::Internal),
        err.text(),
    ))
}

fn internal(text: &str) -> AppApiError {
    AppApiError::Host(AppFailure::new(AppErrorKind::Internal, text))
}

/// 把策略拒绝翻成流程内失败(不展示审批卡)。
fn rejected(reason: String) -> AppApiError {
    AppApiError::Host(AppFailure::new(AppErrorKind::Unsupported, reason))
}

/// 后台结果回信出口:经 `UnboundedSender` 同步投递到窗口线程。
#[derive(Clone)]
struct ChannelSink(tokio::sync::mpsc::UnboundedSender<Outcome>);

impl ReplySink for ChannelSink {
    fn send(&self, outcome: Outcome) {
        let _ = self.0.send(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_services::UsageService as _;

    fn temp_ledger(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-apps-state-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("store_cleanups.json")
    }

    /// 卸载成功且模式为「连数据一起删」:排一次浏览器数据存储清除(且此刻没有 Surface 在池里)。
    #[tokio::test]
    async fn uninstall_program_and_data_queues_a_store_purge() {
        let mut s = AppsState::with_ledger_path(temp_ledger("purge"));
        s.pending_uninstall
            .insert("notes".to_owned(), UninstallMode::ProgramAndData);
        // 走真实的完成型分支(卸载 Ok)。
        let _ = s.install_outcome(
            InstallAction::Uninstall.key(),
            Ok(()),
            InstallDetail::App("notes".to_owned()),
        );
        assert!(s.store_is_pending("notes"), "应排入清除台账");
        assert!(s.store_failures().is_empty());
        // 没有 Surface 在池里 → 立刻可取出去清,并带确定性存储标识。
        let ready = s.store_take_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, "notes");
    }

    /// 卸载成功但模式为「只删程序」:不该碰浏览器数据。
    #[tokio::test]
    async fn uninstall_program_only_does_not_touch_the_store() {
        let mut s = AppsState::with_ledger_path(temp_ledger("program"));
        s.pending_uninstall
            .insert("notes".to_owned(), UninstallMode::Program);
        let _ = s.install_outcome(
            InstallAction::Uninstall.key(),
            Ok(()),
            InstallDetail::App("notes".to_owned()),
        );
        assert!(!s.store_is_pending("notes"));
        assert!(s.store_take_ready().is_empty());
    }

    /// 卸载失败:不排清除,也不残留误导性的待清。
    #[tokio::test]
    async fn a_failed_uninstall_does_not_queue_a_purge() {
        let mut s = AppsState::with_ledger_path(temp_ledger("failed"));
        s.pending_uninstall
            .insert("notes".to_owned(), UninstallMode::ProgramAndData);
        let err = AppApiError::Host(AppFailure::new(AppErrorKind::Internal, "boom"));
        let _ = s.install_outcome(
            InstallAction::Uninstall.key(),
            Err(err),
            InstallDetail::App("notes".to_owned()),
        );
        assert!(!s.store_is_pending("notes"));
        assert!(s.pending_uninstall.is_empty(), "失败后不残留模式");
    }

    // ---- Task 20:应用操作 → 用量账本 → 共享 UsageService 的端到端接线 ----

    fn usage_ledger_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-usage-link-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("usage_ledger.json")
    }

    fn shared_ledger(path: &std::path::Path) -> Arc<std::sync::Mutex<digger_store::UsageLedger>> {
        Arc::new(std::sync::Mutex::new(
            digger_store::UsageLedger::load(path).unwrap(),
        ))
    }

    /// 一次用户发起的真实操作(begin→settle)在账本里落一条记录,经**真实**
    /// `LedgerUsageService` 在相同作用域下可查到(与面板查询同型)。
    #[tokio::test]
    async fn a_completed_app_op_becomes_a_queryable_usage_record() {
        let path = usage_ledger_path("op");
        let scope = "/digger/scope".to_string();
        let ledger = shared_ledger(&path);
        let mut s = AppsState::with_ledger_path(temp_ledger("op"));
        s.attach_usage(ledger.clone(), scope.clone());

        s.begin_usage_for(InstallAction::Install, 7, "安装应用(本机目录)".into());
        assert_eq!(s.usage_in_flight(), 1, "用户动作登记为在途");
        s.settle_usage_for(7);
        assert_eq!(s.usage_in_flight(), 0, "结算后不在途");

        let service = digger_store::LedgerUsageService::new(ledger);
        let rows = service.usage_summary(scope.clone(), None).await.unwrap();
        assert_eq!(rows.len(), 1, "面板查得到这条记录");
        assert_eq!(rows[0].1.tokens_in, 0, "无 agent 运行时,如实为 0");
        assert_eq!(rows[0].0.title, "安装应用(本机目录)");
    }

    /// 记录写盘后重启(重载账本)仍在——真实落盘,不是内存态。
    #[tokio::test]
    async fn a_completed_app_op_survives_a_restart() {
        let path = usage_ledger_path("restart");
        let scope = "/digger/scope".to_string();
        {
            let ledger = shared_ledger(&path);
            let mut s = AppsState::with_ledger_path(temp_ledger("restart"));
            s.attach_usage(ledger, scope.clone());
            s.begin_usage_for(InstallAction::Uninstall, 11, "卸载应用 notes".into());
            s.settle_usage_for(11);
        }
        // 模拟重启:从磁盘重新载入账本再查。
        let service = digger_store::LedgerUsageService::load(&path).unwrap();
        let rows = service.usage_summary(scope, None).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.conversation_id, "digger:uninstall:11");
    }

    /// 未登记的动作键 settle 是 no-op(非用户动作不产生记录);重复 settle 不重复记。
    #[tokio::test]
    async fn non_user_actions_do_not_record_and_settle_is_idempotent() {
        let path = usage_ledger_path("idem");
        let scope = "/digger/scope".to_string();
        let ledger = shared_ledger(&path);
        let mut s = AppsState::with_ledger_path(temp_ledger("idem"));
        s.attach_usage(ledger.clone(), scope.clone());

        // 只读的「出计划」不该登记在途。
        s.begin_usage_for(InstallAction::Plan, 1, "计划".into());
        assert_eq!(s.usage_in_flight(), 0, "只读动作不计入用量");
        s.settle_usage_for(1);
        assert_eq!(ledger.lock().unwrap().len(), 0);

        // 用户动作:重复 settle 第二次无登记可结算,不新增第二条。
        s.begin_usage_for(InstallAction::Stop, 2, "启动/停止应用 notes".into());
        s.settle_usage_for(2);
        s.settle_usage_for(2);
        assert_eq!(ledger.lock().unwrap().len(), 1);
    }

    /// 两个作用域的记录互不相串(scope 隔离)。
    #[tokio::test]
    async fn two_scopes_do_not_cross() {
        let path = usage_ledger_path("scopes");
        let ledger = shared_ledger(&path);
        let mut a = AppsState::with_ledger_path(temp_ledger("scope-a"));
        a.attach_usage(ledger.clone(), "/scope/a".into());
        a.begin_usage_for(InstallAction::Install, 1, "a".into());
        a.settle_usage_for(1);
        let mut b = AppsState::with_ledger_path(temp_ledger("scope-b"));
        b.attach_usage(ledger.clone(), "/scope/b".into());
        b.begin_usage_for(InstallAction::Install, 2, "b".into());
        b.settle_usage_for(2);

        let service = digger_store::LedgerUsageService::new(ledger);
        let ra = service
            .usage_summary("/scope/a".into(), None)
            .await
            .unwrap();
        let rb = service
            .usage_summary("/scope/b".into(), None)
            .await
            .unwrap();
        assert_eq!(ra.len(), 1);
        assert_eq!(rb.len(), 1);
        assert_eq!(ra[0].0.title, "a");
        assert_eq!(rb[0].0.title, "b");
    }

    /// 读失败(损坏账本)与"确实为空"必须能区分:前者 `Unavailable`,后者 `Ok(空)`。
    #[tokio::test]
    async fn read_failure_is_distinct_from_empty() {
        let path = usage_ledger_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        // 损坏:载入即失败(不伪装成空账本)。
        assert!(digger_store::LedgerUsageService::load(&path).is_err());

        // 空:全新路径给 Ok(空)。
        let empty = usage_ledger_path("empty");
        let service = digger_store::LedgerUsageService::load(&empty).unwrap();
        let rows = service
            .usage_summary("/scope/x".into(), None)
            .await
            .unwrap();
        assert!(rows.is_empty());
    }
}
