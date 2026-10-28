//! Digger 产品壳:在共享 bytehost 宿主上启动的产品组合根。
//!
//! 窗口结构(顶栏 / 左右图标栏 / 左右面板区 / 分隔线 / 放大态 / 布局持久化)来自共享骨架 `bytehost-chrome`,与 Dozer 的
//! 窗口结构同源;本文件只做产品侧的事:注册面板、实现 `ShellHost`、把骨架消息接到各面板的切入动作和原生 webview 的
//! 同步上。拥有自己的数据根、主题、布局落盘与退出,**不复制** Dozer 的 `App`/`Workspace`。
//!
//! 范围(S1):以**无 Project** 状态启动;顶栏的项目页签行保留结构、内容留空,"＋"置灰;**不伪造**任何写入能力。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use bytehost_chrome::persist::SCHEMA_VERSION;
use bytehost_chrome::{
    BodyArea, Divider, El, Loaded, PairRects, PanelMeta, PanelRules, Rect, ShellHost, ShellModel,
    ShellMsg, TopBarSlots, metrics_from_theme, pair_view,
};
use bytehost_core::registry::PanelCatalog;
use bytehost_core::{LogicalRect, PanelId, PanelMeta as CatalogEntry};
use bytehost_desktop::{HostMessage, Product, route_lifecycle};
use byteui::interaction::icons::{self, IconKind};
use byteui::theme::{color, font, icon_size};
use dozer_panel_preview::ViewerPanel;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Size, Subscription, Task, window};
use iced_widget::core::Border;

use crate::{paths, shell};

/// 应用面板自己的头部(返回按钮 + 应用 id + 状态)的高度:原生应用 webview 摆在它下面。
const APPS_HEADER_HEIGHT: f32 = 96.0;
/// 用量面板自己的原生头部(标题 + 加载态)的高度:图表 webview 摆在它下面。
const USAGE_HEADER_HEIGHT: f32 = 80.0;
/// 预览展示上下文的常量部分:Digger 面板外观固定(无悬停动画 D2、列表恒在左 D3)。
/// 页签条宽度与项目 id 每帧随当前状态构造,见 [`preview_display`]。
const PREVIEW_LOCALE: &str = "en";

fn preview_display(
    tab_bar_px: f32,
    project_id: i64,
    epoch: u64,
) -> crate::adapters::preview_display::PreviewDisplay<'static> {
    crate::adapters::preview_display::PreviewDisplay {
        locale: PREVIEW_LOCALE,
        tab_bar_px,
        list_on_left: true,
        project_id,
        epoch,
        hover: None,
    }
}

/// 会弹右键菜单的库消息:库从 `last_right_click` 取锚点。
fn opens_a_context_menu(message: &dozer_panel_files::Message) -> bool {
    use dozer_panel_files::Message as F;
    matches!(
        message,
        F::ContextMenuOpen { .. } | F::TabContextMenuOpen { .. }
    )
}

/// Files 库的右键菜单浮层何时画:Files 面板可见,或 Project 面板可见(Project 面板里的预览页签右键
/// 也走 Files 库的页签菜单)。只认 Files 可见时,Project 里右键会"页面被藏了、菜单没画"。
fn files_menu_is_rendered(files_visible: bool, project_visible: bool) -> bool {
    files_visible || project_visible
}

/// 页签数没变时悬停仍然有效;增减(含从无到有)后按下标寻址的悬停必须作废。
fn hover_survives_tab_change(before: Option<usize>, after: Option<usize>) -> bool {
    before == after
}

/// 预览页签条右侧三颗模式按钮的估算占宽(按钮边长 + 与页签组之间的间距,Dozer 同款)。
const PREVIEW_CHROME_RESERVE_PX: f32 = 32.0 * 3.0 + 4.0 * 3.0;

/// 产品消息。**不进入**宿主 enum(宿主只认 `HostMessage<M>`)。
#[derive(Clone)]
pub enum Msg {
    WindowReady(window::Id, Size),
    /// Iced has no minimized event. Query the owning OS window, never infer it from focus.
    WindowStateRefresh(window::Id),
    WindowStateSample {
        id: window::Id,
        request: u64,
        minimized: Option<bool>,
        size: Option<Size>,
    },
    /// 窗口骨架的消息(点图标、拖分隔线、窗口缩放……),由 `bytehost-chrome` 的 `ShellModel` 处理。
    Shell(ShellMsg),
    /// Agent 面板壳(守护进程可用性)的消息。
    AgentShell(crate::adapters::agent::AgentShellMsg),
    /// Agent 面板的库消息,带项目 id(S6d-2):路由只投给该项目的状态。
    Agent(i64, dozer_panel_agent::update::Message),
    /// 某项目在 `diggerd` 里的存活会话恢复完成(接收端不可 `Clone`,只取一次)。
    AgentRestored(i64, crate::adapters::agent::RestoredResult),
    /// 关闭 Agent 面板顶部的错误提示。
    AgentNoticeDismiss,
    /// 关闭 Agent 选择器浮层(点空白 / Esc),不创建会话。
    AgentPickerClose,
    /// 键盘事件(订阅只在 `Status::Ignored` 时产出;是否进终端由焦点决定)。
    AgentKey(iced::keyboard::Event),
    /// 任意鼠标按下(窗口坐标):决定终端是否获得焦点。
    AgentPointer(iced::Point),
    /// 拖入文件(S6d-2 Task 5):写 shell 转义后的路径到活动会话。
    AgentDrop(PathBuf),
    /// 剪贴板读取结果(⌘V)。`project_id` 是发起时的项目。
    AgentPaste(i64, String),
    /// 输入法预编辑文本(提交/清空由 Input 与焦点逻辑负责)。
    AgentPreedit(i64, Option<String>),
    /// 输入法提交的文本(走 `AgentPanel::commit_ime`,负责清预编辑)。
    AgentImeCommit(i64, String),
    /// 光标移动(窗口坐标):悬停命中与拖拽阈值。
    AgentCursor(iced::Point),
    /// 光标离开窗口:清悬停与拖拽。
    AgentCursorLeft,
    /// 预览页签条部件的悬停进入 / 离开(驱动悬停高亮、× 与 2 秒标题气泡)。
    PreviewHover(
        ViewerPanel,
        dozer_panel_preview::display::PreviewHover,
        bool,
    ),
    /// 光标移到第 `index` 个页签上(拖拽换位用,不经库的消息)。
    AgentTabHover(i64, usize),
    /// 鼠标松开:结束页签拖拽。
    AgentPointerRelease,
    /// 关闭溢出下拉(点空白)。
    AgentOverflowDismiss,
    /// Esc:关最上层覆盖层(确认框 → 选择器 → 溢出菜单)。
    AgentEscape,
    /// 「应用」面板的消息(Task 14)。
    Apps(crate::apps::Message),
    /// 「用量」面板的库消息(Task 20)。
    Usage(dozer_panel_usage::Message),
    NativeUsage(crate::adapters::usage_native::Event),
    /// 「待办」面板的库消息(S3b-1)。
    Todo(dozer_panel_todo::Message),
    Conversations(i64, dozer_panel_conversations::Message),
    /// 群聊面板的消息(项目 id 在库 Message 内部)。
    GroupChat(dozer_panel_group_chat::Message),
    GroupChatNoticeDismiss,
    GroupChatTick,
    /// 聊天页加载失败后点「重试」。
    GroupChatRetry,
    /// 群聊原生页面的回传(IPC、构建结果、探针)。
    GroupChatNative(crate::adapters::group_chat_native::Event),
    ConversationsImport(i64, crate::adapters::conversations_import::ImportEvent),
    /// 会话搜索框右键菜单的动作(整框语义,见 `conversations_search_menu`)。
    ConversationsSearchEdit(i64, crate::adapters::conversations_search_menu::SearchEdit),
    ConversationsNoticeDismiss,
    ReviewNative(crate::adapters::review_native::Event),
    /// 待办原生 webview 的回传。
    NativeTodo(crate::adapters::todo_native::Event),
    /// 待办面板可见时的 1 秒轮询。
    TodoPoll,
    /// 「文件」面板的库消息(共享 `dozer-panel-files` 库)。
    Files(dozer_panel_files::Message),
    /// 文件系统监听:当前项目的工作区有增删改,文件树与 git 着色要跟进。
    FilesFsChanged(crate::adapters::files::FsChange),
    /// 关掉「文件」面板的右键菜单 / 移动卡片 / 删除确认(点空白 / Esc)。
    FilesMenuDismiss,
    /// 「编辑器」面板的消息(ADR-013)。
    Markdown(crate::markdown::Message),
    /// 项目页签与注册表(顶栏页签行)。
    Projects(crate::projects::Message),
    /// 每帧焦点探针:`view` 刚渲染完(库的 `text_input` 焦点态已写回),`id` 是渲染时聚焦的项目。
    ProjectFocusFrame(i64),
    ConversationSearchFocusFrame(i64),
    ConversationSearchFocused(i64, bytehost_core::request::RequestToken, bool),
    /// 浏览器面板的库消息(项目 id 是发起时的项目,迟到的异步结果靠它丢弃)。
    Browser(i64, dozer_panel_browser::Message),
    /// 地址栏右键菜单的动作(整框语义,见 `browser_addr_menu`)。
    BrowserAddrEdit(i64, crate::adapters::browser_addr_menu::AddrEdit),
    /// 浏览器面板可见时的 16ms 探针:推进悬停动画并问地址栏是否持有真实焦点。
    BrowserFocusFrame(i64),
    BrowserAddrFocused(i64, bytehost_core::request::RequestToken, bool),
    /// 浏览器网页子视图池的回传(页面加载 / 标题 / 新窗口请求 / 导航无响应)。
    BrowserNative(crate::adapters::browser_native::Event),
    BrowserNoticeDismiss,
    /// 「项目」面板的库消息(共享 `dozer-panel-project` 库)。
    /// 库消息不带项目 id;路由到哪个项目见 `ProjectPanel::resolve_project`。
    Project(dozer_panel_project::Message),
    /// 预览面板的库消息(S5b-2 P4):`Files` / `Project` 各自一份状态。
    /// `project_id` 是**发起时**的项目(G-C2):迟到的 `SaveFinished` /
    /// `Profiled` / `WindowIndex` 靠它丢弃,不按此刻聚焦的项目再猜一次。
    /// `epoch` 是**发起时**的会话代次:A → B → A 之后 A 上一轮的迟到消息
    /// 项目 id 又对上了,靠代次整批丢掉(页签 id 会重新编号,不拦会误伤)。
    Preview(
        dozer_panel_preview::ViewerPanel,
        i64,
        u64,
        dozer_panel_preview::Message,
    ),
    /// 预览原生页面的回传(库已分类的 host IPC / 建页失败)。
    PreviewEvent(crate::adapters::preview_native::Event),
    /// Agent 下行预览命令的轮询结果(`diggerd` → GUI)。
    PreviewCommandsFetched(PreviewCommands),
    /// 一次「删除项目」后台结果:(项目 id, 是否已从注册表移除, 文件系统步骤的错误)。
    ProjectDeleteDone {
        project_id: i64,
        deregistered: bool,
        errors: Vec<String>,
    },
    /// 一次「选择文档链接文件/目录」的返回(库的 `Pick` 由宿主落地)。
    ProjectLinkPicked(dozer_panel_project::links::LinkTarget, Option<PathBuf>),
    /// 面板上按下鼠标:编辑框此刻若还在编辑态,就是失焦了(iced 不报焦点事件,
    /// 只能由"点别处"触发落盘)。
    ProjectPointerDown,
    /// 面板生命周期自检的推进(仅 `DIGGER_LIFECYCLE_SELFTEST` 设置时出现)。
    Lifecycle(crate::lifecycle_selftest::Phase),
    Tick,
    /// 左键按下 / 松开(全局事件):树内拖拽靠它收尾和自愈。
    LeftButton(bool),
    /// 修饰键变化(全局事件):树内单击的 Shift / Cmd 多选读它。
    Modifiers(iced::keyboard::Modifiers),
    /// 双击顶栏空白处:缩放窗口(系统标题栏没了,这个手势要自己补)。
    ZoomToggle,
    ShutdownFinished(Result<(), String>),
    ForceExit,
    CloseRequested,
    Exit,
}

/// Digger 产品壳状态。刻意保持最小:不持有任何 Domain/持久化状态,只有 UI 交互态 + 布局 + 主题。
/// 轮询结果:项目 id + 该项目的待处理命令。
type PreviewCommands = (i64, Vec<dozer_model::PreviewCommand>);

pub struct ProductShell {
    window: Option<window::Id>,
    window_minimized: bool,
    window_state_request: u64,
    catalog: PanelCatalog,
    /// 窗口骨架状态(布局、选中、收起、放大、尺寸……)。
    shell: ShellModel,
    shell_path: PathBuf,
    /// 非 `None` 表示全局 `shell.json` 是**不支持的高版本**写的:内存默认布局仅用于显示,**禁止回写**该文件。
    /// 只在启动读全局文件时设定,换项目不会清掉它。
    shell_save_blocked: Option<String>,
    /// 同上,针对**当前项目**自己的布局文件;换项目时重新判定。
    project_shell_blocked: Option<String>,
    /// 「应用」面板的组合根(状态机 + 执行器 + 内嵌应用服务)。
    apps: crate::apps::AppsState,
    /// 「用量」面板适配(共享 `dozer-panel-usage` 库 + 注入的真实用量服务)。
    usage: crate::adapters::usage::UsagePanel,
    /// 「待办」面板适配(S3b-1):按项目状态 + `diggerd` 的 TodoService。
    todo: crate::adapters::todo::TodoPanel,
    conversations: crate::adapters::conversations::ConversationsPanel,
    browser: crate::adapters::browser::BrowserPanel,
    group_chat: crate::adapters::group_chat::GroupChatPanel,
    /// 「编辑器」面板:自包含的第一方面板(文本与协议在 `digger-editor`,这里只有宿主适配)。
    markdown: crate::markdown::MarkdownPanel,
    /// 项目注册表与已开页签(Project 是 host 层共享业务,ADR-014)。
    projects: crate::projects::ProjectsState,
    /// 「项目」面板适配(共享 `dozer-panel-project` 库 + 注入的项目/记忆服务)。
    project: crate::adapters::project::ProjectPanel,
    /// 「文件」面板适配(共享 `dozer-panel-files` 库;四项能力 Digger 一期全关)。
    files: crate::adapters::files::FilesPanel,
    agent: crate::adapters::agent::AgentPanel,
    /// 终端字体与尺寸(S6d-1 `fonts::terminal_style`):壳持有,Agent 展示适配与
    /// 网格换算都借它,免得每帧泄漏一份。
    term_style: dozer_panel_term::style::TermStyle,
    /// 预览状态层(S5b-2 P4):Files / Project 两个面板各一份库状态 + 原生宿主。
    /// 在 `boot` 里装上;纯逻辑测试可以不要(没有预览就没有页签可开)。
    preview: Option<crate::adapters::preview::PreviewHub>,
    /// S6e-3a:预览上下文推送 + 命令轮询的 UDS 桥(在 `boot` 里装上)。
    preview_bridge: Option<crate::adapters::preview_bridge::PreviewBridge>,
    /// 轮询器读的当前项目 id(≤0 跳过);项目切换时写入。
    preview_project: Arc<AtomicI64>,
    /// 轮询器只启动一次。
    preview_poller_started: Arc<AtomicBool>,
    /// 轮询结果回投通道(只能取一次,`boot` 接成 iced 流)。
    preview_cmds_tx: tokio::sync::mpsc::UnboundedSender<PreviewCommands>,
    preview_cmds_rx:
        std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<PreviewCommands>>>,
    /// N1b:两个预览面板(Files / Project)各一份展示上下文,`view(&self)` 每帧
    /// 原地更新。库的 `view::pane` 要求它活到返回的 `Element` 一样久;以前每帧
    /// `Box::leak` 一份新的(60fps 下无界增长),现在由壳持有,地址稳定。
    preview_displays:
        [std::cell::Cell<crate::adapters::preview_display::PreviewDisplay<'static>>; 2],
    /// 「删除项目」结果回流:库的后台任务在线程池里跑,结果经这里转成产品消息。
    delete_tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    /// 上面那条通道的接收端,`boot` 里取一次接成 iced 流(`None` = 已取走)。
    delete_rx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Msg>>>,
    /// 生命周期自检的驱动(只在环境变量设置时有)。
    lifecycle: Option<crate::lifecycle_selftest::Driver>,
    /// [应用, 用量] 面板上一次是否可见:变为可见时触发各自的切入动作;应用面板变为不可见时关掉正在显示的应用。
    /// 与 [`shell::all_panels`] 同序。
    visible_before: Vec<bool>,
    /// 产品未接业务服务时的启动说明,显示在信息面板。
    boot_note: String,
    closing: bool,
    /// 左键此刻是否物理按住(`Msg::LeftButton` 维护)。
    left_down: bool,
    /// 当前修饰键(`Msg::Modifiers` 维护)。
    modifiers: iced::keyboard::Modifiers,
    /// 最近一次光标位置(`ShellMsg::CursorMoved`):鼠标事件本身不带坐标,
    /// 终端焦点判断用它。
    last_cursor: iced::Point,
    /// 预览页签条里此刻被悬停的部件与进入时刻(Files 与 Project 两个面板共用一份)。
    preview_hover: crate::adapters::hover::HoverTracker<(
        ViewerPanel,
        dozer_panel_preview::display::PreviewHover,
    )>,
    shutdown_started: bool,
    shutdown_error: Option<String>,
    /// 测试用:记录收到的生命周期回调,验证 Surface 事件恰好路由一次(复盘发现 6)。
    #[cfg(test)]
    surface_callbacks: Vec<bytehost_desktop::SurfaceSlot>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn logical(r: Rect) -> LogicalRect {
    LogicalRect {
        x: r.x as f64,
        y: r.y as f64,
        width: r.w as f64,
        height: r.h as f64,
    }
}

/// 项目布局文件:`<root>/shell-projects/<id>.json`。生产里 `root` 是配置目录(`<config>/digger/shell-projects/`),
/// 测试里跟着全局 `shell.json` 走,免得测试互相覆盖、也不碰真实配置目录。
fn project_shell_path_of(root: &Path, id: i64) -> PathBuf {
    root.join("shell-projects").join(format!("{id}.json"))
}

/// 配对预览块里原生 webview 的屏幕矩形。
///
/// 库的 `view::pane` 把**整块内容**(页签条 + 分隔线 + 正文)放进
/// `container(content).padding(region.padding)`。所以:
/// - top = `preview_chrome_top_px()`(38,已含页签条自身高度)+ 一个 padding;
/// - bottom 只留一个 padding(不是两个);
/// - 左右各一个 padding。
///
/// G-I2:旧实现底部又减了一个 `inset`,比 Dozer 的 `webview_geometry`
/// (`y = 顶 + 38`, `h = 底 - y`)矮 8px,也与 iced 里正文容器的实际高度不符。
fn preview_webview_rect(slot: Rect) -> Rect {
    let inset = crate::adapters::preview_display::PreviewDisplay::content_padding();
    let top = byteui::theme::geometry::preview_chrome_top_px() + inset;
    Rect::new(
        slot.x + inset,
        slot.y + top,
        (slot.w - inset * 2.0).max(1.0),
        (slot.h - top).max(1.0),
    )
}

fn review_native_rect(
    shell: &ShellModel,
    content: Rect,
    collapsed: bool,
    notice_height: f32,
) -> Rect {
    let mut block = if collapsed {
        content
    } else {
        shell
            .side_of(&shell::conversations_id())
            .and_then(|side| shell::pair_rects(shell, side))
            .map(|pair| pair.second)
            .unwrap_or(content)
    };
    let notice_height = notice_height.min(block.h.max(0.));
    block.y += notice_height;
    block.h = (block.h - notice_height).max(0.);
    crate::adapters::review_geometry::review_bounds(block, byteui::theme::icon_size::scale())
}

fn group_chat_native_rect(shell: &ShellModel, content: Rect, collapsed: bool) -> Rect {
    let block = if collapsed {
        content
    } else {
        shell
            .side_of(&shell::group_chat_id())
            .and_then(|side| shell::pair_rects(shell, side))
            .map(|pair| pair.second)
            .unwrap_or(content)
    };
    crate::adapters::group_chat_geometry::chat_bounds(block, byteui::theme::icon_size::scale())
}

/// 向下缩 `top` 像素(面板自己的头部占掉的高度)。
fn below_header(r: Rect, top: f32) -> Rect {
    Rect::new(r.x, r.y + top, r.w, (r.h - top).max(1.0))
}

/// 用量面板此刻怎么摆(Dozer `PanelKind::Usage` 分支的三态)。**`body()` 与 `sync_native()` 共用这一个判断**,
/// 谁也不自己再算一遍,免得 body 已经是单栏、图表还停在分栏的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageLayout {
    /// 加载中 / 还没数据:没有列表可拼,内容栏独占全宽。
    ContentOnly,
    /// 有数据、列表被收起:内容栏独占全宽,标题行里留"展开"按钮。
    Collapsed,
    /// 有数据且列表展开:内容 | 列表。
    Paired,
}

/// 用量的原生图表只放在内容栏里:拼分栏时是配对的第二块(镜像与否由骨架几何给出),其余两种状态是整个面板内容区。
fn usage_native_rect(layout: UsageLayout, content: Rect, pair: Option<PairRects>) -> Rect {
    match (layout, pair) {
        (UsageLayout::Paired, Some(p)) => p.second,
        _ => content,
    }
}

/// 待办 webview 的矩形:配对第二块(内容区)扣掉库视图为 iced 预留的顶部行
/// 与底部新增输入。列表收起时内容区独占整块;整个面板不可见时由调用方传
/// `visible=false` 销毁。
fn todo_native_rect(content: Rect, pair: Option<PairRects>, add_input_h: f32) -> Rect {
    let slot = pair.map(|p| p.second).unwrap_or(content);
    let top = dozer_panel_todo::adapter::top_row_inner_h_px();
    Rect::new(
        slot.x,
        slot.y + top,
        slot.w,
        (slot.h - top - add_input_h).max(1.0),
    )
}

/// 本帧要把哪些预览页面放到哪:`sync_native` 的纯数据半段。
///
/// 只读骨架(选中的配对面板)与预览状态,顺便把两个面板的页签条可用宽度
/// 写回 hub。与 `window::run` / `Task` 无关,所以能在无头测试里走真实接线
/// (G-C3 的「宽度确实被 `sync_preview_native` 写回」、G-C5 的「关闭 / 无项目
/// 时同步空清单」都靠它)。
fn collect_preview_desired(
    preview: &mut crate::adapters::preview::PreviewHub,
    shell: &ShellModel,
    rects: &bytehost_chrome::ShellRects,
    project: &dozer_model::ProjectInfo,
    overlay_open: bool,
) -> Vec<(dozer_panel_preview::PreviewWebviewSpec, LogicalRect)> {
    let mut desired: Vec<(dozer_panel_preview::PreviewWebviewSpec, LogicalRect)> = Vec::new();
    let mut tab_bar = (0.0_f32, 0.0_f32);
    for panel in [ViewerPanel::Files, ViewerPanel::Project] {
        let id = match panel {
            ViewerPanel::Project => shell::project_id(),
            _ => shell::files_id(),
        };
        let Some(side) = shell.side_of(&id) else {
            continue;
        };
        if shell.selected(side) != &id || rects.content(side).is_none() {
            continue;
        }
        let Some(slot) = shell::pair_rects(shell, side).map(|p| p.second) else {
            continue;
        };
        let rect = logical(preview_webview_rect(slot));
        desired.extend(preview.desired(panel, project, |_| Some(rect)));
        // 页签条可用宽度:预览块真实宽度减三颗按钮与间距(Dozer 同款估算)。
        let avail = (slot.w - PREVIEW_CHROME_RESERVE_PX).max(0.0);
        match panel {
            ViewerPanel::Project => tab_bar.1 = avail,
            _ => tab_bar.0 = avail,
        }
    }
    preview.set_tab_bar_width(tab_bar.0, tab_bar.1);
    hide_for_overlay(&mut desired, overlay_open);
    desired
}

/// 有 iced 浮层(页签溢出菜单、Files 右键菜单 / 对话框、Project 删除确认卡)展开时,原生 webview
/// 会盖在它上面。Dozer 的做法(`app_modal_open` / `tab_overflow_open` / `panel_popup_open`)是浮层开着
/// 就把 webview 真隐藏、关掉再显示,而不是销毁:页签状态与页面都留着。这里同款:清单项保留,只把
/// `visible` 与 `park_offscreen` 置假,`plan()` 产出 `Update { visible: false }`。
fn hide_for_overlay(
    desired: &mut [(
        dozer_panel_preview::PreviewWebviewSpec,
        bytehost_core::LogicalRect,
    )],
    overlay_open: bool,
) {
    if !overlay_open {
        return;
    }
    for (spec, _) in desired.iter_mut() {
        spec.visible = false;
        spec.park_offscreen = false;
    }
}

impl ProductShell {
    /// 构造:注册产品面板目录、读布局(缺省用默认布局),不打开任何 Project。
    pub fn new() -> Self {
        Self::with_shell_path(paths::shell_path())
    }

    pub(crate) fn with_shell_path(shell_path: PathBuf) -> Self {
        let mut catalog = PanelCatalog::new();
        for (id, title) in [
            (shell::info_id(), "Digger"),
            (shell::apps_id(), "应用"),
            (shell::usage_id(), "用量"),
            (shell::project_id(), "项目"),
            (shell::files_id(), "文件"),
            (shell::todo_id(), "待办"),
            (shell::agent_id(), "Agent"),
            (shell::group_chat_id(), "群聊"),
            (shell::conversations_id(), "会话"),
            (shell::browser_id(), "浏览器"),
        ] {
            catalog
                .register(CatalogEntry {
                    id,
                    title: title.into(),
                    surfaces: vec![],
                })
                .expect("首次注册产品面板");
        }
        catalog
            .register(CatalogEntry::single_surface(shell::markdown_id(), "编辑器"))
            .expect("首次注册 digger.markdown");

        // 启动时读全局布局(项目布局只在切到项目页签时换上,见 `Msg::Projects`)。
        let loaded = shell::load(&shell_path);
        let shell_save_blocked = match &loaded {
            Loaded::Newer { found, .. } => Some(format!(
                "布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**"
            )),
            _ => None,
        };
        let mut model = loaded.into_model();

        // 任务完成用量账本(Task 20):生产者(应用操作)写、用量面板读,同一实例。
        // 载入失败(损坏)不 panic,也**不**退回一次性临时空账本(复审 #9)——后者会把"读不出来"伪装成 "Ok(空)",且新记录
        // 写进下次启动就不再引用的临时文件,等于静默丢。改为:查询恒返回 Unavailable、**不接线**用量生产者(没有记录悄悄
        // 写走),原损坏文件保持原样。
        let scope_path = paths::config_dir();
        let (usage_service, usage_ledger, boot_note) =
            load_usage_ledger(paths::usage_ledger_path());

        let mut apps = crate::apps::AppsState::new();
        // 只有账本可用时才接用量生产者;损坏时**不接**,避免记录写进无引用的临时文件。
        if let Some(ledger) = &usage_ledger {
            apps.attach_usage(ledger.clone(), scope_path.to_string_lossy().to_string());
        }
        let usage = crate::adapters::usage::UsagePanel::new(usage_service, scope_path.clone());
        let todo_client = dozer_client::Client::new(digger_paths::product().socket_path());
        let todo =
            crate::adapters::todo::TodoPanel::new(Arc::new(todo_client.clone()), todo_client);
        let (conversation, summary, transcript) =
            crate::daemon::Daemon::new().conversation_services();
        let copy_cleanup = transcript.clone();
        let conversations = crate::adapters::conversations::ConversationsPanel::new(
            conversation,
            summary,
            transcript,
        );
        let browser = crate::adapters::browser::BrowserPanel::new(Arc::new(
            dozer_client::Client::new(digger_paths::product().socket_path()),
        ));
        let group_chat = crate::adapters::group_chat::GroupChatPanel::new(
            crate::daemon::Daemon::new().services(),
        );
        let markdown = crate::markdown::MarkdownPanel::new();
        // 项目数据与布局同在一个数据目录里:生产是产品数据根,测试是各自的临时目录。
        let data_dir = shell_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(paths::config_dir);
        let projects = if data_dir == paths::config_dir() {
            // 生产:注册表就是 diggerd 用的那个文件,并迁入旧 projects.db。
            crate::projects::ProjectsState::open_with(
                &data_dir,
                &digger_paths::projects_db_path(),
                &digger_paths::legacy_projects_db_path(),
            )
        } else {
            crate::projects::ProjectsState::open_in(&data_dir)
        };
        let mut project = crate::adapters::project::ProjectPanel::new(projects.project_host());
        project.attach_copy_cleanup(copy_cleanup);
        let files = crate::adapters::files::FilesPanel::new(
            data_dir.join(dozer_panel_files::external_apps::CONFIG_FILE),
        );
        let (delete_tx, delete_rx) = tokio::sync::mpsc::unbounded_channel();
        let delete_rx = std::sync::Mutex::new(Some(delete_rx));
        let (preview_cmds_tx, preview_cmds_rx) = tokio::sync::mpsc::unbounded_channel();
        let preview_cmds_rx = std::sync::Mutex::new(Some(preview_cmds_rx));
        let lifecycle = crate::lifecycle_selftest::Driver::from_env();
        // 自检模式需要编辑器一开始就看得见。
        if markdown.is_selftest() || lifecycle.is_some() {
            shell::update(&mut model, ShellMsg::ShowPanel(shell::markdown_id()));
        }

        let mut shell = Self {
            window: None,
            window_minimized: false,
            window_state_request: 0,
            catalog,
            shell: model,
            shell_path,
            shell_save_blocked,
            project_shell_blocked: None,
            apps,
            usage,
            todo,
            markdown,
            projects,
            project,
            files,
            agent: crate::adapters::agent::AgentPanel::new(),
            conversations,
            browser,
            group_chat,
            term_style: crate::fonts::terminal_style(),
            preview: None,
            preview_bridge: None,
            preview_project: Arc::new(AtomicI64::new(0)),
            preview_poller_started: Arc::new(AtomicBool::new(false)),
            preview_cmds_tx,
            preview_cmds_rx,
            preview_displays: [
                std::cell::Cell::new(preview_display(0.0, 0, 0)),
                std::cell::Cell::new(preview_display(0.0, 0, 0)),
            ],
            delete_tx,
            delete_rx,
            lifecycle,
            visible_before: Vec::new(),
            boot_note,
            closing: false,
            left_down: false,
            modifiers: iced::keyboard::Modifiers::default(),
            last_cursor: iced::Point::new(0.0, 0.0),
            preview_hover: crate::adapters::hover::HoverTracker::new(),
            shutdown_started: false,
            shutdown_error: None,
            #[cfg(test)]
            surface_callbacks: Vec::new(),
        };
        // 构造时就记下此刻谁可见:否则第一条消息会把本来就看得见的面板当成"刚变可见",重复触发切入动作
        // (启动时本来就可见的面板由 `boot` 显式触发一次,不靠这里)。
        shell.visible_before = visible_panels(&shell);
        shell
    }

    /// 落盘当前骨架状态(退出 / 布局变化时调用)。失败不 panic,记入 boot_note。
    /// 磁盘上是**不支持的高版本**布局时**拒绝回写**(复盘发现 4:绝不覆盖原文件)。
    ///
    /// 有当前项目时布局写到该项目自己的文件,窗口大小仍写全局 `shell.json`;没有项目时整份写全局。
    fn save_shell(&mut self) {
        let project = self.current_project_shell_path();
        self.save_shell_scoped(project);
    }

    /// 把内存里的布局写到指定项目的文件(`None` = 整份写全局)。切页签时用它写**旧**项目,
    /// 因为此刻 `projects` 里的当前项目已经是新的。
    fn save_shell_for(&mut self, project: Option<i64>) {
        let path = project.and_then(|id| self.project_shell_path_for(id));
        self.save_shell_scoped(path);
    }

    fn save_shell_scoped(&mut self, project_path: Option<PathBuf>) {
        let Some(project_path) = project_path else {
            if let Some(note) = &self.shell_save_blocked {
                self.boot_note = format!("布局未保存:{note}");
                return;
            }
            if let Err(e) = bytehost_chrome::persist::save_atomic(&self.shell_path, &self.shell) {
                self.boot_note = format!("布局保存失败:{e}");
            }
            return;
        };
        if let Some(note) = &self.project_shell_blocked {
            self.boot_note = format!("布局未保存:{note}");
        } else if let Err(e) = bytehost_chrome::persist::save_atomic(&project_path, &self.shell) {
            self.boot_note = format!("布局保存失败:{e}");
        }
        // 窗口大小是窗口的属性,不是项目的:只有全局那份决定下次启动的窗口尺寸,且**只改尺寸**,
        // 其余字段(旧全局布局,新项目的初始值)字节不动。
        if self.shell_save_blocked.is_none()
            && let Err(e) = shell::save_window_size(&self.shell_path, &self.shell)
        {
            self.boot_note = format!("窗口大小未保存:{e}");
        }
    }

    /// 消失的项目页签:abort 转发并 kill 该项目全部会话。
    fn close_agent_for(&mut self, before: &[i64]) {
        for id in before
            .iter()
            .copied()
            .filter(|id| !self.projects.open_ids().contains(id))
        {
            let _ = self.agent.close_project(id);
        }
    }

    /// 切到当前项目后换布局:窗口大小沿用内存里的(窗口没变),其余读该项目自己的文件;
    /// 没有项目或项目没有文件时读全局布局。
    fn load_project_shell(&mut self) {
        let project = self.current_project_shell_path().filter(|p| p.exists());
        // 项目文件是更高 schema 版本写的:照骨架的规矩用默认布局显示,但**不回写**它。
        self.project_shell_blocked = project
            .as_deref()
            .and_then(|p| self.project_save_blocked(p));
        let window = self.shell.window();
        let mut model = shell::load_scoped(&self.shell_path, project.as_deref()).into_model();
        if model.window() != window {
            shell::update(
                &mut model,
                ShellMsg::Resized {
                    w: window.0,
                    h: window.1,
                },
            );
        }
        self.shell = model;
        // 换布局可能让某些面板从看不见变成看得见:在新布局上重算一次,别拿旧布局的可见性比较。
        self.visible_before = visible_panels(self);
    }

    /// 当前项目自己的布局文件;没有当前项目时为 `None`(整份写全局)。
    fn current_project_shell_path(&self) -> Option<PathBuf> {
        let id = self.projects.current_project_id()?;
        self.project_shell_path_for(id)
    }

    /// 某个项目的布局文件(与它是否当前项目无关)。
    fn project_shell_path_for(&self, id: i64) -> Option<PathBuf> {
        let root = self
            .shell_path
            .parent()
            .filter(|p| p != &paths::config_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(paths::config_dir);
        Some(project_shell_path_of(&root, id))
    }

    /// 该项目的布局文件若是更高 schema 版本写的,给它一条说明;调用方据此拒绝回写。
    fn project_save_blocked(&self, project_path: &Path) -> Option<String> {
        match shell::load_scoped(&self.shell_path, Some(project_path)) {
            Loaded::Newer { found, .. } => Some(format!(
                "项目布局 schema 版本 {found} 高于本宿主支持的 {SCHEMA_VERSION},已用默认布局显示且**不覆盖原文件**"
            )),
            _ => None,
        }
    }

    /// 面板此刻是否真的呈现在用户眼前:它是所在栏的选中面板、所在区没收起、也没被另一侧的放大态盖住。
    pub(crate) fn panel_visible(&self, panel: &PanelId) -> bool {
        let Some(side) = self.shell.side_of(panel) else {
            return false;
        };
        self.shell.selected(side) == panel
            && !self.shell.collapsed(side)
            && self.shell.maximized().is_none_or(|m| m == side)
    }

    /// 用量面板此刻的三态(唯一判断,见 [`UsageLayout`])。
    pub(crate) fn usage_layout(&self) -> UsageLayout {
        if !self.usage.has_agent_filter() {
            return UsageLayout::ContentOnly;
        }
        if self.pane_collapsed(&shell::usage_id()) {
            UsageLayout::Collapsed
        } else {
            UsageLayout::Paired
        }
    }

    /// 配对面板的第一块此刻是否被收起(骨架的 `PaneStore` 记账)。
    fn pane_collapsed(&self, panel: &PanelId) -> bool {
        self.shell.panes().get(panel).is_some_and(|d| d.collapsed)
    }

    /// 面板刚被切入(或启动时本来就可见):触发各自的"切入时动作"。
    fn on_visible(&mut self, panel: &PanelId) -> Task<Msg> {
        if *panel == shell::apps_id() {
            // 首次进入「应用」面板:触发一次探测 + 拉列表(Task 14)。
            return self
                .apps
                .update(crate::apps::Message::Opened)
                .map(Msg::Apps);
        }
        if *panel == shell::usage_id() {
            // 进入「用量」面板:触发一次取数(库的 on_activate,Task 20)。
            self.usage.activate();
        }
        if *panel == shell::conversations_id() {
            return self.activate_conversations_panel();
        }
        if *panel == shell::group_chat_id() {
            return self.activate_group_chat_panel();
        }
        if *panel == shell::browser_id() {
            // webview 子视图的 ⌘A/⌘C/⌘V/⌘X 要靠应用主菜单里的 Edit 菜单(wry 的子视图不处理 performKeyEquivalent,
            // 探针实测);Digger 自己不装主菜单,所以第一次显示浏览器时装一个标准的。幂等、仅主线程。
            #[cfg(target_os = "macos")]
            bytehost_desktop::web_child::install_standard_edit_menu();
            return match self.projects.current_project().cloned() {
                Some(project) => self.browser.activate(&project),
                None => Task::none(),
            };
        }
        if *panel == shell::todo_id() {
            self.activate_todo_panel();
        }
        if *panel == shell::project_id() {
            // 进入「项目」面板:当前项目的信息/文档/记忆一次拉齐(库的 on_activate)。
            self.activate_project_panel();
        }
        if *panel == shell::files_id() {
            // 进入「文件」面板:把树根挂到当前项目并拉一次 git 文件状态。
            self.activate_files_panel();
        }
        if *panel == shell::agent_id() {
            // 首次可见才恢复/新建该项目的会话(D1 懒启动)。
            if let Some(project) = self.projects.current_project().cloned() {
                let grid = self.current_agent_grid().unwrap_or((80, 24));
                return self.agent.ensure_started(&project, grid);
            }
        }
        Task::none()
    }

    fn activate_conversations_panel(&mut self) -> Task<Msg> {
        let Some(project) = self.projects.current_project().cloned() else {
            return Task::none();
        };
        self.conversations.ensure_state(&project);
        if self.agent.shell.status == crate::adapters::agent::Status::Ready {
            self.conversations.activate(&project)
        } else {
            Task::none()
        }
    }

    fn activate_group_chat_panel(&mut self) -> Task<Msg> {
        let Some(project) = self.projects.current_project().cloned() else {
            self.group_chat.set_focus(None);
            return Task::none();
        };
        self.group_chat
            .activate(&project, std::time::Instant::now())
    }

    /// 当前项目从 `before` 换成了现在这个(页签点击 / 关闭 / 删除 / 列表修剪都走这里):
    /// 把**旧**项目的布局落到它自己的文件(`save_old`,已删除的项目不写),换上新项目的布局,
    /// 并让 Project 面板跟到新项目。当前项目没变时什么都不做。
    fn on_current_project_changed(&mut self, before: Option<i64>, save_old: bool) {
        if before == self.projects.current_project_id() {
            return;
        }
        if save_old && before.is_some() {
            self.save_shell_for(before);
        }
        // D4:切走项目即销毁该项目的预览页面与内存状态(不常驻)。这不是删除:
        // 已落盘的页签文件与其它项目的白名单都留着,切回来能恢复(G-C1)。
        let current = self.projects.current_project().cloned();
        if let Some(preview) = self.preview.as_mut() {
            preview.set_current_project(current.as_ref());
        }
        if let Some(old) = before
            && before != current.as_ref().map(|p| p.id)
            && let Some(bridge) = &self.preview_bridge
        {
            // 旧项目不再有前台上下文:立刻推 None,避免 Agent 读到旧选区。
            bridge.push(old, None, 0);
        }
        if current.is_none() {
            // 没有前台项目时轮询器跳过,别继续拿旧项目 id 去问。
            self.preview_project.store(0, Ordering::SeqCst);
        }
        self.sync_preview_context(true);
        self.load_project_shell();
        if self.panel_visible(&shell::project_id()) {
            self.activate_project_panel();
        }
        self.activate_files_panel();
        self.activate_todo_panel();
        if self.panel_visible(&shell::conversations_id()) {
            let _ = self.activate_conversations_panel();
        }
        if let Some(old) = before
            && before != self.projects.current_project_id()
        {
            self.group_chat.cancel_dialog(old);
        }
        self.group_chat
            .set_focus(self.projects.current_project_id());
        if self.panel_visible(&shell::group_chat_id()) {
            let _ = self.activate_group_chat_panel();
        }
        // 浏览器已经在显示时切项目:新项目要有自己的状态(否则面板是空白,直到重新切入面板)。
        if self.panel_visible(&shell::browser_id())
            && let Some(project) = self.projects.current_project().cloned()
        {
            let _ = self.browser.activate(&project);
        }
    }

    /// 把待办面板切到当前项目;面板可见时立即拉一次列表与分类。
    fn activate_todo_panel(&mut self) {
        let project = self.projects.current_project().cloned();
        self.todo.set_project(project.as_ref().map(|p| p.id));
        if self.panel_visible(&shell::todo_id())
            && let Some(project) = project
        {
            self.todo.activate(Some(PathBuf::from(project.path)));
        }
    }

    /// 把 Project 面板切到当前项目(切入面板 / 换项目页签时都走这里)。没有项目时什么都不做。
    fn activate_project_panel(&mut self) {
        let Some(project) = self.projects.current_project().cloned() else {
            return;
        };
        self.project.activate(&project);
    }

    /// 把「文件」面板的树根切到当前项目。没有项目时清空。
    fn activate_files_panel(&mut self) {
        let project = self.projects.current_project().cloned();
        self.files.set_project(project.as_ref());
    }

    /// 「删除项目」二次确认卡片点"删除":先取走待确认的层级(库的 `delete_pending`),
    /// 关掉产品页签,再依次 delist + 删文件系统(库的 `spawn_delete_project`)。
    /// 与 Dozer 的 `project_delete_confirm` 同序:关页签必须在发起删除之前。
    fn project_delete_confirm(&mut self, project: &dozer_model::ProjectInfo) {
        let Some(scope) = self.project.take_delete_pending(project.id) else {
            return;
        };
        // G-C1:切项目只是换当前项目;只有真删除才回收这个项目的预览状态、
        // 白名单与落盘文件。
        if let Some(preview) = self.preview.as_mut() {
            preview.forget_project(project.id);
        }
        let before = self.projects.current_project_id();
        let close = self
            .projects
            .update(crate::projects::Message::Close(project.id));
        let _ = close;
        let _ = self.agent.close_project(project.id);
        self.conversations.close_project(project.id);
        self.browser.close_project(project.id);
        self.group_chat.close_project(project.id);
        self.on_current_project_changed(before, before != Some(project.id));
        let (project, tx) = (project.clone(), self.delete_tx.clone());
        self.project
            .delete_project(project, scope, move |project_id, deregistered, errors| {
                let _ = tx.send(Msg::ProjectDeleteDone {
                    project_id,
                    deregistered,
                    errors,
                });
            });
    }

    /// 排空 Project 面板待发提示。Digger 一期没有 Toast:落到 stderr,不静默吞掉。
    fn flush_project_notices(&mut self) {
        let Some(project_id) = self.project.focused_project() else {
            return;
        };
        for (level, text) in self.project.take_notices(project_id) {
            eprintln!("digger: project 面板/{level}: {text}"); // cli-output
        }
    }

    fn on_shell(&mut self, msg: ShellMsg) -> Task<Msg> {
        if self.closing {
            return Task::none();
        }
        // Keep normal geometry during minimize animation, and reject unusable
        // dimensions even before the OS confirms minimization. All size sources
        // (mapped events, WindowReady, samples) pass this boundary before the shared
        // model or persistence. Msg::Shell still queries the OS after rejection;
        // a restore sample clears minimized state before applying its dimensions.
        if let ShellMsg::Resized { w, h } = &msg
            && (self.window_minimized || !w.is_finite() || !h.is_finite() || *w <= 0. || *h <= 0.)
        {
            return Task::none();
        }
        let resized = matches!(msg, ShellMsg::Resized { .. });
        if let ShellMsg::CursorMoved { x, y } = msg {
            // 文件树的拖拽幽灵胶囊要跟着光标走(展示语义读这两个值)。
            let size = (self.shell.window().0, self.shell.window().1);
            self.files.set_window_probe(size, (x, y));
            self.last_cursor = iced::Point::new(x, y);
            // 树内拖拽:越过距离 + 按住时长两道阈值才算真拖拽(否则只是单击)。
            self.files.maybe_confirm_tree_drag(self.left_down);
        }
        let out = shell::update(&mut self.shell, msg);
        publish_panes(self.shell.panes());
        if out.persist {
            self.save_shell();
        }
        let mut tasks = Vec::new();
        // "切入时动作"挂在"从看不见变成看得见"上,而不只是"被切入":展开收起的区、退出放大态、跨栏搬走对侧面板后
        // 本栏退回的面板,都没有 `switched_in`,但用户眼前出现了这个面板。动作本身幂等,与 `switched_in` 重叠无害。
        let visible_now = visible_panels(self);
        let mut entered: Vec<PanelId> = out.switched_in.into_iter().collect();
        for (panel, (now, was)) in shell::all_panels()
            .into_iter()
            .zip(visible_now.iter().copied().zip(self.visible_before.clone()))
        {
            if now && !was && !entered.contains(&panel) {
                entered.push(panel);
            }
        }
        for panel in entered {
            tasks.push(self.on_visible(&panel));
        }
        // 应用面板从可见变成不可见(切走 / 收起 / 被放大盖住):关掉正在显示的应用,别留一个看不见的 webview 盖在别处。
        let apps = shell::all_panels()
            .iter()
            .position(|p| *p == shell::apps_id());
        if apps.is_some_and(|i| {
            self.visible_before.get(i).copied().unwrap_or(false) && !visible_now[i]
        }) {
            self.apps.close_selected();
        }
        if self.agent.picker.is_some() && !self.panel_visible(&shell::agent_id()) {
            self.agent.picker = None;
        }
        // 预览面板失焦(切到别的面板 / 收起):上下文此刻不再变化,立即刷新一次,
        // 别让 Agent 在上一次 250ms 防抖窗口里读到旧选区。
        let preview_was = preview_panel_visible(&self.visible_before);
        let preview_now = preview_panel_visible(&visible_now);
        if preview_was && !preview_now {
            self.sync_preview_context(true);
        }
        self.visible_before = visible_now;
        if resized {
            tasks.push(self.restyle_window());
        }
        Task::batch(tasks)
    }

    /// macOS:红绿灯垂直居中到顶栏里;窗口缩放(含全屏进出)后重设,幂等。其它平台无事可做。
    fn restyle_window(&self) -> Task<Msg> {
        #[cfg(target_os = "macos")]
        if let Some(id) = self.window {
            let top = byteui::theme::geometry::top_bar_height();
            return window::run(id, move |w| {
                bytehost_chrome::platform_macos::center_traffic_lights(w, top)
            })
            .discard();
        }
        Task::none()
    }

    /// Sample both values on the runtime's window thread. A newer request, replacement
    /// window, or closing owner makes an asynchronous response obsolete.
    fn query_window_state(&mut self) -> Task<Msg> {
        let Some(id) = self.window.filter(|_| !self.closing) else {
            return Task::none();
        };
        self.window_state_request = self.window_state_request.wrapping_add(1);
        let request = self.window_state_request;
        window::is_minimized(id).then(move |minimized| {
            window::size(id).map(move |size| Msg::WindowStateSample {
                id,
                request,
                minimized,
                size: Some(size),
            })
        })
    }

    /// 原生 webview 的位置与可见性:矩形取自骨架的面板内容矩形(外框 − 外边距 − 内边距;放大态是放大盒子),
    /// 再各自减掉面板自己头部占的高度。**骨架负责"在哪、看不看得见",面板只负责自己头部多高。**
    fn sync_native(&mut self) -> Task<Msg> {
        let Some(window) = self.window else {
            return Task::none();
        };
        let rects = self.shell.rects(&metrics_from_theme());
        let content_of = |panel: &PanelId| -> Option<Rect> {
            let side = self.shell.side_of(panel)?;
            if self.shell.selected(side) != panel {
                return None;
            }
            rects.content(side)
        };
        let apps_rect = content_of(&shell::apps_id());
        let usage_rect = content_of(&shell::usage_id()).map(|content| {
            let pair = self
                .shell
                .side_of(&shell::usage_id())
                .and_then(|side| shell::pair_rects(&self.shell, side));
            usage_native_rect(self.usage_layout(), content, pair)
        });
        let todo_rect = content_of(&shell::todo_id()).map(|content| {
            let pair = self
                .shell
                .side_of(&shell::todo_id())
                .and_then(|side| shell::pair_rects(&self.shell, side));
            let add_h = self
                .todo
                .current_state()
                .map(|s| s.add_input_height())
                .unwrap_or(dozer_panel_todo::ADD_INPUT_MIN_HEIGHT);
            let pair = if self.pane_collapsed(&shell::todo_id()) {
                None
            } else {
                pair
            };
            todo_native_rect(content, pair, add_h)
        });
        let review_rect = content_of(&shell::conversations_id()).map(|content| {
            let collapsed = self.pane_collapsed(&shell::conversations_id());
            review_native_rect(
                &self.shell,
                content,
                collapsed,
                self.conversations.review_notice_height(collapsed),
            )
        });
        let chat_rect = content_of(&shell::group_chat_id()).map(|content| {
            let collapsed = self.pane_collapsed(&shell::group_chat_id());
            group_chat_native_rect(&self.shell, content, collapsed)
        });
        let markdown_rect = content_of(&shell::markdown_id());
        let hidden = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let closing = self.closing;

        let apps_task = match apps_rect {
            Some(r) if !closing => self
                .apps
                .sync_native(window, logical(below_header(r, APPS_HEADER_HEIGHT)))
                .map(Msg::Apps),
            // 关闭时由 `apps.prepare_shutdown` 收尾;不可见时 `on_shell` 已 `close_selected`,这里仍要让待办的原生动作跑完。
            _ => self.apps.sync_native(window, hidden).map(Msg::Apps),
        };
        let usage_task = self
            .usage
            .sync_native(
                window,
                usage_rect.is_some() && !closing,
                usage_rect
                    .map(|r| logical(below_header(r, USAGE_HEADER_HEIGHT)))
                    .unwrap_or(hidden),
            )
            .map(Msg::NativeUsage);
        // 关闭时销毁编辑器 webview 并丢掉文档;否则只在可见时显示,切走则隐藏。
        let markdown_task = if closing {
            self.markdown.close(window)
        } else {
            self.markdown.sync_native(
                window,
                markdown_rect.is_some(),
                markdown_rect.map(logical).unwrap_or(hidden),
            )
        }
        .map(Msg::Markdown);
        let todo_task = self
            .todo
            .sync_native(
                window,
                todo_rect.is_some() && !closing,
                todo_rect.map(logical).unwrap_or(hidden),
            )
            .map(Msg::NativeTodo);
        let review_task = self
            .conversations
            .sync_native(
                window,
                review_rect.is_some()
                    && !closing
                    && !self.window_minimized
                    && !self.review_overlay_open(),
                review_rect.map(logical).unwrap_or(hidden),
                self.projects.current_project_id(),
            )
            .map(Msg::ReviewNative);
        let chat_overlay = self.chat_overlay_open();
        let chat_task = self
            .group_chat
            .sync_native(
                window,
                chat_rect.is_some() && !closing && !self.window_minimized,
                chat_overlay,
                chat_rect.map(logical).unwrap_or(hidden),
                self.projects.current_project_id(),
                std::time::Instant::now(),
            )
            .map(Msg::GroupChatNative);
        let browser_task = self.browser.sync_native(
            window,
            crate::adapters::browser::NativeInputs {
                project_id: (!closing)
                    .then(|| self.projects.current_project_id())
                    .flatten(),
                content: content_of(&shell::browser_id()),
                window_active: !closing && !self.window_minimized,
                overlay_open: self.chat_overlay_open(),
            },
        );
        let preview_tasks = self.sync_preview_native(&rects, closing);
        Task::batch([
            browser_task,
            chat_task,
            apps_task,
            usage_task,
            markdown_task,
            todo_task,
            review_task,
            Task::batch(preview_tasks),
        ])
    }

    /// iced 内的模态/浮层打开时聊天页停放并隐藏(原生菜单不算,它是独立窗口)。
    fn chat_overlay_open(&self) -> bool {
        self.preview_overlay_open()
            || self.agent.overlay_is_open()
            || self
                .group_chat
                .state(self.projects.current_project_id().unwrap_or_default())
                .is_some_and(|s| s.new_group_dialog_open())
            || self.projects.current_project().is_some_and(|p| {
                self.agent
                    .states
                    .get(&p.id)
                    .is_some_and(|s| s.pending_close_tab.is_some())
            })
    }

    /// Review also yields to Agent and Conversations overlays.
    fn review_overlay_open(&self) -> bool {
        self.preview_overlay_open()
            || self.agent.overlay_is_open()
            || self
                .group_chat
                .state(self.projects.current_project_id().unwrap_or_default())
                .is_some_and(|s| s.new_group_dialog_open())
            || self.projects.current_project().is_some_and(|p| {
                self.conversations
                    .state(p.id)
                    .is_some_and(|s| s.agent_picker_open())
                    || self
                        .agent
                        .states
                        .get(&p.id)
                        .is_some_and(|s| s.pending_close_tab.is_some())
            })
    }
    /// 此刻有没有 iced 浮层会被原生预览 webview 盖住:任一预览面板的页签溢出菜单、Files 的右键菜单 /
    /// 对话框、Project 的删除确认卡。
    fn preview_overlay_open(&self) -> bool {
        let Some(project) = self.projects.current_project() else {
            return false;
        };
        let overflow = |panel: ViewerPanel| {
            self.preview
                .as_ref()
                .and_then(|hub| hub.state(panel, project.id))
                .is_some_and(|state| state.overflow_anchor.is_some())
        };
        overflow(ViewerPanel::Files)
            || overflow(ViewerPanel::Project)
            || self.files.menu().is_some()
            || self.files.has_modal()
            || self.project.delete_card(project.id).is_some()
    }

    /// 预览面板的原生页面:配对第二块(预览)的位置 = 骨架算好的 slot 再扣掉页签条 +
    /// 内容区自己画出来的内边距(与库的 `view::pane` 同口径,见 [`preview_webview_rect`])。
    /// 面板不可见 / 没有当前项目 / 正在关闭时清单为空,宿主销毁全部页面(D4)。
    ///
    /// 返回要等窗口线程建好页面之后再回投的注入任务:新建的句柄在 `window::run`
    /// 闭包里才出现,同一轮 `evaluate_script` 打不进还不存在的页面。
    fn sync_preview_native(
        &mut self,
        rects: &bytehost_chrome::ShellRects,
        closing: bool,
    ) -> Vec<Task<Msg>> {
        let Some(window) = self.window else {
            return Vec::new();
        };
        let overlay_open = self.preview_overlay_open();
        let Some(mut preview) = self.preview.take() else {
            return Vec::new();
        };
        // G-C5:没有当前项目 / 正在关闭时不能直接返回——那会把上一次的页面
        // 留在窗口上。同步一个空清单,`plan()` 就会销毁所有活着的预览 webview。
        let Some(project) = preview_project(self.projects.current_project(), closing) else {
            let host = preview.host().clone();
            self.preview = Some(preview);
            return vec![native_preview_task(window, host, Vec::new(), Vec::new())];
        };
        let tasks = Self::preview_native_tasks(
            &mut preview,
            &self.shell,
            rects,
            &project,
            window,
            overlay_open,
        );
        self.preview = Some(preview);
        tasks
    }

    fn preview_native_tasks(
        preview: &mut crate::adapters::preview::PreviewHub,
        shell: &ShellModel,
        rects: &bytehost_chrome::ShellRects,
        project: &dozer_model::ProjectInfo,
        window: window::Id,
        overlay_open: bool,
    ) -> Vec<Task<Msg>> {
        let desired = collect_preview_desired(preview, shell, rects, project, overlay_open);
        // 建 / 挪 / 销毁在窗口线程内同步完成;脚本注入排在它之后。
        let host = preview.host().clone();
        let mut scripts = Vec::new();
        for panel in [ViewerPanel::Files, ViewerPanel::Project] {
            scripts.extend(preview.host_scripts(panel, project.id));
        }
        vec![native_preview_task(window, host, desired, scripts)]
    }

    fn info_view(&self) -> Element<'_, Msg> {
        let tokens = color::current();
        let dim = tokens.dim;
        container(
            column![
                text(format!(
                    "面板目录已注册 {} 个(产品组合根)",
                    self.catalog.len()
                ))
                .color(tokens.body),
                text(format!("数据根:{}", paths::config_dir().display())).color(dim),
                text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
                text(match self.projects.current_project() {
                    Some(p) => format!("当前 Project:{}({})", p.name, p.path),
                    None => "当前 Project:无".to_string(),
                })
                .color(tokens.body),
                text(&self.boot_note).color(tokens.gold),
                iced::widget::button("退出 Digger").on_press(Msg::Exit),
            ]
            .spacing(8)
            .width(Length::Fill),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn apps_view(&self) -> Element<'_, Msg> {
        container(self.apps.view().map(Msg::Apps))
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn usage_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let tokens = color::current();
        let body: Element<'_, Msg> = match self.usage_layout() {
            UsageLayout::ContentOnly => self
                .usage
                .content_view(self.pane_collapsed(&shell::usage_id())),
            UsageLayout::Collapsed => self.usage.content_view(true),
            // 列表是配对的第一块;"内容在前还是列表在前"由骨架按镜像算好,`pair_view` 直接读。
            UsageLayout::Paired => pair_view(
                area,
                self.usage.list_view(),
                self.usage.content_view(false),
                Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
                tokens.bg,
                tokens.panel,
            ),
        };
        container(body)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn conversations_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        self.conversations
            .view(self.projects.current_project(), area, &self.agent.shell)
    }

    fn todo_view<'a>(&'a self, area: &BodyArea) -> Element<'a, Msg> {
        let tokens = color::current();
        let collapsed = self.pane_collapsed(&shell::todo_id());
        let (list, content) = self.todo.panes(collapsed);
        let body: Element<'_, Msg> = if collapsed {
            content
        } else {
            pair_view(
                area,
                list,
                content,
                Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
                tokens.bg,
                tokens.panel,
            )
        };
        container(body)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn project_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let current = self.projects.current_project();
        // 配对的第一块是项目信息列(D3:列表在左、预览在右)。列宽取骨架算好的 slot 宽度;
        // 列表收起时 `pair_view` 只渲染预览,宽度用整个内容区也无妨。
        let list_w = area.pair.map(|p| p.first.w).unwrap_or(area.rect.w);
        // 整个列表区域接一记"按下":编辑中点到别处就提交(见 `Msg::ProjectPointerDown`)。
        let list: Element<'_, Msg> = iced::widget::mouse_area(self.project.view(
            current,
            Length::Fixed(list_w),
            Border::default(),
        ))
        .on_press(Msg::ProjectPointerDown)
        .into();
        let tokens = color::current();
        let body: Element<'_, Msg> = pair_view(
            area,
            list,
            self.preview_pane(ViewerPanel::Project),
            Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
            tokens.panel,
            // 预览区背景与 Dozer 的 `theme::region::preview_pane()` 同值(见 `PreviewDisplay::pane_style`),
            // 分隔线两侧因此是两种底色。
            crate::adapters::preview_display::PreviewDisplay::background(),
        );
        // 删除确认是覆盖在面板上的三选一卡片(库的 `project_delete_card`),有才盖。
        let overlay: Option<Element<'_, Msg>> = current
            .and_then(|p| self.project.delete_card(p.id))
            .map(|card| {
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .into()
            });
        match overlay {
            Some(card) => iced::widget::stack![body, card].into(),
            None => body,
        }
    }

    fn files_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let tokens = color::current();
        let list_w = area.pair.map(|p| p.first.w).unwrap_or(area.rect.w);
        pair_view(
            area,
            self.files.view(Length::Fixed(list_w), Border::default()),
            self.preview_pane(ViewerPanel::Files),
            Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
            tokens.panel,
            crate::adapters::preview_display::PreviewDisplay::background(),
        )
    }

    /// Hidden Agent panes, focused conversation search and iced overlays block terminal keys.
    pub(crate) fn agent_keys_blocked(&self) -> bool {
        !self.panel_visible(&shell::agent_id())
            || self
                .conversation_search_project()
                .and_then(|pid| self.conversations.state(pid))
                .is_some_and(|state| state.search_focused() || state.agent_picker_open())
            || self.preview_overlay_open()
            || self.agent.overlay_is_open()
            || self
                .group_chat
                .state(self.projects.current_project_id().unwrap_or_default())
                .is_some_and(|s| s.new_group_dialog_open())
            || self
                .projects
                .current_project()
                .and_then(|p| self.agent.states.get(&p.id))
                .is_some_and(|state| state.pending_close_tab.is_some())
    }

    fn conversation_search_project(&self) -> Option<i64> {
        (self.panel_visible(&shell::conversations_id())
            && !self.pane_collapsed(&shell::conversations_id())
            && self.agent.shell.status == crate::adapters::agent::Status::Ready
            && !self.closing)
            .then(|| self.projects.current_project_id())
            .flatten()
    }

    /// 浏览器面板此刻可见(没收起、没被放大的另一侧盖住)时,它所属的当前项目;否则 `None`。
    fn browser_project(&self) -> Option<i64> {
        (self.panel_visible(&shell::browser_id())
            && !self.pane_collapsed(&shell::browser_id())
            && !self.closing)
            .then(|| self.projects.current_project_id())
            .flatten()
    }

    fn run_conversation_requests(&mut self) -> Task<Msg> {
        #[cfg(target_os = "macos")]
        let mut tasks = vec![];
        for (pid, request) in self.conversations.take_requests() {
            if self.conversation_search_project() != Some(pid)
                || self.conversations.state(pid).is_none()
            {
                continue;
            }
            match request {
                dozer_panel_conversations::adapter::ConversationsRequest::ShowAgentFilter {
                    at,
                    current,
                    present,
                } => {
                    #[cfg(target_os = "macos")]
                    {
                        let items = crate::adapters::native_menu::conversation_filter_items(
                            pid, current, &present,
                        );
                        if let Some(message) = byteui::native_menu::show(items, at) {
                            tasks.push(Task::done(message));
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = (at, current, present);
                        self.show_iced_conversation_filter(pid);
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            Task::batch(tasks)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Task::none()
        }
    }

    #[cfg(any(not(target_os = "macos"), test))]
    fn show_iced_conversation_filter(&mut self, pid: i64) {
        self.dismiss_agent_overflow();
        self.agent.close_picker();
        if let Some(state) = self.agent.states.get_mut(&pid) {
            // A previous overflow can already have dismissed the product picker.
            state.agent_picker_open = false;
        }
        self.conversations.open_agent_filter(pid);
    }

    /// 关闭溢出下拉:产品态清掉,并让库把 `term_tab_overflow_anchor` 也归零。
    fn dismiss_agent_overflow(&mut self) {
        if let Some((pid, _)) = self.agent.overflow {
            let project = self.projects.project_by_id(pid);
            let grid = self.current_agent_grid().unwrap_or((80, 24));
            let _ = self.agent.update(
                pid,
                dozer_panel_agent::update::Message::OverflowDismiss,
                project.as_ref(),
                grid,
            );
        }
        self.agent.overflow = None;
    }

    /// Agent 面板的配对视图:列表(库的 `agent_list`)+ 终端条(库的
    /// `terminal_pane`)。只有守护进程可用且当前项目已有状态时才拼配对;其余
    /// 沿用 S6d-1 的壳(无项目 / 连接中 / 不可用 + 重试)。
    fn agent_view(&self, area: &BodyArea) -> Element<'_, Msg> {
        let Some(project) = self.projects.current_project() else {
            return self.agent.shell.view(false).map(Msg::AgentShell);
        };
        if !self.agent.pair_ready(Some(project.id)) {
            return self.agent.shell.view(true).map(Msg::AgentShell);
        }
        let state = &self.agent.states[&project.id];
        let pid = project.id;
        crate::adapters::agent::set_current_agent_project(pid);
        let block =
            crate::adapters::agent_layout::terminal_block(area.rect, area.collapsed, area.pair);
        let tab_bar_px = crate::adapters::agent_tabs::tab_bar_avail_px(
            block.w,
            byteui::theme::icon_size::scale(),
        );
        let display = crate::adapters::agent_display::AgentDisplay {
            panel: &self.agent,
            pid,
            term_style: &self.term_style,
            locale: "en",
            tab_bar_px,
            list_on_left: area.pair.is_some_and(|p| p.first.x <= p.second.x),
        };
        let list_w = area.pair.map(|p| p.first.w).unwrap_or(area.rect.w);
        let no_tasks = &NO_TASKS;
        let list: Element<'_, Msg> = dozer_panel_agent::view::list::agent_list(
            state,
            &display,
            &dozer_panel_agent::card::CardCtx {
                titles: no_tasks,
                project_branch: None,
                project_dirty: false,
            },
            Length::Fixed(list_w),
            Border::default(),
        );
        let term: Element<'_, Msg> = dozer_panel_agent::view::terminal::terminal_pane(
            state,
            &display,
            dozer_panel_agent::display::TerminalSlots {
                floating: None,
                footer: None,
            },
            Length::Fill,
            Border::default(),
        );
        // 输入法:只有终端获得焦点时才包 `ImeSink`(enabled = focus)。
        let term: Element<'_, Msg> = if self.agent.focus {
            let style = self.term_style.clone();
            let (col, row) = state
                .tabs
                .get(state.active)
                .map(|tab| tab.model.cursor())
                .unwrap_or((0, 0));
            let padding = crate::adapters::agent_display::terminal_pane_style().padding;
            let tab_h = crate::adapters::agent_layout::tab_bar_height();
            let cell_w = style.cell_width();
            let line_h = style.line_height_px();
            crate::adapters::agent_ime::ImeSink::new(
                term,
                true,
                move |bounds: iced_core::Rectangle| {
                    iced_core::Rectangle::new(
                        iced_core::Point::new(
                            bounds.x + padding.left + col as f32 * cell_w,
                            bounds.y + padding.top + tab_h + row as f32 * line_h,
                        ),
                        iced_core::Size::new(cell_w, line_h),
                    )
                },
                move |text| Msg::AgentImeCommit(pid, text),
                move |preedit| Msg::AgentPreedit(pid, preedit),
            )
            .into()
        } else {
            term
        };
        let tokens = color::current();
        let body: Element<'_, Msg> = pair_view(
            area,
            list,
            term,
            Msg::Shell(ShellMsg::DividerPress(Divider::Pair(area.side))),
            tokens.bg,
            tokens.term_bg,
        );
        let body = match self.agent.notice.text() {
            Some(notice) => {
                let line: Element<'_, Msg> = row![
                    text(notice.to_owned())
                        .size(byteui::theme::font::caption())
                        .color(tokens.red)
                        .width(Length::Fill),
                    button(text("×").color(tokens.dim)).on_press(Msg::AgentNoticeDismiss),
                ]
                .spacing(8)
                .padding([6, 12])
                .align_y(iced::Alignment::Center)
                .into();
                column![line, body].height(Length::Fill).into()
            }
            None => body,
        };
        // 关页签确认:库的 `close_confirm_spec`(只有 Running/AwaitingInput 才产生)。
        let confirm =
            dozer_panel_agent::picker::close_confirm_spec(state, "en", |m| Msg::Agent(pid, m)).map(
                |spec| {
                    centered_modal(
                        byteui::feedback::dialog::confirm(spec),
                        Msg::Agent(pid, dozer_panel_agent::update::Message::CloseCancel),
                    )
                },
            );
        match confirm {
            Some(card) => iced::widget::stack![body, card].into(),
            None => body,
        }
    }

    /// 回合结束:只在 Project 面板可见、且是当前项目、且状态已存在时刷新记忆。
    fn on_agent_turn_ended(&mut self, pid: i64) {
        if !self.panel_visible(&shell::project_id())
            || self.projects.current_project_id() != Some(pid)
            || !self.project.has_state(pid)
        {
            return;
        }
        if let Some(project) = self.projects.project_by_id(pid) {
            self.project.refresh_memories(&project);
        }
    }

    /// 当前项目的 Agent 终端块矩形(窗口坐标)。Agent 面板不是该侧选中面板时
    /// 没有可测量的配对几何,返回 `None`(沿用上次网格,不发错尺寸)。
    fn current_agent_block(&self) -> Option<Rect> {
        let side = self.shell.side_of(&shell::agent_id())?;
        if self.shell.selected(side) != &shell::agent_id() {
            return None;
        }
        let rects = self.shell.rects(&metrics_from_theme());
        let content = rects.content(side)?;
        let pair = shell::pair_rects(&self.shell, side);
        Some(crate::adapters::agent_layout::terminal_block(
            content,
            pane_collapsed_of(&shell::agent_id()),
            pair,
        ))
    }

    /// 当前项目的 Agent 网格(终端块减去内边距与页签条)。
    fn current_agent_grid(&self) -> Option<(u16, u16)> {
        let block = self.current_agent_block()?;
        Some(crate::adapters::agent_layout::grid(
            block,
            &crate::adapters::agent_display::terminal_pane_style(),
            crate::adapters::agent_layout::tab_bar_height(),
            &self.term_style,
        ))
    }

    /// 文件面板的库消息入口。
    ///
    /// 库里有几条消息的 `update` 是 `unreachable!`——它们**必须由宿主拦截**(跨内核边界):
    /// `TreeRowPress` / `TreeDragRelease` / `TreeRowDoubleClick` / `CopyPath` / `TextInputMenuOpen` /
    /// `FileHistoryRollbackPrevious` / `RequestSendToAgentTerminal`。漏一条,用户一点就崩溃
    /// (2026-10-20:单击文件名崩溃,就是 `TreeRowPress` 原样进了库)。这里逐条处理,其余才交给库。
    fn on_files_message(&mut self, message: dozer_panel_files::Message) -> Task<Msg> {
        use dozer_panel_files::Message as F;
        let project_id = self.projects.current_project_id();
        // 菜单锚点用点击时的光标(Dozer 由窗口事件层统一记 `RightClickAt`,Digger 没有)。
        if opens_a_context_menu(&message) {
            let cursor = self.last_cursor;
            let _ = self.files.update(
                F::RightClickAt {
                    x: cursor.x,
                    y: cursor.y,
                },
                project_id,
            );
        }
        match message {
            // 单击(按下):只武装拖拽,不展开 / 不打开;松开时由 `Msg::LeftButton(false)` 收尾,
            // 没越过阈值就是一次单击,只选中。
            F::TreeRowPress { path, is_dir } => {
                self.files.arm_tree_drag(path, is_dir);
                return Task::none();
            }
            // 通常由 `Msg::LeftButton(false)` 直接收尾;这里兜底同一语义。
            F::TreeDragRelease { shift, command } => {
                return self.files.release_tree_drag(shift, command, project_id);
            }
            // 双击:目录展开 / 折叠(同点箭头),文件在预览里打开(D3)。
            F::TreeRowDoubleClick { path, is_dir } => {
                if is_dir {
                    return self.files.update(F::Toggle(path), project_id);
                }
                return self.preview_open(ViewerPanel::Files, &path);
            }
            // 页签右键"关闭":菜单只带路径,按路径找到页签再经库的 `CloseTab` 关掉。
            F::TabContextMenuCloseTab(_, path) => {
                let close = self.files.update(F::ContextMenuClose, project_id);
                if let (Some(pid), Some(preview)) = (project_id, self.preview.as_mut()) {
                    preview.close_file_tab(pid, &path);
                }
                self.sync_preview_context_from_hub();
                return Task::batch([close, self.sync_native()]);
            }
            // 页签右键"从磁盘重新加载":按页签 id 推进它的重载,原生页面随之重建。
            F::TabReloadFromDisk(path) => {
                let close = self.files.update(F::ContextMenuClose, project_id);
                if let (Some(pid), Some(preview)) = (project_id, self.preview.as_mut()) {
                    preview.reload_file_tab(pid, &path);
                }
                return Task::batch([close, self.sync_native()]);
            }
            // 复制路径:写系统剪贴板,再收起右键菜单。
            F::CopyPath(path, kind) => {
                let root = self
                    .projects
                    .current_project()
                    .map(|p| PathBuf::from(&p.path))
                    .unwrap_or_else(|| path.clone());
                let text = dozer_panel_files::fs::path_string(kind, &path, &root);
                let close = self.files.update(F::ContextMenuClose, project_id);
                return Task::batch([iced::clipboard::write(text), close]);
            }
            // Digger 没有这三个能力(能力开关已关 / 没有通用输入框菜单):收到就忽略,绝不进库。
            F::TextInputMenuOpen(_)
            | F::FileHistoryRollbackPrevious(_)
            | F::RequestSendToAgentTerminal { .. } => {
                eprintln!("digger: files 面板消息在 Digger 里不适用,已忽略"); // cli-output
                return Task::none();
            }
            _ => {}
        }
        let task = self.files.update(message, project_id);
        // 库把"只有产品做得到"的事放进队列:右键菜单落到 iced 浮层,
        // 双击文件进预览(D3);选目录暂时无入口(能力已关)。
        let _ = self.files.take_requests();
        let mut task = task;
        // macOS:库要弹的菜单走原生 NSMenu(阻塞等用户选择,选中后把消息投回来)。原生菜单是
        // 真窗口层级,浮在 wry 页面之上,弹出时不用把文件内容藏起来。
        #[cfg(target_os = "macos")]
        {
            task = Task::batch([task, self.show_native_files_menus()]);
        }
        for request in self.files.take_host_requests() {
            if let Some(path) = crate::adapters::files::FilesPanel::open_path(&request) {
                task = Task::batch([task, self.preview_open(ViewerPanel::Files, path)]);
            }
            let _ = self.files.handle_host_request(&request);
        }
        // Digger 一期无 Toast:提示落到 stderr,不静默吞掉。
        for notice in self.files.take_notices() {
            eprintln!(
                "digger: files 面板/{:?}: {}",
                notice.level, notice.text.fallback
            ); // cli-output
        }
        task
    }

    /// 把当前项目 Files 面板的活动页签上下文推给 `diggerd`。
    /// `immediate` 为假时走 250ms 防抖(库用 `Effect::ContextChanged` 指定)。
    fn sync_preview_context(&mut self, immediate: bool) {
        let Some(project) = self.projects.current_project().cloned() else {
            return;
        };
        self.preview_project.store(project.id, Ordering::SeqCst);
        let context = self
            .preview
            .as_ref()
            .and_then(|hub| hub.state(ViewerPanel::Files, project.id))
            .and_then(|state| dozer_panel_preview::build_preview_context(&state.pane, now_ms()));
        if let Some(bridge) = &self.preview_bridge {
            bridge.push(project.id, context, if immediate { 0 } else { 250 });
        }
    }

    /// 排空库排出的"该推上下文了"标记。
    fn sync_preview_context_from_hub(&mut self) {
        let push = self
            .preview
            .as_mut()
            .and_then(|hub| hub.take_context_push());
        if let Some(immediate) = push {
            self.sync_preview_context(immediate);
        }
    }

    /// 启动 Agent 预览命令轮询(幂等;在 `diggerd` 就绪后调用)。
    fn start_preview_poller(&mut self) {
        let Some(bridge) = self.preview_bridge.clone() else {
            return;
        };
        let tx = self.preview_cmds_tx.clone();
        bridge.spawn_poller(
            Arc::clone(&self.preview_project),
            Arc::clone(&self.preview_poller_started),
            move |pid, commands| {
                let _ = tx.send((pid, commands));
            },
        );
    }

    /// 取走预览命令回投接收端(只能一次,`boot` 接成 iced 流)。
    pub fn take_preview_cmds_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<PreviewCommands>> {
        self.preview_cmds_rx
            .lock()
            .ok()
            .and_then(|mut rx| rx.take())
    }

    /// D3:在右半边的预览里打开一个路径(文件树双击 / Project 文档链接)。
    /// 没有当前项目时什么都不做(预览本来就按项目存)。
    /// macOS:Agent 页签溢出下拉的原生菜单,出现在点击处(光标在溢出按钮上)。选中走
    /// `SelectTabNoDrag`:从菜单选的不是"按下页签",不能武装拖拽——菜单吞掉了鼠标松开事件,
    /// 拖拽状态会一直挂着,之后鼠标扫过页签就会被换位。
    #[cfg(target_os = "macos")]
    fn show_native_agent_overflow(&mut self, pid: i64) -> Task<Msg> {
        if !self.projects.open_ids().contains(&pid) {
            return Task::none();
        }
        let entries = self.agent.overflow_entries(pid);
        if entries.is_empty() {
            return Task::none();
        }
        let at = (self.last_cursor.x, self.last_cursor.y);
        let items = crate::adapters::native_menu::tab_overflow_items(&entries, move |index| {
            Msg::Agent(
                pid,
                dozer_panel_agent::update::Message::SelectTabNoDrag(index),
            )
        });
        match byteui::native_menu::show(items, at) {
            Some(message) => Task::done(message),
            None => Task::none(),
        }
    }

    /// macOS:预览页签溢出下拉的原生菜单。菜单出现在点击处(光标在溢出按钮上)。
    #[cfg(target_os = "macos")]
    fn show_native_preview_overflow(
        &mut self,
        panel: ViewerPanel,
        project_id: i64,
        epoch: u64,
    ) -> Task<Msg> {
        let Some(preview) = self
            .preview
            .as_ref()
            .filter(|p| p.accepts(project_id, epoch))
        else {
            return Task::none();
        };
        let entries = preview.overflow_entries(panel, project_id);
        if entries.is_empty() {
            return Task::none();
        }
        let at = (self.last_cursor.x, self.last_cursor.y);
        let items = crate::adapters::native_menu::tab_overflow_items(&entries, move |index| {
            Msg::Preview(
                panel,
                project_id,
                epoch,
                dozer_panel_preview::Message::SelectTab(index),
            )
        });
        match byteui::native_menu::show(items, at) {
            Some(message) => Task::done(message),
            None => Task::none(),
        }
    }

    /// macOS:逐条弹出库排出的原生菜单,选中项作为 `Msg::Files` 投回。`show` 同步阻塞到用户
    /// 选完(Dozer 同样如此);窗口内容 view 没登记时 `show` 返回 `None`,什么也不发生。
    #[cfg(target_os = "macos")]
    fn show_native_files_menus(&mut self) -> Task<Msg> {
        let mut tasks = Vec::new();
        for (items, at) in self.files.take_menu_requests() {
            let native = crate::adapters::native_menu::to_native_items(items);
            if let Some(message) = byteui::native_menu::show_align_no_icon_left(native, at) {
                tasks.push(Task::done(Msg::Files(message)));
            }
        }
        Task::batch(tasks)
    }

    /// 某个预览面板此刻的页签数(没有状态时为 `None`)。
    fn preview_tab_count(&self, panel: ViewerPanel, project_id: i64) -> Option<usize> {
        self.preview
            .as_ref()
            .and_then(|p| p.state(panel, project_id))
            .map(|state| state.pane.tabs().len())
    }

    fn preview_open(&mut self, panel: ViewerPanel, path: &Path) -> Task<Msg> {
        let Some(project) = self.projects.current_project().cloned() else {
            return Task::none();
        };
        if let Some(preview) = self.preview.as_mut() {
            preview.open(panel, &project, path.to_path_buf(), None);
        }
        self.sync_preview_context_from_hub();
        self.sync_preview_context(true);
        self.sync_native()
    }

    /// 配对第二块:预览面板本体(库的 `view::pane`)。没有当前项目 / 预览层还没装上时
    /// 给一块空面板,配对几何不变。
    fn preview_pane<'a>(&'a self, panel: ViewerPanel) -> Element<'a, Msg> {
        let (Some(preview), Some(current)) =
            (self.preview.as_ref(), self.projects.current_project())
        else {
            return iced::widget::Space::new().into();
        };
        let Some(state) = preview.state(panel, current.id) else {
            return iced::widget::Space::new().into();
        };
        // N1b:展示上下文每帧原地更新(见 `preview_displays` 字段),不再泄漏。
        // 项目 id 每次写入,避免把旧项目固化进去(G-C2)。
        // `preview_pane` 只被 Files / Project 两个面板调用(见 `view`),
        // 其它面板落到 0 槽也不会有调用者读到。
        let slot = if panel == ViewerPanel::Project { 0 } else { 1 };
        let epoch = preview.epoch();
        let mut display = preview_display(preview.tab_bar_px(panel), current.id, epoch);
        display.hover = self.preview_hover.current();
        self.preview_displays[slot].set(display);
        // SAFETY: 见 `preview_displays` 字段说明——槽位在壳的生命周期内地址
        // 固定,里面是 `Copy` 的纯数据;内容在本帧内不会再被写。
        let display: &'a crate::adapters::preview_display::PreviewDisplay<'a> =
            unsafe { &*self.preview_displays[slot].as_ptr() };
        crate::adapters::preview_display::pane(display, state, panel, current, Length::Fill)
    }

    fn markdown_view(&self) -> Element<'_, Msg> {
        // 条在上;下面留空,原生 webview 由 `sync_native` 摆在条的下方。
        column![
            self.markdown.view().map(Msg::Markdown),
            iced::widget::Space::new().height(Length::Fill)
        ]
        .into()
    }
}

/// 生命周期自检要读的东西。
impl ProductShell {
    pub fn window_id(&self) -> Option<window::Id> {
        self.window
    }

    pub fn window_size(&self) -> (f32, f32) {
        self.shell.window()
    }

    pub fn markdown_ready(&self) -> bool {
        self.markdown.is_ready()
    }

    pub fn markdown_last_probe(&self) -> Option<crate::markdown::Probe> {
        self.markdown.last_probe()
    }

    pub fn markdown_probe(&self, id: window::Id) -> Task<Msg> {
        self.markdown.probe(id).map(Msg::Markdown)
    }

    /// 取走「删除项目」结果的接收端(只能取一次;`boot` 用它接成 iced 流)。
    pub fn take_delete_receiver(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<Msg>> {
        self.delete_rx.lock().ok().and_then(|mut r| r.take())
    }

    pub fn usage_ready(&self) -> bool {
        self.usage.native.is_ready()
    }

    pub fn usage_last_probe(&self) -> Option<crate::adapters::usage_native::Probe> {
        self.usage.native.last_probe()
    }

    pub fn usage_probe(&self, id: window::Id) -> Task<Msg> {
        self.usage.native.probe(id).map(Msg::NativeUsage)
    }
}

/// 顶栏三个槽位:标题只是展示;页签行与"＋"由项目注册表状态(`projects`)画。
fn top_bar_slots<'a>(projects: &'a crate::projects::ProjectsState) -> TopBarSlots<'a, Msg> {
    let tokens = color::current();
    let dim = tokens.dim;
    let title = row![
        icons::view(IconKind::Home, icon_size::home(), dim),
        text("Digger").size(font::body()).color(dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let tabs = projects.tabs_view().map(Msg::Projects);
    TopBarSlots {
        title: title.into(),
        tabs,
        right: iced::widget::Space::new().into(),
    }
}

/// 拉起/连接 `diggerd`(boot 与「重试」共用同一条路径)。
fn daemon_ensure_task() -> Task<Msg> {
    daemon_ensure_shell_task().map(Msg::AgentShell)
}

/// 同上,但给 `AgentShell::update` 的重试回调用(它只认壳消息)。
fn daemon_ensure_shell_task() -> Task<crate::adapters::agent::AgentShellMsg> {
    Task::perform(
        async {
            let daemon = crate::daemon::Daemon::new();
            daemon.ensure().await.map_err(|e| e.to_string())
        },
        crate::adapters::agent::AgentShellMsg::Ensured,
    )
}

/// Agent 卡片"当前工作内容"的反查源:Digger 一期没有待办派发记录。
struct NoTasks;

/// 静态实例:库的 `CardCtx` 借 `'a`,返回的元素不能引用函数栈上的临时值。
static NO_TASKS: NoTasks = NoTasks;

impl dozer_panel_agent::adapter::TaskTitleSource for NoTasks {
    fn task_title_for_session(&self, _session_id: &str) -> Option<String> {
        None
    }
}

impl ShellHost<Msg> for ProductShell {
    fn lift(msg: ShellMsg) -> Msg {
        Msg::Shell(msg)
    }

    fn meta(&self, panel: &PanelId) -> Option<PanelMeta> {
        shell::meta(panel)
    }

    fn rules(&self, panel: &PanelId) -> Option<PanelRules> {
        shell::rules(panel)
    }

    fn body<'a>(&'a self, panel: &PanelId, area: &BodyArea) -> El<'a, Msg> {
        match panel.as_str() {
            "digger.info" => self.info_view(),
            "digger.apps" => self.apps_view(),
            "digger.usage" => self.usage_view(area),
            "digger.todo" => self.todo_view(area),
            "digger.markdown" => self.markdown_view(),
            "digger.project" => self.project_view(area),
            "digger.files" => self.files_view(area),
            "digger.agent" => self.agent_view(area),
            "digger.conversations" => self.conversations_view(area),
            "digger.group_chat" => self.group_chat.view(self.projects.current_project(), area),
            "digger.browser" => self.browser.view(self.projects.current_project(), area),
            _ => column![].into(),
        }
    }

    fn top_bar<'a>(&'a self) -> TopBarSlots<'a, Msg> {
        top_bar_slots(&self.projects)
    }

    fn footbar<'a>(&'a self) -> El<'a, Msg> {
        // 底栏占位槽:S1 里没有内容,但高度与 Dozer 一致,窗口结构不变。
        iced::widget::Space::new().into()
    }

    fn on_top_bar_double_click(&self) -> Msg {
        Msg::ZoomToggle
    }
}

/// 载入用量账本并决定产品侧接线(复审 #9)。
///
/// 返回 `(用量服务, 可选共享账本, boot 提示)`:
/// - 成功 → 真账本 + [`LedgerUsageService`](digger_store::LedgerUsageService);
/// - 损坏/读失败 → **不退回临时空账本**,日志查询恒 `Unavailable`,账本为 `None`(调用方据此
///   不接线用量生产者——没有记录会悄悄写进一个下次启动就不再引用的文件)。原文件保持原样。
fn load_usage_ledger(
    path: std::path::PathBuf,
) -> (
    std::sync::Arc<dyn dozer_services::UsageService>,
    Option<std::sync::Arc<std::sync::Mutex<digger_store::UsageLedger>>>,
    String,
) {
    match digger_store::UsageLedger::load(path) {
        Ok(ledger) => {
            let ledger = std::sync::Arc::new(std::sync::Mutex::new(ledger));
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::LedgerUsageService::new(ledger.clone()));
            let note = "尚未打开 Project。可在「应用」页管理并运行应用。".to_string();
            (svc, Some(ledger), note)
        }
        Err(e) => {
            let note = format!("用量账本载入失败,用量查询暂不可用(不会丢数据:原文件未改动): {e}");
            let svc: std::sync::Arc<dyn dozer_services::UsageService> =
                std::sync::Arc::new(digger_store::UnavailableUsageService::new(note.clone()));
            (svc, None, note)
        }
    }
}

/// 某面板在骨架 `PaneStore` 里是否被收起(配对面板专有;无条目视为未收起)。
/// 单独成自由函数是为了让 `PanelDisplay` 适配器不必拿到 `ProductShell`——
/// 它只读**当前窗口**的布局,与 `ProductShell::pane_collapsed` 同源。
pub(crate) fn pane_collapsed_of(panel: &PanelId) -> bool {
    CURRENT_PANES.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|s| s.get(panel).is_some_and(|d| d.collapsed))
    })
}

// 给 `PanelDisplay` 适配器取分栏状态的绕法(同 `main.rs` 的 `CURRENT_WINDOW`):
// 库的展示 trait 没有参数能传 `PaneStore`,适配器又是 `&self` 的零状态对象。产品
// 每帧 `view` 前用 [`publish_panes`] 覆盖一份快照,适配器只读。约束是**只读、
// 每帧覆盖、不跨帧累积**:换项目 / 卸面板时要能把快照清掉,不残留上一帧的收起态。
thread_local! {
    /// 当前窗口骨架的 `PaneStore` 快照(每帧 `view` 前由 `layout()` 刷新)。
    static CURRENT_PANES: std::cell::RefCell<Option<bytehost_desktop::PaneStore>> =
        const { std::cell::RefCell::new(None) };
}

/// 预览原生页面的同步任务:建 / 挪 / 销毁在窗口线程里一次完成,有脚本时紧接着注入。
///
/// N1:`window::run(..)` 返回的 `Task` 必须**作为返回值交给 iced 运行时**。把它在 `async` 块里
/// `let _ =` 构造后丢掉(此前的写法)不会发出任何窗口动作,页面永远建不出来。注入用 `chain`
/// 排在同步之后,保证新建的句柄已经存在。
fn native_preview_task(
    window: window::Id,
    host: crate::adapters::preview_native::Host,
    desired: Vec<(
        dozer_panel_preview::PreviewWebviewSpec,
        bytehost_core::LogicalRect,
    )>,
    scripts: Vec<(usize, String)>,
) -> Task<Msg> {
    let injector = host.clone();
    let sync = window::run(window, move |w| host.sync(w, &desired)).discard();
    if scripts.is_empty() {
        return sync;
    }
    let inject = window::run(window, move |_| {
        for (webview_id, script) in scripts {
            injector.push_script(webview_id, script);
        }
    })
    .discard();
    sync.chain(inject)
}

/// G-C5:预览原生页面该服务于哪个项目。正在关闭时**任何**项目都不服务,
/// 调用方据此同步空清单销毁页面;`None` 项目同理。
fn preview_project(
    current: Option<&dozer_model::ProjectInfo>,
    closing: bool,
) -> Option<dozer_model::ProjectInfo> {
    if closing {
        return None;
    }
    current.cloned()
}

/// 把当前窗口的分栏状态交给只读的展示适配器(`PanelDisplay` 拿不到 `&ProductShell`)。
fn publish_panes(panes: &bytehost_desktop::PaneStore) {
    CURRENT_PANES.with(|p| *p.borrow_mut() = Some(panes.clone()));
}

/// 待办轮询只在面板真正可见且未在退出时订阅。
fn group_chat_poll_active(shell: &ProductShell) -> bool {
    !shell.closing
        && shell
            .group_chat
            .poll_wanted(shell.panel_visible(&shell::group_chat_id()))
}

fn todo_poll_active(shell: &ProductShell) -> bool {
    !shell.closing && shell.panel_visible(&shell::todo_id())
}

/// 每个已注册面板此刻的可见性(下标与 [`shell::all_panels`] 对齐)。
fn preview_panel_visible(flags: &[bool]) -> bool {
    [shell::files_id(), shell::project_id()]
        .iter()
        .filter_map(|id| shell::all_panels().iter().position(|p| p == id))
        .any(|i| flags.get(i).copied().unwrap_or(false))
}

fn visible_panels(shell: &ProductShell) -> Vec<bool> {
    shell::all_panels()
        .iter()
        .map(|p| shell.panel_visible(p))
        .collect()
}

impl Default for ProductShell {
    fn default() -> Self {
        Self::new()
    }
}

impl Product<Msg> for ProductShell {
    fn update(&mut self, msg: HostMessage<Msg>) -> Task<Msg> {
        let HostMessage::Product(m) = msg else {
            // Surface 事件**不在这里**再分发给生命周期:那由外层入口 [`update_host`] 单次调用
            // `route_lifecycle` 完成(复盘发现 6:在此调用会让 `route_lifecycle` → `update` 形成无限递归)。
            return Task::none();
        };
        match m {
            Msg::WindowReady(id, size) => {
                self.window = Some(id);
                self.window_minimized = false;
                let resized = self.on_shell(ShellMsg::Resized {
                    w: size.width,
                    h: size.height,
                });
                // macOS:全屏期间把红绿灯临时挂到内容视图(原生全屏不绘制标题栏,红绿灯会跟着移出窗口)。
                #[cfg(target_os = "macos")]
                let guard = window::run(id, |w| {
                    // 原生菜单(NSMenu)要挂靠窗口内容 view:窗口就绪时登记一次(主线程)。
                    byteui::native_menu::install_content_view(w);
                    // 顶栏拖窗保护:悬停页签/"＋"时 (`projects` 里的悬停状态) 放行按下给 iced,
                    // 悬停在空白顶栏时保留原生拖窗。
                    bytehost_chrome::platform_macos::install_topbar_drag_guard(w);
                    bytehost_chrome::platform_macos::install_fullscreen_guard(w)
                })
                .discard();
                #[cfg(not(target_os = "macos"))]
                let guard = Task::none();
                let query = self.query_window_state();
                Task::batch([resized, guard, query])
            }
            Msg::WindowStateRefresh(id) => {
                if self.window == Some(id) && !self.closing {
                    Task::batch([self.restyle_window(), self.query_window_state()])
                } else {
                    Task::none()
                }
            }
            Msg::WindowStateSample {
                id,
                request,
                minimized,
                size,
            } => {
                if self.closing || self.window != Some(id) || request != self.window_state_request {
                    return Task::none();
                }
                if let Some(minimized) = minimized {
                    self.window_minimized = minimized;
                }
                if let Some(size) = size.filter(|s| (s.width, s.height) != self.shell.window()) {
                    self.on_shell(ShellMsg::Resized {
                        w: size.width,
                        h: size.height,
                    })
                } else {
                    Task::none()
                }
            }
            Msg::Shell(m) => {
                let resized = matches!(m, ShellMsg::Resized { .. });
                let task = self.on_shell(m);
                if resized {
                    Task::batch([task, self.query_window_state()])
                } else {
                    task
                }
            }
            Msg::AgentShell(m) => {
                let was_ready = self.agent.shell.status == crate::adapters::agent::Status::Ready;
                let task = self
                    .agent
                    .shell
                    .update(m, daemon_ensure_shell_task)
                    .map(Msg::AgentShell);
                // 守护进程刚就绪:补推一次当前上下文(启动早期那次可能连不上),
                // 并启动命令轮询。
                if !was_ready && self.agent.shell.status == crate::adapters::agent::Status::Ready {
                    self.sync_preview_context(true);
                    self.start_preview_poller();
                }
                // Agent 面板若已可见,补一次懒启动(启动时它可能先于 `Ensured`
                // 变成可见,那一次什么都没启动)。
                let mut enter = Task::none();
                if self.agent.shell.status == crate::adapters::agent::Status::Ready
                    && self.panel_visible(&shell::agent_id())
                    && let Some(project) = self.projects.current_project().cloned()
                {
                    let grid = self.current_agent_grid().unwrap_or((80, 24));
                    enter = self.agent.ensure_started(&project, grid);
                }
                let conversations = if !was_ready
                    && self.agent.shell.status == crate::adapters::agent::Status::Ready
                    && self.panel_visible(&shell::conversations_id())
                {
                    self.activate_conversations_panel()
                } else {
                    Task::none()
                };
                let group_chat = if !was_ready
                    && self.agent.shell.status == crate::adapters::agent::Status::Ready
                    && self.panel_visible(&shell::group_chat_id())
                {
                    self.activate_group_chat_panel()
                } else {
                    Task::none()
                };
                Task::batch([task, enter, conversations, group_chat])
            }
            Msg::Agent(pid, m) => {
                if matches!(
                    m,
                    dozer_panel_agent::update::Message::PickerToggle
                        | dozer_panel_agent::update::Message::OverflowToggle { .. }
                ) {
                    self.conversations.close_agent_filters();
                }
                // macOS:Agent 页签溢出下拉走原生菜单(与预览页签一致)。库发的锚点是 `(0, 0)`。
                #[cfg(target_os = "macos")]
                if matches!(m, dozer_panel_agent::update::Message::OverflowToggle { .. }) {
                    return self.show_native_agent_overflow(pid);
                }
                let project = self.projects.project_by_id(pid);
                // 项目页签已关:迟到的 Agent 消息一律丢弃。
                let project = project.filter(|p| self.projects.open_ids().contains(&p.id));
                let grid = self.current_agent_grid().unwrap_or((80, 24));
                self.agent.update(pid, m, project.as_ref(), grid)
            }
            Msg::AgentRestored(pid, payload) => {
                let sessions = payload
                    .lock()
                    .ok()
                    .and_then(|mut s| s.take())
                    .unwrap_or_default();
                let project = self
                    .projects
                    .project_by_id(pid)
                    .filter(|p| self.projects.open_ids().contains(&p.id));
                let grid = self.current_agent_grid().unwrap_or((80, 24));
                self.agent
                    .on_restored(pid, project.as_ref(), sessions, grid)
            }
            Msg::AgentNoticeDismiss => {
                self.agent.dismiss_notice();
                Task::none()
            }
            Msg::AgentPickerClose => {
                self.agent.close_picker();
                Task::none()
            }
            Msg::AgentKey(event) => {
                if self.agent_keys_blocked() {
                    return Task::none();
                }
                let visible = self.panel_visible(&shell::agent_id());
                let project = self.projects.current_project();
                self.agent.on_key(&event, visible, project, self.closing)
            }
            Msg::AgentPointer(point) => {
                self.agent.on_pointer(point);
                Task::none()
            }
            Msg::AgentDrop(path) => {
                let visible = self.panel_visible(&shell::agent_id());
                let project = self.projects.current_project();
                self.agent.on_drop(&path, visible, project)
            }
            Msg::AgentPaste(pid, text) => {
                let project = self
                    .projects
                    .project_by_id(pid)
                    .filter(|p| self.projects.open_ids().contains(&p.id));
                self.agent.paste(pid, text, project.as_ref())
            }
            Msg::AgentPreedit(pid, preedit) => {
                let current = self.projects.current_project_id();
                self.agent.set_preedit(pid, preedit, current);
                Task::none()
            }
            Msg::AgentImeCommit(pid, text) => {
                let project = self
                    .projects
                    .project_by_id(pid)
                    .filter(|p| self.projects.open_ids().contains(&p.id));
                self.agent.commit_ime(pid, &text, project.as_ref())
            }
            Msg::AgentCursor(point) => {
                self.last_cursor = point;
                self.agent.set_cursor(point);
                Task::none()
            }
            Msg::AgentCursorLeft => {
                self.agent.clear_hover();
                self.agent.clear_drag();
                // 光标离开窗口 / 窗口失焦:没有 leave 事件可等,预览页签的悬停也一并清掉。
                self.preview_hover.clear();
                Task::none()
            }
            Msg::PreviewHover(panel, hover, entered) => {
                self.preview_hover
                    .on_hover((panel, hover), entered, std::time::Instant::now());
                Task::none()
            }
            Msg::AgentTabHover(pid, index) => {
                if self.agent.is_dragging() && self.projects.current_project_id() == Some(pid) {
                    let _ = self.agent.on_drag_move(pid, index);
                }
                Task::none()
            }
            Msg::AgentPointerRelease => {
                self.agent.on_pointer_released();
                Task::none()
            }
            Msg::AgentOverflowDismiss => {
                self.dismiss_agent_overflow();
                Task::none()
            }
            Msg::AgentEscape => {
                let pid = self.projects.current_project_id();
                let grid = self.current_agent_grid().unwrap_or((80, 24));
                let project = pid.and_then(|pid| self.projects.project_by_id(pid));
                if self.agent.confirm_is_open(pid) {
                    if let Some(pid) = pid {
                        let _ = self.agent.update(
                            pid,
                            dozer_panel_agent::update::Message::CloseCancel,
                            project.as_ref(),
                            grid,
                        );
                    }
                } else if self.agent.picker.is_some() {
                    self.agent.close_picker();
                } else {
                    self.dismiss_agent_overflow();
                }
                Task::none()
            }
            Msg::ZoomToggle => self
                .window
                .map(window::toggle_maximize)
                .unwrap_or_else(Task::none),
            Msg::Apps(m) => self.apps.update(m).map(Msg::Apps),
            Msg::Usage(m) => {
                // 库的 `update` 对这两条是 `unreachable!`(它们必须由宿主拦截):折叠转给骨架,悬停无事可做。
                match m {
                    dozer_panel_usage::Message::ToggleListCollapse => {
                        return self.on_shell(ShellMsg::TogglePane(shell::usage_id()));
                    }
                    dozer_panel_usage::Message::CollapseHover(_) => return Task::none(),
                    m => self.usage.update(m),
                }
                // Digger 一期无 Toast:把库的 outbox 提示落地到 stderr,不静默吞掉。
                for (level, text) in self.usage.take_notices() {
                    eprintln!("digger: usage 面板/{level}: {text}"); // cli-output
                }
                Task::none()
            }
            Msg::Tick => {
                // 悬停提示到期:消息本身会触发重绘(`tooltip_deadline` 只用于判断)。
                let _tooltip_due = self
                    .agent
                    .tooltip_deadline()
                    .is_some_and(|deadline| std::time::Instant::now() >= deadline);
                let apps = self.apps.update(crate::apps::Message::Tick).map(Msg::Apps);
                Task::batch([apps, self.query_window_state()])
            }
            Msg::NativeUsage(event) => {
                self.usage.native.update(event);
                Task::none()
            }
            Msg::Conversations(pid, message) => {
                let pid = message.result_project().unwrap_or(pid);
                let project = self
                    .projects
                    .open_ids()
                    .contains(&pid)
                    .then(|| self.projects.project_by_id(pid))
                    .flatten();
                let task = self.conversations.update(pid, message, project.as_ref());
                let requests = self.run_conversation_requests();
                Task::batch([task, requests])
            }
            Msg::ConversationsSearchEdit(pid, edit) => {
                let project = self
                    .projects
                    .open_ids()
                    .contains(&pid)
                    .then(|| self.projects.project_by_id(pid))
                    .flatten();
                self.conversations.search_edit(pid, edit, project.as_ref())
            }
            Msg::ConversationsImport(pid, event) => {
                if matches!(
                    event,
                    crate::adapters::conversations_import::ImportEvent::Start
                ) && self.projects.current_project_id() != Some(pid)
                {
                    return Task::none();
                }
                let project = self
                    .projects
                    .open_ids()
                    .contains(&pid)
                    .then(|| self.projects.project_by_id(pid))
                    .flatten();
                self.conversations.update_import(
                    pid,
                    event,
                    project.as_ref(),
                    self.agent.shell.status == crate::adapters::agent::Status::Ready,
                    self.closing,
                )
            }
            Msg::ReviewNative(event) => {
                let project = self.projects.current_project();
                self.conversations.update_native(event, project)
            }
            Msg::ConversationsNoticeDismiss => {
                self.conversations.dismiss_notice();
                Task::none()
            }
            Msg::GroupChat(message) => {
                if matches!(
                    message,
                    dozer_panel_group_chat::Message::NewGroupDialogToggle(_)
                ) {
                    self.agent.close_picker();
                }
                let open = self.projects.open_ids().to_vec();
                self.group_chat.update(message, |pid| open.contains(&pid))
            }
            Msg::GroupChatNoticeDismiss => {
                self.group_chat.dismiss_notice();
                Task::none()
            }
            Msg::GroupChatRetry => {
                self.group_chat.retry_page();
                Task::none()
            }
            Msg::GroupChatNative(event) => {
                let (task, show_todo) = self.group_chat.update_native(event);
                if show_todo {
                    return Task::batch([
                        task,
                        self.on_shell(ShellMsg::ShowPanel(shell::todo_id())),
                    ]);
                }
                task
            }
            Msg::GroupChatTick => {
                self.group_chat.tick(
                    std::time::Instant::now(),
                    self.panel_visible(&shell::group_chat_id()),
                );
                Task::none()
            }
            Msg::Todo(message) => {
                for effect in self.todo.update(message) {
                    match effect {
                        crate::adapters::todo::TodoEffect::ToggleListCollapse => {
                            return self.on_shell(ShellMsg::TogglePane(shell::todo_id()));
                        }
                        crate::adapters::todo::TodoEffect::AssignAgent { todo_id, agent } => {
                            self.todo.assign_agent(todo_id, agent);
                        }
                    }
                }
                for (level, text) in self.todo.take_notices() {
                    eprintln!("digger: todo 面板/{level}: {text}"); // cli-output
                }
                for text in self.todo.take_diagnostics() {
                    eprintln!("digger: todo 面板诊断: {text}"); // cli-output
                }
                Task::none()
            }
            Msg::NativeTodo(event) => {
                let Some(event) = self.todo.native.update(event) else {
                    return Task::none();
                };
                for effect in self.todo.handle_webview_event(event) {
                    match effect {
                        crate::adapters::todo::TodoEffect::ToggleListCollapse => {
                            return self.on_shell(ShellMsg::TogglePane(shell::todo_id()));
                        }
                        crate::adapters::todo::TodoEffect::AssignAgent { todo_id, agent } => {
                            self.todo.assign_agent(todo_id, agent);
                        }
                    }
                }
                self.sync_native()
            }
            Msg::TodoPoll => {
                self.todo.poll(std::time::Instant::now());
                Task::none()
            }
            Msg::Files(m) => self.on_files_message(m),
            Msg::LeftButton(down) => {
                self.left_down = down;
                if !down && self.files.tree_drag_active() {
                    let project_id = self.projects.current_project_id();
                    let (shift, command) = (self.modifiers.shift(), self.modifiers.command());
                    return self.files.release_tree_drag(shift, command, project_id);
                }
                Task::none()
            }
            Msg::Modifiers(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Msg::FilesMenuDismiss => {
                let project_id = self.projects.current_project_id();
                self.files.dismiss(project_id)
            }
            Msg::FilesFsChanged(change) => {
                if Some(change.project_id) == self.projects.current_project_id() {
                    self.files.fs_changed(&change)
                } else {
                    Task::none()
                }
            }
            Msg::Markdown(m) => self.markdown.update(m).map(Msg::Markdown),
            Msg::Projects(m) => {
                // 打开/切换项目:若「项目」面板此刻就在眼前,立刻把它切到新项目
                // (否则要等下一次切入才拉数据——用户刚打开的项目看着是空的)。
                let before = self.projects.current_project_id();
                let open_before: Vec<i64> = self.projects.open_ids().to_vec();
                let task = self.projects.update(m).map(Msg::Projects);
                // 消失的项目页签:abort 转发并 kill 该项目全部会话(会话语义同 Dozer)。
                self.close_agent_for(&open_before);
                for id in open_before
                    .iter()
                    .filter(|id| !self.projects.open_ids().contains(id))
                {
                    self.conversations.close_project(*id);
                    self.browser.close_project(*id);
                    self.group_chat.close_project(*id);
                }
                if before != self.projects.current_project_id() {
                    self.agent.reset_input_state();
                }
                self.on_current_project_changed(before, true);
                // 切到已开项目且 Agent 面板仍可见:恢复它的会话(同一项目只启动一次)。
                let mut enter = Task::none();
                if self.panel_visible(&shell::agent_id())
                    && let Some(project) = self.projects.current_project().cloned()
                {
                    let grid = self.current_agent_grid().unwrap_or((80, 24));
                    enter = self.agent.ensure_started(&project, grid);
                }
                let conversations = if self.panel_visible(&shell::conversations_id())
                    && self
                        .projects
                        .current_project_id()
                        .is_some_and(|id| self.conversations.state(id).is_none())
                {
                    self.activate_conversations_panel()
                } else {
                    Task::none()
                };
                Task::batch([task, enter, conversations])
            }
            Msg::ConversationSearchFocusFrame(pid) => {
                if self.conversation_search_project() != Some(pid)
                    || self.conversations.state(pid).is_none()
                {
                    return Task::none();
                }
                let token = self
                    .conversations
                    .state(pid)
                    .unwrap()
                    .requests
                    .begin(Some(pid), "host-search-focus");
                iced::widget::operation::is_focused(
                    dozer_panel_conversations::view::conversation_search_field_id(),
                )
                .map(move |focused| Msg::ConversationSearchFocused(pid, token.clone(), focused))
            }
            Msg::ConversationSearchFocused(pid, token, focused) => {
                let active = self.conversation_search_project();
                self.conversations.reset_inactive_focus(active);
                if active == Some(pid)
                    && self.projects.open_ids().contains(&pid)
                    && self
                        .conversations
                        .state(pid)
                        .is_some_and(|state| state.requests.accepts(&token))
                {
                    self.conversations.set_search_focused(pid, focused);
                }
                Task::none()
            }
            Msg::Browser(pid, message) => {
                let pid = message.result_project().unwrap_or(pid);
                let project = self
                    .projects
                    .open_ids()
                    .contains(&pid)
                    .then(|| self.projects.project_by_id(pid))
                    .flatten();
                let task = self.browser.update(pid, message, project.as_ref());
                Task::batch([task, self.sync_native()])
            }
            Msg::BrowserNative(event) => {
                let projects = &self.projects;
                let task = self.browser.update_native(event, |pid| {
                    projects
                        .open_ids()
                        .contains(&pid)
                        .then(|| projects.project_by_id(pid))
                        .flatten()
                });
                Task::batch([task, self.sync_native()])
            }
            Msg::BrowserNoticeDismiss => {
                self.browser.dismiss_notice();
                self.sync_native()
            }
            Msg::BrowserAddrEdit(pid, edit) => {
                let project = self
                    .projects
                    .open_ids()
                    .contains(&pid)
                    .then(|| self.projects.project_by_id(pid))
                    .flatten();
                let task = self.browser.addr_edit(pid, edit, project.as_ref());
                Task::batch([task, self.sync_native()])
            }
            Msg::BrowserFocusFrame(pid) => {
                if self.browser_project() != Some(pid) {
                    return Task::none();
                }
                // 同一个 16ms 探针顺带检查"导航迟迟没有开始"(池只在这里被问,面板不可见时不问)。
                self.browser.native.poll_stalled();
                self.browser.focus_frame(pid)
            }
            Msg::BrowserAddrFocused(pid, token, focused) => {
                let active = self.browser_project();
                self.browser.reset_inactive_focus(active);
                if active == Some(pid) && self.projects.open_ids().contains(&pid) {
                    self.browser.on_addr_focused(pid, token, focused)
                } else {
                    Task::none()
                }
            }
            Msg::ProjectFocusFrame(id) => {
                let Some(project) = self
                    .projects
                    .current_project()
                    .filter(|p| p.id == id)
                    .cloned()
                else {
                    return Task::none();
                };
                // 库在 `view` 之后把 `text_input` 的真焦点态写进全局锁,这里取走并记账;
                // 从真变假那一刻提交改名/描述(与 Dozer 的 `set_project_name_focused` 同义)。
                let now = dozer_panel_project::take_name_edit_focused();
                let was = self.project.name_edit_was_focused(project.id);
                self.project.set_name_edit_focused(project.id, now);
                if was && !now {
                    self.project.blur_editors(&project);
                }
                Task::none()
            }
            Msg::Project(m) => {
                // 改名结果自带项目 id(不是异步 token):不依赖"当前聚焦项目",也不要求它还是当前页签。
                if let dozer_panel_project::Message::NameRenamed(id, Ok(updated)) = &m {
                    let same = self.project.focused_project() == Some(*id);
                    self.projects.replace_registered(updated.clone());
                    if same && self.panel_visible(&shell::project_id()) {
                        self.activate_project_panel();
                    }
                    return Task::none();
                }
                let Some(project) = self.project.resolve_project(&m).and_then(|id| {
                    self.projects
                        .current_project()
                        .filter(|p| p.id == id)
                        .cloned()
                }) else {
                    return Task::none();
                };
                use dozer_panel_project::Message as PM;
                match m {
                    // 悬停动画:Digger 不挂 hover 动画表,按"未悬停"画,消息直接丢掉。
                    PM::ToolbarHover(..) => {}
                    // 文本编辑框的复制/粘贴菜单:一期不做(与其它面板一致)。
                    PM::TextInputMenuOpen(..) => {}
                    // D3:项目文档链接在右半边的预览里打开(预览失败仍可用系统应用打开)。
                    PM::OpenLink(path) => return self.preview_open(ViewerPanel::Project, &path),
                    // 右键菜单:一期只有"移除",不做浮层,直接落到库的 `LinkRemove`。
                    PM::LinkContextMenu { target, index } => self
                        .project
                        .update(Some(&project), PM::LinkRemove { target, index }),
                    PM::RepairProject => self.project.repair(&project),
                    PM::DeleteProjectConfirm => {
                        self.project_delete_confirm(&project);
                        return Task::none();
                    }
                    PM::Pick(target) => {
                        return Task::perform(
                            async move {
                                let picked = rfd::AsyncFileDialog::new()
                                    .set_title("选择项目文档")
                                    .set_directory(&project.path)
                                    .pick_file()
                                    .await
                                    .map(|h| h.path().to_path_buf());
                                Msg::ProjectLinkPicked(target, picked)
                            },
                            |m| m,
                        );
                    }
                    m => self.project.update(Some(&project), m),
                }
                self.flush_project_notices();
                Task::none()
            }
            Msg::ProjectLinkPicked(target, Some(path)) => {
                let Some(project) = self.projects.current_project().cloned() else {
                    return Task::none();
                };
                let kind = if path.is_dir() {
                    dozer_panel_project::links::LinkKind::Dir
                } else {
                    dozer_panel_project::links::LinkKind::File
                };
                self.project.update(
                    Some(&project),
                    dozer_panel_project::Message::LinkAdd { target, path, kind },
                );
                self.flush_project_notices();
                Task::none()
            }
            Msg::ProjectLinkPicked(_, None) => Task::none(),
            Msg::ProjectPointerDown => {
                let Some(project) = self.projects.current_project().cloned() else {
                    return Task::none();
                };
                if self.project.is_editing(project.id) {
                    self.project.blur_editors(&project);
                    self.flush_project_notices();
                }
                Task::none()
            }
            Msg::ProjectDeleteDone {
                project_id,
                deregistered,
                errors,
            } => {
                for e in &errors {
                    eprintln!("digger: 删除项目 {project_id}: {e}"); // cli-output
                }
                // 注册表已移除 → 页面上的页签也一并关掉(库只碰文件系统,不碰产品页签)。
                if deregistered {
                    let before = self.projects.current_project_id();
                    let close = self
                        .projects
                        .update(crate::projects::Message::Close(project_id));
                    let _ = self.agent.close_project(project_id);
                    self.conversations.close_project(project_id);
                    self.browser.close_project(project_id);
                    self.group_chat.close_project(project_id);
                    self.on_current_project_changed(before, before != Some(project_id));
                    // 被删项目的布局文件不留孤儿。
                    if let Some(path) = self.project_shell_path_for(project_id) {
                        let _ = std::fs::remove_file(path);
                    }
                    return Task::batch([
                        close.map(Msg::Projects),
                        self.projects.refresh().map(Msg::Projects),
                    ]);
                }
                Task::none()
            }
            Msg::PreviewEvent(event) => match event {
                crate::adapters::preview_native::Event::Ipc { message, .. } => {
                    let Some(event) =
                        crate::adapters::preview_native::host_event(&message).cloned()
                    else {
                        return Task::none();
                    };
                    let (Some(panel), Some(project_id)) = (event.panel(), event.project_id())
                    else {
                        return Task::none();
                    };
                    // 原生 IPC 回传也要带代次,否则切走又切回后的旧回传会落到
                    // 同号的重新编号页签上。
                    let epoch = self.preview.as_ref().map(|p| p.epoch()).unwrap_or_default();
                    // 绑定说的事件属于哪个项目就投给哪个项目;项目已切走则由
                    // `PreviewHub::dispatch` 丢掉,不会被投进当前项目的同号页签。
                    Task::done(Msg::Preview(
                        panel,
                        project_id,
                        epoch,
                        dozer_panel_preview::Message::Host(event),
                    ))
                }
                crate::adapters::preview_native::Event::BuildFailed { webview_id, error } => {
                    eprintln!("digger: 预览页面 {webview_id} 构建失败: {error}"); // cli-output
                    Task::none()
                }
            },
            Msg::PreviewCommandsFetched((pid, commands)) => {
                let Some(bridge) = self.preview_bridge.clone() else {
                    return Task::none();
                };
                let mut applied = false;
                for cmd in commands {
                    let outcome = if self.projects.current_project_id() == Some(pid) {
                        self.preview
                            .as_mut()
                            .map(|hub| hub.apply_preview_command(pid, &cmd))
                            .unwrap_or_else(|| dozer_model::PreviewCommandOutcome::NotFound {
                                request_id: cmd.request_id.clone(),
                                detail: "没有预览状态".into(),
                            })
                    } else {
                        // Review Focus 1:非当前项目立刻回 NotFound,不挂起。
                        dozer_model::PreviewCommandOutcome::NotFound {
                            request_id: cmd.request_id.clone(),
                            detail: "项目不在前台".into(),
                        }
                    };
                    bridge.report_outcome(outcome);
                    applied = true;
                }
                self.sync_preview_context_from_hub();
                if applied {
                    self.sync_native()
                } else {
                    Task::none()
                }
            }
            Msg::Preview(panel, project_id, epoch, message) => {
                // macOS:页签溢出下拉走原生菜单(浮在页面之上,弹出时不用藏文件内容),选中项回投
                // `SelectTab`。库的溢出按钮发的锚点是 `(0, 0)`,不拦截会出一个锚在左上角的 iced 下拉。
                #[cfg(target_os = "macos")]
                if matches!(
                    message,
                    dozer_panel_preview::Message::TabOverflowToggle { .. }
                ) {
                    return self.show_native_preview_overflow(panel, project_id, epoch);
                }
                // 按**发起时**的项目路由;项目已切走 / 删除时 `dispatch` 整条丢弃。
                // 代次对不上(切走又切回)也在这一层拒掉,免得同号页签被误伤。
                let tabs_before = self.preview_tab_count(panel, project_id);
                if let Some(preview) = self
                    .preview
                    .as_mut()
                    .filter(|p| p.accepts(project_id, epoch))
                {
                    preview.dispatch(panel, project_id, message);
                }
                // 悬停按页签下标寻址:页签增减后,旧下标可能被别的页签复用,残留的悬停会
                // 落到一个从未被悬停过的页签上(Dozer 同类问题修过两次)。
                if !hover_survives_tab_change(
                    tabs_before,
                    self.preview_tab_count(panel, project_id),
                ) {
                    self.preview_hover.clear();
                }
                self.sync_preview_context_from_hub();
                // 页签状态变了:原生页面的清单 / 编辑命令要在同一帧里跟上。
                self.sync_native()
            }
            Msg::Lifecycle(phase) => {
                let Some(mut driver) = self.lifecycle.take() else {
                    return Task::none();
                };
                let task = driver.advance(self, phase);
                self.lifecycle = Some(driver);
                task
            }
            Msg::ShutdownFinished(result) => match result {
                // 生命周期自检要在关闭之后读句柄表,检查完自己退出
                Ok(()) if self.lifecycle.is_some() => Task::none(),
                Ok(()) => iced::exit(),
                Err(error) => {
                    self.shutdown_error = Some(error);
                    Task::none()
                }
            },
            Msg::ForceExit => iced::exit(),
            Msg::CloseRequested | Msg::Exit => {
                if self.closing {
                    return Task::none();
                }
                self.closing = true;
                self.save_shell();
                self.apps.prepare_shutdown();
                // 关窗口只 detach:abort 转发,会话随 `diggerd` 存活。
                self.agent.prepare_shutdown();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::button;

        // 展示适配器(`PanelDisplay`)拿不到 `&ProductShell`,每帧把分栏状态与窗口尺寸交给它。
        publish_panes(self.shell.panes());
        crate::CURRENT_WINDOW.with(|w| w.set(Some(self.shell.window())));
        crate::CURRENT_CURSOR.set(self.last_cursor);
        let agent_blocked = self.agent_keys_blocked();
        crate::AGENT_KEYS_BLOCKED.store(agent_blocked, std::sync::atomic::Ordering::Relaxed);

        if self.closing {
            let mut body = column![text("正在关闭 Digger…")].spacing(16).padding(20);
            if let Some(error) = &self.shutdown_error {
                body = body
                    .push(text(error))
                    .push(button("强制退出").on_press(Msg::ForceExit));
            }
            return body.into();
        }
        // 名称编辑框的"失焦保存"钩子:库的 `text_input` 焦点态只能借 Operation 在渲染时读到,
        // Digger 没有逐帧原生循环,于是每帧派一个一次性消息,由 `update` 在两帧之间读回
        // (同 Dozer 的 `CaptureNameEditFocus`,只是搭在 "消息→重绘→消息" 的循环上)。
        let body: Element<'_, Msg> =
            bytehost_chrome::view(&self.shell, self, &metrics_from_theme());
        // 「修复项目」进度卡:进行中盖住整个窗口(不可关闭),全部完成后库才放行 `ScaffoldPopupClose`。
        // Dozer 用独立原生窗口承载,Digger 用同一窗口里的整窗覆盖层——卡片本体是库的同一份。
        let card: Option<Element<'_, Msg>> = self
            .project
            .focused_project()
            .and_then(|id| self.project.scaffold_card(id))
            .map(|card| {
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill)
                    .padding(24)
                    .into()
            });
        let body = match card {
            Some(card) => iced::widget::stack![body, card].into(),
            None => body,
        };
        // 「文件」面板的对话框(删除确认 / 移动重命名):居中卡片 + 半透明遮罩,
        // 压在右键菜单之下、正文之上。
        let body = if self.panel_visible(&shell::files_id()) {
            let dialog = self.files.delete_confirm().map(|spec| {
                let spec = byteui::feedback::dialog::ConfirmDialog {
                    icon: spec.icon,
                    title: spec.title,
                    description: spec.description,
                    cancel_label: spec.cancel_label,
                    cancel_msg: Msg::Files(spec.cancel_msg),
                    confirm_label: spec.confirm_label,
                    confirm_msg: Msg::Files(spec.confirm_msg),
                    close_msg: None,
                    confirm_color: spec.confirm_color,
                    content_spacing: spec.content_spacing,
                };
                centered_modal(
                    byteui::feedback::dialog::confirm(spec),
                    Msg::FilesMenuDismiss,
                )
            });
            let move_card = self
                .files
                .move_card()
                .map(|card| centered_modal(card, Msg::FilesMenuDismiss));
            match dialog.or(move_card) {
                Some(card) => iced::widget::stack![body, card].into(),
                None => body,
            }
        } else {
            body
        };
        // 「文件」面板的右键菜单:盖在最上层,点空白或 Esc 关闭。
        let body: Element<'_, Msg> = match self.files.menu() {
            Some(menu)
                if files_menu_is_rendered(
                    self.panel_visible(&shell::files_id()),
                    self.panel_visible(&shell::project_id()),
                ) =>
            {
                let window = (self.shell.window().0, self.shell.window().1);
                iced::widget::stack![
                    body,
                    crate::adapters::menu_overlay::view(
                        menu,
                        window,
                        Msg::Files,
                        Msg::FilesMenuDismiss
                    )
                ]
                .into()
            }
            _ => body,
        };
        // Agent 选择器 / 溢出下拉浮层:盖在正文之上,点空白 / Esc 关闭。
        let window = (self.shell.window().0, self.shell.window().1);
        let agent_overlay = if self.panel_visible(&shell::agent_id()) {
            crate::adapters::agent_display::picker_overlay(&self.agent, window)
                .or_else(|| crate::adapters::agent_display::overflow_overlay(&self.agent, window))
        } else {
            None
        };
        let body: Element<'_, Msg> = match agent_overlay {
            Some(overlay) => iced::widget::stack![body, overlay].into(),
            None => body,
        };
        match self
            .group_chat
            .group_create_overlay(self.projects.current_project())
        {
            Some(overlay) => iced::widget::stack![body, overlay].into(),
            None => body,
        }
    }

    fn on_activated(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }

    fn on_closed(&mut self, slot: bytehost_desktop::SurfaceSlot) {
        #[cfg(test)]
        self.surface_callbacks.push(slot);
        #[cfg(not(test))]
        let _ = slot;
    }
}

/// 产品壳的**唯一**宿主入口:Surface 事件在这里**单次**经 `route_lifecycle` 分发到产品生命周期回调
/// (复盘发现 6),产品消息则走 `Product::update`。`Product::update` 的 Surface 分支只消费、不再重投,
/// 二者分工使路由不成环。
pub fn update_host(shell: &mut ProductShell, msg: HostMessage<Msg>) -> Task<Msg> {
    #[cfg(test)]
    shell.conversations.native.scheduled_actions.clear();
    let task = match msg {
        HostMessage::Surface(event) => route_lifecycle(shell, event),
        HostMessage::Product(m) => Product::update(shell, HostMessage::Product(m)),
    };
    // One drain fans turn completion out to Project memories and Conversations.
    let mut turn_tasks = vec![];
    for (pid, event) in shell.agent.drain_host_events() {
        if matches!(
            event,
            dozer_panel_agent::adapter::AgentHostEvent::TurnEnded { .. }
        ) {
            shell.on_agent_turn_ended(pid);
            let project = shell
                .projects
                .project_by_id(pid)
                .filter(|_| shell.projects.open_ids().contains(&pid));
            let visible = shell.panel_visible(&shell::conversations_id())
                && shell.projects.current_project_id() == Some(pid)
                && shell.agent.shell.status == crate::adapters::agent::Status::Ready;
            turn_tasks.push(
                shell
                    .conversations
                    .on_turn_ended(pid, project.as_ref(), visible),
            );
        }
    }
    let task = Task::batch(std::iter::once(task).chain(turn_tasks));
    let search_project = shell.conversation_search_project();
    shell.conversations.reset_inactive_focus(search_project);
    // 退出分支自己将销毁与 shutdown 串行；其他更新后同步内容与几何。
    let native = shell.sync_native();
    shell
        .agent
        .on_visibility(shell.panel_visible(&shell::agent_id()));
    // Agent 终端块矩形与网格(每条消息一次廉价比较:同网格不发、0 不发)。
    let grid = match (
        shell.projects.current_project().cloned(),
        shell.current_agent_block(),
    ) {
        (Some(project), Some(block)) => {
            shell.agent.set_terminal_rect(iced_core::Rectangle::new(
                iced_core::Point::new(block.x, block.y),
                iced_core::Size::new(block.w, block.h),
            ));
            let grid = crate::adapters::agent_layout::grid(
                block,
                &crate::adapters::agent_display::terminal_pane_style(),
                crate::adapters::agent_layout::tab_bar_height(),
                &shell.term_style,
            );
            shell.agent.sync_grid(&project, grid)
        }
        _ => Task::none(),
    };
    if shell.closing && !shell.shutdown_started && shell.apps.native_idle() {
        shell.shutdown_started = true;
        let drain = shell.agent.drain_critical();
        let apps = shell.apps.shutdown();
        Task::batch([task, native, grid]).chain(Task::perform(
            async move {
                let (apps, ()) = tokio::join!(apps, drain);
                apps
            },
            Msg::ShutdownFinished,
        ))
    } else {
        Task::batch([task, native, grid])
    }
}

/// 名称编辑框焦点探针的订阅数据(项目 id 作 key:切项目就换一条订阅)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NameFocusProbe(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConversationSearchFocusProbe(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BrowserFocusProbe(i64);

/// 键盘事件是否该作为 Agent 输入投上去:`Captured` 的按键已被别处(输入框等)
/// 消费,绝不能进终端;Agent 自己的浮层(选择器 / 关页签确认)打开时也不投。
pub fn agent_key_message(
    status: iced::event::Status,
    event: &iced::keyboard::Event,
) -> Option<Msg> {
    (status == iced::event::Status::Ignored).then(|| Msg::AgentKey(event.clone()))
}

/// `listen_with` 的事件映射(必须是无捕获函数指针):光标与"浮层打开"状态
/// 由 `view` 每帧写进进程级原子,见 `lib.rs` 的 `CURRENT_CURSOR` /
/// `AGENT_KEYS_BLOCKED`。
fn agent_event(
    event: iced::Event,
    status: iced::event::Status,
    _id: iced::window::Id,
) -> Option<Msg> {
    match event {
        iced::Event::Keyboard(event) => {
            if crate::AGENT_KEYS_BLOCKED.load(std::sync::atomic::Ordering::Relaxed) {
                None
            } else {
                agent_key_message(status, &event)
            }
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => {
            Some(Msg::AgentPointer(crate::CURRENT_CURSOR.get()))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Msg::AgentCursor(position))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Msg::AgentCursorLeft),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => Some(Msg::AgentPointerRelease),
        iced::Event::Window(iced::window::Event::Unfocused) => Some(Msg::AgentCursorLeft),
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(Msg::AgentDrop(path)),
        _ => None,
    }
}

/// A native menu can unfocus the window without minimizing it. Events only ask
/// for an OS sample; the guarded response changes review visibility.
fn window_state_event(event: iced::Event, _: iced::event::Status, id: window::Id) -> Option<Msg> {
    matches!(
        event,
        iced::Event::Window(
            window::Event::Focused | window::Event::Unfocused | window::Event::Rescaled(_)
        )
    )
    .then_some(Msg::WindowStateRefresh(id))
}

/// 订阅:窗口关闭请求、骨架需要的鼠标 / 缩放事件,以及仅在有动画时才开的 ~60fps 动画拍。
pub fn subscription(shell: &ProductShell) -> Subscription<Msg> {
    let events = iced::event::listen_with(|evt, _status, _id| {
        if let iced::Event::Window(window::Event::CloseRequested) = evt {
            return Some(Msg::CloseRequested);
        }
        bytehost_chrome::map_event(&evt).map(Msg::Shell)
    });
    if shell.closing {
        return events;
    }
    // 终端输入:键盘只在未被控件消费时投;鼠标按下带最近一次光标位置;
    // 拖入文件走窗口事件。`listen_with` 只收函数指针,光标与浮层状态由
    // `view` 每帧写进进程级原子,供订阅工作线程读取。
    let agent_input = iced::event::listen_with(agent_event);
    // 左键按下 / 松开与修饰键:文件树的单击 / 拖拽 / 多选靠它们(库对这些消息的 `update` 不处理)。
    let pointer = iced::event::listen_with(|evt, _status, _id| match evt {
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
            Some(Msg::LeftButton(true))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Msg::LeftButton(false))
        }
        iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Msg::Modifiers(modifiers))
        }
        _ => None,
    });
    let mut subs = vec![
        events,
        pointer,
        agent_input,
        iced::event::listen_with(window_state_event),
        iced::time::every(std::time::Duration::from_secs(1)).map(|_| Msg::Tick),
    ];
    // 页签拖拽中:在页签行外松手也要结束拖拽(行内松手由页签行自己的 `on_release` 管),
    // 否则之后光标滑过页签行会接着重排并落盘。只在拖拽中订阅。
    if shell.projects.is_dragging() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left
                ))
            )
            .then_some(Msg::Projects(crate::projects::Message::DragEnd))
        }));
    }
    // 待办面板可见时每秒轮询一次(收起整个面板即停,省电)。
    if todo_poll_active(shell) {
        subs.push(iced::time::every(crate::adapters::todo::POLL_INTERVAL).map(|_| Msg::TodoPoll));
    }
    if group_chat_poll_active(shell) {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Msg::GroupChatTick),
        );
    }
    // 名称编辑框的真焦点态只有渲染时(Operation)读得到:用一个 16ms 的订阅在帧与帧之间
    // 把它取回来(同 Dozer 的 `CaptureNameEditFocus`,这里借订阅当逐帧钩子)。
    if let Some(project) = shell.projects.current_project().map(|p| p.id)
        && shell.panel_visible(&shell::project_id())
    {
        subs.push(iced::Subscription::run_with(
            NameFocusProbe(project),
            |probe| {
                let project = probe.0;
                iced::stream::channel(
                    1,
                    move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                        use iced::futures::SinkExt;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                            if out.send(Msg::ProjectFocusFrame(project)).await.is_err() {
                                break;
                            }
                        }
                    },
                )
            },
        ));
    }
    if let Some(pid) = shell.conversation_search_project() {
        subs.push(Subscription::run_with(
            ConversationSearchFocusProbe(pid),
            |probe| {
                let pid = probe.0;
                iced::stream::channel(
                    1,
                    move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                        use iced::futures::SinkExt;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                            if out
                                .send(Msg::ConversationSearchFocusFrame(pid))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                )
            },
        ));
    }
    if let Some(pid) = shell.browser_project() {
        subs.push(Subscription::run_with(BrowserFocusProbe(pid), |probe| {
            let pid = probe.0;
            iced::stream::channel(
                1,
                move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                    use iced::futures::SinkExt;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                        if out.send(Msg::BrowserFocusFrame(pid)).await.is_err() {
                            break;
                        }
                    }
                },
            )
        }));
    }
    #[cfg(any(not(target_os = "macos"), test))]
    if let Some(pid) = shell.conversation_search_project()
        && shell
            .conversations
            .state(pid)
            .is_some_and(|state| state.agent_picker_open())
    {
        subs.push(
            iced::event::listen_with(|event, _, _| {
                matches!(
                    event,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(())
            })
            .with(pid)
            .map(|(pid, ())| {
                Msg::Conversations(pid, dozer_panel_conversations::Message::AgentFilterClose)
            }),
        );
    }
    // 新建群弹窗:Esc 取消(背景点击不关闭,保留草稿)。
    if let Some(pid) = shell.projects.current_project().map(|p| p.id)
        && shell
            .group_chat
            .state(pid)
            .is_some_and(|state| state.new_group_dialog_open())
    {
        subs.push(
            iced::event::listen_with(|evt, _status, _id| {
                matches!(
                    evt,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(())
            })
            .with(pid)
            .map(|(pid, ())| {
                Msg::GroupChat(dozer_panel_group_chat::Message::NewGroupDialogCancel(pid))
            }),
        );
    }

    // 右键菜单打开时订阅 Esc:关掉浮层(点空白走浮层自己的透明按下区)。
    if shell.files.has_modal() || shell.files.menu().is_some() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
            .then_some(Msg::FilesMenuDismiss)
        }));
    }
    // Agent 覆盖层(选择器 / 溢出下拉)或关页签确认打开时 Esc 关最上层。
    let agent_confirm = shell
        .projects
        .current_project()
        .and_then(|p| shell.agent.states.get(&p.id))
        .is_some_and(|state| state.pending_close_tab.is_some());
    if shell.agent.overlay_is_open() || agent_confirm {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
            .then_some(Msg::AgentEscape)
        }));
    }
    if shell.shell.animating() {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| Msg::Shell(ShellMsg::Tick)),
        );
    }
    Subscription::batch(subs)
}

/// 面板内弹窗:整窗半透明遮罩 + 居中卡片。点遮罩关闭。
fn centered_modal<'a>(card: Element<'a, Msg>, on_dismiss: Msg) -> Element<'a, Msg> {
    let scrim: Element<'a, Msg> = iced_widget::mouse_area(
        container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_t: &iced_widget::Theme| container::Style {
                background: Some(
                    iced_widget::core::Color {
                        a: 0.35,
                        ..iced_widget::core::Color::BLACK
                    }
                    .into(),
                ),
                ..container::Style::default()
            }),
    )
    .on_press(on_dismiss)
    .into();
    let centered: Element<'a, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(24)
        .into();
    iced_widget::stack![scrim, centered].into()
}

/// 入口用:初始窗口尺寸 = 上次退出时保存的尺寸(没有就是默认尺寸)。
pub fn default_window_size_for_boot() -> Size {
    let (w, h) = shell::load(&paths::shell_path()).model().window();
    Size::new(w, h)
}

/// 应用初始化:确保产品数据根、设置 ByteBoy2077 主题,异步取窗口尺寸。
pub fn boot() -> (ProductShell, Task<Msg>) {
    if let Err(e) = paths::ensure_data_root() {
        // 数据根不可用不 panic:产品壳仍可打开,只是无法持久化布局。
        // (转为可见提示由产品壳承载。)
        eprintln!("digger: 数据根不可用: {e}"); // cli-output
    }
    color::set_theme(color::ColorTokens::byteboy2077());
    font::set_theme(font::FontTokens::byteboy2077());

    let mut shell = ProductShell::new();
    let size = default_window_size_for_boot();
    // 用量面板要 iced 运行时句柄在后台取数:`boot` 在 iced 运行时内执行,这里补挂。
    shell.usage.attach_handle(tokio::runtime::Handle::current());
    shell
        .project
        .attach_handle(tokio::runtime::Handle::current());
    shell.files.attach_handle(tokio::runtime::Handle::current());
    shell.agent.attach_handle(tokio::runtime::Handle::current());
    shell.todo.attach_handle(tokio::runtime::Handle::current());
    shell
        .conversations
        .attach_handle(tokio::runtime::Handle::current());
    shell
        .browser
        .attach_handle(tokio::runtime::Handle::current());
    shell
        .group_chat
        .attach_handle(tokio::runtime::Handle::current());
    // 预览状态层(S5b-2 P4):原生宿主只在真窗口里建,故不放在 `ProductShell::new()`
    // —— 纯逻辑测试构造产品壳时不该碰 webview。恢复页签放在这里(运行时已就绪)。
    // 缺资源时宿主只会整块白屏:启动即点名报出,与 selftest 同一份名单。
    let missing = crate::preview_assets::missing_in(&crate::preview_assets::assets_dir());
    if !missing.is_empty() {
        eprintln!(
            "digger: 预览资源根 {} 缺 {missing:?}(预览面板会白屏)",
            crate::preview_assets::assets_dir().display()
        ); // cli-output
    }
    shell.preview = Some(crate::adapters::preview::PreviewHub::new(
        crate::adapters::preview_native::Host::new(crate::preview_assets::roots()),
    ));
    if let Some(preview) = shell.preview.as_mut() {
        preview.attach_handle(tokio::runtime::Handle::current());
        preview.set_current_project(shell.projects.current_project());
        preview.finish_startup();
    }
    // S6e-3a:同一个 diggerd socket 的预览桥;启动先推一次当前上下文。
    shell.preview_bridge = Some(crate::adapters::preview_bridge::PreviewBridge::new(
        dozer_client::Client::new(digger_paths::product().socket_path()),
        tokio::runtime::Handle::current(),
    ));
    shell.sync_preview_context(true);
    let preview_cmds_task = match shell.take_preview_cmds_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|(pid, cmds)| (Msg::PreviewCommandsFetched((pid, cmds)), rx))
        })),
        None => Task::none(),
    };
    // 应用面板后台结果回流:一次性流出接收端,持续到进程结束(与窗口尺寸任务并行)。
    use iced::futures::StreamExt;
    let reply_task = match crate::apps::take_reply_receiver(&shell.apps.reply_receiver()) {
        Some(rx) => iced::Task::stream(crate::apps::reply_stream(rx).map(Msg::Apps)),
        None => Task::none(),
    };
    // 用量面板后台取数结果回流:同型,接成 iced 流回投 `Msg::Usage`。
    let usage_task = match shell.usage.take_receiver() {
        Some(rx) => iced::Task::stream(crate::adapters::usage::receiver_stream(rx).map(Msg::Usage)),
        None => Task::none(),
    };
    let size_task = bytehost_desktop::panel::oldest_window().then(move |id| match id {
        Some(id) => bytehost_desktop::panel::window_size(id)
            .map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    let native_usage = match shell.usage.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::NativeUsage(event), rx))
        })),
        None => Task::none(),
    };
    let todo_task = match shell.todo.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::Todo(event), rx))
        })),
        None => Task::none(),
    };
    let native_todo = match shell.todo.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::NativeTodo(event), rx))
        })),
        None => Task::none(),
    };
    let markdown_task = match shell.markdown.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::Markdown(event), rx))
        })),
        None => Task::none(),
    };
    let lifecycle_task = if shell.lifecycle.is_some() {
        crate::lifecycle_selftest::Driver::first()
    } else {
        Task::none()
    };
    // 「项目」面板的后台结果(记忆列表 / git 快照 / 磁盘占用)回流成 iced 流。
    let project_task = match shell.project.take_receiver() {
        Some(rx) => Task::stream(crate::adapters::project::receiver_stream(rx).map(Msg::Project)),
        None => Task::none(),
    };
    // 「文件」面板的后台结果(git 文件状态)回流成 iced 流。
    let files_task = match shell.files.take_receiver() {
        Some(rx) => Task::stream(crate::adapters::files::receiver_stream(rx).map(Msg::Files)),
        None => Task::none(),
    };
    // 预览面板的后台结果 / host 回流(库的 `run_effect` 回投 `Message`)。
    let preview_task = match shell.preview.as_mut().and_then(|p| p.take_receiver()) {
        Some(rx) => Task::stream(crate::adapters::preview::receiver_stream(rx)),
        None => Task::none(),
    };
    // 预览原生页面回传(host IPC / 建页失败):每条转成产品消息,再按宿主分类。
    let preview_native_task = match shell.preview.as_ref().map(|p| p.host().take_receiver()) {
        Some(Some(rx)) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::PreviewEvent(event), rx))
        })),
        _ => Task::none(),
    };
    // 文件系统监听的事件回流(当前项目的增删改 → 树与 git 着色跟进)。
    let fs_task = match shell.files.take_fs_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|c| (Msg::FilesFsChanged(c), rx))
        })),
        None => Task::none(),
    };
    // 「删除项目」的后台结果回流(库的 `spawn_delete_project` 在结束回调里投递)。
    let delete_task = match shell.take_delete_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            let next = rx.recv().await?;
            Some((next, rx))
        })),
        None => Task::none(),
    };
    // S6a:守护进程拉起。异步调 ensure,结果转成 Msg::AgentShell::Ensured。
    let daemon_task = daemon_ensure_task();
    // Agent 面板的后台回投(输出/状态/Attached/失败):一条持续到进程结束的 iced 流。
    let agent_task = match shell.agent.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|(pid, m)| (Msg::Agent(pid, m), rx))
        })),
        None => Task::none(),
    };
    let group_chat_task = match shell.group_chat.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|m| (Msg::GroupChat(m), rx))
        })),
        None => Task::none(),
    };
    let conversations_task = match shell.conversations.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|(pid, m)| (Msg::Conversations(pid, m), rx))
        })),
        None => Task::none(),
    };
    let browser_native_task = match shell.browser.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::BrowserNative(event), rx))
        })),
        None => Task::none(),
    };
    let browser_task = match shell.browser.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|(pid, m)| (Msg::Browser(pid, m), rx))
        })),
        None => Task::none(),
    };
    let group_chat_native_task = match shell.group_chat.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|event| (Msg::GroupChatNative(event), rx))
        })),
        None => Task::none(),
    };
    let review_native_task = match shell.conversations.native.take_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Msg::ReviewNative(event), rx))
        })),
        None => Task::none(),
    };
    let conversations_import_task = match shell.conversations.take_import_receiver() {
        Some(rx) => Task::stream(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|(pid, event)| (Msg::ConversationsImport(pid, event), rx))
        })),
        None => Task::none(),
    };
    // 启动时就可见的面板(例如默认右栏选中的用量)也要触发各自的"切入时动作"。
    let mut initial = Vec::new();
    for panel in shell::all_panels() {
        if shell.panel_visible(&panel) {
            initial.push(shell.on_visible(&panel));
        }
    }
    shell.visible_before = visible_panels(&shell);
    let projects_task = shell.projects.refresh().map(Msg::Projects);
    (
        shell,
        Task::batch([
            projects_task,
            size_task,
            reply_task,
            usage_task,
            native_usage,
            todo_task,
            native_todo,
            markdown_task,
            project_task,
            files_task,
            preview_task,
            preview_native_task,
            fs_task,
            delete_task,
            lifecycle_task,
            daemon_task,
            agent_task,
            group_chat_task,
            conversations_task,
            browser_task,
            browser_native_task,
            conversations_import_task,
            review_native_task,
            group_chat_native_task,
            preview_cmds_task,
            Task::batch(initial),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;
    use bytehost_desktop::SurfaceEvent;

    use dozer_services::{MemoryService, ProjectService};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default, Clone)]
    struct MemoryCalls(Arc<AtomicUsize>);

    struct FakeMemory(MemoryCalls);

    fn fake_unavailable() -> dozer_services::ServiceError {
        dozer_services::ServiceError::new(dozer_services::ServiceErrorKind::Unavailable, "test")
    }

    impl MemoryService for FakeMemory {
        fn list_memories(
            &self,
            _: i64,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<Vec<dozer_model::MemoryInfo>, dozer_services::ServiceError>,
        > {
            self.0.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Vec::new()) })
        }
        fn get_memory(
            &self,
            _: i64,
            _: i64,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<dozer_model::MemoryDetail, dozer_services::ServiceError>,
        > {
            Box::pin(async { Err(fake_unavailable()) })
        }
        fn write_memory(
            &self,
            _: i64,
            _: String,
            _: String,
            _: String,
            _: String,
            _: String,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<dozer_model::MemoryDetail, dozer_services::ServiceError>,
        > {
            Box::pin(async { Err(fake_unavailable()) })
        }
        fn delete_memory(
            &self,
            _: i64,
            _: i64,
            _: String,
        ) -> dozer_services::BoxFuture<'_, Result<(), dozer_services::ServiceError>> {
            Box::pin(async { Err(fake_unavailable()) })
        }
    }

    struct FakeProjects;

    impl ProjectService for FakeProjects {
        fn rename_project(
            &self,
            _: i64,
            _: String,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<Option<dozer_model::ProjectInfo>, dozer_services::ServiceError>,
        > {
            Box::pin(async { Ok(None) })
        }
        fn remove_project(
            &self,
            _: i64,
        ) -> dozer_services::BoxFuture<'_, Result<(), dozer_services::ServiceError>> {
            Box::pin(async { Ok(()) })
        }
        fn open_project(
            &self,
            _: String,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<Option<dozer_model::ProjectInfo>, dozer_services::ServiceError>,
        > {
            Box::pin(async { Ok(None) })
        }
        fn list_projects(
            &self,
        ) -> dozer_services::BoxFuture<
            '_,
            Result<Vec<dozer_model::ProjectInfo>, dozer_services::ServiceError>,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    fn info(id: i64) -> dozer_model::ProjectInfo {
        dozer_model::ProjectInfo {
            id,
            path: format!("/tmp/digger-s6e2-{id}"),
            name: format!("p{id}"),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        }
    }

    fn fake_host(calls: MemoryCalls) -> digger_store::ProjectHost {
        digger_store::ProjectHost {
            projects: Arc::new(FakeProjects),
            memory: Arc::new(FakeMemory(calls)),
        }
    }

    /// 壳 + 假 Project 面板(已切入并建好项目状态),返回内存调用计数。
    fn shell_with_fake_project(tag: &str, project_id: i64) -> (ProductShell, MemoryCalls) {
        let calls = MemoryCalls::default();
        let mut shell = fresh(tag);
        let project = info(project_id);
        shell.projects.seed_for_test(project.clone());
        press(&mut shell, &shell::project_id());
        shell.project =
            crate::adapters::project::ProjectPanel::new(Some(&fake_host(calls.clone())));
        shell
            .project
            .attach_handle(tokio::runtime::Handle::current());
        shell.project.seed_state(&project);
        (shell, calls)
    }

    #[tokio::test]
    async fn turn_ended_refreshes_memories_only_when_the_project_panel_is_visible_and_the_project_is_current()
     {
        // 四个条件全满足 → 1 次。
        let (mut shell, calls) = shell_with_fake_project("t4-all", 7);
        shell.on_agent_turn_ended(7);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 1);

        // Project 面板不可见 → 不刷新。
        press(&mut shell, &shell::markdown_id());
        shell.on_agent_turn_ended(7);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 1);

        // 不是当前项目 → 不刷新。这里只改骨架选中态,不触发 on_visible 的切入刷新。
        shell::update(
            &mut shell.shell,
            ShellMsg::PanelPress {
                panel: shell::project_id(),
            },
        );
        shell.projects.seed_for_test(info(8));
        shell.on_agent_turn_ended(7);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn turn_ended_for_a_background_project_does_not_refresh_the_current_one() {
        let (mut shell, calls) = shell_with_fake_project("t4-bg", 7);
        shell.projects.seed_for_test(info(8));
        assert_eq!(shell.projects.current_project_id(), Some(8));
        shell.on_agent_turn_ended(7);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn turn_ended_without_a_project_panel_state_does_nothing() {
        let (mut shell, calls) = shell_with_fake_project("t4-nostate", 7);
        // 当前项目 9 已开页签但从未 seed 过面板状态。
        shell.projects.seed_for_test(info(9));
        assert_eq!(shell.projects.current_project_id(), Some(9));
        assert!(!shell.project.has_state(9));
        shell.on_agent_turn_ended(9);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn state_changed_events_do_not_refresh() {
        let (mut shell, calls) = shell_with_fake_project("t4-state", 7);
        shell.agent.push_host_event_for_test(
            7,
            dozer_panel_agent::adapter::AgentHostEvent::StateChanged {
                tab_id: 0,
                agent: dozer_model::AgentKind::Claude,
                state: dozer_model::AgentState::Running,
                transcript_path: None,
            },
        );
        let task = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        drop(task);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(calls.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repeated_turn_ended_events_do_not_stack_refreshes() {
        let (mut shell, calls) = shell_with_fake_project("t4-repeat", 7);
        for _ in 0..5 {
            shell.on_agent_turn_ended(7);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let n = calls.0.load(Ordering::SeqCst);
        assert!((1..=5).contains(&n), "刷新请求不应无限堆叠: {n}");
    }

    /// N1b:预览展示上下文不能每帧泄漏。
    ///
    /// 以前 `preview_pane` 每帧 `Box::leak` 一份 `PreviewDisplay`(60fps 下
    /// 两份/帧,无界增长)。现在由 `ProductShell::preview_displays` 持有、
    /// 原地 `Cell::set`;`preview_pane` 里不该再出现任何 leak 写法。
    #[test]
    fn preview_display_is_not_leaked_every_frame() {
        let source = include_str!("product.rs");
        let start = source.find("fn preview_pane").expect("找不到 preview_pane");
        let rest = &source[start..];
        let end = rest
            .find("\n    fn markdown_view")
            .expect("找不到下一个函数");
        let body = &rest[..end];
        assert!(
            !body.contains("leak"),
            "`preview_pane` 又在按帧泄漏展示上下文了"
        );
    }

    /// N1:预览的建 / 挪 / 销毁任务必须真的把窗口动作交给 iced 运行时。
    /// 用运行时同款的 `task::into_stream` 取出 Task 的动作流:`window::run(..)` 被构造后丢掉
    /// (此前 `async` 块里 `let _ = window::run(..)` 的写法)时流里一个窗口动作都没有。
    fn first_window_action_count(task: Task<Msg>) -> usize {
        use iced::futures::StreamExt;
        let Some(mut stream) = iced_runtime::task::into_stream(task) else {
            return 0;
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut count = 0;
            while let Ok(Some(action)) =
                tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await
            {
                if matches!(action, iced_runtime::Action::Window(_)) {
                    count += 1;
                }
            }
            count
        })
    }

    #[test]
    fn the_native_sync_task_hands_a_window_action_to_the_runtime() {
        let host = crate::adapters::preview_native::Host::new(std::env::temp_dir());
        let id = window::Id::unique();
        // 空清单(无项目 / 关闭):销毁旧页面也必须真的发出动作。
        let task = native_preview_task(id, host.clone(), Vec::new(), Vec::new());
        assert!(
            first_window_action_count(task) >= 1,
            "同步任务没有产生任何窗口动作:预览页不会被创建或销毁"
        );
        // 有脚本要注入:同步在前,注入用 chain 排在它后面;至少同步这一步要发出。
        let task = native_preview_task(
            id,
            host,
            Vec::new(),
            vec![(1, "window.__x = 1;".to_string())],
        );
        assert!(first_window_action_count(task) >= 1);
    }

    /// 反面对照:`let _ = window::run(..)`(构造后丢弃)正好是 0 个窗口动作,上面的断言会因此失败。
    #[test]
    fn a_dropped_window_run_emits_nothing() {
        let id = window::Id::unique();
        let task: Task<Msg> = Task::stream(iced::futures::stream::once(async move {
            let _ = window::run(id, |_| ());
            Msg::Tick
        }));
        assert_eq!(first_window_action_count(task), 0);
    }

    /// 浮层开着时原生 webview 必须真隐藏(Dozer 同款),关掉后恢复;页签与页面都留着,不销毁。
    #[test]
    fn an_open_overlay_hides_every_preview_webview_and_closing_it_restores_them() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let spec = |id: usize, park: bool| dozer_panel_preview::PreviewWebviewSpec {
            id,
            url: "dozer://flyfish/host.html".into(),
            visible: !park,
            editor_binding: None,
            reserve: None,
            loading_generation: None,
            park_offscreen: park,
        };
        let mut desired = vec![(spec(1, false), rect), (spec(2, true), rect)];
        hide_for_overlay(&mut desired, false);
        assert!(
            desired[0].0.visible && desired[1].0.park_offscreen,
            "无浮层不动"
        );
        hide_for_overlay(&mut desired, true);
        assert!(
            desired.iter().all(|(s, _)| !s.visible && !s.park_offscreen),
            "浮层开着:全部真隐藏,连停放中的也不能留在屏幕上"
        );
        // 清单项还在(没有销毁):`plan` 对已存在的句柄只会发 Update 不会发 Destroy。
        let live = std::collections::BTreeMap::from([(
            1usize,
            crate::adapters::preview_native::LiveView {
                url: "dozer://flyfish/host.html".into(),
                rect,
                visible: true,
            },
        )]);
        let ops = crate::adapters::preview_native::plan(&live, &desired[..1]);
        assert!(matches!(
            ops.as_slice(),
            [crate::adapters::preview_native::Op::Update {
                id: 1,
                visible: false,
                ..
            }]
        ));
    }

    fn test_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "digger-product-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 测试用:项目布局文件放在测试目录下(生产是 `<config>/shell-projects/`)。
    fn project_path_in(dir: &std::path::Path, id: i64) -> std::path::PathBuf {
        dir.join("shell-projects").join(format!("{id}.json"))
    }

    /// 带独立落盘路径的壳:互不干扰,也不碰真实配置目录。
    fn fresh(tag: &str) -> ProductShell {
        ProductShell::with_shell_path(test_temp_dir(tag).join("shell.json"))
    }

    fn press(shell: &mut ProductShell, panel: &PanelId) {
        let _ = Product::update(
            shell,
            HostMessage::Product(Msg::Shell(ShellMsg::PanelPress {
                panel: panel.clone(),
            })),
        );
    }

    /// Review Focus 2:预览 webview 矩形是配对的第二块,下移页签条 + 内容区内边距,左右内缩同值。
    #[test]
    fn preview_rect_is_the_second_block_below_the_chrome() {
        let slot = Rect::new(600.0, 120.0, 400.0, 500.0);
        let inset = crate::adapters::preview_display::PreviewDisplay::content_padding();
        let chrome = byteui::theme::geometry::preview_chrome_top_px();
        let rect = preview_webview_rect(slot);
        assert_eq!(rect.x, 600.0 + inset);
        assert_eq!(rect.y, 120.0 + chrome + inset);
        assert_eq!(rect.w, 400.0 - inset * 2.0);
        // G-I2:`slot` 就是库 `view::pane` 的内容块(它自己已经 `padding(8)`)。
        // Dozer 的 `webview_geometry` 非放大态是 `y = 顶 + 38`、`h` 一直到
        // `content` 底边(`content` 已扣 padding 与 footbar),**底部不再缩**。
        // 旧实现底部又减了一个 `inset`,比 Dozer 矮 8px。
        assert_eq!(rect.h, 500.0 - chrome - inset);
        assert_eq!(
            rect.y + rect.h,
            slot.y + slot.h,
            "webview 底边必须贴齐内容块底边(G-I2)"
        );
        let style = {
            use dozer_panel_preview::display::PanelDisplay;
            preview_display(0.0, 1, 0).pane_style()
        };
        assert_eq!(
            style.padding,
            iced_widget::core::Padding::from(inset),
            "库拿到的 padding 就是这里用的 inset"
        );
    }

    /// G-M:`CURRENT_PANES` 是给 `PanelDisplay` 的只读快照,必须**每帧覆盖、不跨帧
    /// 累积**:上报一份「Files 收起」的布局后,再上报一份「不收起」的布局,
    /// 查询必须立刻反映新的那一份(而不是还记着旧的)。
    #[test]
    fn pane_snapshot_does_not_outlive_the_frame() {
        let files = crate::shell::files_id();
        let mut collapsed = bytehost_desktop::PaneStore::default();
        collapsed.ensure(&files, bytehost_desktop::PaneDims::new(0.5, true));
        collapsed.set_collapsed(&files, true, 0.5);
        let store = bytehost_desktop::PaneStore::default();
        publish_panes(&collapsed);
        assert!(pane_collapsed_of(&files), "本帧的收起态要能被读到");

        publish_panes(&store);
        assert!(
            !pane_collapsed_of(&files),
            "下一帧换回不收起:快照必须被覆盖,不能残留上一帧"
        );
    }

    /// G-M:`boot()` 里预览宿主的初始化顺序——必须先 `attach_handle`(进 iced
    /// 运行时、有后台)再 `set_current_project` / `finish_startup`,否则启动时
    /// 排出的库侧效果(`SaveFile` / `BuildLineIndex` / 恢复页签)会被静默丢弃。
    #[test]
    fn boot_attaches_the_runtime_handle_before_touching_the_preview_hub() {
        let src = include_str!("product.rs");
        // 只看 `boot()` 这一段(`Msg::Projects` 的处理里也有 set_current_project)。
        let boot = src
            .split_once("pub fn boot() -> (ProductShell, Task<Msg>) {")
            .expect("boot() 还在")
            .1;
        let boot = boot.split_once("\n}\n").expect("boot() 有结尾").0;
        let attach = boot
            .find("preview.attach_handle(")
            .expect("boot() 要给预览挂运行时句柄");
        let set_project = boot
            .find("preview.set_current_project(")
            .expect("boot() 要设当前项目");
        let finish = boot
            .find("preview.finish_startup(")
            .expect("boot() 要结束安全启动");
        assert!(
            attach < set_project && attach < finish,
            "顺序必须是 attach_handle → set_current_project → finish_startup"
        );
    }

    /// G-C5:没有当前项目 / 正在关闭时,预览必须同步空清单(销毁全部页面);
    /// 只有"有项目且没在关闭"才服务页面。
    #[test]
    fn closing_the_last_project_tab_destroys_every_preview_webview() {
        use crate::adapters::preview_native::{LiveView, Op, plan};
        use bytehost_core::LogicalRect;
        use std::collections::BTreeMap;

        let project = dozer_model::ProjectInfo {
            id: 1,
            path: "/tmp/p1".into(),
            name: "p1".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        assert!(preview_project(Some(&project), false).is_some());
        assert!(
            preview_project(None, false).is_none(),
            "关掉最后一个项目后没有当前项目"
        );
        assert!(
            preview_project(Some(&project), true).is_none(),
            "退出过程中不许再服务页面"
        );

        // 空清单在宿主侧就是"销毁全部":证明 G-C5 的同步目标形态成立。
        let mut live = BTreeMap::new();
        for id in [0usize, 1, 1001] {
            live.insert(
                id,
                LiveView {
                    url: format!("dozer://editor/index.html?tab={id}"),
                    rect: LogicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    visible: true,
                },
            );
        }
        let ops = plan(&live, &[]);
        assert_eq!(ops.len(), 3, "三个页面都要销毁: {ops:?}");
        assert!(ops.iter().all(|op| matches!(op, Op::Destroy { .. })));
    }

    /// G-C5:退出时即使还开着项目,也要销毁全部预览页面(`closing == true`)。
    #[test]
    fn quitting_destroys_every_preview_webview() {
        let project = dozer_model::ProjectInfo {
            id: 1,
            path: "/tmp/p1".into(),
            name: "p1".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        assert!(
            preview_project(Some(&project), true).is_none(),
            "退出时清单必须为空"
        );
    }

    #[test]
    fn default_conversations_shell_matches_actual_dozer_native_geometry() {
        let d = shell::defaults();
        let mut model = ShellModel::new(d.rails, d.selected, d.left_width, (1200., 800.)).unwrap();
        let _ = shell::update(
            &mut model,
            ShellMsg::PanelPress {
                panel: shell::conversations_id(),
            },
        );
        let side = model.side_of(&shell::conversations_id()).unwrap();
        let content = model.rects(&metrics_from_theme()).content(side).unwrap();
        let actual = review_native_rect(&model, content, false, 0.);
        eprintln!("S7C_DIGGER_DEFAULT_REVIEW={actual:?} CONTENT={content:?}");
        for (a, b) in [
            (actual.x, 700.),
            (actual.y, 84.),
            (actual.w, 274.4),
            (actual.h, 690.),
        ] {
            assert!(
                (a - b).abs() <= 1.,
                "Digger {actual:?} differs from actual Dozer (700,84,274.4,690)"
            );
        }
        let collapsed = review_native_rect(&model, content, true, 0.);
        assert!(collapsed.w > actual.w);
        let notice = review_native_rect(
            &model,
            content,
            true,
            byteui::theme::geometry::preview_chrome_top_px(),
        );
        assert_eq!(
            notice.y,
            collapsed.y + byteui::theme::geometry::preview_chrome_top_px()
        );
        assert_eq!(
            notice.h,
            collapsed.h - byteui::theme::geometry::preview_chrome_top_px()
        );
        let _ = shell::update(&mut model, ShellMsg::Maximize(side));
        let content = model.rects(&metrics_from_theme()).content(side).unwrap();
        for collapsed in [false, true] {
            let actual = review_native_rect(&model, content, collapsed, 0.);
            let block = if collapsed {
                content
            } else {
                shell::pair_rects(&model, side).unwrap().second
            };
            assert!(actual.x >= block.x && actual.y >= block.y);
            assert!(actual.x + actual.w <= block.x + block.w);
            assert!(actual.y + actual.h <= block.y + block.h);
            assert_eq!(
                actual.y,
                block.y + byteui::theme::geometry::preview_chrome_top_px()
            );
            assert!(
                (actual.y + actual.h
                    - (block.y + block.h
                        - crate::adapters::conversations_display::review_bottom_inset(1.)))
                .abs()
                    < 0.01
            );
        }
    }

    /// G-C3:页签条可用宽度必须每帧取自 `PreviewHub` 的真实几何,不能是常量 0。
    /// 库在用 `avail` 做翻页窗口数学,`0` 会让 50 个页签只剩 1 个可见。
    #[test]
    fn the_tab_bar_gets_the_real_available_width_each_frame() {
        use crate::adapters::preview_display::PreviewDisplay;
        use dozer_panel_preview::ViewerPanel;
        use dozer_panel_preview::display::PanelDisplay;

        let dir = std::env::temp_dir().join(format!("digger-tabbar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut shell = ProductShell::new();
        let project = dozer_model::ProjectInfo {
            id: 5,
            path: dir.to_string_lossy().into_owned(),
            name: "p5".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        shell.preview = Some(crate::adapters::preview::PreviewHub::with_config_dir(
            crate::adapters::preview_native::Host::new(&dir),
            &dir,
        ));
        let hub = shell.preview.as_mut().expect("预览 hub 常在");
        hub.set_current_project(Some(&project));
        hub.set_tab_bar_width(640.0, 320.0);

        let display: PreviewDisplay<'_> = preview_display(
            shell
                .preview
                .as_ref()
                .unwrap()
                .tab_bar_px(ViewerPanel::Files),
            5,
            0,
        );
        assert_eq!(
            display.tab_bar_avail_px(ViewerPanel::Files),
            640.0,
            "文件面板的页签条宽度来自配对几何"
        );
        let display: PreviewDisplay<'_> = preview_display(
            shell
                .preview
                .as_ref()
                .unwrap()
                .tab_bar_px(ViewerPanel::Project),
            5,
            0,
        );
        assert_eq!(
            display.tab_bar_avail_px(ViewerPanel::Project),
            320.0,
            "Project 面板读自己那一份"
        );
        assert!(
            display.tab_bar_avail_px(ViewerPanel::Files) > 0.0,
            "宽度不许再是常量 0"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C3:50 个页签时,页签条必须真的翻页(多于 1 个可见);宽度若恒为 0,
    /// 库的 `tab_window` 只会保底放 1 个。
    #[test]
    fn fifty_tabs_show_more_than_one_tab_and_paginate() {
        use byteui::interaction::tab_strip::tab_window;
        use dozer_panel_preview::{TabKind, ViewerPanel};

        let dir = std::env::temp_dir().join(format!("digger-tab50-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut shell = ProductShell::new();
        let project = dozer_model::ProjectInfo {
            id: 6,
            path: dir.to_string_lossy().into_owned(),
            name: "p6".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        shell.preview = Some(crate::adapters::preview::PreviewHub::with_config_dir(
            crate::adapters::preview_native::Host::new(&dir),
            &dir,
        ));
        let hub = shell.preview.as_mut().expect("预览 hub 常在");
        hub.set_current_project(Some(&project));
        hub.set_tab_bar_width(640.0, 320.0);
        for i in 0..50 {
            let path = dir.join(format!("file_{i:02}.txt"));
            std::fs::write(&path, "x\n").unwrap();
            hub.open(ViewerPanel::Files, &project, path, None);
        }
        let state = hub.state(ViewerPanel::Files, 6).expect("文件面板状态");
        let widths: Vec<f32> = state
            .pane
            .tabs()
            .iter()
            .map(|t| match &t.kind {
                TabKind::File(p) => crate::adapters::tab_chrome::tab_display_width(
                    p.file_name().unwrap().to_string_lossy().as_ref(),
                ),
                _ => crate::adapters::tab_chrome::tab_display_width("Blank"),
            })
            .collect();
        let avail = hub.tab_bar_px(ViewerPanel::Files);
        let window = tab_window(&widths, 4.0, avail, 0);
        assert!(
            window.visible_end - window.first > 1,
            "50 个页签只放出 1 个(avail={avail}): {}..{}",
            window.first,
            window.visible_end
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C3:页签条宽度确实由 `sync_native` 的接线写回 hub,而不是测试自己
    /// 手动 `set_tab_bar_width`。走真实的 `collect_preview_desired`。
    #[test]
    fn syncing_the_native_pages_writes_back_the_tab_bar_width() {
        let dir = std::env::temp_dir().join(format!("digger-gc3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = shell::defaults();
        let mut shell =
            ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).unwrap();
        let files_side = shell.side_of(&shell::files_id()).expect("文件面板在布局里");
        let _ = shell::update(
            &mut shell,
            ShellMsg::PanelPress {
                panel: shell::files_id(),
            },
        );
        let Some(pair) = shell::pair_rects(&shell, files_side).map(|p| p.second) else {
            panic!("选中文件面板后该有配对预览块");
        };
        let rects = shell.rects(&bytehost_chrome::metrics_from_theme());
        let mut hub = crate::adapters::preview::PreviewHub::with_config_dir(
            crate::adapters::preview_native::Host::new(&dir),
            &dir,
        );
        let project = dozer_model::ProjectInfo {
            id: 1,
            path: dir.display().to_string(),
            name: "p".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        // 之前是 0:证明宽度不是构造时就有的。
        assert_eq!(hub.tab_bar_px(ViewerPanel::Files), 0.0);

        let _ = collect_preview_desired(&mut hub, &shell, &rects, &project, false);

        let expected = (pair.w - PREVIEW_CHROME_RESERVE_PX).max(0.0);
        assert!(expected > 0.0, "配对预览块得有宽度: {pair:?}");
        assert_eq!(
            hub.tab_bar_px(ViewerPanel::Files),
            expected,
            "sync 必须把页签条可用宽度写回 hub(G-C3)"
        );
        // 没有页签时不该有页面,但宽度照样写回。
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// G-C5:`preview_project` 被 `sync_preview_native` 真实调用——关闭中或
    /// 没有当前项目时都得到 `None`(调用方据此同步空清单销毁页面)。
    #[cfg(target_os = "macos")]
    #[test]
    fn window_ready_registers_the_native_menu_content_view() {
        // 原生菜单要挂靠窗口内容 view;不在窗口就绪时登记,`byteui::native_menu::show` 会因为
        // "内容 view 未注册"而静默不弹。
        let source = include_str!("product.rs");
        let start = source
            .find("Msg::WindowReady(id, size) => {")
            .expect("找不到 WindowReady 分支");
        let rest = &source[start..];
        let end = rest
            .find("Msg::Shell(m) => self.on_shell(m)")
            .expect("找不到下一分支");
        assert!(
            rest[..end].contains("byteui::native_menu::install_content_view"),
            "WindowReady 里必须登记原生菜单的内容 view"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_preview_overflow_toggle_is_intercepted_into_a_native_menu_on_macos() {
        // 不拦截,库的溢出按钮(锚点 `(0,0)`)会出一个锚在左上角的 iced 下拉,并且藏掉页面。
        let source = include_str!("product.rs");
        let start = source
            .find("Msg::Preview(panel, project_id, epoch, message) => {")
            .expect("找不到 Msg::Preview 分支");
        let rest = &source[start..];
        let end = rest.find("Msg::Lifecycle(phase)").expect("找不到下一分支");
        let arm = &rest[..end];
        assert!(arm.contains("TabOverflowToggle"));
        assert!(arm.contains("show_native_preview_overflow"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_agent_overflow_toggle_is_intercepted_into_a_native_menu_on_macos() {
        // 与预览页签的溢出下拉同口径:macOS 一律走原生菜单。选中走 `SelectTabNoDrag`:从菜单选的
        // 不是"按下页签",不能武装拖拽(菜单吞掉了鼠标松开事件,拖拽会一直挂着)。
        let source = include_str!("product.rs");
        let start = source
            .find("Msg::Agent(pid, m) => {")
            .expect("找不到 Msg::Agent 分支");
        let rest = &source[start..];
        let end = rest.find("Msg::AgentRestored").expect("找不到下一分支");
        let arm = &rest[..end];
        assert!(arm.contains("OverflowToggle"));
        assert!(arm.contains("show_native_agent_overflow"));
        let method = source
            .find("fn show_native_agent_overflow")
            .expect("找不到 show_native_agent_overflow");
        assert!(source[method..].contains("SelectTabNoDrag"));
    }

    #[test]
    fn both_context_menu_messages_need_the_click_position() {
        // 库从 `last_right_click` 取菜单锚点,Digger 里没有窗口事件层替它记,不补就是 (0,0)。
        use dozer_panel_files::Message as F;
        use dozer_panel_files::adapter::PreviewKind;
        assert!(opens_a_context_menu(&F::ContextMenuOpen {
            path: "/x".into(),
            is_dir: false
        }));
        assert!(opens_a_context_menu(&F::TabContextMenuOpen {
            kind: PreviewKind::Files,
            path: "/x".into()
        }));
        assert!(!opens_a_context_menu(&F::ContextMenuClose));
        assert!(!opens_a_context_menu(&F::CopyPath(
            "/x".into(),
            dozer_panel_files::fs::PathKind::Absolute
        )));
    }

    #[test]
    fn the_files_menu_is_drawn_for_either_panel() {
        assert!(files_menu_is_rendered(true, false));
        assert!(
            files_menu_is_rendered(false, true),
            "Project 面板的预览页签右键"
        );
        assert!(!files_menu_is_rendered(false, false));
    }

    #[test]
    fn a_hover_is_dropped_when_the_tab_count_changes() {
        assert!(hover_survives_tab_change(Some(3), Some(3)));
        assert!(!hover_survives_tab_change(Some(3), Some(2)), "关了一个页签");
        assert!(
            !hover_survives_tab_change(Some(2), Some(3)),
            "新开了一个页签"
        );
        assert!(!hover_survives_tab_change(None, Some(1)), "状态刚建出来");
        assert!(hover_survives_tab_change(None, None));
    }

    #[test]
    fn sync_native_asks_preview_project_before_desiring_anything() {
        let source = include_str!("product.rs");
        let start = source
            .find("fn sync_preview_native(")
            .expect("找不到 sync_preview_native");
        let rest = &source[start..];
        let end = rest
            .find("\n    fn preview_native_tasks(")
            .expect("找不到下一个函数");
        let body = &rest[..end];
        assert!(
            body.contains("preview_project("),
            "sync_preview_native 必须先过 preview_project(G-C5)"
        );
        // 空清单分支必须真的同步(否则旧页面留在窗口上)。
        assert!(
            body.contains("native_preview_task(window, host, Vec::new(), Vec::new())"),
            "无项目 / 关闭中必须同步空清单,把旧页面销毁"
        );
        // 以及行为断言本身。
        let project = dozer_model::ProjectInfo {
            id: 7,
            path: "/tmp/p".into(),
            name: "p".into(),
            last_active_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        };
        assert!(preview_project(Some(&project), false).is_some());
        assert!(
            preview_project(Some(&project), true).is_none(),
            "关闭中不服务"
        );
        assert!(preview_project(None, false).is_none(), "无项目不服务");
    }

    /// 只有被骨架选中、且在配对里的面板才该有预览块。
    #[test]
    fn the_pair_rects_exist_only_for_the_selected_paired_panels() {
        let d = shell::defaults();
        let mut shell =
            ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).unwrap();
        let files_side = shell.side_of(&shell::files_id()).expect("文件面板在布局里");
        let _ = shell::update(
            &mut shell,
            ShellMsg::PanelPress {
                panel: shell::files_id(),
            },
        );
        assert!(shell::pair_rects(&shell, files_side).is_some());
        // 选中同侧的另一块(非配对)面板后,该侧不再是配对的可渲染状态。
        let other = [shell::info_id(), shell::apps_id()]
            .into_iter()
            .find(|id| shell.side_of(id) == Some(files_side))
            .expect("同侧总还有别的面板");
        let _ = shell::update(&mut shell, ShellMsg::PanelPress { panel: other });
        let chosen = shell
            .side_of(&shell::files_id())
            .is_some_and(|side| shell.selected(side) == &shell::files_id());
        assert!(!chosen);
    }

    #[test]
    fn starts_without_project() {
        let shell = ProductShell::new();
        assert!(shell.boot_note.contains("尚未打开 Project"));
    }

    #[test]
    fn a_project_opened_in_the_shell_is_current_and_comes_back_after_a_restart() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("project-restore");
        let proj = dir.join("book");
        std::fs::create_dir(&proj).unwrap();
        let info = dozer_model::ProjectInfo {
            id: 1,
            path: proj.to_string_lossy().into_owned(),
            name: "book".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        send(&mut shell, Msg::Projects(P::Opened(Ok(Some(info.clone())))));
        assert_eq!(shell.projects.current_project_id(), Some(1));

        let mut again = ProductShell::with_shell_path(dir.join("shell.json"));
        send(
            &mut again,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![info])))),
        );
        assert_eq!(again.projects.current_project_id(), Some(1));
    }

    /// 布局按项目一份(Task 9):切项目页签换布局,窗口大小仍写全局。
    #[test]
    fn switching_project_tabs_swaps_the_layout_and_keeps_the_window_global() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("project-scope");
        for name in ["a", "b"] {
            std::fs::create_dir(dir.join(name)).unwrap();
        }
        let info = |id: i64, name: &str| dozer_model::ProjectInfo {
            id,
            path: dir.join(name).to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: id as u64,
            created_ms: 1,
            updated_ms: 1,
        };
        // 线程复用会让这个目录带着上一轮的残留(页签、项目库、项目布局都算):构造壳之前全清掉,
        // 再写 B 的项目布局。`project_tabs.json` 里残留的"当前项目"会让壳一启动就切到那个项目。
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        std::fs::create_dir(dir.join("shell-projects")).unwrap();
        let mut b_saved = shell::defaults();
        b_saved.left_width = 400.0;
        let b_model = ShellModel::new(
            b_saved.rails.clone(),
            b_saved.selected.clone(),
            b_saved.left_width,
            b_saved.window,
        )
        .unwrap();
        bytehost_chrome::persist::save_atomic(&project_path_in(&dir, 2), &b_model).unwrap();
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        assert_eq!(shell.projects.current_project_id(), None);
        let a = info(1, "a");
        let b = info(2, "b");
        // `Opened` 的后续步骤是异步任务,测试里跑不到:直接用 `Listed` 建快照并打开这两个页签。
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                a.clone(),
                b.clone(),
            ])))),
        );
        // 两张页签都开,但只让 A 当当前项目:B 此刻不该被任何一次保存碰到。
        shell.projects.add_tab_for_test(a.id);
        shell.projects.add_tab_for_test(b.id);
        send(&mut shell, Msg::Projects(P::Activate(a.id)));
        assert_eq!(shell.projects.current_project_id(), Some(a.id));

        let a_path = project_path_in(&dir, a.id);
        let b_path = project_path_in(&dir, b.id);

        // 在 A 里把左区拉宽:落到 A 的项目文件,不碰 B 的。
        send(
            &mut shell,
            Msg::Shell(ShellMsg::DividerPress(Divider::Zones)),
        );
        send(
            &mut shell,
            Msg::Shell(ShellMsg::CursorMoved { x: 544.0, y: 300.0 }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert_eq!(shell.shell.left_width(), 500.0);
        assert!(a_path.exists(), "A's drag is written to A's file");
        let b_before = std::fs::read(&b_path).unwrap();

        // 全局文件仍存在(窗口大小写在那里)。
        assert!(matches!(
            shell::load(&dir.join("shell.json")),
            Loaded::Restored(_)
        ));

        // 切到 B(第一次把它变成当前项目):B 自己的布局(400)回来,不是 A 的 500。
        send(&mut shell, Msg::Projects(P::Activate(b.id)));
        assert_eq!(shell.shell.left_width(), 400.0, "B's layout, not A's");
        assert_eq!(
            std::fs::read(&b_path).unwrap(),
            b_before,
            "B untouched by A"
        );

        // 切回 A:500 仍在。
        send(&mut shell, Msg::Projects(P::Activate(a.id)));
        assert_eq!(shell.shell.left_width(), 500.0, "A's layout comes back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_project_the_layout_goes_to_the_global_file() {
        let dir = test_temp_dir("global-only");
        let path = dir.join("shell.json");
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conversations_is_registered_as_a_native_panel() {
        let shell = fresh("conversations-catalog");
        let id = PanelId::new("digger.conversations").unwrap();
        let entry = shell.catalog.get(&id).expect("conversations registered");
        assert_eq!(entry.title, "会话");
        assert!(entry.surfaces.is_empty());
    }

    fn conversations_shell(
        tag: &str,
    ) -> (
        ProductShell,
        tokio::runtime::Runtime,
        Arc<crate::adapters::conversations::tests::FakeConversation>,
    ) {
        let rt = crate::adapters::conversations::tests::runtime();
        let (panel, fake, _) = crate::adapters::conversations::tests::panel(&rt);
        let mut shell = fresh(tag);
        shell.conversations = panel;
        shell.agent.shell.status = crate::adapters::agent::Status::Ready;
        (shell, rt, fake)
    }

    #[test]
    fn conversations_import_start_is_limited_to_current_project_and_never_runs_during_exit() {
        use crate::adapters::conversations::tests::{FakeTranscript, import_panel, project};
        use crate::adapters::conversations_import::ImportEvent;
        let (mut shell, rt, _) = conversations_shell("conv-import-explicit-scope");
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(FakeTranscript::default());
        shell.conversations = import_panel(&rt, &dir.path().join("marker.json"), fake.clone());
        let a = project(1, "/a");
        let b = project(2, "/b");
        shell.projects.seed_for_test(a.clone());
        shell.projects.seed_for_test(b.clone());
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Activate(b.id)),
        );
        send(
            &mut shell,
            Msg::ConversationsImport(a.id, ImportEvent::Start),
        );
        rt.block_on(async { tokio::task::yield_now().await });
        assert!(fake.calls.lock().unwrap().is_empty());
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Activate(a.id)),
        );
        shell.closing = true;
        send(
            &mut shell,
            Msg::ConversationsImport(a.id, ImportEvent::Start),
        );
        rt.block_on(async { tokio::task::yield_now().await });
        assert!(fake.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn conversations_results_route_to_the_token_project_even_while_it_is_unfocused() {
        use crate::adapters::conversations::tests::{project, row};
        use dozer_panel_conversations::Message as C;
        let (mut shell, _rt, _) = conversations_shell("conv-token-routing");
        let a = project(1, "/a");
        let b = project(2, "/b");
        shell.projects.seed_for_test(a.clone());
        shell.projects.seed_for_test(b.clone());
        shell.conversations.ensure_state(&a);
        shell.conversations.ensure_state(&b);
        let token = shell
            .conversations
            .state(a.id)
            .unwrap()
            .requests
            .begin(Some(a.id), "sessions");
        send(
            &mut shell,
            Msg::Conversations(
                b.id,
                C::AsyncResult(
                    token,
                    Box::new(C::SessionsRefreshed(a.id, Ok(vec![row("a")]))),
                ),
            ),
        );
        assert_eq!(
            shell.conversations.state(a.id).unwrap().sessions().unwrap()[0].conversation_id,
            "a"
        );
        assert!(
            shell
                .conversations
                .state(b.id)
                .unwrap()
                .sessions()
                .is_none()
        );
    }

    #[test]
    fn conversations_tab_close_reclaims_state_and_drops_late_results() {
        use crate::adapters::conversations::tests::{project, row};
        use dozer_panel_conversations::Message as C;
        let (mut shell, _rt, _) = conversations_shell("conv-close");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Close(a.id)),
        );
        assert!(shell.conversations.state(a.id).is_none());
        send(
            &mut shell,
            Msg::Conversations(a.id, C::SessionsRefreshed(a.id, Ok(vec![row("late")]))),
        );
        assert!(shell.conversations.state(a.id).is_none());
    }

    #[test]
    fn conversations_project_delete_reclaims_state() {
        use crate::adapters::conversations::tests::project;
        let (mut shell, _rt, _) = conversations_shell("conv-delete");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        send(
            &mut shell,
            Msg::ProjectDeleteDone {
                project_id: a.id,
                deregistered: true,
                errors: vec![],
            },
        );
        assert!(shell.conversations.state(a.id).is_none());
    }

    #[test]
    fn conversations_shared_daemon_recovery_fetches_once_without_starting_another_lifecycle() {
        use crate::adapters::agent::{AgentShellMsg, Status};
        use crate::adapters::conversations::tests::{project, receive};
        let (mut shell, rt, fake) = conversations_shell("conv-recovery");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.agent.shell.status = Status::Unavailable("offline".into());
        press(&mut shell, &shell::conversations_id());
        assert!(shell.conversations.state(a.id).is_some());
        assert!(fake.calls.lock().unwrap().is_empty());
        send(&mut shell, Msg::AgentShell(AgentShellMsg::Ensured(Ok(()))));
        let (pid, m) = receive(&mut shell.conversations, &rt);
        send(&mut shell, Msg::Conversations(pid, m));
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        assert!(
            shell
                .conversations
                .state(a.id)
                .unwrap()
                .sessions()
                .is_some()
        );
    }

    #[test]
    fn conversations_project_switch_refreshes_the_visible_new_project_only() {
        use crate::adapters::conversations::tests::{project, receive};
        let (mut shell, rt, fake) = conversations_shell("conv-switch");
        let a = project(1, "/a");
        let b = project(2, "/a/b");
        shell.projects.seed_for_test(a.clone());
        shell.projects.seed_for_test(b.clone());
        press(&mut shell, &shell::conversations_id());
        let _ = receive(&mut shell.conversations, &rt);
        // The new project's persisted layout also has Conversations selected.
        let mut defaults = shell::defaults();
        defaults.selected[1] = shell::conversations_id();
        let layout = ShellModel::new(
            defaults.rails,
            defaults.selected,
            defaults.left_width,
            defaults.window,
        )
        .unwrap();
        let path = shell.project_shell_path_for(a.id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        bytehost_chrome::persist::save_atomic(&path, &layout).unwrap();
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Activate(a.id)),
        );
        let _ = receive(&mut shell.conversations, &rt);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["/a/b", "/a"]
        );
    }

    #[test]
    fn conversations_deleting_the_current_project_refreshes_the_remaining_visible_project() {
        use crate::adapters::conversations::tests::{project, receive};
        let (mut shell, rt, fake) = conversations_shell("conv-delete-switch");
        let a = project(1, "/a");
        let b = project(2, "/a/b");
        shell.projects.seed_for_test(a.clone());
        shell.projects.seed_for_test(b.clone());
        press(&mut shell, &shell::conversations_id());
        let _ = receive(&mut shell.conversations, &rt);
        let mut defaults = shell::defaults();
        defaults.selected[1] = shell::conversations_id();
        let model = ShellModel::new(
            defaults.rails,
            defaults.selected,
            defaults.left_width,
            defaults.window,
        )
        .unwrap();
        let path = shell.project_shell_path_for(a.id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        bytehost_chrome::persist::save_atomic(&path, &model).unwrap();
        send(
            &mut shell,
            Msg::ProjectDeleteDone {
                project_id: b.id,
                deregistered: true,
                errors: vec![],
            },
        );
        assert!(shell.conversations.state(b.id).is_none());
        assert_eq!(shell.projects.current_project_id(), Some(a.id));
        let _ = receive(&mut shell.conversations, &rt);
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.0.clone())
                .collect::<Vec<_>>(),
            vec!["/a/b", "/a"]
        );
    }

    type PendingUnavailableConversations = (
        ProductShell,
        tokio::runtime::Runtime,
        Arc<crate::adapters::conversations::tests::FakeConversation>,
        dozer_model::ProjectInfo,
        (i64, dozer_panel_conversations::Message),
    );
    fn pending_unavailable_conversations(tag: &str) -> PendingUnavailableConversations {
        use crate::adapters::conversations::tests::{project, receive};
        let (mut shell, rt, fake) = conversations_shell(tag);
        let project = project(1, "/a");
        shell.projects.seed_for_test(project.clone());
        *fake.error.lock().unwrap() = Some(dozer_services::ServiceError::new(
            dozer_services::ServiceErrorKind::Unavailable,
            "socket disconnected",
        ));
        press(&mut shell, &shell::conversations_id());
        let pending = receive(&mut shell.conversations, &rt);
        (shell, rt, fake, project, pending)
    }

    fn deliver_conversations_task(
        shell: &mut ProductShell,
        rt: &tokio::runtime::Runtime,
        pid: i64,
        message: dozer_panel_conversations::Message,
    ) {
        use iced::futures::StreamExt;
        let task = Product::update(
            shell,
            HostMessage::Product(Msg::Conversations(pid, message)),
        );
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            let messages = rt.block_on(async {
                let mut messages = vec![];
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        messages.push(message);
                    }
                }
                messages
            });
            for message in messages {
                send(shell, message);
            }
        }
    }

    #[test]
    fn conversations_accepted_unavailable_changes_shared_status_and_retry_refetches() {
        use crate::adapters::agent::{AgentShellMsg, Status};
        use crate::adapters::conversations::tests::{receive, view_texts};
        let (mut shell, rt, fake, project, (pid, message)) =
            pending_unavailable_conversations("conv-accepted-unavailable");
        assert_eq!(shell.agent.shell.status, Status::Ready);
        deliver_conversations_task(&mut shell, &rt, pid, message);
        assert!(
            matches!(&shell.agent.shell.status, Status::Unavailable(reason) if reason.contains("socket disconnected"))
        );
        assert!(
            shell
                .conversations
                .notice
                .as_deref()
                .unwrap()
                .contains("socket disconnected")
        );
        let texts = view_texts(
            &shell.conversations,
            Some(&project),
            &shell.agent.shell,
            700.,
            false,
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("socket disconnected")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|text| text == "重试"), "{texts:?}");
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("没有会话") || text.contains("No conversations")),
            "{texts:?}"
        );
        let empty = dozer_panel_conversations::i18n::t(
            "empty.no_sessions",
            "No conversations yet",
            "zh-CN",
        );
        assert!(!texts.contains(&empty), "{texts:?}");
        *fake.error.lock().unwrap() = None;
        let retry = Product::update(
            &mut shell,
            HostMessage::Product(Msg::AgentShell(AgentShellMsg::Retry)),
        );
        assert_eq!(shell.agent.shell.status, Status::Checking);
        // Retry queues exactly the existing daemon ensure task; do not launch a daemon in this test.
        assert!(iced_runtime::task::into_stream(retry).is_some());
        send(&mut shell, Msg::AgentShell(AgentShellMsg::Ensured(Ok(()))));
        let (pid, message) = receive(&mut shell.conversations, &rt);
        deliver_conversations_task(&mut shell, &rt, pid, message);
        assert_eq!(shell.agent.shell.status, Status::Ready);
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        assert_eq!(
            shell
                .conversations
                .state(project.id)
                .unwrap()
                .sessions()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn conversations_stale_unavailable_does_not_change_shared_daemon_status() {
        use crate::adapters::agent::Status;
        let (mut shell, rt, _, project, (pid, message)) =
            pending_unavailable_conversations("conv-stale-unavailable");
        let _ = shell.conversations.activate(&project);
        deliver_conversations_task(&mut shell, &rt, pid, message);
        assert_eq!(shell.agent.shell.status, Status::Ready);
        assert!(shell.conversations.notice.is_none());
    }

    #[test]
    fn conversations_closed_project_unavailable_does_not_change_shared_daemon_status() {
        use crate::adapters::agent::Status;
        let (mut shell, rt, _, project, (pid, message)) =
            pending_unavailable_conversations("conv-closed-unavailable");
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Close(project.id)),
        );
        deliver_conversations_task(&mut shell, &rt, pid, message);
        assert_eq!(shell.agent.shell.status, Status::Ready);
        assert!(shell.conversations.notice.is_none());
        assert!(shell.conversations.state(project.id).is_none());
    }

    #[test]
    fn conversations_reopened_owner_unavailable_does_not_change_shared_daemon_status() {
        use crate::adapters::agent::Status;
        let (mut shell, rt, _, project, (pid, message)) =
            pending_unavailable_conversations("conv-reopened-unavailable");
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Close(project.id)),
        );
        send(
            &mut shell,
            Msg::Projects(crate::projects::Message::Opened(Ok(Some(project.clone())))),
        );
        deliver_conversations_task(&mut shell, &rt, pid, message);
        assert_eq!(shell.agent.shell.status, Status::Ready);
        assert!(shell.conversations.notice.is_none());
        assert!(
            shell
                .conversations
                .state(project.id)
                .unwrap()
                .sessions()
                .is_none()
        );
    }

    #[test]
    fn browser_is_registered_as_a_native_panel_without_surfaces() {
        let shell = fresh("browser-catalog");
        let entry = shell
            .catalog
            .get(&shell::browser_id())
            .expect("browser registered");
        assert_eq!(entry.title, "浏览器");
        assert!(entry.surfaces.is_empty());
    }

    /// 面板被切入时建当前项目的状态;已经显示时切项目,新项目也立刻有状态(不是空白);
    /// 关掉项目清理它的状态。
    #[test]
    fn browser_state_follows_the_visible_panel_and_the_current_project() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("browser-project-follow");
        for name in ["a", "b"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let project = |id: i64, name: &str| dozer_model::ProjectInfo {
            id,
            path: dir.join(name).to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: id as u64,
            created_ms: 1,
            updated_ms: 1,
        };
        let (a, b) = (project(1, "a"), project(2, "b"));
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                a.clone(),
                b.clone(),
            ])))),
        );
        shell.projects.add_tab_for_test(a.id);
        shell.projects.add_tab_for_test(b.id);
        send(&mut shell, Msg::Projects(P::Activate(a.id)));
        assert!(shell.browser.state(a.id).is_none(), "面板没显示:不建状态");
        press(&mut shell, &shell::browser_id());
        assert!(shell.panel_visible(&shell::browser_id()));
        assert!(
            shell.browser.state(a.id).is_some(),
            "切入面板:当前项目有状态"
        );
        assert!(shell.browser.state(b.id).is_none());
        send(&mut shell, Msg::Projects(P::Activate(b.id)));
        assert!(
            shell.browser.state(b.id).is_some(),
            "面板已显示时切项目:新项目立刻有状态"
        );
        // 项目 A 的页签保留(切回来还在),不因为切走就丢。
        assert!(shell.browser.state(a.id).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `Msg::Browser` 按信封里的项目路由,不按此刻聚焦的项目:迟到的 A 的消息不能改到 B 身上。
    #[test]
    fn browser_messages_route_to_the_project_that_sent_them() {
        let mut shell = fresh("browser-routing");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        shell.browser.attach_handle(rt.handle().clone());
        let (a, b) = (info(1), info(2));
        shell.browser.ensure_state(&a);
        shell.browser.ensure_state(&b);
        // 这两个项目没有真的打开(测试壳里没有项目页签):消息被丢弃,状态原样。
        send(
            &mut shell,
            Msg::Browser(
                1,
                dozer_panel_browser::Message::OpenUrl("https://x.com/".into()),
            ),
        );
        assert_eq!(shell.browser.state(1).unwrap().tab_count(), 1);
        assert_eq!(shell.browser.state(2).unwrap().tab_count(), 1);
    }

    /// 关掉项目时,浏览器状态与排队的导航一并清掉。
    #[test]
    fn closing_a_project_clears_its_browser_state() {
        let mut shell = fresh("browser-close");
        let a = info(1);
        shell.browser.ensure_state(&a);
        shell.browser.close_project(a.id);
        assert!(shell.browser.state(a.id).is_none());
    }

    /// 浏览器第一次显示时必须装 Edit 菜单(没有它 webview 里 ⌘A/⌘C/⌘V/⌘X 全不能用,探针实测)。
    /// 装菜单要主线程 + 真实 NSApplication,测试里调不得;这里钉住调用没被删掉、且只在浏览器分支里。
    #[test]
    fn showing_the_browser_installs_the_edit_menu() {
        let src = include_str!("product.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests").expect("tests module")];
        let branch_start = prod
            .find("if *panel == shell::browser_id() {")
            .expect("browser on_visible branch");
        let branch = &prod[branch_start..branch_start + 900];
        assert!(branch.contains("install_standard_edit_menu()"));
        assert_eq!(prod.matches("install_standard_edit_menu()").count(), 1);
    }

    #[test]
    fn registers_product_panels() {
        let shell = ProductShell::new();
        for id in shell::all_panels() {
            assert!(shell.catalog.contains(&id), "{id}");
        }
        assert_eq!(shell.catalog.len(), 11);
        // 编辑器声明一个主 Surface 槽位;纯 iced 原生面板不声明。
        assert_eq!(
            shell.catalog.get(&shell::markdown_id()).unwrap().surfaces,
            vec![bytehost_desktop::SurfaceSlot::MAIN]
        );
        assert!(
            shell
                .catalog
                .get(&shell::info_id())
                .unwrap()
                .surfaces
                .is_empty()
        );
    }

    #[test]
    fn dismiss_layer_press_closes_the_menu() {
        let mut shell = fresh("agent-overflow-dismiss");
        shell.agent.overflow = Some((7, (10.0, 10.0)));
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::AgentOverflowDismiss));
        assert!(shell.agent.overflow.is_none());
    }

    #[test]
    fn escape_closes_the_topmost_overlay_then_nothing_else() {
        use crate::adapters::agent::PickerMenu;
        let mut shell = fresh("agent-escape");
        shell.agent.open_picker(PickerMenu { pid: 7, at: None });
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::AgentEscape));
        assert!(shell.agent.picker.is_none());
        shell.agent.overflow = Some((7, (10.0, 10.0)));
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::AgentEscape));
        assert!(shell.agent.overflow.is_none());
        // 没有覆盖层时 Esc 什么都不做(不 panic)。
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::AgentEscape));
        assert!(!shell.agent.overlay_is_open());
    }

    #[test]
    fn confirm_dialog_blocks_terminal_keys() {
        use dozer_panel_agent::state::AgentPanelState;
        let mut shell = fresh("agent-confirm-blocks");
        assert!(!shell.agent_keys_blocked());
        let mut state = AgentPanelState::new();
        state.pending_close_tab = Some("s1".into());
        shell.agent.states.insert(7, state);
        assert!(shell.agent.confirm_is_open(Some(7)));
        // 覆盖层同样阻断键盘。
        shell.agent.open_overflow(7, (0.0, 0.0));
        assert!(shell.agent_keys_blocked());
    }

    fn focus_search(shell: &mut ProductShell, pid: i64, focused: bool) {
        let empty = dozer_panel_conversations::WorkspaceState::default();
        let state = shell.conversations.state(pid).unwrap_or(&empty);
        let token = state.requests.begin(Some(pid), "host-search-focus");
        send(shell, Msg::ConversationSearchFocused(pid, token, focused));
    }

    #[test]
    fn conversations_hidden_filter_requests_are_drained_without_opening_or_changing_filter() {
        use crate::adapters::conversations::tests::project;
        use dozer_panel_conversations::Message as C;
        let (mut shell, _rt, _) = conversations_shell("conv-hidden-menu");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        send(
            &mut shell,
            Msg::Conversations(
                1,
                C::AgentFilterSelect(Some(dozer_model::AgentKind::Claude)),
            ),
        );
        send(
            &mut shell,
            Msg::Conversations(1, C::AgentFilterOpen { at: (0., 0.) }),
        );
        assert!(shell.conversations.take_requests().is_empty());
        assert!(!shell.conversations.state(1).unwrap().agent_picker_open());
        assert_eq!(
            shell.conversations.state(1).unwrap().agent_filter(),
            Some(dozer_model::AgentKind::Claude)
        );
    }

    #[test]
    fn conversations_iced_fallback_closes_agent_overlays_and_agent_picker_closes_it() {
        use crate::adapters::agent::PickerMenu;
        use crate::adapters::conversations::tests::project;
        let (mut shell, rt, _) = conversations_shell("conv-iced-fallback");
        shell.agent.attach_handle(rt.handle().clone());
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        shell
            .agent
            .states
            .insert(1, dozer_panel_agent::state::AgentPanelState::new());
        shell.agent.open_picker(PickerMenu { pid: 1, at: None });
        shell.show_iced_conversation_filter(1);
        assert!(!shell.agent.overlay_is_open());
        assert!(shell.conversations.state(1).unwrap().agent_picker_open());
        send(
            &mut shell,
            Msg::Agent(1, dozer_panel_agent::update::Message::PickerToggle),
        );
        assert!(!shell.conversations.state(1).unwrap().agent_picker_open());
        assert!(shell.agent.picker.is_some());
        shell.agent.open_overflow(1, (0., 0.));
        shell.show_iced_conversation_filter(1);
        assert!(!shell.agent.overlay_is_open());
        assert!(shell.conversations.state(1).unwrap().agent_picker_open());
        assert!(shell.agent.states[&1].term_tab_overflow_anchor.is_none());
        assert!(!shell.agent.states[&1].agent_picker_open);
        send(
            &mut shell,
            Msg::Conversations(1, dozer_panel_conversations::Message::AgentFilterClose),
        );
        assert!(!shell.conversations.state(1).unwrap().agent_picker_open());
    }

    #[test]
    fn overlays_stay_mutually_exclusive_with_the_agent_picker_and_overflow_menu() {
        use crate::adapters::agent::PickerMenu;
        let mut shell = fresh("conv-iced-overlay-exclusion");
        shell
            .agent
            .states
            .insert(1, dozer_panel_agent::state::AgentPanelState::new());
        shell.agent.open_picker(PickerMenu { pid: 1, at: None });
        assert!(shell.agent.picker.is_some() && shell.agent.overflow.is_none());
        shell.agent.open_overflow(1, (0., 0.));
        assert!(shell.agent.picker.is_none() && shell.agent.overflow.is_some());
        shell.agent.open_picker(PickerMenu { pid: 1, at: None });
        assert!(shell.agent.picker.is_some() && shell.agent.overflow.is_none());
        assert!(shell.agent_keys_blocked());
    }

    #[test]
    fn conversations_real_focus_operation_updates_gain_and_loss_and_rejects_closed_owner() {
        use crate::adapters::conversations::tests::{project, search_focus_feedback};
        let (mut shell, rt, _) = conversations_shell("conv-real-focus");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        press(&mut shell, &shell::conversations_id());
        for focused in [true, false] {
            let task = Product::update(
                &mut shell,
                HostMessage::Product(Msg::ConversationSearchFocusFrame(1)),
            );
            let message = search_focus_feedback(&shell.conversations, &a, &rt, task, focused);
            send(&mut shell, message);
            assert_eq!(
                shell.conversations.state(1).unwrap().search_focused(),
                focused
            );
        }
        let task = Product::update(
            &mut shell,
            HostMessage::Product(Msg::ConversationSearchFocusFrame(1)),
        );
        let stale = search_focus_feedback(&shell.conversations, &a, &rt, task, true);
        shell.conversations.close_project(1);
        shell.conversations.ensure_state(&a);
        send(&mut shell, stale);
        assert!(
            !shell.conversations.state(1).unwrap().search_focused(),
            "closed owner's query must not focus a reopened state"
        );
    }

    #[test]
    fn conversations_opposite_rail_allows_agent_keys_until_search_really_has_focus() {
        use crate::adapters::conversations::tests::project;
        let (mut shell, _rt, _) = conversations_shell("conv-both-rails");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a);
        press(&mut shell, &shell::conversations_id());
        send(
            &mut shell,
            Msg::Shell(ShellMsg::RailDragMove {
                side: Side::Left,
                index: 0,
            }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert!(shell.panel_visible(&shell::conversations_id()));
        assert!(shell.panel_visible(&shell::agent_id()));
        assert!(!shell.agent_keys_blocked());
        focus_search(&mut shell, 1, true);
        assert!(shell.agent_keys_blocked());
        let _ = update_host(
            &mut shell,
            HostMessage::Product(Msg::Shell(ShellMsg::TogglePane(shell::conversations_id()))),
        );
        assert!(!shell.conversations.state(1).unwrap().search_focused());
        assert!(!shell.agent_keys_blocked());
    }

    #[test]
    fn conversations_state_changed_alone_does_not_refresh() {
        use crate::adapters::conversations::tests::project;
        let (mut shell, rt, fake) = conversations_shell("conv-state-event");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        shell.agent.push_host_event_for_test(
            1,
            dozer_panel_agent::adapter::AgentHostEvent::StateChanged {
                tab_id: 0,
                agent: dozer_model::AgentKind::Claude,
                state: dozer_model::AgentState::Running,
                transcript_path: None,
            },
        );
        let _ = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        rt.block_on(async { tokio::task::yield_now().await });
        assert!(fake.calls.lock().unwrap().is_empty());
        assert!(shell.conversations.dirty.is_empty());
    }

    #[test]
    fn conversations_host_turn_events_respect_readiness_visibility_and_current_project() {
        use crate::adapters::agent::Status;
        use crate::adapters::conversations::tests::{project, receive};
        use dozer_panel_agent::adapter::AgentHostEvent;
        let (mut shell, rt, fake) = conversations_shell("conv-turn-events");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        shell.conversations.ensure_state(&a);
        // Hidden event defers work.
        shell
            .agent
            .push_host_event_for_test(1, AgentHostEvent::TurnEnded { tab_id: 0 });
        let _ = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        assert!(shell.conversations.dirty.contains(&1));
        press(&mut shell, &shell::conversations_id());
        let _ = receive(&mut shell.conversations, &rt);
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        assert!(!shell.conversations.dirty.contains(&1));
        // Visible current ready event refreshes immediately.
        shell
            .agent
            .push_host_event_for_test(1, AgentHostEvent::TurnEnded { tab_id: 0 });
        let _ = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        let _ = receive(&mut shell.conversations, &rt);
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        shell.agent.shell.status = Status::Unavailable("offline".into());
        shell
            .agent
            .push_host_event_for_test(1, AgentHostEvent::TurnEnded { tab_id: 0 });
        let _ = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        rt.block_on(async { tokio::task::yield_now().await });
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        shell.agent.shell.status = Status::Ready;
        shell.projects.seed_for_test(project(2, "/b"));
        shell
            .agent
            .push_host_event_for_test(1, AgentHostEvent::TurnEnded { tab_id: 0 });
        shell
            .agent
            .push_host_event_for_test(2, AgentHostEvent::TurnEnded { tab_id: 0 });
        let _ = update_host(&mut shell, HostMessage::Product(Msg::Tick));
        rt.block_on(async { tokio::task::yield_now().await });
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        assert!(shell.conversations.state(2).is_none());
    }

    #[test]
    fn conversations_search_focus_blocks_keys_and_late_hidden_focus_is_ignored() {
        use crate::adapters::conversations::tests::project;
        let (mut shell, _rt, _) = conversations_shell("conv-focus-routing");
        let a = project(1, "/a");
        shell.projects.seed_for_test(a.clone());
        press(&mut shell, &shell::conversations_id());
        focus_search(&mut shell, 1, true);
        assert!(shell.conversations.state(1).unwrap().search_focused());
        assert!(shell.agent_keys_blocked());
        focus_search(&mut shell, 1, false);
        assert!(!shell.conversations.state(1).unwrap().search_focused());
        press(&mut shell, &shell::agent_id());
        focus_search(&mut shell, 1, true);
        assert!(!shell.conversations.state(1).unwrap().search_focused());
        assert!(!shell.agent_keys_blocked());
        press(&mut shell, &shell::conversations_id());
        focus_search(&mut shell, 2, true);
        assert!(!shell.conversations.state(1).unwrap().search_focused());
        shell.conversations.close_project(1);
        focus_search(&mut shell, 1, true);
        assert!(shell.conversations.state(1).is_none());
    }

    #[test]
    fn conversations_selected_with_agent_hidden_blocks_uncaptured_terminal_keys() {
        let mut shell = fresh("conv-hidden-keys");
        assert!(!shell.agent_keys_blocked());
        press(&mut shell, &shell::conversations_id());
        assert!(!shell.panel_visible(&shell::agent_id()));
        assert!(shell.agent_keys_blocked());
    }

    #[test]
    fn captured_key_events_never_reach_the_terminal() {
        let event = iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character("x".into()),
            modified_key: iced::keyboard::Key::Character("x".into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            text: Some("x".into()),
            repeat: false,
        };
        assert!(agent_key_message(iced::event::Status::Captured, &event).is_none());
        assert!(matches!(
            agent_key_message(iced::event::Status::Ignored, &event),
            Some(Msg::AgentKey(_))
        ));
    }

    #[test]
    fn escape_and_outside_click_close_the_picker_without_creating() {
        use crate::adapters::agent::PickerMenu;
        let mut shell = fresh("agent-picker-close");
        let mut state = dozer_panel_agent::state::AgentPanelState::new();
        state.agent_picker_open = true;
        shell.agent.states.insert(7, state);
        shell.agent.picker = Some(PickerMenu { pid: 7, at: None });
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::AgentPickerClose));
        assert!(shell.agent.picker.is_none());
        assert!(!shell.agent.states[&7].agent_picker_open);
        assert!(
            shell.agent.states[&7].tabs.is_empty(),
            "nothing was created"
        );
    }

    #[test]
    fn the_agent_panel_is_registered_in_the_catalog_and_its_messages_reach_the_shell() {
        let mut shell = fresh("agent-catalog");
        let entry = shell.catalog.get(&shell::agent_id()).expect("registered");
        assert_eq!(entry.title, "Agent");
        // S6a:默认状态改为 Unknown(启动序列开始时置 Checking)。
        assert!(shell.agent.shell.status == crate::adapters::agent::Status::Unknown);
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::AgentShell(
                crate::adapters::agent::AgentShellMsg::Ensured(Err("x".into())),
            )),
        );
        assert_eq!(
            shell.agent.shell.status,
            crate::adapters::agent::Status::Unavailable("x".into())
        );
    }

    #[test]
    fn the_default_shell_shows_the_editor_on_the_left_and_the_agent_on_the_right() {
        let shell = fresh("default");
        assert!(shell.panel_visible(&shell::markdown_id()));
        assert!(shell.panel_visible(&shell::agent_id()));
        assert!(!shell.panel_visible(&shell::info_id()));
        assert!(!shell.panel_visible(&shell::apps_id()));
        assert_eq!(shell.shell.side_of(&shell::apps_id()), Some(Side::Right));
    }

    #[test]
    fn a_panel_is_not_visible_when_its_zone_is_collapsed_or_covered_by_the_other_sides_maximize() {
        let mut shell = fresh("visible");
        press(&mut shell, &shell::markdown_id()); // collapses the left zone
        assert!(!shell.panel_visible(&shell::markdown_id()));
        press(&mut shell, &shell::markdown_id()); // expands it again
        assert!(shell.panel_visible(&shell::markdown_id()));
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Right))),
        );
        assert!(
            !shell.panel_visible(&shell::markdown_id()),
            "covered by the right side's maximize"
        );
        assert!(shell.panel_visible(&shell::agent_id()));
    }

    #[test]
    fn a_shell_change_is_saved_and_restored_on_the_next_start() {
        let dir = test_temp_dir("saved");
        let path = dir.join("shell.json");
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        assert!(path.exists(), "a layout change is written right away");
        let again = ProductShell::with_shell_path(path);
        assert_eq!(again.shell.selected(Side::Left), &shell::info_id());
    }

    /// 复盘发现 4:高版本布局文件退出时**不得覆盖**。
    #[test]
    fn a_newer_schema_shell_file_is_never_overwritten() {
        let dir = test_temp_dir("newer");
        let path = dir.join("shell.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&path, original).unwrap();
        let mut shell = ProductShell::with_shell_path(path.clone());
        press(&mut shell, &shell::info_id());
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "the original file stays byte for byte"
        );
        assert!(shell.boot_note.contains("布局未保存"));
    }

    #[test]
    fn the_native_rect_loses_exactly_the_panel_header_and_never_goes_below_one_pixel() {
        let r = below_header(Rect::new(10.0, 100.0, 300.0, 400.0), 96.0);
        assert_eq!((r.x, r.y, r.w, r.h), (10.0, 196.0, 300.0, 304.0));
        assert_eq!(below_header(Rect::new(0.0, 0.0, 50.0, 40.0), 96.0).h, 1.0);
    }

    /// 评审 Important 3:应用面板只要从"看不见"变成"看得见"就要触发探测 + 拉列表,不能只在"被切入"时。
    /// 上次退出时应用面板被选中但右区收起;重启后点它的图标只是展开(没有 `switched_in`),服务必须随之启动。
    #[tokio::test]
    async fn the_apps_panel_starts_when_expanding_its_zone_makes_it_visible() {
        let dir = test_temp_dir("apps-visible");
        let path = dir.join("shell.json");
        std::fs::write(
            &path,
            r#"{ "selected_right": "digger.apps", "right_collapsed": true }"#,
        )
        .unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert!(
            !shell.panel_visible(&shell::apps_id()),
            "selected but its zone is collapsed"
        );
        assert!(!shell.apps.opened());
        press(&mut shell, &shell::apps_id()); // expands the zone: no panel switch happens
        assert!(shell.panel_visible(&shell::apps_id()));
        assert!(
            shell.apps.opened(),
            "becoming visible by expanding still starts the service"
        );
    }

    /// 评审关注点 4:库的 `update` 对 `ToggleListCollapse` / `CollapseHover` 是 `unreachable!`(它们必须由宿主拦截)。
    /// 任何一条漏进去都会让进程崩溃,所以产品在 `Msg::Usage` 入口先拦截。
    #[test]
    fn library_messages_that_the_kernel_must_intercept_never_reach_the_library() {
        let mut shell = fresh("intercept");
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::CollapseHover(true))),
        );
        assert!(
            shell.shell.panes().get(&shell::usage_id()).is_none(),
            "hover changes nothing"
        );
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Usage(dozer_panel_usage::Message::ToggleListCollapse)),
        );
        assert_eq!(
            shell
                .shell
                .panes()
                .get(&shell::usage_id())
                .map(|d| d.collapsed),
            Some(true),
            "the library's toggle is routed to the shell"
        );
    }

    #[test]
    fn a_successful_rename_updates_the_tab_title() {
        use crate::projects::Message as P;
        let dir = test_temp_dir("rename");
        std::fs::create_dir(dir.join("old-name")).unwrap();
        let old = dozer_model::ProjectInfo {
            id: 7,
            path: dir.join("old-name").to_string_lossy().into_owned(),
            name: "old-name".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                old.clone(),
            ])))),
        );
        shell.projects.open_tab_for_test(old.id);
        send(&mut shell, Msg::Projects(P::Activate(old.id)));
        assert_eq!(shell.projects.current_project().unwrap().name, "old-name");

        let mut renamed = old.clone();
        renamed.name = "new-name".into();
        send(
            &mut shell,
            Msg::Project(dozer_panel_project::Message::NameRenamed(
                old.id,
                Ok(renamed),
            )),
        );
        assert_eq!(
            shell.projects.current_project().unwrap().name,
            "new-name",
            "the registry is the only source of names, so the tab title follows it"
        );
    }

    /// 用户反馈(2026-10-20):Digger 的 Files 单击文件名就崩溃。根因:库对 `TreeRowPress` /
    /// `TreeDragRelease` / `TreeRowDoubleClick` 的 `update` 是 `unreachable!`("由内核拦截"),
    /// Digger 把它们原样交给了库。单击 = 按下 + 松开,必须只选中、不打开预览;双击才打开。
    #[test]
    fn a_single_click_on_a_file_only_selects_it_and_a_double_click_opens_the_preview() {
        use crate::projects::Message as P;
        use dozer_panel_files::Message as F;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let dir = test_temp_dir("click");
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("a.txt"), "hello").unwrap();
        let file = repo.join("a.txt");
        let info = dozer_model::ProjectInfo {
            id: 3,
            path: repo.to_string_lossy().into_owned(),
            name: "repo".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        shell.files.attach_handle(rt.handle().clone());
        shell.preview = Some(crate::adapters::preview::PreviewHub::with_config_dir(
            crate::adapters::preview_native::Host::new(&dir),
            &dir,
        ));
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                info.clone(),
            ])))),
        );
        shell.projects.open_tab_for_test(info.id);
        send(&mut shell, Msg::Projects(P::Activate(info.id)));
        // `open_tab_for_test` 已经让它成了当前项目,切换钩子不会再跑:手动指向树根。
        shell.files.set_project(Some(&info));
        if let Some(hub) = shell.preview.as_mut() {
            hub.set_current_project(Some(&info));
        }

        // 单击 = 按下 + 松开:只选中,不打开预览,不崩溃。
        send(
            &mut shell,
            Msg::Files(F::TreeRowPress {
                path: file.clone(),
                is_dir: false,
            }),
        );
        send(
            &mut shell,
            Msg::Files(F::TreeDragRelease {
                shift: false,
                command: false,
            }),
        );
        assert_eq!(shell.files.selected_paths(), vec![file.clone()]);
        assert!(
            !preview_has_tab_for(&shell, &file),
            "单击不应该打开预览(双击才打开)"
        );

        // 双击文件:在预览里打开。
        send(
            &mut shell,
            Msg::Files(F::TreeRowDoubleClick {
                path: file.clone(),
                is_dir: false,
            }),
        );
        assert!(preview_has_tab_for(&shell, &file), "双击文件应在预览里打开");

        // 双击目录:展开 / 折叠,不是预览。
        send(
            &mut shell,
            Msg::Files(F::TreeRowDoubleClick {
                path: repo.join("sub"),
                is_dir: true,
            }),
        );
    }

    /// 同一类崩溃的整类防护:库里"必须由宿主拦截"的每一条消息,经产品入口发进去都不能 panic。
    #[test]
    fn every_library_message_the_host_must_intercept_is_survivable() {
        use crate::projects::Message as P;
        use dozer_panel_files::Message as F;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let dir = test_temp_dir("intercepts");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let file = repo.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let info = dozer_model::ProjectInfo {
            id: 4,
            path: repo.to_string_lossy().into_owned(),
            name: "repo".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        shell.files.attach_handle(rt.handle().clone());
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                info.clone(),
            ])))),
        );
        shell.projects.open_tab_for_test(info.id);
        send(&mut shell, Msg::Projects(P::Activate(info.id)));
        shell.files.set_project(Some(&info));
        for m in [
            F::TreeRowPress {
                path: file.clone(),
                is_dir: false,
            },
            F::TreeDragRelease {
                shift: false,
                command: false,
            },
            F::TreeRowDoubleClick {
                path: file.clone(),
                is_dir: false,
            },
            F::TreeRowDoubleClick {
                path: repo.clone(),
                is_dir: true,
            },
            F::CopyPath(file.clone(), dozer_panel_files::fs::PathKind::Relative),
            F::TextInputMenuOpen(dozer_panel_files::adapter::TextInputTarget {
                id: iced::widget::Id::new("x"),
                secure: false,
            }),
            F::FileHistoryRollbackPrevious(file.clone()),
            F::RequestSendToAgentTerminal {
                is_dir: false,
                relative: "a.txt".into(),
            },
        ] {
            send(&mut shell, Msg::Files(m));
        }
        // 左键松开(全局事件)也一样:没有拖拽在进行时是空操作。
        send(&mut shell, Msg::LeftButton(true));
        send(&mut shell, Msg::LeftButton(false));
    }

    /// 单击不是拖拽:按住时间太短或位移不够都不能推进到 `Dragging`;真拖拽两道阈值都要过。
    #[test]
    fn a_quick_click_never_becomes_a_drag_but_a_held_move_does() {
        use crate::projects::Message as P;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let dir = test_temp_dir("dragthreshold");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let file = repo.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let info = dozer_model::ProjectInfo {
            id: 5,
            path: repo.to_string_lossy().into_owned(),
            name: "repo".into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        };
        let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
        shell.files.attach_handle(rt.handle().clone());
        send(
            &mut shell,
            Msg::Projects(P::Listed(Ok(crate::projects::Listing::scan(vec![
                info.clone(),
            ])))),
        );
        shell.projects.open_tab_for_test(info.id);
        send(&mut shell, Msg::Projects(P::Activate(info.id)));
        shell.files.set_project(Some(&info));

        shell.files.set_window_probe((800.0, 600.0), (100.0, 100.0));
        shell.files.arm_tree_drag(file.clone(), false);
        // 位移够远但按住时间太短(trackpad 快速点按):不是拖拽。
        shell.files.set_window_probe((800.0, 600.0), (160.0, 100.0));
        assert!(!shell.files.maybe_confirm_tree_drag(true));
        // 按住够久但没动:也不是。
        std::thread::sleep(std::time::Duration::from_millis(350));
        shell.files.set_window_probe((800.0, 600.0), (100.0, 100.0));
        assert!(!shell.files.maybe_confirm_tree_drag(true));
        // 两道都过:才是拖拽。
        shell.files.set_window_probe((800.0, 600.0), (160.0, 100.0));
        assert!(shell.files.maybe_confirm_tree_drag(true));
        // 左键其实没按着却残留拖拽:自愈清空。
        assert!(!shell.files.maybe_confirm_tree_drag(false));
        assert!(!shell.files.tree_drag_active());
    }

    fn preview_has_tab_for(shell: &ProductShell, path: &std::path::Path) -> bool {
        let Some(project) = shell.projects.current_project() else {
            return false;
        };
        shell
            .preview
            .as_ref()
            .and_then(|hub| hub.state(ViewerPanel::Files, project.id))
            .is_some_and(|state| {
                state.pane.tabs().iter().any(
                    |tab| matches!(&tab.kind, dozer_panel_preview::TabKind::File(p) if p == path),
                )
            })
    }

    mod review_lifecycle {
        use super::*;
        include!("product_review_tests.rs");
    }

    mod chat_lifecycle {
        use super::*;
        include!("product_chat_tests.rs");
    }

    fn send(shell: &mut ProductShell, m: Msg) {
        let _ = Product::update(shell, HostMessage::Product(m));
    }

    fn load_sample_rows(shell: &mut ProductShell) {
        send(
            shell,
            Msg::Usage(crate::adapters::usage::sample_loaded_message()),
        );
    }

    #[test]
    fn usage_layout_has_three_states() {
        let mut shell = fresh("layout3");
        assert_eq!(
            shell.usage_layout(),
            UsageLayout::ContentOnly,
            "no data yet: nothing to pair"
        );
        load_sample_rows(&mut shell);
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        assert_eq!(shell.usage_layout(), UsageLayout::Collapsed);
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        assert_eq!(shell.usage_layout(), UsageLayout::Paired);
    }

    /// 评审关注点 3:文件里写着 collapsed 但此刻没有数据 —— 不能显示成一个空列表占位。
    #[test]
    fn collapsed_without_data_is_content_only() {
        let dir = test_temp_dir("collapsed-nodata");
        let path = dir.join("shell.json");
        std::fs::write(
            &path,
            r#"{ "panes": { "digger.usage": { "split": 0.35, "collapsed": true } } }"#,
        )
        .unwrap();
        let mut shell = ProductShell::with_shell_path(path);
        assert_eq!(
            shell
                .shell
                .panes()
                .get(&shell::usage_id())
                .map(|d| d.collapsed),
            Some(true)
        );
        assert_eq!(shell.usage_layout(), UsageLayout::ContentOnly);
        load_sample_rows(&mut shell);
        assert_eq!(
            shell.usage_layout(),
            UsageLayout::Collapsed,
            "the stored state shows up as soon as there is data"
        );
    }

    /// 评审关注点 1、2、5:原生图表只占内容栏,且和 body 读同一个 `usage_layout()`。
    #[test]
    fn native_rect_follows_the_layout() {
        use bytehost_chrome::PairRects;
        let content = Rect::new(693.0, 47.0, 448.0, 700.0);
        // 右区默认:内容在左、列表在右
        let paired = PairRects {
            first: Rect::new(939.0, 47.0, 154.0, 700.0),
            second: Rect::new(693.0, 47.0, 286.0, 700.0),
            divider: Rect::new(979.0, 47.0, 8.0, 700.0),
        };
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, Some(paired)),
            paired.second,
            "only the content pane carries the chart"
        );
        // 无数据 / 收起:整个内容区,即便此刻还拿得到一份旧的配对矩形
        assert_eq!(
            usage_native_rect(UsageLayout::ContentOnly, content, Some(paired)),
            content
        );
        assert_eq!(
            usage_native_rect(UsageLayout::Collapsed, content, Some(paired)),
            content
        );
        // 镜像(拖到左栏):列表在左、内容在右 —— 取的仍是 `second`,位置由骨架几何给出
        let mirrored = PairRects {
            first: Rect::new(693.0, 47.0, 154.0, 700.0),
            second: Rect::new(855.0, 47.0, 286.0, 700.0),
            divider: Rect::new(847.0, 47.0, 8.0, 700.0),
        };
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, Some(mirrored)),
            mirrored.second
        );
        // 没有配对矩形(该侧不是用量)时回落到内容区,不 panic
        assert_eq!(
            usage_native_rect(UsageLayout::Paired, content, None),
            content
        );
    }

    #[tokio::test]
    async fn the_usage_view_builds_in_every_layout_and_side() {
        let mut shell = fresh("usage-views");
        let _ = Product::view(&shell); // ContentOnly
        load_sample_rows(&mut shell);
        let _ = Product::view(&shell); // Paired
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        let _ = Product::view(&shell); // Collapsed
        send(
            &mut shell,
            Msg::Shell(ShellMsg::TogglePane(shell::usage_id())),
        );
        // 拖到左栏(镜像)与放大态
        send(
            &mut shell,
            Msg::Shell(ShellMsg::PanelPress {
                panel: shell::apps_id(),
            }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        send(
            &mut shell,
            Msg::Shell(ShellMsg::PanelPress {
                panel: shell::usage_id(),
            }),
        );
        send(
            &mut shell,
            Msg::Shell(ShellMsg::RailDragMove {
                side: Side::Left,
                index: 0,
            }),
        );
        send(&mut shell, Msg::Shell(ShellMsg::PointerReleased));
        assert_eq!(shell.shell.side_of(&shell::usage_id()), Some(Side::Left));
        let _ = Product::view(&shell);
        send(&mut shell, Msg::Shell(ShellMsg::Maximize(Side::Left)));
        let _ = Product::view(&shell);
    }

    /// 点到「应用」面板会触发探测任务(`tokio::spawn`),所以需要运行时。
    #[tokio::test]
    async fn the_view_builds_in_the_states_a_user_can_reach() {
        let mut shell = fresh("view");
        let _ = Product::view(&shell);
        for panel in shell::all_panels() {
            press(&mut shell, &panel);
            let _ = Product::view(&shell);
        }
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Shell(ShellMsg::Maximize(Side::Left))),
        );
        let _ = Product::view(&shell);
        let _ = Product::update(&mut shell, HostMessage::Product(Msg::Exit));
        let _ = Product::view(&shell);
    }

    /// 复审 #9:损坏账本必须显式 `Unavailable` + **不接线**生产者,而不是退回临时空账本。
    #[tokio::test]
    async fn a_corrupt_usage_ledger_is_unavailable_and_left_untouched() {
        use dozer_services::ServiceErrorKind;

        let dir = test_temp_dir("usage-corrupt");
        let path = dir.join("usage_ledger.json");
        let original = b"{ this is not json";
        std::fs::write(&path, original).unwrap();

        let (service, ledger, note) = load_usage_ledger(path.clone());
        assert!(ledger.is_none(), "损坏时不得接线任何账本(含临时空账本)");
        assert!(note.contains("暂不可用"), "boot 提示应说明不可用:{note}");
        let err = service
            .usage_summary("/p/a".into(), None)
            .await
            .expect_err("损坏账本查询必须 Unavailable,而不是 Ok(空)");
        assert_eq!(err.kind, ServiceErrorKind::Unavailable);

        // 原文件保持原样(未被覆盖/未被悄悄迁移)。
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 复盘发现 6:真实 `ProductShell` 的 Surface 事件经 `update_host` 单次路由,不得无限递归。
    /// `Created`/`Resumed` → 一次 `on_activated`;`Destroyed` → 一次 `on_closed`;`Hidden` → 无回调。
    /// 修复前本用例会因 `route_lifecycle` → `update` → `route_lifecycle` 递归至栈溢出。
    #[test]
    fn surface_events_route_once_without_recursion() {
        let slot = bytehost_desktop::SurfaceSlot(7);
        let mut shell = ProductShell::new();

        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Created 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Resumed { slot }),
        );
        assert_eq!(shell.surface_callbacks, vec![slot], "Resumed 触发一次激活");

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Hidden { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Hidden 不触发生命周期回调"
        );

        shell.surface_callbacks.clear();
        let _ = update_host(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Destroyed { slot }),
        );
        assert_eq!(
            shell.surface_callbacks,
            vec![slot],
            "Destroyed 触发一次关闭"
        );

        // 直接投 Surface 给 `Product::update` 也不得递归(它只消费,不再重投)。
        shell.surface_callbacks.clear();
        let _ = Product::update(
            &mut shell,
            HostMessage::Surface(SurfaceEvent::Created { slot }),
        );
        assert!(
            shell.surface_callbacks.is_empty(),
            "Surface 只在 update_host 路由一次,Product::update 不再分发"
        );
    }

    /// Review Focus 3:待办 webview 的底边永远给新增输入框让位,顶边让开库的
    /// 顶栏行;列表收起 / 宽窄窗口都不重叠。
    #[test]
    fn todo_native_rect_never_overlaps_the_add_input() {
        let top = dozer_panel_todo::adapter::top_row_inner_h_px();
        let add = dozer_panel_todo::ADD_INPUT_MIN_HEIGHT;
        for (content, second) in [
            (
                Rect::new(0.0, 0.0, 640.0, 800.0),
                Rect::new(260.0, 0.0, 380.0, 800.0),
            ),
            (
                Rect::new(0.0, 0.0, 1600.0, 900.0),
                Rect::new(500.0, 0.0, 1100.0, 900.0),
            ),
            (
                Rect::new(0.0, 0.0, 700.0, 500.0),
                Rect::new(300.0, 0.0, 400.0, 500.0),
            ),
        ] {
            let pair = PairRects {
                first: Rect::new(content.x, content.y, 0.0, content.h),
                second,
                divider: Rect::new(second.x, second.y, 0.0, second.h),
            };
            for pair in [Some(pair), None] {
                let rect = todo_native_rect(content, pair, add);
                let slot = pair.map(|p| p.second).unwrap_or(content);
                assert!(rect.x >= slot.x && rect.y >= slot.y + top);
                assert!(rect.x + rect.w <= slot.x + slot.w + 0.001);
                assert!(
                    rect.y + rect.h + add <= slot.y + slot.h + 0.001,
                    "webview must not cover the add input: {rect:?} vs {slot:?}"
                );
            }
        }
    }

    #[test]
    fn todo_native_is_destroyed_when_the_panel_is_not_visible() {
        let mut shell = fresh("todo-destroy");
        shell.window = Some(window::Id::unique());
        shell.todo.set_project(Some(1));
        press(&mut shell, &shell::todo_id());
        let _ = shell.sync_native();
        assert!(shell.todo.native.is_live(), "visible todo builds the view");
        press(&mut shell, &shell::info_id());
        let _ = shell.sync_native();
        assert!(
            !shell.todo.native.is_live(),
            "switching away must destroy the native view"
        );
    }

    #[test]
    fn the_poll_tick_is_subscribed_only_while_the_panel_is_visible() {
        let mut shell = fresh("todo-poll");
        assert!(!todo_poll_active(&shell), "todo is not selected by default");
        press(&mut shell, &shell::todo_id());
        assert!(todo_poll_active(&shell));
        press(&mut shell, &shell::info_id());
        assert!(!todo_poll_active(&shell));
        shell.closing = true;
        assert!(!todo_poll_active(&shell));
    }

    #[test]
    fn todo_message_variants_route_into_the_panel() {
        let mut shell = fresh("todo-route");
        press(&mut shell, &shell::todo_id());
        assert!(!shell.pane_collapsed(&shell::todo_id()));
        let _ = Product::update(
            &mut shell,
            HostMessage::Product(Msg::Todo(dozer_panel_todo::Message::ToggleListCollapse)),
        );
        assert!(
            shell.pane_collapsed(&shell::todo_id()),
            "ToggleListCollapse must flip the shell PaneStore"
        );
    }

    #[test]
    fn group_chat_poll_is_off_by_default_and_when_closing() {
        let mut shell = ProductShell::new();
        assert!(!group_chat_poll_active(&shell));
        shell.closing = true;
        assert!(!group_chat_poll_active(&shell));
    }
}
