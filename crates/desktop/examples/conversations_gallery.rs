//! Shared conversations views + native review; synthetic services only.
//! --check runs the same initial state setup headlessly; never invents NativeLoaded.
#[path = "support/conversations.rs"]
mod support;
use bytehost_chrome::{BodyArea, ShellModel, ShellMsg, metrics_from_theme};
use digger_desktop::{
    adapters::{
        agent::{AgentShell, Status},
        conversations::ConversationsPanel,
        conversations_import::ImportMarker,
        review_geometry::review_bounds,
    },
    fonts,
    product::Msg,
    shell,
};
use dozer_model::{AgentKind, ProjectInfo};
use dozer_panel_conversations::Message;
use iced::widget::{Space, column, container, row};
use iced::{Element, Length, Task, window};
use std::sync::Arc;

const PID: i64 = 7;
struct Gallery {
    panel: ConversationsPanel,
    project: ProjectInfo,
    daemon: AgentShell,
    layout: ShellModel,
    window: Option<window::Id>,
    // Keep async fake service work alive without depending on the app executor.
    _runtime: tokio::runtime::Runtime,
    receiver: Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>>,
    _isolated: tempfile::TempDir,
    state_name: String,
}
impl Gallery {
    fn new(name: &str) -> Self {
        assert!(support::STATES.contains(&name));
        let isolated = tempfile::Builder::new()
            .prefix("digger-conversations-gallery-")
            .tempdir_in(std::env::var_os("TMPDIR").expect("TMPDIR required"))
            .unwrap();
        let marker = isolated.path().join("imported.json");
        if name != "import-prompt" {
            let mut value = ImportMarker::default();
            value.mark(PID, 0, 1792324800000);
            value.save(&marker).unwrap();
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut panel = ConversationsPanel::new_with_marker_path(
            Arc::new(support::FakeConversationService { state: name.into() }),
            support::unavailable(),
            support::unavailable(),
            marker,
        );
        panel.attach_handle(runtime.handle().clone());
        let project = ProjectInfo {
            id: PID,
            path: isolated
                .path()
                .join("synthetic-project")
                .display()
                .to_string(),
            name: "Synthetic fixture project".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        let defaults = shell::defaults();
        let mut layout = ShellModel::new(
            defaults.rails,
            defaults.selected,
            defaults.left_width,
            (1200., 800.),
        )
        .unwrap();
        let _ = shell::update(
            &mut layout,
            ShellMsg::PanelPress {
                panel: shell::conversations_id(),
            },
        );
        let _ = panel.activate(&project);
        let mut receiver = panel.take_receiver().unwrap();
        let receive = |receiver: &mut tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>| {
            runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
                    .await
                    .unwrap()
                    .unwrap()
            })
        };
        let (pid, message) = receive(&mut receiver);
        let _ = panel.update(pid, message, Some(&project));
        if matches!(
            name,
            "list-and-detail-loading"
                | "list-and-detail"
                | "detail-error"
                | "detail-empty-entries"
                | "long-detail"
        ) {
            let fixture = match name {
                "detail-empty-entries" => "empty",
                "long-detail" => "long",
                _ => "tools",
            };
            let _ = panel.update(
                PID,
                Message::SessionOpen(fixture.into(), AgentKind::Claude),
                Some(&project),
            );
            {
                let (pid, message) = receive(&mut receiver);
                let _ = panel.update(pid, message, Some(&project));
            }
        }
        // Receiver stays owned until boot takes it; messages are never discarded.
        Self {
            panel,
            project,
            daemon: AgentShell {
                status: if name == "daemon-unavailable" {
                    Status::Unavailable(
                        "Synthetic daemon unavailable; no connection attempted".into(),
                    )
                } else {
                    Status::Ready
                },
            },
            layout,
            _runtime: runtime,
            receiver: Some(receiver),
            _isolated: isolated,
            window: None,
            state_name: name.into(),
        }
    }
    fn area(&self) -> BodyArea {
        let side = self.layout.side_of(&shell::conversations_id()).unwrap();
        BodyArea {
            side,
            rect: self
                .layout
                .rects(&metrics_from_theme())
                .content(side)
                .unwrap(),
            pair: shell::pair_rects(&self.layout, side),
            mirrored: false,
            split: 0.35,
            collapsed: false,
        }
    }
    fn sync(&mut self) -> Task<Msg> {
        // Deterministic pre-document-load fixture: keep genuine snapshot pending.
        if self.state_name == "list-and-detail-loading" {
            return Task::none();
        }
        let Some(id) = self.window else {
            return Task::none();
        };
        let area = self.area();
        let rect = review_bounds(area.pair.unwrap().second, byteui::theme::icon_size::scale());
        self.panel
            .sync_native(
                id,
                true,
                bytehost_core::LogicalRect {
                    x: rect.x.into(),
                    y: rect.y.into(),
                    width: rect.w.into(),
                    height: rect.h.into(),
                },
                Some(PID),
            )
            .map(Msg::ReviewNative)
    }
    fn contract(&self) -> serde_json::Value {
        let state = self.panel.state(PID).unwrap();
        serde_json::json!({"config_dir": digger_paths::config_dir(), "state": self.state_name, "rows": state.sessions().map_or(0, |r| r.len()), "review": state.review.is_some(), "entries": state.review.as_ref().map_or(0, |r| r.entries.len()), "error": state.review.as_ref().and_then(|r| r.error.clone()), "loaded": state.review.as_ref().is_some_and(|r| r.loaded_nonce == Some(r.nonce)), "snapshot": state.review_snapshot().lock().unwrap().is_some(), "import_prompt": self.panel.import_prompt(PID, self.daemon.status == Status::Ready).is_some(), "iced_filter": state.agent_picker_open(), "daemon_ready": self.daemon.status == Status::Ready})
    }
}

fn boot(name: &str) -> (Gallery, Task<Msg>) {
    let mut gallery = Gallery::new(name);
    let receiver = gallery.receiver.take().unwrap();
    let messages = Task::stream(iced::futures::stream::unfold(
        receiver,
        |mut rx| async move {
            rx.recv()
                .await
                .map(|(pid, m)| (Msg::Conversations(pid, m), rx))
        },
    ));
    let native = gallery.panel.native.take_receiver().unwrap();
    let native = Task::stream(iced::futures::stream::unfold(native, |mut rx| async move {
        rx.recv().await.map(|m| (Msg::ReviewNative(m), rx))
    }));
    let ready = bytehost_desktop::panel::oldest_window().then(|id| match id {
        Some(id) => Task::done(Msg::WindowReady(id, iced::Size::new(1200., 800.))),
        None => Task::none(),
    });
    (gallery, Task::batch([messages, native, ready]))
}
fn update(gallery: &mut Gallery, message: Msg) -> Task<Msg> {
    let mut tasks = vec![];
    match message {
        Msg::WindowReady(id, size) => {
            gallery.window = Some(id);
            let _ = shell::update(
                &mut gallery.layout,
                ShellMsg::Resized {
                    w: size.width,
                    h: size.height,
                },
            );
            #[cfg(target_os = "macos")]
            tasks.push(window::run(id, |w| byteui::native_menu::install_content_view(w)).discard());
            if gallery.state_name == "agent-filter-open" {
                eprintln!(
                    "Gallery agent-filter-open: click the shared agent-filter control to open the actual native menu."
                );
            }
        }
        Msg::Conversations(pid, message) => {
            tasks.push(gallery.panel.update(pid, message, Some(&gallery.project)))
        }
        Msg::ReviewNative(event) => {
            tasks.push(gallery.panel.update_native(event, Some(&gallery.project)))
        }
        Msg::ConversationsNoticeDismiss => gallery.panel.dismiss_notice(),
        Msg::Shell(ShellMsg::DividerPress(_)) => {}
        // Keep synthetic import disabled: the real explicit import is exercised in Digger.
        Msg::ConversationsImport(..) => {
            gallery.panel.notice = Some(
                "Synthetic gallery: use the isolated Digger launcher for explicit import.".into(),
            )
        }
        _ => {}
    }
    for (pid, request) in gallery.panel.take_requests() {
        match request {
            dozer_panel_conversations::adapter::ConversationsRequest::ShowAgentFilter {
                at,
                current,
                present,
            } => {
                #[cfg(target_os = "macos")]
                if let Some(message) = byteui::native_menu::show(
                    digger_desktop::adapters::native_menu::conversation_filter_items(
                        pid, current, &present,
                    ),
                    at,
                ) {
                    tasks.push(Task::done(message));
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (at, current, present);
                    gallery.panel.open_agent_filter(pid);
                }
            }
        }
    }
    tasks.push(gallery.sync());
    Task::batch(tasks)
}
fn view(gallery: &Gallery) -> Element<'_, Msg> {
    let area = gallery.area();
    let body = gallery
        .panel
        .view(Some(&gallery.project), &area, &gallery.daemon);
    column![
        Space::new().height(Length::Fixed(area.rect.y)),
        row![
            Space::new().width(Length::Fixed(area.rect.x)),
            container(body)
                .width(Length::Fixed(area.rect.w))
                .height(Length::Fixed(area.rect.h))
        ]
    ]
    .into()
}
/// GUI launches require the same canonical data-root override as production.
/// Headless --check owns only its temp marker and can report the resolver safely.
fn check_gui_isolation() -> Result<(), String> {
    let tmp = std::path::PathBuf::from(std::env::var_os("TMPDIR").ok_or("TMPDIR required")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    for name in ["HOME", "DIGGER_DATA_DIR"] {
        let path = std::path::PathBuf::from(std::env::var_os(name).ok_or_else(|| {
            format!("{name} required; use scripts/launch-conversations-fixture.sh")
        })?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
        if path == tmp || !path.starts_with(&tmp) {
            return Err(format!(
                "{name} must be a strict descendant of actual TMPDIR; use the isolated launcher"
            ));
        }
    }
    Ok(())
}
fn main() -> iced::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "--list") {
        println!(
            "conversations_gallery --state <name> [--check]\n{}\nNo daemon/backend/model. Native agent menu: click shared filter control.",
            support::STATES.join("\n")
        );
        return Ok(());
    }
    let mut name = "empty".to_owned();
    let mut check = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state" => name = args.next().cloned().unwrap_or_default(),
            "--check" => check = true,
            a if a.starts_with("--state=") => name = a[8..].into(),
            _ => {
                eprintln!("Unknown argument {arg}");
                std::process::exit(2);
            }
        }
    }
    if !support::STATES.contains(&name.as_str()) {
        eprintln!("Unknown gallery state: {name}; use --list");
        std::process::exit(2);
    }
    byteui::theme::color::set_theme(byteui::theme::color::ColorTokens::byteboy2077());
    byteui::theme::font::set_theme(byteui::theme::font::FontTokens::byteboy2077());
    if check {
        println!("{}", Gallery::new(&name).contract());
        return Ok(());
    }
    if let Err(error) = check_gui_isolation() {
        eprintln!("{error}");
        std::process::exit(2);
    }
    let title = format!("Conversations gallery · {name}");
    iced::application(move || boot(&name), update, view)
        .title(move |_: &Gallery| title.clone())
        .font(fonts::JETBRAINS_MONO)
        .font(dozer_panel_term::symbols::SYMBOLS_FONT_DATA)
        .window_size(iced::Size::new(1200., 800.))
        .subscription(|_: &Gallery| {
            window::resize_events().map(|(id, size)| Msg::WindowReady(id, size))
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_ten_states_use_shared_state_and_never_premark_native_loaded() {
        for name in support::STATES {
            let gallery = Gallery::new(name);
            let state = gallery.contract();
            assert_eq!(
                state["loaded"], false,
                "{name}: native IPC must mark loaded"
            );
            assert_eq!(
                state["iced_filter"], false,
                "native menu must not use iced overlay"
            );
            assert_eq!(state["import_prompt"], name == "import-prompt", "{name}");
            assert_eq!(
                state["daemon_ready"],
                name != "daemon-unavailable",
                "{name}"
            );
            assert_eq!(
                state["review"],
                matches!(
                    name,
                    "list-and-detail-loading"
                        | "list-and-detail"
                        | "detail-error"
                        | "detail-empty-entries"
                        | "long-detail"
                ),
                "{name}"
            );
            match name {
                "list-and-detail" => {
                    assert!(state["entries"].as_u64().unwrap() > 0);
                    assert_eq!(state["snapshot"], true);
                }
                "long-detail" => assert_eq!(state["entries"], 200),
                "detail-error" => assert!(
                    state["error"]
                        .as_str()
                        .unwrap()
                        .contains("Synthetic detail failure")
                ),
                "detail-empty-entries" => {
                    assert_eq!(state["entries"], 0);
                    assert_eq!(state["snapshot"], true);
                }
                "list-and-detail-loading" => {
                    assert_eq!(
                        dozer_panel_conversations::snapshot::review_pane_state(
                            gallery.panel.state(PID).unwrap().review.as_ref()
                        ),
                        dozer_panel_conversations::snapshot::ReviewPaneState::Loading
                    );
                    assert!(state["entries"].as_u64().unwrap() > 0);
                    assert_eq!(state["snapshot"], true);
                }
                _ => {}
            }
            let _shared_widget = view(&gallery);
        }
    }
}
