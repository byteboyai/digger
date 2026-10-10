//! 编辑器宿主 spike(ADR-001 验证项 1/2):iced 窗口 + wry 子视图 + Tiptap 宿主。
//! 设 `SELFTEST=1` 则宿主自动执行脚本化编辑,Rust 校验后打印 PASS/FAIL 并退出。

mod bridge;
mod doc;

use bridge::{Binding, Validator};
use doc::{Block, Kind, Op};
use iced::futures::SinkExt;
use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const LEFT_W: f32 = 340.0;
const TOP_BAR_H: f32 = 40.0;
const MAX_FRAME: usize = 2 * 1024 * 1024;
const HOST_HTML: &str = include_str!("../host/dist/index.html");

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
    Timeout,
}

struct State {
    window: Option<window::Id>,
    size: Size,
    binding: Binding,
    validator: Validator,
    blocks: Vec<Block>,
    rev: u64,
    commits: u32,
    log: Vec<String>,
    changes: Vec<String>,
    selftest: bool,
    original_ids: Vec<String>,
    snapshots: u32,
    rejected: Vec<String>,
    viewports: Vec<(f64, f64)>,
}

fn state_ref(b: &Binding, rev: u64) -> String {
    format!("doc:{}@{}", b.document_id, rev)
}

impl State {
    fn new() -> Self {
        let binding = Binding {
            project_id: uuid::Uuid::now_v7().to_string(),
            panel_id: uuid::Uuid::now_v7().to_string(),
            document_id: uuid::Uuid::now_v7().to_string(),
        };
        let blocks = vec![
            Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Heading(1), text: "第一章 开端".into() },
            Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: "夜色渐深，城市的灯火一盏盏亮起。".into() },
            Block { id: uuid::Uuid::now_v7().to_string(), kind: Kind::Paragraph, text: "他推开门，雨声涌了进来。".into() },
        ];
        Self {
            window: None,
            size: Size::new(1100.0, 720.0),
            validator: Validator::new(binding.clone(), MAX_FRAME),
            binding,
            original_ids: blocks.iter().map(|b| b.id.clone()).collect(),
            blocks,
            rev: 1,
            commits: 0,
            log: vec![],
            changes: vec![],
            selftest: std::env::var("SELFTEST").is_ok(),
            snapshots: 0,
            rejected: vec![],
            viewports: vec![],
        }
    }

    fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        if self.selftest {
            println!("{line}");
        }
        self.log.push(line);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    fn bounds(&self) -> wry::Rect {
        use wry::dpi::{LogicalPosition, LogicalSize};
        wry::Rect {
            position: LogicalPosition::new(LEFT_W as f64, TOP_BAR_H as f64).into(),
            size: LogicalSize::new((self.size.width - LEFT_W).max(1.0) as f64, (self.size.height - TOP_BAR_H).max(1.0) as f64).into(),
        }
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

fn send_host(id: window::Id, ty: &str, payload: serde_json::Value) -> Task<Message> {
    let msg = serde_json::json!({"type": ty, "payload": payload}).to_string();
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

fn describe(op: &Op) -> String {
    match op {
        Op::InsertBlock { index, .. } => format!("InsertBlock @{index}"),
        Op::DeleteBlock { id } => format!("DeleteBlock {}", &id[id.len().saturating_sub(6)..]),
        Op::MoveBlock { to, .. } => format!("MoveBlock →{to}"),
        Op::ReplaceBlockContent { text, .. } => format!("ReplaceBlockContent \"{text}\""),
        Op::SetBlockAttributes { kind, .. } => format!("SetBlockAttributes {kind:?}"),
    }
}

fn update(s: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::Ready(id, size) => {
            s.window = Some(id);
            s.size = size;
            let bounds = s.bounds();
            let script = format!("window.__DIGGER_BINDING__ = {};", serde_json::to_string(&s.binding).unwrap());
            let tx = TX.get().unwrap().clone();
            let build = window::run(id, move |w| {
                let wv = wry::WebViewBuilder::new()
                    .with_bounds(bounds)
                    .with_initialization_script(script)
                    .with_html(HOST_HTML)
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
            if s.selftest {
                Task::batch([build, Task::perform(tokio::time::sleep(std::time::Duration::from_secs(20)), |_| Message::Timeout)])
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
                let b = s.bounds();
                HOST.with(|h| {
                    if let Some(wv) = h.borrow().as_ref() {
                        let _ = wv.set_bounds(b);
                    }
                });
            }
            Task::none()
        }
        Message::Timeout => {
            println!("FAIL: self-test timed out");
            std::process::exit(2);
        }
        Message::Bridge(raw) => {
            let Some(win) = s.window else { return Task::none() };
            match s.validator.accept(&raw) {
                Err(r) => {
                    s.note(format!("REJECT {r:?}"));
                    Task::none()
                }
                Ok(env) => match env.ty.as_str() {
                    "ready" => {
                        s.note(format!("← ready viewport {}", env.payload["viewport"]));
                        s.viewports.push((env.payload["viewport"]["w"].as_f64().unwrap_or(0.0), env.payload["viewport"]["h"].as_f64().unwrap_or(0.0)));
                        let load = send_host(win, "load", serde_json::json!({
                            "stateRef": state_ref(&s.binding, s.rev),
                            "doc": doc::to_pm(&s.blocks),
                        }));
                        if s.selftest {
                            load.chain(window::resize(win, Size::new(900.0, 600.0)))
                                .chain(send_host(win, "selftest", serde_json::json!({})))
                        } else {
                            load
                        }
                    }
                    "edit" => {
                        let base = env.payload["baseRev"].as_str().unwrap_or("");
                        let cur = state_ref(&s.binding, s.rev);
                        if base != cur {
                            s.note(format!("← edit REJECTED stale base {base} != {cur}"));
                            s.rejected.push("stale".into());
                            return send_host(win, "rejected", serde_json::json!({"reason": "stale"}));
                        }
                        match doc::parse_pm(&env.payload["doc"]) {
                            Err(e) => {
                                s.note(format!("← edit REJECTED invalid {e:?}"));
                                s.rejected.push(format!("{e:?}"));
                                send_host(win, "rejected", serde_json::json!({"reason": format!("{e:?}")}))
                            }
                            Ok((new_blocks, warnings)) => {
                                let ops = doc::diff(&s.blocks, &new_blocks);
                                if !ops.is_empty() {
                                    let summary = ops.iter().map(describe).collect::<Vec<_>>().join("; ");
                                    s.blocks = new_blocks;
                                    s.rev += 1;
                                    s.commits += 1;
                                    s.changes.push(format!("rev {}: {summary}", s.rev));
                                    s.note(format!("← edit → Draft Change → Commit rev {} [{summary}] {warnings:?}", s.rev));
                                } else {
                                    s.note("← edit (no semantic change)");
                                }
                                send_host(win, "committed", serde_json::json!({"stateRef": state_ref(&s.binding, s.rev)}))
                            }
                        }
                    }
                    "snapshot" => {
                        s.snapshots += 1;
                        let path = std::env::temp_dir().join("digger-spike-journal.json");
                        let _ = std::fs::write(&path, env.payload.to_string());
                        s.note(format!("← snapshot #{} → journal", s.snapshots));
                        Task::none()
                    }
                    "selftest_done" => {
                        finish_selftest(s);
                        iced::exit()
                    }
                    "title" => {
                        s.note(format!("← resize viewport {}", env.payload["viewport"]));
                        s.viewports.push((env.payload["viewport"]["w"].as_f64().unwrap_or(0.0), env.payload["viewport"]["h"].as_f64().unwrap_or(0.0)));
                        Task::none()
                    }
                    other => {
                        s.note(format!("← {other} {}", if other == "selection" { String::new() } else { env.payload.to_string() }));
                        Task::none()
                    }
                },
            }
        }
    }
}

fn finish_selftest(s: &State) {
    let mut ok = true;
    let mut check = |name: &str, cond: bool| {
        println!("{}: {name}", if cond { "PASS" } else { "FAIL" });
        ok &= cond;
    };
    let ids: std::collections::HashSet<&str> = s.blocks.iter().map(|b| b.id.as_str()).collect();
    check("block ids are unique after split", ids.len() == s.blocks.len());
    check("final block count is 3 (split +1, delete -1)", s.blocks.len() == 3);
    check("heading block id stable across edits", s.blocks.first().map(|b| b.id.as_str()) == Some(s.original_ids[0].as_str()));
    check("untouched paragraph id stable", s.blocks.iter().any(|b| b.id == s.original_ids[1]));
    check("deleted paragraph id gone", !s.blocks.iter().any(|b| b.id == s.original_ids[2]));
    check("typed text reached the AST", s.blocks.iter().any(|b| b.text.contains("自测追加")));
    check("at least 3 commits", s.commits >= 3);
    check("no stale/invalid rejections from our own edits", s.rejected.is_empty());
    check("recovery snapshots were journaled", s.snapshots >= 1);
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0;
    check("host viewport at start = window minus left panel and top bar (760x680)", s.viewports.first().is_some_and(|v| near(*v, (760.0, 680.0))));
    check("host viewport follows window resize (900x600 → 560x560)", s.viewports.last().is_some_and(|v| near(*v, (560.0, 560.0))));
    println!("viewports: {:?}", s.viewports);
    println!("final blocks: {:?}", s.blocks.iter().map(|b| (&b.kind, &b.text)).collect::<Vec<_>>());
    println!("{}", if ok { "SELFTEST PASS" } else { "SELFTEST FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

fn view(s: &State) -> Element<'_, Message> {
    let blocks = s.blocks.iter().fold(column![text("Official blocks").size(14)].spacing(2), |c, b| {
        c.push(text(format!("{:?}  {}", b.kind, b.text)).size(12))
    });
    let changes = s.changes.iter().rev().take(8).fold(column![text("Draft Changes → Commits").size(14)].spacing(2), |c, l| {
        c.push(text(l.clone()).size(11))
    });
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
    row![left, column![top, Space::new().width(Length::Fill).height(Length::Fill)]].into()
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
