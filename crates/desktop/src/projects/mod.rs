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
use iced::widget::{button, container, mouse_area, row, text, tooltip};
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
    /// 页签被按住(拖拽排序的起点)。
    DragStart(i64),
    /// 拖拽中光标的窗口坐标:算落点。
    DragMove(iced::Point),
    /// 松开:拖拽结束,取消页签标记。
    DragEnd,
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
pub const TAB_SPACING: f32 = 6.0;
/// 页签行自己(不是某张页签)的悬停标记:只用来区分"鼠标在行上",不参与拖窗让路。
const ROW_HOVER: i64 = -2;
const ADD_TAB_HOVER: i64 = -1;

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 拖拽时光标 x 的原点:`DragMove` 来自整行的 `mouse_area`,坐标相对页签行自己,第一张页签的左缘就是 0。
const ROW_LEFT: f32 = 0.0;

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
    /// 正被按住拖动的页签(拖拽排序)。
    dragging: Option<i64>,
    /// 拖拽开始时各页签宽度(按标签字数估):落点用它们的中点换算。精确的像素级命中
    /// 要等 iced 暴露 `operate` 之外的消息,眼下不值得为它引入一层自绘。
    drag_row: Option<(f32, Vec<f32>)>,
    notice: Option<(NoticeKind, String)>,
}

impl ProjectsState {
    /// 在数据目录 `dir` 里打开注册表与页签文件。生产传 `paths::config_dir()`;测试传各自的临时目录。
    pub fn open_in(dir: &Path) -> Self {
        Self::open_with(
            dir,
            &dir.join(paths::PROJECTS_DB_FILE),
            &dir.join("projects.db"),
        )
    }

    /// 页签文件在 `dir`,注册表在 `db_path`(生产:`projects_db_path()`,与 `diggerd` 同一文件),
    /// 并先把 `legacy_db`(旧 `projects.db`)迁入。迁移失败显示"项目功能不可用",不静默回退。
    pub fn open_with(dir: &Path, db_path: &Path, legacy_db: &Path) -> Self {
        let loaded = ProjectTabs::load(&dir.join(paths::PROJECT_TABS_FILE));
        let (host, notice) = match ProjectHost::open_in(db_path, legacy_db) {
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
            dragging: None,
            drag_row: None,
            notice,
        }
    }

    /// 项目/记忆服务的进程内装配(Project 面板直接用它,与注册表共享同一个数据库文件)。
    pub fn project_host(&self) -> Option<&ProjectHost> {
        self.host.as_ref()
    }

    /// 是否正按住某张页签拖动(产品据此决定要不要订阅全局鼠标释放)。
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
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

    /// 换掉注册表快照里的一条(改名成功时用库返回的新 `ProjectInfo`)。
    pub fn replace_registered(&mut self, info: dozer_model::ProjectInfo) {
        match self.registry.iter_mut().find(|p| p.id == info.id) {
            Some(slot) => *slot = info,
            None => self.registry.insert(0, info),
        }
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
        if let Message::DragStart(id) = msg {
            self.dragging = Some(id);
            // 页签行从左栏面板区左端开始;这里只用来把光标换算成"跨过几张页签",
            // 不参与布局。
            let widths = self
                .tabs
                .open()
                .iter()
                .filter_map(|id| self.registry.iter().find(|p| p.id == *id))
                .map(|p| Self::tab_width(&p.name))
                .collect();
            self.drag_row = Some((ROW_LEFT, widths));
            return Task::none();
        }
        if let Message::DragEnd = msg {
            self.dragging = None;
            self.drag_row = None;
            return Task::none();
        }
        if let Message::DragMove(cursor) = msg {
            let Some(from) = self.dragging else {
                return Task::none();
            };
            if self.drag_row.is_none() {
                // 拖拽开始那一帧还没渲染过:用渲染时记下的左端和当前页签宽度补上。
                let widths = self
                    .tabs
                    .open()
                    .iter()
                    .filter_map(|id| self.registry.iter().find(|p| p.id == *id))
                    .map(|p| Self::tab_width(&p.name))
                    .collect();
                self.drag_row = Some((ROW_LEFT, widths));
            }
            if let Some(to) = self.landing_index(from, cursor.x)
                && self.tabs.move_tab(from, to)
            {
                self.save_tabs();
                // 顺序变了,宽度表跟着换(名字与顺序都取自当前页签)。
                let widths = self
                    .tabs
                    .open()
                    .iter()
                    .filter_map(|id| self.registry.iter().find(|p| p.id == *id))
                    .map(|p| Self::tab_width(&p.name))
                    .collect();
                self.drag_row = Some((ROW_LEFT, widths));
            }
            return Task::none();
        }
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
            Message::HoverEnter(_)
            | Message::HoverLeave(_)
            | Message::DragStart(_)
            | Message::DragMove(_)
            | Message::DragEnd => Task::none(),
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

    /// 拖拽落点:光标落在第几张页签的右半边,就插到它后面(返回值是移走自己之后的
    /// 下标,由 `ProjectTabs::move_tab` 用)。
    fn landing_index(&self, from: i64, x: f32) -> Option<usize> {
        let (left, widths) = self.drag_row.as_ref()?;
        let at = self.tabs.open().iter().position(|t| *t == from)?;
        let mut edge = *left;
        let mut index = 0;
        for width in widths {
            if x > edge + width * 0.5 {
                index += 1;
            }
            edge += width + TAB_SPACING;
        }
        Some(
            index
                .min(widths.len())
                .saturating_sub(usize::from(index > at)),
        )
    }

    /// 页签宽度估值(字号 × 字数 + 关闭按钮 + 内边距),只用来分"拖过了几张"。
    fn tab_width(name: &str) -> f32 {
        font::body() as f32 * 0.6 * name.chars().count() as f32 + 28.0
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
            let tab: Element<'_, Message> = if self.dragging == Some(*id) {
                use iced_widget::core::Border;
                // 被拖动的页签画成"抬起"的样子,让拖拽有反馈。
                container(tab)
                    .padding([1, 2])
                    .style(move |_t| iced::widget::container::Style {
                        background: Some(tokens.panel.into()),
                        border: Border {
                            radius: 4.0.into(),
                            width: 1.0,
                            color: tokens.gold,
                        },
                        ..Default::default()
                    })
                    .into()
            } else {
                tab.into()
            };
            tabs = tabs.push(
                mouse_area(tab)
                    .on_enter(Message::HoverEnter(*id))
                    .on_exit(Message::HoverLeave(*id))
                    .on_press(Message::DragStart(*id)),
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
        // 拖拽排序:拖拽中整行挂在 `mouse_area` 上,光标滑进页签间隙也能收到移动事件
        // (单个页签的 `on_move` 一离开自己就断,坐标系也与整行不同,所以页签自己不挂 `on_move`);
        // 松手结束拖拽(行外松手由产品的全局鼠标释放订阅兜底)。页签被拖出顺序之外时
        // 由 `landing_index` 按"跨过几张页签"换算。
        let row = mouse_area(tabs).on_enter(Message::HoverEnter(ROW_HOVER));
        if self.dragging.is_some() {
            row.on_move(Message::DragMove)
                .on_release(Message::DragEnd)
                .into()
        } else {
            row.into()
        }
    }
}

#[cfg(test)]
mod tests;
