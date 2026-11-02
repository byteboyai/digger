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

mod management;
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
use bytehost_panel::host::{Act, PanelEffect};
use bytehost_panel::install::{
    ActKind, Effect as InstallEffect, InstallFlowState, Message as InstallMessage,
};
use bytehost_panel::key::AppKey;
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
enum UsageSink {
    /// 旧 JSON 账本(测试 / 小规模兼容路径;同步写入)。
    Json(digger_store::TaskRunner),
    /// SQLite 后台 worker(生产路径;提交不等待磁盘)。
    Worker(digger_store::UsageHandle),
}

enum UsageFinish {
    Done {
        event: digger_store::CompletionEvent,
        result: Result<bool, String>,
    },
    Pending {
        event: digger_store::CompletionEvent,
        rx: tokio::sync::oneshot::Receiver<Result<bool, String>>,
    },
}

pub struct UsageRuntime {
    sink: UsageSink,
    clock: digger_store::SystemClock,
    running: digger_store::RunningTasks,
    /// 已完成但尚未确认落盘的事件。重试只重新提交同一个事件(相同 run_id / 结束时刻 / 累计值),
    /// 不会重新安装、停止或卸载任何应用。
    failed: std::collections::BTreeMap<String, digger_store::CompletionEvent>,
    /// Registered before submission, removed only after a durable acknowledgement is handled.
    unconfirmed: std::collections::BTreeMap<String, digger_store::CompletionEvent>,
    scope_path: String,
    /// 本次进程启动的**运行命名空间**:账本按 `run_id` 幂等 upsert,而 `ticket.sequence`
    /// 每次启动从 0 重新计数。若 `run_id` 只由「动作标签 + sequence」拼成,重启后按相同顺序
    /// 做同类操作就会算出与上次相同的 id,第二次真实操作会**覆盖**第一次的历史记录(而非新增)。
    /// 故拼入一个进程内唯一、跨重启不同的前缀;同一次运行的内部重试仍复用同一 id。
    boot_id: String,
}

impl UsageRuntime {
    pub fn new(
        ledger: std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>,
        scope_path: String,
    ) -> Self {
        Self {
            sink: UsageSink::Json(digger_store::TaskRunner::new(ledger)),
            clock: digger_store::SystemClock,
            running: digger_store::RunningTasks::new(),
            failed: std::collections::BTreeMap::new(),
            unconfirmed: std::collections::BTreeMap::new(),
            scope_path,
            boot_id: new_boot_id(),
        }
    }

    pub fn new_db(handle: digger_store::UsageHandle, scope_path: String) -> Self {
        Self {
            sink: UsageSink::Worker(handle),
            clock: digger_store::SystemClock,
            running: digger_store::RunningTasks::new(),
            failed: std::collections::BTreeMap::new(),
            unconfirmed: std::collections::BTreeMap::new(),
            scope_path,
            boot_id: new_boot_id(),
        }
    }

    fn begin(
        &self,
        run_id: String,
        project_id: i64,
        project_path: String,
        agent: dozer_model::AgentKind,
        title: String,
    ) -> digger_store::RunningTask {
        match &self.sink {
            UsageSink::Json(runner) => runner.begin(run_id, project_id, project_path, agent, title),
            UsageSink::Worker(_) => {
                digger_store::begin_run(&self.clock, run_id, project_id, project_path, agent, title)
            }
        }
    }

    /// 结算一次运行。生产路径把完成事件投给后台 worker;结束时刻只在这里取一次。
    fn finish(&mut self, key: u64) -> Option<UsageFinish> {
        let run = self.running.take(key)?;
        // 诚实原则:Digger 当前无 agent 运行时,真实操作不产生 token / 回合,如实记 0;
        // `files_touched` 一期无从得知(操作由 app-host 执行,不回传文件清单),留空不伪造。
        let event =
            digger_store::complete_run(&self.clock, run, digger_store::TaskOutcome::default());
        Some(self.submit(event))
    }

    /// 提交一个不可变完成事件。失败 / 异步失败都保留同一事件供 UI 重试。
    fn submit(&mut self, event: digger_store::CompletionEvent) -> UsageFinish {
        self.unconfirmed.insert(event.run_id.clone(), event.clone());
        match &mut self.sink {
            UsageSink::Json(runner) => {
                let result = runner
                    .ledger()
                    .lock()
                    .map_err(|_| "用量账本锁被污染".to_string())
                    .and_then(|mut ledger| ledger.record(event.clone()).map_err(|e| e.to_string()));
                UsageFinish::Done { event, result }
            }
            UsageSink::Worker(handle) => match handle.record(event.clone()) {
                Ok(rx) => UsageFinish::Pending { event, rx },
                Err(e) => UsageFinish::Done {
                    event,
                    result: Err(e.to_string()),
                },
            },
        }
    }
}

/// 生成一个跨进程重启唯一、但**不泄漏用户数据**的运行命名空间前缀。
///
/// 无需引入 `uuid` 依赖:纳秒级启动时刻 + 进程 id 的组合已足以区分重启前后两次会话
/// (即使 PID 被复用,纳秒时刻也几乎不可能撞上)。仅用于账本 id 的消歧,不进日志、不作安全用途。
fn new_boot_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}-{:x}", nanos, std::process::id())
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
    SurfaceOpened(String, u64, bytehost_core::SurfaceSlot, Result<(), String>),
    OpenApp(String),
    CloseApp,
    Tick,
    NativeSynced,
    NativeOperationDone,
    RetryCleanup(String),
    /// 一次浏览器数据存储清除的结果(窗口线程 `remove_data_store` 回调回传)。
    StoreRemovalResult(String, StoreRemovalOutcome),
    /// 一次用量写入的确认结果(后台 worker 返回;失败必须可见,事件可原样重试)。
    UsagePersisted(digger_store::CompletionEvent, Result<bool, String>),
    /// 用户点「重试用量记录」:重试同一个完成事件,不重新执行业务操作。
    RetryUsage(String),
    /// 一次清除请求已提交到窗口线程(真实结果随后经 [`Message::StoreRemovalResult`] 回);无需处理。
    StoreRemovalSubmitted,
    /// 用户点「重试」:应用服务此前启动失败(`Startup::Unavailable`),重新起一次。
    /// 修复端口占用/同根锁冲突后,不必退出进程即可恢复(此前只能重开面板,且因
    /// `opened` 未复位而实际无效——复审 #7)。
    RetryService,
    /// 用户关闭顶部提示条(复审 #8)。
    DismissNotice,
    /// 从应用面板直达共享设置的应用页。
    OpenSettings,
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
    /// 最近一条要展示给用户的一次性消息(复审 #8)。状态机产生的 `Effect::Notice`
    /// 此前落进执行器兜底分支被静默丢弃(回滚失败/运行时卸载失败主要靠它暴露),
    /// 现在由组合根接住并渲染为面板顶部可关闭的提示条。
    notice: Option<Notice>,
    service_root: PathBuf,
    quitting: bool,
    selected: Option<String>,
    native_actions: Vec<SurfaceAction>,
    native_busy: bool,
    opening: bool,
    list_in_flight: bool,
    list_dirty: bool,
    last_poll: std::time::Instant,
}

/// 面板顶部展示的一条状态机瞬时消息(复审 #8)。`key` 是状态机给的稳定标识,
/// 用于去重(同一事件重复投递时只保留一条)。
#[derive(Debug, Clone)]
pub struct Notice {
    pub level: bytehost_panel::notice::NoticeLevel,
    pub text: String,
    pub _key: String,
}

impl AppsState {
    /// 在组合根启动时构造:服务以不可用占位开始,后台异步起真服务。
    pub fn new() -> Self {
        Self::with_paths(
            paths::state_dir().join("store_cleanups.json"),
            paths::bytehost_root(),
        )
    }

    /// 构造并指定清除台账文件路径(生产传产品状态目录;测试传临时路径,避免碰真实用户数据)。
    #[cfg(test)]
    pub(crate) fn with_ledger_path(ledger: PathBuf) -> Self {
        let root = ledger.with_extension("host");
        Self::with_paths(ledger, root)
    }

    fn with_paths(ledger: PathBuf, service_root: PathBuf) -> Self {
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
            notice: None,
            service_root,
            quitting: false,
            selected: None,
            native_actions: Vec::new(),
            native_busy: false,
            opening: false,
            list_in_flight: false,
            list_dirty: false,
            last_poll: std::time::Instant::now(),
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

    /// 生产接法:把完成事件投给 SQLite 后台 worker,不在窗口线程写盘。
    pub fn attach_usage_db(&mut self, handle: digger_store::UsageHandle, scope_path: String) {
        self.usage = Some(UsageRuntime::new_db(handle, scope_path));
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
        let run_id = format!(
            "digger:{}:{}:{}",
            usage.boot_id,
            action_run_label(action),
            key
        );
        let run = usage.begin(
            run_id,
            1,
            usage.scope_path.clone(),
            dozer_model::AgentKind::Unknown,
            title,
        );
        usage.running.insert(key, run);
    }

    /// 一次真实操作完成:结算成用量记录(写账本)。无登记的键(非用户动作/未接线)则不动。
    fn settle_usage_for(&mut self, key: u64) -> Option<iced::Task<Message>> {
        let usage = self.usage.as_mut()?;
        // 诚实原则:Digger 当前无 agent 运行时,真实操作不产生 token / 回合,如实记 0;
        // `files_touched` 一期无从得知(操作由 app-host 执行,不回传文件清单),留空不伪造。
        let finish = usage.finish(key)?;
        self.handle_usage_finish(finish)
    }

    fn handle_usage_finish(&mut self, finish: UsageFinish) -> Option<iced::Task<Message>> {
        match finish {
            UsageFinish::Done { event, result } => {
                self.complete_usage_submission(event, result);
                None
            }
            UsageFinish::Pending { event, rx } => Some(iced::Task::perform(
                async move {
                    let result = match rx.await {
                        Ok(result) => result,
                        Err(_) => Err("用量后台服务已停止".to_string()),
                    };
                    (event, result)
                },
                |(event, result)| Message::UsagePersisted(event, result),
            )),
        }
    }

    fn remember_failed_usage(&mut self, event: digger_store::CompletionEvent, error: &str) {
        self.push_notice(
            bytehost_panel::notice::NoticeLevel::Error,
            format!(
                "用量记录未持久化(run {}),可在提示条点「重试用量记录」;不会重新执行业务操作: {error}",
                event.run_id
            ),
            format!("usage-persist:{}", event.run_id),
        );
        if let Some(usage) = self.usage.as_mut() {
            usage
                .unconfirmed
                .insert(event.run_id.clone(), event.clone());
            usage.failed.insert(event.run_id.clone(), event);
        }
    }

    /// 重试一个此前失败的同一完成事件(相同 run_id / 结束时刻 / 累计值)。
    fn retry_usage_for(&mut self, run_id: &str) -> Option<iced::Task<Message>> {
        let usage = self.usage.as_mut()?;
        let event = usage.failed.remove(run_id)?;
        let finish = usage.submit(event);
        self.handle_usage_finish(finish)
    }

    /// 尚未确认落盘的用量事件(`run_id`, 标题),供设置页 / 顶栏展示重试入口。
    pub fn failed_usage(&self) -> Vec<(String, String)> {
        self.usage
            .as_ref()
            .map(|u| {
                u.failed
                    .values()
                    .map(|e| (e.run_id.clone(), e.title.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn complete_usage_submission(
        &mut self,
        event: digger_store::CompletionEvent,
        result: Result<bool, String>,
    ) {
        match result {
            Ok(_) => {
                if let Some(usage) = self.usage.as_mut() {
                    usage.failed.remove(&event.run_id);
                    usage.unconfirmed.remove(&event.run_id);
                }
            }
            Err(error) => self.remember_failed_usage(event, &error),
        }
    }

    /// A drained worker does not imply all writes succeeded or their acknowledgements were handled.
    /// Keep the immutable events until that fact is known before permitting a normal product exit.
    pub fn usage_persistence_error(&self) -> Option<String> {
        let usage = self.usage.as_ref()?;
        if usage.unconfirmed.is_empty() && usage.running.is_empty() {
            return None;
        }
        let failed = usage.failed.len();
        let waiting = usage.unconfirmed.len().saturating_sub(failed);
        let settling = usage.running.len();
        Some(format!(
            "还有 {} 条用量记录未确认落盘（{failed} 条失败，{waiting} 条等待确认，{settling} 条待结算）。强制退出可能丢失这些记录。",
            usage.unconfirmed.len() + settling
        ))
    }

    /// 供测试/诊断:当前在途的运行数(不接线时恒 0)。
    #[allow(dead_code)]
    pub fn usage_in_flight(&self) -> usize {
        self.usage.as_ref().map(|u| u.running.len()).unwrap_or(0)
    }

    /// 接住状态机的一条瞬时消息(复审 #8):同一 `key` 覆盖为最新,避免重复。
    /// 同时落一行 stderr(digger 一期无 Toast),但**不**把它当唯一出口。
    fn push_notice(
        &mut self,
        level: bytehost_panel::notice::NoticeLevel,
        text: String,
        key: String,
    ) {
        eprintln!("digger: 应用面板/{level:?}: {text}"); // cli-output
        self.notice = Some(Notice {
            level,
            text,
            _key: key,
        });
    }

    /// 当前要展示的瞬时消息(视图读取);`None` = 无。
    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// 用户关闭提示条。
    pub(crate) fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// 后台异步起一个进程内应用服务(不阻塞窗口)。
    fn start_service(&mut self) {
        self.exec
            .start_service(self.scope, self.panel, self.service_root.clone());
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
                if !self.opened && !self.quitting {
                    self.opened = true;
                    self.startup = Startup::Starting;
                    self.start_service();
                }
                iced::Task::none()
            }
            Message::OpenApp(id) => {
                if !self.quitting && matches!(self.startup, Startup::Ready) {
                    self.close_selected();
                    if self.store.is_pending(&id) {
                        self.push_notice(
                            bytehost_panel::notice::NoticeLevel::Error,
                            "浏览器数据尚未清除，完成清理后才能打开此应用".into(),
                            "cleanup".into(),
                        );
                    } else if let Ok(app) = bytehost_apps::id::AppId::new(id.clone())
                        && let Some(key) = digger_app_host::AppKeyId::from_app_id(&app)
                    {
                        self.selected = Some(id);
                        self.opening = true;
                        self.exec.run(
                            self.scope,
                            self.panel,
                            self.generation,
                            PanelEffect::StartApp(key),
                        );
                    }
                }
                iced::Task::none()
            }
            Message::CloseApp => {
                self.close_selected();
                iced::Task::none()
            }
            Message::Tick => {
                if !self.quitting
                    && matches!(self.startup, Startup::Ready)
                    && self.last_poll.elapsed() >= std::time::Duration::from_secs(5)
                {
                    self.last_poll = std::time::Instant::now();
                    self.refresh_list();
                }
                iced::Task::none()
            }
            Message::NativeSynced => {
                self.native_busy = false;
                iced::Task::none()
            }
            Message::NativeOperationDone => iced::Task::none(),
            Message::RetryCleanup(id) => {
                if let Err(e) = self.store.request(&id) {
                    self.push_notice(
                        bytehost_panel::notice::NoticeLevel::Error,
                        format!("清理意图未落盘,重启后可能不会继续;可再次点「重试清理」: {e}"),
                        "cleanup-persist".into(),
                    );
                }
                iced::Task::none()
            }
            Message::Install(m) => {
                if self.quitting || !matches!(self.startup, Startup::Ready) {
                    iced::Task::none()
                } else {
                    self.step(m)
                }
            }
            Message::Outcome(outcome) => self.deliver(outcome),
            Message::SourcePicked(path) => self.step(InstallMessage::SourcePicked(path)),
            // 应用 Surface 的上行事件:创建结果/导航完成/IPC。三者都不改状态机。
            // 创建失败记进控制器(持久状态,留给未来视口内联显示),不是一次性 Toast。
            Message::SurfaceOpened(app, generation, slot, result) => {
                if self.surfaces.accepts(&app, generation) {
                    self.surfaces.report_open_result(&app, result);
                } else {
                    self.native_actions.push(SurfaceAction::Destroy { slot });
                }
                iced::Task::none()
            }
            // 一次浏览器数据存储清除的结果:更新持久台账与内存重试队列;仍待重试的靠 `store_wake_task`
            // 到点再驱动,终态失败的留在台账里(设置页可见)。
            Message::StoreRemovalResult(app, outcome) => {
                if let Err(e) = self.store.settle(&app, outcome, std::time::Instant::now()) {
                    self.push_notice(
                        bytehost_panel::notice::NoticeLevel::Error,
                        format!("浏览器数据清理结果未落盘,可重试: {e}"),
                        "cleanup-settle".into(),
                    );
                }
                iced::Task::none()
            }
            // 用户显式重试:仅在启动失败(`Unavailable`)时有效。复位 `opened` 并重新
            // 起服务 + 触发探测,等价于「回到全新首次打开」的路径。
            Message::RetryService => {
                if matches!(self.startup, Startup::Unavailable(_)) {
                    self.opened = false;
                    self.update(Message::Opened)
                } else {
                    iced::Task::none()
                }
            }
            // 用户关闭顶部提示条(复审 #8)。
            Message::DismissNotice => {
                self.dismiss_notice();
                iced::Task::none()
            }
            // 清除请求已提交(真实结果异步回);无操作。
            Message::StoreRemovalSubmitted => iced::Task::none(),
            Message::UsagePersisted(event, result) => {
                self.complete_usage_submission(event, result);
                iced::Task::none()
            }
            Message::RetryUsage(run_id) => self
                .retry_usage_for(&run_id)
                .unwrap_or_else(iced::Task::none),
            // 由产品组合根拦截并打开设置窗口,状态机无动作。
            Message::OpenSettings => iced::Task::none(),
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
            InstallEffect::List | InstallEffect::HostChanged => {
                self.refresh_list();
                None
            }
            // 原生对话框:目录 / 压缩包。
            InstallEffect::PickSource => Some(Self::pick_task(true)),
            InstallEffect::PickArchive => Some(Self::pick_task(false)),
            // 状态机要展示的一次性消息(复审 #8):此前落进执行器兜底分支被静默丢弃——
            // 回滚失败、运行时卸载失败等主要靠它暴露,用户因此拿不到失败原因。接住并渲染。
            InstallEffect::Notice { level, text, key } => {
                self.push_notice(level, text, key);
                None
            }
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
                if self.selected.as_deref() == Some(&id) {
                    self.close_selected();
                }
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
            Outcome::Changed => {
                self.refresh_list();
                iced::Task::none()
            }
            Outcome::Action {
                ticket,
                app,
                act,
                result,
            } => {
                if self.requests.accept(*ticket) {
                    if self.selected.as_deref() == Some(app.as_str()) {
                        self.opening = false;
                    }
                    self.refresh_list();
                    match result {
                        Ok(())
                            if *act == Act::Start
                                && self.selected.as_deref() == Some(app.as_str())
                                && !self.quitting =>
                        {
                            if let Some(key) = digger_app_host::AppKeyId::from_app_id(app) {
                                self.exec.run(
                                    self.scope,
                                    self.panel,
                                    self.generation,
                                    PanelEffect::FetchLaunchUrl(key),
                                );
                            }
                        }
                        Err(e) => self.push_notice(
                            bytehost_panel::notice::NoticeLevel::Error,
                            e.text().into(),
                            "app-start".into(),
                        ),
                        _ => {}
                    }
                }
                iced::Task::none()
            }
            Outcome::ServiceReady(ticket, in_process) => {
                let in_process = in_process.clone();
                if self.requests.accept(*ticket) {
                    self.service_ready(in_process)
                } else {
                    iced::Task::none()
                }
            }
            Outcome::ServiceFailed(ticket, err) => {
                if !self.requests.accept(*ticket) || self.quitting {
                    return iced::Task::none();
                }
                // 启动失败:复位 `opened`,否则重开面板会被 `opened` 挡住、永不重试
                // (复审 #7)。失败原因保留在 `startup` 里供界面显示。
                self.startup = Startup::Unavailable(err.text().to_owned());
                self.opened = false;
                iced::Task::none()
            }
            Outcome::List { ticket, result } => {
                if self.requests.accept(*ticket) {
                    self.list_in_flight = false;
                    if self.list_dirty {
                        self.list_dirty = false;
                        self.refresh_list();
                    }
                    if let (Some(selected), Ok(apps)) = (&self.selected, result)
                        && !self.opening
                        && !apps.iter().any(|a| {
                            a.id.as_str() == selected
                                && matches!(
                                    a.observed,
                                    bytehost_apps::state::ObservedState::Running
                                        | bytehost_apps::state::ObservedState::Starting
                                )
                        })
                    {
                        self.close_selected();
                    }
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
                    // 一次真实操作完成:结算它登记的任务运行(后台写用量账本),再翻成状态机消息。
                    let usage = self.settle_usage_for(ticket.sequence);
                    let install = self.install_outcome(ticket.key, result.clone(), detail.clone());
                    match usage {
                        Some(task) => iced::Task::batch([task, install]),
                        None => install,
                    }
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
                if self.requests.accept(*ticket)
                    && !self.quitting
                    && self.selected.as_deref() == Some(app.as_str())
                {
                    match result {
                        // 地址到手只登记为"待创建":一期尚无应用视口几何,创建由产品在拿到视口时
                        // 调 `surface_open_pending` 触发。地址不入日志、不入任何持久状态。
                        Ok(url) => self.surfaces.set_pending_url(app.as_str(), url.clone()),
                        // 取地址失败:不残留任何凭证。
                        Err(_) => {
                            self.native_actions
                                .extend(self.surfaces.close(app.as_str()));
                            self.push_notice(
                                bytehost_panel::notice::NoticeLevel::Error,
                                "获取应用地址失败，请重新打开".into(),
                                "launch".into(),
                            );
                        }
                    }
                }
                iced::Task::none()
            }
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
        if self.opened && !self.quitting {
            self.exec.subscribe();
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
                        self.native_actions.extend(self.surfaces.close(id.as_str()));
                        if let Err(e) = self.store.request(id.as_str()) {
                            self.push_notice(
                                bytehost_panel::notice::NoticeLevel::Error,
                                format!("清理待办未落盘,重启后仍可能需要清理: {e}"),
                                "cleanup-request".into(),
                            );
                        }
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

    fn refresh_list(&mut self) {
        if self.quitting || !matches!(self.startup, Startup::Ready) {
            return;
        }
        if self.list_in_flight {
            self.list_dirty = true;
            return;
        }
        self.list_in_flight = true;
        self.exec.run(
            self.scope,
            self.panel,
            self.generation,
            PanelEffect::FetchList,
        );
    }

    pub fn close_selected(&mut self) {
        self.opening = false;
        if let Some(id) = self.selected.take() {
            if let Ok(app) = bytehost_apps::id::AppId::new(id.clone())
                && let Some(key) = digger_app_host::AppKeyId::from_app_id(&app)
            {
                self.requests.invalidate_app(key);
            }
            self.native_actions.extend(self.surfaces.close(&id));
        }
    }

    pub fn sync_native(
        &mut self,
        window: iced::window::Id,
        rect: LogicalRect,
    ) -> iced::Task<Message> {
        if self.native_busy {
            return iced::Task::none();
        }
        if let Some(id) = self.selected.clone() {
            self.native_actions
                .extend(self.surfaces.open_pending(&id, rect));
            self.native_actions.extend(self.surfaces.resize(&id, rect));
        }
        let actions = std::mem::take(&mut self.native_actions);
        if !actions.is_empty() {
            self.native_busy = true;
            let mut task = iced::Task::none();
            for action in actions {
                task = task.chain(surface_task(window, action));
            }
            // 仅在原生 Destroy 完成后消费待清队列。
            return task.chain(iced::Task::done(Message::NativeSynced));
        }
        if self.quitting {
            iced::Task::none()
        } else {
            store_removal_task(window, self.store_take_ready())
        }
    }

    pub fn prepare_shutdown(&mut self) {
        self.quitting = true;
        self.service.begin_shutdown();
        self.close_selected();
        self.native_actions.extend(self.surfaces.close_all());
    }

    pub fn native_idle(&self) -> bool {
        !self.native_busy && self.native_actions.is_empty()
    }

    pub fn shutdown(
        &self,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send + 'static {
        self.exec.shutdown()
    }

    /// 视图渲染入口。
    pub fn view(&self) -> iced::Element<'_, Message> {
        if let Some(id) = &self.selected {
            use iced::widget::{button, column, text};
            let status = self
                .surfaces
                .open_error(id)
                .unwrap_or(if self.surfaces.is_open(id) {
                    "应用内容"
                } else {
                    "正在打开应用…"
                });
            let mut content = column![
                button("返回应用管理").on_press(Message::CloseApp),
                text(id),
                text(status)
            ]
            .spacing(8);
            if let Some(notice) = &self.notice {
                content = content.push(text(&notice.text));
            }
            content.into()
        } else {
            management::view(self)
        }
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

    /// 仍在排队 / 重试中的数据存储清除数量;设置页展示进度。
    pub fn store_pending_count(&self) -> usize {
        self.store.pending_count()
    }

    /// 清理台账读不出来的原因(设置页展示;此时不允许覆盖原文件)。
    pub fn store_unavailable_reason(&self) -> Option<&str> {
        self.store.unavailable_reason()
    }

    /// 清理台账仍有未落盘的内存意图。
    pub fn store_dirty(&self) -> bool {
        self.store.dirty()
    }

    /// 最近一次清理台账持久化失败原文。
    pub fn store_last_error(&self) -> Option<&str> {
        self.store.last_error()
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

    /// 服务是否已被「应用」面板的首次可见触发启动。
    #[cfg(test)]
    pub(crate) fn opened(&self) -> bool {
        self.opened
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
            generation,
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
                |slot, ipc| {
                    use bytehost_desktop::app_webview::AppSurfaceIpc;
                    let change = match ipc {
                        AppSurfaceIpc::ZoomIn => {
                            "Math.min(3, (Number(document.documentElement.style.zoom)||1)+0.1)"
                        }
                        AppSurfaceIpc::ZoomOut => {
                            "Math.max(0.5, (Number(document.documentElement.style.zoom)||1)-0.1)"
                        }
                        AppSurfaceIpc::ZoomReset => "1",
                        // WKWebView owns focus and mouse release; no competing drag state.
                        AppSurfaceIpc::Focus | AppSurfaceIpc::MouseUp => return,
                    };
                    bytehost_desktop::evaluate_app(
                        slot,
                        &format!("document.documentElement.style.zoom={change};"),
                    );
                },
                |_slot| {},
                // Digger 一期应用无 i18n 桥:不注入脚本桥。
                false,
                move |built| match built.result {
                    Ok(()) => Message::SurfaceOpened(app.clone(), generation, slot, Ok(())),
                    Err(e) => Message::SurfaceOpened(app.clone(), generation, slot, Err(e)),
                },
            )
        }
        SurfaceAction::Resize { slot, rect } => iced::window::run(window, move |_| {
            bytehost_desktop::set_app_bounds(slot, rect);
            Message::NativeOperationDone
        }),
        SurfaceAction::Destroy { slot } => iced::window::run(window, move |_| {
            bytehost_desktop::destroy_app(slot);
            Message::NativeOperationDone
        }),
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
        let drain = iced::Task::stream(iced::futures::StreamExt::take(rx, 1));
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

    #[test]
    fn an_operation_waiting_for_its_completion_message_also_blocks_clean_exit() {
        let path = usage_ledger_path("exit-outcome-pending");
        let mut state = AppsState::with_ledger_path(temp_ledger("exit-outcome-pending"));
        state.attach_usage(shared_ledger(&path), "/scope".into());
        state.begin_usage_for(InstallAction::Install, 7, "operation".into());
        assert!(
            state
                .usage_persistence_error()
                .unwrap()
                .contains("1 条待结算")
        );
        let _ = state.settle_usage_for(7);
        assert!(state.usage_persistence_error().is_none());
    }

    #[test]
    fn a_failed_usage_submission_keeps_the_exact_event_for_retry() {
        let dir =
            std::env::temp_dir().join(format!("digger-apps-usage-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let worker = digger_store::spawn_usage_worker(dir.join("state/usage.sqlite3"), 4).unwrap();
        let handle = worker.handle();
        let mut s = AppsState::with_ledger_path(dir.join("store_cleanups.json"));
        s.attach_usage_db(handle, "/scope".into());
        s.begin_usage_for(InstallAction::Install, 7, "安装应用 notes".into());
        // Close the worker before the settle: the submission fails synchronously.
        worker.shutdown().unwrap();
        let _ = s.settle_usage_for(7);
        let before = s
            .usage
            .as_ref()
            .unwrap()
            .failed
            .values()
            .next()
            .cloned()
            .expect("the failed event is retained");
        assert_eq!(s.failed_usage().len(), 1);
        assert_eq!(s.usage_in_flight(), 0);

        // Retry must resubmit the same event (run_id, start/finish, counters) and must not
        // create a new running task or re-run the business action.
        let _ = s.retry_usage_for(&before.run_id);
        assert_eq!(s.usage_in_flight(), 0);
        let after = s
            .usage
            .as_ref()
            .unwrap()
            .failed
            .get(&before.run_id)
            .cloned()
            .expect("still failed with the same run id");
        assert_eq!(before, after);
        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(
            rows[0].0.conversation_id.starts_with("digger:")
                && rows[0].0.conversation_id.ends_with(":uninstall:11"),
            "run_id 仍含动作标签 + 键: {}",
            rows[0].0.conversation_id
        );
    }

    /// 复审 #1 回归:跨重启的**不同**操作(相同动作、相同 sequence,因 RequestTable 每次启动
    /// 从 0 计数)必须各留一条,而不是因 run_id 相同被覆盖。修复前账本按 run_id upsert,
    /// 第二次会盖掉第一次 → 只 1 条;修复后应 2 条。
    #[tokio::test]
    async fn distinct_ops_across_restarts_do_not_overwrite_each_other() {
        let path = usage_ledger_path("cross-restart");
        let scope = "/digger/scope".to_string();

        // 第一次「会话」:sequence 1。
        {
            let ledger = shared_ledger(&path);
            let mut s = AppsState::with_ledger_path(temp_ledger("cr-a"));
            s.attach_usage(ledger, scope.clone());
            s.begin_usage_for(InstallAction::Install, 1, "第一次安装".into());
            s.settle_usage_for(1);
        }
        // 重启:新进程(新的 boot_id),sequence 仍从 1 开始。
        {
            let ledger = shared_ledger(&path);
            let mut s = AppsState::with_ledger_path(temp_ledger("cr-b"));
            s.attach_usage(ledger, scope.clone());
            s.begin_usage_for(InstallAction::Install, 1, "第二次安装".into());
            s.settle_usage_for(1);
        }

        let service = digger_store::LedgerUsageService::load(&path).unwrap();
        let rows = service.usage_summary(scope, None).await.unwrap();
        assert_eq!(rows.len(), 2, "跨重启的不同操作各留一条,不被覆盖");
        let titles: Vec<_> = rows.iter().map(|r| r.0.title.clone()).collect();
        assert!(titles.contains(&"第一次安装".to_string()));
        assert!(titles.contains(&"第二次安装".to_string()));
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

    /// 复审 #7 回归:服务启动失败后必须能重试。
    ///
    /// 修复前 `Opened` 在启动前就把 `opened` 置 true 且 `ServiceFailed` 不复位,故
    /// 重开面板会被 `opened` 挡住、永不重启。修复后:`ServiceFailed` 复位 `opened`,
    /// 且 `Message::RetryService` 可在 Unavailable 下重新起服务。
    #[tokio::test]
    async fn a_failed_service_start_can_retry() {
        let ledger = temp_ledger("retry-real");
        let root = ledger.with_extension("host");
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::write(&root, "not a directory").unwrap();
        let mut s = AppsState::with_paths(ledger, root.clone());
        let mut rx = take_reply_receiver(&s.reply_receiver()).unwrap();
        let _ = s.update(Message::Opened);
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(outcome, Outcome::ServiceFailed(..)),
            "real failure: {outcome:?}"
        );
        let _ = s.update(Message::Outcome(Arc::new(outcome)));
        assert!(matches!(s.startup, Startup::Unavailable(_)));
        assert!(!s.opened);

        std::fs::remove_file(&root).unwrap();
        let _ = s.update(Message::RetryService);
        assert!(s.opened, "retry owns the in-flight startup");
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(outcome, Outcome::ServiceReady(..)),
            "real recovery: {outcome:?}"
        );
        let _ = s.update(Message::Outcome(Arc::new(outcome)));
        assert!(matches!(s.startup, Startup::Ready));
        assert!(s.list_in_flight, "success must refresh authoritative state");
        let api = s.service.api();
        let before = api.read().await.service().clone();
        let _ = s.update(Message::Opened);
        // If a duplicate genesis was dispatched it is included in shutdown's tracked tasks.
        s.prepare_shutdown();
        s.shutdown().await.unwrap();
        let after = api.read().await.service().clone();
        assert!(
            Arc::ptr_eq(&before, &after),
            "reopening cannot replace the active backend"
        );
        let restarted = InProcess::start(&root).await;
        use bytehost_client::AppHostApi;
        assert!(
            restarted.app_list().await.is_ok(),
            "shutdown releases same-root ownership"
        );
        restarted.service().shutdown_reporting().await.unwrap();
    }

    #[tokio::test]
    async fn closing_during_startup_waits_and_releases_the_root() {
        use bytehost_client::AppHostApi;
        let ledger = temp_ledger("exit-starting");
        let root = ledger.with_extension("host");
        let mut s = AppsState::with_paths(ledger, root.clone());
        let _ = s.update(Message::Opened);
        s.prepare_shutdown();
        s.shutdown().await.unwrap();
        let restarted = InProcess::start(&root).await;
        assert!(restarted.app_list().await.is_ok());
        restarted.service().shutdown_reporting().await.unwrap();
    }

    #[tokio::test]
    async fn stale_native_creation_is_destroyed_after_close() {
        let mut s = AppsState::with_ledger_path(temp_ledger("stale-native"));
        let slot = bytehost_core::SurfaceSlot(9999);
        let _ = s.update(Message::SurfaceOpened("closed".into(), 0, slot, Ok(())));
        assert_eq!(s.native_actions, vec![SurfaceAction::Destroy { slot }]);
    }

    #[tokio::test]
    async fn changes_coalesce_while_a_list_request_is_in_flight() {
        let mut s = AppsState::with_ledger_path(temp_ledger("coalesced"));
        s.startup = Startup::Ready;
        let mut rx = take_reply_receiver(&s.reply_receiver()).unwrap();
        s.refresh_list();
        s.refresh_list();
        s.refresh_list();
        assert!(s.list_dirty);
        let first = rx.recv().await.unwrap();
        let _ = s.deliver(Arc::new(first));
        assert!(!s.list_dirty);
        assert!(s.list_in_flight);
        let second = rx.recv().await.unwrap();
        let _ = s.deliver(Arc::new(second));
        assert!(!s.list_in_flight);
        assert!(
            rx.try_recv().is_err(),
            "three invalidations require only two reads"
        );
    }

    /// 复审 #8 回归:状态机产出的 `Effect::Notice` 必须被组合根接住并可见,而不是
    /// 落进执行器兜底分支被静默丢弃(回滚/运行时卸载失败主要靠它暴露)。
    #[tokio::test]
    async fn a_notice_effect_becomes_a_visible_notice() {
        use bytehost_panel::notice::NoticeLevel;

        let ledger = shared_ledger(&usage_ledger_path("notice"));
        let mut s = AppsState::with_ledger_path(temp_ledger("notice"));
        s.attach_usage(ledger, "/digger/scope".into());
        assert!(s.notice().is_none());

        // 直接派发一条 Notice effect(模拟状态机吐出的失败提示)。
        let task = s.dispatch(InstallEffect::Notice {
            level: NoticeLevel::Error,
            text: "回滚失败:磁盘已满".into(),
            key: "rollback-failed".into(),
        });
        assert!(task.is_none(), "Notice 不需要 iced 任务");
        let n = s.notice().expect("Notice 必须被接住,不能丢弃");
        assert_eq!(n.text, "回滚失败:磁盘已满");
        assert_eq!(n.level, NoticeLevel::Error);

        // 用户关闭。
        let _ = s.update(Message::DismissNotice);
        assert!(s.notice().is_none());
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
