//! Markdown 面板(ADR-013):`digger.markdown`,第一方 Builtin 面板。
//!
//! 独立性(bytehost 的设计原则):
//! - 文本、块、协议规则全在 `digger-editor`(纯库);本模块只做宿主适配——webview、几何、iced 栏。
//! - 不依赖其他面板、不依赖 `app-host` / `bytehost-apps`;`product.rs` 只持有一个 [`MarkdownPanel`]
//!   并转发它的消息与几何,和 `apps` / `usage` 的接法一致。
//! - webview 由 [`host`] 的第一方构建器建,句柄表私有,与应用 webview 永不共用一个槽。
//! - 每条上行消息带 generation:切换/重建之后迟到的旧消息被丢弃,绝不应用。
//!
//! 本切片**不持久化**:文档是内存里的示例章节,面板上写明「未持久化 / 示例章节」。

mod host;

pub use host::Probe;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytehost_core::LogicalRect;
use digger_editor::bridge::{Bridge, Handled, Mode, Report, StandIn};
use digger_editor::protocol::EditorOutbound;
use iced::{Element, Task, window};

/// 面板顶栏高度:wry 子视图永远画在 iced 内容之上(ADR-001 §3),所以 webview 矩形要让出这一条。
pub const BAR_HEIGHT: f64 = 56.0;

const SAMPLE: &str = include_str!("sample.md");

/// 自检模式的环境变量:设置后用 harness 页面启动,自动跑 T1–T23 并以退出码报告。
const SELFTEST_ENV: &str = "DIGGER_EDITOR_SELFTEST";

#[derive(Debug, Clone)]
pub enum Message {
    /// 页面发来的原始消息,带它所属 webview 的 generation。
    Page(u64, String),
    Built(u64, Result<(), String>),
    /// 用户点「重试」:失败后真实重建一个新 generation。
    Retry,
    /// 生命周期自检的探针结果。
    Probed(Probe),
    /// 顶栏按钮:替身 Agent。
    StandIn(StandIn),
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

pub struct MarkdownPanel {
    bridge: Bridge,
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
        Self {
            bridge: Bridge::new(SAMPLE, mode, nonce.clone()),
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
            tx,
            rx: Some(rx),
        }
    }

    /// 自检模式下产品壳启动即打开本面板。
    pub fn is_selftest(&self) -> bool {
        self.bridge.mode() == Mode::SelfTest
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
            Message::Page(generation, raw) => {
                if !self.is_live() || generation != self.generation() {
                    return Task::none(); // 迟到的旧 generation:丢弃,绝不应用
                }
                match self.bridge.handle(&raw) {
                    Handled::Reply(out) => {
                        match &out {
                            EditorOutbound::Open { .. } => self.ready = true,
                            EditorOutbound::Reject { reason, .. } => {
                                self.rejects += 1;
                                self.last_reject = Some(reason.clone());
                            }
                            _ => {}
                        }
                        self.push(&out)
                    }
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

    /// 顶栏:状态 + 替身 Agent 按钮。
    pub fn view(&self) -> Element<'_, Message> {
        use iced::widget::{button, column, row, text};
        let doc = self.bridge.doc();
        let status = if let Some(error) = self.failed_error() {
            format!("编辑器创建失败：{error}")
        } else if self.is_selftest() {
            "自检中…".to_string()
        } else if self.ready {
            let sync = match &self.last_reject {
                Some(reason) => format!(" · 拒绝 {}（最近：{reason}）", self.rejects),
                None => String::new(),
            };
            format!("版本 {} · {} 个字符{sync}", doc.version, doc.char_count())
        } else {
            match &self.creation {
                CreationState::Building { .. } | CreationState::Idle => "正在打开…".to_string(),
                CreationState::Closed => "编辑器已关闭".to_string(),
                CreationState::Failed { .. } => "编辑器未打开".to_string(),
                CreationState::Ready { .. } => "正在等待页面…".to_string(),
            }
        };
        let mut rewrite = button("Agent 改写一段");
        let mut append = button("Agent 追加一段");
        if self.ready {
            rewrite = rewrite.on_press(Message::StandIn(StandIn::RewriteParagraph(0)));
            append = append.on_press(Message::StandIn(StandIn::Append));
        }
        let mut retry = button("重试");
        if matches!(self.creation, CreationState::Failed { .. }) {
            retry = retry.on_press(Message::Retry);
        }
        column![
            row![
                text("未持久化 / 示例章节"),
                text(status),
                rewrite,
                append,
                retry
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
        ]
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
