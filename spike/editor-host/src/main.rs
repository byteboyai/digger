//! 编辑器宿主 spike(ADR-001 验证项 1–4):iced 窗口 + wry 子视图 + Tiptap 宿主。
//!
//! `SELFTEST=<mode>` 自动执行并打印 PASS/FAIL:
//!   basic | merge | fuzz | perf | inputrule | policy | crash-a | crash-b-accept | crash-b-discard | crash-b-stale
//! crash-b-* 需要 `SPIKE_DIR` 指向 crash-a 留下的数据目录,见 run_recovery_test.sh。

mod bridge;
mod doc;
mod history;
mod hosts;
mod merge;
mod probes;
mod store;

use bridge::{Binding, Validator};
use bytehost_webview as _;
use doc::{Block, Kind, Op};
use iced::futures::SinkExt;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use merge::{Resolution, merge};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use history::History;
use store::{Journal, Store};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const LEFT_W: f32 = 340.0;
const TOP_BAR_H: f32 = 40.0;
const RECOVERY_BAR_H: f32 = 44.0;
const MAX_FRAME: usize = 2 * 1024 * 1024;
const HOST_TIPTAP: &str = include_str!("../host/dist-tiptap/tiptap.html");
const HOST_MILKDOWN: &str = include_str!("../host/dist-milkdown/milkdown.html");

/// `HOST=milkdown` 选 Milkdown 宿主,默认 Tiptap。
fn host_html() -> &'static str {
    if std::env::var("HOST").as_deref() == Ok("milkdown") { HOST_MILKDOWN } else { HOST_TIPTAP }
}

thread_local! {
    static HOST: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}
static TX: OnceLock<UnboundedSender<String>> = OnceLock::new();
static RX: Mutex<Option<UnboundedReceiver<String>>> = Mutex::new(None);

#[derive(Debug, Clone)]
enum Message {
    Ready(window::Id, Size),
    HostBuilt(Result<(), String>),
    Resized(window::Id, Size),
    Bridge(String),
    RecoveryAccept,
    RecoveryDiscard,
    Report,
    Timeout,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Crash {
    Accept,
    Discard,
    Stale,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Interactive,
    Basic,
    Merge,
    Perf,
    Fuzz,
    Policy,
    InputRule,
    CrashA,
    CrashB(Crash),
}

struct Recovery {
    candidate: Vec<Block>,
    ops: Vec<String>,
    conflicts: Vec<String>,
}

struct PendingConflict {
    base_rev: u64,
    local: Vec<Block>,
    /// 产生该冲突的那条 edit 的 seq:解决后的 rebase 只对它有效。
    seq: u64,
}

struct State {
    window: Option<window::Id>,
    size: Size,
    store: Store,
    binding: Binding,
    validator: Validator,
    blocks: Vec<Block>,
    rev: u64,
    /// 持久化的增量历史:追加式日志 + 检查点,按需重建任意旧版本。
    history: History,
    commits: u32,
    log: Vec<String>,
    changes: Vec<String>,
    mode: Mode,
    stage: u32,
    original_ids: Vec<String>,
    snapshots: u32,
    rejected: Vec<String>,
    viewports: Vec<(f64, f64)>,
    recovery: Option<Recovery>,
    pending_conflict: Option<PendingConflict>,
    disk_rev: u64,
    mark_rev: u64,
    mark_commits: u32,
    fails: u32,
    /// Rust 认为宿主基线所处的正式版本(load / committed / advance / rebase 发出时更新)。
    host_rev: u64,
    resyncs: u32,
    rng: u64,
    fuzz_steps: u32,
    fuzz_bad: u32,
    down_bytes: usize,
    down_max: usize,
    apply_ms_max: f64,
    probe_idx: usize,
    probe_port: u16,
    probe_results: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    probe_recs: std::collections::HashMap<String, hosts::Recorder>,
}

fn state_ref(b: &Binding, rev: u64) -> String {
    format!("doc:{}@{}", b.document_id, rev)
}
fn parse_ref(r: &str) -> Option<u64> {
    r.rsplit('@').next()?.parse().ok()
}
fn seq(tasks: Vec<Task<Message>>) -> Task<Message> {
    tasks.into_iter().fold(Task::none(), |a, b| a.chain(b))
}
fn contains(texts: &[String], i: usize, needle: &str) -> bool {
    texts.get(i).is_some_and(|t| t.contains(needle))
}
fn texts_of(doc: &Value) -> Vec<String> {
    doc::parse_pm(doc).map(|(b, _)| b.into_iter().map(|b| b.text).collect()).unwrap_or_default()
}

fn describe(op: &Op) -> String {
    match op {
        Op::InsertBlock { index, .. } => format!("InsertBlock @{index}"),
        Op::DeleteBlock { id } => format!("DeleteBlock {}", &id[id.len().saturating_sub(6)..]),
        Op::MoveBlock { to, .. } => format!("MoveBlock →{to}"),
        Op::ReplaceBlockContent { text, .. } => format!("ReplaceBlockContent \"{text}\""),
        Op::SetBlockAttributes { kind, .. } => format!("SetBlockAttributes {kind:?}"),
    }
}

impl State {
    fn new() -> Self {
        let mode = match std::env::var("SELFTEST").as_deref() {
            Err(_) => Mode::Interactive,
            Ok("merge") => Mode::Merge,
            Ok("perf") => Mode::Perf,
            Ok("fuzz") => Mode::Fuzz,
            Ok("policy") => Mode::Policy,
            Ok("inputrule") => Mode::InputRule,
            Ok("crash-a") => Mode::CrashA,
            Ok("crash-b-accept") => Mode::CrashB(Crash::Accept),
            Ok("crash-b-discard") => Mode::CrashB(Crash::Discard),
            Ok("crash-b-stale") => Mode::CrashB(Crash::Stale),
            Ok(_) => Mode::Basic,
        };
        let dir = match std::env::var("SPIKE_DIR") {
            Ok(d) => std::path::PathBuf::from(d),
            Err(_) if mode == Mode::Interactive => std::env::temp_dir().join("digger-spike"),
            Err(_) => std::env::temp_dir().join(format!("digger-spike-{}", uuid::Uuid::new_v4())),
        };
        let store = Store::new(dir.clone());
        let hist_dir = dir.join("history");
        let history = if History::exists(&hist_dir) {
            History::open(&hist_dir).expect("open history (corrupt log?)")
        } else {
            let binding = Binding {
                project_id: uuid::Uuid::now_v7().to_string(),
                panel_id: uuid::Uuid::now_v7().to_string(),
                document_id: uuid::Uuid::now_v7().to_string(),
            };
            let mut blocks = vec![
                Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Heading(1), text: "第一章 开端".into() },
                Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: "夜色渐深，城市的灯火一盏盏亮起。".into() },
                Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: "他推开门，雨声涌了进来。".into() },
            ];
            if mode == Mode::Perf || mode == Mode::Fuzz {
                let default = if mode == Mode::Fuzz { 30 } else { 1500 };
                let n: usize = std::env::var("BLOCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(default);
                let line = "夜色渐深，城市的灯火一盏盏亮起，他推开门，雨声涌了进来，所有的往事像潮水一样漫过脚踝。";
                for i in 0..n {
                    blocks.push(Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: format!("{i}. {line}") });
                }
            }
            History::create(&hist_dir, binding, 1, &blocks, history::DEFAULT_CHECKPOINT_EVERY).expect("create history")
        };
        let mut history = history;
        // 保留策略:历史超过 2000 个版本时,在启动时压缩到最近 500 个版本之前的检查点;
        // 恢复日志仍引用的基线不能丢。宿主基线都是本次会话内的新版本,不受影响。
        if history.head() - history.oldest() > 2000 {
            let journal_base = store.load_journal().map(|j| j.base_rev).unwrap_or(u64::MAX);
            let floor = history.head().saturating_sub(500).min(journal_base);
            if floor > history.oldest() {
                let _ = history.compact(floor);
            }
        }
        let head = history.head();
        let head_blocks = history.blocks_at(head).expect("rebuild head").as_ref().clone();
        let mut s = Self {
            window: None,
            size: Size::new(1100.0, 720.0),
            validator: Validator::new(history.binding.clone(), MAX_FRAME),
            binding: history.binding.clone(),
            original_ids: head_blocks.iter().map(|b| b.id.clone()).collect(),
            blocks: head_blocks,
            rev: head,
            disk_rev: head,
            history,
            store,
            commits: 0,
            log: vec![],
            changes: vec![],
            mode,
            stage: 0,
            snapshots: 0,
            rejected: vec![],
            viewports: vec![],
            recovery: None,
            pending_conflict: None,
            mark_rev: 0,
            mark_commits: 0,
            fails: 0,
            host_rev: 0,
            resyncs: 0,
            rng: std::env::var("FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x9E37_79B9_7F4A_7C15_u64) | 1,
            fuzz_steps: 0,
            fuzz_bad: 0,
            down_bytes: 0,
            down_max: 0,
            apply_ms_max: 0.0,
            probe_idx: 0,
            probe_port: 0,
            probe_results: Default::default(),
            probe_recs: Default::default(),
        };
        if mode == Mode::CrashB(Crash::Stale) {
            // 应用关闭期间 Agent 已经改过正式状态(与未提交缓冲区不冲突的块)。
            s.agent_edit(2, |t| format!("{t} [Agent-offline]"));
            s.commits = 0; // 不计入本会话的用户提交
        }
        s.recovery = s.detect_recovery();
        s
    }

    fn selftest(&self) -> bool {
        self.mode != Mode::Interactive
    }

    fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        if self.selftest() {
            println!("{line}");
        }
        self.log.push(line);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    fn check(&mut self, name: &str, cond: bool) {
        println!("{}: {name}", if cond { "PASS" } else { "FAIL" });
        if !cond {
            self.fails += 1;
        }
    }

    fn debounce_ms(&self) -> u64 {
        match self.mode {
            Mode::Interactive => 1200,
            Mode::Basic => 0,
            _ => 600_000, // 自测里由脚本显式 flush,编辑一直停在"未发送"
        }
    }

    fn bounds(&self) -> wry::Rect {
        use wry::dpi::{LogicalPosition, LogicalSize};
        let top = TOP_BAR_H + if self.recovery.is_some() { RECOVERY_BAR_H } else { 0.0 };
        wry::Rect {
            position: LogicalPosition::new(LEFT_W as f64, top as f64).into(),
            size: LogicalSize::new((self.size.width - LEFT_W).max(1.0) as f64, (self.size.height - top).max(1.0) as f64).into(),
        }
    }

    fn apply_bounds(&self) {
        let b = self.bounds();
        HOST.with(|h| {
            if let Some(wv) = h.borrow().as_ref() {
                let _ = wv.set_bounds(b);
            }
        });
    }

    /// 提交一个新的正式版本(spike 里 Draft Change → Commit 合为一步)。
    fn commit(&mut self, blocks: Vec<Block>, label: &str) -> String {
        let summary = doc::diff(&self.blocks, &blocks).iter().map(describe).collect::<Vec<_>>().join("; ");
        let rev = self.rev + 1;
        // 先落盘(追加一条增量并 fsync),成功后才改内存:崩溃时磁盘与内存不会出现"内存领先"。
        self.history.append(rev, &self.blocks, &blocks, label).expect("append to history log");
        self.blocks = blocks;
        self.rev = rev;
        self.commits += 1;
        self.changes.push(format!("rev {} [{label}]: {summary}", self.rev));
        // 日志里的缓冲区若已等于正式状态,就不再需要。
        if let Some(j) = self.store.load_journal() {
            if self.journal_blocks(&j).is_some_and(|b| b == self.blocks) {
                self.store.clear_journal();
            }
        }
        summary
    }

    /// 模拟 Agent / 其他写入者推进正式状态:改第 `idx` 块的文本。
    fn agent_edit(&mut self, idx: usize, f: impl FnOnce(&str) -> String) {
        let mut b = self.blocks.clone();
        b[idx].text = f(&b[idx].text);
        self.commit(b, "agent");
    }

    /// 还原日志里的缓冲区:把增量应用到它所基于的历史版本上。
    fn journal_blocks(&self, j: &Journal) -> Option<Vec<Block>> {
        let base = self.history.blocks_at(j.base_rev)?;
        let (delta, _) = doc::parse_delta(&j.delta).ok()?;
        doc::apply_delta(&base, &delta).ok()
    }

    fn rnd(&mut self, n: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n.max(1) as u64) as usize
    }

    /// 下行:相对宿主基线(`history[host_rev]`)的块级增量,宿主只在 `fromRev` 与自己的基线一致且干净时才应用。
    /// `claim_from` 仅供自测伪造错误的 fromRev。
    fn advance_msg(&mut self, claim_from: Option<u64>) -> Value {
        let from = self.host_rev;
        let base = self.history.blocks_at(from).expect("host baseline revision is still in the history");
        let delta = doc::make_delta(&base, &self.blocks);
        self.host_rev = self.rev;
        let m = json!({"fromRev": claim_from.unwrap_or(from), "rev": self.rev, "stateRef": state_ref(&self.binding, self.rev), "delta": delta.to_json()});
        let n = m.to_string().len();
        self.down_bytes += n;
        self.down_max = self.down_max.max(n);
        m
    }

    fn fuzz_mutate(&mut self) {
        let mut b = self.blocks.clone();
        for _ in 0..1 + self.rnd(3) {
            match self.rnd(5) {
                0 if b.len() > 3 => {
                    let i = self.rnd(b.len());
                    b.remove(i);
                }
                1 => {
                    let i = self.rnd(b.len());
                    b[i].text.push('字');
                }
                2 => {
                    let kind = if self.rnd(4) == 0 { Kind::Heading(2) } else { Kind::Paragraph };
                    let at = self.rnd(b.len() + 1);
                    b.insert(at, Block { id: uuid::Uuid::new_v4().to_string(), kind, text: format!("新块{}", self.fuzz_steps) });
                }
                3 if b.len() > 1 => {
                    let from = self.rnd(b.len());
                    let blk = b.remove(from);
                    let to = self.rnd(b.len() + 1);
                    b.insert(to, blk);
                }
                _ => {
                    let i = self.rnd(b.len());
                    b[i].kind = if b[i].kind == Kind::Paragraph { Kind::Heading(2) } else { Kind::Paragraph };
                }
            }
        }
        self.commit(b, "agent");
    }

    fn doc_msg(&self) -> Value {
        json!({"stateRef": state_ref(&self.binding, self.rev), "rev": self.rev, "doc": doc::to_pm(&self.blocks)})
    }

    fn detect_recovery(&mut self) -> Option<Recovery> {
        let j = self.store.load_journal()?;
        if j.document_id != self.binding.document_id {
            return None;
        }
        let local = self.journal_blocks(&j)?;
        let (candidate, conflicts) = if j.base_rev == self.rev {
            (local, vec![])
        } else {
            let base = self.history.blocks_at(j.base_rev)?;
            let m = merge(&base, &local, &self.blocks, None);
            if m.conflicts.is_empty() {
                (m.blocks, vec![])
            } else {
                (merge(&base, &local, &self.blocks, Some(Resolution::Mine)).blocks, m.conflicts)
            }
        };
        if candidate == self.blocks {
            self.store.clear_journal();
            return None;
        }
        let ops = doc::diff(&self.blocks, &candidate).iter().map(describe).collect();
        Some(Recovery { candidate, ops, conflicts })
    }

    fn accept_recovery(&mut self, win: window::Id) -> Task<Message> {
        let Some(r) = self.recovery.take() else { return Task::none() };
        let summary = self.commit(r.candidate, "recovery");
        self.store.clear_journal();
        self.note(format!("recovery accepted → Commit rev {} [{summary}]", self.rev));
        self.apply_bounds();
        let m = self.advance_msg(None);
        send_host(win, "official_advanced", m)
    }

    fn discard_recovery(&mut self) {
        self.recovery = None;
        self.store.clear_journal();
        self.note("recovery discarded");
        self.apply_bounds();
    }
}

thread_local! {
    static PROBES: RefCell<Vec<wry::WebView>> = const { RefCell::new(Vec::new()) };
}

fn launch_probe(s: &mut State, win: window::Id) -> Task<Message> {
    let p = &probes::PROBES[s.probe_idx];
    let rec = hosts::Recorder::default();
    s.probe_recs.insert(p.tag.to_string(), rec.clone());
    let origin_url = format!("http://policy-a.localhost:{}/", s.probe_port);
    let is_app = p.class == hosts::Class::App;
    let spec = hosts::Build {
        class: p.class,
        bounds: s.bounds(),
        app: is_app.then(|| (bytehost_webview::AppOrigin::from_url(&origin_url).expect("app origin"), p.store_id.to_string())),
        tag: Some(p.tag.to_string()),
        forward: TX.get().unwrap().clone(),
        rec,
    };
    let load_url = format!("{origin_url}?t={}&m={}", p.tag, p.mode);
    s.note(format!("probe `{}` ({:?})", p.tag, p.class));
    window::run(win, move |w| {
        let (wb, _) = hosts::builder(&spec);
        let wb = if is_app { wb.with_url(&load_url) } else { wb.with_html(probes::EDITOR_PROBE_HTML) };
        let wv = wb.build_as_child(&w).map_err(|e| e.to_string())?;
        PROBES.with(|v| v.borrow_mut().push(wv));
        Ok(())
    })
    .map(Message::HostBuilt)
}

fn policy_event(s: &mut State, win: window::Id, raw: &str) -> Task<Message> {
    let Some((tag, k, v)) = probes::parse_event(raw) else { return Task::none() };
    s.probe_results.entry(tag.clone()).or_default().insert(k.clone(), v);
    if k != "done" || Some(tag.as_str()) != probes::PROBES.get(s.probe_idx).map(|p| p.tag) {
        return Task::none();
    }
    let p = &probes::PROBES[s.probe_idx];
    let results = probes::evaluate(p, &s.probe_results[&tag], &s.probe_recs[&tag]);
    for (name, ok) in results {
        s.check(&name, ok);
    }
    s.probe_idx += 1;
    if s.probe_idx < probes::PROBES.len() {
        launch_probe(s, win)
    } else {
        finish(s)
    }
}

fn boot() -> (State, Task<Message>) {
    let (tx, rx) = unbounded_channel();
    let _ = TX.set(tx);
    *RX.lock().unwrap() = Some(rx);
    let t = window::oldest().then(|id| match id {
        Some(id) => window::size(id).map(move |s| Message::Ready(id, s)),
        None => Task::none(),
    });
    (State::new(), t)
}

fn send_host(id: window::Id, ty: &str, payload: Value) -> Task<Message> {
    let msg = json!({"type": ty, "payload": payload}).to_string();
    let js = format!("window.__digger.receive({})", serde_json::to_string(&msg).unwrap());
    window::run(id, move |_| {
        HOST.with(|h| {
            if let Some(wv) = h.borrow().as_ref() {
                let _ = wv.evaluate_script(&js);
            }
        })
    })
    .discard()
}

fn st(win: window::Id, ty: &str, payload: Value) -> Task<Message> {
    send_host(win, ty, payload)
}

fn delayed_report() -> Task<Message> {
    Task::perform(tokio::time::sleep(std::time::Duration::from_millis(400)), |_| Message::Report)
}

fn update(s: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::Ready(id, size) => {
            s.window = Some(id);
            s.size = size;
            let bounds = s.bounds();
            let script = format!("window.__DIGGER_BINDING__ = {};", serde_json::to_string(&s.binding).unwrap());
            let tx = TX.get().unwrap().clone();
            let html = host_html();
            let policy_mode = s.mode == Mode::Policy;
            if policy_mode {
                s.probe_port = probes::start_server(tx.clone());
                return launch_probe(s, id);
            }
            // 生产路径与探针共用 `hosts::builder`:编辑器宿主的每项 wry 设置都来自 `hosts::pinned(Class::Editor)`。
            let spec = hosts::Build { class: hosts::Class::Editor, bounds, app: None, tag: None, forward: tx, rec: hosts::Recorder::default() };
            let build = window::run(id, move |w| {
                let (wb, _) = hosts::builder(&spec);
                let wv = wb
                    .with_initialization_script(script)
                    .with_html(html)
                    .build_as_child(&w)
                    .map_err(|e| e.to_string())?;
                HOST.with(|h| *h.borrow_mut() = Some(wv));
                Ok(())
            })
            .map(Message::HostBuilt);
            if s.selftest() {
                Task::batch([build, Task::perform(tokio::time::sleep(std::time::Duration::from_secs(60)), |_| Message::Timeout)])
            } else {
                build
            }
        }
        Message::HostBuilt(r) => {
            s.note(format!("host built: {r:?}"));
            Task::none()
        }
        Message::Resized(id, size) => {
            s.size = size;
            if Some(id) == s.window {
                s.apply_bounds();
            }
            Task::none()
        }
        Message::Timeout => {
            println!("FAIL: self-test timed out at stage {}", s.stage);
            std::process::exit(2);
        }
        Message::RecoveryAccept => s.window.map(|w| s.accept_recovery(w)).unwrap_or_else(Task::none),
        Message::RecoveryDiscard => {
            s.discard_recovery();
            Task::none()
        }
        Message::Report => s.window.map(|w| st(w, "st_report", json!({}))).unwrap_or_else(Task::none),
        Message::Bridge(raw) => {
            let Some(win) = s.window else { return Task::none() };
            if s.mode == Mode::Policy {
                return policy_event(s, win, &raw);
            }
            match s.validator.accept(&raw) {
                Err(r) => {
                    s.note(format!("REJECT {r:?}"));
                    Task::none()
                }
                Ok(env) => on_envelope(s, win, env),
            }
        }
    }
}

fn viewport_of(p: &Value) -> (f64, f64) {
    (p["viewport"]["w"].as_f64().unwrap_or(0.0), p["viewport"]["h"].as_f64().unwrap_or(0.0))
}

fn on_envelope(s: &mut State, win: window::Id, env: bridge::Envelope) -> Task<Message> {
    let p = &env.payload;
    match env.ty.as_str() {
        "ready" => {
            s.note(format!("← ready viewport {}", p["viewport"]));
            s.viewports.push(viewport_of(p));
            let mut load = s.doc_msg();
            s.host_rev = s.rev;
            load["editDebounceMs"] = json!(s.debounce_ms());
            let load = send_host(win, "load", load);
            match s.mode {
                Mode::Interactive => load,
                Mode::Basic => seq(vec![load, window::resize(win, Size::new(900.0, 600.0)), st(win, "selftest", json!({}))]),
                Mode::Policy => Task::none(), // 探针模式没有编辑器宿主
                Mode::InputRule => seq(vec![load, st(win, "st_inputrule", json!({}))]),
                Mode::Fuzz => {
                    s.stage = 1;
                    seq(vec![load, st(win, "st_report", json!({}))])
                }
                Mode::Perf => {
                    s.stage = 1;
                    seq(vec![load, st(win, "st_perf", json!({}))])
                }
                Mode::Merge => {
                    s.stage = 1;
                    seq(vec![load, st(win, "st_type", json!({"block": 1, "text": " [本地]"})), st(win, "st_report", json!({}))])
                }
                Mode::CrashA => seq(vec![load, st(win, "st_type", json!({"block": 1, "text": " [未提交]"}))]),
                Mode::CrashB(_) => {
                    s.stage = 1;
                    seq(vec![load, st(win, "st_report", json!({}))])
                }
            }
        }
        "edit" => on_edit(s, win, p, env.seq),
        "resolve" => {
            let Some(pc) = s.pending_conflict.take() else {
                s.note("← resolve without a pending conflict (dropped)");
                return Task::none();
            };
            let res = if p["choice"] == "mine" { Resolution::Mine } else { Resolution::Official };
            let base = s.history.blocks_at(pc.base_rev).expect("conflict base is in the history");
            let m = merge(&base, &pc.local, &s.blocks, Some(res));
            if m.blocks != s.blocks {
                let summary = s.commit(m.blocks, "resolved");
                s.note(format!("← resolve({:?}) → Commit rev {} [{summary}]", res, s.rev));
            }
            send_rebase(s, win, "resolved", pc.seq, &pc.local)
        }
        "resync" => {
            s.resyncs += 1;
            s.note(format!("← resync requested ({}) → full reload", p["reason"].as_str().unwrap_or("?")));
            s.host_rev = s.rev;
            let mut load = s.doc_msg();
            load["editDebounceMs"] = json!(s.debounce_ms());
            let t = send_host(win, "load", load);
            if s.mode == Mode::Merge && s.stage == 19 {
                s.stage = 20;
                return t.chain(st(win, "st_report", json!({})));
            }
            t
        }
        "snapshot" => {
            s.snapshots += 1;
            let base_rev = parse_ref(p["baseRev"].as_str().unwrap_or("")).unwrap_or(0);
            let delta_bytes = p.to_string().len();
            let local = s
                .history
                .blocks_at(base_rev)
                .and_then(|base| doc::parse_delta(p).ok().and_then(|(d, _)| doc::apply_delta(&base, &d).ok()));
            let equal = local.as_ref().is_some_and(|b| *b == s.blocks);
            if equal {
                s.store.clear_journal();
            } else {
                // 无法还原(未知基线/非法增量)的缓冲区也照原样留存:日志是原始缓冲区,不是已校验的状态。
                s.store.save_journal(&Journal { document_id: s.binding.document_id.clone(), base_rev, delta: json!({
                    "changed": p["changed"], "removed": p["removed"], "order": p["order"] }) });
            }
            s.note(format!("← snapshot #{} ({delta_bytes} B) {}", s.snapshots, if equal { "(= official, journal cleared)" } else { "→ journal" }));
            if s.mode == Mode::CrashA && !equal {
                // 独立重新打开磁盘上的日志:证明提交确实已落盘,而不是只存在于内存。
                let disk_ok = History::open(&s.store.dir().join("history"))
                    .ok()
                    .is_some_and(|h| h.head() == s.rev && h.blocks_at(h.head()).is_some_and(|b| *b == s.blocks) && h.verify().is_ok());
                println!("CRASH-A: journal fsynced, official on disk unchanged = {disk_ok}; aborting now");
                std::process::abort();
            }
            Task::none()
        }
        "title" => {
            s.note(format!("← resize viewport {}", p["viewport"]));
            s.viewports.push(viewport_of(p));
            Task::none()
        }
        "selftest_done" => {
            finish_basic(s);
            finish(s)
        }
        "selftest_report" => on_report(s, win, p),
        other => {
            s.note(format!("← {other} {}", if other == "selection" { String::new() } else { p.to_string() }));
            Task::none()
        }
    }
}

/// 下行 rebase:相对宿主"已发送快照"(= `sent`,即宿主在发出 ack_seq 那条 edit 时的文档)的块级增量。
fn send_rebase(s: &mut State, win: window::Id, reason: &str, ack_seq: u64, sent: &[Block]) -> Task<Message> {
    let delta = doc::make_delta(sent, &s.blocks);
    s.host_rev = s.rev;
    let m = json!({"ackSeq": ack_seq, "rev": s.rev, "stateRef": state_ref(&s.binding, s.rev), "reason": reason, "delta": delta.to_json()});
    let t = send_host(win, "rebase", m);
    Task::batch([t]).chain(hook(s, win, "rebase"))
}

fn on_edit(s: &mut State, win: window::Id, p: &Value, edit_seq: u64) -> Task<Message> {
    let base = p["baseRev"].as_str().unwrap_or("");
    let Some(base_rev) = parse_ref(base) else {
        s.rejected.push("bad base".into());
        return send_host(win, "rejected", json!({"reason": "bad base"}));
    };
    // 宿主的基线 == history[baseRev]:增量只能应用在它之上,而不是当前正式版本之上。
    let Some(base_blocks) = s.history.blocks_at(base_rev) else {
        s.rejected.push("unknown base".into());
        return send_host(win, "rejected", json!({"reason": "unknown base"}));
    };
    let applied = doc::parse_delta(p).and_then(|(d, w)| doc::apply_delta(&base_blocks, &d).map(|b| (b, w, d.changed.len())));
    let (local, warnings, n_changed) = match applied {
        Ok(v) => v,
        Err(e) => {
            s.note(format!("← edit REJECTED invalid {e:?}"));
            s.rejected.push(format!("{e:?}"));
            return send_host(win, "rejected", json!({"reason": format!("{e:?}")}));
        }
    };
    if base_rev == s.rev {
        if local != s.blocks {
            let summary = s.commit(local, "human");
            s.note(format!("← edit Δ{n_changed} block(s) → Commit rev {} [{summary}] {warnings:?}", s.rev));
        } else {
            s.note("← edit (no semantic change)");
        }
        let m = json!({"stateRef": state_ref(&s.binding, s.rev), "rev": s.rev});
        s.host_rev = s.rev;
        return send_host(win, "committed", m);
    }
    // 基线过期:正式状态在宿主有未发送编辑期间前进了 → 块级三方合并,绝不覆盖宿主的会话。
    let m = merge(&base_blocks, &local, &s.blocks, None);
    if m.conflicts.is_empty() {
        s.note(format!("← edit on stale base {base_rev} (official is {}): auto-merged", s.rev));
        if m.blocks != s.blocks {
            let summary = s.commit(m.blocks, "merged");
            s.note(format!("   → Commit rev {} [{summary}]", s.rev));
        }
        s.pending_conflict = None;
        send_rebase(s, win, "merged", edit_seq, &local)
    } else {
        s.note(format!("← edit on stale base {base_rev}: CONFLICT on {} block(s), nothing committed", m.conflicts.len()));
        let items: Vec<Value> = m
            .conflicts
            .iter()
            .map(|id| {
                let mine = local.iter().find(|b| &b.id == id).map(|b| b.text.clone()).unwrap_or_default();
                let off = s.blocks.iter().find(|b| &b.id == id).map(|b| b.text.clone()).unwrap_or_default();
                json!({"blockId": id, "mine": mine, "official": off})
            })
            .collect();
        s.pending_conflict = Some(PendingConflict { base_rev, local, seq: edit_seq });
        let mut msg = s.doc_msg();
        msg["conflicts"] = json!(items);
        send_host(win, "conflict", msg).chain(hook(s, win, "conflict"))
    }
}

/// 自测脚本的"Rust 已发出 rebase/conflict"挂钩:推进到下一阶段并请求宿主报告状态。
fn hook(s: &mut State, win: window::Id, event: &str) -> Task<Message> {
    let report = st(win, "st_report", json!({}));
    match (s.mode, s.stage, event) {
        (Mode::Merge, 3, "rebase") | (Mode::Merge, 9, "rebase") | (Mode::Merge, 15, "rebase") | (Mode::Merge, 24, "rebase") => {
            s.stage += 1;
            report
        }
        (Mode::Merge, 7, "conflict") | (Mode::Merge, 13, "conflict") => {
            s.stage += 1;
            report
        }
        _ => Task::none(),
    }
}

fn on_perf(s: &mut State, r: &Value) -> Task<Message> {
    let p = &r["perf"];
    // Rust 侧代价:收下一条真实形状的块级 edit 帧 → 校验 → 解析增量 → 还原 → diff。
    let mid = s.blocks.len() / 2;
    let mut edited = s.blocks[mid].clone();
    edited.text.push('字');
    let one = doc::to_pm(std::slice::from_ref(&edited))["content"][0].clone();
    let mut vis = s.blocks.clone();
    vis.insert(mid, Block { id: "new-block".into(), kind: Kind::Paragraph, text: "新增".into() });
    let (binding, rev, blocks) = (s.binding.clone(), s.rev, s.blocks.clone());
    let run = |payload: Value| -> (usize, f64, usize, bool) {
        let frame = json!({"hostKind":"editor","binding":binding,"seq":1,"type":"edit","payload":payload}).to_string();
        let mut v = Validator::new(binding.clone(), MAX_FRAME);
        let t = std::time::Instant::now();
        let env = v.accept(&frame);
        let ok = env.as_ref().ok().and_then(|e| doc::parse_delta(&e.payload).ok()).and_then(|(d, _)| doc::apply_delta(&blocks, &d).ok());
        let ops = ok.as_ref().map(|b| doc::diff(&blocks, b).len()).unwrap_or(0);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        (frame.len(), ms, ops, env.is_ok())
    };
    let (f1, ms1, ops1, ok1) = run(json!({"baseRev": state_ref(&s.binding, rev), "changed": [one], "removed": []}));
    let newn = doc::to_pm(std::slice::from_ref(&vis[mid]))["content"][0].clone();
    let after = vis[mid - 1].id.clone();
    let next = vis[mid + 1].id.clone();
    let (f2, ms2, ops2, ok2) = run(json!({"baseRev": state_ref(&s.binding, rev), "changed": [newn], "removed": [],
        "placements": [{"id": "new-block", "after": after}, {"id": next, "after": "new-block"}]}));
    s.check("perf: single-block edit frame accepted by the bridge", ok1);
    s.check("perf: insert-block edit frame accepted by the bridge", ok2);
    println!(
        "PERF {} | blocks {} | load {:.1} ms | getJSON {:.1} ms ({} KB whole doc) | tx mean {:.2} ms p95 {:.2} ms | tx→paint {:.1} ms | host delta build {:.2} ms",
        p["adapter"].as_str().unwrap_or("?"), p["blocks"], p["loadMs"].as_f64().unwrap_or(0.0), p["getJsonMs"].as_f64().unwrap_or(0.0),
        p["jsonBytes"].as_u64().unwrap_or(0) / 1024, p["txMeanMs"].as_f64().unwrap_or(0.0), p["txP95Ms"].as_f64().unwrap_or(0.0),
        p["txToPaintMs"].as_f64().unwrap_or(0.0), p["deltaMs"].as_f64().unwrap_or(0.0),
    );
    println!(
        "PERF   edit frame: host single-block {} B, host split-block {} B | Rust frames: single {} B ({:.2} ms, {} op), insert {} B ({:.2} ms, {} ops)",
        p["deltaBytes"], p["structuralBytes"], f1, ms1, ops1, f2, ms2, ops2
    );
    s.check("perf: single-block edit yields exactly one semantic op", ops1 == 1);
    s.check("perf: insert yields InsertBlock + ReplaceBlockContent", ops2 == 2);
    finish(s)
}

fn on_report(s: &mut State, win: window::Id, r: &Value) -> Task<Message> {
    if !r["inputrule"].is_null() {
        let i = &r["inputrule"];
        println!("INPUTRULE {i}");
        s.check("input rule '## ' fired", i["handled"] == true);
        s.check("paragraph became a heading", i["type"] == "heading" && i["level"] == 2);
        s.check("block id preserved across the type change", i["before"] == i["after"] && i["after"].is_string());
        return finish(s);
    }
    if !r["perf"].is_null() {
        return on_perf(s, r);
    }
    let texts = texts_of(&r["doc"]);
    let off: Vec<String> = s.blocks.iter().map(|b| b.text.clone()).collect();
    let sess = |t: Task<Message>| t;
    match (s.mode, s.stage) {
        // ---------------- merge / rebase / conflict ----------------
        (Mode::Merge, 1) => {
            s.check("S1 host has the typed, unsent edit", contains(&texts, 1, "[本地]") && r["clean"] == false);
            s.agent_edit(2, |t| format!("{t} [Agent]"));
            s.stage = 2;
            let adv = { let m = s.advance_msg(None); send_host(win, "official_advanced", m) };
            sess(seq(vec![adv, st(win, "st_report", json!({}))]))
        }
        (Mode::Merge, 2) => {
            s.check("S1 official advanced while host was dirty", s.rev == 2);
            s.check("S1 host kept its unsent edit (not overwritten)", contains(&texts, 1, "[本地]"));
            s.check("S1 host did NOT silently take the official change", !contains(&texts, 2, "[Agent]"));
            s.stage = 3;
            st(win, "st_flush", json!({}))
        }
        (Mode::Merge, 4) => {
            s.check("S1 disjoint edits auto-merged: host has both", contains(&texts, 1, "[本地]") && contains(&texts, 2, "[Agent]"));
            s.check("S1 official has both", contains(&off, 1, "[本地]") && contains(&off, 2, "[Agent]"));
            s.check("S1 no conflict banner", r["conflictVisible"] == false && s.pending_conflict.is_none());
            s.check("S1 host clean and based on latest rev", r["clean"] == true && r["rev"] == s.rev);
            s.stage = 5;
            seq(vec![st(win, "st_type", json!({"block": 1, "text": " [X]"})), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 5) => {
            s.agent_edit(1, |_| "AGENT-P1".into());
            s.stage = 6;
            seq(vec![{ let m = s.advance_msg(None); send_host(win, "official_advanced", m) }, st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 6) => {
            s.check("S2 host kept [X] despite official_advanced", contains(&texts, 1, "[X]"));
            s.mark_rev = s.rev;
            s.stage = 7;
            st(win, "st_flush", json!({}))
        }
        (Mode::Merge, 8) => {
            s.check("S2 same-block edits → conflict banner shown", r["conflictVisible"] == true);
            s.check("S2 nothing committed while conflicted", s.rev == s.mark_rev && s.pending_conflict.is_some());
            s.check("S2 host still shows my text", contains(&texts, 1, "[X]"));
            s.stage = 9;
            st(win, "st_click", json!({"which": "mine"}))
        }
        (Mode::Merge, 10) => {
            s.check("S2 keep-mine: official has my text", contains(&off, 1, "[X]"));
            s.check("S2 keep-mine: host == official, banner gone", texts == off && r["conflictVisible"] == false);
            s.stage = 11;
            seq(vec![st(win, "st_type", json!({"block": 1, "text": " [Y]"})), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 11) => {
            s.agent_edit(1, |_| "AGENT2".into());
            s.stage = 12;
            seq(vec![{ let m = s.advance_msg(None); send_host(win, "official_advanced", m) }, st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 12) => {
            s.stage = 13;
            st(win, "st_flush", json!({}))
        }
        (Mode::Merge, 14) => {
            s.check("S3 conflict banner shown", r["conflictVisible"] == true);
            s.stage = 15;
            st(win, "st_click", json!({"which": "official"}))
        }
        (Mode::Merge, 16) => {
            s.check("S3 take-official: official keeps agent text", off.get(1).map(String::as_str) == Some("AGENT2"));
            s.check("S3 take-official: host shows official text", texts == off && !contains(&texts, 1, "[Y]"));
            s.mark_commits = s.commits;
            s.agent_edit(2, |t| format!("{t}!"));
            s.mark_commits = s.commits; // agent 提交不算宿主提交
            s.stage = 17;
            seq(vec![{ let m = s.advance_msg(None); send_host(win, "official_advanced", m) }, st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 17) => {
            s.check("S4 clean host applies official_advanced", texts == off);
            s.stage = 18;
            delayed_report()
        }
        (Mode::Merge, 18) => {
            s.check("S4 applying official state produced no spurious edit/commit", s.commits == s.mark_commits);
            s.check("S4 block-scoped downstream needed no resync", s.resyncs == 0);
            // S5: a downstream delta that claims the wrong baseline must NOT be applied; the host asks for a resync.
            s.agent_edit(2, |t| format!("{t}?"));
            let wrong = s.rev + 7;
            let m = s.advance_msg(Some(wrong));
            s.stage = 19;
            send_host(win, "official_advanced", m)
        }
        (Mode::Merge, 20) => {
            s.check("S5 mismatching fromRev → host asked for exactly one resync", s.resyncs == 1);
            s.check("S5 after the resync the host equals the official state", texts == off);
            // S6: a correct delta changes only the changed block; untouched nodes keep their identity.
            let (id0, id1, id2) = (s.blocks[0].id.clone(), s.blocks[1].id.clone(), s.blocks[2].id.clone());
            s.original_ids = vec![id0, id1, id2];
            s.agent_edit(2, |t| format!("{t}#"));
            s.stage = 21;
            let m = s.advance_msg(None);
            seq(vec![st(win, "st_mark", json!({})), send_host(win, "official_advanced", m), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 21) => {
            let [id0, id1, id2] = [0, 1, 2].map(|i| s.original_ids[i].clone());
            s.check("S6 host equals official after in-place delta", texts == off);
            s.check("S6 untouched blocks keep node identity", r["identity"][&id0] == true && r["identity"][&id1] == true);
            s.check("S6 the changed block was replaced", r["identity"][&id2] == false);
            s.stage = 22;
            seq(vec![st(win, "st_type", json!({"block": 1, "text": " [Z]"})), st(win, "st_mark", json!({})), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 22) => {
            // S7: dirty host ignores the advance; the merged rebase is a delta and leaves untouched blocks alone.
            s.agent_edit(2, |t| format!("{t}@"));
            s.stage = 23;
            let m = s.advance_msg(None);
            seq(vec![send_host(win, "official_advanced", m), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 23) => {
            s.check("S7 dirty host kept its text and ignored the advance", contains(&texts, 1, "[Z]") && !contains(&texts, 2, "@"));
            s.stage = 24;
            st(win, "st_flush", json!({}))
        }
        (Mode::Merge, 25) => {
            let [id0, id1, _] = [0, 1, 2].map(|i| s.original_ids[i].clone());
            s.check("S7 rebase delta merged both sides", texts == off && contains(&texts, 1, "[Z]") && contains(&texts, 2, "@"));
            s.check("S7 rebase left the typed block and the heading as the same nodes", r["identity"][&id0] == true && r["identity"][&id1] == true);
            s.check("S7 no resync was needed", s.resyncs == 1);
            s.check("no stale/invalid rejections in merge flow", s.rejected.is_empty());
            finish(s)
        }
        // ---------------- fuzz: random structural downstream deltas ----------------
        (Mode::Fuzz, 1) => {
            s.check("F0 initial load equals official", doc::parse_pm(&r["doc"]).is_ok_and(|(b, _)| b == s.blocks));
            s.fuzz_mutate();
            s.stage = 2;
            let m = s.advance_msg(None);
            seq(vec![send_host(win, "official_advanced", m), st(win, "st_report", json!({}))])
        }
        (Mode::Fuzz, 2) => {
            s.apply_ms_max = s.apply_ms_max.max(r["lastApplyMs"].as_f64().unwrap_or(0.0));
            let same = doc::parse_pm(&r["doc"]).is_ok_and(|(b, _)| b == s.blocks);
            if !same {
                s.fuzz_bad += 1;
                println!("fuzz mismatch at step {}: host {} blocks vs official {}", s.fuzz_steps, texts.len(), s.blocks.len());
                if let Ok((hb, _)) = doc::parse_pm(&r["doc"]) {
                    let official: std::collections::HashSet<&str> = s.blocks.iter().map(|b| b.id.as_str()).collect();
                    for (i, b) in hb.iter().enumerate().filter(|(_, b)| !official.contains(b.id.as_str())) {
                        println!("  extra in host: index {i} of {} kind {:?} text {:?}", hb.len(), b.kind, b.text);
                    }
                    println!("  official last block kind: {:?}", s.blocks.last().map(|b| &b.kind));
                }
            }
            s.fuzz_steps += 1;
            if s.fuzz_bad > 0 || s.fuzz_steps >= std::env::var("FUZZ_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(80) {
                s.check(&format!("fuzz: host == official after each of {} random structural steps", s.fuzz_steps), s.fuzz_bad == 0);
                s.check("fuzz: never needed a resync", s.resyncs == 0);
                println!("FUZZ {} blocks, {} steps: downstream avg {} B, max {} B | host apply max {:.1} ms", s.blocks.len(), s.fuzz_steps, s.down_bytes / s.fuzz_steps.max(1) as usize, s.down_max, s.apply_ms_max);
                return finish(s);
            }
            s.fuzz_mutate();
            let m = s.advance_msg(None);
            seq(vec![send_host(win, "official_advanced", m), st(win, "st_report", json!({}))])
        }
        // ---------------- crash recovery (second process) ----------------
        (Mode::CrashB(kind), 1) => {
            let expect_rev = if kind == Crash::Stale { s.disk_rev + 1 } else { s.disk_rev };
            s.check("B1 official state on startup = what was committed before the crash", s.rev == expect_rev);
            s.check("B1 host loaded OFFICIAL text, not the journal buffer", !contains(&texts, 1, "[未提交]"));
            let has_cand = s.recovery.as_ref().is_some_and(|r| r.ops.iter().any(|o| o.contains("[未提交]")));
            s.check("B1 recovery candidate derived from journal", has_cand);
            s.check("B1 recovery bar visible", s.recovery.is_some());
            s.check("B1 host rect pushed down by the bar (viewport h = 720-40-44 = 636)", s.viewports.first().is_some_and(|v| (v.1 - 636.0).abs() < 2.0));
            if kind == Crash::Stale {
                let c = s.recovery.as_ref().map(|r| r.conflicts.is_empty() && r.candidate.get(2).is_some_and(|b| b.text.contains("[Agent-offline]"))).unwrap_or(false);
                s.check("B1 stale journal rebased onto newer official state without conflict", c);
            }
            s.mark_commits = s.commits;
            let act = if kind == Crash::Discard {
                s.discard_recovery();
                Task::none()
            } else {
                s.accept_recovery(win)
            };
            s.stage = 2;
            act.chain(delayed_report())
        }
        (Mode::CrashB(kind), 2) => {
            let accepted = kind != Crash::Discard;
            s.check("B2 journal removed", !s.store.journal_path().exists());
            s.check("B2 recovery bar gone, host rect restored (viewport h = 680)", s.recovery.is_none() && s.viewports.last().is_some_and(|v| (v.1 - 680.0).abs() < 2.0));
            if accepted {
                s.check("B2 accepted: candidate committed to official", contains(&off, 1, "[未提交]") && s.commits == s.mark_commits + 1);
                s.check("B2 accepted: host shows the recovered text", contains(&texts, 1, "[未提交]"));
            } else {
                s.check("B2 discarded: official unchanged, no commit", !contains(&off, 1, "[未提交]") && s.commits == s.mark_commits);
                s.check("B2 discarded: host unchanged", !contains(&texts, 1, "[未提交]"));
            }
            finish(s)
        }
        _ => Task::none(),
    }
}

fn finish_basic(s: &mut State) {
    let ids: std::collections::HashSet<&str> = s.blocks.iter().map(|b| b.id.as_str()).collect();
    let uniq = ids.len() == s.blocks.len();
    let n3 = s.blocks.len() == 3;
    let h = s.blocks.first().map(|b| b.id.as_str()) == Some(s.original_ids[0].as_str());
    let p1 = s.blocks.iter().any(|b| b.id == s.original_ids[1]);
    let p2 = !s.blocks.iter().any(|b| b.id == s.original_ids[2]);
    let typed = s.blocks.iter().any(|b| b.text.contains("自测追加"));
    let (commits, rej, snaps) = (s.commits >= 3, s.rejected.is_empty(), s.snapshots >= 1);
    let (v0, v1) = (s.viewports.first().copied(), s.viewports.last().copied());
    let near = |a: Option<(f64, f64)>, b: (f64, f64)| a.is_some_and(|a| (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0);
    s.check("block ids are unique after split", uniq);
    s.check("final block count is 3 (split +1, delete -1)", n3);
    s.check("heading block id stable across edits", h);
    s.check("untouched paragraph id stable", p1);
    s.check("deleted paragraph id gone", p2);
    s.check("typed text reached the AST", typed);
    s.check("at least 3 commits", commits);
    s.check("no stale/invalid rejections from our own edits", rej);
    s.check("recovery snapshots were received", snaps);
    s.check("host viewport at start = 760x680", near(v0, (760.0, 680.0)));
    s.check("host viewport follows window resize (900x600 → 560x560)", near(v1, (560.0, 560.0)));
}

fn finish(s: &mut State) -> Task<Message> {
    if s.fails > 0 {
        println!("SELFTEST FAIL ({} failed)", s.fails);
        std::process::exit(1);
    }
    println!("SELFTEST PASS");
    iced::exit()
}

fn view(s: &State) -> Element<'_, Message> {
    let blocks = s.blocks.iter().fold(column![text("Official blocks").size(14)].spacing(2), |c, b| {
        c.push(text(format!("{:?}  {}", b.kind, b.text)).size(12))
    });
    let changes = s.changes.iter().rev().take(8).fold(column![text("Commits").size(14)].spacing(2), |c, l| c.push(text(l.clone()).size(11)));
    let log = s.log.iter().rev().take(40).fold(column![].spacing(1), |c, l| c.push(text(l.clone()).size(11)));
    let left = container(
        column![
            text("editor-host spike").size(18),
            text(format!("rev {}  commits {}  snapshots {}  history {} KB (rev {}..{})", s.rev, s.commits, s.snapshots, s.history.file_len() / 1024, s.history.oldest(), s.history.head())).size(12),
            blocks,
            changes,
            text("bridge log").size(14),
            scrollable(log).height(Length::Fill),
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Length::Fixed(LEFT_W))
    .height(Length::Fill);
    let top = container(text(format!("editor host bound to document …{}", &s.binding.document_id[s.binding.document_id.len() - 6..])).size(13))
        .padding([10, 14])
        .height(Length::Fixed(TOP_BAR_H))
        .width(Length::Fill);
    let mut right = column![top];
    if let Some(r) = &s.recovery {
        let note = if r.conflicts.is_empty() {
            format!("上次未正常退出，发现 {} 处未提交的编辑", r.ops.len())
        } else {
            format!("上次未正常退出：{} 处未提交编辑，其中 {} 块与新的正式版本冲突（恢复时保留我的）", r.ops.len(), r.conflicts.len())
        };
        right = right.push(
            container(
                row![
                    text(note).size(13),
                    Space::new().width(Length::Fill),
                    button("恢复为变更").on_press(Message::RecoveryAccept),
                    button("丢弃").on_press(Message::RecoveryDiscard),
                ]
                .spacing(8),
            )
            .padding([6, 14])
            .height(Length::Fixed(RECOVERY_BAR_H))
            .width(Length::Fill),
        );
    }
    right = right.push(Space::new().width(Length::Fill).height(Length::Fill));
    row![left, right].into()
}

fn subscription(_: &State) -> Subscription<Message> {
    Subscription::batch([
        window::resize_events().map(|(id, size)| Message::Resized(id, size)),
        Subscription::run(|| {
            iced::stream::channel(100, async |mut out| {
                let mut rx = RX.lock().unwrap().take().expect("receiver taken once");
                while let Some(raw) = rx.recv().await {
                    let _ = out.send(Message::Bridge(raw)).await;
                }
            })
        }),
    ])
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("Digger editor host spike")
        .subscription(subscription)
        .window_size(Size::new(1100.0, 720.0))
        .run()
}
