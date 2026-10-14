//! 编辑器引擎对比 spike 的运行器:iced 窗口 + 一个 wry 子视图加载所选引擎的页面。
//!   ENGINE=cm|pm  选择引擎(默认 cm)
//!   MODE=test     自动执行页面里的测试,打印结果并退出(默认手动:打开一份示例章节供人工试用)

mod persist;
mod sync;

use iced::futures::SinkExt;
use iced::widget::{column, container, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

const TOP_H: f32 = 56.0;
const HTML_CM: &str = include_str!("../host/dist-cm/cm.html");
const HTML_PM: &str = include_str!("../host/dist-pm/pm.html");

thread_local! { static HOST: RefCell<Option<wry::WebView>> = const { RefCell::new(None) }; }
static TX: OnceLock<UnboundedSender<String>> = OnceLock::new();
static RX: Mutex<Option<UnboundedReceiver<String>>> = Mutex::new(None);

#[derive(Debug, Clone)]
enum Message {
    Ready(window::Id, Size),
    Built(Result<(), String>),
    Resized(Size),
    Page(String),
    Timeout,
}

struct State {
    win: Option<window::Id>,
    doc: sync::Doc,
    engine: &'static str,
    test: bool,
    size: Size,
    fails: u32,
    passes: u32,
    status: String,
}

/// Rust → page: `window.__md.receive(<json>)`.
fn send_page(win: window::Id, msg: Value) -> Task<Message> {
    let js = format!("window.__md && window.__md.receive({})", serde_json::to_string(&msg.to_string()).unwrap());
    window::run(win, move |_| HOST.with(|h| { if let Some(wv) = h.borrow().as_ref() { let _ = wv.evaluate_script(&js); } })).discard()
}

fn bounds(size: Size) -> wry::Rect {
    use wry::dpi::{LogicalPosition, LogicalSize};
    wry::Rect {
        position: LogicalPosition::new(0.0, TOP_H as f64).into(),
        size: LogicalSize::new(size.width as f64, (size.height - TOP_H).max(1.0) as f64).into(),
    }
}

fn boot() -> (State, Task<Message>) {
    let (tx, rx) = unbounded_channel();
    let _ = TX.set(tx);
    *RX.lock().unwrap() = Some(rx);
    let engine = if std::env::var("ENGINE").as_deref() == Ok("pm") { "pm" } else { "cm" };
    let test = std::env::var("MODE").as_deref() == Ok("test");
    let t = window::oldest().then(|id| match id {
        Some(id) => window::size(id).map(move |s| Message::Ready(id, s)),
        None => Task::none(),
    });
    (State { win: None, doc: sync::Doc::new(), engine, test, size: Size::new(1100.0, 760.0), fails: 0, passes: 0, status: String::new() }, t)
}

fn update(s: &mut State, msg: Message) -> Task<Message> {
    match msg {
        Message::Ready(id, size) => {
            s.size = size;
            s.win = Some(id);
            let (b, tx, html, test) = (bounds(size), TX.get().unwrap().clone(), if s.engine == "pm" { HTML_PM } else { HTML_CM }, s.test);
            let build = window::run(id, move |w| {
                let wv = wry::WebViewBuilder::new()
                    .with_bounds(b)
                    .with_devtools(false)
                    .with_initialization_script(format!("window.__MODE__ = {:?};", if test { "test" } else { "manual" }))
                    .with_html(html)
                    .with_navigation_handler(|u| u == "about:blank")
                    .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                    .with_download_started_handler(|_, _| false)
                    .with_ipc_handler(move |req| { let _ = tx.send(req.into_body()); })
                    .build_as_child(&w)
                    .map_err(|e| e.to_string())?;
                HOST.with(|h| *h.borrow_mut() = Some(wv));
                Ok(())
            })
            .map(Message::Built);
            if test {
                Task::batch([build, Task::perform(tokio::time::sleep(std::time::Duration::from_secs(120)), |_| Message::Timeout)])
            } else {
                build
            }
        }
        Message::Built(r) => {
            if let Err(e) = r { println!("FAIL: could not build the webview: {e}"); std::process::exit(2); }
            Task::none()
        }
        Message::Resized(size) => {
            s.size = size;
            HOST.with(|h| { if let Some(wv) = h.borrow().as_ref() { let _ = wv.set_bounds(bounds(size)); } });
            Task::none()
        }
        Message::Timeout => { println!("FAIL: timed out"); std::process::exit(2); }
        Message::Page(raw) => {
            let Ok(v) = serde_json::from_str::<Value>(&raw) else {
                // e.g. a lone surrogate in a patch: say so, so the editor does not wait forever
                return s.win.map(|w| send_page(w, json!({"t": "reject", "reason": "unparsable message", "version": s.doc.version}))).unwrap_or_else(Task::none);
            };
            match v["t"].as_str() {
                Some("result") => {
                    let ok = v["pass"].as_bool().unwrap_or(false);
                    println!("{}: {}{}", if ok { "PASS" } else { "FAIL" }, v["name"].as_str().unwrap_or("?"), match v["detail"].as_str() { Some(d) if !d.is_empty() => format!("\n      {d}"), _ => String::new() });
                    if ok { s.passes += 1 } else { s.fails += 1 }
                }
                Some("load") => {
                    s.doc.load(v["text"].as_str().unwrap_or(""));
                    return s.win.map(|w| send_page(w, s.doc.blocks_message())).unwrap_or_else(Task::none);
                }
                Some("edit") => {
                    let changes: Option<Vec<(usize, usize, String)>> = v["changes"].as_array().and_then(|a| {
                        a.iter().map(|c| Some((c["from"].as_u64()? as usize, c["to"].as_u64()? as usize, c["insert"].as_str()?.to_string()))).collect()
                    });
                    let reply = match changes {
                        None => json!({"t": "reject", "reason": "malformed edit"}),
                        Some(ch) => match s.doc.edit(v["base"].as_u64().unwrap_or(0), &ch) {
                            Ok(()) => s.doc.blocks_message(),
                            Err(e) => json!({"t": "reject", "reason": format!("{e:?}"), "version": s.doc.version}),
                        },
                    };
                    return s.win.map(|w| send_page(w, reply)).unwrap_or_else(Task::none);
                }
                Some("agent") => {
                    // a test hook standing in for the Agent runtime: an operation addressed by block id
                    let text = v["text"].as_str().unwrap_or("").to_string();
                    let id = v["id"].as_str().unwrap_or("").to_string();
                    let op = match v["op"].as_str() {
                        Some("replace_block") => Some(sync::AgentOp::ReplaceBlock { id, text }),
                        Some("insert_after") => Some(sync::AgentOp::InsertAfter { id, text }),
                        Some("append") => Some(sync::AgentOp::Append { text }),
                        _ => None,
                    };
                    // optional precondition: hash of the block text the Agent read (see Doc::agent_step)
                    let expect = v["expect"].as_str().map(String::from);
                    let reply = match op.map(|op| s.doc.agent_plan(&[sync::Step { op, expect }])) {
                        Some(Ok(mut patches)) => s.doc.update_message(&patches.remove(0), "agent"),
                        Some(Err((_, e))) => json!({"t": "agent_error", "reason": e.to_string(), "stale": matches!(e, sync::AgentError::Stale { .. })}),
                        None => json!({"t": "agent_error", "reason": "unknown operation"}),
                    };
                    return s.win.map(|w| send_page(w, reply)).unwrap_or_else(Task::none);
                }
                Some("info") => println!("INFO: {}\n      {}", v["name"].as_str().unwrap_or("?"), v["detail"].as_str().unwrap_or("")),
                Some("ready") => s.status = format!("engine: {} — 用中文/日文输入法在示例章节里打字;右侧面板可让 Agent 在你组合输入时改写。看:候选框是否保持、已组合的字是否被打断或重复、光标是否跳走", v["engine"].as_str().unwrap_or("?")),
                Some("done") => {
                    println!("SUMMARY {}: {} passed, {} failed", s.engine, s.passes, s.fails);
                    return iced::exit();
                }
                _ => {}
            }
            Task::none()
        }
    }
}

fn view(s: &State) -> Element<'_, Message> {
    let title = format!("Markdown engine spike — {}", if s.engine == "pm" { "ProseMirror (source-preserving)" } else { "CodeMirror 6 (live preview)" });
    container(column![text(title).size(16), text(if s.test { "running automated checks…".to_string() } else { s.status.clone() }).size(12)].spacing(4))
        .padding([8, 14]).height(Length::Fixed(TOP_H)).width(Length::Fill).into()
}

fn subscription(_: &State) -> Subscription<Message> {
    Subscription::batch([
        window::resize_events().map(|(_, size)| Message::Resized(size)),
        Subscription::run(|| {
            iced::stream::channel(100, async |mut out| {
                let mut rx = RX.lock().unwrap().take().expect("receiver taken once");
                while let Some(raw) = rx.recv().await { let _ = out.send(Message::Page(raw)).await; }
            })
        }),
    ])
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("Markdown engine spike")
        .subscription(subscription)
        .window_size(Size::new(1100.0, 760.0))
        .run()
}
