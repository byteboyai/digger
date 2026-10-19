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
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use bytehost_core::LogicalRect;
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
    allowed: Arc<Mutex<HashSet<PathBuf>>>,
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Event>>>,
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
            allowed: Arc::new(Mutex::new(HashSet::new())),
            tx,
            rx: Mutex::new(Some(rx)),
        }
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
        let path = path.as_ref();
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.allowed.lock().expect("allowed 锁").insert(path);
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
                let allowed = allowed.lock().expect("allowed 锁");
                let uri = request.uri().to_string();
                let reply = dozer_panel_preview::assets::handle_protocol(&root, &allowed, &uri)
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

/// 由库的期望清单 + 产品的矩形计算,得到可跨线程送进 `window::run` 的目标表。
/// 在宿主线程之外调用(纯数据),避免把 `PreviewPane` 的借用带进 `'static` 闭包。
pub fn desired(
    pane: &dozer_panel_preview::PreviewPane,
    project_id: i64,
    panel: ViewerPanel,
    locale: &str,
    rect: impl Fn(&PreviewWebviewSpec) -> Option<LogicalRect>,
) -> Vec<(PreviewWebviewSpec, LogicalRect)> {
    pane.desired_webviews(project_id, panel, locale)
        .into_iter()
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
        .filter_map(|spec| rect(&spec).map(|rect| (spec, rect)))
        .collect()
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
