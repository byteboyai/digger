//! 编辑器宿主 spike(ADR-001 验证项 1–4):iced 窗口 + wry 子视图 + Tiptap 宿主。
//!
//! `SELFTEST=<mode>` 自动执行并打印 PASS/FAIL:
//!   basic | merge | perf | crash-a | crash-b-accept | crash-b-discard | crash-b-stale
//! crash-b-* 需要 `SPIKE_DIR` 指向 crash-a 留下的数据目录,见 run_recovery_test.sh。

mod bridge;
mod doc;
mod merge;
mod store;

use bridge::{Binding, Validator};
use doc::{Block, Kind, Op};
use iced::futures::SinkExt;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use merge::{Resolution, merge};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use store::{Journal, Persisted, Store};
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
}

struct State {
    window: Option<window::Id>,
    size: Size,
    store: Store,
    binding: Binding,
    validator: Validator,
    blocks: Vec<Block>,
    rev: u64,
    history: BTreeMap<u64, Vec<Block>>,
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
        let store = Store::new(dir);
        let persisted = store.load().unwrap_or_else(|| {
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
            if mode == Mode::Perf {
                let n: usize = std::env::var("BLOCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
                let line = "夜色渐深，城市的灯火一盏盏亮起，他推开门，雨声涌了进来，所有的往事像潮水一样漫过脚踝。";
                for i in 0..n {
                    blocks.push(Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: format!("{i}. {line}") });
                }
            }
            let p = Persisted { binding, rev: 1, history: BTreeMap::from([(1, blocks.clone())]), blocks };
            store.save(&p);
            p
        });
        let mut s = Self {
            window: None,
            size: Size::new(1100.0, 720.0),
            validator: Validator::new(persisted.binding.clone(), MAX_FRAME),
            binding: persisted.binding,
            original_ids: persisted.blocks.iter().map(|b| b.id.clone()).collect(),
            blocks: persisted.blocks,
            rev: persisted.rev,
            disk_rev: persisted.rev,
            history: persisted.history,
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

    fn persist(&self) {
        self.store.save(&Persisted {
            binding: self.binding.clone(),
            rev: self.rev,
            blocks: self.blocks.clone(),
            history: self.history.clone(),
        });
    }

    /// 提交一个新的正式版本(spike 里 Draft Change → Commit 合为一步)。
    fn commit(&mut self, blocks: Vec<Block>, label: &str) -> String {
        let summary = doc::diff(&self.blocks, &blocks).iter().map(describe).collect::<Vec<_>>().join("; ");
        self.blocks = blocks;
        self.rev += 1;
        self.commits += 1;
        self.history.insert(self.rev, self.blocks.clone());
        self.persist();
        self.changes.push(format!("rev {} [{label}]: {summary}", self.rev));
        // 日志里的缓冲区若已等于正式状态,就不再需要。
        if let Some(j) = self.store.load_journal() {
            if doc::parse_pm(&j.doc).is_ok_and(|(b, _)| b == self.blocks) {
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

    fn doc_msg(&self) -> Value {
        json!({"stateRef": state_ref(&self.binding, self.rev), "rev": self.rev, "doc": doc::to_pm(&self.blocks)})
    }

    fn detect_recovery(&mut self) -> Option<Recovery> {
        let j = self.store.load_journal()?;
        if j.document_id != self.binding.document_id {
            return None;
        }
        let (local, _) = doc::parse_pm(&j.doc).ok()?;
        let (candidate, conflicts) = if j.base_rev == self.rev {
            (local, vec![])
        } else {
            let base = self.history.get(&j.base_rev)?;
            let m = merge(base, &local, &self.blocks, None);
            if m.conflicts.is_empty() {
                (m.blocks, vec![])
            } else {
                (merge(base, &local, &self.blocks, Some(Resolution::Mine)).blocks, m.conflicts)
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
        let mut m = self.doc_msg();
        m["reason"] = json!("recovery");
        send_host(win, "rebase", m)
    }

    fn discard_recovery(&mut self) {
        self.recovery = None;
        self.store.clear_journal();
        self.note("recovery discarded");
        self.apply_bounds();
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
            let build = window::run(id, move |w| {
                let wv = wry::WebViewBuilder::new()
                    .with_bounds(bounds)
                    .with_initialization_script(script)
                    .with_html(html)
                    .with_devtools(false)
                    .with_ipc_handler(move |req| {
                        let _ = tx.send(req.into_body());
                    })
                    .build_as_child(&w)
                    .map_err(|e| e.to_string())?;
                HOST.with(|h| *h.borrow_mut() = Some(wv));
                Ok(())
            })
            .map(Message::HostBuilt);
            if s.selftest() {
                Task::batch([build, Task::perform(tokio::time::sleep(std::time::Duration::from_secs(40)), |_| Message::Timeout)])
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
            load["editDebounceMs"] = json!(s.debounce_ms());
            let load = send_host(win, "load", load);
            match s.mode {
                Mode::Interactive => load,
                Mode::Basic => seq(vec![load, window::resize(win, Size::new(900.0, 600.0)), st(win, "selftest", json!({}))]),
                Mode::InputRule => seq(vec![load, st(win, "st_inputrule", json!({}))]),
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
        "edit" => on_edit(s, win, p),
        "resolve" => {
            let Some(pc) = s.pending_conflict.take() else {
                s.note("← resolve without a pending conflict (dropped)");
                return Task::none();
            };
            let res = if p["choice"] == "mine" { Resolution::Mine } else { Resolution::Official };
            let base = s.history[&pc.base_rev].clone();
            let m = merge(&base, &pc.local, &s.blocks, Some(res));
            if m.blocks != s.blocks {
                let summary = s.commit(m.blocks, "resolved");
                s.note(format!("← resolve({:?}) → Commit rev {} [{summary}]", res, s.rev));
            }
            send_rebase(s, win, "resolved")
        }
        "snapshot" => {
            s.snapshots += 1;
            let base_rev = parse_ref(p["baseRev"].as_str().unwrap_or("")).unwrap_or(0);
            let equal = doc::parse_pm(&p["doc"]).is_ok_and(|(b, _)| b == s.blocks);
            if equal {
                s.store.clear_journal();
            } else {
                s.store.save_journal(&Journal { document_id: s.binding.document_id.clone(), base_rev, doc: p["doc"].clone() });
            }
            s.note(format!("← snapshot #{} {}", s.snapshots, if equal { "(= official, journal cleared)" } else { "→ journal" }));
            if s.mode == Mode::CrashA && !equal {
                let disk_ok = s.store.load().is_some_and(|d| d.rev == s.rev && d.blocks == s.blocks);
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

fn send_rebase(s: &mut State, win: window::Id, reason: &str) -> Task<Message> {
    let mut m = s.doc_msg();
    m["reason"] = json!(reason);
    let t = send_host(win, "rebase", m);
    Task::batch([t]).chain(hook(s, win, "rebase"))
}

fn on_edit(s: &mut State, win: window::Id, p: &Value) -> Task<Message> {
    let base = p["baseRev"].as_str().unwrap_or("");
    let Some(base_rev) = parse_ref(base) else {
        s.rejected.push("bad base".into());
        return send_host(win, "rejected", json!({"reason": "bad base"}));
    };
    let (local, warnings) = match doc::parse_pm(&p["doc"]) {
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
            s.note(format!("← edit → Draft Change → Commit rev {} [{summary}] {warnings:?}", s.rev));
        } else {
            s.note("← edit (no semantic change)");
        }
        let m = json!({"stateRef": state_ref(&s.binding, s.rev), "rev": s.rev});
        return send_host(win, "committed", m);
    }
    // 基线过期:正式状态在宿主有未发送编辑期间前进了 → 块级三方合并,绝不覆盖宿主的会话。
    let Some(base_blocks) = s.history.get(&base_rev).cloned() else {
        s.rejected.push("unknown base".into());
        return send_host(win, "rejected", json!({"reason": "unknown base"}));
    };
    let m = merge(&base_blocks, &local, &s.blocks, None);
    if m.conflicts.is_empty() {
        s.note(format!("← edit on stale base {base_rev} (official is {}): auto-merged", s.rev));
        if m.blocks != s.blocks {
            let summary = s.commit(m.blocks, "merged");
            s.note(format!("   → Commit rev {} [{summary}]", s.rev));
        }
        s.pending_conflict = None;
        send_rebase(s, win, "merged")
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
        s.pending_conflict = Some(PendingConflict { base_rev, local });
        let mut msg = s.doc_msg();
        msg["conflicts"] = json!(items);
        send_host(win, "conflict", msg).chain(hook(s, win, "conflict"))
    }
}

/// 自测脚本的"Rust 已发出 rebase/conflict"挂钩:推进到下一阶段并请求宿主报告状态。
fn hook(s: &mut State, win: window::Id, event: &str) -> Task<Message> {
    let report = st(win, "st_report", json!({}));
    match (s.mode, s.stage, event) {
        (Mode::Merge, 3, "rebase") | (Mode::Merge, 9, "rebase") | (Mode::Merge, 15, "rebase") => {
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
    // Rust 侧代价:把整篇候选作为一条 edit 帧收下并解析、diff(单块有改动)。
    let mut local = s.blocks.clone();
    let mid = local.len() / 2;
    local[mid].text.push('字');
    let frame = json!({"hostKind":"editor","binding":s.binding,"seq":1,"type":"edit",
        "payload":{"baseRev": state_ref(&s.binding, s.rev), "doc": doc::to_pm(&local)}})
    .to_string();
    let mut v = Validator::new(s.binding.clone(), MAX_FRAME);
    let t = std::time::Instant::now();
    let env = v.accept(&frame);
    let accept_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let parsed = env.as_ref().ok().and_then(|e| doc::parse_pm(&e.payload["doc"]).ok());
    let parse_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let ops = parsed.as_ref().map(|(b, _)| doc::diff(&s.blocks, b).len()).unwrap_or(0);
    let diff_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "PERF {} | blocks {} | load {:.1} ms | getJSON {:.1} ms ({} KB) | tx mean {:.2} ms p95 {:.2} ms | tx→paint {:.1} ms | Rust: frame {} KB accept {:.1} ms parse {:.1} ms diff {:.2} ms ({} op)",
        p["adapter"].as_str().unwrap_or("?"), p["blocks"], p["loadMs"].as_f64().unwrap_or(0.0), p["getJsonMs"].as_f64().unwrap_or(0.0),
        p["jsonBytes"].as_u64().unwrap_or(0) / 1024, p["txMeanMs"].as_f64().unwrap_or(0.0), p["txP95Ms"].as_f64().unwrap_or(0.0),
        p["txToPaintMs"].as_f64().unwrap_or(0.0), frame.len() / 1024, accept_ms, parse_ms, diff_ms, ops
    );
    s.check("perf: edit frame fits the bridge size limit", env.is_ok());
    s.check("perf: single-block change yields exactly one semantic op", ops == 1);
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
            let adv = send_host(win, "official_advanced", s.doc_msg());
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
            seq(vec![send_host(win, "official_advanced", s.doc_msg()), st(win, "st_report", json!({}))])
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
            seq(vec![send_host(win, "official_advanced", s.doc_msg()), st(win, "st_report", json!({}))])
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
            seq(vec![send_host(win, "official_advanced", s.doc_msg()), st(win, "st_report", json!({}))])
        }
        (Mode::Merge, 17) => {
            s.check("S4 clean host applies official_advanced", texts == off);
            s.stage = 18;
            delayed_report()
        }
        (Mode::Merge, 18) => {
            s.check("S4 applying official state produced no spurious edit/commit", s.commits == s.mark_commits);
            s.check("no stale/invalid rejections in merge flow", s.rejected.is_empty());
            finish(s)
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
            text(format!("rev {}  commits {}  snapshots {}", s.rev, s.commits, s.snapshots)).size(12),
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
