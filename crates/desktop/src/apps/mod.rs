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

mod approval;
mod management;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use bytehost_apps::proto::{AppErrorKind, AppFailure};
use bytehost_client::AppApiError;
use bytehost_client::in_process::InProcess;
use bytehost_panel::install::{
    Effect as InstallEffect, InstallFlowState, Message as InstallMessage,
};

use digger_app_host::effects::{Executor, InstallAction, Outcome, ReplySink};
use digger_app_host::request::{PanelInstanceId, RequestKey, RequestTable, ScopeId};
use digger_app_host::service::HostService;
use digger_app_host::trust;

use crate::paths;

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
}

impl AppsState {
    /// 在组合根启动时构造:服务以不可用占位开始,后台异步起真服务。
    pub fn new() -> Self {
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
        }
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
        match msg {
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
        }
    }

    /// 喂一条消息给状态机,并把吐出的 effect 逐条派发。
    ///
    /// 若状态机进入 `Flow::Picking`(需要原生对话框),返回对应的 pick `Task`;其余情况返回 `none`。
    fn step(&mut self, msg: InstallMessage) -> iced::Task<Message> {
        let effects = self.flow.update(msg, now_ms());
        let mut pick = iced::Task::none();
        for effect in effects {
            if let Some(task) = self.dispatch(effect) {
                pick = task;
            }
        }
        pick
    }

    /// 派发一条安装流 effect。返回 `Some(task)` = 需要弹原生对话框。
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
            // 其余可派发的 effect 走执行器;执行器认不了的一律忽略(超出 Task 14 范围)。
            other => {
                let _ = self
                    .exec
                    .run_install(self.scope, self.panel, self.generation, other);
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
            Outcome::Install {
                ticket,
                result,
                plan,
            } => {
                if self.requests.accept(*ticket) {
                    self.install_outcome(ticket.key, result.clone(), plan.clone())
                } else {
                    iced::Task::none()
                }
            }
            // 其余结果(取址/日志/动作)超出 Task 14 范围。
            _ => iced::Task::none(),
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
        plan: Option<Box<bytehost_apps::plan::InstallPlan>>,
    ) -> iced::Task<Message> {
        match InstallAction::from_key(key) {
            Some(InstallAction::Plan) => {
                let msg = match (result, plan) {
                    (Ok(()), Some(p)) => match trust::review(&p) {
                        trust::Review::Allowed => InstallMessage::PlanLoaded(Ok(p)),
                        trust::Review::Rejected(reason) => {
                            InstallMessage::PlanLoaded(Err(failure(&rejected(reason))))
                        }
                    },
                    (Err(e), _) => InstallMessage::PlanLoaded(Err(failure(&e))),
                    (Ok(()), None) => InstallMessage::PlanLoaded(Err(failure(&internal(
                        "计划请求成功却没有计划正文",
                    )))),
                };
                self.step(msg)
            }
            Some(InstallAction::Install) => {
                self.step(InstallMessage::InstallDone(result.map_err(|e| failure(&e))))
            }
            // Task 14 只接了出计划与安装;探针/停止/回滚/卸载在 Task 15。
            Some(InstallAction::Probe) => self.step(InstallMessage::ProbesLoaded(
                result.map(|()| Vec::new()).map_err(|e| failure(&e)),
            )),
            _ => iced::Task::none(),
        }
    }

    /// 视图渲染入口。
    pub fn view(&self) -> iced::Element<'_, Message> {
        management::view(self)
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

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
