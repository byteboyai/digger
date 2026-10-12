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
use bytehost_desktop::layout::{LegacyPanelAdapter, atomic_save_layout, migrate_legacy_file};
use bytehost_desktop::{HostMessage, Product, SurfaceEvent, route_lifecycle};
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
    layout: Option<Layout>,
    layout_path: PathBuf,
    active: ActivePanel,
    /// 产品未接业务服务时的启动说明,显示在信息面板。
    boot_note: String,
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
        let layout = load_layout(&layout_path, &catalog);

        Self {
            window: None,
            size: default_window_size(),
            catalog,
            layout,
            layout_path,
            active: ActivePanel::Info,
            boot_note: "尚未打开 Project;业务服务未接通(应用服务在 Task 12/13 接入)。".into(),
        }
    }

    /// 当前应显示的布局(缺省=None 时用内存默认;产品一期只保存不渲染分栏).
    fn current_layout(&self) -> &Layout {
        self.layout
            .as_ref()
            .expect("构造时必已装入布局(默认或磁盘)")
    }

    /// 落盘当前布局(退出/布局变化时调用)。失败不 panic,记入 boot_note。
    fn save_layout(&mut self) {
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

/// 读布局:优先迁移磁盘文件;无可用历史 → 默认布局。
fn load_layout(path: &std::path::Path, catalog: &PanelCatalog) -> Option<Layout> {
    if let Ok(Some(outcome)) = migrate_legacy_file(path, &DiggerLayoutAdapter) {
        return Some(outcome.layout);
    }
    // 全新安装或以稳定 schema 直接 load。
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(layout) = Layout::load(&text, catalog)
    {
        return Some(layout);
    }
    default_layout()
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
        if let HostMessage::Surface(event) = msg {
            // 宿主事件交回默认处理(一期没有 Surface,仅保持接缝)。
            return self.update_surface(event);
        }
        let HostMessage::Product(m) = msg else {
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
                Task::none()
            }
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

    fn on_activated(&mut self, _slot: bytehost_desktop::SurfaceSlot) {}
    fn on_closed(&mut self, _slot: bytehost_desktop::SurfaceSlot) {}
}

impl ProductShell {
    fn update_surface(&mut self, event: SurfaceEvent) -> Task<Msg> {
        // 一期没有 Surface 承载;保持接缝,避免死代码告警。
        let _ = route_lifecycle(self, event);
        Task::none()
    }

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
        use iced::widget::{column, text};
        use iced::{Alignment, Length};

        let tokens = color::current();
        column![
            text("应用区占位")
                .size(font::subtitle())
                .color(tokens.cream),
            text("应用安装 / 运行 / 管理服务尚未接通(Task 12/13)。").color(tokens.gold),
            text("此处不会伪造任何安装或运行能力。").color(tokens.dim),
        ]
        .spacing(8)
        .align_x(Alignment::Start)
        .width(Length::Fill)
        .into()
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
    let t = bytehost_desktop::panel::oldest_window().then(move |id| match id {
        Some(id) => bytehost_desktop::panel::window_size(id)
            .map(move |s| Msg::WindowReady(id, s.unwrap_or(size))),
        None => Task::none(),
    });
    (shell, t)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
