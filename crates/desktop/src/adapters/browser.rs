//! 浏览器面板的项目级宿主:每个项目一份库 `State`(页签 / 地址栏 / 收藏夹缓存),库负责状态机与视图,
//! 这里只接 Digger 的消息、异步通道、地址栏焦点探针和右键菜单。
//!
//! 网页本身由原生子视图承载(`browser_native`,Task 7);这里不碰 wry。后退 / 前进 / 刷新只是记下来
//! ([`BrowserPanel::take_nav`]),由原生层取走执行。
use std::collections::HashMap;
use std::sync::Arc;

use bytehost_chrome::{BodyArea, Rect};
use bytehost_core::LogicalRect;
use bytehost_desktop::web_child::WebTabSpec;
use dozer_model::ProjectInfo;
use dozer_panel_browser::{Message, NavAction, State, addr_field_id};
use dozer_services::BookmarkService;
use iced::widget::{column, container, operation};
use iced::{Border, Element, Length, Task, window};

use super::browser_addr_menu::{AddrEdit, pasted_addr};
use super::browser_display::DiggerBrowserDisplay;
use super::browser_native::{self, BrowserNative, Desired, Outcome, Sidebar, webview_rect};
use super::notice::NoticeBar;
use crate::product::Msg;

/// 收藏夹侧栏展开时网页区占的比例(v1 固定,分隔条不可拖,见 `browser_display`)。
const BOOKMARKS_CONTENT_SPLIT: f32 = 0.72;

/// 浏览器是单栏面板(没有配对规则),骨架给的 `mirrored` 恒为假:收藏夹在网页后面。
/// 视图与网页几何共用这一个值,免得两边对"侧栏在哪"各算各的。
const SIDEBAR_MIRRORED: bool = false;

/// 本帧同步原生层需要的输入(都是产品壳算好的)。
pub struct NativeInputs {
    /// 当前项目;没有、或正在关闭时为 `None`(销毁全部网页)。
    pub project_id: Option<i64>,
    /// 面板内容矩形;面板没被选中时为 `None`(停放)。
    pub content: Option<Rect>,
    /// 窗口没最小化、没在关闭。
    pub window_active: bool,
    /// 有 iced 浮层会被网页盖住。
    pub overlay_open: bool,
}

pub struct BrowserPanel {
    states: HashMap<i64, State>,
    service: Arc<dyn BookmarkService>,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<(i64, Message)>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>>,
    nav: Vec<(i64, NavAction)>,
    display: DiggerBrowserDisplay,
    pub native: BrowserNative,
    /// 页面没响应 / 网址被挡这类一次性提示。显示在页签栏上方,所以占的高度要从网页矩形里扣掉。
    pub notice: NoticeBar,
}

/// 地址栏真实焦点变化落到库状态上;返回「这一次要不要对地址栏做一次全选」。
///
/// 只在**变化**时写(探针每 16ms 来一次,没变就什么都不做);全选只在获得焦点那一次发,
/// 库把它做成一次性标志(消费即复位),所以同一次聚焦不会重复全选。
pub fn apply_addr_focus(state: &mut State, focused: bool) -> bool {
    if state.addr_focused() == focused {
        return false;
    }
    state.set_addr_focused(focused);
    focused && state.take_addr_select_all_pending()
}

/// 地址栏此刻屏幕上显示的那一份文本:编辑中是草稿,否则是当前页签网址。
fn displayed_addr(state: &State) -> String {
    if state.addr_focused() {
        state.addr_text().to_owned()
    } else {
        state
            .desired_webviews()
            .into_iter()
            .find(|spec| spec.visible)
            .map(|spec| spec.url)
            .unwrap_or_default()
    }
}

impl BrowserPanel {
    pub fn new(service: Arc<dyn BookmarkService>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            states: HashMap::new(),
            service,
            handle: None,
            tx,
            rx: Some(rx),
            nav: vec![],
            display: DiggerBrowserDisplay,
            native: BrowserNative::new(),
            notice: NoticeBar::default(),
        }
    }

    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<(i64, Message)>> {
        self.rx.take()
    }

    pub fn state(&self, pid: i64) -> Option<&State> {
        self.states.get(&pid)
    }

    pub fn ensure_state(&mut self, project: &ProjectInfo) -> &mut State {
        self.states.entry(project.id).or_default()
    }

    /// 面板第一次对这个项目可见:建状态并刷新收藏夹(全局 + 本项目)。
    pub fn activate(&mut self, project: &ProjectInfo) -> Task<Msg> {
        let pid = project.id;
        let fresh = !self.states.contains_key(&pid);
        self.ensure_state(project);
        if fresh && let Some(handle) = self.handle.as_ref() {
            let tx = self.tx.clone();
            dozer_panel_browser::request_bookmarks_refresh(
                &self.states[&pid].requests,
                Some(pid),
                &self.service,
                handle,
                move |m| {
                    let _ = tx.send((pid, m));
                },
            );
        }
        Task::none()
    }

    pub fn close_project(&mut self, pid: i64) {
        self.states.remove(&pid);
        self.nav.retain(|(id, _)| *id != pid);
    }

    pub fn dismiss_notice(&mut self) {
        self.notice.dismiss();
    }

    /// 提示条占的高度(没有提示为 0)。网页矩形要整体下移这么多。
    pub fn notice_height(&self) -> f32 {
        if self.notice.is_some() {
            byteui::theme::geometry::preview_chrome_top_px()
        } else {
            0.
        }
    }

    /// 本帧期望的原生状态(纯函数,不碰窗口):几何、页签清单、停放条件都在这里算。
    fn desired(&self, inputs: &NativeInputs) -> Desired {
        let state = inputs.project_id.and_then(|pid| self.states.get(&pid));
        let rect = match (state, inputs.content) {
            (Some(state), Some(content)) => {
                let sidebar = state.bookmarks_open().then_some(Sidebar {
                    content_fraction: BOOKMARKS_CONTENT_SPLIT,
                    mirrored: SIDEBAR_MIRRORED,
                });
                let r = webview_rect(
                    content,
                    self.display_padding(),
                    self.notice_height(),
                    sidebar,
                );
                LogicalRect {
                    x: f64::from(r.x),
                    y: f64::from(r.y),
                    width: f64::from(r.w),
                    height: f64::from(r.h),
                }
            }
            // 骨架对"没有可用矩形"的约定占位;原生层见到它就不显示子视图。
            _ => LogicalRect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
        };
        Desired {
            // 项目还没建状态(面板从没激活过)等同没有项目:不建池。
            project_id: state.and(inputs.project_id),
            shown: inputs.content.is_some() && inputs.window_active,
            overlay_open: inputs.overlay_open,
            rect,
            tabs: state
                .map(|s| {
                    s.desired_webviews()
                        .into_iter()
                        .map(|spec| WebTabSpec {
                            id: spec.id,
                            url: spec.url,
                            visible: spec.visible,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// 取走排队的导航:只对"现在显示的项目"的激活页签执行,别的项目 / 没有页签的丢弃。
    fn nav_targets(&mut self, project_id: Option<i64>) -> Vec<(usize, NavAction)> {
        let active = project_id
            .and_then(|pid| self.states.get(&pid))
            .and_then(State::active_webview_id);
        self.take_nav()
            .into_iter()
            .filter_map(|(pid, action)| {
                (Some(pid) == project_id)
                    .then_some(active)
                    .flatten()
                    .map(|tab| (tab, action))
            })
            .collect()
    }

    /// 让原生层收敛到库的页签清单,并执行排队的后退 / 前进 / 刷新。
    pub fn sync_native(&mut self, window: window::Id, inputs: NativeInputs) -> Task<Msg> {
        let desired = self.desired(&inputs);
        let navs = self.nav_targets(desired.project_id);
        let mut tasks = vec![self.native.sync(window, desired)];
        for (tab, action) in navs {
            tasks.push(self.native.nav(window, tab, action));
        }
        Task::batch(tasks).map(Msg::BrowserNative)
    }

    /// 处理原生层事件。库消息交给 `update`(`projects` 按项目 id 取项目信息),提示进提示条。
    pub fn update_native(
        &mut self,
        event: browser_native::Event,
        project_of: impl Fn(i64) -> Option<ProjectInfo>,
    ) -> Task<Msg> {
        match self.native.update(event) {
            Some(Outcome::Lib(pid, message)) => {
                let project = project_of(pid);
                self.update(pid, message, project.as_ref())
            }
            Some(Outcome::Notice(text)) => {
                self.notice.push(text);
                Task::none()
            }
            None => Task::none(),
        }
    }

    fn display_padding(&self) -> iced::Padding {
        use dozer_panel_browser::BrowserDisplay;
        self.display.region().padding
    }

    /// 取走后退 / 前进 / 刷新请求,交给原生层对对应项目的激活页签执行。
    pub fn take_nav(&mut self) -> Vec<(i64, NavAction)> {
        std::mem::take(&mut self.nav)
    }

    pub fn update(&mut self, pid: i64, msg: Message, project: Option<&ProjectInfo>) -> Task<Msg> {
        let mut inner = &msg;
        while let Message::AsyncResult(_, message) = inner {
            inner = message;
        }
        match inner {
            // 库对这条消息什么也不做;右键菜单由宿主自己弹。
            Message::TextInputMenuOpen(_) => return self.open_addr_menu(pid),
            Message::Nav(action) => {
                if self.states.contains_key(&pid) {
                    self.nav.push((pid, *action));
                }
                return Task::none();
            }
            // v1 不做页签拖拽换位和收藏夹分隔条拖拽(Digger 的壳没有对应通路)。
            Message::DragHover(_) | Message::ColumnDragStart => return Task::none(),
            _ => {}
        }
        self.update_library(pid, msg, project)
    }

    fn update_library(
        &mut self,
        pid: i64,
        msg: Message,
        project: Option<&ProjectInfo>,
    ) -> Task<Msg> {
        // 结果自带的作用域优先于宿主信封上写的项目。
        let pid = msg.result_project().unwrap_or(pid);
        if project.is_none_or(|p| p.id != pid) {
            return Task::none();
        }
        let (Some(state), Some(handle)) = (self.states.get_mut(&pid), self.handle.as_ref()) else {
            return Task::none();
        };
        let tx = self.tx.clone();
        dozer_panel_browser::update(state, msg, Some(pid), &self.service, handle, move |m| {
            let _ = tx.send((pid, m));
        });
        Task::none()
    }

    /// 每帧探针(16ms,面板可见时):推进悬停动画,并问 iced 地址栏是否持有真实焦点。
    pub fn focus_frame(&mut self, pid: i64) -> Task<Msg> {
        let Some(state) = self.states.get_mut(&pid) else {
            return Task::none();
        };
        state.advance_hover_anims();
        let token = state.requests.begin(Some(pid), "host-addr-focus");
        operation::is_focused(addr_field_id())
            .map(move |focused| Msg::BrowserAddrFocused(pid, token.clone(), focused))
    }

    pub fn on_addr_focused(
        &mut self,
        pid: i64,
        token: bytehost_core::request::RequestToken,
        focused: bool,
    ) -> Task<Msg> {
        let Some(state) = self.states.get_mut(&pid) else {
            return Task::none();
        };
        // 过期的探针结果(期间菜单动作已强制聚焦、或面板被隐藏)一律丢弃。
        if !state.requests.accepts(&token) {
            return Task::none();
        }
        if apply_addr_focus(state, focused) {
            operation::select_all(addr_field_id())
        } else {
            Task::none()
        }
    }

    /// 面板不可见 / 项目不是当前项目时,地址栏焦点不能留着(否则终端键盘转发会被一个看不见的输入框堵住)。
    pub fn reset_inactive_focus(&mut self, active: Option<i64>) {
        for (&pid, state) in &mut self.states {
            if Some(pid) != active {
                state.set_addr_focused(false);
                // 让还没送达的焦点查询作废。
                state.requests.begin(Some(pid), "host-addr-focus");
            }
        }
    }

    /// 强制把地址栏置为「正在编辑」(菜单动作要先聚焦,聚焦会把当前网址预填进草稿),
    /// 并作废在途的探针结果:否则一个在聚焦之前算出的 `false` 会把刚预填的草稿清掉。
    fn force_addr_focus(&mut self, pid: i64) {
        if let Some(state) = self.states.get_mut(&pid) {
            state.set_addr_focused(true);
            // 全选由菜单动作自己决定要不要做,不留给下一次聚焦。
            let _ = state.take_addr_select_all_pending();
            state.requests.begin(Some(pid), "host-addr-focus");
        }
    }

    /// 地址栏右键:在光标处弹原生菜单(macOS)。非 macOS 没有对应的 iced 浮层,保持空操作。
    fn open_addr_menu(&self, pid: i64) -> Task<Msg> {
        #[cfg(target_os = "macos")]
        {
            let Some(state) = self.states.get(&pid) else {
                return Task::none();
            };
            let at = crate::CURRENT_CURSOR.get();
            let items = super::browser_addr_menu::menu_items(pid, displayed_addr(state).is_empty());
            match byteui::native_menu::show(items, (at.x, at.y)) {
                Some(message) => Task::done(message),
                None => Task::none(),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = pid;
            Task::none()
        }
    }

    /// 执行菜单动作。整框语义:剪切 / 复制作用于整个显示文本,粘贴追加(没聚焦时替换),全选精确。
    pub fn addr_edit(
        &mut self,
        pid: i64,
        edit: AddrEdit,
        project: Option<&ProjectInfo>,
    ) -> Task<Msg> {
        let Some(state) = self.states.get(&pid) else {
            return Task::none();
        };
        let displayed = displayed_addr(state);
        let was_focused = state.addr_focused();
        let field = addr_field_id;
        match edit {
            AddrEdit::Copy => iced::clipboard::write(displayed),
            AddrEdit::Cut => {
                self.force_addr_focus(pid);
                let cleared = self.update(pid, Message::AddrInput(String::new()), project);
                Task::batch([
                    iced::clipboard::write(displayed),
                    cleared,
                    operation::focus(field()),
                ])
            }
            AddrEdit::SelectAll => {
                self.force_addr_focus(pid);
                Task::batch([operation::focus(field()), operation::select_all(field())])
            }
            AddrEdit::Paste => iced::clipboard::read()
                .map(move |text| Msg::BrowserAddrEdit(pid, AddrEdit::Pasted(text))),
            AddrEdit::Pasted(None) => Task::none(),
            AddrEdit::Pasted(Some(text)) => {
                self.force_addr_focus(pid);
                let next = pasted_addr(&displayed, &text, !was_focused);
                let updated = self.update(pid, Message::AddrInput(next), project);
                Task::batch([
                    updated,
                    operation::focus(field()),
                    operation::move_cursor_to_end(field()),
                ])
            }
        }
    }

    pub fn view<'a>(&'a self, project: Option<&ProjectInfo>, _area: &BodyArea) -> Element<'a, Msg> {
        let empty = || {
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let Some(project) = project else {
            return empty();
        };
        let Some(state) = self.states.get(&project.id) else {
            return empty();
        };
        let pid = project.id;
        let page: Element<'a, Msg> = dozer_panel_browser::view(
            &self.display,
            state,
            Some(pid),
            BOOKMARKS_CONTENT_SPLIT,
            Length::Fill,
            Border::default(),
            SIDEBAR_MIRRORED,
        )
        .map(move |message| Msg::Browser(pid, message));
        match self.notice.view(Msg::BrowserNoticeDismiss) {
            Some(notice) => column![
                container(notice)
                    .width(Length::Fill)
                    .height(Length::Fixed(self.notice_height())),
                page
            ]
            .into(),
            None => page,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dozer_model::{BookmarkInfo, BookmarkScope};
    use dozer_panel_browser::BrowserDisplay;
    use dozer_services::{BoxFuture, ServiceError};
    use std::cell::Cell;

    struct NoBookmarks;
    impl BookmarkService for NoBookmarks {
        fn list_bookmarks(
            &self,
            _: Option<i64>,
        ) -> BoxFuture<'_, Result<Vec<BookmarkInfo>, ServiceError>> {
            Box::pin(async { Ok(vec![]) })
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

    fn project(id: i64) -> ProjectInfo {
        ProjectInfo {
            id,
            path: format!("/tmp/project-{id}"),
            name: format!("project {id}"),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    fn panel() -> BrowserPanel {
        let mut panel = BrowserPanel::new(Arc::new(NoBookmarks));
        panel.attach_handle(tokio::runtime::Handle::current());
        panel
    }

    /// 地址栏焦点只在变化时写入库状态;获得焦点那一次请求一次全选,之后(即使探针还在报 true)不再请求。
    #[test]
    fn addr_focus_bridge_sets_state_once_per_change() {
        let mut state = State::default();
        assert!(!apply_addr_focus(&mut state, false), "没变化:什么都不做");
        assert!(!state.addr_focused());
        assert!(apply_addr_focus(&mut state, true), "获得焦点:请求全选");
        assert!(state.addr_focused());
        assert!(!apply_addr_focus(&mut state, true), "仍聚焦:不再变化");
        assert!(!apply_addr_focus(&mut state, false), "失焦不全选");
        assert!(!state.addr_focused());
        assert!(apply_addr_focus(&mut state, true), "再次聚焦又全选一次");
    }

    #[test]
    fn select_all_runs_once_after_focus_gain_and_not_again() {
        let mut state = State::default();
        assert!(apply_addr_focus(&mut state, true));
        // 全选标志已被消费:库里不再有待办的全选。
        assert!(!state.take_addr_select_all_pending());
        for _ in 0..3 {
            assert!(!apply_addr_focus(&mut state, true));
        }
    }

    /// 在途探针结果在菜单强制聚焦之后到达:必须被丢弃,不能把刚预填的草稿清掉。
    #[tokio::test]
    async fn a_probe_result_from_before_a_forced_focus_is_dropped() {
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let stale = panel.states[&1].requests.begin(Some(1), "host-addr-focus");
        panel.force_addr_focus(1);
        assert!(panel.states[&1].addr_focused());
        let _ = panel.on_addr_focused(1, stale, false);
        assert!(
            panel.states[&1].addr_focused(),
            "过期的 false 不得清掉强制聚焦"
        );
    }

    #[tokio::test]
    async fn the_address_bar_shows_the_url_until_focused_then_the_draft() {
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(1, Message::OpenUrl("https://a.com/".into()), Some(&p));
        assert_eq!(displayed_addr(&panel.states[&1]), "https://a.com/");
        panel.force_addr_focus(1);
        assert_eq!(displayed_addr(&panel.states[&1]), "https://a.com/");
        let _ = panel.update(1, Message::AddrInput("https://b".into()), Some(&p));
        assert_eq!(displayed_addr(&panel.states[&1]), "https://b");
    }

    #[tokio::test]
    async fn navigation_buttons_are_queued_for_the_native_layer_and_not_applied() {
        let mut panel = panel();
        let (a, b) = (project(1), project(2));
        panel.ensure_state(&a);
        panel.ensure_state(&b);
        let _ = panel.update(1, Message::Nav(NavAction::Back), Some(&a));
        let _ = panel.update(2, Message::Nav(NavAction::Refresh), Some(&b));
        // 项目没建状态:不排队。
        let _ = panel.update(3, Message::Nav(NavAction::Forward), Some(&project(3)));
        assert_eq!(
            panel.take_nav(),
            vec![(1, NavAction::Back), (2, NavAction::Refresh)]
        );
        assert!(panel.take_nav().is_empty(), "取走即清空");
    }

    #[tokio::test]
    async fn closing_a_project_drops_its_state_and_queued_navigation() {
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(1, Message::Nav(NavAction::Back), Some(&p));
        panel.close_project(1);
        assert!(panel.state(1).is_none());
        assert!(panel.take_nav().is_empty());
    }

    #[tokio::test]
    async fn a_message_for_a_project_that_is_not_open_is_ignored() {
        let mut panel = panel();
        let (open, closing) = (project(1), project(2));
        panel.ensure_state(&open);
        panel.ensure_state(&closing);
        // 项目 2 有状态但已经不是打开的项目(正在关闭):信封带着别的项目 / 没有项目,都不得改它。
        let _ = panel.update(2, Message::OpenUrl("https://x.com/".into()), Some(&open));
        let _ = panel.update(2, Message::OpenUrl("https://y.com/".into()), None);
        assert_eq!(panel.states[&2].tab_count(), 1);
        assert_eq!(displayed_addr(&panel.states[&2]), "about:blank");
        assert_eq!(displayed_addr(&panel.states[&1]), "about:blank");
        // 同一条消息发给真正打开的项目 2 才生效。
        let _ = panel.update(2, Message::OpenUrl("https://z.com/".into()), Some(&closing));
        assert_eq!(panel.states[&2].tab_count(), 2);
    }

    fn inputs(project_id: Option<i64>, content: Option<Rect>) -> NativeInputs {
        NativeInputs {
            project_id,
            content,
            window_active: true,
            overlay_open: false,
        }
    }

    fn content() -> Rect {
        Rect {
            x: 100.,
            y: 50.,
            w: 500.,
            h: 600.,
        }
    }

    #[tokio::test]
    async fn the_native_geometry_follows_the_notice_and_the_open_sidebar() {
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let plain = panel.desired(&inputs(Some(1), Some(content())));
        assert!(plain.shown);
        panel.notice.push_quiet("页面没有响应");
        let with_notice = panel.desired(&inputs(Some(1), Some(content())));
        assert_eq!(
            with_notice.rect.y,
            plain.rect.y + f64::from(panel.notice_height())
        );
        assert!(panel.notice_height() > 0.);
        panel.dismiss_notice();
        let _ = panel.update(1, Message::BookmarksToggle, Some(&p));
        let with_sidebar = panel.desired(&inputs(Some(1), Some(content())));
        assert!(with_sidebar.rect.width < plain.rect.width);
        assert_eq!(with_sidebar.rect.x, plain.rect.x, "侧栏在网页后面");
    }

    #[tokio::test]
    async fn nothing_is_shown_without_content_a_project_state_or_an_active_window() {
        let mut panel = panel();
        let p = project(1);
        // 项目没建状态:不建池。
        assert_eq!(
            panel.desired(&inputs(Some(1), Some(content()))).project_id,
            None
        );
        panel.ensure_state(&p);
        // 面板没被选中(没有内容矩形):停放。
        let parked = panel.desired(&inputs(Some(1), None));
        assert_eq!(parked.project_id, Some(1));
        assert!(!parked.shown);
        assert!(parked.rect.width <= 1. && parked.rect.height <= 1.);
        // 窗口最小化 / 关闭中。
        let mut minimized = inputs(Some(1), Some(content()));
        minimized.window_active = false;
        assert!(!panel.desired(&minimized).shown);
        // 没有当前项目:销毁。
        assert_eq!(
            panel.desired(&inputs(None, Some(content()))).project_id,
            None
        );
    }

    #[tokio::test]
    async fn the_page_list_mirrors_the_librarys_tabs_with_only_the_active_one_visible() {
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let _ = panel.update(1, Message::OpenUrl("https://a.com/".into()), Some(&p));
        let d = panel.desired(&inputs(Some(1), Some(content())));
        assert_eq!(d.tabs.len(), 2);
        assert_eq!(d.tabs.iter().filter(|t| t.visible).count(), 1);
        assert!(d.tabs.last().unwrap().visible, "新开的页签是激活的");
        assert_eq!(d.tabs.last().unwrap().url, "https://a.com/");
    }

    #[tokio::test]
    async fn queued_navigation_only_reaches_the_shown_projects_active_tab() {
        let mut panel = panel();
        let (a, b) = (project(1), project(2));
        panel.ensure_state(&a);
        panel.ensure_state(&b);
        let _ = panel.update(1, Message::Nav(NavAction::Back), Some(&a));
        let _ = panel.update(2, Message::Nav(NavAction::Refresh), Some(&b));
        let active = panel.states[&1].active_webview_id().unwrap();
        assert_eq!(panel.nav_targets(Some(1)), vec![(active, NavAction::Back)]);
        // 项目 2 的刷新随这一轮被丢弃,不会等到它被显示时再突然执行。
        let _ = panel.update(2, Message::Nav(NavAction::Forward), Some(&b));
        assert!(panel.nav_targets(None).is_empty());
        assert!(panel.take_nav().is_empty());
    }

    #[tokio::test]
    async fn native_events_route_to_the_project_that_owns_the_generation() {
        use bytehost_desktop::web_child::WebEvent;
        let mut panel = panel();
        let (a, b) = (project(1), project(2));
        panel.ensure_state(&a);
        panel.ensure_state(&b);
        // 当前显示项目 2:迟到的项目 1 事件(旧一代)不得改任何状态。
        let _ = panel
            .native
            .prepare(&panel.desired(&inputs(Some(1), Some(content()))));
        let old = panel.native.generation_for_tests();
        let _ = panel
            .native
            .prepare(&panel.desired(&inputs(Some(2), Some(content()))));
        let tab = panel.states[&2].active_webview_id().unwrap();
        let lookup = |pid| (pid == 2).then(|| project(2));
        let _ = panel.update_native(
            browser_native::Event::Web(WebEvent::PageLoaded {
                generation: old,
                id: tab,
                url: "https://stale.com/".into(),
            }),
            lookup,
        );
        assert_eq!(displayed_addr(&panel.states[&2]), "about:blank");
        let now = panel.native.generation_for_tests();
        let _ = panel.update_native(
            browser_native::Event::Web(WebEvent::PageLoaded {
                generation: now,
                id: tab,
                url: "https://fresh.com/".into(),
            }),
            lookup,
        );
        assert_eq!(displayed_addr(&panel.states[&2]), "https://fresh.com/");
        // 导航无响应:进提示条,不改页签。
        let _ = panel.update_native(
            browser_native::Event::Web(WebEvent::NavigationStalled {
                generation: now,
                id: tab,
                url: "https://slow.com/".into(),
            }),
            lookup,
        );
        assert!(panel.notice.text().unwrap().contains("slow.com"));
        assert_eq!(displayed_addr(&panel.states[&2]), "https://fresh.com/");
    }

    /// 只有收藏夹侧栏展开时才会向宿主要分隔条。
    #[tokio::test]
    async fn bookmarks_divider_only_built_when_open() {
        struct Counting(Cell<usize>);
        impl BrowserDisplay for Counting {
            fn region(&self) -> dozer_panel_browser::BrowserRegion {
                DiggerBrowserDisplay.region()
            }
            fn locale(&self) -> &str {
                "zh-CN"
            }
            fn line_height_factor(&self) -> f32 {
                1.2
            }
            fn tab_divider<'a, M: 'a>(&self) -> Element<'a, M, iced::Theme, iced::Renderer> {
                DiggerBrowserDisplay.tab_divider()
            }
            fn tab<'a, M: Clone + 'a>(
                &self,
                v: dozer_panel_browser::TabView,
                on_select: M,
                on_close: M,
                title_hover: impl Fn(bool) -> M + 'a,
                close_hover: impl Fn(bool) -> M + 'a,
            ) -> Element<'a, M, iced::Theme, iced::Renderer> {
                DiggerBrowserDisplay.tab(v, on_select, on_close, title_hover, close_hover)
            }
            fn bookmarks_divider<'a, M: Clone + 'a>(
                &self,
                on_drag: M,
            ) -> Element<'a, M, iced::Theme, iced::Renderer> {
                self.0.set(self.0.get() + 1);
                DiggerBrowserDisplay.bookmarks_divider(on_drag)
            }
        }
        let mut panel = panel();
        let p = project(1);
        panel.ensure_state(&p);
        let display = Counting(Cell::new(0));
        let build = |panel: &BrowserPanel| {
            let _ = dozer_panel_browser::view(
                &display,
                &panel.states[&1],
                Some(1),
                BOOKMARKS_CONTENT_SPLIT,
                Length::Fill,
                Border::default(),
                false,
            );
        };
        build(&panel);
        assert_eq!(display.0.get(), 0, "侧栏关着:不要分隔条");
        let _ = panel.update(1, Message::BookmarksToggle, Some(&p));
        build(&panel);
        assert_eq!(display.0.get(), 1, "侧栏展开:恰好一条");
    }
}
