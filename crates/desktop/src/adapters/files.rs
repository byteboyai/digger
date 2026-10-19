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
pub struct FilesPanel {
    state: Option<WorkspaceState>,
    app_state: AppState,
    external_apps: ExternalAppsConfig,
    caps: FilesCapabilities,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<FilesMessage>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<FilesMessage>>,
    window: WindowProbe,
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
        Self {
            state: None,
            app_state: AppState::default(),
            external_apps: dozer_panel_files::external_apps::load_from(&external_apps_path),
            caps: FilesCapabilities::default(),
            handle: None,
            tx,
            rx: Some(rx),
            window: WindowProbe::default(),
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

    /// 树根跟随当前项目:换项目整棵重置,并拉一次 git 文件状态。`None`(没有项目)时清空。
    pub fn set_project(&mut self, project: Option<&ProjectInfo>) {
        let Some(project) = project else {
            self.state = None;
            return;
        };
        let root = PathBuf::from(&project.path);
        match &mut self.state {
            Some(state) => state.reset_for_project(FileTree::new(root.clone())),
            None => self.state = Some(WorkspaceState::new(FileTree::new(root.clone()))),
        }
        self.refresh_git(project.id, root);
    }

    /// 后台算一次本项目子树内的 git 文件状态,投回 `StatusesRefreshed`。
    fn refresh_git(&self, project_id: i64, root: PathBuf) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let tx = self.tx.clone();
        handle.spawn(async move {
            let statuses = tokio::task::spawn_blocking({
                let root = root.clone();
                move || dozer_panel_files::git_status::file_statuses(&root)
            })
            .await
            .unwrap_or_default();
            let _ = tx.send(FilesMessage::StatusesRefreshed(project_id, statuses));
        });
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
            "en",
        );
        iced::Task::none()
    }

    /// 排空库交给宿主的"只有产品做得到"的请求。Task 9 接产品动作。
    #[allow(dead_code)]
    pub fn take_requests(&mut self) -> Vec<dozer_panel_files::adapter::FilesRequest> {
        self.state
            .as_mut()
            .map(WorkspaceState::take_requests)
            .unwrap_or_default()
    }

    /// 排空待发提示(库的 outbox)。
    pub fn take_notices(&mut self) -> Vec<dozer_panel_host::Notice> {
        self.state
            .as_mut()
            .map(|s| s.take_outbox())
            .unwrap_or_default()
    }

    /// 排空宿主动作请求(搜索 / 文件历史 / 打开文件 / 选目录)。Task 9 接产品动作。
    #[allow(dead_code)]
    pub fn take_host_requests(&mut self) -> Vec<dozer_panel_host::HostRequest<FilesMessage>> {
        self.state
            .as_mut()
            .map(|s| s.host.take())
            .unwrap_or_default()
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
        "en"
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

    /// 能力四项全关:关掉的条目整条不出现。
    #[test]
    fn capabilities_are_all_off() {
        let caps = FilesCapabilities::default();
        assert!(!caps.file_history);
        assert!(!caps.send_to_agent);
        assert!(!caps.search);
        assert!(!caps.preview_tabs);
    }
}
