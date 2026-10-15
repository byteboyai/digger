//! 项目注册表与项目页签在产品壳里的状态与视图。
//!
//! Project 是 host 层的共享业务(ADR-014):身份、注册表、Project 级共享记忆的存储来自 Dozer 与 Digger 共用的
//! `dozer-store-project`,由 `digger-store` 装配;这里只管 Digger 壳自己的事——哪些项目页签开着、哪个是当前、
//! 顶栏页签行怎么画、"＋"怎么弹文件夹选择器。页签状态**不进**共享骨架(Dozer 迁到共享顶栏时再一起上提)。

mod tabs;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};
use digger_store::ProjectHost;
use dozer_model::ProjectInfo;
use iced::widget::{button, row, text, tooltip};
use iced::{Alignment, Element, Task};

use crate::paths;
use tabs::ProjectTabs;

#[derive(Debug, Clone)]
pub enum Message {
    /// 顶栏"＋":弹原生文件夹选择器。
    PickFolder,
    /// 选择器返回(取消为 `None`)。
    FolderPicked(Option<PathBuf>),
    /// `open_project` 的结果:`Ok(None)` = 所选路径不是目录。
    Opened(Result<Option<ProjectInfo>, String>),
    /// `list_projects` 的结果。
    Listed(Result<Vec<ProjectInfo>, String>),
    Activate(i64),
    Close(i64),
}

pub struct ProjectsState {
    /// `None`:数据库打不开,项目功能不可用(原因在 `notice`),**不**退回临时库。
    host: Option<ProjectHost>,
    tabs: ProjectTabs,
    tabs_path: PathBuf,
    /// 磁盘上是不支持的高版本页签文件:内存里用空状态显示,**禁止回写**。
    tabs_blocked: bool,
    /// 注册表快照(最近优先)。页签只存 id,名字与路径以它为准。
    registry: Vec<ProjectInfo>,
    /// 目录已不存在的项目:页签标灰,不自动删除。
    missing: HashSet<i64>,
    notice: Option<String>,
}

impl ProjectsState {
    /// 在数据目录 `dir` 里打开注册表与页签文件。生产传 `paths::config_dir()`;测试传各自的临时目录。
    pub fn open_in(dir: &Path) -> Self {
        let loaded = ProjectTabs::load(&dir.join(paths::PROJECT_TABS_FILE));
        let (host, notice) = match ProjectHost::open(&dir.join(paths::PROJECTS_DB_FILE)) {
            Ok(host) => (Some(host), None),
            Err(e) => (None, Some(format!("项目功能不可用: {e}"))),
        };
        Self {
            host,
            tabs: loaded.tabs,
            tabs_path: dir.join(paths::PROJECT_TABS_FILE),
            tabs_blocked: loaded.save_blocked,
            registry: Vec::new(),
            missing: HashSet::new(),
            notice,
        }
    }

    /// 当前页签对应的项目;注册表还没读回来、或没有页签时为 `None`。
    pub fn current_project(&self) -> Option<&ProjectInfo> {
        let id = self.tabs.active()?;
        self.registry.iter().find(|p| p.id == id)
    }

    /// 当前项目 id(只读)。依赖 `project_id` 的面板(S3b 的 todo)从这里取。
    #[allow(dead_code)] // 第一个调用方是 S3b 的 todo 面板;在此之前只有测试用它
    pub fn current_project_id(&self) -> Option<i64> {
        self.current_project().map(|p| p.id)
    }

    #[cfg(test)]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 向注册表取最新列表(启动时与每次打开后调用)。
    pub fn refresh(&self) -> Task<Message> {
        let Some(host) = &self.host else {
            return Task::none();
        };
        let service = host.projects.clone();
        Task::perform(async move { service.list_projects().await.map_err(|e| e.to_string()) }, Message::Listed)
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::PickFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new().set_title("选择项目文件夹").pick_folder().await.map(|h| h.path().to_path_buf())
                },
                Message::FolderPicked,
            ),
            Message::FolderPicked(None) => Task::none(),
            Message::FolderPicked(Some(path)) => {
                let Some(host) = &self.host else {
                    return Task::none();
                };
                let service = host.projects.clone();
                let path = path.to_string_lossy().into_owned();
                Task::perform(async move { service.open_project(path).await.map_err(|e| e.to_string()) }, Message::Opened)
            }
            Message::Opened(Ok(Some(info))) => {
                self.notice = None;
                self.tabs.open_tab(info.id);
                // 立刻进快照:页签一出现就有名字,不等后面的 `Listed`。
                self.registry.retain(|p| p.id != info.id);
                self.registry.insert(0, info);
                self.save_tabs();
                self.refresh()
            }
            Message::Opened(Ok(None)) => {
                self.notice = Some("所选路径不是可打开的文件夹".into());
                Task::none()
            }
            Message::Opened(Err(e)) => {
                self.notice = Some(format!("打开项目失败: {e}"));
                Task::none()
            }
            Message::Listed(Ok(list)) => {
                let registered: HashSet<i64> = list.iter().map(|p| p.id).collect();
                self.tabs.prune(&registered);
                self.missing = list.iter().filter(|p| !Path::new(&p.path).is_dir()).map(|p| p.id).collect();
                self.registry = list;
                self.save_tabs();
                Task::none()
            }
            Message::Listed(Err(e)) => {
                // 读失败不等于"没有项目":保留现有页签与快照,只提示。
                self.notice = Some(format!("读取项目列表失败: {e}"));
                Task::none()
            }
            Message::Activate(id) => {
                if self.tabs.activate(id) {
                    self.save_tabs();
                }
                Task::none()
            }
            Message::Close(id) => {
                self.tabs.close_tab(id);
                self.save_tabs();
                Task::none()
            }
        }
    }

    fn save_tabs(&mut self) {
        if self.tabs_blocked {
            return;
        }
        if let Err(e) = self.tabs.save(&self.tabs_path) {
            self.notice = Some(format!("项目页签未保存: {e}"));
        }
    }

    /// 顶栏页签行:已开项目的页签 + "＋"。
    pub fn tabs_view(&self) -> Element<'_, Message> {
        let tokens = color::current();
        let dim = tokens.dim;
        let mut tabs = row![].spacing(6).align_y(Alignment::Center);
        for id in self.tabs.open() {
            // 注册表里没有的 id(尚未读回来)先不画。
            let Some(project) = self.registry.iter().find(|p| p.id == *id) else {
                continue;
            };
            let label_color = if self.missing.contains(id) || self.tabs.active() != Some(*id) { dim } else { tokens.body };
            let name = if self.missing.contains(id) { format!("{}(目录已不存在)", project.name) } else { project.name.clone() };
            tabs = tabs
                .push(button(text(name).size(font::body()).color(label_color)).style(button::text).on_press(Message::Activate(*id)))
                .push(button(text("×").size(font::body()).color(dim)).style(button::text).on_press(Message::Close(*id)));
        }
        if self.tabs.open().is_empty() {
            tabs = tabs.push(text("无项目").size(font::body()).color(dim));
        }
        let plus = button(icons::view(IconKind::Plus, icon_size::rail(), dim)).style(button::text).on_press_maybe(self.host.as_ref().map(|_| Message::PickFolder));
        let hint = if self.host.is_some() { "打开项目文件夹" } else { "项目功能不可用" };
        tabs = tabs.push(tooltip(plus, iced::widget::container(text(hint).size(font::body())).padding(6).style(iced::widget::container::rounded_box), tooltip::Position::Bottom));
        if let Some(notice) = &self.notice {
            tabs = tabs.push(text(notice.clone()).size(font::body()).color(tokens.gold));
        }
        tabs.into()
    }
}

#[cfg(test)]
mod tests;
