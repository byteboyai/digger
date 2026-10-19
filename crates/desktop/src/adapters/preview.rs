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
            project_root: None,
            safe_startup: true,
            tab_bar_px: (0.0, 0.0),
            locale: "en",
            persistence,
            host,
            handle: None,
            tx,
            rx: Some(rx),
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

    /// 处理一条库消息。项目对不上(切走了 / 删了)就丢掉。
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
    pub fn set_current_project(&mut self, project: Option<&ProjectInfo>) {
        let SameProject::Change { ids } = self.classify(project) else {
            return;
        };
        self.files = None;
        self.project = None;
        self.project_id = ids;
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

    /// 项目被删除:丢掉状态、白名单与落盘文件。
    pub fn forget_project(&mut self, project_id: i64) {
        if self.project_id == Some(project_id) {
            self.files = None;
            self.project = None;
            self.project_id = None;
            self.project_root = None;
        }
        self.host.clear_allowed();
        self.persistence.remove(project_id);
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
        let offset = crate::adapters::preview_native::id_offset(panel);
        state
            .pane
            .take_host_scripts(&live, project_id, panel)
            .into_iter()
            .map(|(id, script)| (id + offset, script))
            .collect()
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
        for effect in fx {
            if is_product_effect(&effect) {
                self.product_effect(panel, project_id, effect);
            } else {
                let tx = tx.clone();
                run_effect(
                    effect,
                    move |message| {
                        let _ = tx.send(Msg::Preview(panel, message));
                    },
                    &handle,
                    persistence.clone(),
                );
            }
        }
    }

    fn product_effect(&mut self, panel: ViewerPanel, project_id: i64, effect: Effect) {
        let _ = (panel, project_id);
        match effect {
            Effect::AllowFile(path) => self.host.allow_file(path),
            // D3:系统应用打开只从降级页 /页签菜单发起,用系统打开器。
            Effect::OpenExternal { path } => crate::adapters::files::open_with_default(
                &crate::adapters::files::system_opener(),
                &path,
            ),
            Effect::PushHostScript { webview_id, script } => {
                self.host.push_script(webview_id, script)
            }
            // Digger 一期不做:MCP 上下文推送(没有 agent 侧上下文)、页签拖拽换位
            // 与悬停记录(无 hover 动画,见 D2)、预览内选区右键菜单(D3 只留文件树 /
            // Project 链接的右键)。
            Effect::ContextChanged { .. }
            | Effect::TabSelected { .. }
            | Effect::TabClosed { .. }
            | Effect::PathOpened(_)
            | Effect::SelectionContextMenu { .. } => {}
            _ => {}
        }
    }
}

/// 一期能力集(计划 Global Constraints):只开代码 / 渲染 / 富文本宿主,
/// 表格 / PlantUML / PDF / 媒体 / JSON 一律走降级页并保留"用系统应用打开"。
fn digger_capabilities() -> PreviewCapabilities {
    PreviewCapabilities {
        code: true,
        rendered: true,
        json: false,
        streamed: false,
        tabular: false,
        plantuml: false,
        pdf: false,
        media: false,
    }
}

/// Digger 一期真正有页面资源的宿主:代码编辑器、Flyfish(带资源根)、
/// 隔离 HTML。图片标注与 PlantUML 渲染器不在发布包里。
fn digger_hosts_url(url: &str) -> bool {
    url.starts_with(dozer_panel_preview::EDITOR_URL_PREFIX)
        || url.starts_with("dozer://flyfish/")
        || url.starts_with("dozer://html/")
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
            | Effect::PushHostScript { .. }
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
fn behavior_of(_panel: ViewerPanel) -> PanelBehavior {
    PanelBehavior {
        persist_tabs: true,
        materialize_suspended: true,
    }
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
    fn tabular_json_and_media_files_get_the_degraded_page_not_a_webview() {
        let dir = std::env::temp_dir().join(format!("digger-preview-caps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut hub = hub(&dir);
        let p1 = project(1, &dir);
        hub.set_current_project(Some(&p1));
        for (name, _body) in [
            ("t.csv", "a,b\n1,2\n"),
            ("j.json", "{\"a\":1}\n"),
            ("p.png", "not really a png"),
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
            "一期能力集下这些类型走降级页,不该有原生页面: {desired:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
}
