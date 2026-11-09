//! Panel routing, geometry and display helpers moved out of the composition root.
//!
//! These are pure functions (no state ownership); `ProductShell` remains the single state owner and
//! keeps the assembly order in `boot`.

use super::*;

pub(crate) fn preview_display(
    tab_bar_px: f32,
    project_id: i64,
    epoch: u64,
) -> crate::adapters::preview_display::PreviewDisplay<'static> {
    crate::adapters::preview_display::PreviewDisplay {
        locale: crate::settings::language::code(),
        tab_bar_px,
        list_on_left: true,
        project_id,
        epoch,
        hover: None,
    }
}

/// 会弹右键菜单的库消息:库从 `last_right_click` 取锚点。
pub(crate) fn opens_a_context_menu(message: &dozer_panel_files::Message) -> bool {
    use dozer_panel_files::Message as F;
    matches!(
        message,
        F::ContextMenuOpen { .. } | F::TabContextMenuOpen { .. }
    )
}

/// Files 库的右键菜单浮层何时画:Files 面板可见,或 Project 面板可见(Project 面板里的预览页签右键
/// 也走 Files 库的页签菜单)。只认 Files 可见时,Project 里右键会"页面被藏了、菜单没画"。
pub(crate) fn files_menu_is_rendered(files_visible: bool, project_visible: bool) -> bool {
    files_visible || project_visible
}

/// 页签数没变时悬停仍然有效;增减(含从无到有)后按下标寻址的悬停必须作废。
pub(crate) fn hover_survives_tab_change(before: Option<usize>, after: Option<usize>) -> bool {
    before == after
}

/// 预览页签条右侧三颗模式按钮的估算占宽(按钮边长 + 与页签组之间的间距,Dozer 同款)。
pub(crate) const PREVIEW_CHROME_RESERVE_PX: f32 = 32.0 * 3.0 + 4.0 * 3.0;

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 设置页里的一行普通文字(产品自己的提示 / 状态行)。
pub(crate) fn text_line<'a>(label: impl Into<String>, tint: iced::Color) -> Element<'a, Msg> {
    iced::widget::text(label.into())
        .size(byteui::theme::font::label())
        .color(tint)
        .into()
}

/// 设置窗口的导航映射(`&'static` 函数指针,可安全借进返回的 `Element`)。
pub(crate) fn settings_select(id: &bytehost_settings::PageId) -> Msg {
    Msg::Settings(crate::settings::Message::Select(id.clone()))
}

pub(crate) fn settings_theme_selected(scheme: byteui::theme::color::ColorScheme) -> Msg {
    Msg::Settings(crate::settings::Message::ThemeSelected(scheme))
}

pub(crate) fn settings_apps_install(message: bytehost_panel::install::Message) -> Msg {
    Msg::Apps(crate::apps::Message::Install(message))
}

/// 应用行里的产品附加说明(运行时问题)。
pub(crate) fn settings_app_notes<'a>(
    app: &bytehost_apps::proto::AppSummary,
) -> Vec<Element<'a, Msg>> {
    match &app.issue {
        Some(issue) => {
            let (title, detail) = bytehost_panel::host::issue_texts(issue);
            vec![text_line(format!("{title}:{detail}"), color::current().red)]
        }
        None => Vec::new(),
    }
}

/// 应用行里的产品前置动作:非过渡态应用可以「打开」。
pub(crate) fn settings_app_leading<'a>(
    app: &bytehost_apps::proto::AppSummary,
) -> Vec<Element<'a, Msg>> {
    if app.observed.is_transient() {
        return Vec::new();
    }
    vec![
        iced::widget::button(
            iced::widget::text("打开")
                .size(byteui::theme::font::body())
                .color(color::current().gold),
        )
        .padding([4, 12])
        .on_press(Msg::Apps(crate::apps::Message::OpenApp(
            app.id.as_str().to_owned(),
        )))
        .into(),
    ]
}

pub(crate) fn logical(r: Rect) -> LogicalRect {
    LogicalRect {
        x: r.x as f64,
        y: r.y as f64,
        width: r.w as f64,
        height: r.h as f64,
    }
}

/// 项目布局文件:`<root>/shell-projects/<id>.json`。生产里 `root` 是配置目录(`<config>/digger/shell-projects/`),
/// 测试里跟着全局 `shell.json` 走,免得测试互相覆盖、也不碰真实配置目录。
pub(crate) fn project_shell_path_of(root: &Path, id: i64) -> PathBuf {
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
pub(crate) fn preview_webview_rect(slot: Rect) -> Rect {
    let inset = crate::adapters::preview_display::PreviewDisplay::content_padding();
    let top = byteui::theme::geometry::preview_chrome_top_px() + inset;
    Rect::new(
        slot.x + inset,
        slot.y + top,
        (slot.w - inset * 2.0).max(1.0),
        (slot.h - top).max(1.0),
    )
}

pub(crate) fn review_native_rect(
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

pub(crate) fn group_chat_native_rect(shell: &ShellModel, content: Rect, collapsed: bool) -> Rect {
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
pub(crate) fn below_header(r: Rect, top: f32) -> Rect {
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
pub(crate) fn usage_native_rect(
    layout: UsageLayout,
    content: Rect,
    pair: Option<PairRects>,
) -> Rect {
    match (layout, pair) {
        (UsageLayout::Paired, Some(p)) => p.second,
        _ => content,
    }
}

/// 待办 webview 的矩形:配对第二块(内容区)扣掉库视图为 iced 预留的顶部行
/// 与底部新增输入。列表收起时内容区独占整块;整个面板不可见时由调用方传
/// `visible=false` 销毁。
pub(crate) fn todo_native_rect(content: Rect, pair: Option<PairRects>, add_input_h: f32) -> Rect {
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
pub(crate) fn collect_preview_desired(
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
        let maximized = crate::panel_surface::is_maximized(shell, side);
        let body = crate::panel_surface::content_rect(rects.content(side).unwrap(), maximized);
        let rect = logical(crate::panel_surface::native_rect(
            preview_webview_rect(slot),
            body,
            crate::panel_surface::native_radius(maximized),
        ));
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
pub(crate) fn hide_for_overlay(
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

pub(crate) fn top_bar_slots<'a>(
    projects: &'a crate::projects::ProjectsState,
) -> TopBarSlots<'a, Msg> {
    let tokens = color::current();
    let dim = tokens.dim;
    let title = row![
        icons::view(IconKind::Home, icon_size::home(), dim),
        text("Digger").size(font::body()).color(dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let tabs = projects.tabs_view().map(Msg::Projects);
    let settings = iced::widget::button(icons::view(IconKind::Settings, icon_size::row(), dim))
        .padding(4)
        .on_press(Msg::Settings(crate::settings::Message::Open(None)))
        .style(|_t: &iced::Theme, status: iced::widget::button::Status| {
            let colors = color::current();
            iced::widget::button::Style {
                background: match status {
                    iced::widget::button::Status::Hovered
                    | iced::widget::button::Status::Pressed => Some(
                        iced::Color {
                            a: 0.15,
                            ..colors.gold
                        }
                        .into(),
                    ),
                    _ => None,
                },
                ..Default::default()
            }
        });
    TopBarSlots {
        title: title.into(),
        tabs,
        right: settings.into(),
    }
}

/// 拉起/连接 `diggerd`(boot 与「重试」共用同一条路径)。

pub(crate) fn native_preview_task(
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
pub(crate) fn preview_project(
    current: Option<&dozer_model::ProjectInfo>,
    closing: bool,
) -> Option<dozer_model::ProjectInfo> {
    if closing {
        return None;
    }
    current.cloned()
}

/// 把当前窗口的分栏状态交给只读的展示适配器(`PanelDisplay` 拿不到 `&ProductShell`)。
pub(crate) fn publish_panes(panes: &bytehost_desktop::PaneStore) {
    CURRENT_PANES.with(|p| *p.borrow_mut() = Some(panes.clone()));
}

/// 待办轮询只在面板真正可见且未在退出时订阅。
pub(crate) fn group_chat_poll_active(shell: &ProductShell) -> bool {
    !shell.closing
        && shell
            .group_chat
            .poll_wanted(shell.panel_visible(&shell::group_chat_id()))
}

pub(crate) fn todo_poll_active(shell: &ProductShell) -> bool {
    !shell.closing && shell.panel_visible(&shell::todo_id())
}

/// 每个已注册面板此刻的可见性(下标与 [`shell::all_panels`] 对齐)。
pub(crate) fn preview_panel_visible(flags: &[bool]) -> bool {
    [shell::files_id(), shell::project_id()]
        .iter()
        .filter_map(|id| shell::all_panels().iter().position(|p| p == id))
        .any(|i| flags.get(i).copied().unwrap_or(false))
}

pub(crate) fn visible_panels(shell: &ProductShell) -> Vec<bool> {
    shell::all_panels()
        .iter()
        .map(|p| shell.panel_visible(p))
        .collect()
}
