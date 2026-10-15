//! Digger 产品壳:在共享 bytehost 宿主上启动的产品组合根。
//!
//! 窗口结构(顶栏 / 左右图标栏 / 左右面板区 / 分隔线 / 放大态 / 布局持久化)来自共享骨架 `bytehost-chrome`,与 Dozer 的
//! 窗口结构同源;本文件只做产品侧的事:注册面板、实现 `ShellHost`、把骨架消息接到各面板的切入动作和原生 webview 的
//! 同步上。拥有自己的数据根、主题、布局落盘与退出,**不复制** Dozer 的 `App`/`Workspace`。
//!
//! 范围(S1):以**无 Project** 状态启动;顶栏的项目页签行保留结构、内容留空,"＋"置灰;**不伪造**任何写入能力。

use std::path::PathBuf;

use bytehost_chrome::persist::SCHEMA_VERSION;
use bytehost_chrome::{BodyArea, El, Loaded, PanelMeta, PanelRules, Rect, ShellHost, ShellModel, ShellMsg, TopBarSlots, metrics_from_theme};
use bytehost_core::registry::PanelCatalog;
use bytehost_core::{LogicalRect, PanelId, PanelMeta as CatalogEntry};
use bytehost_desktop::{HostMessage, Product, route_lifecycle};
use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};

use iced::widget::{column, container, row, text, tooltip};
use iced::{Element, Length, Size, Subscription, Task, window};

use crate::{paths, shell};

/// 应用面板自己的头部(返回按钮 + 应用 id + 状态)的高度:原生应用 webview 摆在它下面。
const APPS_HEADER_HEIGHT: f32 = 96.0;
/// 用量面板自己的原生头部(标题 + 加载态)的高度:图表 webview 摆在它下面。
const USAGE_HEADER_HEIGHT: f32 = 80.0;

/// 产品消息。**不进入**宿主 enum(宿主只认 `HostMessage<M>`)。
#[derive(Debug, Clone)]
pub enum Msg {
    WindowReady(window::Id, Size),
    /// 窗口骨架的消息(点图标、拖分隔线、窗口缩放……),由 `bytehost-chrome` 的 `ShellModel` 处理。
    Shell(ShellMsg),
    /// 「应用」面板的消息(Task 14)。
    Apps(crate::apps::Message),
    /// 「用量」面板的库消息(Task 20)。
    Usage(dozer_panel_usage::Message),
    NativeUsage(crate::adapters::usage_native::Event),
    /// 「编辑器」面板的消息(ADR-013)。
    Markdown(crate::markdown::Message),
    /// 面板生命周期自检的推进(仅 `DIGGER_LIFECYCLE_SELFTEST` 设置时出现)。
    Lifecycle(crate::lifecycle_selftest::Phase),
    Tick,
    /// 双击顶栏空白处:缩放窗口(系统标题栏没了,这个手势要自己补)。
    ZoomToggle,
    ShutdownFinished(Result<(), String>),
    ForceExit,
    CloseRequested,
    Exit,
}

/// Digger 产品壳状态。刻意保持最小:不持有任何 Domain/持久化状态,只有 UI 交互态 + 布局 + 主题。
pub struct ProductShell {
    window: Option<window::Id>,
    catalog: PanelCatalog,
    /// 窗口骨架状态(布局、选中、收起、放大、尺寸……)。
    shell: ShellModel,
    shell_path: PathBuf,
    /// 非 `None` 表示磁盘上是**不支持的高版本**布局:内存默认布局仅用于显示,**禁止回写**该文件。
    shell_save_blocked: Option<String>,
    /// 「应用」面板的组合根(状态机 + 执行器 + 内嵌应用服务)。
    apps: crate::apps::AppsState,
    /// 「用量」面板适配(共享 `dozer-panel-usage` 库 + 注入的真实用量服务)。
    usage: crate::adapters::usage::UsagePanel,
    /// 「编辑器」面板:自包含的第一方面板(文本与协议在 `digger-editor`,这里只有宿主适配)。
    markdown: crate::markdown::MarkdownPanel,
    /// 生命周期自检的驱动(只在环境变量设置时有)。
    lifecycle: Option<crate::lifecycle_selftest::Driver>,
    /// [应用, 用量] 面板上一次是否可见:变为可见时触发各自的切入动作;应用面板变为不可见时关掉正在显示的应用。
    visible_before: [bool; 2],
    /// 产品未接业务服务时的启动说明,显示在信息面板。
    boot_note: String,
    closing: bool,
    shutdown_started: bool,
    shutdown_error: Option<String>,
    /// 测试用:记录收到的生命周期回调,验证 Surface 事件恰好路由一次(复盘发现 6)。
    #[cfg(test)]
    surface_callbacks: Vec<bytehost_desktop::SurfaceSlot>,
}

fn logical(r: Rect) -> LogicalRect {
    LogicalRect { x: r.x as f64, y: r.y as f64, width: r.w as f64, height: r.h as f64 }
}

/// 向下缩 `top` 像素(面板自己的头部占掉的高度)。
fn below_header(r: Rect, top: f32) -> Rect {
    Rect::new(r.x, r.y + top, r.w, (r.h - top).max(1.0))
}

impl ProductShell {
    /// 构造:注册产品面板目录、读布局(缺省用默认布局),不打开任何 Project。
    pub fn new() -> Self {
        Self::with_shell_path(paths::shell_path())
    }

    pub(crate) fn with_shell_path(shell_path: PathBuf) -> Self {
        let mut catalog = PanelCatalog::new();
        for (id, title) in [(shell::info_id(), "Digger"), (shell::apps_id(), "应用"), (shell::usage_id(), "用量")] {
            catalog.register(CatalogEntry { id, title: title.into(), surfaces: vec![] }).expect("首次注册产品面板");
        }
        catalog.register(CatalogEntry::single_surface(shell::markdown_id(), "编辑器")).expect("首次注册 digger.markdown");

        let loaded = shell::load(&shell_path);
        let shell_save_blocked = match &loaded {
            Loaded::Newer { found, .. } => Some(format!("布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**")),
            _ => None,
        };
        let mut model = loaded.into_model();

        // 任务完成用量账本(Task 20):生产者(应用操作)写、用量面板读,同一实例。
        // 载入失败(损坏)不 panic,也**不**退回一次性临时空账本(复审 #9)——后者会把"读不出来"伪装成 "Ok(空)",且新记录
        // 写进下次启动就不再引用的临时文件,等于静默丢。改为:查询恒返回 Unavailable、**不接线**用量生产者(没有记录悄悄
        // 写走),原损坏文件保持原样。
        let scope_path = paths::config_dir();
        let (usage_service, usage_ledger, boot_note) = load_usage_ledger(paths::usage_ledger_path());

        let mut apps = crate::apps::AppsState::new();
        // 只有账本可用时才接用量生产者;损坏时**不接**,避免记录写进无引用的临时文件。
        if let Some(ledger) = &usage_ledger {
            apps.attach_usage(ledger.clone(), scope_path.to_string_lossy().to_string());
        }
        let usage = crate::adapters::usage::UsagePanel::new(usage_service, scope_path.clone());
        let markdown = crate::markdown::MarkdownPanel::new();
        let lifecycle = crate::lifecycle_selftest::Driver::from_env();
        // 自检模式需要编辑器一开始就看得见。
        if markdown.is_selftest() || lifecycle.is_some() {
            shell::update(&mut model, ShellMsg::ShowPanel(shell::markdown_id()));
        }

        Self {
            window: None,
            catalog,
            shell: model,
            shell_path,
            shell_save_blocked,
            apps,
            usage,
            markdown,
            lifecycle,
            visible_before: [false; 2],
            boot_note,
            closing: false,
            shutdown_started: false,
            shutdown_error: None,
            #[cfg(test)]
            surface_callbacks: Vec::new(),
        }
    }

    /// 落盘当前骨架状态(退出 / 布局变化时调用)。失败不 panic,记入 boot_note。
    /// 磁盘上是**不支持的高版本**布局时**拒绝回写**(复盘发现 4:绝不覆盖原文件)。
    fn save_shell(&mut self) {
        if let Some(note) = &self.shell_save_blocked {
            self.boot_note = format!("布局未保存:{note}");
            return;
        }
        if let Err(e) = bytehost_chrome::persist::save_atomic(&self.shell_path, &self.shell) {
            self.boot_note = format!("布局保存失败:{e}");
        }
    }

    /// 面板此刻是否真的呈现在用户眼前:它是所在栏的选中面板、所在区没收起、也没被另一侧的放大态盖住。
    pub(crate) fn panel_visible(&self, panel: &PanelId) -> bool {
        let Some(side) = self.shell.side_of(panel) else { return false };
        self.shell.selected(side) == panel && !self.shell.collapsed(side) && self.shell.maximized().is_none_or(|m| m == side)
    }

    /// 面板刚被切入(或启动时本来就可见):触发各自的"切入时动作"。
    fn on_visible(&mut self, panel: &PanelId) -> Task<Msg> {
        if *panel == shell::apps_id() {
            // 首次进入「应用」面板:触发一次探测 + 拉列表(Task 14)。
            return self.apps.update(crate::apps::Message::Opened).map(Msg::Apps);
        }
        if *panel == shell::usage_id() {
            // 进入「用量」面板:触发一次取数(库的 on_activate,Task 20)。
            self.usage.activate();
        }
        Task::none()
    }

    fn on_shell(&mut self, msg: ShellMsg) -> Task<Msg> {
        if self.closing {
            return Task::none();
        }
        let resized = matches!(msg, ShellMsg::Resized { .. });
        let out = shell::update(&mut self.shell, msg);
        if out.persist {
            self.save_shell();
        }
        let mut tasks = Vec::new();
        // "切入时动作"挂在"从看不见变成看得见"上,而不只是"被切入":展开收起的区、退出放大态、跨栏搬走对侧面板后
        // 本栏退回的面板,都没有 `switched_in`,但用户眼前出现了这个面板。动作本身幂等,与 `switched_in` 重叠无害。
        let visible_now = [shell::apps_id(), shell::usage_id()].map(|p| self.panel_visible(&p));
        let mut entered: Vec<PanelId> = out.switched_in.into_iter().collect();
        for (panel, (now, was)) in [shell::apps_id(), shell::usage_id()].into_iter().zip(visible_now.into_iter().zip(self.visible_before)) {
            if now && !was && !entered.contains(&panel) {
                entered.push(panel);
            }
        }
        for panel in entered {
            tasks.push(self.on_visible(&panel));
        }
        // 应用面板从可见变成不可见(切走 / 收起 / 被放大盖住):关掉正在显示的应用,别留一个看不见的 webview 盖在别处。
        if self.visible_before[0] && !visible_now[0] {
            self.apps.close_selected();
        }
        self.visible_before = visible_now;
        if resized {
            tasks.push(self.restyle_window());
        }
        Task::batch(tasks)
    }

    /// macOS:红绿灯垂直居中到顶栏里;窗口缩放(含全屏进出)后重设,幂等。其它平台无事可做。
    fn restyle_window(&self) -> Task<Msg> {
        #[cfg(target_os = "macos")]
        if let Some(id) = self.window {
            let top = byteui::theme::geometry::top_bar_height();
            return window::run(id, move |w| bytehost_chrome::platform_macos::center_traffic_lights(w, top)).discard();
        }
        Task::none()
    }

    /// 原生 webview 的位置与可见性:矩形取自骨架的面板内容矩形(外框 − 外边距 − 内边距;放大态是放大盒子),
    /// 再各自减掉面板自己头部占的高度。**骨架负责"在哪、看不看得见",面板只负责自己头部多高。**
    fn sync_native(&mut self) -> Task<Msg> {
        let Some(window) = self.window else {
            return Task::none();
        };
        let rects = self.shell.rects(&metrics_from_theme());
        let content_of = |panel: &PanelId| -> Option<Rect> {
            let side = self.shell.side_of(panel)?;
            if self.shell.selected(side) != panel {
                return None;
            }
            rects.content(side)
        };
        let apps_rect = content_of(&shell::apps_id());
        let usage_rect = content_of(&shell::usage_id());
        let markdown_rect = content_of(&shell::markdown_id());
        let hidden = LogicalRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
        let closing = self.closing;

        let apps_task = match apps_rect {
            Some(r) if !closing => self.apps.sync_native(window, logical(below_header(r, APPS_HEADER_HEIGHT))).map(Msg::Apps),
            // 关闭时由 `apps.prepare_shutdown` 收尾;不可见时 `on_shell` 已 `close_selected`,这里仍要让待办的原生动作跑完。
            _ => self.apps.sync_native(window, hidden).map(Msg::Apps),
        };
        let usage_task = self
            .usage
            .sync_native(window, usage_rect.is_some() && !closing, usage_rect.map(|r| logical(below_header(r, USAGE_HEADER_HEIGHT))).unwrap_or(hidden))
            .map(Msg::NativeUsage);
        // 关闭时销毁编辑器 webview 并丢掉文档;否则只在可见时显示,切走则隐藏。
        let markdown_task = if closing {
            self.markdown.close(window)
        } else {
            self.markdown.sync_native(window, markdown_rect.is_some(), markdown_rect.map(logical).unwrap_or(hidden))
        }
        .map(Msg::Markdown);
        Task::batch([apps_task, usage_task, markdown_task])
    }

    fn info_view(&self) -> Element<'_, Msg> {
        let tokens = color::current();
        let dim = tokens.dim;
        container(
            column![
                text(format!("面板目录已注册 {} 个(产品组合根)", self.catalog.len())).color(tokens.body),
                text(format!("数据根:{}", paths::config_dir().display())).color(dim),
                text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
                text("当前 Project:无").color(tokens.body),
                text(&self.boot_note).color(tokens.gold),
                iced::widget::button("退出 Digger").on_press(Msg::Exit),
            ]
            .spacing(8)
            .width(Length::Fill),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn apps_view(&self) -> Element<'_, Msg> {
        container(self.apps.view().map(Msg::Apps)).padding(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn usage_view(&self) -> Element<'_, Msg> {
        container(self.usage.view()).padding(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn markdown_view(&self) -> Element<'_, Msg> {
        // 条在上;下面留空,原生 webview 由 `sync_native` 摆在条的下方。
        column![self.markdown.view().map(Msg::Markdown), iced::widget::Space::new().height(Length::Fill)].into()
    }
}

/// 生命周期自检要读的东西。
impl ProductShell {
    pub fn window_id(&self) -> Option<window::Id> {
        self.window
    }

    pub fn window_size(&self) -> (f32, f32) {
        self.shell.window()
    }

    pub fn markdown_ready(&self) -> bool {
        self.markdown.is_ready()
    }

    pub fn markdown_last_probe(&self) -> Option<crate::markdown::Probe> {
        self.markdown.last_probe()
    }

    pub fn markdown_probe(&self, id: window::Id) -> Task<Msg> {
        self.markdown.probe(id).map(Msg::Markdown)
    }
}

/// 顶栏三个槽位。S1 没有 Project:标题只是展示,页签行显示"无项目","＋"置灰并提示。
fn top_bar_slots<'a>() -> TopBarSlots<'a, Msg> {
    let tokens = color::current();
    let dim = tokens.dim;
    let title = row![
        icons::view(IconKind::Home, icon_size::home(), dim),
        text("Digger").size(font::body()).color(dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let plus = tooltip(
        container(icons::view(IconKind::Plus, icon_size::rail(), dim)).padding(6),
        container(text("Project 尚未接入").size(font::body())).padding(6).style(container::rounded_box),
        tooltip::Position::Bottom,
    );
    let tabs = row![text("无项目").size(font::body()).color(dim), plus].spacing(10).align_y(iced::Alignment::Center);
    TopBarSlots { title: title.into(), tabs: tabs.into(), right: iced::widget::Space::new().into() }
}

impl ShellHost<Msg> for ProductShell {
    fn lift(msg: ShellMsg) -> Msg {
        Msg::Shell(msg)
    }

    fn meta(&self, panel: &PanelId) -> Option<PanelMeta> {
        shell::meta(panel)
    }

    fn rules(&self, panel: &PanelId) -> Option<PanelRules> {
        shell::rules(panel)
    }

    fn body<'a>(&'a self, panel: &PanelId, _area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(),
            "digger.markdown" => self.markdown_view(),
            _ => column![].into(),
        }
    }

    fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
        top_bar_slots()
    }

    fn footbar<'a>(&'a self) -> El<'a, Msg> {
        // 底栏占位槽:S1 里没有内容,但高度与 Dozer 一致,窗口结构不变。
        iced::widget::Space::new().into()
    }

    fn on_top_bar_double_click(&self) -> Msg {
        Msg::ZoomToggle
    }
}

/// 载入用量账本并决定产品侧接线(复审 #9)。
///
/// 返回 `(用量服务, 可选共享账本, boot 提示)`:
/// - 成功 → 真账本 + [`LedgerUsageService`](digger_store::LedgerUsageService);
/// - 损坏/读失败 → **不退回临时空账本**,日志查询恒 `Unavailable`,账本为 `None`(调用方据此
///   不接线用量生产者——没有记录会悄悄写进一个下次启动就不再引用的文件)。原文件保持原样。
fn load_usage_ledger(
    path: std::path::PathBuf,
) -> (
    std::sync::Arc<dyn dozer_services::UsageService>,
    Option<std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>>,
    String,
) {
    match digger_store::UsageLedger::load(path) {
        Ok(ledger) => {
            let ledger = std::sync::Arc::new(std::sync::Mutex::new(ledger));
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::LedgerUsageService::new(ledger.clone()));
            let note = "尚未打开 Project。可在「应用」页管理并运行应用。".to_string();
            (svc, Some(ledger), note)
        }
        Err(e) => {
            let note = format!("用量账本载入失败,用量查询暂不可用(不会丢数据:原文件未改动): {e}");
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::UnavailableUsageService::new(note.clone()));
            (svc, None, note)
        }
    }
}

impl Default for ProductShell {
    fn default() -> Self {
        Self::new()
    }
}

impl Product<Msg> for ProductShell {
    fn update(&mut self, msg: HostMessage<Msg>) -> Task<Msg> {
        let HostMessage::Product(m) = msg else {
            // Surface 事件**不在这里**再分发给生命周期:那由外层入口 [`update_host`] 单次调用
            // `route_lifecycle` 完成(复盘发现 6:在此调用会让 `route_lifecycle` → `update` 形成无限递归)。
            return Task::none();
        };
        match m {
            Msg::WindowReady(id, size) => {
                self.window = Some(id);
                let resized = self.on_shell(ShellMsg::Resized { w: size.width, h: size.height });
                // macOS:全屏期间把红绿灯临时挂到内容视图(原生全屏不绘制标题栏,红绿灯会跟着移出窗口)。
                #[cfg(target_os = "macos")]
                let guard = window::run(id, |w| bytehost_chrome::platform_macos::install_fullscreen_guard(w)).discard();
                #[cfg(not(target_os = "macos"))]
                let guard = Task::none();
                Task::batch([resized, guard])
            }
            Msg::Shell(m) => self.on_shell(m),
            Msg::ZoomToggle => self.window.map(window::toggle_maximize).unwrap_or_else(Task::none),
            Msg::Apps(m) => self.apps.update(m).map(Msg::Apps),
            Msg::Usage(m) => {
                self.usage.update(m);
                // Digger 一期无 Toast:把库的 outbox 提示落地到 stderr,不静默吞掉。
                for (level, text) in self.usage.take_notices() {
                    eprintln!("digger: usage 面板/{level}: {text}"); // cli-output
                }
                Task::none()
            }
            Msg::Tick => self.apps.update(crate::apps::Message::Tick).map(Msg::Apps),
            Msg::NativeUsage(event) => {
                self.usage.native.update(event);
                Task::none()
            }
            Msg::Markdown(m) => self.markdown.update(m).map(Msg::Markdown),
            Msg::Lifecycle(phase) => {
                let Some(mut driver) = self.lifecycle.take() else {
                    return Task::none();
                };
                let task = driver.advance(self, phase);
                self.lifecycle = Some(driver);
                task
            }
            Msg::ShutdownFinished(result) => match result {
                // 生命周期自检要在关闭之后读句柄表,检查完自己退出
                Ok(()) if self.lifecycle.is_some() => Task::none(),
                Ok(()) => iced::exit(),
                Err(error) => {
                    self.shutdown_error = Some(error);
                    Task::none()
                }
            },
            Msg::ForceExit => iced::exit(),
            Msg::CloseRequested | Msg::Exit => {
                if self.closing {
                    return Task::none();
                }
                self.closing = true;
                self.save_shell();
                self.apps.prepare_shutdown();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::button;

        if self.closing {
            let mut body = column![text("正在关闭 Digger…")].spacing(16).padding(20);
            if let Some(error) = &self.shutdown_error {
                body = body.push(text(error)).push(button("强制退出").on_press(Msg::ForceExit));
            }
            return body.into();
        }
        bytehost_chrome::view(&self.shell, self, &metrics_from_theme())
    }

    fn on_activated(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }

    fn on_closed(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }
}

/// 产品壳的**唯一**宿主入口:Surface 事件在这里**单次**经 `route_lifecycle` 分发到产品生命周期回调
/// (复盘发现 6),产品消息则走 `Product::update`。`Product::update` 的 Surface 分支只消费、不再重投,
/// 二者分工使路由不成环。
pub fn update_host(shell: &mut ProductShell, msg: HostMessage<Msg>) -> Task<Msg> {
    let task = match msg {
        HostMessage::Surface(event) => route_lifecycle(shell, event),
        HostMessage::Product(m) => Product::update(shell, HostMessage::Product(m)),
    };
    // 退出分支自己将销毁与 shutdown 串行；其他更新后同步内容与几何。
    let native = shell.sync_native();
    if shell.closing && !shell.shutdown_started && shell.apps.native_idle() {
        shell.shutdown_started = true;
        Task::batch([task, native]).chain(Task::perform(shell.apps.shutdown(), Msg::ShutdownFinished))
    } else {
        Task::batch([task, native])
    }
}

/// 订阅:窗口关闭请求、骨架需要的鼠标 / 缩放事件,以及仅在有动画时才开的 ~60fps 动画拍。
pub fn subscription(shell: &ProductShell) -> Subscription<Msg> {
    let events = iced::event::listen_with(|evt, _status, _id| {
        if let iced::Event::Window(window::Event::CloseRequested) = evt {
            return Some(Msg::CloseRequested);
        }
        bytehost_chrome::map_event(&evt).map(Msg::Shell)
    });
    if shell.closing {
        return events;
    }
    let mut subs = vec![events, iced::time::every(std::time::Duration::from_secs(1)).map(|_| Msg::Tick)];
    if shell.shell.animating() {
        subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Msg::Shell(ShellMsg::Tick)));
    }
    Subscription::batch(subs)
}

/// 入口用:初始窗口尺寸 = 上次退出时保存的尺寸(没有就是默认尺寸)。
pub fn default_window_size_for_boot() -> Size {
    let (w, h) = shell::load(&paths::shell_path()).model().window();
    Size::new(w, h)
}

/// 应用初始化:确保产品数据根、设置 ByteBoy2077 主题,异步取窗口尺寸。
pub fn boot() -> (ProductShell, Task<Msg>) {
    if let Err(e) = paths::ensure_data_root() {
        // 数据根不可用不 panic:产品壳仍可打开,只是无法持久化布局。
        // (转为可见提示由产品壳承载。)
        eprintln!("digger: 数据根不可用: {e}"); // cli-output
    }
    color::set_theme(color::ColorTokens::byteboy2077());
    font::set_theme(font::FontTokens::byteboy2077());

    let mut shell = ProductShell::new();
    let size = default_window_size_for_boot();
    // 用量面板要 iced 运行时句柄在后台取数:`boot` 在 iced 运行时内执行,这里补挂。
    shell.usage.attach_handle(tokio::runtime::Handle::current());
    // 应用面板后台结果回流:一次性流出接收端,持续到进程结束(与窗口尺寸任务并行)。
    use iced::futures::StreamExt;
    let reply_task = match crate::apps::take_reply_receiver(&shell.apps.reply_receiver()) {
        Some(rx) => iced::Task::stream(crate::apps::reply_stream(rx).map(Msg::Apps)),
        None => Task::none(),
    };
    // 用量面板后台取数结果回流:同型,接成 iced 流回投 `Msg::Usage`。
    let usage_task = match shell.usage.take_receiver() {
        Some(rx) => iced::Task::stream(crate::adapters::usage::receiver_stream(rx).map(Msg::Usage)),
        None => Task::none(),
    };
    let size_task = bytehost_desktop::panel::oldest_window().then(move |id| match id {
        Some(id) => bytehost_desktop::panel::window_size(id).map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    let native_usage = match shell.usage.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|event| (Msg::NativeUsage(event), rx)) })),
        None => Task::none(),
    };
    let markdown_task = match shell.markdown.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|event| (Msg::Markdown(event), rx)) })),
        None => Task::none(),
    };
    let lifecycle_task = if shell.lifecycle.is_some() { crate::lifecycle_selftest::Driver::first() } else { Task::none() };
    // 启动时就可见的面板(例如默认右栏选中的用量)也要触发各自的"切入时动作"。
    let mut initial = Vec::new();
    for panel in [shell::apps_id(), shell::usage_id()] {
        if shell.panel_visible(&panel) {
            initial.push(shell.on_visible(&panel));
        }
    }
    shell.visible_before = [shell::apps_id(), shell::usage_id()].map(|p| shell.panel_visible(&p));
    (shell, Task::batch([size_task, reply_task, usage_task, native_usage, markdown_task, lifecycle_task, Task::batch(initial)]))
}


#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;
    use bytehost_desktop::SurfaceEvent;

    fn test_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("digger-product-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 带独立落盘路径的壳:互不干扰,也不碰真实配置目录。
    fn fresh(tag: &str) -> ProductShell {
        ProductShell::with_shell_path(test_temp_dir(tag).join("shell.json"))
    }

    fn press(shell: &mut ProductShell, panel: &PanelId) {
        let _ = Product::update(shell, HostMessage::Product(Msg::Shell(ShellMsg::PanelPress { panel: panel.clone() })));
    }

    #[test]
    fn starts_without_project() {
        let shell = ProductShell::new();
        assert!(shell.boot_note.contains("尚未打开 Project"));
    }

    #[test]
    fn registers_product_panels() {
        let shell = ProductShell::new();
        for id in shell::all_panels() {
            assert!(shell.catalog.contains(&id), "{id}");
        }
        assert_eq!(shell.catalog.len(), 4);
        // 编辑器声明一个主 Surface 槽位;纯 iced 原生面板不声明。
        assert_eq!(shell.catalog.get(&shell::markdown_id()).unwrap().surfaces, vec![bytehost_desktop::SurfaceSlot::MAIN]);
        assert!(shell.catalog.get(&shell::info_id()).unwrap().surfaces.is_empty());
    }

    #[test]
    fn the_default_shell_shows_the_editor_on_the_left_and_usage_on_the_right() {
        let shell = fresh("default");
        assert!(shell.panel_visible(&shell::markdown_id()));
        assert!(shell.panel_visible(&shell::usage_id()));
        assert!(!shell.panel_visible(&shell::info_id()));
        assert!(!shell.panel_visible(&shell::apps_id()));
        assert_eq!(shell.shell.side_of(&shell::apps_id()), Some(Side::Right));
    }

    #[test]
    fn a_panel_is_not_visible_when_its_zone_is_collapsed_or_covered_by_the_other_sides_maximize() {
        let mut shell = fresh("visible");
        press(&mut shell, &shell::markdown_id()); // collapses the left zone
        assert!(!shell.panel_visible(&shell::markdown_id()));
        press(&mut shell, &shell::markdown_id()); // expands it again
        assert!(shell.panel_visible(&shell::markdown_id()));
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Right))));
        assert!(!shell.panel_visible(&shell::markdown_id()), "covered by the right side's maximize");
        assert!(shell.panel_visible(&shell::usage_id()));
    }

    #[test]
    fn a_shell_change_is_saved_and_restored_on_the_next_start() {
        let dir = test_temp_dir("saved");
        let path = dir.join("shell.json");
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        assert!(path.exists(), "a layout change is written right away");
        let again = ProductShell::with_shell_path(path);
        assert_eq!(again.shell.selected(Side::Left), &shell::info_id());
    }

    /// 复盘发现 4:高版本布局文件退出时**不得覆盖**。
    #[test]
    fn a_newer_schema_shell_file_is_never_overwritten() {
        let dir = test_temp_dir("newer");
        let path = dir.join("shell.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&path, original).unwrap();
        let mut shell = ProductShell::with_shell_path(path.clone());
        assert!(shell.shell_save_blocked.is_some());
        press(&mut shell, &shell::info_id());
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original, "the original file stays byte for byte");
        assert!(shell.boot_note.contains("布局未保存"));
    }

    #[test]
    fn the_native_rect_loses_exactly_the_panel_header_and_never_goes_below_one_pixel() {
        let r = below_header(Rect::new(10.0, 100.0, 300.0, 400.0), 96.0);
        assert_eq!((r.x, r.y, r.w, r.h), (10.0, 196.0, 300.0, 304.0));
        assert_eq!(below_header(Rect::new(0.0, 0.0, 50.0, 40.0), 96.0).h, 1.0);
    }

    /// 评审 Important 3:应用面板只要从"看不见"变成"看得见"就要触发探测 + 拉列表,不能只在"被切入"时。
    /// 上次退出时应用面板被选中但右区收起;重启后点它的图标只是展开(没有 `switched_in`),服务必须随之启动。
    #[tokio::test]
    async fn the_apps_panel_starts_when_expanding_its_zone_makes_it_visible() {
        let dir = test_temp_dir("apps-visible");
        let path = dir.join("shell.json");
        std::fs::write(&path, r#"{ "selected_right": "digger.apps", "right_collapsed": true }"#).unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert!(!shell.panel_visible(&shell::apps_id()), "selected but its zone is collapsed");
        assert!(!shell.apps.opened());
        press(&mut shell, &shell::apps_id()); // expands the zone: no panel switch happens
        assert!(shell.panel_visible(&shell::apps_id()));
        assert!(shell.apps.opened(), "becoming visible by expanding still starts the service");
    }

    /// 点到「应用」面板会触发探测任务(`tokio::spawn`),所以需要运行时。
    #[tokio::test]
    async fn the_view_builds_in_the_states_a_user_can_reach() {
        let mut shell = fresh("view");
        let _ = Product::view(&shell);
        for panel in shell::all_panels() {
            press(&mut shell, &panel);
            let _ = Product::view(&shell);
        }
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Left))));
        let _ = Product::view(&shell);
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        let _ = Product::view(&shell);
    }

    /// 复审 #9:损坏账本必须显式 `Unavailable` + **不接线**生产者,而不是退回临时空账本。
    #[tokio::test]
    async fn a_corrupt_usage_ledger_is_unavailable_and_left_untouched() {
        use dozer_services::ServiceErrorKind;

        let dir = test_temp_dir("usage-corrupt");
        let path = dir.join("usage_ledger.json");
        let original = b"{ this is not json";
        std::fs::write(&path, original).unwrap();

        let (service, ledger, note) = load_usage_ledger(path.clone());
        assert!(ledger.is_none(), "损坏时不得接线任何账本(含临时空账本)");
        assert!(note.contains("暂不可用"), "boot 提示应说明不可用:{note}");
        let err = service
            .usage_summary("/p/a".into(), None)
            .await
            .expect_err("损坏账本查询必须 Unavailable,而不是 Ok(空)");
        assert_eq!(err.kind, ServiceErrorKind::Unavailable);

        // 原文件保持原样(未被覆盖/未被悄悄迁移)。
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 复盘发现 6:真实 `ProductShell` 的 Surface 事件经 `update_host` 单次路由,不得无限递归。
    /// `Created`/`Resumed` → 一次 `on_activated`;`Destroyed` → 一次 `on_closed`;`Hidden` → 无回调。
    /// 修复前本用例会因 `route_lifecycle` → `update` → `route_lifecycle` 递归至栈溢出。
    #[test]
    fn surface_events_route_once_without_recursion() {
        let slot = bytehost_desktop::SurfaceSlot(7);
        let mut shell = ProductShell::new();

        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Created 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Resumed { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Resumed 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Hidden { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Hidden 不触发生命周期回调"
        );

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Destroyed { slot }),
        );
        assert_eq!(
            shell.surface_callbacks,
            vec![slot],
            "Destroyed 触发一次关闭"
        );

        // 直接投 Surface 给 `Product::update` 也不得递归(它只消费,不再重投)。
        shell.surface_callbacks.clear();
        let _ = Product::update(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Surface 只在 update_host 路由一次,Product::update 不再分发"
        );
    }
}
