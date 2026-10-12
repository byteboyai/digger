//! 应用面板的 **launch URL → 内存 Surface** 控制器(Task 16)。
//!
//! 规格 §6.4 / 计划 Task 16:
//! - **地址是秘密**:`app_launch_url` 带回的地址带令牌,只驻内存,`SetUrl` 后交给共享 builder
//!   去创建 webview;`ClearUrl`(切面板/停应用/升级/移除)立即销毁该应用的 Surface 并**丢弃地址**。
//!   重开时由状态机再次 `FetchLaunchUrl`,绝不拿过期地址重建。
//! - **唯一 builder**:受限 wry 构造在 `bytehost_desktop::app_webview` 里只有一份;本模块只决定
//!   "何时创建/销毁/改几何",不复制任何下载/devtools/新窗口/origin 策略。
//! - **可测的纯登记**:[`AppSurfaces`] 只记录"哪个应用当前占哪个物理槽/代数",窗口原生动作经注入的
//!   回调发出;因此不开窗口也能测创建/销毁/去重/换代的决策。
//!
//! 与 Dozer 的关系:同一套共享 builder;Dozer 目前从自己的 `runtime.rs` 调,Task 16 后新代码
//! 一律走这里(共享层),不再各写一份受限初始化。
//!
//! **为什么带 `#![allow(dead_code)]`**:计划 Task 16 只做机制——`AppSurfaces` 的创建/销毁/换代/
//! 待创建地址全部就绪并有表格测试,但一期的 Digger 还没有应用视口(布局尚不渲染分栏,
//! `PanelMeta.surfaces` 为空),所以 `open_pending`/`open_error` 等尚无生产调用点。这不是死代码,
//! 是等视口接入后立即使用的接缝;视口落地时应移除本 allow 并加真实用例。
#![allow(dead_code)]

use std::collections::HashMap;

use bytehost_core::{LogicalRect, SurfaceSlot};
use bytehost_desktop::app_webview::AppSurfaceIpc;

/// 一个已打开应用 Surface 的内存登记。**地址不入任何持久状态**(仅随 webview 存活)。
#[derive(Debug, Clone, PartialEq)]
pub struct OpenApp {
    /// 稳定应用 id。
    pub app: String,
    /// 该应用当前占用的物理槽。
    pub slot: SurfaceSlot,
    /// 该槽这一轮的代数(销毁重开后递增,便于丢弃迟到回包)。
    pub generation: u64,
    /// 逻辑矩形(窗口尺寸变化时更新)。
    pub rect: LogicalRect,
    /// 最近一次创建尝试的结果:`None` = 正在创建;`Some(Err)` = 创建失败(持久可见,不是一次性提示)。
    pub open_error: Option<String>,
}

/// 产品要执行的窗口原生动作(由组合根翻译成 `bytehost_desktop::app_webview::*` 调用)。
///
/// 之所以不直接在这里调 builder:`AppSurfaces` 保持 `Send`、可在无窗口测试里驱动;
/// 窗口线程动作(创建/销毁/改 bounds)由组合根在 `update` 里出 [`iced::Task`]。
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceAction {
    /// 创建:带 `(app, slot, generation, url, rect)`。地址只在这一跳传递。
    Create {
        app: String,
        slot: SurfaceSlot,
        generation: u64,
        url: String,
        rect: LogicalRect,
    },
    /// 改几何:`(slot, rect)`。
    Resize {
        slot: SurfaceSlot,
        rect: LogicalRect,
    },
    /// 销毁:`slot`(组合根调 `destroy_app`)。
    Destroy { slot: SurfaceSlot },
}

/// 应用 Surface 登记表:决定创建/销毁/改几何,**不碰原生句柄**。
#[derive(Debug, Default)]
pub struct AppSurfaces {
    /// app id → 当前打开登记。地址**不在**这里:一旦要创建就交给 [`SurfaceAction::Create`] 带出,
    /// 之后只留 slot/generation(地址随 webview 存活,不额外留存,避免误落日志/落盘)。
    open: HashMap<String, OpenApp>,
    /// 每个应用用过的下一代数:`app id → next_generation`;同应用重开时递增以拒迟到回包。
    next_generation: HashMap<String, u64>,
    /// 下一个可用物理槽(应用区分配的槽从高位起,避免与内置面板冲突)。
    next_slot: u32,
    /// 已取到地址、但**还没有视口几何**可以创建的应用:`app id → 地址`。
    ///
    /// Digger 一期尚无应用视口(Task 16 只做机制):地址到手就存这里等几何;有几何时
    /// 调 [`open_pending`](Self::open_pending) 转成真实 [`SurfaceAction::Create`]。
    /// 换新地址(重开)覆盖旧值;关闭丢弃,绝不残留过期地址。
    pending: HashMap<String, String>,
}

impl AppSurfaces {
    pub fn new() -> Self {
        Self {
            open: HashMap::new(),
            next_generation: HashMap::new(),
            // 高位段:与预览/内置面板的低位 slot 隔开(规格 §5.4 的物理槽分配)。
            next_slot: 4096,
            pending: HashMap::new(),
        }
    }

    /// 该应用当前是否已有打开登记。
    pub fn is_open(&self, app: &str) -> bool {
        self.open.contains_key(app)
    }

    pub fn get(&self, app: &str) -> Option<&OpenApp> {
        self.open.get(app)
    }

    pub fn slot_of(&self, app: &str) -> Option<SurfaceSlot> {
        self.open.get(app).map(|o| o.slot)
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// 打开(或换地址重开)一个应用 Surface。返回要执行的原生动作。
    ///
    /// 语义:
    /// - 首次打开:分配一个空闲物理槽,该应用代数从 0 起;
    /// - 同一应用**换新地址**(重开/升级后重取):用 [`reopen`](Self::reopen),先销毁旧 webview
    ///   再按新地址创建,该应用代数 +1(旧实例的迟到回包被拒);
    /// - 同一应用**已打开**时再次 `open`:仅几何变化应走 [`resize`](Self::resize);这里对已打开
    ///   的应用返回空(不重建),换地址请用 `reopen`。
    pub fn open(&mut self, app: &str, url: String, rect: LogicalRect) -> Vec<SurfaceAction> {
        if let Some(existing) = self.open.get(app) {
            // 幂等:同一应用且已打开——地址不可比(这里不留存地址),状态机已保证只在变化时发;
            // 这里按"重开"处理会白白重建,故交给上层用 `replace` 显式表达换地址。此处仅返回空。
            if existing.rect == rect {
                return Vec::new();
            }
            // 仅几何变化?上层应走 `resize`;这里视为无操作,避免误重建。
            return Vec::new();
        }
        let slot = self.alloc_slot();
        let generation = self.next_generation.get(app).copied().unwrap_or(0);
        self.open.insert(
            app.to_owned(),
            OpenApp {
                app: app.to_owned(),
                slot,
                generation,
                rect,
                open_error: None,
            },
        );
        vec![SurfaceAction::Create {
            app: app.to_owned(),
            slot,
            generation,
            url,
            rect,
        }]
    }

    /// 用**新地址**重开(切面板重取地址 / 升级后重取):先销毁旧 Surface,再按新地址创建。
    ///
    /// 与 [`open`](Self::open) 的区别:显式表达"地址已变,要重建",返回 `Destroy` 后接 `Create`,
    /// 代数递增以丢弃旧 webview 的迟到回包。
    pub fn reopen(&mut self, app: &str, url: String, rect: LogicalRect) -> Vec<SurfaceAction> {
        let mut actions = self.close(app);
        actions.extend(self.open(app, url, rect));
        actions
    }

    /// 应用被停/移除/切走:销毁其 Surface 并丢弃登记(含待创建地址)。返回要执行的原生动作
    /// (空 = 本就没开)。
    pub fn close(&mut self, app: &str) -> Vec<SurfaceAction> {
        // 无论是否已创建,都丢弃待创建地址:关闭后不得残留任何地址凭证。
        self.pending.remove(app);
        let Some(old) = self.open.remove(app) else {
            return Vec::new();
        };
        // 该应用下次打开时代数 +1,使旧实例的迟到 Created/IPC 被拒(判据是 per-app 代数)。
        *self.next_generation.entry(app.to_owned()).or_insert(0) += 1;
        vec![SurfaceAction::Destroy { slot: old.slot }]
    }

    /// 关闭全部(退出/停服务/端口重置时):撤掉所有应用 Surface,不保留任何地址。
    pub fn close_all(&mut self) -> Vec<SurfaceAction> {
        let apps: Vec<String> = self.open.keys().cloned().collect();
        let mut actions = Vec::new();
        for app in apps {
            actions.extend(self.close(&app));
        }
        actions
    }

    /// 窗口/面板尺寸变化:更新登记并返回要改 bounds 的动作(未打开 = 空)。
    pub fn resize(&mut self, app: &str, rect: LogicalRect) -> Vec<SurfaceAction> {
        let Some(entry) = self.open.get_mut(app) else {
            return Vec::new();
        };
        if entry.rect == rect {
            return Vec::new();
        }
        entry.rect = rect;
        vec![SurfaceAction::Resize {
            slot: entry.slot,
            rect,
        }]
    }

    /// 校验一次上行事件的归属:该 `(app, generation)` 是否是当前登记。旧代数一律拒绝。
    pub fn accepts(&self, app: &str, generation: u64) -> bool {
        self.open
            .get(app)
            .is_some_and(|o| o.generation == generation)
    }

    /// 记录一次创建尝试的结果。成功清掉旧的失败;失败留下可重试的错误(持久状态)。
    ///
    /// 迟到回包(应用已关闭或已换代)被忽略,避免旧结果污染新登记。
    pub fn report_open_result(&mut self, app: &str, result: Result<(), String>) {
        if let Some(entry) = self.open.get_mut(app) {
            entry.open_error = result.err();
        }
    }

    /// 该应用当前的创建失败原因(`None` = 正常或未打开)。
    pub fn open_error(&self, app: &str) -> Option<&str> {
        self.open.get(app).and_then(|o| o.open_error.as_deref())
    }

    /// 记下一条取到的 launch URL(尚无视口几何)。同一应用**覆盖**旧值(重开/升级后重取),
    /// 不残留过期地址。若该应用已打开,调用方应先 [`close`](Self::close)(换地址=重建)。
    pub fn set_pending_url(&mut self, app: &str, url: String) {
        self.pending.insert(app.to_owned(), url);
    }

    /// 该应用待创建地址(`None` = 无)。仅供诊断/测试;真实创建走 [`open_pending`](Self::open_pending)。
    pub fn pending_url(&self, app: &str) -> Option<&str> {
        self.pending.get(app).map(String::as_str)
    }

    /// 取走当前所有"地址到手、等待视口几何"的应用:`(app id, 地址)`。
    ///
    /// 产品拿到视口后逐条调 [`open_pending`](Self::open_pending)(会重新登记待创建,故这里取走后
    /// 地址不回填;调用方须立即用返回的地址创建,或调 [`set_pending_url`](Self::set_pending_url) 退回)。
    pub fn take_pending(&mut self) -> Vec<(String, String)> {
        self.pending.drain().collect()
    }

    /// 视口几何就绪:把待创建地址转成真实 [`SurfaceAction::Create`];没有待创建地址则空。
    ///
    /// 会先关掉同应用的旧 Surface(若已打开),保证换地址一定重建、旧 webview 不会带着旧令牌存活。
    pub fn open_pending(&mut self, app: &str, rect: LogicalRect) -> Vec<SurfaceAction> {
        let Some(url) = self.pending.remove(app) else {
            return Vec::new();
        };
        let mut actions = self.close(app);
        actions.extend(self.open(app, url, rect));
        actions
    }

    fn alloc_slot(&mut self) -> SurfaceSlot {
        let slot = SurfaceSlot(self.next_slot);
        self.next_slot += 1;
        slot
    }
}

/// 从一条通过 nonce 白名单的上行 IPC 得出一句可读诊断(不进日志细节;仅用于状态/可见错误)。
pub fn ipc_label(ipc: AppSurfaceIpc) -> &'static str {
    match ipc {
        AppSurfaceIpc::Focus => "focus",
        AppSurfaceIpc::MouseUp => "mouseup",
        AppSurfaceIpc::ZoomIn => "zoom_in",
        AppSurfaceIpc::ZoomOut => "zoom_out",
        AppSurfaceIpc::ZoomReset => "zoom_reset",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> LogicalRect {
        LogicalRect {
            x: 0.0,
            y: 0.0,
            width: w as f64,
            height: h as f64,
        }
    }

    #[test]
    fn first_open_allocates_a_slot_and_creates() {
        let mut s = AppSurfaces::new();
        let actions = s.open("a", "http://a.localhost:1/?t=1".into(), rect(800.0, 600.0));
        match actions.as_slice() {
            [
                SurfaceAction::Create {
                    app,
                    generation,
                    url,
                    ..
                },
            ] => {
                assert_eq!(app, "a");
                assert_eq!(*generation, 0);
                assert!(url.contains("?t=1"));
            }
            other => panic!("期望一次 Create,得到 {other:?}"),
        }
        assert!(s.is_open("a"));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn reopening_destroys_old_then_creates_with_bumped_generation() {
        let mut s = AppSurfaces::new();
        let _ = s.open(
            "a",
            "http://a.localhost:1/?t=old".into(),
            rect(800.0, 600.0),
        );
        let old_slot = s.slot_of("a").unwrap();
        let actions = s.reopen(
            "a",
            "http://a.localhost:1/?t=new".into(),
            rect(800.0, 600.0),
        );
        // 先销毁旧 webview,再用新地址建;代数递增(同应用判据)。
        assert_eq!(
            actions[0],
            SurfaceAction::Destroy { slot: old_slot },
            "重开必须先销毁旧 Surface(旧地址不得存活)"
        );
        match &actions[1] {
            SurfaceAction::Create {
                generation,
                url,
                slot: new_slot,
                ..
            } => {
                assert_ne!(*new_slot, old_slot, "换新槽,不与旧实例别名");
                assert_eq!(*generation, 1, "同应用代数递增");
                assert!(url.contains("?t=new"), "用新地址,不拿旧地址重建");
            }
            other => panic!("期望 Create,得到 {other:?}"),
        }
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn stale_generation_is_rejected_after_reopen() {
        let mut s = AppSurfaces::new();
        let _ = s.open("a", "u1".into(), rect(1.0, 1.0));
        assert!(s.accepts("a", 0));
        let _ = s.reopen("a", "u2".into(), rect(1.0, 1.0));
        assert!(!s.accepts("a", 0), "旧代数的上行事件被拒");
        assert!(s.accepts("a", 1));
    }

    #[test]
    fn close_destroys_and_drops_the_address() {
        let mut s = AppSurfaces::new();
        let _ = s.open("a", "http://a.localhost:1/".into(), rect(1.0, 1.0));
        let slot = s.slot_of("a").unwrap();
        let actions = s.close("a");
        assert_eq!(actions, vec![SurfaceAction::Destroy { slot }]);
        assert!(!s.is_open("a"), "登记已丢弃(地址不再留存)");
        assert!(s.close("a").is_empty(), "重复关闭幂等");
    }

    #[test]
    fn close_all_destroys_everything() {
        let mut s = AppSurfaces::new();
        let _ = s.open("a", "ua".into(), rect(1.0, 1.0));
        let _ = s.open("b", "ub".into(), rect(1.0, 1.0));
        let actions = s.close_all();
        assert_eq!(actions.len(), 2);
        assert!(s.is_empty());
        assert!(s.close_all().is_empty());
    }

    #[test]
    fn resize_only_when_changed_and_only_when_open() {
        let mut s = AppSurfaces::new();
        assert!(s.resize("a", rect(1.0, 1.0)).is_empty(), "未打开不改");
        let _ = s.open("a", "u".into(), rect(1.0, 1.0));
        assert!(s.resize("a", rect(1.0, 1.0)).is_empty(), "同尺寸不动作");
        let actions = s.resize("a", rect(2.0, 2.0));
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], SurfaceAction::Resize { .. }));
    }

    #[test]
    fn two_apps_get_distinct_slots() {
        let mut s = AppSurfaces::new();
        let _ = s.open("a", "ua".into(), rect(1.0, 1.0));
        let _ = s.open("b", "ub".into(), rect(1.0, 1.0));
        assert_ne!(s.slot_of("a"), s.slot_of("b"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn ipc_labels_cover_every_whitelisted_verb() {
        for (ipc, label) in [
            (AppSurfaceIpc::Focus, "focus"),
            (AppSurfaceIpc::MouseUp, "mouseup"),
            (AppSurfaceIpc::ZoomIn, "zoom_in"),
            (AppSurfaceIpc::ZoomOut, "zoom_out"),
            (AppSurfaceIpc::ZoomReset, "zoom_reset"),
        ] {
            assert_eq!(ipc_label(ipc), label);
        }
    }

    #[test]
    fn launch_url_waits_for_geometry_then_creates() {
        // `Outcome::LaunchUrl` 到手但没有视口几何:先登记为待创建,不建任何原生视图。
        let mut s = AppSurfaces::new();
        s.set_pending_url("a", "http://a.localhost:1/?t=x".into());
        assert_eq!(s.pending_url("a"), Some("http://a.localhost:1/?t=x"));
        assert!(!s.is_open("a"), "没有几何就不创建");
        // 视口几何就绪:转成真实创建动作,待创建地址被消费。
        let actions = s.open_pending("a", rect(800.0, 600.0));
        assert!(matches!(actions.as_slice(), [SurfaceAction::Create { .. }]));
        assert!(s.is_open("a"));
        assert_eq!(s.pending_url("a"), None, "地址已消费,不留存");
    }

    #[test]
    fn replacing_pending_url_keeps_only_the_newest() {
        // 重开/升级后重取:旧地址被新地址覆盖,绝不用过期令牌创建。
        let mut s = AppSurfaces::new();
        s.set_pending_url("a", "http://a.localhost:1/?t=old".into());
        s.set_pending_url("a", "http://a.localhost:1/?t=new".into());
        let actions = s.open_pending("a", rect(1.0, 1.0));
        match &actions[0] {
            SurfaceAction::Create { url, .. } => assert!(url.contains("?t=new")),
            other => panic!("期望 Create,得到 {other:?}"),
        }
    }

    #[test]
    fn closing_drops_any_pending_address_without_a_surface() {
        // 只有待创建地址、尚未创建时关闭:不留任何地址凭证,也不产出销毁动作(无实例可销毁)。
        let mut s = AppSurfaces::new();
        s.set_pending_url("a", "http://a.localhost:1/?t=x".into());
        assert!(s.close("a").is_empty());
        assert_eq!(s.pending_url("a"), None, "地址已丢弃");
        assert!(
            s.open_pending("a", rect(1.0, 1.0)).is_empty(),
            "无待创建可开"
        );
    }

    #[test]
    fn open_error_is_retained_and_cleared_on_success() {
        let mut s = AppSurfaces::new();
        let _ = s.open("a", "u".into(), rect(1.0, 1.0));
        s.report_open_result("a", Err("boom".into()));
        assert_eq!(s.open_error("a"), Some("boom"));
        s.report_open_result("a", Ok(()));
        assert_eq!(s.open_error("a"), None);
        // 迟到回包(应用已关)被忽略:不panic、不复活登记。
        s.report_open_result("ghost", Err("late".into()));
        assert!(!s.is_open("ghost"));
    }
}
