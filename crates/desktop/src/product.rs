//! Digger 产品壳:在共享 bytehost 宿主上启动的最小产品组合根(Task 11)。
//!
//! 范围(计划 Task 11):
//! - 以**无 Project** 状态启动,能注册产品面板、保存布局、退出;
//! - 注册一个**产品信息/状态面板**(纯 iced 原生,无 Surface)与一个**应用区占位**;
//! - 占位明确标注"服务未接通",**不伪造**任何写入能力;
//! - 拥有自己的数据根、主题、布局落盘与退出,**不复制** Dozer 的 `App`/`Workspace`。
//!
//! 后续任务(Task 12/13)才接应用服务与 app-host;这里只搭壳与接缝。

use std::path::PathBuf;

use bytehost_core::registry::PanelCatalog;
use bytehost_core::{Layout, PanelId, PanelMeta, Rail};
use bytehost_desktop::layout::{
    LegacyPanelAdapter, MigrateError, atomic_save_layout, migrate_legacy_file,
    reconcile_with_catalog,
};
use bytehost_desktop::{HostMessage, Product, route_lifecycle};
use byteui::theme::{color, font};

use iced::{Element, Size, Subscription, Task, window};

use crate::paths;

/// 产品信息/状态面板的稳定 id。
fn info_panel_id() -> PanelId {
    PanelId::new("digger.info").expect("合法 panel id")
}

/// 应用区面板的稳定 id(Dozer 用 `app:<id>`;Digger 一期先注册为占位面板)。
fn apps_panel_id() -> PanelId {
    PanelId::new("digger.apps").expect("合法 panel id")
}

/// 产品消息。**不进入**宿主 enum(宿主只认 `HostMessage<M>`)。
#[derive(Debug, Clone)]
pub enum Msg {
    WindowReady(window::Id, Size),
    SelectPanel(ActivePanel),
    /// 「应用」面板的消息(Task 14)。
    Apps(crate::apps::Message),
    CloseRequested,
    Exit,
}

/// 左栏当前显示的产品面板(一期只有两个占位面板,不接业务)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePanel {
    #[default]
    Info,
    Apps,
}

/// Digger 产品壳状态。刻意保持最小:不持有任何 Domain/持久化状态,
/// 只有 UI 交互态 + 布局 + 主题。
pub struct ProductShell {
    window: Option<window::Id>,
    size: Size,
    catalog: PanelCatalog,
    layout: Layout,
    /// 非 `None` 表示磁盘上是**不支持的高版本**布局:内存默认布局仅用于显示,**禁止回写**该文件。
    layout_save_blocked: Option<String>,
    layout_path: PathBuf,
    active: ActivePanel,
    /// 「应用」面板的组合根(状态机 + 执行器 + 内嵌应用服务)。
    apps: crate::apps::AppsState,
    /// 产品未接业务服务时的启动说明,显示在信息面板。
    boot_note: String,
    /// 测试用:记录收到的生命周期回调,验证 Surface 事件恰好路由一次(复盘发现 6)。
    #[cfg(test)]
    surface_callbacks: Vec<bytehost_desktop::SurfaceSlot>,
}

impl ProductShell {
    /// 构造:注册产品面板目录、读布局(缺省用默认布局),不打开任何 Project。
    pub fn new() -> Self {
        let mut catalog = PanelCatalog::new();
        catalog
            .register(PanelMeta {
                id: info_panel_id(),
                title: "Digger".into(),
                surfaces: vec![],
            })
            .expect("首次注册 digger.info");
        catalog
            .register(PanelMeta {
                id: apps_panel_id(),
                title: "应用".into(),
                surfaces: vec![],
            })
            .expect("首次注册 digger.apps");

        let layout_path = paths::layout_path();
        let (layout, layout_save_blocked) = load_layout(&layout_path, &catalog).layout();

        Self {
            window: None,
            size: default_window_size(),
            catalog,
            layout,
            layout_save_blocked,
            layout_path,
            active: ActivePanel::Info,
            apps: crate::apps::AppsState::new(),
            boot_note: "尚未打开 Project;业务服务未接通(应用服务在 Task 12/13 接入)。".into(),
            #[cfg(test)]
            surface_callbacks: Vec::new(),
        }
    }

    /// 当前应显示的布局(内存默认或磁盘)。
    fn current_layout(&self) -> &Layout {
        &self.layout
    }

    /// 落盘当前布局(退出/布局变化时调用)。失败不 panic,记入 boot_note。
    /// 磁盘上是**不支持的高版本**布局时**拒绝回写**(复盘发现 4:绝不覆盖原文件)。
    fn save_layout(&mut self) {
        if let Some(note) = &self.layout_save_blocked {
            self.boot_note = format!("布局未保存:{note}");
            return;
        }
        let layout = self.current_layout().clone();
        if let Err(e) = atomic_save_layout(&self.layout_path, &layout) {
            self.boot_note = format!("布局保存失败:{e}");
        }
    }
}

impl Default for ProductShell {
    fn default() -> Self {
        Self::new()
    }
}

/// 产品侧历史布局适配器:Digger 一期没有历史布局格式,只为满足宿主迁移流程。
/// `rails_from_legacy` 返回空(全新安装),`map_token` 只认自己的两个面板 id。
struct DiggerLayoutAdapter;

impl LegacyPanelAdapter for DiggerLayoutAdapter {
    fn rails_from_legacy(&self, legacy: &serde_json::Value) -> Vec<Vec<String>> {
        // 一期:Digger 的 layout.json 已是稳定 schema,历史字段只有 rails 的 id 列表。
        let Some(obj) = legacy.as_object() else {
            return Vec::new();
        };
        let Some(rails) = obj.get("rails").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        rails
            .iter()
            .map(|rail| {
                rail.get("items")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    fn map_token(&self, token: &str) -> Option<PanelId> {
        // 稳定 id 直接透传;不认识的返回 None(保留进 retained_unknown,不阻塞开窗)。
        PanelId::new(token).ok()
    }

    fn default_rail_width(&self) -> f32 {
        280.0
    }
}

/// 布局加载结果。高版本文件必须**拒绝覆盖**(复盘发现 4):内存里用默认布局显示,但不允许回写原文件。
enum LoadedLayout {
    /// 可正常读写的布局(全新安装 / 稳定格式 / 历史格式迁移成功)。
    Writable(Layout),
    /// 磁盘文件 schema 版本高于宿主支持:内存用默认布局显示,**禁止保存**回该文件。
    Unsupported { layout: Layout, note: String },
}

impl LoadedLayout {
    fn layout(self) -> (Layout, Option<String>) {
        match self {
            Self::Writable(layout) => (layout, None),
            Self::Unsupported { layout, note } => (layout, Some(note)),
        }
    }
}

/// 读布局:
/// 1. 稳定 schema 文件(带 `schema_version`)**直接**走 [`Layout::load`](盘点发现 5:不再送进历史迁移,
///    以免宽度被重建为默认、`retained_unknown` 被清空);
/// 2. 只有**真正历史格式**(无 `schema_version`)才走 `migrate_legacy_file`;
/// 3. 迁移或稳定 load 撞到**更高 schema 版本** → [`LoadedLayout::Unsupported`](盘点发现 4:拒绝覆盖原文件);
/// 4. 文件缺失/损坏 → 默认布局(可写)。
fn load_layout(path: &std::path::Path, catalog: &PanelCatalog) -> LoadedLayout {
    let Ok(text) = std::fs::read_to_string(path) else {
        // 文件不存在:全新安装,默认布局可写。
        return LoadedLayout::Writable(default_layout_or_empty());
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        // 损坏文件:当作没有可用布局,给默认布局(可覆盖)。
        return LoadedLayout::Writable(default_layout_or_empty());
    };
    if value.get("schema_version").is_some() {
        // 稳定格式:直接 load(宽度钳制 + 未知摘取 + 版本闸门都在 `Layout::load` 里)。
        return match Layout::load(&text, catalog) {
            Ok(layout) => LoadedLayout::Writable(layout),
            Err(bytehost_core::LayoutError::UnknownSchemaVersion { found, supported }) => {
                LoadedLayout::Unsupported {
                    layout: default_layout_or_empty(),
                    note: format!(
                        "布局 schema 版本 {found} 高于本宿主支持的 {supported},已用默认布局显示且**不覆盖原文件**"
                    ),
                }
            }
            Err(_) => LoadedLayout::Writable(default_layout_or_empty()),
        };
    }
    // 历史格式:经产品适配器迁移。
    match migrate_legacy_file(path, &DiggerLayoutAdapter) {
        Ok(Some(outcome)) => match reconcile_with_catalog(outcome.layout, catalog) {
            Ok(layout) => LoadedLayout::Writable(layout),
            Err(_) => LoadedLayout::Writable(default_layout_or_empty()),
        },
        Err(MigrateError::UnsupportedNewerVersion { found, supported }) => {
            LoadedLayout::Unsupported {
                layout: default_layout_or_empty(),
                note: format!(
                    "历史布局 schema 版本 {found} 高于本宿主支持的 {supported},已用默认布局显示且**不覆盖原文件**"
                ),
            }
        }
        // 无历史内容 / 非对象:默认布局(可写)。
        Ok(None) | Err(MigrateError::NotAnObject) => {
            LoadedLayout::Writable(default_layout_or_empty())
        }
    }
}

/// 默认布局;`Layout::from_default` 理论上不会失败,兜底给空布局。
fn default_layout_or_empty() -> Layout {
    default_layout().unwrap_or(Layout {
        schema_version: bytehost_core::LAYOUT_SCHEMA_VERSION,
        rails: Vec::new(),
        retained_unknown: Vec::new(),
    })
}

/// 默认布局:左栏放信息/应用两个面板,右栏空(空栏合法)。
fn default_layout() -> Option<Layout> {
    let left = vec![info_panel_id()];
    let rails = vec![
        Rail::new(left, 280.0),
        Rail::new(vec![apps_panel_id()], 280.0),
    ];
    Layout::from_default(rails).ok()
}

fn default_window_size() -> Size {
    let (w, h) = byteui::theme::geometry::initial_window_size();
    Size::new(w, h)
}

/// 入口用:初始窗口尺寸(默认布局的同等口径)。
pub fn default_window_size_for_boot() -> Size {
    default_window_size()
}

impl Product<Msg> for ProductShell {
    fn update(&mut self, msg: HostMessage<Msg>) -> Task<Msg> {
        let HostMessage::Product(m) = msg else {
            // Surface 事件**不在这里**再分发给生命周期:那由外层入口 [`update_host`] 单次调用
            // `route_lifecycle` 完成(复盘发现 6:在此调用会让 `route_lifecycle` → `update` 形成无限递归)。
            // 这里只做本产品对宿主事件的处理(一期无 Surface,仅消费不重投)。
            return Task::none();
        };
        match m {
            Msg::WindowReady(id, size) => {
                self.window = Some(id);
                self.size = size;
                Task::none()
            }
            Msg::SelectPanel(panel) => {
                self.active = panel;
                // 首次进入「应用」面板:触发一次探测 + 拉列表(Task 14)。
                if panel == ActivePanel::Apps {
                    return self
                        .apps
                        .update(crate::apps::Message::Opened)
                        .map(Msg::Apps);
                }
                Task::none()
            }
            Msg::Apps(m) => self.apps.update(m).map(Msg::Apps),
            Msg::CloseRequested | Msg::Exit => {
                self.save_layout();
                iced::exit()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::Length;
        use iced::widget::{button, column, container, row, text};

        let tokens = color::current();
        let header = text("Digger").size(font::title()).color(tokens.cream);

        let body = match self.active {
            ActivePanel::Info => self.info_view(),
            ActivePanel::Apps => self.apps_view(),
        };

        let tabs = row![
            button("信息").on_press(Msg::SelectPanel(ActivePanel::Info)),
            button("应用").on_press(Msg::SelectPanel(ActivePanel::Apps)),
        ]
        .spacing(12);

        let content = column![header, tabs, body]
            .spacing(16)
            .padding(20)
            .width(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t| container::Style {
                background: Some(tokens.bg.into()),
                text_color: Some(tokens.cream),
                ..Default::default()
            })
            .into()
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
    match msg {
        HostMessage::Surface(event) => route_lifecycle(shell, event),
        HostMessage::Product(m) => Product::update(shell, HostMessage::Product(m)),
    }
}

impl ProductShell {
    fn info_view(&self) -> Element<'_, Msg> {
        use iced::Length;
        use iced::widget::{button, column, text};

        let tokens = color::current();
        let dim = tokens.dim;
        column![
            text(format!(
                "面板目录已注册 {} 个(产品组合根)",
                self.catalog.len()
            ))
            .color(tokens.body),
            text(format!("数据根:{}", paths::config_dir().display())).color(dim),
            text(format!("应用宿主根:{}", paths::bytehost_root().display())).color(dim),
            text("当前 Project:无").color(tokens.body),
            text(&self.boot_note).color(tokens.gold),
            button("退出 Digger").on_press(Msg::Exit),
        ]
        .spacing(8)
        .width(Length::Fill)
        .into()
    }

    fn apps_view(&self) -> Element<'_, Msg> {
        self.apps.view().map(Msg::Apps)
    }
}

/// 订阅:窗口关闭请求。
pub fn subscription(_shell: &ProductShell) -> Subscription<Msg> {
    iced::event::listen_with(|evt, _status, _id| match evt {
        iced::Event::Window(window::Event::CloseRequested) => Some(Msg::CloseRequested),
        _ => None,
    })
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

    let shell = ProductShell::new();
    let size = shell.size;
    // 应用面板后台结果回流:一次性流出接收端,持续到进程结束(与窗口尺寸任务并行)。
    use iced::futures::StreamExt;
    let reply_task = match crate::apps::take_reply_receiver(&shell.apps.reply_receiver()) {
        Some(rx) => iced::Task::stream(crate::apps::reply_stream(rx).map(Msg::Apps)),
        None => Task::none(),
    };
    let size_task = bytehost_desktop::panel::oldest_window().then(move |id| match id {
        Some(id) => bytehost_desktop::panel::window_size(id)
            .map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    (shell, Task::batch([size_task, reply_task]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_desktop::SurfaceEvent;

    #[test]
    fn starts_without_project() {
        let shell = ProductShell::new();
        assert!(shell.boot_note.contains("尚未打开 Project"));
    }

    #[test]
    fn registers_product_panels() {
        let shell = ProductShell::new();
        assert!(shell.catalog.contains(&info_panel_id()));
        assert!(shell.catalog.contains(&apps_panel_id()));
        assert_eq!(shell.catalog.len(), 2);
    }

    #[test]
    fn has_default_layout() {
        let shell = ProductShell::new();
        let layout = shell.current_layout();
        assert_eq!(layout.visible_panel_count(), 2);
    }

    #[test]
    fn layout_round_trips_through_adapter() {
        // 空历史 → 空栏;稳定 id 透传。
        let adapter = DiggerLayoutAdapter;
        let legacy = serde_json::json!({
            "rails": [
                { "items": ["digger.info"] },
                { "items": [] }
            ]
        });
        let rails = adapter.rails_from_legacy(&legacy);
        assert_eq!(rails.len(), 2);
        assert_eq!(rails[0], vec!["digger.info".to_string()]);
        assert!(rails[1].is_empty());
        assert_eq!(adapter.map_token("digger.apps"), Some(apps_panel_id()));
        assert_eq!(adapter.map_token("not a valid id"), None);
    }

    fn test_catalog() -> PanelCatalog {
        let mut catalog = PanelCatalog::new();
        catalog
            .register(PanelMeta {
                id: info_panel_id(),
                title: "Digger".into(),
                surfaces: vec![],
            })
            .unwrap();
        catalog
            .register(PanelMeta {
                id: apps_panel_id(),
                title: "应用".into(),
                surfaces: vec![],
            })
            .unwrap();
        catalog
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

    /// 复盘发现 4:高版本布局加载失败后退出**不得覆盖**原文件。
    #[test]
    fn a_newer_schema_layout_is_never_overwritten() {
        let dir = test_temp_dir("newer-schema");
        let path = dir.join("layout.json");
        let original = r#"{ "schema_version": 999, "rails": [] }"#;
        std::fs::write(&path, original).unwrap();

        let loaded = load_layout(&path, &test_catalog());
        let (layout, blocked) = loaded.layout();
        assert!(blocked.is_some(), "高版本应进入不可写状态:{blocked:?}");
        // 内存显示默认布局(两个面板),但禁止保存。
        assert_eq!(layout.visible_panel_count(), 2);
        // 模拟退出:按 save_layout 的门禁,blocked 时不落盘。
        if blocked.is_none() {
            atomic_save_layout(&path, &layout).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "原高版本文件必须逐字节保持"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 复盘发现 5:稳定 schema 文件不走历史迁移,宽度与保留项原样往返。
    #[test]
    fn a_stable_layout_keeps_its_width_and_retained_items() {
        let dir = test_temp_dir("stable-width");
        let path = dir.join("layout.json");
        let stable = serde_json::json!({
            "schema_version": 1,
            "rails": [
                { "items": ["digger.info"], "width": 420.0 },
                { "items": [], "width": 300.0 }
            ],
            "retained_unknown": ["digger.removed"]
        });
        std::fs::write(&path, stable.to_string()).unwrap();

        let (layout, blocked) = load_layout(&path, &test_catalog()).layout();
        assert!(blocked.is_none(), "稳定格式应可写:{blocked:?}");
        assert_eq!(layout.rails[0].width, 420.0, "宽度不得被重建为默认");
        assert_eq!(layout.rails[1].width, 300.0);
        assert_eq!(
            layout.retained_unknown,
            vec![PanelId::new("digger.removed").unwrap()],
            "保留项不得被清空"
        );

        // 直接走稳定 load 的往返:再存再读仍是 420 + 保留项。
        atomic_save_layout(&path, &layout).unwrap();
        let (again, blocked) = load_layout(&path, &test_catalog()).layout();
        assert!(blocked.is_none());
        assert_eq!(again.rails[0].width, 420.0);
        assert_eq!(again.retained_unknown.len(), 1);
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
}
