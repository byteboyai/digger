//! iced 自绘的上下文菜单浮层(S5b-1 Task 9)。
//!
//! 菜单原语(项、分隔线、外框)、锚点钳制 `MenuAnchor`、按行渲染的 `view_rows` 与带遮罩的外框
//! `overlay_shell` 已迁到 `byteui::interaction::menu`(X1,与 Dozer 共用;原先这里抄过一份 Dozer 的
//! `chrome/menu.rs`)。外框度量来自 `byteui::theme::region::context_menu()`(不透明背景)。
//!
//! 本文件只剩 Digger 自己的两样:
//! - [`MenuOverlay<M>`]:条目类型是文件库的 `dozer_panel_files::menu::MenuItem`,byteui 不能依赖
//!   Dozer 库 crate,所以这一层留在这里;
//! - 把产品 `Msg` 固定下来的薄包装 [`OverlayRow`] / [`view_rows`],调用点不用写泛型参数。

use iced_widget::core::{Element, Length};

use crate::product::Msg;

pub use byteui::interaction::menu::MenuAnchor;

type El<'a> = Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer>;

/// 与具体面板无关的一行(Agent 选择器等复用同一套外框与行样式),消息类型固定为产品 `Msg`。
pub type OverlayRow = byteui::interaction::menu::OverlayRow<Msg>;

/// 一条菜单(条目已规整)。空 `items` 表示不弹——产品据此不出浮层。
#[derive(Debug, Clone)]
pub struct MenuOverlay<M> {
    pub anchor: MenuAnchor,
    pub items: Vec<dozer_panel_files::menu::MenuItem<M>>,
}

impl<M> MenuOverlay<M> {
    /// 空条目列表不构成一次弹出(库的正常输出;产品侧据此跳过渲染)。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// 菜单外形尺寸:宽恒为 `menu_item_width`(与 Dozer 常宽菜单一致),高按条目数估算(分隔线只占一条),
/// 供 [`MenuAnchor::clamped`] 用。
#[allow(dead_code)] // 保留给需要预先量尺寸的调用方;菜单视图走 byteui 的 `rows_size`。
pub fn menu_size<M>(items: &[dozer_panel_files::menu::MenuItem<M>]) -> (f32, f32) {
    let rows: Vec<byteui::interaction::menu::OverlayRow<()>> = items
        .iter()
        .map(|item| match item {
            dozer_panel_files::menu::MenuItem::Entry { .. } => {
                byteui::interaction::menu::OverlayRow::Entry {
                    icon: None,
                    label: String::new(),
                    enabled: true,
                    msg: Box::new(()),
                    tint: None,
                }
            }
            dozer_panel_files::menu::MenuItem::Separator => {
                byteui::interaction::menu::OverlayRow::Separator
            }
        })
        .collect();
    byteui::interaction::menu::rows_size(&rows)
}

/// 渲染菜单浮层:整窗铺满,右下对齐到锚点(钳制后)。`map` 把库消息转成产品消息。
pub fn view<'a, M: Clone + 'a>(
    overlay: &'a MenuOverlay<M>,
    window: (f32, f32),
    map: impl Fn(M) -> Msg + 'a,
    on_dismiss: Msg,
) -> El<'a> {
    if overlay.is_empty() {
        // 空条目表不构成一次弹出:渲染成一个不拦事件的空层。
        return iced_widget::container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let rows = overlay
        .items
        .iter()
        .cloned()
        .map(|item| match item {
            dozer_panel_files::menu::MenuItem::Entry {
                icon,
                label,
                enabled,
                msg,
            } => OverlayRow::Entry {
                icon,
                label,
                enabled,
                msg: Box::new(map(msg)),
                tint: None,
            },
            dozer_panel_files::menu::MenuItem::Separator => OverlayRow::Separator,
        })
        .collect();
    view_rows(rows, overlay.anchor, window, on_dismiss)
}

/// 按已规整的行渲染浮层。空行表不弹(返回一层的空容器)。委托给 byteui 的 `view_rows`。
pub fn view_rows<'a>(
    rows: Vec<OverlayRow>,
    anchor: MenuAnchor,
    window: (f32, f32),
    on_dismiss: Msg,
) -> El<'a> {
    byteui::interaction::menu::view_rows(rows, anchor, window, on_dismiss)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_menu_background_is_fully_opaque() {
        // wgpu 在线性色彩空间里混合:alpha 0.98 让底下的内容留 2% 线性亮度,换算成屏幕
        // 上的 sRGB 观感约 15%,亮字和卡片边框就会"淡淡透出来"(溢出菜单盖在卡片列表上
        // 时实测如此)。菜单盖在别的内容上,背景必须不透明。
        let bg = byteui::theme::region::context_menu()
            .background
            .expect("菜单有背景色");
        assert_eq!(bg.a, 1.0, "菜单背景 alpha 必须是 1.0,实际 {}", bg.a);
    }

    use super::*;
    use dozer_panel_files::menu::MenuItem;

    fn item(label: &str) -> MenuItem<Msg> {
        MenuItem::Entry {
            icon: None,
            label: label.into(),
            enabled: true,
            msg: Msg::ZoomToggle,
        }
    }

    /// 空条目列表不弹:产品侧据此跳过渲染。
    #[test]
    fn an_empty_item_list_does_not_open_a_popup() {
        let overlay: MenuOverlay<Msg> = MenuOverlay {
            anchor: MenuAnchor { x: 10.0, y: 10.0 },
            items: Vec::new(),
        };
        assert!(overlay.is_empty());
    }

    /// 右下角放不下时向窗口内翻,永不越界。
    #[test]
    fn the_anchor_is_clamped_inside_the_window() {
        let window = (800.0, 600.0);
        let size = (220.0, 300.0);
        let a = MenuAnchor { x: 790.0, y: 590.0 }.clamped(size, window);
        assert_eq!(a, MenuAnchor { x: 580.0, y: 300.0 });
        let b = MenuAnchor { x: -5.0, y: -5.0 }.clamped(size, window);
        assert_eq!(b, MenuAnchor { x: 0.0, y: 0.0 });
    }

    /// 菜单高度按条目数增长,分隔线只占一条。
    #[test]
    fn menu_height_grows_with_entries() {
        let two = menu_size(&[item("a"), item("b")]).1;
        let three = menu_size(&[item("a"), item("b"), item("c")]).1;
        assert!(three > two);
        let sep = menu_size(&[MenuItem::<Msg>::Separator]).1;
        assert!(sep < two);
    }
}
