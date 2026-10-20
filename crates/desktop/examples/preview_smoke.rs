//! 预览宿主示例(S5b-2 P3 Task 8):`cargo run -p digger-desktop --example preview_smoke`
//!
//! 不接产品壳——只把 P3 的宿主适配器拉到真实窗口里:左边文件列表,右边预览,
//! 顶部三个按钮(关闭当前页签 / 放大(藏起 webview)/ 全部销毁),底部状态行
//! 显示存活句柄数。
//!
//! `--selftest`:无交互地逐项打开夹具,等 host 就绪,断言存活句柄数与期望一致;
//! 关掉全部页签后回到 0、重开仍是同一批;接着跑生命周期压力(开/关 50 次、
//! 切项目 20 次、放大/还原 20 次、一次开 50 个页签)并逐步核对句柄数。任何
//! 一步不符即非零退出。用于在无人盯屏时回归。
//!
//! 面板区域 = 整块 webview 矩形(顶栏高度留给产品壳,这里不画)。

#[path = "../src/adapters/preview_native/mod.rs"]
mod preview_native;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytehost_core::LogicalRect;
use dozer_panel_preview::host_ipc::IpcMessage;
use dozer_panel_preview::{PreviewPane, PreviewPanelState, TabKind, ViewerPanel};
use iced::{Element, Task, window};

const TOP_BAR: f64 = 48.0;
const STATUS_BAR: f64 = 26.0;
const WINDOW: iced::Size = iced::Size::new(1100.0, 760.0);

/// 自检用的夹具(随仓库提交,示例不再依赖 `../dozer` 源码树)。
/// `.md`/`.html`/`.png`/`.svg`/`.pdf` 是真正会建 webview 的类型(N5 起
/// `.pdf` 进 flyfish);`.py`/`.txt` 走原生编辑器,`.json`/`.csv` 在 Digger
/// 的能力下走原生降级页,都不产生句柄。
fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fixtures/preview")
}

fn fixtures() -> Vec<(PathBuf, &'static str)> {
    let dir = fixture_dir();
    [
        ("hello.py", "py"),
        ("notes.md", "md"),
        ("page.html", "html"),
        ("pic.png", "png"),
        ("logo.svg", "svg"),
        ("data.json", "json"),
        ("sheet.csv", "csv"),
        ("doc.pdf", "pdf"),
        ("带 空格 #1.txt", "txt"),
    ]
    .iter()
    .map(|(name, kind)| (dir.join(name), *kind))
    .collect()
}

/// 示例的**开发态** `assets/` 目录。与产品同一份规则(库的源码树 / 打包态
/// `Contents/Resources`),只是示例先按 `Cargo.lock` 钉住的 git 检出解析,
/// 免得 `CARGO_MANIFEST_DIR` 指向 Digger 时退回到错误的路径。
fn assets_dir() -> PathBuf {
    if let Some(root) = std::env::var_os("DIGGER_PREVIEW_ASSETS") {
        return PathBuf::from(root);
    }
    match dozer_panel_preview::assets::bundle_resources_root(
        &std::env::current_exe().unwrap_or_default(),
    ) {
        Some(resources) => resources,
        None => git_assets_dir(),
    }
}

fn asset_roots() -> PathBuf {
    assets_dir().join("flyfish")
}

/// `dozer-panel-preview` 的 `assets/` 目录:从本仓 `Cargo.lock` 的 `source`
/// 取 git 检查点 sha,再到 `~/.cargo/git/checkouts/<仓库>-<hash>/` 下找同名检出。
fn git_assets_dir() -> PathBuf {
    git_assets_root().unwrap_or_else(|| {
        dozer_panel_preview::assets::from_lib_source()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    })
}

fn git_assets_root() -> Option<PathBuf> {
    let lock =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"))
            .ok()?;
    let sha = lock
        .split("name = \"dozer-panel-preview\"")
        .nth(1)?
        .lines()
        .find_map(|line| line.strip_prefix("source = "))?
        .rsplit_once('#')?
        .1
        .trim_end_matches('"')
        .to_string();
    let short = &sha[..7.min(sha.len())];
    let mut checkouts: Vec<_> = std::fs::read_dir(cargo_home()?.join("git/checkouts"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| {
            dir.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("dozer-"))
        })
        .collect();
    checkouts.sort();
    checkouts.into_iter().find_map(|dir| {
        let root = dir.join(short).join("crates/dozer-panel-preview/assets");
        root.is_dir().then_some(root)
    })
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

/// 名字里带空格、`#`、中日文的夹具:走 `file URL → dozer:// → 白名单` 全程,
/// 钉住 Task 8 Step 3 的那一条(临时文件,进程退出后由调用方清理)。
fn awkward_fixture() -> Option<(PathBuf, &'static str)> {
    let path = std::env::temp_dir().join("digger preview 空格 #1 中文.md");
    std::fs::write(&path, "# 标题\n\n带空格、井号与中文。\n").ok()?;
    Some((path, "md"))
}

#[derive(Debug, Clone)]
enum Msg {
    Ready(window::Id),
    Resized(iced::Size),
    ListPicked(PathBuf),
    CloseCurrent,
    Maximize,
    DestroyAll,
    Ipc(preview_native::Event),
    /// 自检心跳:状态机只在收到消息时推进,而"句柄已建但还没发 Ready"
    /// 这类中间态不会有事件,靠它把检查再跑一遍。
    Tick,
}

struct App {
    window: Option<window::Id>,
    size: (f64, f64),
    host: Arc<preview_native::Host>,
    state: PreviewPanelState,
    list: Vec<(PathBuf, &'static str)>,
    selected: Option<PathBuf>,
    /// 右侧是否让位给"放大"——这是一个纯 UI 开关(真产品里由配对布局决定),
    /// 宿主侧只负责把对应 webview 藏起。
    maximized: bool,
}

fn build() -> (App, Task<Msg>) {
    let mut list = fixtures();
    if let Some(awkward) = awkward_fixture() {
        list.push(awkward);
    }
    let mut pane = PreviewPane::default();
    for (path, _) in &list {
        pane.open_path_provisional(path.clone());
    }
    let host = preview_native::Host::new(asset_roots());
    // 预览里的编辑器要能读回原文件(保存/冲突检测走同一白名单)。
    for tab in pane.tabs() {
        if let TabKind::File(path) = &tab.kind {
            host.allow_file(path);
        }
    }
    *IPC_RX.lock().expect("rx 锁") = host.take_receiver();
    let host = Arc::new(host);
    let selected = list.first().map(|(p, _)| p.clone());
    let app = App {
        window: None,
        size: (WINDOW.width as f64, WINDOW.height as f64),
        host,
        state: PreviewPanelState::new(pane),
        list,
        selected,
        maximized: false,
    };
    let task = window::open(window::Settings {
        size: WINDOW,
        ..Default::default()
    })
    .1
    .map(Msg::Ready);
    (app, task)
}

impl App {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ready(id) => self.window = Some(id),
            Msg::Resized(size) => {
                self.size = (size.width as f64, size.height as f64);
            }
            Msg::ListPicked(path) => self.open_file(path),
            Msg::CloseCurrent => {
                let active = self.state.pane.active_idx();
                if let Some(id) = self.state.pane.tabs().get(active).map(|t| t.id) {
                    self.state.pane.close_by_id(id);
                }
            }
            Msg::Maximize => self.maximized = !self.maximized,
            Msg::DestroyAll => self.host.destroy_all(),
            Msg::Tick => {}
            Msg::Ipc(preview_native::Event::Ipc { ref message, .. })
                if !matches!(message, IpcMessage::ZoomIn | IpcMessage::ZoomOut) =>
            {
                eprintln!("preview_smoke: {msg:?}"); // cli-output
            }
            Msg::Ipc(_) => {}
        }
        self.sync()
    }

    fn open_file(&mut self, path: PathBuf) {
        self.host.allow_file(&path);
        self.selected = Some(path.clone());
        self.state.pane.open_path_provisional(path);
    }

    /// 把库的期望清单对齐到预览自己的句柄表。`desired` 先在这里(宿主线程之外)
    /// 算好,再送进 `window::run` 的 `'static` 闭包。
    fn sync(&mut self) -> Task<Msg> {
        let Some(id) = self.window else {
            return Task::none();
        };
        let rect = LogicalRect {
            x: 0.0,
            y: TOP_BAR,
            width: self.size.0,
            height: (self.size.1 - TOP_BAR).max(1.0),
        };
        // 放大时整块被 iced 内容盖住,宿主把 webview 全部藏起(不是销毁)。
        let visible = !self.maximized;
        let desired =
            preview_native::desired(&self.state.pane, 0, ViewerPanel::Files, "en", |_| {
                Some(rect)
            });
        let desired = if visible {
            desired
        } else {
            desired
                .into_iter()
                .map(|(spec, rect)| {
                    let mut spec = spec;
                    spec.visible = false;
                    (spec, rect)
                })
                .collect()
        };
        let host = Arc::clone(&self.host);
        window::run(id, move |w| host.sync(w, &desired)).discard()
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::{button, column, container, row, scrollable, text};
        use iced::{Length, Theme};

        let title = format!(
            "preview_smoke  live={}  tabs={}  maximized={}",
            self.host.live_count(),
            self.state.pane.tabs().len(),
            self.maximized,
        );

        let toolbar = row![
            text(title).width(Length::Fill),
            button("关闭当前页签").on_press(Msg::CloseCurrent),
            button(if self.maximized {
                "还原"
            } else {
                "放大(藏起)"
            })
            .on_press(Msg::Maximize),
            button("全部销毁").on_press(Msg::DestroyAll),
        ]
        .spacing(8)
        .padding(8);

        let mut files = column![].spacing(2).padding(8);
        for (path, kind) in &self.list {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let label = format!("[{kind}] {name}");
            let mut b = button(text(label)).width(Length::Fill);
            if self.selected.as_deref() == Some(path.as_path()) {
                b = b.style(button::primary);
            }
            files = files.push(b.on_press(Msg::ListPicked(path.clone())));
        }

        let body: Element<'_, Msg> = if self.maximized {
            container(
                column![
                    text("放大:预览 webview 已被藏起(仍存活)"),
                    text(format!("live={}", self.host.live_count())),
                ]
                .spacing(6)
                .padding(16),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(text(format!(
                "预览区 … (webview 由宿主绘制,live={})",
                self.host.live_count()
            )))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        let content = row![
            container(scrollable(files)).width(Length::FillPortion(1)),
            container(body).width(Length::FillPortion(3)),
        ]
        .spacing(1)
        .height(Length::Fill);

        container(column![
            toolbar,
            content,
            container(text(format!("宿主存活句柄 {}", self.host.live_count())))
                .height(iced::Length::Fixed(STATUS_BAR as f32)),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Theme::Dark.palette().background.into()),
            ..Default::default()
        })
        .into()
    }
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    app.update(msg)
}

fn view(app: &App) -> Element<'_, Msg> {
    app.view()
}

/// 示例是单进程单窗口,IPC receiver 放进进程级静态槽,使订阅可以是无捕获
/// `fn() -> Stream`(`Subscription::run` 的唯一形态)。产品里应走自己的消息通道。
static IPC_RX: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<preview_native::Event>>> =
    Mutex::new(None);

fn ipc_stream() -> impl iced::futures::Stream<Item = Msg> {
    let rx = IPC_RX.lock().expect("rx 锁").take();
    iced::futures::stream::unfold(rx, |rx| async move {
        let mut rx = rx?;
        rx.recv().await.map(|event| (Msg::Ipc(event), Some(rx)))
    })
}

fn subscription(_: &App) -> iced::Subscription<Msg> {
    iced::Subscription::batch([
        iced::Subscription::run(ipc_stream),
        window::resize_events().map(|(_, size)| Msg::Resized(size)),
    ])
}

fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--selftest") {
        return selftest();
    }
    iced::application(build, update, view)
        .title("Digger preview smoke")
        .window(window::Settings {
            size: WINDOW,
            ..Default::default()
        })
        .subscription(subscription)
        .run()
}

// ---------------------------------------------------------------------------
// `--selftest`
// ---------------------------------------------------------------------------

/// 无 iced 事件循环的自检:`sync` 依赖窗口线程,所以这里用 `wry` 之外的裸窗口
/// 起不了;改为**每个夹具单独打开 + 在真实事件循环里推进若干帧**,以 IPC 的
/// `Ready` 作为"建好了"的信号。
///
/// 实现上跑一个最小 iced 应用:进入事件循环后按状态机推进,结束时以退出码
/// 汇报。这样 `window::run` 与产品同路径。
fn selftest() -> iced::Result {
    eprintln!("selftest: 启动"); // cli-output
    let list = fixtures();
    if let Err(reason) = selftest_preconditions(&list) {
        eprintln!("selftest: FAIL {reason}"); // cli-output
        std::process::exit(1);
    }

    let mut pane = PreviewPane::default();
    let host = preview_native::Host::new(asset_roots());
    for (path, _) in &list {
        host.allow_file(path);
        pane.open_path_provisional(path.clone());
    }
    *IPC_RX.lock().expect("rx 锁") = host.take_receiver();
    let host = Arc::new(host);

    let app = SelfTest {
        step: 0,
        window: None,
        size: (WINDOW.width as f64, WINDOW.height as f64),
        host,
        state: PreviewPanelState::new(pane),
        list,
        ready: std::collections::BTreeSet::new(),
        live_at_startup: 0,
        maximized: false,
        stress: None,
        deadline: Instant::now() + Duration::from_secs(90),
        failure: None,
    };

    eprintln!("selftest: 夹具 {} 个,进入事件循环", app.list.len()); // cli-output
    let slot = Mutex::new(Some(app));
    iced::application(
        move || {
            eprintln!("selftest: boot"); // cli-output
            let app = slot
                .lock()
                .expect("selftest 锁")
                .take()
                .expect("只 boot 一次");
            (
                app,
                window::open(window::Settings {
                    size: WINDOW,
                    ..Default::default()
                })
                .1
                .map(Msg::Ready),
            )
        },
        selftest_update,
        selftest_view,
    )
    .title("Digger preview selftest")
    .subscription(|_: &SelfTest| {
        iced::Subscription::batch([
            iced::Subscription::run(ipc_stream),
            window::resize_events().map(|(_, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(100)).map(|_| Msg::Tick),
        ])
    })
    .run()
}

/// 自检的前置条件。**缺夹具 / 缺资源一律算失败**,不再"找不到就跳过"——
/// 跳过会让一条永远不成立的回归在 CI 里保持绿色。
fn selftest_preconditions(list: &[(PathBuf, &'static str)]) -> Result<(), String> {
    let dir = fixture_dir();
    let absent: Vec<_> = list
        .iter()
        .map(|(path, _)| path)
        .filter(|path| !path.is_file())
        .map(|path| path.display().to_string())
        .collect();
    if !absent.is_empty() {
        return Err(format!("夹具缺失({}):{absent:?}", dir.display()));
    }
    let root = assets_dir();
    let missing = missing_assets(&root);
    if !missing.is_empty() {
        return Err(format!("资源根 {} 缺 {missing:?}", root.display()));
    }
    Ok(())
}

/// 示例进程里资源根的检查(产品侧是 `preview_assets::missing_in`,两边同一份名单)。
#[allow(dead_code)]
fn missing_assets(root: &std::path::Path) -> Vec<String> {
    let dirs = ["editor", "flyfish", "image-annotate"]
        .into_iter()
        .filter(|dir| !root.join(dir).is_dir())
        .map(|dir| format!("{dir}/"));
    let files = ["flyfish/host.html"]
        .into_iter()
        .filter(|file| !root.join(file).is_file())
        .map(str::to_string);
    dirs.chain(files).collect()
}

fn selftest_update(app: &mut SelfTest, msg: Msg) -> Task<Msg> {
    app.update(msg)
}

fn selftest_view(app: &SelfTest) -> Element<'_, Msg> {
    app.view()
}

/// Task 10 Step 2 的压力阶段进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StressPhase {
    #[default]
    OpenClose,
    SwitchProject,
    Maximize,
    Tabs,
}

#[derive(Debug, Default)]
struct Stress {
    phase: StressPhase,
    /// 当前阶段已经完成的动作数。
    done: usize,
    /// 本阶段见过的最大存活句柄数。
    peak: usize,
    /// 开/关阶段:上一拍是"开"还是"关"。
    close_next: bool,
    /// 页签阶段:50 个页签是否已经开过(下一拍核对清单)。
    opened: bool,
}

struct SelfTest {
    step: usize,
    window: Option<window::Id>,
    size: (f64, f64),
    host: Arc<preview_native::Host>,
    state: PreviewPanelState,
    list: Vec<(PathBuf, &'static str)>,
    ready: std::collections::BTreeSet<usize>,
    /// 第 0 步通过时的句柄数,第 2 步要证明"关掉全部页签就回到 0"。
    live_at_startup: usize,
    /// 放大:整块预览被 iced 盖住,webview 全部藏起但句柄仍在。
    maximized: bool,
    /// 压力测试的进度;`None` = 还没进压力阶段。
    stress: Option<Stress>,
    deadline: Instant,
    failure: Option<String>,
}

impl SelfTest {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Ready(id) => self.window = Some(id),
            Msg::Resized(size) => self.size = (size.width as f64, size.height as f64),
            Msg::Ipc(preview_native::Event::Ipc {
                webview_id,
                message: IpcMessage::Host(_) | IpcMessage::FocusedEnvelope,
            }) => {
                self.ready.insert(webview_id);
            }
            Msg::Ipc(preview_native::Event::Ipc { .. }) => {}
            Msg::Ipc(preview_native::Event::BuildFailed { error, .. }) => {
                self.failure = Some(format!("host 建失败:{error}"));
            }
            Msg::Tick => {}
            _ => {}
        }
        let sync = self.sync();
        Task::batch([sync, self.advance()])
    }

    fn sync(&mut self) -> Task<Msg> {
        let Some(id) = self.window else {
            return Task::none();
        };
        let rect = LogicalRect {
            x: 0.0,
            y: TOP_BAR,
            width: self.size.0,
            height: (self.size.1 - TOP_BAR).max(1.0),
        };
        let desired =
            preview_native::desired(&self.state.pane, 0, ViewerPanel::Files, "en", |_| {
                Some(rect)
            });
        let host = Arc::clone(&self.host);
        window::run(id, move |w| host.sync(w, &desired)).discard()
    }

    /// 每个"拍"检查一次期望;不满足就等下一拍(由 IPC/窗口事件驱动)。
    fn advance(&mut self) -> Task<Msg> {
        if std::env::var_os("SMOKE_DBG").is_some() {
            eprintln!(
                "DBG tick step={} live={} stress={:?}",
                self.step,
                self.host.live_count(),
                self.stress.as_ref().map(|s| (s.phase, s.done))
            ); // cli-output
        }
        if let Some(reason) = &self.failure {
            eprintln!("selftest: FAIL {reason}"); // cli-output
            std::process::exit(1);
        }
        if Instant::now() > self.deadline {
            let step = self.step;
            self.failure = Some(format!(
                "第 {step} 步超时(live={} expected={} ready={})",
                self.host.live_count(),
                self.expected_handles(),
                self.ready.len()
            ));
            return Task::none();
        }
        let expected = self.expected_handles();
        let actual = self.host.live_count();
        if actual != expected {
            return Task::none();
        }
        match self.step {
            0 => {
                // 句柄数相等只证明 Create 执行了;还要至少收到一条 host IPC,
                // 才证明协议应答 + 注入脚本 + classify_ipc 整条通路是通的。
                if self.ready.is_empty() {
                    return Task::none();
                }
                if actual != Self::first_batch_handles() {
                    self.failure = Some(format!(
                        "第 0 步句柄数 {actual} != 首批期望 {}(.md/.svg/.pdf/.html/.png 各 1;.py/.json/.csv/.txt 不建 webview)",
                        Self::first_batch_handles()
                    ));
                    return Task::none();
                }
                eprintln!("selftest: 第 0 步 ok(live={actual})"); // cli-output
                self.live_at_startup = actual;
                self.step = 1;
                // 关掉全部页签(不是直接销毁句柄):走库的 `close_by_id` 链路,
                // 期望清单随即变空,宿主在下一帧 `sync` 里销毁。
                let ids: Vec<_> = self.state.pane.tabs().iter().map(|tab| tab.id).collect();
                for id in ids {
                    self.state.pane.close_by_id(id);
                }
                return Task::none();
            }
            1 => {
                eprintln!("selftest: 第 1 步 ok(关掉全部页签后 live=0)"); // cli-output
                self.step = 2;
                self.ready.clear();
                // 重新打开同一批夹具,验证"关掉再开"仍然每个各一个 webview。
                for (path, _) in &self.list {
                    self.state.pane.open_path_provisional(path.clone());
                }
                return Task::none();
            }
            2 => {
                if self.ready.is_empty() {
                    return Task::none();
                }
                if actual != self.live_at_startup {
                    self.failure = Some(format!(
                        "第 2 步重开后 live={actual} != {}(首次 {})",
                        Self::first_batch_handles(),
                        self.live_at_startup
                    ));
                    return Task::none();
                }
                eprintln!("selftest: 第 2 步 ok(重开后 live={actual})"); // cli-output
                self.step = 3;
                // 压力阶段从"全关"的干净状态开始,免得把首批的句柄算进峰值。
                let ids: Vec<_> = self.state.pane.tabs().iter().map(|tab| tab.id).collect();
                for id in ids {
                    self.state.pane.close_by_id(id);
                }
                self.stress = Some(Stress::default());
                return Task::none();
            }
            3 => return self.advance_stress(actual),
            _ => {}
        }
        eprintln!("selftest: PASS"); // cli-output
        std::process::exit(0);
    }

    /// 自动化生命周期检查(Task 10 Step 2)。每到一个"稳定点"(句柄数与期望
    /// 一致)就发下一步动作:
    ///
    /// - 连续开 / 关预览页签 50 次;
    /// - 切"项目"20 次(每次把整块预览换成另一组文件再换回,等价于产品里切
    ///   项目页签时的 `set_current_project` + 全关);
    /// - 放大 / 还原 20 次(放大时 webview 全藏起,但句柄数不变);
    /// - 一次打开 50 个页签。
    ///
    /// 每一拍都核对句柄数,峰值也记下来:`sync` 是异步的,但期望清单与句柄表
    /// 收敛后必须一致,且不能越积越多。
    fn advance_stress(&mut self, actual: usize) -> Task<Msg> {
        let expected = self.expected_handles();
        let Some(stress) = self.stress.as_mut() else {
            return Task::none();
        };
        if actual != expected {
            return Task::none();
        }
        // 开/关切项目阶段要求句柄收敛到期望且至少收到过一次 IPC;许多页签
        // 阶段只要求收敛;放大阶段每拍只做"放大"或"还原"其中一下。
        // 放大阶段只做可见性切换;页签阶段(一次 50 个 `Loading` 页签)宿主建得
        // 慢,不等 IPC——句柄数相等就够,就绪由下一步的整体流程证明。
        if !matches!(stress.phase, StressPhase::Maximize | StressPhase::Tabs)
            && self.ready.is_empty()
            && actual > 0
        {
            return Task::none();
        }
        stress.peak = stress.peak.max(actual);
        eprintln!(
            "DBG phase={:?} done={} actual={actual} expected={expected}",
            stress.phase, stress.done
        ); // cli-output
        match stress.phase {
            StressPhase::OpenClose => {
                if stress.done == 50 {
                    eprintln!("selftest: 压力 开/关 50 次 ok(peak={})", stress.peak); // cli-output
                    stress.phase = StressPhase::SwitchProject;
                    stress.done = 0;
                    stress.peak = 0;
                    return Task::none();
                }
                stress.done += 1;
                if stress.close_next {
                    let ids: Vec<_> = self.state.pane.tabs().iter().map(|t| t.id).collect();
                    for id in ids {
                        self.state.pane.close_by_id(id);
                    }
                    self.ready.clear();
                } else {
                    for (path, _) in &self.list {
                        self.state.pane.open_path_provisional(path.clone());
                    }
                }
                stress.close_next = !stress.close_next;
                Task::none()
            }
            StressPhase::SwitchProject => {
                if stress.done == 20 {
                    eprintln!("selftest: 压力 切项目 20 次 ok(peak={})", stress.peak); // cli-output
                    stress.phase = StressPhase::Maximize;
                    stress.done = 0;
                    stress.peak = 0;
                    return Task::none();
                }
                // 切走:全关(产品里是销毁整块预览);切回:只开前两个夹具。
                stress.done += 1;
                if stress.close_next {
                    let ids: Vec<_> = self.state.pane.tabs().iter().map(|t| t.id).collect();
                    for id in ids {
                        self.state.pane.close_by_id(id);
                    }
                    self.ready.clear();
                } else {
                    for (path, _) in self.list.iter().take(2) {
                        self.state.pane.open_path_provisional(path.clone());
                    }
                }
                stress.close_next = !stress.close_next;
                Task::none()
            }
            StressPhase::Maximize => {
                if self.maximized {
                    // 上一拍已经切到"放大"并记过数,这一拍先还原,`sync` 在
                    // `update` 末尾把可见性改回来;下一拍再切"放大"。
                    self.maximized = false;
                    return Task::none();
                }
                if stress.done == 20 {
                    eprintln!("selftest: 压力 放大/还原 20 次 ok(peak={})", stress.peak); // cli-output
                    stress.phase = StressPhase::Tabs;
                    stress.done = 0;
                    stress.peak = 0;
                    return Task::none();
                }
                // 放大时 `sync` 把全部 webview 的 `visible` 置 false,句柄数不变。
                self.maximized = true;
                stress.done += 1;
                Task::none()
            }
            StressPhase::Tabs => {
                if stress.opened {
                    // 全部 50 个页签都必须进了期望清单——即 tab 栏溢出只裁显示、
                    // 不丢页签,切到溢出的那个仍能建出对应宿主。
                    if expected != Self::MANY_TABS {
                        self.failure = Some(format!(
                            "50 页签时期望清单 {expected} != {}",
                            Self::MANY_TABS
                        ));
                        return Task::none();
                    }
                    eprintln!("selftest: 压力 50 个页签 ok(live={actual})"); // cli-output
                    self.host.destroy_all();
                    self.step = 4;
                    return Task::none();
                }
                // 同一路径重复打开会被库去重;用不同的临时文件名凑 50 个页签
                // (进程退出前清理)。
                for i in 0..Self::MANY_TABS {
                    let copy = std::env::temp_dir().join(format!("digger stress {i}.md"));
                    let _ = std::fs::write(&copy, format!("# stress {i}\n"));
                    self.host.allow_file(&copy);
                    self.state.pane.open_path_provisional(copy);
                }
                stress.opened = true;
                Task::none()
            }
        }
    }

    /// 一次打开的页签数(Task 10 Step 2 的"50 个页签打开不崩溃")。
    const MANY_TABS: usize = 50;

    /// 首批夹具里真正建 webview 的数量:`.py`(原生编辑器,不建宿主)之外
    /// 的渲染型各一——`.md` / `.svg` / `.pdf`(flyfish)、`.html`(隔离渲染)、
    /// `.png`(image-annotate),共 5 个。`.json` / `.csv` 在 Digger 的能力下
    /// 是原生降级页,`.txt` 走原生编辑器,都不建宿主。
    fn first_batch_handles() -> usize {
        5
    }

    fn expected_handles(&self) -> usize {
        self.state
            .pane
            .desired_webviews(0, ViewerPanel::Files, "en")
            .len()
    }

    fn view(&self) -> Element<'_, Msg> {
        iced::widget::text(format!(
            "selftest step {} live={}",
            self.step,
            self.host.live_count()
        ))
        .into()
    }
}
