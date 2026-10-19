//! Digger 产品壳:在共享 bytehost 宿主上启动的产品组合根。
//!
//! 窗口结构(顶栏 / 左右图标栏 / 左右面板区 / 分隔线 / 放大态 / 布局持久化)来自共享骨架 `bytehost-chrome`,与 Dozer 的
//! 窗口结构同源;本文件只做产品侧的事:注册面板、实现 `ShellHost`、把骨架消息接到各面板的切入动作和原生 webview 的
//! 同步上。拥有自己的数据根、主题、布局落盘与退出,**不复制** Dozer 的 `App`/`Workspace`。
//!
//! 范围(S1):以**无 Project** 状态启动;顶栏的项目页签行保留结构、内容留空,"＋"置灰;**不伪造**任何写入能力。

use std::path::{Path, PathBuf};

use bytehost_chrome::persist::SCHEMA_VERSION;
use bytehost_chrome::{
    BodyArea, Divider, El, Loaded, PairRects, PanelMeta, PanelRules, Rect, ShellHost, ShellModel,
    ShellMsg, TopBarSlots, metrics_from_theme, pair_view,
};
use bytehost_core::registry::PanelCatalog;
use bytehost_core::{LogicalRect, PanelId, PanelMeta as CatalogEntry};
use bytehost_desktop::{HostMessage, Product, route_lifecycle};
use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use iced_widget::core::Border;

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
    /// 「文件」面板的库消息(共享 `dozer-panel-files` 库)。
    Files(dozer_panel_files::Message),
    /// 文件系统监听:当前项目的工作区有增删改,文件树与 git 着色要跟进。
    FilesFsChanged(crate::adapters::files::FsChange),
    /// 关掉「文件」面板的右键菜单 / 移动卡片 / 删除确认(点空白 / Esc)。
    FilesMenuDismiss,
    /// 「编辑器」面板的消息(ADR-013)。
    Markdown(crate::markdown::Message),
    /// 项目页签与注册表(顶栏页签行)。
    Projects(crate::projects::Message),
    /// 每帧焦点探针:`view` 刚渲染完(库的 `text_input` 焦点态已写回),`id` 是渲染时聚焦的项目。
    ProjectFocusFrame(i64),
    /// 「项目」面板的库消息(共享 `dozer-panel-project` 库)。
    /// 库消息不带项目 id;路由到哪个项目见 `ProjectPanel::resolve_project`。
    Project(dozer_panel_project::Message),
    /// 一次「删除项目」后台结果:(项目 id, 是否已从注册表移除, 文件系统步骤的错误)。
    ProjectDeleteDone {
        project_id: i64,
        deregistered: bool,
        errors: Vec<String>,
    },
    /// 一次「选择文档链接文件/目录」的返回(库的 `Pick` 由宿主落地)。
    ProjectLinkPicked(dozer_panel_project::links::LinkTarget, Option<PathBuf>),
    /// 面板上按下鼠标:编辑框此刻若还在编辑态,就是失焦了(iced 不报焦点事件,
    /// 只能由"点别处"触发落盘)。
    ProjectPointerDown,
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
    /// 非 `None` 表示全局 `shell.json` 是**不支持的高版本**写的:内存默认布局仅用于显示,**禁止回写**该文件。
    /// 只在启动读全局文件时设定,换项目不会清掉它。
    shell_save_blocked: Option<String>,
    /// 同上,针对**当前项目**自己的布局文件;换项目时重新判定。
    project_shell_blocked: Option<String>,
    /// 「应用」面板的组合根(状态机 + 执行器 + 内嵌应用服务)。
    apps: crate::apps::AppsState,
    /// 「用量」面板适配(共享 `dozer-panel-usage` 库 + 注入的真实用量服务)。
    usage: crate::adapters::usage::UsagePanel,
    /// 「编辑器」面板:自包含的第一方面板(文本与协议在 `digger-editor`,这里只有宿主适配)。
    markdown: crate::markdown::MarkdownPanel,
    /// 项目注册表与已开页签(Project 是 host 层共享业务,ADR-014)。
    projects: crate::projects::ProjectsState,
    /// 「项目」面板适配(共享 `dozer-panel-project` 库 + 注入的项目/记忆服务)。
    project: crate::adapters::project::ProjectPanel,
    /// 「文件」面板适配(共享 `dozer-panel-files` 库;四项能力 Digger 一期全关)。
    files: crate::adapters::files::FilesPanel,
    /// 「删除项目」结果回流:库的后台任务在线程池里跑,结果经这里转成产品消息。
    delete_tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    /// 上面那条通道的接收端,`boot` 里取一次接成 iced 流(`None` = 已取走)。
    delete_rx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Msg>>>,
    /// 生命周期自检的驱动(只在环境变量设置时有)。
    lifecycle: Option<crate::lifecycle_selftest::Driver>,
    /// [应用, 用量] 面板上一次是否可见:变为可见时触发各自的切入动作;应用面板变为不可见时关掉正在显示的应用。
    /// 与 [`shell::all_panels`] 同序。
    visible_before: Vec<bool>,
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
    LogicalRect {
        x: r.x as f64,
        y: r.y as f64,
        width: r.w as f64,
        height: r.h as f64,
    }
}

/// 项目布局文件:`<root>/shell-projects/<id>.json`。生产里 `root` 是配置目录(`<config>/digger/shell-projects/`),
/// 测试里跟着全局 `shell.json` 走,免得测试互相覆盖、也不碰真实配置目录。
fn project_shell_path_of(root: &Path, id: i64) -> PathBuf {
    root.join("shell-projects").join(format!("{id}.json"))
}

/// 向下缩 `top` 像素(面板自己的头部占掉的高度)。
fn below_header(r: Rect, top: f32) -> Rect {
    Rect::new(r.x, r.y + top, r.w, (r.h - top).max(1.0))
}

/// 用量面板此刻怎么摆(Dozer `PanelKind::Usage` 分支的三态)。**`body()` 与 `sync_native()` 共用这一个判断**,
/// 谁也不自己再算一遍,免得 body 已经是单栏、图表还停在分栏的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageLayout {
    /// 加载中 / 还没数据:没有列表可拼,内容栏独占全宽。
    ContentOnly,
    /// 有数据、列表被收起:内容栏独占全宽,标题行里留"展开"按钮。
    Collapsed,
    /// 有数据且列表展开:内容 | 列表。
    Paired,
}

/// 用量的原生图表只放在内容栏里:拼分栏时是配对的第二块(镜像与否由骨架几何给出),其余两种状态是整个面板内容区。
fn usage_native_rect(layout: UsageLayout, content: Rect, pair: Option<PairRects>) -> Rect {
    match (layout, pair) {
        (UsageLayout::Paired, Some(p)) => p.second,
        _ => content,
    }
}

impl ProductShell {
    /// 构造:注册产品面板目录、读布局(缺省用默认布局),不打开任何 Project。
    pub fn new() -> Self {
        Self::with_shell_path(paths::shell_path())
    }

    pub(crate) fn with_shell_path(shell_path: PathBuf) -> Self {
        let mut catalog = PanelCatalog::new();
        for (id, title) in [
            (shell::info_id(), "Digger"),
            (shell::apps_id(), "应用"),
            (shell::usage_id(), "用量"),
            (shell::project_id(), "项目"),
            (shell::files_id(), "文件"),
        ] {
            catalog
                .register(CatalogEntry {
                    id,
                    title: title.into(),
                    surfaces: vec![],
                })
                .expect("首次注册产品面板");
        }
        catalog
            .register(CatalogEntry::single_surface(shell::markdown_id(), "编辑器"))
            .expect("首次注册 digger.markdown");

        // 启动时读全局布局(项目布局只在切到项目页签时换上,见 `Msg::Projects`)。
        let loaded = shell::load(&shell_path);
        let shell_save_blocked = match &loaded {
            Loaded::Newer { found, .. } => Some(format!(
                "布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**"
            )),
            _ => None,
        };
        let mut model = loaded.into_model();

        // 任务完成用量账本(Task 20):生产者(应用操作)写、用量面板读,同一实例。
        // 载入失败(损坏)不 panic,也**不**退回一次性临时空账本(复审 #9)——后者会把"读不出来"伪装成 "Ok(空)",且新记录
        // 写进下次启动就不再引用的临时文件,等于静默丢。改为:查询恒返回 Unavailable、**不接线**用量生产者(没有记录悄悄
        // 写走),原损坏文件保持原样。
        let scope_path = paths::config_dir();
        let (usage_service, usage_ledger, boot_note) =
            load_usage_ledger(paths::usage_ledger_path());

        let mut apps = crate::apps::AppsState::new();
        // 只有账本可用时才接用量生产者;损坏时**不接**,避免记录写进无引用的临时文件。
        if let Some(ledger) = &usage_ledger {
            apps.attach_usage(ledger.clone(), scope_path.to_string_lossy().to_string());
        }
        let usage = crate::adapters::usage::UsagePanel::new(usage_service, scope_path.clone());
        let markdown = crate::markdown::MarkdownPanel::new();
        // 项目数据与布局同在一个数据目录里:生产是产品数据根,测试是各自的临时目录。
        let data_dir = shell_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(paths::config_dir);
        let projects = crate::projects::ProjectsState::open_in(&data_dir);
        let project = crate::adapters::project::ProjectPanel::new(projects.project_host());
        let files = crate::adapters::files::FilesPanel::new(
            data_dir.join(dozer_panel_files::external_apps::CONFIG_FILE),
        );
        let (delete_tx, delete_rx) = tokio::sync::mpsc::unbounded_channel();
        let delete_rx = std::sync::Mutex::new(Some(delete_rx));
        let lifecycle = crate::lifecycle_selftest::Driver::from_env();
        // 自检模式需要编辑器一开始就看得见。
        if markdown.is_selftest() || lifecycle.is_some() {
            shell::update(&mut model, ShellMsg::ShowPanel(shell::markdown_id()));
        }

        let mut shell = Self {
            window: None,
            catalog,
            shell: model,
            shell_path,
            shell_save_blocked,
            project_shell_blocked: None,
            apps,
            usage,
            markdown,
            projects,
            project,
            files,
            delete_tx,
            delete_rx,
            lifecycle,
            visible_before: Vec::new(),
            boot_note,
            closing: false,
            shutdown_started: false,
            shutdown_error: None,
            #[cfg(test)]
            surface_callbacks: Vec::new(),
        };
        // 构造时就记下此刻谁可见:否则第一条消息会把本来就看得见的面板当成"刚变可见",重复触发切入动作
        // (启动时本来就可见的面板由 `boot` 显式触发一次,不靠这里)。
        shell.visible_before = visible_panels(&shell);
        shell
    }

    /// 落盘当前骨架状态(退出 / 布局变化时调用)。失败不 panic,记入 boot_note。
    /// 磁盘上是**不支持的高版本**布局时**拒绝回写**(复盘发现 4:绝不覆盖原文件)。
    ///
    /// 有当前项目时布局写到该项目自己的文件,窗口大小仍写全局 `shell.json`;没有项目时整份写全局。
    fn save_shell(&mut self) {
        let project = self.current_project_shell_path();
        self.save_shell_scoped(project);
    }

    /// 把内存里的布局写到指定项目的文件(`None` = 整份写全局)。切页签时用它写**旧**项目,
    /// 因为此刻 `projects` 里的当前项目已经是新的。
    fn save_shell_for(&mut self, project: Option<i64>) {
        let path = project.and_then(|id| self.project_shell_path_for(id));
        self.save_shell_scoped(path);
    }

    fn save_shell_scoped(&mut self, project_path: Option<PathBuf>) {
        let Some(project_path) = project_path else {
            if let Some(note) = &self.shell_save_blocked {
                self.boot_note = format!("布局未保存:{note}");
                return;
            }
            if let Err(e) = bytehost_chrome::persist::save_atomic(&self.shell_path, &self.shell) {
                self.boot_note = format!("布局保存失败:{e}");
            }
            return;
        };
        if let Some(note) = &self.project_shell_blocked {
            self.boot_note = format!("布局未保存:{note}");
        } else if let Err(e) = bytehost_chrome::persist::save_atomic(&project_path, &self.shell) {
            self.boot_note = format!("布局保存失败:{e}");
        }
        // 窗口大小是窗口的属性,不是项目的:只有全局那份决定下次启动的窗口尺寸,且**只改尺寸**,
        // 其余字段(旧全局布局,新项目的初始值)字节不动。
        if self.shell_save_blocked.is_none()
            && let Err(e) = shell::save_window_size(&self.shell_path, &self.shell)
        {
            self.boot_note = format!("窗口大小未保存:{e}");
        }
    }

    /// 切到当前项目后换布局:窗口大小沿用内存里的(窗口没变),其余读该项目自己的文件;
    /// 没有项目或项目没有文件时读全局布局。
    fn load_project_shell(&mut self) {
        let project = self.current_project_shell_path().filter(|p| p.exists());
        // 项目文件是更高 schema 版本写的:照骨架的规矩用默认布局显示,但**不回写**它。
        self.project_shell_blocked = project
            .as_deref()
            .and_then(|p| self.project_save_blocked(p));
        let window = self.shell.window();
        let mut model = shell::load_scoped(&self.shell_path, project.as_deref()).into_model();
        if model.window() != window {
            shell::update(
                &mut model,
                ShellMsg::Resized {
                    w: window.0,
                    h: window.1,
                },
            );
        }
        self.shell = model;
        // 换布局可能让某些面板从看不见变成看得见:在新布局上重算一次,别拿旧布局的可见性比较。
        self.visible_before = visible_panels(self);
    }

    /// 当前项目自己的布局文件;没有当前项目时为 `None`(整份写全局)。
    fn current_project_shell_path(&self) -> Option<PathBuf> {
        let id = self.projects.current_project_id()?;
        self.project_shell_path_for(id)
    }

    /// 某个项目的布局文件(与它是否当前项目无关)。
    fn project_shell_path_for(&self, id: i64) -> Option<PathBuf> {
        let root = self
            .shell_path
            .parent()
            .filter(|p| p != &paths::config_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(paths::config_dir);
        Some(project_shell_path_of(&root, id))
    }

    /// 该项目的布局文件若是更高 schema 版本写的,给它一条说明;调用方据此拒绝回写。
    fn project_save_blocked(&self, project_path: &Path) -> Option<String> {
        match shell::load_scoped(&self.shell_path, Some(project_path)) {
            Loaded::Newer { found, .. } => Some(format!(
                "项目布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**"
            )),
            _ => None,
        }
    }

    /// 面板此刻是否真的呈现在用户眼前:它是所在栏的选中面板、所在区没收起、也没被另一侧的放大态盖住。
    pub(crate) fn panel_visible(&self, panel: &PanelId) -> bool {
        let Some(side) = self.shell.side_of(panel) else {
            return false;
        };
        self.shell.selected(side) == panel
            && !self.shell.collapsed(side)
            && self.shell.maximized().is_none_or(|m| m == side)
    }

    /// 用量面板此刻的三态(唯一判断,见 [`UsageLayout`])。
    pub(crate) fn usage_layout(&self) -> UsageLayout {
        if !self.usage.has_agent_filter() {
            return UsageLayout::ContentOnly;
        }
        if self.pane_collapsed(&shell::usage_id()) {
            UsageLayout::Collapsed
        } else {
            UsageLayout::Paired
        }
    }

    /// 配对面板的第一块此刻是否被收起(骨架的 `PaneStore` 记账)。
    fn pane_collapsed(&self, panel: &PanelId) -> bool {
        self.shell.panes().get(panel).is_some_and(|d| d.collapsed)
    }

    /// 面板刚被切入(或启动时本来就可见):触发各自的"切入时动作"。
    fn on_visible(&mut self, panel: &PanelId) -> Task<Msg> {
        if *panel == shell::apps_id() {
            // 首次进入「应用」面板:触发一次探测 + 拉列表(Task 14)。
            return self
                .apps
                .update(crate::apps::Message::Opened)
                .map(Msg::Apps);
        }
        if *panel == shell::usage_id() {
            // 进入「用量」面板:触发一次取数(库的 on_activate,Task 20)。
            self.usage.activate();
        }
        if *panel == shell::project_id() {
            // 进入「项目」面板:当前项目的信息/文档/记忆一次拉齐(库的 on_activate)。
            self.activate_project_panel();
        }
        if *panel == shell::files_id() {
            // 进入「文件」面板:把树根挂到当前项目并拉一次 git 文件状态。
            self.activate_files_panel();
        }
        Task::none()
    }

    /// 当前项目从 `before` 换成了现在这个(页签点击 / 关闭 / 删除 / 列表修剪都走这里):
    /// 把**旧**项目的布局落到它自己的文件(`save_old`,已删除的项目不写),换上新项目的布局,
    /// 并让 Project 面板跟到新项目。当前项目没变时什么都不做。
    fn on_current_project_changed(&mut self, before: Option<i64>, save_old: bool) {
        if before == self.projects.current_project_id() {
            return;
        }
        if save_old && before.is_some() {
            self.save_shell_for(before);
        }
        self.load_project_shell();
        if self.panel_visible(&shell::project_id()) {
            self.activate_project_panel();
        }
        self.activate_files_panel();
    }

    /// 把 Project 面板切到当前项目(切入面板 / 换项目页签时都走这里)。没有项目时什么都不做。
    fn activate_project_panel(&mut self) {
        let Some(project) = self.projects.current_project().cloned() else {
            return;
        };
        self.project.activate(&project);
    }

    /// 把「文件」面板的树根切到当前项目。没有项目时清空。
    fn activate_files_panel(&mut self) {
        let project = self.projects.current_project().cloned();
        self.files.set_project(project.as_ref());
    }

    /// 「删除项目」二次确认卡片点"删除":先取走待确认的层级(库的 `delete_pending`),
    /// 关掉产品页签,再依次 delist + 删文件系统(库的 `spawn_delete_project`)。
    /// 与 Dozer 的 `project_delete_confirm` 同序:关页签必须在发起删除之前。
    fn project_delete_confirm(&mut self, project: &dozer_model::ProjectInfo) {
        let Some(scope) = self.project.take_delete_pending(project.id) else {
            return;
        };
        let before = self.projects.current_project_id();
        let close = self
            .projects
            .update(crate::projects::Message::Close(project.id));
        let _ = close;
        self.on_current_project_changed(before, before != Some(project.id));
        let (project, tx) = (project.clone(), self.delete_tx.clone());
        self.project
            .delete_project(project, scope, move |project_id, deregistered, errors| {
                let _ = tx.send(Msg::ProjectDeleteDone {
                    project_id,
                    deregistered,
                    errors,
                });
            });
    }

    /// 排空 Project 面板待发提示。Digger 一期没有 Toast:落到 stderr,不静默吞掉。
    fn flush_project_notices(&mut self) {
        let Some(project_id) = self.project.focused_project() else {
            return;
        };
        for (level, text) in self.project.take_notices(project_id) {
            eprintln!("digger: project 面板/{level}: {text}"); // cli-output
        }
    }

    fn on_shell(&mut self, msg: ShellMsg) -> Task<Msg> {
        if self.closing {
            return Task::none();
        }
        let resized = matches!(msg, ShellMsg::Resized { .. });
        if let ShellMsg::CursorMoved { x, y } = msg {
            // 文件树的拖拽幽灵胶囊要跟着光标走(展示语义读这两个值)。
            let size = (self.shell.window().0, self.shell.window().1);
            self.files.set_window_probe(size, (x, y));
        }
        let out = shell::update(&mut self.shell, msg);
        if out.persist {
            self.save_shell();
        }
        let mut tasks = Vec::new();
        // "切入时动作"挂在"从看不见变成看得见"上,而不只是"被切入":展开收起的区、退出放大态、跨栏搬走对侧面板后
        // 本栏退回的面板,都没有 `switched_in`,但用户眼前出现了这个面板。动作本身幂等,与 `switched_in` 重叠无害。
        let visible_now = visible_panels(self);
        let mut entered: Vec<PanelId> = out.switched_in.into_iter().collect();
        for (panel, (now, was)) in shell::all_panels()
            .into_iter()
            .zip(visible_now.iter().copied().zip(self.visible_before.clone()))
        {
            if now && !was && !entered.contains(&panel) {
                entered.push(panel);
            }
        }
        for panel in entered {
            tasks.push(self.on_visible(&panel));
        }
        // 应用面板从可见变成不可见(切走 / 收起 / 被放大盖住):关掉正在显示的应用,别留一个看不见的 webview 盖在别处。
        let apps = shell::all_panels()
            .iter()
            .position(|p| *p == shell::apps_id());
        if apps.is_some_and(|i| {
            self.visible_before.get(i).copied().unwrap_or(false) && !visible_now[i]
        }) {
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
            return window::run(id, move |w| {
                bytehost_chrome::platform_macos::center_traffic_lights(w, top)
            })
            .discard();
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
        let usage_rect = content_of(&shell::usage_id()).map(|content| {
            let pair = self
                .shell
                .side_of(&shell::usage_id())
                .and_then(|side| shell::pair_rects(&self.shell, side));
            usage_native_rect(self.usage_layout(), content, pair)
        });
        let markdown_rect = content_of(&shell::markdown_id());
        let hidden = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let closing = self.closing;

        let apps_task = match apps_rect {
            Some(r) if !closing => self
                .apps
                .sync_native(window, logical(below_header(r, APPS_HEADER_HEIGHT)))
                .map(Msg::Apps),
            // 关闭时由 `apps.prepare_shutdown` 收尾;不可见时 `on_shell` 已 `close_selected`,这里仍要让待办的原生动作跑完。
            _ => self.apps.sync_native(window, hidden).map(Msg::Apps),
        };
        let usage_task = self
            .usage
            .sync_native(
                window,
                usage_rect.is_some() && !closing,
                usage_rect
                    .map(|r| logical(below_header(r, USAGE_HEADER_HEIGHT)))
                    .unwrap_or(hidden),
            )
            .map(Msg::NativeUsage);
        // 关闭时销毁编辑器 webview 并丢掉文档;否则只在可见时显示,切走则隐藏。
        let markdown_task = if closing {
            self.markdown.close(window)
        } else {
            self.markdown.sync_native(
                window,
                markdown_rect.is_some(),
                markdown_rect.map(logical).unwrap_or(hidden),
            )
        }
        .map(Msg::Markdown);
        Task::batch([apps_task, usage_task, markdown_task])
    }

    fn info_view(&self) -> Element<'_, Msg> {
        let tokens = color::current();
        let dim = tokens.dim;
        container(
            column![
                text(format!(
                    "面板目录已注册 {} 个(产品组合根)",
                    self.catalog.len()
                ))
                .color(tokens.body),
                text(format!("数据根:{}", paths::config_dir().display())).color(dim),
                text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
                text(match self.projects.current_project() {
                    Some(p) => format!("当前 Project:{}({})", p.name, p.path),
                    None => "当前 Project:无".to_string(),
                })
                .color(tokens.body),
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
        container(self.apps.view().map(Msg::Apps))
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn usage_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let tokens = color::current();
        let body: Element<'_, Msg> = match self.usage_layout() {
            UsageLayout::ContentOnly => self
                .usage
                .content_view(self.pane_collapsed(&shell::usage_id())),
            UsageLayout::Collapsed => self.usage.content_view(true),
            // 列表是配对的第一块;"内容在前还是列表在前"由骨架按镜像算好,`pair_view` 直接读。
            UsageLayout::Paired => pair_view(
                area,
                self.usage.list_view(),
                self.usage.content_view(false),
                Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
                tokens.bg,
                tokens.panel,
            ),
        };
        container(body)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn project_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let current = self.projects.current_project();
        // 整个面板区域接一记"按下":编辑中点到别处就提交(见 `Msg::ProjectPointerDown`)。
        let body: Element<'_, Msg> = iced::widget::mouse_area(self.project.view(
            current,
            Length::Fixed(area.rect.w),
            Border::default(),
        ))
        .on_press(Msg::ProjectPointerDown)
        .into();
        // 删除确认是覆盖在面板上的三选一卡片(库的 `project_delete_card`),有才盖。
        let overlay: Option<Element<'_, Msg>> = current
            .and_then(|p| self.project.delete_card(p.id))
            .map(|card| {
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .into()
            });
        match overlay {
            Some(card) => iced::widget::stack![body, card].into(),
            None => body,
        }
    }

    fn files_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        self.files
            .view(Length::Fixed(area.rect.w), Border::default())
    }

    fn markdown_view(&self) -> Element<'_, Msg> {
        // 条在上;下面留空,原生 webview 由 `sync_native` 摆在条的下方。
        column![
            self.markdown.view().map(Msg::Markdown),
            iced::widget::Space::new().height(Length::Fill)
        ]
        .into()
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

    /// 取走「删除项目」结果的接收端(只能取一次;`boot` 用它接成 iced 流)。
    pub fn take_delete_receiver(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Msg>> {
        self.delete_rx.lock().ok().and_then(|mut r| r.take())
    }

    pub fn usage_ready(&self) -> bool {
        self.usage.native.is_ready()
    }

    pub fn usage_last_probe(&self) -> Option<crate::adapters::usage_native::Probe> {
        self.usage.native.last_probe()
    }

    pub fn usage_probe(&self, id: window::Id) -> Task<Msg> {
        self.usage.native.probe(id).map(Msg::NativeUsage)
    }
}

/// 顶栏三个槽位:标题只是展示;页签行与"＋"由项目注册表状态(`projects`)画。
fn top_bar_slots<'a>(projects: &'a crate::projects::ProjectsState) -> TopBarSlots<'a, Msg> {
    let tokens = color::current();
    let dim = tokens.dim;
    let title = row![
        icons::view(IconKind::Home, icon_size::home(), dim),
        text("Digger").size(font::body()).color(dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let tabs = projects.tabs_view().map(Msg::Projects);
    TopBarSlots {
        title: title.into(),
        tabs,
        right: iced::widget::Space::new().into(),
    }
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

    fn body<'a>(&'a self, panel: &PanelId, area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(area),
            "digger.markdown" => self.markdown_view(),
            "digger.project" => self.project_view(area),
            "digger.files" => self.files_view(area),
            _ => column![].into(),
        }
    }

    fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
        top_bar_slots(&self.projects)
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

/// 用系统默认应用打开一个路径(文档链接)。失败只记日志:用户还能从别的入口打开。
fn open_in_system(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    if let Err(e) = std::process::Command::new(opener).arg(path).spawn() {
        eprintln!("digger: 打开 {} 失败: {e}", path.display()); // cli-output
    }
}

/// 每个已注册面板此刻的可见性(下标与 [`shell::all_panels`] 对齐)。
fn visible_panels(shell: &ProductShell) -> Vec<bool> {
    shell::all_panels()
        .iter()
        .map(|p| shell.panel_visible(p))
        .collect()
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
                let resized = self.on_shell(ShellMsg::Resized {
                    w: size.width,
                    h: size.height,
                });
                // macOS:全屏期间把红绿灯临时挂到内容视图(原生全屏不绘制标题栏,红绿灯会跟着移出窗口)。
                #[cfg(target_os = "macos")]
                let guard = window::run(id, |w| {
                    // 顶栏拖窗保护:悬停页签/"＋"时 (`projects` 里的悬停状态) 放行按下给 iced,
                    // 悬停在空白顶栏时保留原生拖窗。
                    bytehost_chrome::platform_macos::install_topbar_drag_guard(w);
                    bytehost_chrome::platform_macos::install_fullscreen_guard(w)
                })
                .discard();
                #[cfg(not(target_os = "macos"))]
                let guard = Task::none();
                Task::batch([resized, guard])
            }
            Msg::Shell(m) => self.on_shell(m),
            Msg::ZoomToggle => self
                .window
                .map(window::toggle_maximize)
                .unwrap_or_else(Task::none),
            Msg::Apps(m) => self.apps.update(m).map(Msg::Apps),
            Msg::Usage(m) => {
                // 库的 `update` 对这两条是 `unreachable!`(它们必须由宿主拦截):折叠转给骨架,悬停无事可做。
                match m {
                    dozer_panel_usage::Message::ToggleListCollapse => {
                        return self.on_shell(ShellMsg::TogglePane(shell::usage_id()));
                    }
                    dozer_panel_usage::Message::CollapseHover(_) => return Task::none(),
                    m => self.usage.update(m),
                }
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
            Msg::Files(m) => {
                let project_id = self.projects.current_project_id();
                let task = self.files.update(m, project_id);
                // 库把"只有产品做得到"的事放进队列:右键菜单落到 iced 浮层,
                // 打开文件交给系统默认应用;选目录暂时无入口(能力已关)。
                let _ = self.files.take_requests();
                for request in self.files.take_host_requests() {
                    let _ = self.files.handle_host_request(&request);
                }
                // Digger 一期无 Toast:提示落到 stderr,不静默吞掉。
                for notice in self.files.take_notices() {
                    eprintln!(
                        "digger: files 面板/{:?}: {}",
                        notice.level, notice.text.fallback
                    ); // cli-output
                }
                task
            }
            Msg::FilesMenuDismiss => {
                let project_id = self.projects.current_project_id();
                self.files.dismiss(project_id)
            }
            Msg::FilesFsChanged(change) => {
                if Some(change.project_id) == self.projects.current_project_id() {
                    self.files.fs_changed(&change)
                } else {
                    Task::none()
                }
            }
            Msg::Markdown(m) => self.markdown.update(m).map(Msg::Markdown),
            Msg::Projects(m) => {
                // 打开/切换项目:若「项目」面板此刻就在眼前,立刻把它切到新项目
                // (否则要等下一次切入才拉数据——用户刚打开的项目看着是空的)。
                let before = self.projects.current_project_id();
                let task = self.projects.update(m).map(Msg::Projects);
                self.on_current_project_changed(before, true);
                task
            }
            Msg::ProjectFocusFrame(id) => {
                let Some(project) = self
                    .projects
                    .current_project()
                    .filter(|p| p.id == id)
                    .cloned()
                else {
                    return Task::none();
                };
                // 库在 `view` 之后把 `text_input` 的真焦点态写进全局锁,这里取走并记账;
                // 从真变假那一刻提交改名/描述(与 Dozer 的 `set_project_name_focused` 同义)。
                let now = dozer_panel_project::take_name_edit_focused();
                let was = self.project.name_edit_was_focused(project.id);
                self.project.set_name_edit_focused(project.id, now);
                if was && !now {
                    self.project.blur_editors(&project);
                }
                Task::none()
            }
            Msg::Project(m) => {
                // 改名结果自带项目 id(不是异步 token):不依赖"当前聚焦项目",也不要求它还是当前页签。
                if let dozer_panel_project::Message::NameRenamed(id, Ok(updated)) = &m {
                    let same = self.project.focused_project() == Some(*id);
                    self.projects.replace_registered(updated.clone());
                    if same && self.panel_visible(&shell::project_id()) {
                        self.activate_project_panel();
                    }
                    return Task::none();
                }
                let Some(project) = self.project.resolve_project(&m).and_then(|id| {
                    self.projects
                        .current_project()
                        .filter(|p| p.id == id)
                        .cloned()
                }) else {
                    return Task::none();
                };
                use dozer_panel_project::Message as PM;
                match m {
                    // 悬停动画:Digger 不挂 hover 动画表,按"未悬停"画,消息直接丢掉。
                    PM::ToolbarHover(..) => {}
                    // 文本编辑框的复制/粘贴菜单:一期不做(与其它面板一致)。
                    PM::TextInputMenuOpen(..) => {}
                    // 没有预览栏(S5b):点文档用系统默认应用打开。
                    PM::OpenLink(path) => open_in_system(&path),
                    // 右键菜单:一期只有"移除",不做浮层,直接落到库的 `LinkRemove`。
                    PM::LinkContextMenu { target, index } => self
                        .project
                        .update(Some(&project), PM::LinkRemove { target, index }),
                    PM::RepairProject => self.project.repair(&project),
                    PM::DeleteProjectConfirm => {
                        self.project_delete_confirm(&project);
                        return Task::none();
                    }
                    PM::Pick(target) => {
                        return Task::perform(
                            async move {
                                let picked = rfd::AsyncFileDialog::new()
                                    .set_title("选择项目文档")
                                    .set_directory(&project.path)
                                    .pick_file()
                                    .await
                                    .map(|h| h.path().to_path_buf());
                                Msg::ProjectLinkPicked(target, picked)
                            },
                            |m| m,
                        );
                    }
                    m => self.project.update(Some(&project), m),
                }
                self.flush_project_notices();
                Task::none()
            }
            Msg::ProjectLinkPicked(target, Some(path)) => {
                let Some(project) = self.projects.current_project().cloned() else {
                    return Task::none();
                };
                let kind = if path.is_dir() {
                    dozer_panel_project::links::LinkKind::Dir
                } else {
                    dozer_panel_project::links::LinkKind::File
                };
                self.project.update(
                    Some(&project),
                    dozer_panel_project::Message::LinkAdd { target, path, kind },
                );
                self.flush_project_notices();
                Task::none()
            }
            Msg::ProjectLinkPicked(_, None) => Task::none(),
            Msg::ProjectPointerDown => {
                let Some(project) = self.projects.current_project().cloned() else {
                    return Task::none();
                };
                if self.project.is_editing(project.id) {
                    self.project.blur_editors(&project);
                    self.flush_project_notices();
                }
                Task::none()
            }
            Msg::ProjectDeleteDone {
                project_id,
                deregistered,
                errors,
            } => {
                for e in &errors {
                    eprintln!("digger: 删除项目 {project_id}: {e}"); // cli-output
                }
                // 注册表已移除 → 页面上的页签也一并关掉(库只碰文件系统,不碰产品页签)。
                if deregistered {
                    let before = self.projects.current_project_id();
                    let close = self
                        .projects
                        .update(crate::projects::Message::Close(project_id));
                    self.on_current_project_changed(before, before != Some(project_id));
                    // 被删项目的布局文件不留孤儿。
                    if let Some(path) = self.project_shell_path_for(project_id) {
                        let _ = std::fs::remove_file(path);
                    }
                    return Task::batch([
                        close.map(Msg::Projects),
                        self.projects.refresh().map(Msg::Projects),
                    ]);
                }
                Task::none()
            }
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
                body = body
                    .push(text(error))
                    .push(button("强制退出").on_press(Msg::ForceExit));
            }
            return body.into();
        }
        // 名称编辑框的"失焦保存"钩子:库的 `text_input` 焦点态只能借 Operation 在渲染时读到,
        // Digger 没有逐帧原生循环,于是每帧派一个一次性消息,由 `update` 在两帧之间读回
        // (同 Dozer 的 `CaptureNameEditFocus`,只是搭在 "消息→重绘→消息" 的循环上)。
        let body: Element<'_, Msg> =
            bytehost_chrome::view(&self.shell, self, &metrics_from_theme());
        // 「修复项目」进度卡:进行中盖住整个窗口(不可关闭),全部完成后库才放行 `ScaffoldPopupClose`。
        // Dozer 用独立原生窗口承载,Digger 用同一窗口里的整窗覆盖层——卡片本体是库的同一份。
        let card: Option<Element<'_, Msg>> = self
            .project
            .focused_project()
            .and_then(|id| self.project.scaffold_card(id))
            .map(|card| {
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .padding(24)
                    .into()
            });
        let body = match card {
            Some(card) => iced::widget::stack![body, card].into(),
            None => body,
        };
        // 「文件」面板的对话框(删除确认 / 移动重命名):居中卡片 + 半透明遮罩,
        // 压在右键菜单之下、正文之上。
        let body = if self.panel_visible(&shell::files_id()) {
            let dialog = self.files.delete_confirm().map(|spec| {
                let spec = byteui::feedback::dialog::ConfirmDialog {
                    icon: spec.icon,
                    title: spec.title,
                    description: spec.description,
                    cancel_label: spec.cancel_label,
                    cancel_msg: Msg::Files(spec.cancel_msg),
                    confirm_label: spec.confirm_label,
                    confirm_msg: Msg::Files(spec.confirm_msg),
                    close_msg: None,
                    confirm_color: spec.confirm_color,
                    content_spacing: spec.content_spacing,
                };
                centered_modal(
                    byteui::feedback::dialog::confirm(spec),
                    Msg::FilesMenuDismiss,
                )
            });
            let move_card = self
                .files
                .move_card()
                .map(|card| centered_modal(card, Msg::FilesMenuDismiss));
            match dialog.or(move_card) {
                Some(card) => iced::widget::stack![body, card].into(),
                None => body,
            }
        } else {
            body
        };
        // 「文件」面板的右键菜单:盖在最上层,点空白或 Esc 关闭。
        match self.files.menu() {
            Some(menu) if self.panel_visible(&shell::files_id()) => {
                let window = (self.shell.window().0, self.shell.window().1);
                iced::widget::stack![
                    body,
                    crate::adapters::menu_overlay::view(
                        menu,
                        window,
                        Msg::Files,
                        Msg::FilesMenuDismiss
                    )
                ]
                .into()
            }
            _ => body,
        }
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
        Task::batch([task, native])
            .chain(Task::perform(shell.apps.shutdown(), Msg::ShutdownFinished))
    } else {
        Task::batch([task, native])
    }
}

/// 名称编辑框焦点探针的订阅数据(项目 id 作 key:切项目就换一条订阅)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NameFocusProbe(i64);

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
    let mut subs = vec![
        events,
        iced::time::every(std::time::Duration::from_secs(1)).map(|_| Msg::Tick),
    ];
    // 页签拖拽中:在页签行外松手也要结束拖拽(行内松手由页签行自己的 `on_release` 管),
    // 否则之后光标滑过页签行会接着重排并落盘。只在拖拽中订阅。
    if shell.projects.is_dragging() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left
                ))
            )
            .then_some(Msg::Projects(crate::projects::Message::DragEnd))
        }));
    }
    // 名称编辑框的真焦点态只有渲染时(Operation)读得到:用一个 16ms 的订阅在帧与帧之间
    // 把它取回来(同 Dozer 的 `CaptureNameEditFocus`,这里借订阅当逐帧钩子)。
    if let Some(project) = shell.projects.current_project().map(|p| p.id)
        && shell.panel_visible(&shell::project_id())
    {
        subs.push(iced::Subscription::run_with(
            NameFocusProbe(project),
            |probe| {
                let project = probe.0;
                iced::stream::channel(
                    1,
                    move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                        use iced::futures::SinkExt;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                            if out.send(Msg::ProjectFocusFrame(project)).await.is_err() {
                                break;
                            }
                        }
                    },
                )
            },
        ));
    }
    // 右键菜单打开时订阅 Esc:关掉浮层(点空白走浮层自己的透明按下区)。
    if shell.files.has_modal() || shell.files.menu().is_some() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
            .then_some(Msg::FilesMenuDismiss)
        }));
    }
    if shell.shell.animating() {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| Msg::Shell(ShellMsg::Tick)),
        );
    }
    Subscription::batch(subs)
}

/// 面板内弹窗:整窗半透明遮罩 + 居中卡片。点遮罩关闭。
fn centered_modal<'a>(card: Element<'a, Msg>, on_dismiss: Msg) -> Element<'a, Msg> {
    let scrim: Element<'a, Msg> = iced_widget::mouse_area(
        container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_t: &iced_widget::Theme| container::Style {
                background: Some(
                    iced_widget::core::Color {
                        a: 0.35,
                        ..iced_widget::core::Color::BLACK
                    }
                    .into(),
                ),
                ..container::Style::default()
            }),
    )
    .on_press(on_dismiss)
    .into();
    let centered: Element<'a, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(24)
        .into();
    iced_widget::stack![scrim, centered].into()
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
    shell
        .project
        .attach_handle(tokio::runtime::Handle::current());
    shell.files.attach_handle(tokio::runtime::Handle::current());
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
        Some(id) => bytehost_desktop::panel::window_size(id)
            .map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    let native_usage = match shell.usage.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::NativeUsage(event), rx))
        })),
        None => Task::none(),
    };
    let markdown_task = match shell.markdown.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::Markdown(event), rx))
        })),
        None => Task::none(),
    };
    let lifecycle_task = if shell.lifecycle.is_some() {
        crate::lifecycle_selftest::Driver::first()
    } else {
        Task::none()
    };
    // 「项目」面板的后台结果(记忆列表 / git 快照 / 磁盘占用)回流成 iced 流。
    let project_task = match shell.project.take_receiver() {
        Some(rx) => Task::stream(crate::adapters::project::receiver_stream(rx).map(Msg::Project)),
        None => Task::none(),
    };
    // 「文件」面板的后台结果(git 文件状态)回流成 iced 流。
    let files_task = match shell.files.take_receiver() {
        Some(rx) => Task::stream(crate::adapters::files::receiver_stream(rx).map(Msg::Files)),
        None => Task::none(),
    };
    // 文件系统监听的事件回流(当前项目的增删改 → 树与 git 着色跟进)。
    let fs_task = match shell.files.take_fs_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|c| (Msg::FilesFsChanged(c), rx))
        })),
        None => Task::none(),
    };
    // 「删除项目」的后台结果回流(库的 `spawn_delete_project` 在结束回调里投递)。
    let delete_task = match shell.take_delete_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            let next = rx.recv().await?;
            Some((next, rx))
        })),
        None => Task::none(),
    };
    // 启动时就可见的面板(例如默认右栏选中的用量)也要触发各自的"切入时动作"。
    let mut initial = Vec::new();
    for panel in shell::all_panels() {
        if shell.panel_visible(&panel) {
            initial.push(shell.on_visible(&panel));
        }
    }
    shell.visible_before = visible_panels(&shell);
    let projects_task = shell.projects.refresh().map(Msg::Projects);
    (
        shell,
        Task::batch([
            projects_task,
            size_task,
            reply_task,
            usage_task,
            native_usage,
            markdown_task,
            project_task,
            files_task,
            fs_task,
            delete_task,
            lifecycle_task,
            Task::batch(initial),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;
    use bytehost_desktop::SurfaceEvent;

    fn test_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "digger-product-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 测试用:项目布局文件放在测试目录下(生产是 `<config>/shell-projects/`)。
    fn project_path_in(dir: &std::path::Path, id: i64) -> std::path::PathBuf {
        dir.join("shell-projects").join(format!("{id}.json"))
    }

    /// 带独立落盘路径的壳:互不干扰,也不碰真实配置目录。
    fn fresh(tag: &str) -> ProductShell {
        ProductShell::with_shell_path(test_temp_dir(tag).join("shell.json"))
    }

    fn press(shell: &mut ProductShell, panel: &PanelId) {
        let _ = Product::update(
            shell,
            HostMessage::Product(Msg::Shell(ShellMsg::PanelPress {
                panel: panel.clone(),
            })),
        );
    }

    #[test]
    fn starts_without_project() {
        let shell = ProductShell::new();
        assert!(shell.boot_note.contains("尚未打开 Project"));
    }

    #[test]
    fn a_project_opened_in_the_shell_is_current_and_comes_back_after_a_restart() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("project-restore");
        let proj = dir.join("book");
        std::fs::create_dir(&proj).unwrap();
        let info = dozer_model::ProjectInfo {
            id: 1,
            path: proj.to_string_lossy().into_owned(),
            name: "book".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        send(&mut shell, Msg::Projects(P::Opened(Ok(Some(info.clone())))));
        assert_eq!(shell.projects.current_project_id(), Some(1));

        let mut again = ProductShell::with_shell_path(dir.join("shell.json"));
        send(
            &mut again,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![info])))),
        );
        assert_eq!(again.projects.current_project_id(), Some(1));
    }

    /// 布局按项目一份(Task 9):切项目页签换布局,窗口大小仍写全局。
    #[test]
    fn switching_project_tabs_swaps_the_layout_and_keeps_the_window_global() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("project-scope");
        for name in ["a", "b"] {
            std::fs::create_dir(dir.join(name)).unwrap();
        }
        let info = |id: i64, name: &str| dozer_model::ProjectInfo {
            id,
            path: dir.join(name).to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: id as u64,
            created_ms: 1,
            updated_ms: 1,
        };
        // 线程复用会让这个目录带着上一轮的残留(页签、项目库、项目布局都算):构造壳之前全清掉,
        // 再写 B 的项目布局。`project_tabs.json` 里残留的"当前项目"会让壳一启动就切到那个项目。
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        std::fs::create_dir(dir.join("shell-projects")).unwrap();
        let mut b_saved = shell::defaults();
        b_saved.left_width = 400.0;
        let b_model = ShellModel::new(
            b_saved.rails.clone(),
            b_saved.selected.clone(),
            b_saved.left_width,
            b_saved.window,
        )
        .unwrap();
        bytehost_chrome::persist::save_atomic(&project_path_in(&dir, 2), &b_model).unwrap();
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        assert_eq!(shell.projects.current_project_id(), None);
        let a = info(1, "a");
        let b = info(2, "b");
        // `Opened` 的后续步骤是异步任务,测试里跑不到:直接用 `Listed` 建快照并打开这两个页签。
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                a.clone(),
                b.clone(),
            ])))),
        );
        // 两张页签都开,但只让 A 当当前项目:B 此刻不该被任何一次保存碰到。
        shell.projects.add_tab_for_test(a.id);
        shell.projects.add_tab_for_test(b.id);
        send(&mut shell, Msg::Projects(P::Activate(a.id)));
        assert_eq!(shell.projects.current_project_id(), Some(a.id));

        let a_path = project_path_in(&dir, a.id);
        let b_path = project_path_in(&dir, b.id);

        // 在 A 里把左区拉宽:落到 A 的项目文件,不碰 B 的。
        send(
            &mut shell,
            Msg::Shell(ShellMsg::DividerPress(Divider::Zones)),
        );
        send(
            &mut shell,
            Msg::Shell(ShellMsg::CursorMoved { x: 544.0, y: 300.0 }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert_eq!(shell.shell.left_width(), 500.0);
        assert!(a_path.exists(), "A's drag is written to A's file");
        let b_before = std::fs::read(&b_path).unwrap();

        // 全局文件仍存在(窗口大小写在那里)。
        assert!(matches!(
            shell::load(&dir.join("shell.json")),
            Loaded::Restored(_)
        ));

        // 切到 B(第一次把它变成当前项目):B 自己的布局(400)回来,不是 A 的 500。
        send(&mut shell, Msg::Projects(P::Activate(b.id)));
        assert_eq!(shell.shell.left_width(), 400.0, "B's layout, not A's");
        assert_eq!(
            std::fs::read(&b_path).unwrap(),
            b_before,
            "B untouched by A"
        );

        // 切回 A:500 仍在。
        send(&mut shell, Msg::Projects(P::Activate(a.id)));
        assert_eq!(shell.shell.left_width(), 500.0, "A's layout comes back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_project_the_layout_goes_to_the_global_file() {
        let dir = test_temp_dir("global-only");
        let path = dir.join("shell.json");
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registers_product_panels() {
        let shell = ProductShell::new();
        for id in shell::all_panels() {
            assert!(shell.catalog.contains(&id), "{id}");
        }
        assert_eq!(shell.catalog.len(), 6);
        // 编辑器声明一个主 Surface 槽位;纯 iced 原生面板不声明。
        assert_eq!(
            shell.catalog.get(&shell::markdown_id()).unwrap().surfaces,
            vec![bytehost_desktop::SurfaceSlot::MAIN]
        );
        assert!(
            shell
                .catalog
                .get(&shell::info_id())
                .unwrap()
                .surfaces
                .is_empty()
        );
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
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Right))),
        );
        assert!(
            !shell.panel_visible(&shell::markdown_id()),
            "covered by the right side's maximize"
        );
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
        press(&mut shell, &shell::info_id());
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "the original file stays byte for byte"
        );
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
        std::fs::write(
            &path,
            r#"{ "selected_right": "digger.apps", "right_collapsed": true }"#,
        )
        .unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert!(
            !shell.panel_visible(&shell::apps_id()),
            "selected but its zone is collapsed"
        );
        assert!(!shell.apps.opened());
        press(&mut shell, &shell::apps_id()); // expands the zone: no panel switch happens
        assert!(shell.panel_visible(&shell::apps_id()));
        assert!(
            shell.apps.opened(),
            "becoming visible by expanding still starts the service"
        );
    }

    /// 评审关注点 4:库的 `update` 对 `ToggleListCollapse` / `CollapseHover` 是 `unreachable!`(它们必须由宿主拦截)。
    /// 任何一条漏进去都会让进程崩溃,所以产品在 `Msg::Usage` 入口先拦截。
    #[test]
    fn library_messages_that_the_kernel_must_intercept_never_reach_the_library() {
        let mut shell = fresh("intercept");
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::CollapseHover(true))),
        );
        assert!(
            shell.shell.panes().get(&shell::usage_id()).is_none(),
            "hover changes nothing"
        );
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::ToggleListCollapse)),
        );
        assert_eq!(
            shell
                .shell
                .panes()
                .get(&shell::usage_id())
                .map(|d| d.collapsed),
            Some(true),
            "the library's toggle is routed to the shell"
        );
    }

    #[test]
    fn a_successful_rename_updates_the_tab_title() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("rename");
        std::fs::create_dir(dir.join("old-name")).unwrap();
        let old = dozer_model::ProjectInfo {
            id: 7,
            path: dir.join("old-name").to_string_lossy().into_owned(),
            name: "old-name".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                old.clone(),
            ])))),
        );
        shell.projects.open_tab_for_test(old.id);
        send(&mut shell, Msg::Projects(P::Activate(old.id)));
        assert_eq!(shell.projects.current_project().unwrap().name, "old-name");

        let mut renamed = old.clone();
        renamed.name = "new-name".into();
        send(
            &mut shell,
            Msg::Project(dozer_panel_project::Message::NameRenamed(
                old.id,
                Ok(renamed),
            )),
        );
        assert_eq!(
            shell.projects.current_project().unwrap().name,
            "new-name",
            "the registry is the only source of names, so the tab title follows it"
        );
    }

    fn send(shell: &mut ProductShell, m: Msg) {
        let _ = Product::update(shell, HostMessage::Product(m));
    }

    fn load_sample_rows(shell: &mut ProductShell) {
        send(
            shell,
            Msg::Usage(crate::adapters::usage::sample_loaded_message()),
        );
    }

    #[test]
    fn usage_layout_has_three_states() {
        let mut shell = fresh("layout3");
        assert_eq!(
            shell.usage_layout(),
            UsageLayout::ContentOnly,
            "no data yet: nothing to pair"
        );
        load_sample_rows(&mut shell);
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        assert_eq!(shell.usage_layout(), UsageLayout::Collapsed);
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
    }

    /// 评审关注点 3:文件里写着 collapsed 但此刻没有数据 —— 不能显示成一个空列表占位。
    #[test]
    fn collapsed_without_data_is_content_only() {
        let dir = test_temp_dir("collapsed-nodata");
        let path = dir.join("shell.json");
        std::fs::write(
            &path,
            r#"{ "panes": { "digger.usage": { "split": 0.35, "collapsed": true } } }"#,
        )
        .unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert_eq!(
            shell
                .shell
                .panes()
                .get(&shell::usage_id())
                .map(|d| d.collapsed),
            Some(true)
        );
        assert_eq!(shell.usage_layout(), UsageLayout::ContentOnly);
        load_sample_rows(&mut shell);
        assert_eq!(
            shell.usage_layout(),
            UsageLayout::Collapsed,
            "the stored state shows up as soon as there is data"
        );
    }

    /// 评审关注点 1、2、5:原生图表只占内容栏,且和 body 读同一个 `usage_layout()`。
    #[test]
    fn native_rect_follows_the_layout() {
        use bytehost_chrome::PairRects;
        let content = Rect::new(693.0, 47.0, 448.0, 700.0);
        // 右区默认:内容在左、列表在右
        let paired = PairRects {
            first: Rect::new(939.0, 47.0, 154.0, 700.0),
            second: Rect::new(693.0, 47.0, 286.0, 700.0),
            divider: Rect::new(979.0, 47.0, 8.0, 700.0),
        };
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, Some(paired)),
            paired.second,
            "only the content pane carries the chart"
        );
        // 无数据 / 收起:整个内容区,即便此刻还拿得到一份旧的配对矩形
        assert_eq!(
            usage_native_rect(UsageLayout::ContentOnly, content, Some(paired)),
            content
        );
        assert_eq!(
            usage_native_rect(UsageLayout::Collapsed, content, Some(paired)),
            content
        );
        // 镜像(拖到左栏):列表在左、内容在右 —— 取的仍是 `second`,位置由骨架几何给出
        let mirrored = PairRects {
            first: Rect::new(693.0, 47.0, 154.0, 700.0),
            second: Rect::new(855.0, 47.0, 286.0, 700.0),
            divider: Rect::new(847.0, 47.0, 8.0, 700.0),
        };
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, Some(mirrored)),
            mirrored.second
        );
        // 没有配对矩形(该侧不是用量)时回落到内容区,不 panic
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, None),
            content
        );
    }

    #[tokio::test]
    async fn the_usage_view_builds_in_every_layout_and_side() {
        let mut shell = fresh("usage-views");
        let _ = Product::view(&shell); // ContentOnly
        load_sample_rows(&mut shell);
        let _ = Product::view(&shell); // Paired
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        let _ = Product::view(&shell); // Collapsed
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        // 拖到左栏(镜像)与放大态
        send(
            &mut shell,
            Msg::Shell(ShellMsg::PanelPress {
                panel: shell::apps_id(),
            }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        send(
            &mut shell,
            Msg::Shell(ShellMsg::PanelPress {
                panel: shell::usage_id(),
            }),
        );
        send(
            &mut shell,
            Msg::Shell(ShellMsg::RailDragMove {
                side: Side::Left,
                index: 0,
            }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert_eq!(shell.shell.side_of(&shell::usage_id()), Some(Side::Left));
        let _ = Product::view(&shell);
        send(&mut shell, Msg::Shell(ShellMsg::Maximize(Side::Left)));
        let _ = Product::view(&shell);
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
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Left))),
        );
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
