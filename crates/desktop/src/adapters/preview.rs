//! 预览面板的 **Digger 状态层**(S5b-2 P4 Task 7)。
//!
//! Files 与 Project 两个面板各有一份库状态 `PreviewPanelState`,按
//! `(project_id, ViewerPanel)` 分开存:D5 的约定是"切走项目即销毁该项目的
//! 预览",所以这里不进常驻表——每个项目只保留**当前**这一份,切走时整份丢掉
//! (脏页签靠恢复文件兜底,不弹确认)。
//!
//! 这一层只做产品侧该做的事:
//! - 打包 `UpdateCtx`(能力、页签宽度、行为差异)→ 调库 `update` → 执行效果;
//! - 库执行的效果(读文件、解析表格、写盘……)一律交给 `run_effect`;
//! - 产品才有的效果(白名单、宿主脚本、外部打开、页签选中/关闭)在这里落地;
//! - `Effect::PersistTabs` 走 [`DiggerPersistence`],落 `preview_state/<id>.json`。
//!
//! 库消息里带不了项目 id,调用方(`product.rs`)必须先按当前项目路由;这里再
//! 用 `project_id` 做一次迟到消息过滤:项目对不上就整条丢弃,不 panic。
//!
//! 产品壳接线见 `product.rs`(`sync_native` / `body` / `Msg::Preview`)。
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use crate::product::Msg;
use dozer_model::ProjectInfo;
use dozer_panel_preview::capabilities::PreviewCapabilities;
use dozer_panel_preview::effects::{Effect, run_effect};
use dozer_panel_preview::update::PanelBehavior;
use dozer_panel_preview::{
    Message, PreviewPanelState, PreviewPersistence, PreviewState, UpdateCtx, ViewerPanel,
};

use crate::adapters::preview_native::Host;

/// 两个预览面板共享的库状态 + 产品侧接线。
pub struct PreviewHub {
    /// 当前项目的 Files 预览。
    files: Option<PreviewPanelState>,
    /// 当前项目的 Project 预览。
    project: Option<PreviewPanelState>,
    /// 上面两份状态属于哪个项目。
    project_id: Option<i64>,
    /// 会话代次:每次 `project_id` / 项目根发生变化就 +1。
    ///
    /// A → B → A 回到 A 时,`project_id` 又会等于 A,光靠项目 id 拦不住 A 在
    /// 上一轮排出的迟到消息(`SaveFinished` / `Profiled` / 原生 IPC 回传)。
    /// 而页签 id 在重新打开时是从头编号的,旧消息很可能命中同号页签。投递时
    /// 带上发起那一刻的 epoch,回投时和当前 epoch 比对就能整批丢掉。
    epoch: u64,
    /// 当前项目的根目录(库判"是否在项目根内"时用)。
    project_root: Option<PathBuf>,
    /// digger 已确认启动完成之前,库里不调度索引 / 快照类后台任务。
    safe_startup: bool,
    /// 页签条的真实可用宽度;由 `product.rs` 在算配对几何后每帧写回。
    tab_bar_px: (f32, f32),
    locale: &'static str,
    persistence: Arc<DiggerPersistence>,
    /// 原生宿主:Diger 只用一个共享实例,白名单/脚本注入都走它。
    host: Host,
    handle: Option<tokio::runtime::Handle>,
    /// 产品侧回流通道:库执行的效果用 `run_effect` 回投 `Message`,这里转成
    /// `Msg::Preview`。按面板分流。
    tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<Msg>>,
    /// 库排出的 `ContextChanged` 是否应立即推送(S6e-3a)。`Some(true)` 立即,
    /// `Some(false)` 250ms 防抖;由产品排空后调 `sync_preview_context`。
    context_push: Option<bool>,
}

impl PreviewHub {
    pub fn new(host: Host) -> Self {
        Self::with_config_dir(host, crate::paths::config_dir())
    }

    /// 落盘根可注入(测试用临时目录,生产走 [`crate::paths::config_dir`])。
    pub fn with_config_dir(host: Host, config_dir: impl Into<PathBuf>) -> Self {
        let persistence = Arc::new(DiggerPersistence::new(config_dir.into()));
        Self::with_persistence(host, persistence)
    }

    /// 落盘根可注入,且与 `config_dir` 解耦(测试里画一次就是一次盘上写)。
    pub fn with_persistence(host: Host, persistence: Arc<DiggerPersistence>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            files: None,
            project: None,
            project_id: None,
            epoch: 0,
            project_root: None,
            safe_startup: true,
            tab_bar_px: (0.0, 0.0),
            locale: "en",
            persistence,
            host,
            handle: None,
            tx,
            rx: Some(rx),
            context_push: None,
        }
    }

    /// 在当前项目上打开一个路径(D3:文件树双击 / Project 文档链接都走这里)。
    pub fn open(
        &mut self,
        panel: ViewerPanel,
        project: &ProjectInfo,
        path: PathBuf,
        line: Option<usize>,
    ) -> Vec<Effect> {
        self.set_current_project(Some(project));
        self.open_here(panel, project.id, path, line)
    }

    fn open_here(
        &mut self,
        panel: ViewerPanel,
        project_id: i64,
        path: PathBuf,
        line: Option<usize>,
    ) -> Vec<Effect> {
        self.run(panel, project_id, |st, ctx| {
            dozer_panel_preview::update(
                st,
                Message::OpenPath {
                    path,
                    target_line: line,
                },
                ctx,
            )
        })
    }

    /// 当前会话代次。后台效果回投时必须带上它(见 [`Self::accepts`])。
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 一条回投消息还属于当前会话吗?项目对不上(切走了 / 删了)**或**代次对
    /// 不上(A → B → A 之后 A 的旧消息)都算过期。
    pub fn accepts(&self, project_id: i64, epoch: u64) -> bool {
        self.project_id == Some(project_id) && self.epoch == epoch
    }

    /// 处理一条库消息。项目 / 代次对不上就丢掉。
    pub fn dispatch(
        &mut self,
        panel: ViewerPanel,
        project_id: i64,
        message: Message,
    ) -> Vec<Effect> {
        if self.project_id != Some(project_id) {
            return Vec::new();
        }
        self.run(panel, project_id, |st, ctx| {
            dozer_panel_preview::update(st, message, ctx)
        })
    }

    /// 当前项目变了:旧项目的两份状态整份丢掉(不常驻),新项目按需恢复页签。
    ///
    /// G-C1:切换**不是**删除。切走时:
    ///
    /// - 旧项目已落盘的页签(`preview_state/<id>.json`)**原样留着**(落盘发生在
    ///   库排出 `PersistTabs` 时,即打开 / 关闭页签那一刻;切换本身不落盘),
    ///   切回来由 `restore_tabs` 恢复(D4);
    /// - 旧项目的白名单**不撤**(切回来还要用),只有 [`Self::forget_project`]
    ///   才撤。
    ///
    /// 换句话说,`set_current_project` 不碰磁盘、也不碰任何项目的授权,只换内存
    /// 里那两份状态。
    pub fn set_current_project(&mut self, project: Option<&ProjectInfo>) {
        let SameProject::Change { ids } = self.classify(project) else {
            return;
        };
        self.files = None;
        self.project = None;
        self.project_id = ids;
        // 代次推进:上一轮排出的后台消息从此不再属于当前状态。
        self.epoch = self.epoch.wrapping_add(1);
        self.project_root = project.map(|p| PathBuf::from(&p.path));
        if let Some(id) = self.project_id {
            let safe = self.safe_startup;
            let persistence = self.persistence.clone();
            self.run(ViewerPanel::Files, id, |st, ctx| {
                dozer_panel_preview::restore_tabs(st, persistence.as_ref(), ctx, safe)
            });
            // Project 面板的页签是会话期状态(Dozer 侧同样不落盘),不恢复。
        }
    }

    /// 项目被**删除**:丢掉该项目自己的状态、白名单与落盘文件。
    ///
    /// 只在删除项目时调用(切项目走 [`Self::set_current_project`])。
    ///
    /// 三件事**总是**按 `project_id` 做,不看它此刻是不是当前项目:删除一个
    /// 非当前项目时,它的白名单和落盘文件一样要撤。内存态只在被删的正是当前
    /// 项目时才清(否则会把别的项目的 `PreviewPane` 一起丢掉)。
    pub fn forget_project(&mut self, project_id: i64) {
        if self.project_id == Some(project_id) {
            self.files = None;
            self.project = None;
            self.project_id = None;
            self.epoch = self.epoch.wrapping_add(1);
            self.project_root = None;
        }
        self.host.clear_allowed_for(project_id);
        self.persistence.remove(project_id);
    }

    /// 立刻把某面板的当前页签快照落盘,不等库排出 `PersistTabs`。
    ///
    /// 生产代码**不调用**它(切项目 / 退出都不额外落盘;页签增减时库自己排
    /// `PersistTabs`),只有测试用它把「页签已存在」变成可断言的。
    pub fn persist_now(&self, panel: ViewerPanel, project_id: i64) {
        let Some(state) = self.state(panel, project_id) else {
            return;
        };
        let snapshot = state.pane.persisted_snapshot();
        if let Err(e) = self.persistence.save(project_id, &snapshot) {
            eprintln!("digger: 保存预览状态 {project_id} 失败: {e}"); // cli-output
        }
    }

    /// 当前项目里某一面板该有的原生页面清单(Task 8 的 `sync_native` 每帧取)。
    /// 项目对不上时返回空清单。
    pub fn desired(
        &self,
        panel: ViewerPanel,
        project: &ProjectInfo,
        rect: impl Fn(&dozer_panel_preview::PreviewWebviewSpec) -> Option<bytehost_core::LogicalRect>,
    ) -> Vec<(
        dozer_panel_preview::PreviewWebviewSpec,
        bytehost_core::LogicalRect,
    )> {
        if self.project_id != Some(project.id) {
            return Vec::new();
        }
        let Some(state) = self.state(panel, project.id) else {
            return Vec::new();
        };
        crate::adapters::preview_native::desired(&state.pane, project.id, panel, self.locale, rect)
            .into_iter()
            // 能力集只在路由阶段生效;扩展名兜底会绕过它(图片直落
            // image-annotate,`.puml` 直落渲染器)。Digger 一期没有这两套渲染
            // 资源,这里按 URL 身份再筛一遍;被筛掉的页签留降级页 + "用系统
            // 应用打开"。
            .filter(|(spec, _)| digger_hosts_url(&spec.url))
            .collect()
    }

    pub fn state(&self, panel: ViewerPanel, project_id: i64) -> Option<&PreviewPanelState> {
        if self.project_id != Some(project_id) {
            return None;
        }
        match panel {
            ViewerPanel::Project => self.project.as_ref(),
            _ => self.files.as_ref(),
        }
    }

    /// 当前项目里某一面板待注入的 host 命令(编辑器 / 表格 / PlantUML)。
    /// 产品在 `sync_native` 建好页面后调用,再逐个 `eval` 到对应的 webview。
    pub fn host_scripts(&mut self, panel: ViewerPanel, project_id: i64) -> Vec<(usize, String)> {
        if self.project_id != Some(project_id) {
            return Vec::new();
        }
        // 先取 id 快照,再拿状态的可变借用(两者都借 `self`)。库按面板各自数
        // id,共享句柄表里 `Project` 侧带偏移,脚本注入前换算回池内 key。
        let live = self.host.live_ids();
        let Some(state) = self.state_mut(panel) else {
            return Vec::new();
        };
        let scripts = state.pane.take_host_scripts(&live, project_id, panel);
        with_pool_ids(scripts, panel)
    }

    /// 取走库排出的"该推上下文了"标记:`Some(true)` 立即、`Some(false)` 防抖。
    pub fn take_context_push(&mut self) -> Option<bool> {
        self.context_push.take()
    }

    /// 应用一条 Agent 下行的预览命令(S6e-3a Task 6)。
    ///
    /// 目标项目不是当前前台 → `NotFound`;路径/页签找不到 → `NotFound`;
    /// `Suspended` 壳先物化再执行;其余交给库的 `apply_preview_command`。
    pub fn apply_preview_command(
        &mut self,
        project_id: i64,
        cmd: &dozer_model::PreviewCommand,
    ) -> dozer_model::PreviewCommandOutcome {
        use dozer_model::PreviewCommandOutcome as O;
        let rid = cmd.request_id.clone();
        if self.project_id != Some(project_id) {
            return O::NotFound {
                request_id: rid,
                detail: "项目不在前台".into(),
            };
        }
        let empty = dozer_panel_preview::PreviewPane::default();
        let files = self.files.as_ref().map(|s| &s.pane).unwrap_or(&empty);
        let project = self.project.as_ref().map(|s| &s.pane).unwrap_or(&empty);
        let Some((panel, tab_id)) =
            dozer_panel_preview::resolve_command_target(files, project, cmd)
        else {
            return O::NotFound {
                request_id: rid,
                detail: "找不到该路径对应的预览 tab".into(),
            };
        };
        let caps = digger_capabilities();
        let width = |title: &str| crate::adapters::tab_chrome::tab_display_width(title);
        let recovery_dir = self.persistence.recovery_dir();
        let failures_path = self.persistence.failures_path();
        let safe_startup = self.safe_startup;
        let locale = self.locale;
        let project_root = self.project_root.clone();
        let avail_px = match panel {
            ViewerPanel::Project => self.tab_bar_px.1,
            _ => self.tab_bar_px.0,
        };
        let live = self.host.live_ids();
        let mut fx = Vec::new();
        let outcome = {
            let ctx = UpdateCtx {
                panel,
                project_id: Some(project_id),
                project_root: project_root.as_deref(),
                caps: &caps,
                locale,
                tab_bar_avail_px: avail_px,
                tab_display_width: &width,
                active_webview_pool_id: None,
                behavior: behavior_of(panel),
                recovery_dir: Some(&recovery_dir),
                failures_path,
                safe_startup,
            };
            let state = self.panel_state_mut(panel);
            if state.pane.is_pending_load(tab_id) {
                dozer_panel_preview::load_preview_tab(&mut state.pane, &ctx, tab_id, &mut fx);
            }
            let outcome = state.pane.apply_preview_command(tab_id, cmd);
            let drained = state.pane.take_host_scripts(&live, project_id, panel);
            for (webview_id, script) in drained {
                self.host.push_script(webview_id, script);
            }
            outcome
        };
        self.execute(panel, project_id, fx);
        outcome
    }

    fn state_mut(&mut self, panel: ViewerPanel) -> Option<&mut PreviewPanelState> {
        match panel {
            ViewerPanel::Project => self.project.as_mut(),
            _ => self.files.as_mut(),
        }
    }

    /// 库里记录的"启动已就绪"。
    pub fn finish_startup(&mut self) {
        self.safe_startup = false;
    }

    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 页签条可用宽度(逻辑像素),由配对几何写回;沿用既有宽度时改的是
    /// `iced` 侧的状态,只有变了才值得重绘。
    pub fn set_tab_bar_width(&mut self, files: f32, project: f32) {
        self.tab_bar_px = (files, project);
    }

    /// 某面板页签条的可用宽度(建 `PreviewDisplay` 时读)。
    pub fn tab_bar_px(&self, panel: ViewerPanel) -> f32 {
        match panel {
            ViewerPanel::Project => self.tab_bar_px.1,
            _ => self.tab_bar_px.0,
        }
    }

    /// 宿主副本(`sync` / 脚本注入属于宿主线程操作)。
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// 宿主句柄的共享副本(注入脚本要进 `window::run` 的 `'static` 闭包)。
    pub fn host_sender(&self) -> Host {
        self.host.clone()
    }

    pub fn take_receiver(&mut self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Msg>> {
        self.rx.take()
    }

    fn classify(&self, project: Option<&ProjectInfo>) -> SameProject {
        let next = project.map(|p| p.id);
        if next == self.project_id {
            if let Some(project) = project {
                let root = PathBuf::from(&project.path);
                if self.project_root.as_deref() != Some(root.as_path()) {
                    return SameProject::Change { ids: next };
                }
            }
            SameProject::Same
        } else {
            SameProject::Change { ids: next }
        }
    }

    fn panel_state_mut(&mut self, panel: ViewerPanel) -> &mut PreviewPanelState {
        match panel {
            ViewerPanel::Project => self.project.get_or_insert_with(default_state),
            _ => self.files.get_or_insert_with(default_state),
        }
    }

    /// 跑一段库逻辑:打包 `UpdateCtx` → 执行 → 执行效果。
    fn run(
        &mut self,
        panel: ViewerPanel,
        project_id: i64,
        f: impl FnOnce(&mut PreviewPanelState, &UpdateCtx<'_>) -> Vec<Effect>,
    ) -> Vec<Effect> {
        self.project_id = Some(project_id);
        self.run_in(panel, project_id, f)
    }

    fn run_in(
        &mut self,
        panel: ViewerPanel,
        project_id: i64,
        f: impl FnOnce(&mut PreviewPanelState, &UpdateCtx<'_>) -> Vec<Effect>,
    ) -> Vec<Effect> {
        let caps = digger_capabilities();
        let width = |title: &str| crate::adapters::tab_chrome::tab_display_width(title);
        let recovery_dir = self.persistence.recovery_dir();
        let failures_path = self.persistence.failures_path();
        let safe_startup = self.safe_startup;
        let locale = self.locale;
        let project_root = self.project_root.clone();
        let avail_px = match panel {
            ViewerPanel::Project => self.tab_bar_px.1,
            _ => self.tab_bar_px.0,
        };
        let live = self.host.live_ids();
        let state = self.panel_state_mut(panel);
        let (fx, drained) = {
            let ctx = UpdateCtx {
                panel,
                project_id: Some(project_id),
                project_root: project_root.as_deref(),
                caps: &caps,
                locale,
                tab_bar_avail_px: avail_px,
                tab_display_width: &width,
                active_webview_pool_id: None,
                behavior: behavior_of(panel),
                recovery_dir: Some(&recovery_dir),
                failures_path,
                safe_startup,
            };
            let fx = f(state, &ctx);
            // 处理体排出的"立即注入"脚本每轮取走,交给宿主。
            let drained = state.pane.take_host_scripts(&live, project_id, panel);
            (fx, drained)
        };
        self.execute(panel, project_id, fx);
        for (webview_id, script) in drained {
            self.host.push_script(webview_id, script);
        }
        Vec::new()
    }

    /// 执行效果:产品侧的落地,库侧的交 `run_effect`。
    fn execute(&mut self, panel: ViewerPanel, project_id: i64, fx: Vec<Effect>) {
        let Some(handle) = self.handle.clone() else {
            // 还没进 iced 运行时(启动早期,或测试):产品侧效果照做;库侧效果
            // 本来要在后台跑,这里没有后台——直接越过,不让它们落到产品分支。
            for effect in fx {
                if is_product_effect(&effect) {
                    self.product_effect(panel, project_id, effect);
                }
            }
            return;
        };
        let persistence: Arc<dyn PreviewPersistence> = self.persistence.clone();
        let tx = self.tx.clone();
        let epoch = self.epoch;
        for effect in fx {
            if is_product_effect(&effect) {
                self.product_effect(panel, project_id, effect);
            } else {
                let tx = tx.clone();
                run_effect(
                    effect,
                    move |message| {
                        let _ = tx.send(Msg::Preview(panel, project_id, epoch, message));
                    },
                    &handle,
                    persistence.clone(),
                );
            }
        }
    }

    fn product_effect(&mut self, panel: ViewerPanel, project_id: i64, effect: Effect) {
        let _ = panel;
        match effect {
            // 授权带上项目:切 / 删别的项目不影响本项目已打开的文件(G-C1)。
            Effect::AllowFile(path) => self.host.allow_file_for(project_id, path),
            // D3:系统应用打开只从降级页 /页签菜单发起,用系统打开器。
            Effect::OpenExternal { path } => crate::adapters::files::open_with_default(
                &crate::adapters::files::system_opener(),
                &path,
            ),
            // S6e-3a:库用 `ContextChanged { immediate }` 告诉产品何时推上下文;
            // 只记一个待办标记,由 `product.rs` 排空后调 `sync_preview_context`。
            Effect::ContextChanged { immediate } => {
                self.context_push = Some(self.context_push.unwrap_or(false) || immediate);
            }
            // Digger 一期不做:页签拖拽换位与悬停记录(无 hover 动画,见 D2)、
            // 预览内选区右键菜单(D3 只留文件树 / Project 链接的右键)。
            Effect::TabSelected { .. }
            | Effect::TabClosed { .. }
            | Effect::PathOpened(_)
            | Effect::SelectionContextMenu { .. } => {}
            _ => {}
        }
    }
}

/// 一期能力集(计划 Global Constraints):开代码 / 渲染 / 富文本 / 媒体宿主。
///
/// G-I4:`media` 必须开——`.png` 等光栅图由 `is_image_annotate_extension` 认领,
/// 只有 `media: true` 才路由到 Rendered,再落到 `dozer://image-annotate/`。
/// 发布包本来就带 `image-annotate/`(`package-macos.sh` 强制要求)。
///
/// N5(用户 2026-10-20 裁决):`pdf` 开——`.pdf` 与 `.md` / `.html` 一样进
/// flyfish webview,不再走降级页。打包脚本因此**不再**删除
/// `flyfish/vendor/pdf`(约 21 MB,大头是 PDF 的 CMap)。
/// 表格 / PlantUML / JSON 仍走降级页并保留"用系统应用打开"。
fn digger_capabilities() -> PreviewCapabilities {
    PreviewCapabilities {
        code: true,
        rendered: true,
        json: false,
        streamed: false,
        tabular: false,
        plantuml: false,
        pdf: true,
        media: true,
    }
}

/// Digger 一期真正有页面资源的宿主:代码编辑器、Flyfish(带资源根)、
/// 隔离 HTML、图片标注。PlantUML 渲染器不在发布包里。
fn digger_hosts_url(url: &str) -> bool {
    url.starts_with(dozer_panel_preview::EDITOR_URL_PREFIX)
        || url.starts_with("dozer://flyfish/")
        || url.starts_with("dozer://html/")
        || url.starts_with("dozer://image-annotate/")
}

/// 是否由产品侧执行(其余交给库的 `run_effect`)。
fn is_product_effect(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::AllowFile(_)
            | Effect::OpenExternal { .. }
            | Effect::ContextChanged { .. }
            | Effect::TabSelected { .. }
            | Effect::TabClosed { .. }
            | Effect::PathOpened(_)
            | Effect::SelectionContextMenu { .. }
    )
}

enum SameProject {
    Same,
    Change { ids: Option<i64> },
}

fn default_state() -> PreviewPanelState {
    PreviewPanelState::default()
}

/// Digger 的两个预览面板都是"页签持久化 + 可物化"(与 Dozer 的 Files 面板同型;
/// Dozer 的 Project 面板是会话期状态,Digger 的 Project 面板是项目信息列,
/// 不在其中)。
fn behavior_of(panel: ViewerPanel) -> PanelBehavior {
    PanelBehavior {
        // G-C4:`PersistTabs` 只带 `project_id`,两个面板都写就是互相覆盖
        // `<id>.json`。Project 面板的页签是会话期状态(与 Dozer 一致),
        // 只有 Files 落盘。
        persist_tabs: panel != ViewerPanel::Project,
        materialize_suspended: true,
    }
}

/// G-I1:库的 `take_host_scripts` 已返回 `EditorHostBinding::webview_id()`
/// (Project 侧含 `PROJECT_PREVIEW_ID_OFFSET`),产品**不能再加**偏移。
/// `panel` 只保留在签名里,免得将来误以为要按面板换算。
fn with_pool_ids(scripts: Vec<(usize, String)>, _panel: ViewerPanel) -> Vec<(usize, String)> {
    scripts
}

/// 把预览状态层的回流接成一条持续到进程结束的 iced 流(库效果 / 后台结果)。
pub fn receiver_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<Msg>,
) -> impl iced::futures::Stream<Item = Msg> {
    iced::futures::stream::unfold(Some(rx), |maybe| async move {
        let mut rx = maybe?;
        let next = rx.recv().await?;
        Some((next, Some(rx)))
    })
}

/// 落盘实现:`<config>/preview_state/<project_id>.json` + `<config>/preview_recovery`。
/// 目录布局沿用库的 [`dozer_panel_preview::FilePersistence`],但删除接口与
/// 错误口径是 Digger 自己的。
#[derive(Debug, Clone)]
pub struct DiggerPersistence {
    config_dir: PathBuf,
    inner: dozer_panel_preview::FilePersistence,
}

impl DiggerPersistence {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        let config_dir = config_dir.into();
        let inner = dozer_panel_preview::FilePersistence {
            config_dir: config_dir.clone(),
        };
        Self { config_dir, inner }
    }

    /// 删项目时清掉它的页签文件(库没有删除接口,Dozer 的删项目也不清)。
    pub fn remove(&self, project_id: i64) {
        let path = self.state_path(project_id);
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("digger: 删除预览状态 {} 失败: {e}", path.display()); // cli-output
        }
    }

    pub fn state_path(&self, project_id: i64) -> PathBuf {
        self.config_dir
            .join("preview_state")
            .join(format!("{project_id}.json"))
    }
}

impl PreviewPersistence for DiggerPersistence {
    fn load(&self, project_id: i64) -> PreviewState {
        self.inner.load(project_id)
    }

    fn save(&self, project_id: i64, state: &PreviewState) -> std::io::Result<()> {
        self.inner.save(project_id, state)
    }

    fn recovery_dir(&self) -> PathBuf {
        self.inner.recovery_dir()
    }

    fn failures_path(&self) -> PathBuf {
        self.inner.failures_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use dozer_panel_preview::{TabKind, ViewerPanel};

    fn project(id: i64, dir: &Path) -> ProjectInfo {
        ProjectInfo {
            id,
            path: dir.to_string_lossy().into_owned(),
            name: format!("p{id}"),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    fn write(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "print(1)\n").unwrap();
        path
    }

    fn hub(root: &Path) -> PreviewHub {
        PreviewHub::with_config_dir(Host::new(root), root)
    }

    #[test]
    fn a_late_message_for_a_forgotten_project_is_dropped() {
        let dir =
            std::env::temp_dir().join(format!("digger-preview-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&dir, "a.py"), None);
        hub.forget_project(1);
        assert!(hub.state(ViewerPanel::Files, 1).is_none());
        let fx = hub.dispatch(ViewerPanel::Files, 1, Message::CloseTab(1));
        assert!(fx.is_empty());
        assert!(hub.state(ViewerPanel::Files, 1).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 迟到消息过滤的会话代次:A → B → A 回到 A 之后,`project_id` 又等于 A,
    /// 光靠项目 id 拦不住 A 在**上一轮**排出的后台消息。而页签 id 在重新打开
    /// 时从头编号,旧消息很可能命中同号页签。`accepts` 必须同时比对代次。
    #[test]
    fn going_away_and_back_invalidates_the_previous_epoch() {
        let dir = std::env::temp_dir().join(format!("digger-epoch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let a = project(1, &one);
        let b = project(2, &two);
        hub.set_current_project(Some(&a));
        let first = hub.epoch();
        assert!(hub.accepts(1, first), "刚切到 A:本轮消息有效");

        hub.set_current_project(Some(&b));
        hub.set_current_project(Some(&a));
        let second = hub.epoch();
        assert_ne!(first, second, "回到同一个项目也必须换一代");
        assert!(hub.accepts(1, second), "本轮消息有效");
        assert!(
            !hub.accepts(1, first),
            "A 上一轮的迟到消息必须被拒(G-C2 的代次)"
        );
        // 项目 id 也对不上的老消息自然还是拒。
        assert!(!hub.accepts(2, second));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_and_project_panels_do_not_share_tabs() {
        let dir = std::env::temp_dir().join(format!("digger-preview-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        let a = write(&dir, "a.py");
        let b = write(&dir, "README.md");
        hub.open(ViewerPanel::Files, &p1, a.clone(), None);
        hub.open(ViewerPanel::Project, &p1, b.clone(), None);
        let files = hub.state(ViewerPanel::Files, 1).unwrap().pane.tabs();
        let project = hub.state(ViewerPanel::Project, 1).unwrap().pane.tabs();
        assert!(
            files
                .iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if *p == a))
        );
        assert!(
            !files
                .iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if *p == b))
        );
        assert!(
            project
                .iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if *p == b))
        );
        assert!(
            !project
                .iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if *p == a))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_projects_drops_the_previous_tabs() {
        let dir =
            std::env::temp_dir().join(format!("digger-preview-switch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&one, "a.py"), None);
        hub.set_current_project(Some(&p2));
        assert!(hub.state(ViewerPanel::Files, 1).is_none());
        let tabs = hub.state(ViewerPanel::Files, 2).unwrap().pane.tabs();
        assert_eq!(tabs.len(), 1, "only the Blank placeholder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forget_project_removes_state_allowlist_and_the_persisted_file() {
        let dir = std::env::temp_dir().join(format!("digger-preview-purge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let persistence = DiggerPersistence::new(&dir);
        std::fs::create_dir_all(dir.join("preview_state")).unwrap();
        std::fs::write(persistence.state_path(7), "{}").unwrap();
        let mut hub = hub(&dir);
        let p7 = project(7, &dir);
        let file = write(&dir, "a.py");
        hub.set_current_project(Some(&p7));
        hub.open(ViewerPanel::Files, &p7, file.clone(), None);
        assert!(hub.host.accepts(&file));
        // 状态是异步落盘的,`forget_project` 必须排在它之后才谈得上"删掉"。
        std::fs::write(persistence.state_path(7), "{}").unwrap();
        hub.forget_project(7);
        assert!(!hub.host.accepts(&file), "allowlist cleared");
        assert!(!persistence.state_path(7).exists(), "state file removed");
        assert!(hub.state(ViewerPanel::Files, 7).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C1:切项目不删盘、不撤授权,切回来页签恢复。
    #[test]
    fn switching_projects_keeps_the_persisted_preview_state() {
        // G-C1(第一轮修复的遗留问题):旧版从 hub 层直接调 `persist_now`,既绕开
        // 产品路径,又靠只有测试才会用的落盘入口,修复前也能通过。现在走真实
        // 接线:打开页签由库排出 `PersistTabs`,`pump` 执行它落盘;然后只用
        // `set_current_project` 切走再切回,断言页签从盘上恢复。
        let dir = std::env::temp_dir().join(format!("digger-preview-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let persistence = Arc::new(DiggerPersistence::new(&dir));
        let p2dir = dir.join("two");
        std::fs::create_dir_all(&p2dir).unwrap();
        let p1 = project(1, &dir);
        let p2 = project(2, &p2dir);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut hub = PreviewHub::with_persistence(Host::new(&dir), persistence.clone());
        // `PersistTabs` 是库侧效果,要走后台;不给运行时它会被丢掉(G-M)。
        hub.attach_handle(runtime.handle().clone());
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&dir, "a.py"), None);
        // 等落盘任务跑完。
        runtime.block_on(async { tokio::task::yield_now().await });
        std::thread::sleep(std::time::Duration::from_millis(50));

        // 真实路径:`open` 里库排出的 `PersistTabs` 由库侧效果落盘,
        // 不经过只有测试才调的 `persist_now`。
        let state = persistence.state_path(1);
        assert!(state.exists(), "打开页签后 PersistTabs 应已落盘");

        // 切走不删盘、不撤授权,切回来能恢复。
        hub.set_current_project(Some(&p2));
        assert!(state.exists(), "切项目不许删旧项目的落盘文件");
        hub.set_current_project(Some(&p1));
        let tabs = hub.state(ViewerPanel::Files, 1).unwrap().pane.tabs();
        assert!(
            tabs.iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if p.ends_with("a.py"))),
            "切回来要恢复 a.py: {tabs:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C2:迟到的保存结果带着**发起时**的项目;切到 B 后必须被丢掉,
    /// 不能落到 B 里同号的页签。
    #[test]
    fn a_late_save_result_for_project_a_never_reaches_project_b() {
        let dir = std::env::temp_dir().join(format!("digger-preview-late-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&one, "a.py"), None);
        hub.set_current_project(Some(&p2));
        let before: Vec<(usize, bool)> = hub
            .state(ViewerPanel::Files, 2)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| (t.id, t.dirty))
            .collect();
        // A 的页签 id 从 1 起,B 的也一样——这正是串台的温床。
        let fx = hub.dispatch(
            ViewerPanel::Files,
            1,
            Message::SaveFinished {
                tab_id: 1,
                revision: 9,
                result: Ok(()),
            },
        );
        assert!(fx.is_empty(), "A 的迟到结果不许触发 B 的后续效果");
        let after: Vec<(usize, bool)> = hub
            .state(ViewerPanel::Files, 2)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| (t.id, t.dirty))
            .collect();
        assert_eq!(before, after, "B 的页签不许被 A 的保存结果改动");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C2:webview IPC 的绑定说的项目不是当前项目时整条丢弃。
    #[test]
    fn a_late_ipc_event_with_a_foreign_binding_is_dropped() {
        use dozer_panel_preview::host_ipc::HostEvent;
        use dozer_panel_preview::{
            HostBinding, ImageAnnotateEvent, PROTOCOL_VERSION, WebviewEnvelope,
        };
        let dir = std::env::temp_dir().join(format!("digger-preview-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&one, "a.png"), None);
        // 切到 B:此时 A 的页签已销毁,但页面可能还在飞一条迟到的 IPC。
        hub.set_current_project(Some(&p2));
        let binding = HostBinding::new(1, ViewerPanel::Files, 1, "p1-t1".to_string());
        let event = HostEvent::ImageAnnotate {
            binding: binding.clone(),
            envelope: WebviewEnvelope {
                protocol_version: PROTOCOL_VERSION,
                project_id: 1,
                panel: "files".to_string(),
                tab_id: 1,
                document_id: "p1-t1".to_string(),
                revision: 0,
                request_id: None,
                payload: ImageAnnotateEvent::Ready,
            },
        };
        let panel = event.panel().unwrap();
        let project_id = event.project_id().unwrap();
        assert_eq!(project_id, 1, "绑定自带项目 id");
        let fx = hub.dispatch(panel, project_id, Message::Host(event));
        assert!(fx.is_empty(), "外来绑定不许产生效果");
        assert!(
            hub.state(ViewerPanel::Files, 1).is_none(),
            "A 的状态不许被迟到事件重建"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C2:页签 id 每个项目都从 1 数,跨项目不可互换。
    #[test]
    fn tab_ids_are_not_shared_across_projects() {
        let dir = std::env::temp_dir().join(format!("digger-preview-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&one, "a.py"), None);
        hub.open(ViewerPanel::Files, &p1, write(&one, "b.py"), None);
        let a_ids: Vec<usize> = hub
            .state(ViewerPanel::Files, 1)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| t.id)
            .collect();
        hub.set_current_project(Some(&p2));
        hub.open(ViewerPanel::Files, &p2, write(&two, "c.py"), None);
        let b_ids: Vec<usize> = hub
            .state(ViewerPanel::Files, 2)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(a_ids, vec![0, 1, 2], "A 是 Blank + 两个文件");
        assert_eq!(b_ids, vec![0, 1], "B 是 Blank + 一个文件");
        assert_eq!(a_ids[1], b_ids[1], "文件页签 id 两边都从 1 数");
        // 同一条 `SelectTab` 投给 A 时不许碰 B(项目 id 是唯一的区分手段)。
        let before: Vec<(usize, bool)> = hub
            .state(ViewerPanel::Files, 2)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| (t.id, t.dirty))
            .collect();
        let fx = hub.dispatch(ViewerPanel::Files, 1, Message::SelectTab(1));
        assert!(fx.is_empty(), "A 的项目 id 与当前项目不符,整条丢弃");
        let after: Vec<(usize, bool)> = hub
            .state(ViewerPanel::Files, 2)
            .unwrap()
            .pane
            .tabs()
            .iter()
            .map(|t| (t.id, t.dirty))
            .collect();
        assert_eq!(before, after, "B 不受影响");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C1:白名单按项目隔离——删 / 切别的项目不能收回当前项目已打开文件的授权。
    #[test]
    fn deleting_project_a_does_not_revoke_project_bs_allowlist() {
        let dir =
            std::env::temp_dir().join(format!("digger-preview-isolate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let persistence = Arc::new(DiggerPersistence::new(&dir));
        let mut hub = PreviewHub::with_persistence(Host::new(&dir), persistence);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        let a = write(&one, "a.py");
        let b = write(&two, "b.py");
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, a.clone(), None);
        hub.set_current_project(Some(&p2));
        hub.open(ViewerPanel::Files, &p2, b.clone(), None);
        assert!(hub.host.accepts(&b), "b 已授权");
        assert!(hub.host.accepts(&a), "切走不撤别的项目的授权(切回来还要用)");

        // 删掉 A:只撤 A 的授权,当前项目 B 的文件必须还能读。
        hub.forget_project(1);
        assert!(hub.host.accepts(&b), "删 A 不能收回 B 的授权");
        assert!(!hub.host.accepts(&a), "删 A 要撤掉 A 自己的授权");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C1:只有删除才回收状态 / 授权 / 落盘文件。
    #[test]
    fn switching_projects_never_forgets_a_project() {
        // G-C1(第一轮修复的遗留问题):旧版数 `forget_project(` 的行数,修复前
        // 后都等于 1,是循环论证。这里改成行为断言:切换走真实入口
        // (`set_current_project`),断言被切走项目的白名单与落盘文件都还在;
        // 只有 `forget_project` 才会让它们消失。
        let dir =
            std::env::temp_dir().join(format!("digger-preview-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let persistence = Arc::new(DiggerPersistence::new(&dir));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut hub = PreviewHub::with_persistence(Host::new(&dir), persistence.clone());
        hub.attach_handle(runtime.handle().clone());
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        let a = write(&one, "a.py");
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, a.clone(), None);
        // `PersistTabs` 排在 current-thread 运行时上,靠 `block_on` 推进。
        runtime.block_on(async { tokio::task::yield_now().await });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(hub.host.accepts(&a));
        let state = persistence.state_path(1);
        assert!(state.exists(), "打开页签后应已落盘");

        // 真实切换:A → B。切走**不是**删除。
        hub.set_current_project(Some(&p2));
        assert!(hub.host.accepts(&a), "切走不撤旧项目的授权");
        assert!(state.exists(), "切走不删旧项目的落盘文件");

        // B → A 再切回来,页签恢复。
        hub.set_current_project(Some(&p1));
        assert!(hub.state(ViewerPanel::Files, 1).is_some(), "切回来状态重建");

        // 只有 `forget_project` 才回收。
        hub.forget_project(1);
        assert!(!hub.host.accepts(&a), "删除才撤授权");
        assert!(!state.exists(), "删除才删落盘文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-M:`attach_handle` 之前 `execute` 会**静默**丢掉库侧效果(启动早期
    /// 还没进 iced 运行时,没有后台可跑)。产品侧效果照做。`boot()` 里必须
    /// 先 `attach_handle` 再 `set_current_project` / 恢复页签——这条测试把
    /// 「先后顺序错了会丢效果」变成可断言的:同一个 `SaveFile` 在挂句柄前
    /// 只留下产品侧痕迹,挂句柄后才会走库。
    #[test]
    fn library_effects_are_dropped_until_a_runtime_handle_is_attached() {
        let dir = std::env::temp_dir().join(format!("digger-preview-boot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        let a = write(&dir, "a.py");
        let b = write(&dir, "b.py");
        let effects = |path: &std::path::Path| {
            vec![
                Effect::AllowFile(path.to_path_buf()),
                Effect::SaveFile {
                    tab_id: 1,
                    path: path.to_path_buf(),
                    text: "x".to_string(),
                    revision: 0,
                },
            ]
        };
        hub.run_in(ViewerPanel::Files, 1, |_, _| effects(&a));
        assert!(hub.host.accepts(&a), "产品侧效果在挂句柄前照做");
        let mut rx = hub.rx.take().expect("rx 仍在");
        assert!(
            rx.try_recv().is_err(),
            "没挂句柄就没有后台,SaveFile 不该有回投"
        );
        hub.rx = Some(rx);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        hub.attach_handle(runtime.handle().clone());
        hub.run_in(ViewerPanel::Files, 1, |_, _| effects(&b));
        assert!(hub.host.accepts(&b));
        assert!(
            hub.handle.is_some(),
            "boot() 必须先把句柄挂上,后面的库效果才不会丢"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allow_file_effect_populates_the_host_allowlist_with_canonical_paths() {
        let dir = std::env::temp_dir().join(format!("digger-preview-allow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        let file = write(&dir, "a.py");
        let dotty = dir.join(".").join("a.py");
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, dotty, None);
        let canonical = std::fs::canonicalize(&file).unwrap();
        assert!(hub.host.accepts(&canonical));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-I1:库的 `take_host_scripts` 返回的 id 已经是 `binding.webview_id()`
    /// (Project 侧含偏移);Digger 再加一次就会指到不存在的页面。
    #[test]
    fn project_panel_host_scripts_target_the_right_webview_id() {
        use crate::adapters::preview_native::id_offset;
        let dir = std::env::temp_dir().join(format!("digger-preview-gi1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        let doc = dir.join("lib.rs");
        std::fs::write(&doc, "fn main() {}\n").unwrap();
        hub.open(ViewerPanel::Project, &p1, doc.clone(), None);
        let state = hub.state_mut(ViewerPanel::Project).expect("Project 状态");
        let tab_id = state.pane.tabs()[1].id;
        state.pane.queue_editor_command(
            tab_id,
            dozer_panel_preview::EditorCommand::SetDocument {
                text: "x".into(),
                revision: 0,
                language: "markdown".into(),
                read_only: true,
            },
        );
        let expected = dozer_panel_preview::EditorHostBinding::new(
            1,
            ViewerPanel::Project,
            tab_id,
            doc.clone(),
        )
        .webview_id();
        assert_eq!(
            expected,
            id_offset(ViewerPanel::Project) + tab_id,
            "库侧 id 本身已经含偏移"
        );

        // G-I1:走生产路径 `host_scripts()`(内部即 `take_host_scripts` +
        // `with_pool_ids`),而不是自己构造一个恒等函数再断言它恒等。
        // 无 wry 的测试里 `live_ids()` 本来是空的,`take_host_scripts` 会把
        // 命令留到下轮(永远取不到);这里把库算出来的 id 标成「活着」。
        hub.host.set_test_live_ids([expected]);
        let scripts = hub.host_scripts(ViewerPanel::Project, 1);
        assert_eq!(
            scripts.len(),
            1,
            "命令已排队并应从生产路径取出: {scripts:?}"
        );
        assert_eq!(
            scripts[0].0, expected,
            "生产路径给注入脚本的 id 必须是库的 binding.webview_id(),不能再叠加偏移"
        );
        assert!(
            scripts[0].1.contains("SetDocument") || !scripts[0].1.is_empty(),
            "注入脚本不能为空"
        );

        // 反向对照:再叠加一次偏移必须对不上(证明上一条不是恒等式).
        assert_ne!(
            scripts[0].0,
            expected + id_offset(ViewerPanel::Project),
            "把偏移加回去必须与库给的 id 不同"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C4:只有 Files 面板落盘页签;Project 面板的页签不许覆盖同一个文件。
    #[test]
    fn only_the_files_panel_persists_its_tabs() {
        assert!(behavior_of(ViewerPanel::Files).persist_tabs);
        assert!(
            !behavior_of(ViewerPanel::Project).persist_tabs,
            "Project 页签是会话期状态(Dozer 同款)"
        );
    }

    /// G-C4:Project 面板打开的页签不会出现在重启后的 Files 恢复里。
    #[test]
    fn project_tabs_never_overwrite_the_files_state_file() {
        let dir = std::env::temp_dir().join(format!("digger-preview-gc4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let persistence = Arc::new(DiggerPersistence::new(&dir));
        let mut hub = PreviewHub::with_persistence(Host::new(&dir), persistence.clone());
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        let files = write(&dir, "a.py");
        hub.open(ViewerPanel::Files, &p1, files.clone(), None);
        hub.persist_now(ViewerPanel::Files, 1);
        let after_files = std::fs::read_to_string(persistence.state_path(1)).unwrap();

        let doc = dir.join("README.md");
        std::fs::write(&doc, "# hi\n").unwrap();
        hub.open(ViewerPanel::Project, &p1, doc.clone(), None);
        // 库排出的 `PersistTabs` 也要真的不再出现(行为开关,不是只靠 `persist_now` 的面板参数)。
        let fx = hub.dispatch(ViewerPanel::Project, 1, Message::SelectTab(1));
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::PersistTabs { .. })),
            "Project 更新不该排 PersistTabs: {fx:?}"
        );
        hub.persist_now(ViewerPanel::Files, 1);
        assert_eq!(
            std::fs::read_to_string(persistence.state_path(1)).unwrap(),
            after_files,
            "Project 页签不许进 Files 的落盘"
        );
        assert!(
            !after_files.contains("README.md"),
            "Files 状态里没有 Project 的文档: {after_files}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn effects_without_a_digger_meaning_are_ignored_not_errors() {
        let dir = std::env::temp_dir().join(format!("digger-preview-noop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        hub.product_effect(ViewerPanel::Files, 1, Effect::TabSelected { index: 1 });
        hub.product_effect(
            ViewerPanel::Files,
            1,
            Effect::TabClosed {
                index: 1,
                old_len: 2,
            },
        );
        hub.product_effect(
            ViewerPanel::Files,
            1,
            Effect::ContextChanged { immediate: true },
        );
        hub.product_effect(ViewerPanel::Files, 1, Effect::PathOpened(dir.join("a.py")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tabular_json_and_plantuml_get_the_degraded_page_not_a_webview() {
        let dir = std::env::temp_dir().join(format!("digger-preview-caps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        for (name, _body) in [
            ("t.csv", "a,b\n1,2\n"),
            ("j.json", "{\"a\":1}\n"),
            ("d.puml", "@startuml\n@enduml\n"),
        ] {
            hub.open(ViewerPanel::Files, &p1, write(&dir, name), None);
        }
        let desired = hub.desired(ViewerPanel::Files, &p1, |_| {
            Some(bytehost_core::LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            })
        });
        assert!(
            desired.is_empty(),
            "表格 / JSON / PlantUML 走降级页,不该有原生页面: {desired:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-I4:`.png` 走 image-annotate、`.svg` 走 Flyfish——两者都必须是 webview,
    /// 与打包脚本(`image-annotate/` 必须存在)和 Task 10 真机清单一致。
    #[test]
    fn png_and_svg_open_in_a_webview_in_the_preview_hub() {
        let dir = std::env::temp_dir().join(format!("digger-preview-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        for name in ["p.png", "v.svg"] {
            hub.open(ViewerPanel::Files, &p1, write(&dir, name), None);
        }
        let desired = hub.desired(ViewerPanel::Files, &p1, |_| {
            Some(bytehost_core::LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            })
        });
        let urls: Vec<&str> = desired.iter().map(|(spec, _)| spec.url.as_str()).collect();
        assert!(
            urls.iter()
                .any(|u| u.starts_with("dozer://image-annotate/")),
            ".png 应走 image-annotate: {urls:?}"
        );
        assert!(
            urls.iter().any(|u| u.starts_with("dozer://flyfish/")),
            ".svg 应走 Flyfish: {urls:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N5(用户 2026-10-20 裁决「PDF 要开」):`.pdf` 进 flyfish webview,
    /// 不再走降级页。
    ///
    /// 注意库的时序:`open` 之后页签先按名字路由成 `Rendered`(所以立刻取
    /// `desired` 看不出能力开关的差别),真正的降级在 `Profiled` 之后由
    /// `gate_backend` 做。这里断言两件能直接钉住能力的事:
    ///
    /// 1. `digger_capabilities().pdf` 为真(Digger 侧的唯一开关);
    /// 2. 库对该能力确实放行 `.pdf`(`backend_enabled` 是 `gate_backend`
    ///    唯一判据),并确认 `pdf: false` 会拒绝——防止开关写反。
    #[test]
    fn pdf_capability_is_on_and_the_library_honors_it() {
        let caps = digger_capabilities();
        assert!(caps.pdf, "N5:PDF 能力必须开");

        let pdf = std::path::Path::new("/tmp/doc.pdf");
        let rendered = dozer_preview_core::backend::PreviewBackend::Rendered(
            dozer_preview_core::backend::RenderedBackend {
                renderer: dozer_preview_core::backend::RenderedRenderer::Flyfish,
                mode: dozer_preview_core::backend::RenderedMode::Rendered,
                source_language: None,
            },
        );
        assert!(
            dozer_panel_preview::capabilities::backend_enabled(&rendered, pdf, &caps),
            "能力开着时 .pdf 必须放行"
        );
        let mut off = digger_capabilities();
        off.pdf = false;
        assert!(
            !dozer_panel_preview::capabilities::backend_enabled(&rendered, pdf, &off),
            "pdf 关掉时必须拒绝(否则开关形同虚设)"
        );
    }

    #[test]
    fn a_code_tab_wants_one_webview_and_only_its_own_project() {
        let dir = std::env::temp_dir().join(format!("digger-preview-code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        let p2 = project(2, &dir);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&dir, "a.py"), None);
        fn rect(_: &dozer_panel_preview::PreviewWebviewSpec) -> Option<bytehost_core::LogicalRect> {
            Some(bytehost_core::LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            })
        }
        // 打开只建立页签(Loading);清单要等 profile 判定完成后才有原生页面,
        // 判定读文件、要在运行时里跑,单测只看清单形状。
        let desired = hub.desired(ViewerPanel::Files, &p1, rect);
        assert!(
            desired
                .iter()
                .all(|(spec, _)| spec.url.starts_with(dozer_panel_preview::EDITOR_URL_PREFIX)),
            "该面板只该有代码页签的编辑器宿主: {desired:?}"
        );
        assert!(hub.desired(ViewerPanel::Files, &p2, rect).is_empty());
        // 面板之间不共享:Project 面板自己还没有任何页签。
        assert!(hub.state(ViewerPanel::Project, p1.id).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_projects_keep_independent_tabs_per_panel() {
        let dir = std::env::temp_dir().join(format!("digger-preview-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &one);
        let p2 = project(2, &two);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&one, "a.py"), None);
        hub.set_current_project(Some(&p2));
        hub.open(ViewerPanel::Files, &p2, write(&two, "b.py"), None);
        let t2 = hub.state(ViewerPanel::Files, 2).unwrap().pane.tabs();
        assert!(
            t2.iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if p.ends_with("b.py")))
        );
        assert!(
            !t2.iter()
                .any(|t| matches!(&t.kind, TabKind::File(p) if p.ends_with("a.py"))),
            "project 1's tab never leaks into project 2"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn command(
        project_id: i64,
        target: dozer_model::PreviewCommandTarget,
        action: dozer_model::PreviewCommandAction,
        expected_revision: Option<u64>,
    ) -> dozer_model::PreviewCommand {
        dozer_model::PreviewCommand {
            request_id: "r1".into(),
            project_id,
            target,
            action,
            expected_revision,
        }
    }

    fn path_target(path: &Path) -> dozer_model::PreviewCommandTarget {
        dozer_model::PreviewCommandTarget::Path {
            path: path.to_string_lossy().into_owned(),
        }
    }

    /// Task 6:命令只对前台项目生效;非当前项目立即 NotFound(Review Focus 1)。
    #[test]
    fn a_command_for_a_non_current_project_is_not_found() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-pid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        let out = hub.apply_preview_command(
            2,
            &command(
                2,
                path_target(Path::new("/tmp/a.rs")),
                dozer_model::PreviewCommandAction::Reveal { line: 1, column: 1 },
                None,
            ),
        );
        assert!(matches!(
            out,
            dozer_model::PreviewCommandOutcome::NotFound { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Task 6:路径没有对应页签 → NotFound,不隐式打开文件。
    #[test]
    fn a_command_for_an_unknown_path_is_not_found() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, write(&dir, "a.rs"), None);
        let out = hub.apply_preview_command(
            1,
            &command(
                1,
                path_target(Path::new("/tmp/not-open.rs")),
                dozer_model::PreviewCommandAction::Reveal { line: 1, column: 1 },
                None,
            ),
        );
        assert!(matches!(
            out,
            dozer_model::PreviewCommandOutcome::NotFound { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Task 6:同一个路径在 Files 与 Project 都打开时,Path 目标优先 Files。
    #[test]
    fn a_path_open_in_both_panels_is_executed_in_files() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        let shared = write(&dir, "same.rs");
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, shared.clone(), None);
        hub.open(ViewerPanel::Project, &p1, shared.clone(), None);
        let files_id = hub
            .files
            .as_ref()
            .unwrap()
            .pane
            .tabs()
            .iter()
            .find(|t| matches!(&t.kind, TabKind::File(p) if p == &shared))
            .unwrap()
            .id;
        let project_id = hub
            .project
            .as_ref()
            .unwrap()
            .pane
            .tabs()
            .iter()
            .find(|t| matches!(&t.kind, TabKind::File(p) if p == &shared))
            .unwrap()
            .id;
        let out = hub.apply_preview_command(
            1,
            &command(
                1,
                path_target(&shared),
                dozer_model::PreviewCommandAction::Reveal { line: 2, column: 1 },
                None,
            ),
        );
        assert!(matches!(
            out,
            dozer_model::PreviewCommandOutcome::Accepted { .. }
        ));
        let ids: std::collections::HashSet<usize> = (0..64).collect();
        let files_cmds = hub
            .files
            .as_mut()
            .unwrap()
            .pane
            .take_pending_editor_commands_for(&ids, 1, ViewerPanel::Files);
        assert!(
            files_cmds.iter().any(|(id, _)| *id == files_id),
            "Files 页签应收到导航命令"
        );
        let project_cmds = hub
            .project
            .as_mut()
            .unwrap()
            .pane
            .take_pending_editor_commands_for(&ids, 1, ViewerPanel::Project);
        assert!(
            !project_cmds.iter().any(|(id, _)| *id == project_id),
            "Project 同路径页签不应被选中"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Task 6:Suspended 壳先物化,再应用命令。
    #[test]
    fn a_command_for_a_suspended_tab_materializes_it_before_executing() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-susp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write(&dir, "restored.rs");
        let mut hub = hub(&dir);
        hub.persistence
            .save(
                1,
                &dozer_panel_preview::PreviewState {
                    version: dozer_panel_preview::PREVIEW_STATE_VERSION,
                    tabs: vec![dozer_panel_preview::PersistedPreviewTab {
                        path: path.clone(),
                        active: true,
                        ..Default::default()
                    }],
                    active_path: Some(path.clone()),
                    paths: Vec::new(),
                },
            )
            .unwrap();
        let p1 = project(1, &dir);
        // safe_startup 下只恢复壳,不自动物化。
        hub.set_current_project(Some(&p1));
        let id = hub
            .files
            .as_ref()
            .unwrap()
            .pane
            .tabs()
            .iter()
            .find(|t| matches!(&t.kind, TabKind::File(p) if p == &path))
            .unwrap()
            .id;
        assert!(hub.files.as_ref().unwrap().pane.is_pending_load(id));
        let out = hub.apply_preview_command(
            1,
            &command(
                1,
                path_target(&path),
                dozer_model::PreviewCommandAction::Reveal { line: 1, column: 1 },
                None,
            ),
        );
        assert!(matches!(
            out,
            dozer_model::PreviewCommandOutcome::Accepted { .. }
        ));
        assert!(
            !hub.files.as_ref().unwrap().pane.is_pending_load(id),
            "命令到达后该壳应已进入物化流程"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Task 6:replace 的 expected_revision 不匹配时由库返回 StaleRevision。
    #[test]
    fn a_stale_revision_edit_is_rejected_with_stale_revision() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-rev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write(&dir, "edit.rs");
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, path.clone(), None);
        let out = hub.apply_preview_command(
            1,
            &command(
                1,
                path_target(&path),
                dozer_model::PreviewCommandAction::Replace {
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 2,
                    text: "x".into(),
                },
                Some(99),
            ),
        );
        assert!(matches!(
            out,
            dozer_model::PreviewCommandOutcome::StaleRevision { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Task 5:关掉最后一个文本页签时,库排出**立即**推送,且上下文为 `None`。
    #[test]
    fn closing_the_last_text_tab_flags_an_immediate_none_context() {
        let dir = std::env::temp_dir().join(format!("digger-cmd-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write(&dir, "only.rs");
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        hub.open(ViewerPanel::Files, &p1, path.clone(), None);
        let _ = hub.take_context_push(); // 丢掉 open 自己排的那次
        let id = hub
            .files
            .as_ref()
            .unwrap()
            .pane
            .tabs()
            .iter()
            .find(|t| matches!(&t.kind, TabKind::File(p) if p == &path))
            .unwrap()
            .id;
        hub.dispatch(ViewerPanel::Files, 1, Message::CloseTab(id));
        assert_eq!(
            hub.take_context_push(),
            Some(true),
            "关闭页签必须立即推,不能等 250ms 防抖"
        );
        let state = hub.state(ViewerPanel::Files, 1).unwrap();
        eprintln!(
            "CLOSE_CTX={:?} tabs={}",
            dozer_panel_preview::build_preview_context(&state.pane, 1),
            state.pane.tabs().len()
        );
        assert!(
            dozer_panel_preview::build_preview_context(&state.pane, 1).is_none(),
            "没有活动文本页签时上下文必须是 None"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
