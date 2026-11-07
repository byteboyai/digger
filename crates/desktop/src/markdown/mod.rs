//! Markdown 面板(ADR-013):`digger.markdown`,第一方 Builtin 面板。
//!
//! 独立性(bytehost 的设计原则):
//! - 文本、块、协议规则全在 `digger-editor`(纯库);本模块只做宿主适配——webview、几何、iced 栏。
//! - 不依赖其他面板、不依赖 `app-host` / `bytehost-apps`;`product.rs` 只持有一个 [`MarkdownPanel`]
//!   并转发它的消息与几何,和 `apps` / `usage` 的接法一致。
//! - webview 由 [`host`] 的第一方构建器建,句柄表私有,与应用 webview 永不共用一个槽。
//! - 每条上行消息带 generation:切换/重建之后迟到的旧消息被丢弃,绝不应用。
//!
//! 正常模式绑定项目内的真实文稿;SelfTest 保留独立内存示例。
//! 保存和会话转换通过页面 flush 屏障,未完成的 W1 能力见验收记录。

mod host;
mod io;
mod recovery_clock;
pub(crate) mod workbench;

pub use host::Probe;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytehost_core::LogicalRect;
use digger_editor::bridge::{Bridge, Handled, Mode, Report, StandIn};
use digger_editor::protocol::EditorOutbound;
use digger_store::writing::{LoadedDocument, WritingSnapshot};
use iced::{Element, Task, window};
use workbench::{SaveOutcome, Status as WorkbenchStatus, Workbench, WritingIdentity};

/// 面板顶栏高度:wry 子视图永远画在 iced 内容之上(ADR-001 §3),所以 webview 矩形要让出这一条。
pub const BAR_HEIGHT: f64 = 84.0;

const SAMPLE: &str = include_str!("sample.md");

/// 自检模式的环境变量:设置后用 harness 页面启动,自动跑 T1–T23 并以退出码报告。
const SELFTEST_ENV: &str = "DIGGER_EDITOR_SELFTEST";

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(u64, Result<Box<Workbench>, String>),
    CancelLoad,
    RetryProjectLoad,
    CheckMirrors,
    MirrorsChecked(u64, Result<Option<String>, String>),
    ReviewExternal,
    ExternalReviewed(
        u64,
        WritingIdentity,
        Result<digger_store::writing::ExternalReview, String>,
    ),
    CloseExternalReview,
    ResolveExternal(bool),
    ExternalResolved(
        u64,
        WritingIdentity,
        Result<Vec<digger_store::writing::Document>, String>,
    ),
    ManageDocuments(bool),
    TransitionSave,
    TransitionKeep,
    TransitionCancel,
    HostTransition(u64),
    HostTransitionReady(u64, Result<(), String>),
    TransitionPersisted(u64, WritingIdentity, u64, Result<(), String>),
    /// 页面发来的原始消息,带它所属 webview 的 generation。
    Page(u64, String),
    Built(u64, Result<(), String>),
    /// 用户点「重试」:失败后真实重建一个新 generation。
    Retry,
    /// 生命周期自检的探针结果。
    Probed(Probe),
    /// 顶栏按钮:替身 Agent(仅自检模式显示)。
    StandIn(StandIn),
    /// 新建文稿表单。
    NewTitleChanged(String),
    NewPathChanged(String),
    ImportPathChanged(String),
    Create,
    Import,
    OpenFile(PathBuf),
    /// 从项目内已导入文稿列表切换。
    OpenDocument(digger_store::writing::DocumentId),
    /// 显式保存(Cmd/Ctrl-S 与顶栏按钮同一条路径)。
    Save,
    /// 恢复草稿:显式接受为一次 Recovery Change。
    AcceptRecovery,
    /// 落后/冲突草稿另存为副本,不覆盖 Official State。
    RecoverySaveCopy,
    DiscardRecovery,
    RecoveryDiscarded(u64, WritingIdentity, Result<(), String>),
    ReviewRecovery(bool),
    /// 编辑后 500ms 的一次自动恢复草稿写入(有界,不阻塞页面 ACK)。
    RecoveryTick,
    /// 后台保存完成;history 只在失败时归还。
    SaveFinished(
        WritingIdentity,
        Result<SaveOutcome, String>,
        Vec<(u64, u64, u64)>,
    ),
    /// 后台恢复草稿写入完成。
    RecoveryFinished(u64, WritingIdentity, Result<(), String>),
    /// 页面 flush 屏障超时。
    FlushTimeout(u64),
    Done,
}

/// 编辑器 webview 的创建状态。可见性 / 几何独立保存在 `shown` / `bounds`,不参与这个状态机。
#[derive(Debug, Clone, PartialEq)]
pub enum CreationState {
    /// 还没有创建过。
    Idle,
    /// 正在创建某个 generation。
    Building { generation: u64 },
    /// 该 generation 已创建,等待页面 ready。
    Ready { generation: u64 },
    /// 该 generation 创建失败;可点「重试」或切走再回来。
    Failed { generation: u64, error: String },
    /// 退出时销毁。
    Closed,
}

#[derive(Debug, Clone)]
enum Transition {
    Host(u64),
    Document(digger_store::writing::DocumentId),
    Create,
    Import,
    RecoveryCopy,
    AcceptRecovery,
    File(PathBuf),
}

pub struct MarkdownPanel {
    io: io::IoExecutor,
    recovery_clock: recovery_clock::RecoveryClock,
    external_review: Option<digger_store::writing::ExternalReview>,
    mirror_operation: u64,
    mirror_busy: bool,
    last_mirror_check: std::time::Instant,
    load_ticket: u64,
    loading: bool,
    requested_project: Option<PathBuf>,
    retired_generation: Option<u64>,
    reviewing_recovery: bool,
    managing_documents: bool,
    transition_choice: bool,
    save_then_transition: bool,
    pending_save: Option<WritingIdentity>,
    pending_transition: Option<Transition>,
    transition_generation: u64,
    transition_flushed: bool,
    prepared_transition: Option<(WritingIdentity, u64)>,
    bridge: Bridge,
    allow_unbound_editor: bool,
    nonce: String,
    /// 当前 webview 的 generation;每次销毁 / 重建都 +1,迟到的旧消息据此丢弃。
    epoch: Arc<AtomicU64>,
    creation: CreationState,
    /// 页面说过 ready、Rust 已回 open:此后才能向页面推送。
    ready: bool,
    shown: bool,
    bounds: Option<LogicalRect>,
    /// 把回复送回页面要窗口 id。
    window: Option<window::Id>,
    rejects: u32,
    last_reject: Option<String>,
    stand_in_runs: usize,
    last_probe: Option<Probe>,
    selftest: SelfTestTally,
    workbench: Workbench,
    new_title: String,
    new_path: String,
    import_path: String,
    last_error: Option<String>,
    recovery_operation: u64,
    recovery_busy: bool,
    recovery_pending_write: bool,
    recovery_timer_armed: bool,
    pending_flush: Option<u64>,
    next_flush_request: u64,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Message>>,
}

#[derive(Default)]
struct SelfTestTally {
    passes: u32,
    fails: u32,
}

/// 握手 nonce:进程内唯一且不可预测到足以挡住页面里的杂散脚本(页面本身是编译进来的第一方代码)。
fn fresh_nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!(
        "{:016x}{:016x}",
        h.finish(),
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    )
}

impl MarkdownPanel {
    pub fn new() -> Self {
        let mode = if std::env::var_os(SELFTEST_ENV).is_some() {
            Mode::SelfTest
        } else {
            Mode::Normal
        };
        Self::with_mode(mode)
    }

    fn with_mode(mode: Mode) -> Self {
        let nonce = fresh_nonce();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // 正常模式不再展示会冒充真实文稿的样例;SelfTest 仍是独立内存 fixture。
        let initial = if mode == Mode::SelfTest { SAMPLE } else { "" };
        Self {
            io: io::IoExecutor::new(),
            recovery_clock: recovery_clock::RecoveryClock::default(),
            external_review: None,
            mirror_operation: 0,
            mirror_busy: false,
            last_mirror_check: std::time::Instant::now(),
            load_ticket: 0,
            loading: false,
            requested_project: None,
            retired_generation: None,
            reviewing_recovery: false,
            managing_documents: false,
            transition_choice: false,
            save_then_transition: false,
            pending_save: None,
            pending_transition: None,
            transition_generation: 0,
            transition_flushed: false,
            prepared_transition: None,
            bridge: Bridge::new(initial, mode, nonce.clone()),
            // Lifecycle is an explicit memory-only geometry fixture. Ordinary
            // writing sessions require a bound document before creating a page.
            allow_unbound_editor: mode == Mode::SelfTest
                || std::env::var("DIGGER_LIFECYCLE_SELFTEST").is_ok_and(|value| value != "usage"),
            nonce,
            epoch: Arc::new(AtomicU64::new(0)),
            creation: CreationState::Idle,
            ready: false,
            shown: false,
            bounds: None,
            window: None,
            rejects: 0,
            last_reject: None,
            stand_in_runs: 0,
            last_probe: None,
            selftest: SelfTestTally::default(),
            workbench: Workbench::new(),
            new_title: String::new(),
            new_path: String::new(),
            import_path: String::new(),
            last_error: None,
            recovery_operation: 0,
            recovery_busy: false,
            recovery_pending_write: false,
            recovery_timer_armed: false,
            pending_flush: None,
            next_flush_request: 1,
            tx,
            rx: Some(rx),
        }
    }

    /// 产品提交项目变更的入口。ProductShell 先通过 HostTransition 完成页面
    /// flush 和保存/恢复草稿持久化,再调用这里。磁盘载入使用有界后台队列。
    pub fn set_project(&mut self, project: Option<PathBuf>) {
        if self.workbench.project_root() == project.as_deref() && !self.loading {
            return;
        }
        if self.workbench.current().is_some_and(|doc| doc.dirty)
            && !self
                .prepared_transition
                .as_ref()
                .is_some_and(|(identity, version)| {
                    self.workbench.identity().as_ref() == Some(identity)
                        && self.bridge.doc().version == *version
                })
        {
            self.last_error = Some("请先完成文稿切换的保存选择".into());
            return;
        }
        // The product's host-transition barrier has already protected the old
        // buffer. Invalidate its projection before exposing the new project.
        self.load_ticket = self
            .load_ticket
            .checked_add(1)
            .expect("load ticket exhausted");
        let ticket = self.load_ticket;
        self.loading = true;
        self.requested_project = project.clone();
        self.last_error = None;
        if self.is_live() {
            self.retired_generation = Some(self.generation());
        }
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.creation = CreationState::Idle;
        self.ready = false;
        self.shown = false;
        self.workbench.close_project();
        self.bridge.replace_document(None, "");
        let mut workbench = self.workbench.clone();
        let tx = self.tx.clone();
        if let Err(error) = self.io.dispatch(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(root) = project {
                    workbench.open_project(root)?;
                    workbench.replay_mirrors()?;
                }
                if let Some(first) = workbench.documents().first().cloned() {
                    workbench.open_document(&first.id)?;
                }
                Ok::<_, digger_store::writing::WritingError>(Box::new(workbench))
            }))
            .map_err(|_| "文稿载入异常退出".to_owned())
            .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = tx.send(Message::Loaded(ticket, result));
        }) {
            self.loading = false;
            self.last_error = Some(error);
        }
    }

    fn queue_load(&mut self, action: Transition) -> Task<Message> {
        self.load_ticket = self
            .load_ticket
            .checked_add(1)
            .expect("load ticket exhausted");
        let ticket = self.load_ticket;
        self.loading = true;
        let mut workbench = self.workbench.clone();
        let title = self.new_title.trim().to_owned();
        let path = self.new_path.trim().to_owned();
        let import = self.import_path.trim().to_owned();
        let job = self.io.submit(move || {
            match action {
                Transition::Document(id) => {
                    workbench.open_document(&id)?;
                }
                Transition::Create => {
                    workbench.create(&title, &path)?;
                }
                Transition::Import => {
                    workbench.import(&import)?;
                }
                Transition::File(path) => {
                    let root = workbench.project_root().ok_or_else(|| {
                        digger_store::writing::WritingError::NotFound("当前项目".into())
                    })?;
                    let relative = path
                        .strip_prefix(root)
                        .map_err(|_| {
                            digger_store::writing::WritingError::InvalidPath(
                                "文稿不在当前项目内".into(),
                            )
                        })?
                        .to_string_lossy()
                        .into_owned();
                    workbench.import(&relative)?;
                }
                Transition::RecoveryCopy => {
                    workbench.save_recovery_copy()?;
                }
                _ => unreachable!(),
            }
            workbench.replay_mirrors()?;
            Ok::<_, digger_store::writing::WritingError>(Box::new(workbench))
        });
        match job {
            Ok(rx) => Task::perform(
                async move {
                    rx.await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r)
                        .and_then(|r| r.map_err(|e| e.to_string()))
                },
                move |result| Message::Loaded(ticket, result),
            ),
            Err(error) => {
                self.loading = false;
                self.last_error = Some(error);
                self.release_transition()
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn settle_load_for_test(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while self.loading {
            match self.rx.as_mut().expect("test message receiver").try_recv() {
                Ok(message) => {
                    let _ = self.update(message);
                }
                Err(_) if std::time::Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => panic!("writing load did not finish: {error}"),
            }
        }
    }

    fn load_document_into_bridge(&mut self, loaded: &LoadedDocument) {
        match serde_json::from_str::<digger_editor::doc::State>(&loaded.snapshot.state_json) {
            Ok(state) => self
                .bridge
                .replace_document(Some(&state), &loaded.snapshot.text),
            Err(error) => {
                self.last_error = Some(format!("块身份状态无法解析：{error}"));
                self.bridge.replace_document(None, "");
            }
        }
    }

    fn force_rebuild(&mut self) {
        if self.is_live() {
            self.retired_generation = Some(self.generation());
        }
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.ready = false;
        self.creation = CreationState::Idle;
        self.bounds = None;
    }

    fn has_editable_session(&self) -> bool {
        self.allow_unbound_editor || self.workbench.current().is_some()
    }

    pub(crate) fn maintenance(&mut self) -> Task<Message> {
        if self.last_mirror_check.elapsed() < std::time::Duration::from_secs(5) {
            return Task::none();
        }
        self.check_mirrors(false)
    }

    fn check_mirrors(&mut self, retry: bool) -> Task<Message> {
        if self.loading
            || self.mirror_busy
            || self.pending_save.is_some()
            || self.pending_transition.is_some()
            || self.workbench.current().is_none()
        {
            return Task::none();
        }
        self.mirror_operation += 1;
        let ticket = self.mirror_operation;
        let mut workbench = self.workbench.clone();
        self.last_mirror_check = std::time::Instant::now();
        match self.io.submit(move || {
            if retry {
                workbench.retry_mirrors()?;
            }
            workbench.replay_mirrors()?;
            Ok::<_, digger_store::writing::WritingError>(
                workbench.mirror_error().map(str::to_owned),
            )
        }) {
            Ok(rx) => {
                self.mirror_busy = true;
                Task::perform(
                    async move {
                        rx.await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r)
                            .and_then(|r| r.map_err(|e| e.to_string()))
                    },
                    move |result| Message::MirrorsChecked(ticket, result),
                )
            }
            Err(error) => {
                self.last_error = Some(error);
                Task::none()
            }
        }
    }

    pub(crate) fn has_transition_choice(&self) -> bool {
        self.transition_choice
    }

    pub(crate) fn needs_transition_barrier(&self) -> bool {
        self.bridge.mode() == Mode::Normal
            && self.workbench.current().is_some()
            && (self.loading
                || self.ready
                || self.workbench.current().is_some_and(|doc| doc.dirty)
                || self.pending_save.is_some()
                || self.pending_flush.is_some()
                || self.recovery_busy)
    }

    pub(crate) fn release_transition(&mut self) -> Task<Message> {
        self.prepared_transition = None;
        self.push(&EditorOutbound::InputLock { locked: false })
    }

    fn request_transition(&mut self, action: Transition) -> Task<Message> {
        if self.loading
            || self.pending_save.is_some()
            || self.pending_flush.is_some()
            || self.pending_transition.is_some()
        {
            let error = "正在保存或切换文稿，请等待完成后重试".to_string();
            self.last_error = Some(error.clone());
            return match action {
                Transition::Host(token) => {
                    Task::done(Message::HostTransitionReady(token, Err(error)))
                }
                _ => Task::none(),
            };
        }
        self.transition_generation += 1;
        self.pending_transition = Some(action);
        if self.bridge.mode() != Mode::Normal || self.workbench.current().is_none() {
            return self.complete_transition();
        }
        if !self.ready {
            // An unopened page cannot have unacknowledged input. Preserve any
            // Rust-owned recovery buffer before proceeding.
            self.transition_flushed = true;
            return self.choose_transition();
        }
        let request_id = self.next_flush_request;
        self.next_flush_request += 1;
        self.pending_flush = Some(request_id);
        Task::batch([
            self.push(&EditorOutbound::Flush {
                request_id,
                freeze: true,
            }),
            Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                },
                move |_| Message::FlushTimeout(request_id),
            ),
        ])
    }

    fn choose_transition(&mut self) -> Task<Message> {
        if self.workbench.current().is_some_and(|doc| doc.dirty) {
            self.transition_choice = true;
            Task::none()
        } else {
            self.start_transition_write()
        }
    }

    fn start_transition_write(&mut self) -> Task<Message> {
        if self.recovery_busy {
            return Task::none();
        }
        if !self.workbench.current().is_some_and(|doc| doc.dirty) {
            if let Some(identity) = self.workbench.identity() {
                self.prepared_transition = Some((identity, self.bridge.doc().version));
            }
            return self.complete_transition();
        }
        let state = self.bridge.export_snapshot();
        let result = self.workbench.prepare_recovery(
            &state.text,
            &serde_json::to_string(&state).unwrap(),
            1,
            state.version,
        );
        self.bridge.restore_history(state.history.clone());
        match result {
            Ok(plan) => {
                let identity = self.workbench.identity().unwrap();
                let transition_generation = self.transition_generation;
                match self.io.submit(move || plan.run()) {
                    Ok(rx) => Task::perform(
                        async move {
                            rx.await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r)
                                .and_then(|r| r.map_err(|e| e.to_string()))
                        },
                        move |result| {
                            Message::TransitionPersisted(
                                transition_generation,
                                identity,
                                state.version,
                                result,
                            )
                        },
                    ),
                    Err(error) => self.fail_transition(error),
                }
            }
            Err(error) => self.fail_transition(error.to_string()),
        }
    }

    fn fail_transition(&mut self, error: String) -> Task<Message> {
        self.transition_generation += 1;
        self.transition_choice = false;
        self.save_then_transition = false;
        self.last_error = Some(error.clone());
        self.pending_flush = None;
        self.transition_flushed = false;
        let action = self.pending_transition.take();
        let release = self.release_transition();
        match action {
            Some(Transition::Host(token)) => Task::batch([
                release,
                Task::done(Message::HostTransitionReady(token, Err(error))),
            ]),
            _ => release,
        }
    }

    fn complete_transition(&mut self) -> Task<Message> {
        self.transition_flushed = false;
        let Some(action) = self.pending_transition.take() else {
            return Task::none();
        };
        if let Transition::Host(token) = action {
            return Task::done(Message::HostTransitionReady(token, Ok(())));
        }
        if let Transition::AcceptRecovery = action {
            self.reviewing_recovery = false;
            if let Some((state, text)) = self.workbench.accepted_recovery() {
                self.bridge.replace_document(Some(&state), &text);
                // A recovery envelope contains uncommitted history; durable
                // document imports deliberately do not restore pending rows.
                self.bridge.restore_history(state.history.clone());
                self.workbench.mark_dirty(self.bridge.doc().version);
                self.force_rebuild();
            } else {
                self.last_error = Some("草稿版本已经变化，请另存副本".into());
            }
            return self.release_transition();
        }
        self.queue_load(action)
    }

    /// 切换/关闭前把当前未提交缓冲落到恢复草稿。失败时调用方必须留在旧会话。
    fn persist_transition_draft(&mut self) -> Result<(), String> {
        if self
            .prepared_transition
            .as_ref()
            .is_some_and(|(id, version)| {
                self.workbench.identity().as_ref() == Some(id)
                    && self.bridge.doc().version == *version
            })
        {
            return Ok(());
        }
        if self.bridge.mode() != Mode::Normal
            || !self
                .workbench
                .current()
                .is_some_and(|current| current.dirty)
        {
            return Ok(());
        }
        let state = self.bridge.export_snapshot();
        let result = self.workbench.write_recovery(
            &state.text,
            &serde_json::to_string(&state).unwrap_or_default(),
            1,
            self.bridge.doc().version,
        );
        // 恢复草稿不是提交:未确认的 Agent history 仍归当前文档。
        self.bridge.restore_history(state.history.clone());
        result.map_err(|error| error.to_string())
    }

    fn save_current(&mut self) -> Task<Message> {
        if self.bridge.mode() != Mode::Normal || self.workbench.current().is_none() {
            return Task::none();
        }
        if self.loading
            || self.pending_flush.is_some()
            || self.pending_save.is_some()
            || self.pending_transition.is_some()
            || matches!(self.workbench.status(), WorkbenchStatus::Saving)
        {
            return Task::none();
        }
        // 保存必须先经过页面 flush 屏障:页面 sent/queued/IME 排空后,才允许
        // 截取 Rust 已确认的版本,避免把旧快照标成已保存。
        if !self.ready {
            self.last_error = Some("页面尚未就绪，无法确认保存水位".into());
            self.workbench.mark_save_failed("页面尚未就绪");
            return Task::none();
        }
        let request_id = self.next_flush_request;
        self.next_flush_request += 1;
        self.pending_flush = Some(request_id);
        let flush = self.push(&EditorOutbound::Flush {
            request_id,
            freeze: false,
        });
        let timeout = Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            },
            move |_| Message::FlushTimeout(request_id),
        );
        Task::batch([flush, timeout])
    }

    fn on_flushed(
        &mut self,
        request_id: u64,
        version: u64,
        error: Option<String>,
    ) -> Task<Message> {
        if self.pending_flush != Some(request_id) {
            return Task::none();
        }
        self.pending_flush = None;
        if let Some(error) = error {
            if self.pending_transition.is_some() {
                return self.fail_transition(error);
            }
            self.last_error = Some(format!("保存前刷新失败：{error}"));
            self.workbench.mark_save_failed(error);
            return Task::none();
        }
        if self.bridge.doc().version != version {
            if self.pending_transition.is_some() {
                return self.fail_transition("页面确认版本已经变化".into());
            }
            self.last_error = Some(format!(
                "页面确认版本 {version} 与 Rust 版本 {} 不一致",
                self.bridge.doc().version
            ));
            self.workbench.mark_save_failed("页面/Rust 版本不一致");
            return Task::none();
        }
        if self.pending_transition.is_some() {
            self.transition_flushed = true;
            return self.choose_transition();
        }
        self.start_save_after_flush()
    }

    fn start_save_after_flush(&mut self) -> Task<Message> {
        let state = self.bridge.export_snapshot();
        let history = state.history.clone();
        let snapshot = WritingSnapshot::new(
            state.text.clone(),
            serde_json::to_string(&state).unwrap_or_default(),
            1,
        );
        let identity = self.workbench.identity().expect("active document");
        match self.workbench.prepare_save(snapshot) {
            Ok(plan) => match self.io.submit(move || plan.run()) {
                Ok(rx) => {
                    self.pending_save = Some(identity.clone());
                    Task::perform(
                        async move {
                            rx.await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r)
                                .and_then(|r| r.map_err(|e| e.to_string()))
                        },
                        move |result| Message::SaveFinished(identity, result, history),
                    )
                }
                Err(error) => {
                    self.bridge.restore_history(history);
                    self.workbench.mark_save_failed(&error);
                    self.last_error = Some(error);
                    Task::none()
                }
            },
            Err(error) => {
                self.bridge.restore_history(history);
                self.workbench.mark_save_failed(&error);
                self.last_error = Some(error.to_string());
                Task::none()
            }
        }
    }

    fn schedule_recovery(&mut self) -> Task<Message> {
        if self.recovery_timer_armed {
            return Task::none();
        }
        self.recovery_timer_armed = true;
        let delay = self
            .recovery_clock
            .remaining(std::time::Instant::now())
            .unwrap_or(std::time::Duration::from_millis(500));
        Task::perform(
            async move {
                tokio::time::sleep(delay).await;
            },
            |_| Message::RecoveryTick,
        )
    }

    fn write_recovery_now(&mut self) -> Task<Message> {
        if self.bridge.mode() != Mode::Normal
            || self.pending_transition.is_some()
            || self.recovery_busy
            || !self
                .workbench
                .current()
                .is_some_and(|current| current.dirty)
        {
            return Task::none();
        }
        let state = self.bridge.export_snapshot();
        let history = state.history.clone();
        let plan = self.workbench.prepare_recovery(
            &state.text,
            &serde_json::to_string(&state).unwrap_or_default(),
            1,
            self.bridge.doc().version,
        );
        // 恢复草稿不是提交:未确认的 history 仍归当前文档。
        self.bridge.restore_history(history);
        match plan {
            Ok(plan) => {
                let identity = self.workbench.identity().expect("active document");
                self.recovery_operation += 1;
                let operation = self.recovery_operation;
                self.recovery_busy = true;
                match self.io.submit(move || plan.run()) {
                    Ok(rx) => Task::perform(
                        async move {
                            rx.await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r)
                                .and_then(|r| r.map_err(|e| e.to_string()))
                        },
                        move |result| Message::RecoveryFinished(operation, identity, result),
                    ),
                    Err(error) => {
                        self.recovery_busy = false;
                        self.last_error = Some(error);
                        Task::none()
                    }
                }
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                Task::none()
            }
        }
    }

    /// 自检模式下产品壳启动即打开本面板。
    pub fn is_selftest(&self) -> bool {
        self.bridge.mode() == Mode::SelfTest
    }

    /// 测试 / 生命周期自检读取当前真实文稿。
    #[cfg(test)]
    pub(crate) fn current_document(&self) -> Option<&workbench::OpenDocument> {
        self.workbench.current()
    }

    /// 页面消息的接收端,启动时接成 iced 流(一次性)。
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Message>> {
        self.rx.take()
    }

    fn generation(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// webview 存在(创建中或已创建)。
    fn is_live(&self) -> bool {
        matches!(
            self.creation,
            CreationState::Building { .. } | CreationState::Ready { .. }
        )
    }

    /// 当前创建状态(测试 / 生命周期自检读)。
    pub fn creation_state(&self) -> &CreationState {
        &self.creation
    }

    fn failed_error(&self) -> Option<&str> {
        match &self.creation {
            CreationState::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CancelLoad => {
                if !self.loading {
                    return Task::none();
                }
                self.load_ticket += 1;
                self.loading = false;
                self.last_error =
                    Some("已停止等待；已准入的后台操作仍会结算，可重新载入查看结果".into());
                self.release_transition()
            }
            Message::RetryProjectLoad => {
                self.set_project(self.requested_project.clone());
                Task::none()
            }
            Message::CheckMirrors => self.check_mirrors(true),
            Message::MirrorsChecked(ticket, result) => {
                if ticket == self.mirror_operation {
                    self.mirror_busy = false;
                    match result {
                        Ok(notice) => self.workbench.set_mirror_notice(notice),
                        Err(error) => self.workbench.set_mirror_notice(Some(error)),
                    }
                }
                Task::none()
            }
            Message::ReviewExternal => {
                if self.loading || self.mirror_busy {
                    return Task::none();
                }
                let Some(identity) = self.workbench.identity() else {
                    return Task::none();
                };
                let workbench = self.workbench.clone();
                self.mirror_operation += 1;
                let operation = self.mirror_operation;
                match self.io.submit(move || workbench.external_review()) {
                    Ok(rx) => {
                        self.mirror_busy = true;
                        Task::perform(
                            async move {
                                rx.await
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r)
                                    .and_then(|r| r.map_err(|e| e.to_string()))
                            },
                            move |result| Message::ExternalReviewed(operation, identity, result),
                        )
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::ExternalReviewed(operation, identity, result) => {
                if operation != self.mirror_operation {
                    return Task::none();
                }
                self.mirror_busy = false;
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                match result {
                    Ok(review) => self.external_review = Some(review),
                    Err(error) => self.last_error = Some(error),
                }
                Task::none()
            }
            Message::CloseExternalReview => {
                self.external_review = None;
                Task::none()
            }
            Message::ResolveExternal(copy) => {
                if self.mirror_busy {
                    return Task::none();
                }
                let (Some(identity), Some(review)) =
                    (self.workbench.identity(), self.external_review.clone())
                else {
                    return Task::none();
                };
                let workbench = self.workbench.clone();
                self.mirror_operation += 1;
                let operation = self.mirror_operation;
                match self
                    .io
                    .submit(move || workbench.resolve_external(&review, copy))
                {
                    Ok(rx) => {
                        self.mirror_busy = true;
                        Task::perform(
                            async move {
                                rx.await
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r)
                                    .and_then(|r| r.map_err(|e| e.to_string()))
                            },
                            move |result| Message::ExternalResolved(operation, identity, result),
                        )
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::ExternalResolved(operation, identity, result) => {
                if operation != self.mirror_operation {
                    return Task::none();
                }
                self.mirror_busy = false;
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                match result {
                    Ok(documents) => {
                        self.workbench.apply_document_list(documents);
                        self.external_review = None;
                        self.last_error = None;
                        self.check_mirrors(false)
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::ReviewRecovery(show) => {
                self.reviewing_recovery = show;
                Task::none()
            }
            Message::ManageDocuments(show) => {
                self.managing_documents = show;
                Task::none()
            }
            Message::Loaded(ticket, result) => {
                if ticket != self.load_ticket {
                    return Task::none();
                }
                self.loading = false;
                self.mirror_operation += 1;
                self.mirror_busy = false;
                self.recovery_operation += 1;
                self.recovery_busy = false;
                self.recovery_clock.clear();
                match result {
                    Ok(workbench) => {
                        self.workbench = *workbench;
                        if let Some(current) = self.workbench.current().cloned() {
                            let loaded = LoadedDocument {
                                document: current.document,
                                snapshot: current.snapshot,
                            };
                            self.load_document_into_bridge(&loaded);
                        } else {
                            self.bridge.replace_document(None, "");
                        }
                        self.external_review = None;
                        self.reviewing_recovery = false;
                        self.managing_documents = false;
                        self.new_title.clear();
                        self.new_path.clear();
                        self.import_path.clear();
                        self.last_error = None;
                        self.force_rebuild();
                    }
                    Err(error) => self.last_error = Some(error),
                }
                self.release_transition()
            }
            Message::TransitionSave if self.transition_choice => {
                self.transition_choice = false;
                self.save_then_transition = true;
                let task = self.start_save_after_flush();
                if self.pending_save.is_none() {
                    return self.fail_transition(
                        self.last_error
                            .clone()
                            .unwrap_or_else(|| "保存未启动".into()),
                    );
                }
                task
            }
            Message::TransitionKeep if self.transition_choice => {
                self.transition_choice = false;
                self.start_transition_write()
            }
            Message::TransitionCancel if self.pending_transition.is_some() => {
                self.fail_transition("已取消切换，文稿仍保留".into())
            }
            Message::TransitionSave | Message::TransitionKeep | Message::TransitionCancel => {
                Task::none()
            }
            Message::OpenFile(path) => self.request_transition(Transition::File(path)),
            Message::Page(generation, raw) => {
                if !self.has_editable_session()
                    || self.loading
                    || !self.is_live()
                    || generation != self.generation()
                {
                    return Task::none(); // 迟到的旧 generation:丢弃,绝不应用
                }
                match self.bridge.handle(&raw) {
                    Handled::Reply(out) => {
                        let edited = matches!(
                            &out,
                            EditorOutbound::Blocks { .. }
                                | EditorOutbound::Incremental { .. }
                                | EditorOutbound::Update { .. }
                        );
                        match &out {
                            EditorOutbound::Open { .. } => self.ready = true,
                            EditorOutbound::Reject { reason, .. } => {
                                self.rejects += 1;
                                self.last_reject = Some(reason.clone());
                            }
                            _ => {}
                        }
                        if edited {
                            self.workbench.mark_dirty(self.bridge.doc().version);
                            self.recovery_clock.edit(std::time::Instant::now());
                            self.recovery_pending_write = true;
                        }
                        let reply = self.push(&out);
                        if edited {
                            Task::batch([reply, self.schedule_recovery()])
                        } else {
                            reply
                        }
                    }
                    Handled::Flushed {
                        request_id,
                        version,
                        error,
                    } => self.on_flushed(request_id, version, error),
                    Handled::Report(report) => self.report(report),
                }
            }
            Message::Built(generation, result) => {
                // 只接受当前 Building generation 的结果;旧 generation 的迟到成功 / 失败都不改变新实例。
                if !matches!(self.creation, CreationState::Building { generation: g } if g == generation)
                    || generation != self.generation()
                {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        self.creation = CreationState::Ready { generation };
                    }
                    Err(error) => {
                        self.ready = false;
                        self.creation = CreationState::Failed { generation, error };
                    }
                }
                Task::none()
            }
            Message::Retry => self.start_build(),
            Message::StandIn(what) => {
                if !self.ready {
                    return Task::none();
                }
                let what = match what {
                    StandIn::RewriteParagraph(_) => {
                        self.stand_in_runs += 1;
                        StandIn::RewriteParagraph(self.stand_in_runs - 1)
                    }
                    other => other,
                };
                match self.bridge.stand_in(what) {
                    Some(out) => self.push(&out),
                    None => Task::none(),
                }
            }
            Message::Probed(probe) => {
                self.last_probe = Some(probe);
                Task::none()
            }
            Message::NewTitleChanged(title) => {
                self.new_title = title;
                Task::none()
            }
            Message::NewPathChanged(path) => {
                self.new_path = path;
                Task::none()
            }
            Message::ImportPathChanged(path) => {
                self.import_path = path;
                Task::none()
            }
            Message::Create => self.request_transition(Transition::Create),
            Message::Import => self.request_transition(Transition::Import),
            Message::OpenDocument(id) => self.request_transition(Transition::Document(id)),
            Message::HostTransition(token) => self.request_transition(Transition::Host(token)),
            Message::HostTransitionReady(..) => Task::none(),
            Message::TransitionPersisted(transition_generation, identity, version, result) => {
                if transition_generation != self.transition_generation
                    || self.pending_transition.is_none()
                {
                    return Task::none();
                }
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                if let Err(error) = result {
                    return self.fail_transition(error);
                }
                if self.bridge.doc().version != version {
                    return self.fail_transition("草稿持久化期间文稿又发生变化，请重试".into());
                }
                self.prepared_transition = Some((identity, version));
                self.complete_transition()
            }
            Message::Save => self.save_current(),
            Message::RecoveryTick => {
                self.recovery_timer_armed = false;
                if self
                    .recovery_clock
                    .remaining(std::time::Instant::now())
                    .is_some_and(|delay| !delay.is_zero())
                {
                    return self.schedule_recovery();
                }
                self.recovery_clock.clear();
                self.write_recovery_now()
            }
            Message::SaveFinished(identity, result, history) => {
                if self.pending_save.as_ref() == Some(&identity) {
                    self.pending_save = None;
                }
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                match result {
                    Ok(outcome) => {
                        self.workbench.apply_save(outcome);
                        self.last_error = None;
                        if self.save_then_transition {
                            self.save_then_transition = false;
                            self.prepared_transition = Some((identity, self.bridge.doc().version));
                            return self.complete_transition();
                        }
                    }
                    Err(error) => {
                        self.bridge.restore_history(history);
                        self.workbench.mark_save_failed(&error);
                        self.last_error = Some(error.clone());
                        if self.save_then_transition {
                            return self.fail_transition(error);
                        }
                    }
                }
                Task::none()
            }
            Message::FlushTimeout(request_id) => {
                if self.pending_flush == Some(request_id) {
                    self.pending_flush = None;
                    if self.pending_transition.is_some() {
                        return self.fail_transition("切换前页面刷新超时，原文稿仍保留".into());
                    }
                    self.workbench
                        .mark_save_failed("保存前的页面刷新超时；输入仍保留，可重试");
                    self.last_error = Some("保存前的页面刷新超时；输入仍保留，可重试".into());
                }
                Task::none()
            }
            Message::RecoveryFinished(operation, identity, result) => {
                if operation != self.recovery_operation {
                    return Task::none();
                }
                self.recovery_busy = false;
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                if self.pending_transition.is_some()
                    && self.transition_flushed
                    && !self.transition_choice
                    && !self.save_then_transition
                {
                    return self.start_transition_write();
                }
                self.recovery_busy = false;
                if let Err(error) = result {
                    self.last_error = Some(error);
                    self.recovery_pending_write = false;
                    return Task::none();
                }
                if self.recovery_pending_write
                    && self
                        .workbench
                        .current()
                        .is_some_and(|current| current.dirty)
                {
                    self.recovery_pending_write = false;
                    return self.schedule_recovery();
                }
                Task::none()
            }
            Message::AcceptRecovery => self.request_transition(Transition::AcceptRecovery),
            Message::RecoverySaveCopy => self.request_transition(Transition::RecoveryCopy),
            Message::DiscardRecovery => {
                if self.recovery_busy
                    || self.pending_save.is_some()
                    || self.pending_transition.is_some()
                {
                    self.last_error = Some("草稿仍在写入，请等待完成后重试".into());
                    return Task::none();
                }
                let Some(identity) = self.workbench.identity() else {
                    return Task::none();
                };
                let mut workbench = self.workbench.clone();
                self.recovery_operation += 1;
                let operation = self.recovery_operation;
                match self.io.submit(move || workbench.discard_recovery()) {
                    Ok(rx) => {
                        self.recovery_busy = true;
                        Task::perform(
                            async move {
                                rx.await
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r)
                                    .and_then(|r| r.map_err(|e| e.to_string()))
                            },
                            move |result| Message::RecoveryDiscarded(operation, identity, result),
                        )
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::RecoveryDiscarded(operation, identity, result) => {
                if operation != self.recovery_operation {
                    return Task::none();
                }
                self.recovery_busy = false;
                if self.workbench.identity().as_ref() != Some(&identity) {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        self.workbench.clear_recovery_notice();
                        self.reviewing_recovery = false;
                    }
                    Err(error) => self.last_error = Some(error),
                }
                if self.pending_transition.is_some()
                    && self.transition_flushed
                    && !self.transition_choice
                {
                    self.start_transition_write()
                } else {
                    Task::none()
                }
            }
            Message::Done => Task::none(),
        }
    }

    /// 页面已说过 ready、Rust 已回 open。
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// 读真实 webview 的状态(生命周期自检用),结果经 [`Message::Probed`] 回来。
    pub fn probe(&self, id: window::Id) -> Task<Message> {
        let generation = self.generation();
        window::run(id, move |_| Message::Probed(host::probe(generation)))
    }

    pub fn last_probe(&self) -> Option<Probe> {
        self.last_probe
    }

    /// 把回复送回页面(窗口线程上执行)。
    fn push(&self, out: &EditorOutbound) -> Task<Message> {
        let Some(window_id) = self.window else {
            return Task::none();
        };
        let (generation, json) = (self.generation(), out.to_json());
        window::run(window_id, move |_| {
            host::send(generation, &json);
            Message::Done
        })
    }

    /// 自检页的上报:打印,并在结束时以退出码报告。正常运行时 Bridge 不会产生这些。
    fn report(&mut self, report: Report) -> Task<Message> {
        match report {
            Report::Ready { .. } => {}
            Report::Result { name, pass, detail } => {
                println!(
                    "{}: {name}{}",
                    if pass { "PASS" } else { "FAIL" },
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("\n      {detail}")
                    }
                ); // cli-output
                if pass {
                    self.selftest.passes += 1
                } else {
                    self.selftest.fails += 1
                }
            }
            Report::Info { name, detail } => println!("INFO: {name}\n      {detail}"), // cli-output
            Report::Done => {
                println!(
                    "SUMMARY digger-editor selftest: {} passed, {} failed",
                    self.selftest.passes, self.selftest.fails
                ); // cli-output
                std::process::exit(i32::from(
                    self.selftest.fails > 0 || self.selftest.passes == 0,
                ));
            }
        }
        Task::none()
    }

    /// 面板几何与可见性同步。`rect` 是面板内容区;webview 让出顶栏。
    ///
    /// - Idle / Closed / 失败后重新显示 → 建一个新 generation;
    /// - Ready 隐藏时不重建(文档与撤销栈留在页面里),显示只改可见性 / 几何;
    /// - Failed 且仍在显示 → 等用户点「重试」,不每帧自动重试。
    pub fn sync_native(
        &mut self,
        id: window::Id,
        visible: bool,
        rect: LogicalRect,
    ) -> Task<Message> {
        self.window = Some(id);
        if let Some(generation) = self.retired_generation.take() {
            return window::run(id, move |_| {
                host::destroy(generation);
                Message::Done
            });
        }
        let visible = visible
            && self.has_editable_session()
            && !self.loading
            && !self.transition_choice
            && !self.managing_documents
            && !self.reviewing_recovery
            && self.external_review.is_none();
        let web = LogicalRect {
            x: rect.x,
            y: rect.y + BAR_HEIGHT,
            width: rect.width,
            height: (rect.height - BAR_HEIGHT).max(1.0),
        };
        let was_shown = self.shown;
        if !visible {
            // 切走:隐藏而不是留一个看不见的 webview 盖在别的面板上;文档与撤销历史留在页面里
            self.shown = false;
            if !self.is_live() || !was_shown {
                return Task::none();
            }
            let generation = self.generation();
            return window::run(id, move |_| {
                host::set_visible(generation, false);
                Message::Done
            });
        }
        match &self.creation {
            CreationState::Building { .. } => {
                let moved = self.bounds != Some(web);
                self.bounds = Some(web);
                self.shown = true;
                if moved {
                    let generation = self.generation();
                    return window::run(id, move |_| {
                        host::set_bounds(generation, web);
                        Message::Done
                    });
                }
                Task::none()
            }
            CreationState::Ready { .. } => {
                let moved = self.bounds != Some(web);
                self.shown = true;
                self.bounds = Some(web);
                if was_shown && !moved {
                    return Task::none();
                }
                let generation = self.generation();
                window::run(id, move |_| {
                    if moved {
                        host::set_bounds(generation, web);
                    }
                    if !was_shown {
                        host::set_visible(generation, true);
                    }
                    Message::Done
                })
            }
            // 仍在显示时的失败只等显式重试;切走再回来(showing 一次新的一次)才自动重建。
            CreationState::Failed { .. } if was_shown => Task::none(),
            _ => self.start_build_with(id, web),
        }
    }

    /// 建一个新 generation:先销毁旧句柄(迟到回包由 generation 挡掉),清掉旧错误,再构建。
    fn start_build_with(&mut self, id: window::Id, web: LogicalRect) -> Task<Message> {
        if matches!(self.creation, CreationState::Building { .. }) {
            return Task::none();
        }
        let old = self.epoch.fetch_add(1, Ordering::SeqCst);
        let generation = old + 1;
        self.ready = false;
        self.rejects = 0;
        self.last_reject = None;
        self.shown = true;
        self.bounds = Some(web);
        self.creation = CreationState::Building { generation };
        let (nonce, mode, tx) = (self.nonce.clone(), self.bridge.mode(), self.tx.clone());
        let destroy = window::run(id, move |_| {
            host::destroy(old);
            Message::Done
        });
        let build = window::run(id, move |w| {
            Message::Built(
                generation,
                host::build(&w, generation, web, &nonce, mode, tx),
            )
        });
        Task::batch([destroy, build])
    }

    /// 顶栏「重试」:只有 Failed 状态可触发;用上次的窗口与几何真实重建。
    fn start_build(&mut self) -> Task<Message> {
        let (Some(id), Some(web)) = (self.window, self.bounds) else {
            return Task::none();
        };
        if !matches!(self.creation, CreationState::Failed { .. }) {
            return Task::none();
        }
        self.start_build_with(id, web)
    }

    pub fn sync_theme(&self) -> Task<Message> {
        let Some(id) = self.window.filter(|_| self.is_live()) else {
            return Task::none();
        };
        let generation = self.generation();
        let theme = crate::settings::theme::code();
        window::run(id, move |_| {
            host::set_theme(generation, theme);
            Message::Done
        })
    }

    /// 关闭面板:销毁 webview 并丢掉文档(退出时调用)。迟到的消息从此被 generation 挡掉。
    pub fn close(&mut self, id: window::Id) -> Task<Message> {
        if let Err(error) = self.persist_transition_draft() {
            self.last_error = Some(format!("关闭前无法保留草稿：{error}"));
            return Task::none();
        }
        if matches!(self.creation, CreationState::Idle | CreationState::Closed) {
            return Task::none();
        }
        let generation = self.epoch.fetch_add(1, Ordering::SeqCst);
        self.creation = CreationState::Closed;
        self.ready = false;
        self.shown = false;
        self.bounds = None;
        window::run(id, move |_| {
            host::destroy(generation);
            Message::Done
        })
    }

    fn status_text(&self) -> String {
        if self.loading {
            return "正在载入文稿…".into();
        }
        if let Some(error) = self.failed_error() {
            return format!("编辑器创建失败：{error}");
        }
        if self.is_selftest() {
            return "自检中…".to_string();
        }
        match self.workbench.status() {
            WorkbenchStatus::NoProject => "打开项目后新建或导入文稿".to_string(),
            WorkbenchStatus::Empty => "尚未选择文稿".to_string(),
            WorkbenchStatus::Ready => match self.workbench.mirror_error() {
                Some(error) => format!("已保存，文件待同步：{error}"),
                None => "已保存".into(),
            },
            WorkbenchStatus::Dirty => "未保存".to_string(),
            WorkbenchStatus::Saving => "保存中…".to_string(),
            WorkbenchStatus::Saved => match self.workbench.mirror_error() {
                Some(error) => format!("已保存，文件待同步：{error}"),
                None => "已保存".to_string(),
            },
            WorkbenchStatus::SaveFailed(reason) => format!("保存失败：{reason}"),
            WorkbenchStatus::RecoveryPending { .. } => "发现恢复草稿".to_string(),
            WorkbenchStatus::Error(reason) => format!("文稿库错误：{reason}"),
        }
    }

    fn project_label(&self) -> String {
        self.workbench
            .project_root()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("无项目")
            .to_owned()
    }

    /// 顶栏:真实项目/文稿、保存/恢复入口;Agent 替身只在自检模式保留。
    pub fn view(&self) -> Element<'_, Message> {
        use iced::widget::{button, column, row, text, text_input};
        if let Some(review) = self
            .external_review
            .as_ref()
            .filter(|_| !self.transition_choice)
        {
            let external = review
                .candidate
                .as_ref()
                .map(|source| source.text.as_str())
                .unwrap_or("（外部文件不存在）");
            let mut actions = row![
                button("返回").on_press(Message::CloseExternalReview),
                button("重新预览").on_press(Message::ReviewExternal),
                button("重试文件同步").on_press(Message::CheckMirrors)
            ]
            .spacing(8);
            if !self.mirror_busy {
                actions = actions.push(
                    button("确认以已保存正文替换外部文件")
                        .on_press(Message::ResolveExternal(false)),
                );
                if review.candidate.is_some() {
                    actions = actions.push(
                        button("将预览的外部正文另存副本").on_press(Message::ResolveExternal(true)),
                    );
                }
            }
            let mut content = column![
                text(format!(
                    "文件比较：{}，已保存版本 {}",
                    review.document.relative_path, review.document.revision
                )),
                actions,
                text("已保存正文（不含当前未保存输入）"),
                text(&review.official.text),
                text("外部文件正文"),
                text(external)
            ]
            .spacing(12);
            if let Some(error) = &self.last_error {
                content = content.push(text(error));
            }
            return iced::widget::scrollable(content)
                .height(iced::Length::Fill)
                .into();
        }
        if self.reviewing_recovery && !self.transition_choice {
            if let Some(pending) = self.workbench.recovery_pending() {
                let official = self
                    .workbench
                    .current()
                    .map(|current| current.snapshot.text.as_str())
                    .unwrap_or("");
                let mut actions = row![
                    button("返回").on_press(Message::ReviewRecovery(false)),
                    button("另存副本").on_press(Message::RecoverySaveCopy),
                    button("丢弃草稿").on_press(Message::DiscardRecovery)
                ]
                .spacing(8);
                if pending.class == digger_store::writing::RecoveryClass::Recoverable {
                    actions =
                        actions.push(button("恢复为未保存文稿").on_press(Message::AcceptRecovery));
                }
                return iced::widget::scrollable(
                    column![
                        text(format!(
                            "恢复预览：基于版本 {}，当前版本 {}",
                            pending.draft.base_revision,
                            self.workbench.current().unwrap().document.revision
                        )),
                        actions,
                        text("已保存正文"),
                        text(official),
                        text("恢复草稿正文"),
                        text(&pending.draft.text)
                    ]
                    .spacing(12),
                )
                .height(iced::Length::Fill)
                .into();
            }
        }
        if self.loading {
            return column![
                text("正在载入文稿…"),
                button("停止等待").on_press(Message::CancelLoad)
            ]
            .spacing(8)
            .into();
        }
        if self.managing_documents
            && !self.transition_choice
            && self.workbench.project_root().is_some()
        {
            let mut documents = column![
                text("项目文稿"),
                row![
                    text_input("标题", &self.new_title).on_input(Message::NewTitleChanged),
                    text_input("相对路径，如 book/01.md", &self.new_path)
                        .on_input(Message::NewPathChanged),
                    button("新建").on_press(Message::Create)
                ]
                .spacing(8),
                row![
                    text_input("导入项目内 .md", &self.import_path)
                        .on_input(Message::ImportPathChanged),
                    button("导入").on_press(Message::Import)
                ]
                .spacing(8),
                button("返回文稿").on_press(Message::ManageDocuments(false))
            ]
            .spacing(8);
            if let Some(error) = &self.last_error {
                documents = documents.push(text(error));
            }
            for document in self.workbench.documents() {
                documents = documents.push(
                    button(text(format!(
                        "{} — {}",
                        document.title, document.relative_path
                    )))
                    .on_press(Message::OpenDocument(document.id.clone())),
                );
            }
            return iced::widget::scrollable(documents)
                .height(iced::Length::Fill)
                .into();
        }
        if self.transition_choice {
            return column![
                text("离开当前文稿前，如何处理未保存的内容？"),
                row![
                    button("保存后继续").on_press(Message::TransitionSave),
                    button("保留恢复草稿后继续").on_press(Message::TransitionKeep),
                    button("取消").on_press(Message::TransitionCancel)
                ]
                .spacing(8)
            ]
            .spacing(8)
            .into();
        }
        let mut bar = row![].spacing(8).align_y(iced::Alignment::Center);
        if self.workbench.project_root().is_none() {
            bar = bar.push(text("打开项目后新建或导入文稿"));
            if self.requested_project.is_some() {
                bar = bar.push(button("重新载入项目文稿").on_press(Message::RetryProjectLoad));
            }
        } else {
            bar = bar
                .push(text(format!("项目：{}", self.project_label())))
                .push(button("文稿管理").on_press(Message::ManageDocuments(true)));
            if let Some(current) = self.workbench.current() {
                bar = bar.push(text(format!("文稿：{}", current.document.title)));
                bar = bar.push(text(self.status_text()));
                let mut save = button("保存");
                if self.bridge.mode() == Mode::Normal {
                    save = save.on_press(Message::Save);
                }
                bar = bar
                    .push(save)
                    .push(button("比较文件").on_press(Message::ReviewExternal));
            } else {
                bar = bar.push(
                    text_input("标题", &self.new_title)
                        .on_input(Message::NewTitleChanged)
                        .width(120.0),
                );
                bar = bar.push(
                    text_input("相对路径（如 book/01.md）", &self.new_path)
                        .on_input(Message::NewPathChanged)
                        .width(220.0),
                );
                bar = bar.push(button("新建").on_press(Message::Create));
                bar = bar.push(
                    text_input("导入项目内 .md", &self.import_path)
                        .on_input(Message::ImportPathChanged)
                        .width(180.0),
                );
                bar = bar.push(button("导入").on_press(Message::Import));
                bar = bar.push(text(self.status_text()));
            }
            if let Some(pending) = self.workbench.recovery_pending() {
                let label = match pending.class {
                    digger_store::writing::RecoveryClass::Recoverable => "恢复草稿",
                    digger_store::writing::RecoveryClass::Behind => "恢复草稿（版本落后）",
                    digger_store::writing::RecoveryClass::Conflict => "恢复草稿（版本冲突）",
                    digger_store::writing::RecoveryClass::Redundant => "恢复草稿（冗余）",
                };
                bar = bar.push(button("预览草稿").on_press(Message::ReviewRecovery(true)));
                bar = bar.push(text(format!(
                    "{label}（基于版本 {}）",
                    pending.draft.base_revision
                )));
                if pending.class == digger_store::writing::RecoveryClass::Recoverable {
                    bar = bar.push(button("恢复").on_press(Message::AcceptRecovery));
                } else {
                    bar = bar.push(button("另存副本").on_press(Message::RecoverySaveCopy));
                }
                bar = bar.push(button("丢弃").on_press(Message::DiscardRecovery));
            }
        }
        if self.is_selftest() {
            let mut rewrite = button("Agent 改写一段");
            let mut append = button("Agent 追加一段");
            if self.ready {
                rewrite = rewrite.on_press(Message::StandIn(StandIn::RewriteParagraph(0)));
                append = append.on_press(Message::StandIn(StandIn::Append));
            }
            bar = bar.push(rewrite).push(append);
        }
        if let Some(warning) = self.workbench.recovery_warning() {
            bar = bar.push(text(warning));
        }
        if let Some(error) = &self.last_error {
            bar = bar.push(text(format!("操作失败：{error}")));
        }
        let mut retry = button("重试");
        if matches!(self.creation, CreationState::Failed { .. }) {
            retry = retry.on_press(Message::Retry);
        }
        bar = bar.push(retry);
        if self.pending_transition.is_some() {
            bar = bar.push(button("取消切换").on_press(Message::TransitionCancel));
        }

        let mut docs = row![].spacing(6).align_y(iced::Alignment::Center);
        if !self.workbench.documents().is_empty() {
            docs = docs.push(text("文稿："));
            for document in self.workbench.documents().iter().take(8) {
                docs = docs.push(
                    button(text(document.title.clone()))
                        .on_press(Message::OpenDocument(document.id.clone())),
                );
            }
        }
        column![bar, docs]
            .spacing(4)
            .height(iced::Length::Fixed(BAR_HEIGHT as f32))
            .into()
    }
}

impl Default for MarkdownPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
