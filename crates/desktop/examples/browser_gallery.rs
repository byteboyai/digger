//! 浏览器面板的视图画廊:页签栏、地址栏、收藏夹侧栏、提示条、内联错误;合成的收藏夹服务,没有真实 webview
//! (网页区是空白占位——原生网页层只能在真实窗口里看,见 ADR-016 与验收清单 I 区)。
//!
//! `--list` 列出状态;`--state <名>` 选状态;`--check` 不开窗口,只打印该状态的契约(页签数、侧栏是否展开、是否有提示条)。
//! 内联错误(`inline-error`)是库的私有字段,契约里读不到,只能在窗口里看。
use bytehost_chrome::{BodyArea, Rect, Side};
use digger_desktop::{adapters::browser::BrowserPanel, product::Msg};
use dozer_model::{BookmarkInfo, BookmarkScope, ProjectInfo};
use dozer_panel_browser::Message;
use dozer_services::{BookmarkService, BoxFuture, ServiceError};
use iced::{Element, Length, Task};
use std::sync::Arc;

const PID: i64 = 7;
const STATES: &[&str] = &[
    "blank",
    "two-tabs",
    "bookmarks-open",
    "notice",
    "inline-error",
];

struct SampleBookmarks;
impl BookmarkService for SampleBookmarks {
    fn list_bookmarks(
        &self,
        _: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<BookmarkInfo>, ServiceError>> {
        Box::pin(async { Ok(sample_bookmarks()) })
    }
    fn add_bookmark(
        &self,
        _: BookmarkScope,
        _: Option<i64>,
        _: String,
        _: String,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
    fn remove_bookmark(&self, _: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Ok(()) })
    }
}

fn sample_bookmarks() -> Vec<BookmarkInfo> {
    let mark = |id, scope, project_id, title: &str, url: &str| BookmarkInfo {
        id,
        scope,
        project_id,
        title: title.into(),
        url: url.into(),
        created_ms: 1,
    };
    vec![
        mark(
            1,
            BookmarkScope::Global,
            None,
            "Rust 文档",
            "https://doc.rust-lang.org/",
        ),
        mark(
            2,
            BookmarkScope::Project,
            Some(PID),
            "项目看板",
            "https://example.com/board",
        ),
    ]
}

struct Gallery {
    panel: BrowserPanel,
    project: ProjectInfo,
    state_name: String,
    runtime: tokio::runtime::Runtime,
}

impl Gallery {
    fn new(name: &str) -> Self {
        assert!(STATES.contains(&name));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut panel = BrowserPanel::new(Arc::new(SampleBookmarks));
        panel.attach_handle(runtime.handle().clone());
        let project = ProjectInfo {
            id: PID,
            path: "/tmp/synthetic-project".into(),
            name: "Synthetic fixture project".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        let _ = panel.activate(&project);
        let mut gallery = Self {
            panel,
            project,
            state_name: name.into(),
            runtime,
        };
        gallery.apply(name);
        gallery
    }

    fn send(&mut self, message: Message) {
        let project = self.project.clone();
        let _ = self.panel.update(PID, message, Some(&project));
    }

    fn apply(&mut self, name: &str) {
        match name {
            "blank" => {}
            "two-tabs" => self.send(Message::OpenUrl("https://example.com/docs".into())),
            "bookmarks-open" => {
                self.send(Message::OpenUrl("https://doc.rust-lang.org/".into()));
                self.send(Message::BookmarksToggle);
            }
            "notice" => self
                .panel
                .notice
                .push_quiet("页面没有响应,无法打开:https://slow.example.com/"),
            "inline-error" => {
                // 协议相对地址不能转成本地文件:库给出内联错误。
                self.send(Message::AddrInput("//evil.example.com/x".into()));
                self.send(Message::AddrSubmit);
            }
            _ => unreachable!(),
        }
    }

    fn contract(&self) -> String {
        let state = self.panel.state(PID).expect("state");
        format!(
            "state={} tabs={} bookmarks_open={} notice={} address_draft={:?}",
            self.state_name,
            state.tab_count(),
            state.bookmarks_open(),
            self.panel.notice.text().is_some(),
            state.addr_text(),
        )
    }
}

fn area() -> BodyArea {
    BodyArea {
        side: Side::Right,
        rect: Rect {
            x: 0.,
            y: 0.,
            w: 520.,
            h: 640.,
        },
        pair: None,
        mirrored: false,
        split: 0.,
        collapsed: false,
    }
}

fn view(gallery: &Gallery) -> Element<'_, Msg> {
    iced::widget::container(gallery.panel.view(Some(&gallery.project), &area()))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn update(gallery: &mut Gallery, message: Msg) -> Task<Msg> {
    let project = gallery.project.clone();
    match message {
        Msg::Browser(pid, message) => gallery.panel.update(pid, message, Some(&project)),
        Msg::BrowserNoticeDismiss => {
            gallery.panel.dismiss_notice();
            Task::none()
        }
        _ => Task::none(),
    }
}

fn main() -> iced::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "--list") {
        println!(
            "browser_gallery --state <name> [--check]\n{}\nSynthetic bookmarks; no real webview (see ADR-016).",
            STATES.join("\n")
        );
        return Ok(());
    }
    let mut name = "two-tabs".to_owned();
    let mut check = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--state" => name = it.next().cloned().unwrap_or_default(),
            "--check" => check = true,
            a if a.starts_with("--state=") => name = a[8..].into(),
            _ => {
                eprintln!("Unknown argument {arg}");
                std::process::exit(2);
            }
        }
    }
    if !STATES.contains(&name.as_str()) {
        eprintln!("Unknown gallery state: {name}; use --list");
        std::process::exit(2);
    }
    byteui::theme::color::set_theme(byteui::theme::color::ColorTokens::byteboy2077());
    byteui::theme::font::set_theme(byteui::theme::font::FontTokens::byteboy2077());
    let gallery = Gallery::new(&name);
    if check {
        println!("{}", gallery.contract());
        return Ok(());
    }
    let _keep_runtime_alive = &gallery.runtime;
    iced::application(move || (Gallery::new(&name), Task::none()), update, view)
        .window_size((520.0, 640.0))
        .run()
}
