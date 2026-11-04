//! Files 面板的 **Digger 宿主适配层**(S5b-1 Task 7)。
//!
//! 面板本体(状态 / 更新 / 视图 / 拖拽状态机)在共享库 `dozer-panel-files`,Dozer 与 Digger
//! 共用唯一实现。本文件只做产品适配:
//!
//! - **展示语义**:[`PanelDisplay<Msg>`] 实现(面板头、内边距、行字号/行距、区域样式、locale、
//!   窗口尺寸与光标位置)。
//! - **能力**:Digger 一期四项全关(`FilesCapabilities::default()`):没有 git 历史、不发送到
//!   Agent、没有搜索、没有预览多标签——关掉的能力整条不出现。
//! - **后台结果**:git 文件状态在 Digger 侧现算(库只提供 [`file_statuses`] 这类纯函数),
//!   结果经一条 `mpsc` 回流成 `Message::StatusesRefreshed`。

use std::path::PathBuf;

use dozer_model::ProjectInfo;
use dozer_panel_files::Message as FilesMessage;
use dozer_panel_files::adapter::{FilesCapabilities, PanelDisplay};
use dozer_panel_files::external_apps::ExternalAppsConfig;
use dozer_panel_files::fs::FileTree;
use dozer_panel_files::state::{AppState, WorkspaceState};
use iced::{Element, Length};
use iced_widget::core::{Border, Padding};

use crate::product::Msg;

/// Files 面板的宿主状态:库的 [`WorkspaceState`] 只对应当前项目,换项目时整棵重置。
/// 待弹的原生菜单:`(菜单项, 窗口坐标)`。
#[cfg(target_os = "macos")]
pub type NativeMenuRequest = (
    Vec<dozer_panel_files::menu::MenuItem<FilesMessage>>,
    (f32, f32),
);

pub struct FilesPanel {
    state: Option<WorkspaceState>,
    app_state: AppState,
    external_apps: ExternalAppsConfig,
    caps: FilesCapabilities,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<FilesMessage>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<FilesMessage>>,
    window: WindowProbe,
    /// 这台面板当前服务的项目 id:监听事件与后台结果都按它过滤,旧项目的迟到消息丢弃。
    project_id: Option<i64>,
    /// 非 Git 目录没有 runtime 身份,保留旧 watch 只驱动树刷新。
    plain_watch: Option<bytegit::WatchHandle>,
    fs_tx: tokio::sync::mpsc::UnboundedSender<FsChange>,
    fs_rx: Option<tokio::sync::mpsc::UnboundedReceiver<FsChange>>,
    /// iced 自绘的右键菜单(Digger 不认原生 NSMenu,`ShowNativeMenu` 一律落到这里)。
    menu: Option<crate::adapters::menu_overlay::MenuOverlay<FilesMessage>>,
    /// macOS:库排出的"弹菜单"请求不建 iced 浮层,留给产品弹原生 NSMenu(原生菜单浮在 wry
    /// 页面之上,弹出时不用把文件内容藏起来)。`(菜单项, 窗口坐标)`,产品用 `take_menu_requests` 取走。
    #[cfg(target_os = "macos")]
    native_menus: Vec<NativeMenuRequest>,
}

/// 用系统默认应用打开一个路径。生产走 `open <path>`(路径作为独立参数,不经 shell)。
pub type Opener = std::sync::Arc<dyn Fn(&std::path::Path) -> std::io::Result<()> + Send + Sync>;

/// 生产用打开器:macOS 走 `open`,其它平台 `xdg-open`;路径永远是独立的参数。
pub fn system_opener() -> Opener {
    std::sync::Arc::new(|path: &std::path::Path| {
        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(not(target_os = "macos"))]
        let program = "xdg-open";
        std::process::Command::new(program)
            .arg(path)
            .spawn()
            .map(|_| ())
    })
}

/// 用系统默认应用打开一个路径(降级页 / 页签菜单的"用系统应用打开")。失败只记日志。
pub fn open_with_default(opener: &Opener, path: &std::path::Path) {
    if let Err(e) = opener(path) {
        eprintln!("digger: 打开 {} 失败: {e}", path.display()); // cli-output
    }
}

/// 一次工作区变更(文件树需要重读的那部分)。
#[derive(Debug, Clone, Default)]
pub struct FsChange {
    pub project_id: i64,
    /// 工作区有改动(增删改文件)。
    pub workdir_changed: bool,
    /// `.git` 引用类文件变化(创建/删除 `.git` 时用于身份升格)。
    pub refs_changed: bool,
}

/// 窗口尺寸与光标位置:展示语义里拖拽幽灵胶囊要用,由产品每帧写回。
#[derive(Default)]
pub struct WindowProbe {
    size: (f32, f32),
    cursor: (f32, f32),
}

impl FilesPanel {
    /// 构造:注入外部应用配置路径(与 Dozer 同名的 `external_apps.json`)。运行时句柄由
    /// [`FilesPanel::attach_handle`] 在进入 iced 运行时后补挂。
    pub fn new(external_apps_path: PathBuf) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (fs_tx, fs_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state: None,
            app_state: AppState::default(),
            external_apps: dozer_panel_files::external_apps::load_from(&external_apps_path),
            // 预览页签的右键菜单(关闭 / 复制路径 / 用外部软件打开 / 从磁盘重新加载)要它;
            // 文件历史、发送到 Agent、搜索仍然关闭。
            caps: FilesCapabilities {
                preview_tabs: true,
                ..FilesCapabilities::default()
            },
            handle: None,
            tx,
            rx: Some(rx),
            window: WindowProbe::default(),
            project_id: None,
            plain_watch: None,
            fs_tx,
            fs_rx: Some(fs_rx),
            menu: None,
            #[cfg(target_os = "macos")]
            native_menus: Vec::new(),
        }
    }

    /// 挂上 iced 运行时句柄(仅可在运行时上下文内调用)。
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 取走后台结果接收端(组合根在 `boot` 里接成 iced 流;只该取一次)。
    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<FilesMessage>> {
        self.rx.take()
    }

    /// 每帧写回窗口尺寸与光标位置(库画拖拽胶囊时读)。Task 8 接每帧探针。
    #[allow(dead_code)]
    pub fn set_window_probe(&mut self, size: (f32, f32), cursor: (f32, f32)) {
        self.window.size = size;
        self.window.cursor = cursor;
    }

    /// 树根跟随当前项目:换项目时丢弃旧树与旧监听、按新路径重建,并拉一次 git 文件状态。
    /// `None`(没有项目)时清空。目录已不存在时树为空,由视图显示空盒并在 `take_notices` 里
    /// 报一条错误提示(不 panic)。
    pub fn set_project(&mut self, project: Option<&ProjectInfo>) {
        let Some(project) = project else {
            self.state = None;
            self.project_id = None;
            return;
        };
        self.project_id = Some(project.id);
        self.plain_watch = None;
        let root = PathBuf::from(&project.path);
        match &mut self.state {
            Some(state) => state.reset_for_project(FileTree::new(root.clone())),
            None => self.state = Some(WorkspaceState::new(FileTree::new(root.clone()))),
        }
        if bytegit::Repo::discover_workdir(&root).is_err() {
            self.start_plain_watch(project.id, root.clone());
        }
        if let Some(state) = self.state.as_mut()
            && !root.is_dir()
        {
            state.outbox.push(
                dozer_panel_host::NoticeLevel::Error,
                format!("项目目录不存在:{}", root.display()),
            );
        }
    }

    /// 升格为 runtime 订阅时停掉 plain watch。
    pub fn stop_plain_watch(&mut self) {
        self.plain_watch = None;
    }

    /// 取走非 Git 目录的监听事件接收端。
    pub fn take_fs_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<FsChange>> {
        self.fs_rx.take()
    }

    /// 非 Git 目录的旧式监听:只重读文件树,不产生 Git 查询。
    pub fn fs_changed(&mut self, change: &FsChange) -> iced::Task<Msg> {
        if self.project_id == Some(change.project_id)
            && change.workdir_changed
            && let Some(state) = self.state.as_mut()
        {
            state.reload_tree_from_disk();
        }
        iced::Task::none()
    }

    fn start_plain_watch(&mut self, project_id: i64, root: PathBuf) {
        let tx = self.fs_tx.clone();
        let options = bytegit::WatchOptions::new(std::time::Duration::from_millis(300));
        match bytegit::watch(&root, options, move |changes| {
            let _ = tx.send(FsChange {
                project_id,
                workdir_changed: changes.workdir_changed,
                refs_changed: changes.refs_changed,
            });
        }) {
            Ok(handle) => self.plain_watch = Some(handle),
            Err(e) => eprintln!("digger: 非 Git 目录监听启动失败: {e}"), // cli-output
        }
    }

    /// 应用共享 runtime 已验证 token 的组合快照。
    pub fn apply_snapshot(
        &mut self,
        project_id: i64,
        statuses: std::collections::HashMap<PathBuf, dozer_panel_files::git_status::FileGitStatus>,
        info: dozer_panel_files::state::GitInfo,
    ) {
        if self.project_id == Some(project_id)
            && let Some(state) = self.state.as_mut()
        {
            state.apply_snapshot(statuses, info);
        }
    }

    /// 交给宿主的后台目录刷新计划(不做磁盘 IO)。
    pub fn begin_tree_refresh(
        &mut self,
        paths: &[PathBuf],
        full: bool,
    ) -> Option<dozer_panel_files::fs::TreeRefreshRequest> {
        self.state
            .as_mut()
            .and_then(|state| state.begin_tree_refresh(paths, full))
    }

    /// 合并后台目录结果;返回下一轮计划。
    pub fn apply_tree_refresh(
        &mut self,
        owner: u64,
        generation: u64,
        snapshots: Vec<dozer_panel_files::fs::DirSnapshot>,
    ) -> Option<dozer_panel_files::fs::TreeRefreshRequest> {
        self.state
            .as_mut()
            .and_then(|state| state.apply_tree_refresh(owner, generation, snapshots))
    }

    /// 处理一条库消息。`project_id` 是消息所属项目(注册表里已经没有时丢弃)。
    pub fn update(&mut self, message: FilesMessage, project_id: Option<i64>) -> iced::Task<Msg> {
        let (Some(project_id), Some(handle)) = (project_id, self.handle.clone()) else {
            return iced::Task::none();
        };
        let Some(state) = self.state.as_mut() else {
            return iced::Task::none();
        };
        let tx = self.tx.clone();
        dozer_panel_files::update::update(
            state,
            &mut self.app_state,
            message,
            project_id,
            &handle,
            move |m| {
                let _ = tx.send(m);
            },
            &self.external_apps,
            &self.caps,
            crate::settings::language::code(),
        );
        iced::Task::none()
    }

    /// 排空库交给宿主的"只有产品做得到"的请求(右键菜单 / 分支弹层)。
    ///
    /// Digger 不认 macOS 原生 NSMenu:`ShowNativeMenu` 与 `ShowContextMenu` 同口径,
    /// 都落到 iced 自绘的 [`crate::adapters::menu_overlay`];空条目表不弹。
    pub fn take_requests(&mut self) -> Vec<dozer_panel_files::adapter::FilesRequest> {
        let requests = self
            .state
            .as_mut()
            .map(WorkspaceState::take_requests)
            .unwrap_or_default();
        for request in &requests {
            match request {
                dozer_panel_files::adapter::FilesRequest::ShowContextMenu { items, at }
                | dozer_panel_files::adapter::FilesRequest::ShowNativeMenu { items, at } => {
                    self.show_menu(items, *at);
                }
                // 分支弹层:Digger 一期没有分支弹层(降级为只读,见报告 D6)。
                dozer_panel_files::adapter::FilesRequest::ShowBranchPicker { .. } => {}
            }
        }
        requests
    }

    /// 当前要渲染的右键菜单(没有时为 `None`)。
    /// macOS:库要弹菜单——不建 iced 浮层,排进待弹的原生菜单队列(空菜单不弹)。
    #[cfg(target_os = "macos")]
    fn show_menu(
        &mut self,
        items: &[dozer_panel_files::menu::MenuItem<FilesMessage>],
        at: (f32, f32),
    ) {
        if !items.is_empty() {
            self.native_menus.push((items.to_vec(), at));
        }
    }

    /// 其他平台:iced 自绘浮层(空条目表不弹)。
    #[cfg(not(target_os = "macos"))]
    fn show_menu(
        &mut self,
        items: &[dozer_panel_files::menu::MenuItem<FilesMessage>],
        at: (f32, f32),
    ) {
        let overlay = crate::adapters::menu_overlay::MenuOverlay {
            anchor: crate::adapters::menu_overlay::MenuAnchor { x: at.0, y: at.1 },
            items: items.to_vec(),
        };
        self.menu = (!overlay.is_empty()).then_some(overlay);
    }

    /// macOS:取走待弹的原生菜单 `(菜单项, 窗口坐标)`,产品逐条弹出。
    #[cfg(target_os = "macos")]
    pub fn take_menu_requests(&mut self) -> Vec<NativeMenuRequest> {
        std::mem::take(&mut self.native_menus)
    }

    pub fn menu(&self) -> Option<&crate::adapters::menu_overlay::MenuOverlay<FilesMessage>> {
        self.menu.as_ref()
    }

    /// 文件面板上此刻该显示的确认对话框(删除确认);没有就是 `None`。
    /// 由产品叠加渲染 —— Digger 的弹窗是面板内的居中卡片,不是独立窗口。
    pub fn delete_confirm(
        &self,
    ) -> Option<byteui::feedback::dialog::ConfirmDialog<dozer_panel_files::Message>> {
        self.state.as_ref().and_then(|s| {
            dozer_panel_files::dialogs::delete_confirm_spec(s, crate::settings::language::code())
        })
    }

    /// 移动/重命名卡片(库内已有的 `files_move_card`)。库在没有待办移动时返回空列。
    /// 产品据此判断要不要出遮罩。
    pub fn move_card(
        &self,
    ) -> Option<iced::Element<'_, Msg, iced_widget::Theme, iced_renderer::Renderer>> {
        let state = self.state.as_ref()?;
        state.pending_move.as_ref()?;
        Some(
            dozer_panel_files::view::files_move_card(state, crate::settings::language::code())
                .map(Msg::Files),
        )
    }

    /// 此刻是否有面板内弹窗(删除确认 / 移动卡片)。产品据此订阅 Esc。
    pub fn has_modal(&self) -> bool {
        let Some(state) = self.state.as_ref() else {
            return false;
        };
        state.pending_move.is_some()
            || state
                .tree_delete_confirm
                .as_ref()
                .is_some_and(|t| !t.is_empty())
    }

    /// Esc / 取消:先关菜单,再退移动卡片,最后退删除确认(与 Dozer 的层级一致)。
    pub fn dismiss(&mut self, project_id: Option<i64>) -> iced::Task<Msg> {
        if self.menu.take().is_some() {
            return self.update(FilesMessage::ContextMenuClose, project_id);
        }
        // 库的取消分支是同步改状态(不派任务);避免 `iced::Task` 落空把
        // 「按 Esc 但没轮询到」误判成没关。
        if let Some(state) = self.state.as_mut() {
            if state.pending_move.is_some() {
                state.pending_move = None;
                return iced::Task::none();
            }
            if state.tree_delete_confirm.take().is_some() {
                return iced::Task::none();
            }
        }
        iced::Task::none()
    }

    /// 排空宿主动作请求(搜索/文件历史 Digger 没有,能力开关也已关掉,正常不会出现)。
    /// 选目录交给调用方(产品弹系统对话框);`OpenFile` 由调用方路由到预览(D3)。
    pub fn take_host_requests(&mut self) -> Vec<dozer_panel_host::HostRequest<FilesMessage>> {
        self.state
            .as_mut()
            .map(|s| s.host.take())
            .unwrap_or_default()
    }

    /// 落地一条宿主动作。选目录返回 `Some(start)`,产品据此弹系统对话框。
    /// `OpenFile` 不在这里打开:`open_path` 返回路径,调用方按 D3 送进预览。
    pub fn handle_host_request(
        &mut self,
        request: &dozer_panel_host::HostRequest<FilesMessage>,
    ) -> Option<PathBuf> {
        match request {
            dozer_panel_host::HostRequest::Command(dozer_panel_host::PanelCommand::OpenFile {
                ..
            }) => None,
            dozer_panel_host::HostRequest::Command(
                dozer_panel_host::PanelCommand::SearchIn { .. }
                | dozer_panel_host::PanelCommand::ShowFileHistory { .. },
            ) => None,
            dozer_panel_host::HostRequest::PickDirectory { start, .. } => start.clone(),
        }
    }

    /// 这条宿主动作要预览打开哪个文件(文件树双击 / 回车)。
    pub fn open_path(request: &dozer_panel_host::HostRequest<FilesMessage>) -> Option<&PathBuf> {
        match request {
            dozer_panel_host::HostRequest::Command(dozer_panel_host::PanelCommand::OpenFile {
                path,
            }) => Some(path),
            _ => None,
        }
    }

    /// 排空待发提示(库的 outbox)。
    pub fn take_notices(&mut self) -> Vec<dozer_panel_host::Notice> {
        self.state
            .as_mut()
            .map(|s| s.take_outbox())
            .unwrap_or_default()
    }

    /// 树内行被按下:武装拖拽(库的 `TreeRowPress` 由宿主拦截,`files::update` 收到会 `unreachable!`)。
    /// 单击只选中,不展开 / 不打开,那要等松开(见 [`Self::release_tree_drag`])或双击。
    pub fn arm_tree_drag(&mut self, path: PathBuf, is_dir: bool) {
        let cursor = self.window.cursor;
        if let Some(state) = self.state.as_mut() {
            state.arm_tree_drag(path, is_dir, cursor, std::time::Instant::now());
        }
    }

    /// 是否有一场树内拖拽在进行(含还没越过阈值的 `Pending`):产品据此决定左键松开时要不要收尾。
    pub fn tree_drag_active(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|s| s.tree_drag_press_pos().is_some())
    }

    /// 左键松开:把"这场拖拽有没有走到 `Dragging`"读成 `confirmed`,发库的 `TreeDragEnd`。
    /// 没越过阈值就是一次单击(只选中)。
    pub fn release_tree_drag(
        &mut self,
        shift: bool,
        command: bool,
        project_id: Option<i64>,
    ) -> iced::Task<Msg> {
        let confirmed = self.state.as_ref().is_some_and(|s| s.tree_drag_confirmed());
        self.update(
            FilesMessage::TreeDragEnd {
                confirmed,
                shift,
                command,
            },
            project_id,
        )
    }

    /// 每次光标移动调用:`Pending` 的拖拽同时越过距离和按住时长两道阈值才推进到 `Dragging`
    /// (与 Dozer 同一套阈值,在 `dozer-panel-host`)。左键并未物理按住却还残留拖拽时自愈清空。
    /// 返回是否发生了推进。
    pub fn maybe_confirm_tree_drag(&mut self, left_mouse_down: bool) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        if !left_mouse_down {
            state.cancel_tree_drag();
            return false;
        }
        let cursor = self.window.cursor;
        let confirm = state.tree_drag_is_pending()
            && state
                .tree_drag_press_pos()
                .is_some_and(|p| dozer_panel_host::tree_drag_past_threshold(p, cursor))
            && state
                .tree_drag_armed_at()
                .is_some_and(|t| dozer_panel_host::tree_drag_held_long_enough(t.elapsed()));
        if confirm {
            state.confirm_tree_drag();
        }
        confirm
    }

    /// 当前树里被选中的路径(排序后,测试里断言"单击只选中"用)。
    #[cfg(test)]
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self
            .state
            .as_ref()
            .map(|s| s.tree_selected.iter().cloned().collect())
            .unwrap_or_default();
        paths.sort();
        paths
    }

    /// 面板视图(区域几何由骨架给出,与 Dozer 的 `files_pane` 同义)。
    pub fn view(&self, width: Length, outer: Border) -> Element<'_, Msg> {
        match &self.state {
            Some(state) => dozer_panel_files::view::view(state, self, width, outer, 0.0, 0.0, 0.0)
                .map(Msg::Files),
            None => iced::widget::container(iced::widget::Space::new())
                .width(width)
                .height(Length::Fill)
                .into(),
        }
    }
}

impl PanelDisplay for FilesPanel {
    fn panel_head<'a>(
        &self,
        icon: byteui::interaction::icons::IconKind,
        title: String,
    ) -> Element<'a, FilesMessage, iced_widget::Theme, iced_renderer::Renderer> {
        let tokens = byteui::theme::color::current();
        iced::widget::row![
            byteui::interaction::icons::view(
                icon,
                byteui::theme::icon_size::row(),
                tokens.tab_active_border,
            ),
            iced::widget::text(title)
                .size(byteui::theme::font::title())
                .color(tokens.tab_active_border),
        ]
        .spacing(8)
        .height(Length::Fixed(byteui::theme::geometry::tab_button_size()))
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn header_padding(&self) -> Padding {
        // 与 Dozer `workspace.json` 的 `project_pane.padding` 一致(8,随主题缩放)。
        let s = byteui::theme::icon_size::scale();
        Padding::new(8.0 * s)
    }

    fn tree_row_font_size(&self) -> f32 {
        // 与 Dozer 一致:终端字号 × 主题缩放。
        14.0 * byteui::theme::icon_size::scale()
    }

    fn line_height_factor(&self) -> f32 {
        // 与 Dozer `terminal.json` 的 `line_height_factor` 对齐。
        1.2
    }

    fn region(&self) -> byteui::theme::region::RegionStyle {
        // 与 Dozer `workspace.json` 的 `project_pane` 一致(背景 BG、无边框、padding 8、gap 2)。
        byteui::theme::region::RegionStyle {
            background: byteui::theme::color::current().bg.into(),
            border: None,
            padding: iced_widget::core::Padding::new(8.0 * byteui::theme::icon_size::scale()),
            gap: 2.0,
            margin: iced_widget::core::Padding::ZERO,
        }
    }

    fn locale(&self) -> &str {
        crate::settings::language::code()
    }

    fn window_size(&self) -> (f32, f32) {
        self.window.size
    }

    fn last_cursor(&self) -> (f32, f32) {
        self.window.cursor
    }
}

/// 把 Files 面板的后台结果接收端接成一条持续到进程结束的 iced 流。
pub fn receiver_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<FilesMessage>,
) -> impl iced::futures::Stream<Item = FilesMessage> {
    iced::futures::stream::unfold(Some(rx), |maybe| async move {
        let mut rx = maybe?;
        let next = rx.recv().await?;
        Some((next, Some(rx)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(tag: &str) -> FilesPanel {
        let dir = std::env::temp_dir().join(format!(
            "digger-files-panel-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        FilesPanel::new(dir.join("external_apps.json"))
    }

    fn project(name: &str, dir: &std::path::Path) -> ProjectInfo {
        ProjectInfo {
            id: 7,
            path: dir.to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    /// 没有项目时面板画空盒(还没有库状态),不 panic。
    #[test]
    fn with_no_project_there_is_no_state() {
        let panel = panel("no-project");
        assert!(panel.state.is_none());
        let _ = panel.view(Length::Fill, Border::default());
    }

    /// 树根跟随项目:换到另一个目录后,库状态的树根就是新目录。
    #[test]
    fn set_project_points_the_tree_root_at_the_project() {
        let a = std::env::temp_dir().join("digger-files-panel-root-a");
        let b = std::env::temp_dir().join("digger-files-panel-root-b");
        let mut panel = panel("set-project");
        panel.set_project(Some(&project("a", &a)));
        let state = panel.state.as_ref().expect("state built");
        let tree = state.file_tree.as_ref().expect("tree built");
        assert_eq!(tree.root(), a.as_path());
        panel.set_project(Some(&project("b", &b)));
        let state = panel.state.as_ref().expect("state kept");
        let tree = state.file_tree.as_ref().expect("tree rebuilt");
        assert_eq!(tree.root(), b.as_path());
        panel.set_project(None);
        assert!(panel.state.is_none());
    }

    /// 两个临时项目目录,各放一个不同的文件;用完删掉。
    struct TwoProjects {
        a: PathBuf,
        b: PathBuf,
    }

    impl TwoProjects {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "digger-files-panel-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let a = base.join("a");
            let b = base.join("b");
            std::fs::create_dir_all(&a).unwrap();
            std::fs::create_dir_all(&b).unwrap();
            std::fs::write(a.join("only-in-a.txt"), "a").unwrap();
            std::fs::write(b.join("only-in-b.txt"), "b").unwrap();
            Self { a, b }
        }
    }

    impl Drop for TwoProjects {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.a.parent().unwrap());
        }
    }

    fn visible_names(panel: &FilesPanel) -> Vec<String> {
        panel
            .state
            .as_ref()
            .and_then(|s| s.file_tree.as_ref())
            .map(|t| {
                t.visible_rows()
                    .iter()
                    .map(|r| r.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    /// 没有项目:面板是空的,且不向宿主提任何请求或命令。
    #[test]
    fn without_a_project_the_panel_is_empty_and_requests_nothing() {
        let mut panel = panel("without-project");
        panel.set_project(None);
        let _ = panel.view(Length::Fill, Border::default());
        assert!(panel.take_requests().is_empty());
        assert!(panel.take_host_requests().is_empty());
        assert!(panel.take_notices().is_empty());
    }

    /// 换项目重建树根:可见行只含新项目的文件。
    #[test]
    fn switching_projects_rebuilds_the_tree_root() {
        let dirs = TwoProjects::new("switch");
        let mut panel = panel("switch-projects");
        panel.set_project(Some(&project("a", &dirs.a)));
        assert_eq!(visible_names(&panel), vec!["only-in-a.txt"]);
        panel.set_project(Some(&project("b", &dirs.b)));
        assert_eq!(visible_names(&panel), vec!["only-in-b.txt"]);
    }

    /// 项目目录已经不存在:不 panic,报一条错误提示。
    #[test]
    fn a_project_whose_directory_vanished_shows_a_notice_and_does_not_panic() {
        let dirs = TwoProjects::new("vanished");
        let mut panel = panel("vanished");
        std::fs::remove_dir_all(&dirs.a).unwrap();
        panel.set_project(Some(&project("a", &dirs.a)));
        let _ = panel.view(Length::Fill, Border::default());
        let notices = panel.take_notices();
        assert!(
            notices
                .iter()
                .any(|n| matches!(n.level, dozer_panel_host::NoticeLevel::Error)),
            "missing root must be reported: {notices:?}"
        );
    }

    /// 旧项目的迟到完成消息被丢弃:B 的树不受 A 的 `PasteDone` 影响。
    #[test]
    fn late_paste_done_from_previous_project_is_ignored() {
        let dirs = TwoProjects::new("late");
        let mut panel = panel("late-paste");
        panel.set_project(Some(&project("a", &dirs.a)));
        panel.set_project(Some(&project("b", &dirs.b)));
        let before = visible_names(&panel);
        let _ = panel.update(
            FilesMessage::PasteDone(
                7,
                Err(dozer_panel_files::fs::ProjectFsError::Io {
                    detail: "stale".into(),
                }),
            ),
            Some(7),
        );
        assert_eq!(visible_names(&panel), before);
    }

    /// 打开文件:D3——双击不再交给系统应用,而是把原路径交出去,由产品路由到预览。
    #[test]
    fn open_file_command_hands_the_path_back_for_the_preview() {
        let mut panel = panel("open-file");
        let weird = PathBuf::from("/tmp/a dir/with \"quotes\" and 'single'.txt");
        let request =
            dozer_panel_host::HostRequest::Command(dozer_panel_host::PanelCommand::OpenFile {
                path: weird.clone(),
            });
        assert!(panel.handle_host_request(&request).is_none());
        assert_eq!(FilesPanel::open_path(&request), Some(&weird));
    }

    /// G-M:降级页 / 页签菜单的"用系统应用打开"走 `system_opener`——**路径
    /// 路径必须以**单个**参数交给打开器,不经 shell、不因空格 / 引号 / `$`
    /// 被拆分或展开。
    ///
    /// N3:旧版把假 opener 写进临时目录再 `unsafe set_var("PATH", ..)`,靠
    /// 「测试单线程跑」这个**仓库里没人强制**的前提,而且假 opener 先建空
    /// 日志再写入、测试每 10ms 轮询,可能读到空文件。现在 `Opener` 本身就是
    /// 可注入的函数参数,直接传一个记录闭包即可。
    #[test]
    fn system_opener_receives_the_path_verbatim_as_a_single_argument() {
        let weird = PathBuf::from("/tmp/a dir/with \"quotes\" and 'single' $HOME;x.txt");
        let seen: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = {
            let seen = std::sync::Arc::clone(&seen);
            std::sync::Arc::new(move |path: &std::path::Path| {
                seen.lock().unwrap().push(path.to_path_buf());
                Ok(())
            }) as Opener
        };
        open_with_default(&recorder, &weird);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![weird.clone()],
            "路径必须原样、唯一地交给打开器"
        );
    }

    /// N3:生产打开器真的不走 shell —— 路径作为 `Command::arg` 的独立参数。
    #[test]
    fn the_system_opener_never_goes_through_a_shell() {
        let source = include_str!("files.rs");
        let start = source
            .find("pub fn system_opener()")
            .expect("找不到 system_opener");
        let rest = &source[start..];
        let end = rest.find("\n}\n").expect("找不到 system_opener 结尾");
        let body = &rest[..end];
        assert!(
            body.contains(".arg(path)"),
            "路径必须作为独立参数传给 open / xdg-open"
        );
        for shell in ["sh -c", "bash -c", "cmd /C"] {
            assert!(!body.contains(shell), "打开器里出现了 shell: {shell}");
        }
    }

    /// 没有项目时右键空白区域:不 panic,也不弹空菜单。
    #[test]
    fn the_context_menu_request_is_not_dropped_when_no_project_is_open() {
        let mut panel = panel("menu-no-project");
        panel.set_project(None);
        let _ = panel.update(FilesMessage::RightClickAt { x: 10.0, y: 20.0 }, None);
        let _ = panel.update(
            FilesMessage::ContextMenuOpen {
                path: PathBuf::from("/tmp"),
                is_dir: true,
            },
            None,
        );
        assert!(panel.take_requests().is_empty());
        assert!(panel.menu().is_none());
        let _ = panel.view(Length::Fill, Border::default());
    }

    /// 库给出空条目表时不弹浮层(不是画一个空盒子)。
    #[test]
    fn an_empty_item_list_does_not_open_a_popup() {
        let mut panel = panel("menu-empty");
        let empty = crate::adapters::menu_overlay::MenuOverlay::<FilesMessage> {
            anchor: crate::adapters::menu_overlay::MenuAnchor { x: 0.0, y: 0.0 },
            items: Vec::new(),
        };
        assert!(empty.is_empty());
        panel.menu = Some(empty);
        // 产品侧按 `is_empty` / 条目数决定不渲染;这里确认面板能识别空表。
        assert!(panel.menu().is_some_and(|m| m.is_empty()));
        panel.menu = None;
        assert!(panel.menu().is_none());
    }

    /// Esc 的层级:先关右键菜单,再退移动卡片;都关着时什么也不做。
    #[test]
    fn escape_closes_the_menu_before_the_dialogs() {
        let mut panel = panel("esc-layers");
        assert!(!panel.has_modal());
        let dir = std::env::temp_dir().join(format!("digger-esc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        panel.set_project(Some(&project("esc", &dir)));
        assert!(!panel.has_modal());
        assert!(panel.delete_confirm().is_none());
        panel.state.as_mut().expect("state").tree_delete_confirm =
            Some(vec![(PathBuf::from("/tmp/a"), false)]);
        assert!(panel.has_modal());
        assert!(panel.delete_confirm().is_some());
        let _ = panel.dismiss(None);
        assert!(!panel.has_modal(), "delete confirm is dismissed");
        assert!(panel.delete_confirm().is_none());
    }

    /// 只开预览页签菜单:文件历史、发送到 Agent、搜索仍关,关掉的条目整条不出现。
    #[test]
    fn only_the_preview_tab_menu_capability_is_on() {
        let panel = panel("caps");
        assert!(panel.caps.preview_tabs);
        assert!(!panel.caps.file_history);
        assert!(!panel.caps.send_to_agent);
        assert!(!panel.caps.search);
    }

    /// 预览页签右键菜单:有"关闭"和"从磁盘重新加载",没有文件历史条目。
    #[tokio::test]
    async fn the_preview_tab_menu_has_close_and_reload_but_no_history() {
        use dozer_panel_files::adapter::PreviewKind;
        use dozer_panel_files::menu::MenuItem;
        let dir = std::env::temp_dir().join(format!("digger-tab-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut panel = panel("tab-menu");
        panel.attach_handle(tokio::runtime::Handle::current());
        panel.set_project(Some(&project("p", &dir)));
        let _ = panel.update(FilesMessage::RightClickAt { x: 30.0, y: 40.0 }, Some(7));
        let _ = panel.update(
            FilesMessage::TabContextMenuOpen {
                kind: PreviewKind::Files,
                path: dir.join("a.rs"),
            },
            Some(7),
        );
        let _ = panel.take_requests();
        // macOS:菜单交给产品弹原生菜单(不建 iced 浮层);其他平台:iced 浮层。
        #[cfg(target_os = "macos")]
        let (items, anchor) = {
            assert!(panel.menu().is_none(), "macOS 不能同时出 iced 浮层");
            let mut pending = panel.take_menu_requests();
            assert_eq!(pending.len(), 1, "应有一条待弹的原生菜单");
            pending.remove(0)
        };
        #[cfg(not(target_os = "macos"))]
        let (items, anchor) = {
            let menu = panel.menu().expect("页签右键应弹出菜单");
            (menu.items.clone(), (menu.anchor.x, menu.anchor.y))
        };
        let msgs: Vec<&FilesMessage> = items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Entry { msg, .. } => Some(msg),
                MenuItem::Separator => None,
            })
            .collect();
        assert!(
            msgs.iter()
                .any(|m| matches!(m, FilesMessage::TabContextMenuCloseTab(..)))
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, FilesMessage::TabReloadFromDisk(..)))
        );
        assert!(
            !msgs
                .iter()
                .any(|m| matches!(m, FilesMessage::FileHistoryOpen(..))),
            "Digger 没有文件历史"
        );
        assert_eq!(anchor, (30.0, 40.0));
    }
}
