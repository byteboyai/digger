//! 预览的原生宿主(S5b-2 P3 Task 7):把共享库算好的期望清单落成 wry 子视图。
//!
//! 本模块只做三件事:
//! 1. 持有 **预览自己的** wry 句柄表(与 `markdown/host.rs` 各一张,规格 Q1);
//! 2. 把 `PreviewPane::desired_webviews` 的清单按 [`plan`] 差分后执行
//!    `Create/Update/Destroy`;
//! 3. 把网页侧的协议请求 / IPC 转交给共享库(`dozer_panel_preview::assets`、
//!    `host_script`、`host_ipc`),本模块不解析任何预览语义。
//!
//! 所有 wry 调用都必须在窗口线程上(`iced::window::run` 内)发生;`sync` 与
//! `Event` 的投递通道就是为此服务的。
//!
//! P3 只交付宿主本身:产品壳接线是后续任务,当前唯一消费者是
//! `examples/preview_smoke.rs`,所以整模块对 `digger` bin 是死代码。
#![allow(dead_code)]

mod plan;

pub use plan::{LiveView, Op, plan};

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use bytehost_core::LogicalRect;
use dozer_panel_preview::assets::AllowedFiles;
use dozer_panel_preview::host_ipc::{HostEvent, IpcContext, IpcMessage, classify_ipc};
use dozer_panel_preview::host_script::{HostScriptOptions, preview_host_script};
use dozer_panel_preview::{PreviewWebviewSpec, ViewerPanel};

thread_local! {
    /// 预览宿主(不含章节编辑器)的全部存活句柄。只由窗口线程读写。
    static VIEWS: RefCell<BTreeMap<usize, wry::WebView>> = const { RefCell::new(BTreeMap::new()) };
}

/// 页面侧发生的事,经 iced 消息回流给产品。
#[derive(Debug, Clone)]
pub enum Event {
    /// 一次预览 host IPC(库已分类)。产品按自己的语义处理。
    Ipc {
        webview_id: usize,
        message: IpcMessage,
    },
    /// 某个句柄建失败(HTML/URL 合法也不会失败,主要是宿主环境问题)。
    BuildFailed { webview_id: usize, error: String },
}

/// 产品可注入的上下文:资源根 + 允许读取的绝对路径 + IPC 回流通道。
pub struct Host {
    assets_root: PathBuf,
    /// 白名单按项目隔离(G-C1):删 / 切别的项目不许收回本项目已打开文件的授权。
    allowed: Arc<Mutex<std::collections::HashMap<i64, AllowedFiles>>>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Event>>>>,
    /// 测试专用:无 wry 时无法建真句柄,这里伪造「哪些 id 活着」,好让
    /// `live_ids()`(进而 `host_scripts()`)在无头测试里走通生产路径。
    #[cfg(test)]
    test_live: Arc<Mutex<Option<std::collections::HashSet<usize>>>>,
}

/// 句柄表是 `thread_local`,副本等价于同一宿主(共享白名单与回流通道),只为了
/// 能搬进 `window::run` 的 `'static` 闭包做脚本注入。
impl Clone for Host {
    fn clone(&self) -> Self {
        Self {
            assets_root: self.assets_root.clone(),
            allowed: Arc::clone(&self.allowed),
            tx: self.tx.clone(),
            rx: Arc::clone(&self.rx),
            #[cfg(test)]
            test_live: Arc::clone(&self.test_live),
        }
    }
}

impl Host {
    /// `dev_assets_root` 是源码树里的 `assets/flyfish`;打包态自动改用
    /// `.app/Contents/Resources/flyfish`(库的 `bundled_assets_root`)。
    pub fn new(dev_assets_root: impl Into<PathBuf>) -> Self {
        let root = dozer_panel_preview::assets::bundled_assets_root(
            &std::env::current_exe().unwrap_or_default(),
            &dev_assets_root.into(),
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            assets_root: root,
            allowed: Arc::new(Mutex::new(std::collections::HashMap::new())),
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
            #[cfg(test)]
            test_live: Arc::new(Mutex::new(None)),
        }
    }

    /// 测试专用:把「活着的句柄 id」固定成给定集合(不再读 thread-local
    /// `VIEWS`,那里没有 wry 就永远是空的)。
    #[cfg(test)]
    pub fn set_test_live_ids(&self, ids: impl IntoIterator<Item = usize>) {
        *self.test_live.lock().expect("test_live 锁") = Some(ids.into_iter().collect());
    }

    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }

    /// 允许预览 host 读取的绝对路径(打开的文件、PlantUML 依赖、HTML 相对资源)。
    ///
    /// 规范化为绝对路径再入表:页面的 `__file__` 请求里是规范化后的路径,而
    /// 白名单是精确匹配(库的 `serve_allowlisted_file` 不做 canonicalize),带
    /// `..` 或相对分量入表会在真机上 404。
    pub fn allow_file(&self, path: impl AsRef<Path>) {
        self.allow_file_for(0, path);
    }

    /// 为某个项目授权一个路径。项目 id 只做隔离用,不参与协议应答。
    pub fn allow_file_for(&self, project_id: i64, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.allowed
            .lock()
            .expect("allowed 锁")
            .entry(project_id)
            .or_default()
            .insert(path);
    }

    /// 撤掉某个项目的全部授权(删除项目时)。
    pub fn clear_allowed_for(&self, project_id: i64) {
        self.allowed.lock().expect("allowed 锁").remove(&project_id);
    }

    /// 清空所有项目的白名单(测试 / 宿主自检用)。
    pub fn clear_allowed(&self) {
        self.allowed.lock().expect("allowed 锁").clear();
    }

    /// 某路径是否已在任一项目的白名单里(测试与宿主自检用,不触发读取)。
    pub fn accepts(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.allowed
            .lock()
            .expect("allowed 锁")
            .values()
            .any(|files| files.contains(&path))
    }

    /// 当前活着的预览句柄 id(库取宿主脚本时据此剔除已销毁页签)。
    pub fn live_ids(&self) -> std::collections::HashSet<usize> {
        #[cfg(test)]
        if let Some(fixed) = self.test_live.lock().expect("test_live 锁").as_ref() {
            return fixed.clone();
        }
        VIEWS.with(|v| v.borrow().keys().copied().collect())
    }

    /// 向某个已建好的预览页面注入脚本(只在该页面确实活着时生效)。
    pub fn push_script(&self, webview_id: usize, script: String) {
        with(webview_id, |view| {
            let _ = view.evaluate_script(&script);
        });
    }

    /// 这一帧建好页面后,把库排队的 host 命令(编辑器 / 表格 / PlantUML)取成
    /// 可注入脚本。命令队列在库一侧,生成脚本同在库内,产品只负责 `eval`。
    pub fn take_host_scripts(
        &self,
        state: &mut dozer_panel_preview::PreviewPanelState,
        project_id: i64,
        panel: ViewerPanel,
    ) -> Vec<(usize, String)> {
        // G-I1:库已返回 `binding.webview_id()`(Project 侧含偏移),不能再加。
        state
            .pane
            .take_host_scripts(&self.live_ids(), project_id, panel)
    }

    pub fn take_receiver(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Event>> {
        self.rx.lock().expect("rx 锁").take()
    }

    /// 后台 send 用的发送端副本(窗口线程与回流任务共用同一通道)。
    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }

    /// 把清单对齐到句柄表。必须在窗口线程上调用。
    ///
    /// `desired` 里的 rect 由产品算(本适配器不猜布局);库只提供
    /// `PreviewWebviewSpec`。
    pub fn sync(
        &self,
        window: &dyn wry::raw_window_handle::HasWindowHandle,
        desired: &[(PreviewWebviewSpec, LogicalRect)],
    ) {
        let live = snapshot();
        for op in plan(&live, desired) {
            match op {
                Op::Create { spec, rect } => self.create(window, spec, rect),
                Op::Update {
                    id,
                    url,
                    rect,
                    visible,
                } => {
                    let navigate = url.clone();
                    with(id, |view| {
                        if let Some(url) = &navigate {
                            let _ = view.load_url(url);
                        }
                        let _ = view.set_bounds(bounds(rect));
                        let _ = view.set_visible(visible);
                    });
                    record(id, rect, visible, url);
                }
                Op::Destroy { id } => forget(id),
            }
        }
    }

    /// 当前存活的句柄数(示例/自检用来钉住生命周期;产品可省)。
    pub fn live_count(&self) -> usize {
        VIEWS.with(|v| v.borrow().len())
    }

    /// 销毁本宿主的全部句柄(窗口关闭时调用)。
    pub fn destroy_all(&self) {
        VIEWS.with(|v| v.borrow_mut().clear());
        LIVE.with(|l| l.borrow_mut().clear());
    }

    fn create(
        &self,
        window: &dyn wry::raw_window_handle::HasWindowHandle,
        spec: PreviewWebviewSpec,
        rect: LogicalRect,
    ) {
        let context = IpcContext::new(spec.id, &spec.url, spec.editor_binding.clone());
        let script = preview_host_script(HostScriptOptions {
            text_editing: context.kind.wants_text_editing(),
            flyfish: context.kind == dozer_panel_preview::host_ipc::WebviewKind::Flyfish,
        });
        let allowed = Arc::clone(&self.allowed);
        // N6:这个页面属于哪个项目,在**创建时**由 Rust 一次定下(绑定里的项目,否则 Rust 拼出来的
        // URL 里的 `proj`),之后协议请求带什么 `proj` 都不作数——页面自己能改请求 URL。
        let bound_project = spec
            .editor_binding
            .as_ref()
            .map(|b| b.project_id)
            .or_else(|| project_from_url(&spec.url));
        let root = self.assets_root.clone();
        let tx = self.tx.clone();
        let webview_id = spec.id;
        let visible = spec.visible || spec.park_offscreen;
        let built = wry::WebViewBuilder::new()
            .with_bounds(bounds(rect))
            .with_devtools(false)
            .with_clipboard(true)
            .with_download_started_handler(|_, _| false)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_navigation_handler(|url| navigation_allowed(&url))
            .with_initialization_script(&script)
            .with_ipc_handler(move |request| {
                let message = classify_ipc(&context, &request.into_body());
                let _ = tx.send(Event::Ipc {
                    webview_id,
                    message,
                });
            })
            .with_custom_protocol("dozer".into(), move |_id, request| {
                // N6:只查**这个页面所属项目**的白名单(创建时绑定,见 `bound_project`)。
                // 此前把各项目的授权合并成一份,A 项目的页面能读到只授权给 B 的文件;
                // 后来改成按请求 URL 里的 `proj` 查,页面自己改 URL 就能越过隔离。
                let uri = request.uri().to_string();
                // 页面的静态资源(编辑器脚本 / 字体 / 宿主页)不需要白名单;只有 `__file__/`
                // 读文件才要。没有绑定项目的页面拿到空白名单,任何文件读取都落到 404。
                let files = allowed_for_project(&allowed, bound_project);
                let reply = dozer_panel_preview::assets::handle_protocol(&root, &files, &uri)
                    .unwrap_or_else(dozer_panel_preview::assets::not_found);
                wry::http::Response::builder()
                    .status(reply.status)
                    .header("Content-Type", reply.mime)
                    .body(std::borrow::Cow::Owned(reply.body))
                    .expect("构造协议应答")
            })
            .with_url(&spec.url)
            .build_as_child(&window);
        match built {
            Ok(view) => {
                let _ = view.set_visible(visible);
                VIEWS.with(|v| {
                    v.borrow_mut().insert(spec.id, view);
                });
                record(spec.id, rect, visible, Some(spec.url.clone()));
            }
            Err(error) => {
                let _ = self.tx.send(Event::BuildFailed {
                    webview_id: spec.id,
                    error: error.to_string(),
                });
            }
        }
    }
}

thread_local! {
    /// 与 `VIEWS` 平行的一份最小记账(URL / rect / visible),供 `plan` 使用。
    static LIVE: RefCell<BTreeMap<usize, LiveView>> = const { RefCell::new(BTreeMap::new()) };
}

fn snapshot() -> BTreeMap<usize, LiveView> {
    LIVE.with(|l| l.borrow().clone())
}

fn record(id: usize, rect: LogicalRect, visible: bool, url: Option<String>) {
    LIVE.with(|l| {
        let mut live = l.borrow_mut();
        let entry = live.entry(id).or_insert_with(|| LiveView {
            url: String::new(),
            rect,
            visible,
        });
        if let Some(url) = url {
            entry.url = url;
        }
        entry.rect = rect;
        entry.visible = visible;
    });
}

/// 销毁一个句柄并把它从记账里抹掉。只删 `VIEWS` 会让 `LIVE` 留着旧项:同 id 的清单项再出现时
/// `plan` 看到"已存在"只发 `Update`,而句柄早没了,webview 永远建不回来。
fn forget(id: usize) {
    VIEWS.with(|v| {
        v.borrow_mut().remove(&id);
    });
    LIVE.with(|l| {
        l.borrow_mut().remove(&id);
    });
}

/// URL 查询串里的 `proj=<id>`。只在**创建页面时**对 Rust 自己拼的 URL 用一次,不对页面发来的请求用。
fn project_from_url(url: &str) -> Option<i64> {
    url.split_once('?')
        .map(|(_, query)| query)
        .unwrap_or("")
        .split('&')
        .find_map(|pair| pair.strip_prefix("proj="))
        .and_then(|v| v.parse::<i64>().ok())
}

/// N6:一个页面能用的白名单 = **它自己那个项目**的那一份;没有绑定项目的页面拿空白名单。
fn allowed_for_project(
    allowed: &std::sync::Mutex<std::collections::HashMap<i64, AllowedFiles>>,
    project: Option<i64>,
) -> AllowedFiles {
    project
        .and_then(|id| allowed.lock().expect("allowed 锁").get(&id).cloned())
        .unwrap_or_default()
}

/// 预览页面允许发生的导航:库的 `dozer://` 协议(含程序化 `load_url`:重载、主题、语言变化都会换 URL),
/// 以及 HTML 隔离宿主内部的 `about:blank` / `about:srcdoc` iframe。其余(http/https/file…)一律拒绝。
/// wry 对所有导航动作都调用这个处理器,不区分主 frame 与子 frame。
fn navigation_allowed(url: &str) -> bool {
    url.starts_with("dozer://") || url == "about:blank" || url == "about:srcdoc"
}

fn with(id: usize, f: impl FnOnce(&wry::WebView)) {
    VIEWS.with(|v| {
        if let Some(view) = v.borrow().get(&id) {
            f(view);
        }
    });
}

fn bounds(rect: LogicalRect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(rect.x, rect.y).into(),
        size: wry::dpi::LogicalSize::new(rect.width, rect.height).into(),
    }
}

/// `Files` 与 `Project` 是两个独立 `PreviewPane`,各自从 0 数 webview id;两边同进
/// 一张句柄表,`Project` 侧统一加这个偏移(库的 editor binding 同款)。
pub const PROJECT_PREVIEW_ID_OFFSET: usize = dozer_panel_preview::PROJECT_PREVIEW_ID_OFFSET;

/// 某面板的 webview id 在共享句柄表里的偏移。
pub fn id_offset(panel: ViewerPanel) -> usize {
    match panel {
        ViewerPanel::Project => PROJECT_PREVIEW_ID_OFFSET,
        _ => 0,
    }
}

/// 由库的期望清单 + 产品的矩形计算,得到可跨线程送进 `window::run` 的目标表。
/// 在宿主线程之外调用(纯数据),避免把 `PreviewPane` 的借用带进 `'static` 闭包。
pub fn desired(
    pane: &dozer_panel_preview::PreviewPane,
    project_id: i64,
    panel: ViewerPanel,
    locale: &str,
    rect: impl Fn(&PreviewWebviewSpec) -> Option<LogicalRect>,
) -> Vec<(PreviewWebviewSpec, LogicalRect)> {
    // 库有两个期望清单:渲染型宿主(Flyfish / HTML / image-annotate)与 CodeMirror 编辑器宿主
    // (文本 / 代码文件)。Dozer 把它们合起来(`app.rs` 的 `specs.extend(desired_editor_webviews)`);
    // 只取前者,文本文件的编辑器页面就永远不会被创建,页签停在"加载中"。JSON / 表格宿主在
    // Digger 里是有意关闭的能力,不并入。
    pane.desired_webviews(project_id, panel, locale)
        .into_iter()
        .chain(pane.desired_editor_webviews(project_id, panel, locale))
        .map(|mut spec| {
            // T9:与 Dozer 的产品侧同款——Rendered host(Flyfish / 隔离 HTML /
            // image-annotate / PlantUML)的归属写进 URL,host 回传 envelope 时
            // 由 `classify_ipc` 据此校验;JS 不能自报。库生成的 URL 只带
            // `p`/`theme`/`locale`,绑定必须由调用方追加。
            if dozer_panel_preview::hosts_rendered_binding(&spec.url) {
                let doc = format!("p{project_id}-t{}", spec.id);
                spec.url.push_str(&format!(
                    "&proj={project_id}&panel={}&tab={}&doc={}",
                    dozer_panel_preview::panel_token(panel),
                    spec.id,
                    dozer_panel_preview::encode_component(&doc),
                ));
            }
            spec
        })
        .map(|mut spec| {
            spec.id += id_offset(panel);
            spec
        })
        .filter_map(|spec| rect(&spec).map(|rect| (spec, rect)))
        .collect()
}

/// 一条预览 IPC 属于哪个预览面板(editor / rendered host 都带绑定;其余变体
/// 不属于预览面板,返回 `None`)。
pub fn panel_of(message: &IpcMessage) -> Option<ViewerPanel> {
    host_event(message).and_then(HostEvent::panel)
}

/// 让 `HostEvent` 也能直接从 IPC 通道取出(便利函数,不改变分类语义)。
pub fn host_event(message: &IpcMessage) -> Option<&HostEvent> {
    match message {
        IpcMessage::Host(event) => Some(event),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// N6:项目 A 的页面读不到只授权给项目 B 的文件。
    ///
    /// 此前协议闭包把所有项目的白名单合并成一份,分项目存储只做了撤销
    /// 隔离;A 页面只要知道路径就能读到 B 的文件。
    #[test]
    fn a_page_of_project_a_cannot_read_a_file_allowed_only_for_project_b() {
        let dir = std::env::temp_dir().join(format!(
            "digger-n6-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let b_file = dir.join("b-secret.txt");
        std::fs::write(&b_file, "B-SECRET").unwrap();
        let a_file = dir.join("a-ok.txt");
        std::fs::write(&a_file, "A-OK").unwrap();
        // 生产路径由 `Host::allow_file_for` 先 canonicalize(macOS 的
        // `/var` → `/private/var`);测试里对齐同一口径。
        let b_file = std::fs::canonicalize(&b_file).unwrap();
        let a_file = std::fs::canonicalize(&a_file).unwrap();

        let allowed = std::sync::Mutex::new(std::collections::HashMap::new());
        {
            let mut guard = allowed.lock().unwrap();
            let mut a = AllowedFiles::new();
            a.insert(&a_file);
            guard.insert(1, a);
            let mut b = AllowedFiles::new();
            b.insert(&b_file);
            guard.insert(2, b);
        }

        // A 的页面请求 B 的文件:`__file__/<abs>` 不存在于 A 的白名单 → 404。
        let b_uri = format!(
            "dozer://editor/__file__/{}?proj=1&panel=files&tab=1&doc=x",
            dozer_panel_preview::webview::encode_component(&b_file.display().to_string())
        );
        let a_files = allowed_for_project(&allowed, Some(1));
        let reply = dozer_panel_preview::assets::handle_protocol(&dir, &a_files, &b_uri)
            .unwrap_or_else(dozer_panel_preview::assets::not_found);
        assert_ne!(reply.status, 200, "A 读到了只授权给 B 的文件");
        assert!(
            !String::from_utf8_lossy(&reply.body).contains("B-SECRET"),
            "A 的应答里出现了 B 的内容"
        );

        // 正向对照:A 读自己的文件能成功(证明 404 来自隔离,不是路径写错)。
        let a_uri = format!(
            "dozer://editor/__file__/{}?proj=1&panel=files&tab=1&doc=x",
            dozer_panel_preview::webview::encode_component(&a_file.display().to_string())
        );
        let a_files = allowed_for_project(&allowed, Some(1));
        let ok = dozer_panel_preview::assets::handle_protocol(&dir, &a_files, &a_uri)
            .unwrap_or_else(dozer_panel_preview::assets::not_found);
        assert_eq!(ok.status, 200, "A 读不到自己的文件(测试自证不成立)");
        assert!(String::from_utf8_lossy(&ok.body).contains("A-OK"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = Path::new(&dir);
    }

    #[test]
    fn a_protocol_request_without_a_project_gets_no_file_access() {
        let allowed = std::sync::Mutex::new(std::collections::HashMap::new());
        {
            let mut guard = allowed.lock().unwrap();
            let mut any = AllowedFiles::new();
            any.insert("/etc/hosts");
            guard.insert(1, any);
        }
        // 静态资源:不带 proj,必须照常解析(用空白名单)。
        let files = allowed_for_project(&allowed, None);
        assert!(!files.contains(Path::new("/etc/hosts")));
    }

    /// N6:页面自己把请求 URL 里的 `proj` 改成别的项目,也读不到别的项目的文件——归属在创建页面时就
    /// 由 Rust 定下,请求里带什么都不作数。
    #[test]
    fn a_forged_proj_in_the_request_url_does_not_switch_the_project() {
        let dir = std::env::temp_dir().join(format!(
            "digger-n6-forged-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let b_file = std::fs::canonicalize({
            let f = dir.join("b-secret.txt");
            std::fs::write(&f, "B-SECRET").unwrap();
            f
        })
        .unwrap();
        let allowed = std::sync::Mutex::new(std::collections::HashMap::new());
        {
            let mut b = AllowedFiles::new();
            b.insert(&b_file);
            allowed.lock().unwrap().insert(2, b);
        }
        // 这个页面创建时绑定的是项目 1;请求 URL 谎称 `proj=2`。
        let files = allowed_for_project(
            &allowed,
            project_from_url("dozer://editor/index.html?proj=1&tab=1"),
        );
        let forged = format!(
            "dozer://editor/__file__/{}?proj=2&panel=files&tab=1&doc=x",
            dozer_panel_preview::webview::encode_component(&b_file.display().to_string())
        );
        let reply = dozer_panel_preview::assets::handle_protocol(&dir, &files, &forged)
            .unwrap_or_else(dozer_panel_preview::assets::not_found);
        assert_ne!(reply.status, 200);
        assert!(!String::from_utf8_lossy(&reply.body).contains("B-SECRET"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn navigation_policy_allows_the_protocol_and_srcdoc_but_nothing_external() {
        for ok in [
            "dozer://editor/index.html?p=%2Ftmp%2Fa.py",
            "dozer://flyfish/host.html?p=x&_r=2",
            "dozer://html/host.html",
            "about:blank",
            "about:srcdoc",
        ] {
            assert!(navigation_allowed(ok), "{ok}");
        }
        for bad in [
            "https://example.com",
            "http://localhost:1",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "dozer.evil://x",
            "",
        ] {
            assert!(!navigation_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn a_destroyed_id_is_created_again_not_updated() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let spec = PreviewWebviewSpec {
            id: 77,
            url: "dozer://editor/index.html".into(),
            visible: true,
            editor_binding: None,
            reserve: None,
            loading_generation: None,
            park_offscreen: false,
        };
        let desired = vec![(spec, rect)];
        record(77, rect, true, Some("dozer://editor/index.html".into()));
        assert!(
            plan(&snapshot(), &desired).is_empty(),
            "unchanged and live: nothing to do"
        );
        forget(77);
        assert!(!snapshot().contains_key(&77));
        assert!(matches!(
            plan(&snapshot(), &desired).as_slice(),
            [Op::Create { .. }]
        ));
    }

    #[test]
    fn host_event_only_matches_host_variant() {
        let msg = IpcMessage::ZoomIn;
        assert!(host_event(&msg).is_none());
    }

    /// Task 6:打包脚本按名字拷这三个目录,少一个宿主就整块白屏。
    /// 断言的是**库源码树**的布局,与 `scripts/package-macos.sh` 的名单一一对应。
    /// git 依赖检出在 `~/.cargo/git/checkouts/...`,所以只在本机源码树存在时检查
    /// (CI 上跳过,打包脚本自己会用 `cargo metadata` 找同一目录并在缺失时报错)。
    #[test]
    fn library_assets_have_the_three_first_batch_dirs() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../dozer/crates/dozer-panel-preview/assets");
        if !assets.is_dir() {
            eprintln!("skip: 本机无 dozer 源码树");
            return;
        }
        for dir in ["editor", "flyfish", "image-annotate"] {
            assert!(
                assets.join(dir).is_dir(),
                "shared preview assets 缺 {dir}:{}",
                assets.display()
            );
        }
    }

    /// 约定测试:库给的 Rendered host URL 不带归属绑定,`desired` 必须补上
    /// `proj/panel/tab/doc`,否则 `classify_ipc` 会把每条 host IPC 都拒成
    /// `Foreign("host URL 缺少归属查询串")`(真机冒烟踩过)。
    #[test]
    fn desired_appends_host_binding_for_rendered_hosts() {
        use dozer_panel_preview::{HostBinding, PreviewPane};

        let dir = std::env::temp_dir().join(format!("digger-preview-p3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("note.md");
        std::fs::write(&md, "# 标题\n\n正文\n").unwrap();

        let mut pane = PreviewPane::default();
        pane.open_path_provisional(md.clone());
        let desired = super::desired(&pane, 42, ViewerPanel::Files, "zh-CN", |_| {
            Some(LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            })
        });
        let rendered = desired
            .iter()
            .find(|(spec, _)| dozer_panel_preview::hosts_rendered_binding(&spec.url))
            .expect("Markdown 应有一个 Rendered host");
        let binding = dozer_panel_preview::flyfish_binding_from_url(&rendered.0.url)
            .expect("URL 必须带完整归属查询串");
        assert_eq!(
            binding,
            HostBinding::new(
                42,
                ViewerPanel::Files,
                rendered.0.id,
                format!("p42-t{}", rendered.0.id)
            )
        );

        let _ = std::fs::remove_file(md);
        let _ = std::fs::remove_dir(dir);
    }
}
