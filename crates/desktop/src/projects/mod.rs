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
use iced::widget::{button, mouse_area, row, text, tooltip};
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
    /// `list_projects` 的结果,连同每个项目的目录是否还在。
    Listed(Result<Listing, String>),
    Activate(i64),
    Close(i64),
    /// 鼠标进入某个页签(含它的关闭按钮)或"＋":顶栏上的可交互控件。
    HoverEnter(i64),
    /// 鼠标离开它。
    HoverLeave(i64),
}

/// 一次注册表读取的结果:项目列表(最近优先)与"目录已不存在"的项目 id。
#[derive(Debug, Clone)]
pub struct Listing {
    pub projects: Vec<ProjectInfo>,
    pub missing: HashSet<i64>,
}

impl Listing {
    /// 对每个项目的路径做一次 `is_dir()`。这是阻塞的磁盘访问(项目在断开的网络盘上时可能很久),
    /// 所以只在后台阻塞线程池里调用,不进 `update`。
    pub fn scan(projects: Vec<ProjectInfo>) -> Self {
        let missing = projects
            .iter()
            .filter(|p| !Path::new(&p.path).is_dir())
            .map(|p| p.id)
            .collect();
        Self { projects, missing }
    }
}

/// 提示按来源分类:同一来源的下一次成功才清掉它,别的来源的事件不会误清。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    /// 注册表打不开:项目功能整体不可用,永不被清掉。
    Unavailable,
    /// 打开项目失败 / 所选不是文件夹:下一次开始选择时清掉。
    Open,
    /// 读取项目列表失败:下一次成功读取时清掉。
    List,
    /// 页签文件没保存:下一次保存成功时清掉。
    Save,
}

/// 顶栏上提示文字的最大字数;全文放在悬停提示里。
const NOTICE_MAX_CHARS: usize = 48;

/// "＋"不是页签:`hovered` 用这个 id 记它。
pub const ADD_TAB_HOVER: i64 = -1;

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub struct ProjectsState {
    /// `None`:数据库打不开,项目功能不可用(原因在 `notice`),**不**退回临时库。
    host: Option<ProjectHost>,
    tabs: ProjectTabs,
    /// 上一次成功落盘(或读盘)的页签状态:没变化就不写盘。
    saved: ProjectTabs,
    tabs_path: PathBuf,
    /// 磁盘上是不支持的高版本页签文件:内存里用空状态显示,**禁止回写**。
    tabs_blocked: bool,
    /// 注册表快照(最近优先)。页签只存 id,名字与路径以它为准。
    registry: Vec<ProjectInfo>,
    /// 目录已不存在的项目:页签标灰,不自动删除。
    missing: HashSet<i64>,
    /// 此刻悬停在顶栏可交互控件上的页签("＋"记为 [`ADD_TAB_HOVER`])。
    hovered: Option<i64>,
    notice: Option<(NoticeKind, String)>,
}

impl ProjectsState {
    /// 在数据目录 `dir` 里打开注册表与页签文件。生产传 `paths::config_dir()`;测试传各自的临时目录。
    pub fn open_in(dir: &Path) -> Self {
        let loaded = ProjectTabs::load(&dir.join(paths::PROJECT_TABS_FILE));
        let (host, notice) = match ProjectHost::open(&dir.join(paths::PROJECTS_DB_FILE)) {
            Ok(host) => (Some(host), None),
            Err(e) => (
                None,
                Some((NoticeKind::Unavailable, format!("项目功能不可用: {e}"))),
            ),
        };
        Self {
            host,
            saved: loaded.tabs.clone(),
            tabs: loaded.tabs,
            tabs_path: dir.join(paths::PROJECT_TABS_FILE),
            tabs_blocked: loaded.save_blocked,
            registry: Vec::new(),
            missing: HashSet::new(),
            hovered: None,
            notice,
        }
    }

    /// 项目/记忆服务的进程内装配(Project 面板直接用它,与注册表共享同一个数据库文件)。
    pub fn project_host(&self) -> Option<&ProjectHost> {
        self.host.as_ref()
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
        self.notice.as_ref().map(|(_, text)| text.as_str())
    }

    /// 此刻是否有顶栏可交互控件被悬停。原生拖窗覆写读的是进程级原子量(由 [`Self::set_hovered`] 维护),
    /// 这个读取器给测试用。
    #[allow(dead_code)] // 只有测试读它
    pub fn topbar_control_hovered(&self) -> bool {
        self.hovered.is_some()
    }

    fn set_hovered(&mut self, hovered: Option<i64>) {
        self.hovered = hovered;
        #[cfg(target_os = "macos")]
        bytehost_chrome::platform_macos::set_topbar_control_hovered(hovered.is_some());
    }

    fn set_notice(&mut self, kind: NoticeKind, text: String) {
        // "项目功能不可用"是根因,别的提示不盖掉它。
        if matches!(self.notice, Some((NoticeKind::Unavailable, _))) {
            return;
        }
        self.notice = Some((kind, text));
    }

    fn clear_notice(&mut self, kind: NoticeKind) {
        if self.notice.as_ref().is_some_and(|(k, _)| *k == kind) {
            self.notice = None;
        }
    }

    /// 向注册表取最新列表(启动时与每次打开后调用)。
    pub fn refresh(&self) -> Task<Message> {
        let Some(host) = &self.host else {
            return Task::none();
        };
        let service = host.projects.clone();
        Task::perform(
            async move {
                let projects = service.list_projects().await.map_err(|e| e.to_string())?;
                // 目录是否还在要 stat 磁盘,放到阻塞线程池里做,不占界面线程。
                tokio::task::spawn_blocking(move || Listing::scan(projects))
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::Listed,
        )
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        if let Message::HoverEnter(id) = msg {
            self.set_hovered(Some(id));
            return Task::none();
        }
        if let Message::HoverLeave(id) = msg {
            // 从一个页签的关闭按钮移到它的名字上时,两边的 enter/exit 顺序不保证:
            // 只在离开的正是当前悬停目标时才清,免得"离开关闭按钮"把刚进入的页签也清掉。
            if self.hovered == Some(id) {
                self.set_hovered(None);
            }
            return Task::none();
        }
        match msg {
            Message::PickFolder => {
                self.clear_notice(NoticeKind::Open);
                Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("选择项目文件夹")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::FolderPicked,
                )
            }
            Message::FolderPicked(None) => Task::none(),
            Message::FolderPicked(Some(path)) => {
                let Some(host) = &self.host else {
                    return Task::none();
                };
                let service = host.projects.clone();
                let path = path.to_string_lossy().into_owned();
                Task::perform(
                    async move { service.open_project(path).await.map_err(|e| e.to_string()) },
                    Message::Opened,
                )
            }
            Message::Opened(Ok(Some(info))) => {
                self.clear_notice(NoticeKind::Open);
                self.tabs.open_tab(info.id);
                // `open_project` 刚确认过它是目录:之前标灰的,现在不灰,不必等后面的 `Listed`。
                self.missing.remove(&info.id);
                // 立刻进快照:页签一出现就有名字,不等后面的 `Listed`。
                self.registry.retain(|p| p.id != info.id);
                self.registry.insert(0, info);
                self.save_tabs();
                self.refresh()
            }
            Message::Opened(Ok(None)) => {
                self.set_notice(NoticeKind::Open, "所选路径不是可打开的文件夹".into());
                Task::none()
            }
            Message::Opened(Err(e)) => {
                self.set_notice(NoticeKind::Open, format!("打开项目失败: {e}"));
                Task::none()
            }
            Message::Listed(Ok(listing)) => {
                let registered: HashSet<i64> = listing.projects.iter().map(|p| p.id).collect();
                self.tabs.prune(&registered);
                self.missing = listing.missing;
                self.registry = listing.projects;
                self.clear_notice(NoticeKind::List);
                self.save_tabs();
                Task::none()
            }
            Message::Listed(Err(e)) => {
                // 读失败不等于"没有项目":保留现有页签与快照,只提示。
                self.set_notice(NoticeKind::List, format!("读取项目列表失败: {e}"));
                Task::none()
            }
            // 已在函数开头处理(不落盘、不碰注册表):这里只是让穷尽匹配完整。
            Message::HoverEnter(_) | Message::HoverLeave(_) => Task::none(),
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
        // 没变化就不写盘:每次注册表刷新都会走到这里。
        if self.tabs_blocked || self.tabs == self.saved {
            return;
        }
        match self.tabs.save(&self.tabs_path) {
            Ok(()) => {
                self.saved = self.tabs.clone();
                self.clear_notice(NoticeKind::Save);
            }
            Err(e) => self.set_notice(NoticeKind::Save, format!("项目页签未保存: {e}")),
        }
    }

    /// 测试用:直接开页签(`Opened` 的后续步骤是异步任务,单元测试跑不到)。
    #[cfg(test)]
    #[allow(dead_code)] // 只在测试里调用
    pub fn open_tab_for_test(&mut self, id: i64) {
        self.tabs.open_tab(id);
        self.save_tabs();
    }

    /// 测试用:`open_tab` 会顺带把新页签变成当前;要单独摆布"当前项目"时用它。
    #[cfg(test)]
    #[allow(dead_code)] // 只在测试里调用
    pub fn add_tab_for_test(&mut self, id: i64) {
        self.tabs.add_tab(id);
        self.save_tabs();
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
            let label_color = if self.missing.contains(id) || self.tabs.active() != Some(*id) {
                dim
            } else {
                tokens.body
            };
            let name = if self.missing.contains(id) {
                format!("{}(目录已不存在)", project.name)
            } else {
                project.name.clone()
            };
            // 整张页签(名字 + 关闭按钮)算一个顶栏可交互控件:悬停期间原生拖窗让路,
            // 否则落在原生拖窗带里的按下会变成拖窗,点不动页签。离开时清掉(见 `update`)。
            let tab = row![
                button(text(name).size(font::body()).color(label_color))
                    .style(button::text)
                    .on_press(Message::Activate(*id)),
                button(text("×").size(font::body()).color(dim))
                    .style(button::text)
                    .on_press(Message::Close(*id)),
            ]
            .align_y(Alignment::Center);
            tabs = tabs.push(
                mouse_area(tab)
                    .on_enter(Message::HoverEnter(*id))
                    .on_exit(Message::HoverLeave(*id)),
            );
        }
        if self.tabs.open().is_empty() {
            tabs = tabs.push(text("无项目").size(font::body()).color(dim));
        }
        let plus = button(icons::view(IconKind::Plus, icon_size::rail(), dim))
            .style(button::text)
            .on_press_maybe(self.host.as_ref().map(|_| Message::PickFolder));
        let plus = mouse_area(plus)
            .on_enter(Message::HoverEnter(ADD_TAB_HOVER))
            .on_exit(Message::HoverLeave(ADD_TAB_HOVER));
        let hint = if self.host.is_some() {
            "打开项目文件夹"
        } else {
            "项目功能不可用"
        };
        tabs = tabs.push(tooltip(
            plus,
            iced::widget::container(text(hint).size(font::body()))
                .padding(6)
                .style(iced::widget::container::rounded_box),
            tooltip::Position::Bottom,
        ));
        if let Some((_, full)) = &self.notice {
            // 顶栏只放截短的一行,全文在悬停提示里,免得长错误把"＋"和页签挤出顶栏。
            let short = text(shorten(full, NOTICE_MAX_CHARS))
                .size(font::body())
                .color(tokens.gold);
            let tip = iced::widget::container(text(full.clone()).size(font::body()))
                .padding(6)
                .style(iced::widget::container::rounded_box);
            tabs = tabs.push(tooltip(short, tip, tooltip::Position::Bottom));
        }
        tabs.into()
    }
}

#[cfg(test)]
mod tests;
