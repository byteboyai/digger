//! iced 自绘的上下文菜单浮层(S5b-1 Task 9)。
//!
//! byteui 的 `interaction::context_menu` 只负责"右键触发",没有菜单外框与
//! 菜单项渲染;那部分在 Dozer 的 `chrome/menu.rs`。Digger 与 Dozer 共用同一套
//! 视觉:外壳用 `context_menu` 区域(半透明 PANEL 底、1px 描边、圆角 9、内边距
//! 8、项间距 1),菜单项为"图标 + 文字"、hover 切 `tab_hover` 底并左右内缩
//! `menu_hover_inset`。度量全部取 `byteui::theme::geometry`,与 Dozer 同源。

use iced_widget::core::{Border, Color, Element, Length, Padding, Shadow, Vector};
use iced_widget::{button, column, container, row, text};

use crate::product::Msg;

type El<'a> = Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer>;

/// 弹出位置:左上角锚在 `(x, y)`,并钳制在窗口 `(w, h)` 内。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuAnchor {
    pub x: f32,
    pub y: f32,
}

impl MenuAnchor {
    /// 按菜单实际尺寸把锚点钳进窗口:右侧/下侧放不下时向左/上翻。
    pub fn clamped(self, size: (f32, f32), window: (f32, f32)) -> Self {
        let (menu_w, menu_h) = size;
        let x = self.x.min((window.0 - menu_w).max(0.0)).max(0.0);
        let y = self.y.min((window.1 - menu_h).max(0.0)).max(0.0);
        Self { x, y }
    }
}

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

/// 菜单外形尺寸:宽恒为 `menu_item_width`(与 Dozer 常宽菜单一致),高按条目数估算
/// (项高 + 上下内边距 + 项间距),供 [`MenuAnchor::clamped`] 用。分隔线只占一条。
#[allow(dead_code)] // 保留给需要预先量尺寸的调用方;菜单视图走 `rows_size`。
pub fn menu_size<M>(items: &[dozer_panel_files::menu::MenuItem<M>]) -> (f32, f32) {
    let region = region();
    let row_h = row_height();
    let gap = byteui::theme::geometry::menu_gap();
    let width =
        byteui::theme::geometry::menu_item_width() + region.padding.left + region.padding.right;
    let mut height = region.padding.top + region.padding.bottom;
    for item in items {
        height += match item {
            dozer_panel_files::menu::MenuItem::Entry { .. } => row_h + gap,
            dozer_panel_files::menu::MenuItem::Separator => 1.0 + gap,
        };
    }
    (width, height)
}

/// 单个菜单行的高度(上下内边距 + 一行图标/文字)。
fn row_height() -> f32 {
    let inset = byteui::theme::geometry::menu_hover_inset();
    let pad_h = (byteui::theme::geometry::menu_pad_h() - inset).max(0.0);
    let text_h = byteui::theme::font::label() as f32 * 1.2;
    // 按钮上下 padding + 内容高度;左右 padding 不影响高度。
    let _ = pad_h;
    byteui::theme::geometry::menu_pad_v() * 2.0 + text_h
}

/// `context_menu` 区域样式(与 Dozer `theme::region::context_menu()` 同值):
/// 背景取 `panel` + alpha 0xfa,描边 1px、圆角 9、内边距 8、项间距 1。
fn region() -> byteui::theme::region::RegionStyle {
    let scale = byteui::theme::icon_size::scale();
    let mut bg = byteui::theme::color::current().panel;
    bg.a = 0xfa as f32 / 255.0;
    byteui::theme::region::RegionStyle {
        background: Some(bg),
        border: Some(Border {
            color: byteui::theme::color::current().border,
            width: 1.0,
            radius: 9.0.into(),
        }),
        padding: Padding::new(8.0 * scale),
        gap: 1.0 * scale,
        margin: Padding::ZERO,
    }
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

/// 与具体面板无关的一行(Agent 选择器等复用同一套外框与行样式)。
pub enum OverlayRow {
    Entry {
        icon: Option<byteui::interaction::icons::IconKind>,
        label: String,
        enabled: bool,
        msg: Box<Msg>,
        /// 图标自己的颜色(Agent 选择器用);`None` 跟随文字色。
        tint: Option<Color>,
    },
    Separator,
}

/// 按已规整的行渲染浮层。空行表不弹(返回一层的空容器)。
pub fn view_rows<'a>(
    rows: Vec<OverlayRow>,
    anchor: MenuAnchor,
    window: (f32, f32),
    on_dismiss: Msg,
) -> El<'a> {
    if rows.is_empty() {
        return iced_widget::container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let region = region();
    let (w, h) = rows_size(&rows);
    let anchor = anchor.clamped((w, h), window);
    let body: Vec<El<'a>> = rows
        .into_iter()
        .map(|row| match row {
            OverlayRow::Entry {
                icon,
                label,
                enabled,
                msg,
                tint,
            } => {
                let color = if enabled {
                    byteui::theme::color::current().body
                } else {
                    byteui::theme::color::current().dim
                };
                item_row(
                    icon_leading(icon, tint.unwrap_or(color)),
                    label,
                    color,
                    enabled.then_some(*msg),
                )
            }
            OverlayRow::Separator => separator(),
        })
        .collect();
    let menu: El<'a> = container(column(body).spacing(region.gap))
        .width(Length::Fixed(w))
        .padding(region.padding)
        .style(move |_t: &iced_widget::Theme| container::Style {
            background: region.background.map(Into::into),
            border: region.border.unwrap_or_default(),
            shadow: Shadow {
                color: Color {
                    a: 0.45,
                    ..Color::BLACK
                },
                offset: Vector::new(0.0, 10.0),
                blur_radius: 28.0,
            },
            ..container::Style::default()
        })
        .into();
    // 点空白处关闭:整窗一记透明按下区,菜单自己叠在上面吞掉自己的点击。
    let scrim: El<'a> = iced_widget::mouse_area(
        container(iced_widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(on_dismiss)
    .into();
    let positioned: El<'a> = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: anchor.y,
            left: anchor.x,
            right: 0.0,
            bottom: 0.0,
        })
        .into();
    iced_widget::stack![scrim, positioned].into()
}

/// 已规整行的外形尺寸(宽恒为 `menu_item_width` + 内边距;高按行数估算)。
fn rows_size(rows: &[OverlayRow]) -> (f32, f32) {
    let region = region();
    let row_h = row_height();
    let gap = byteui::theme::geometry::menu_gap();
    let width =
        byteui::theme::geometry::menu_item_width() + region.padding.left + region.padding.right;
    let mut height = region.padding.top + region.padding.bottom;
    for row in rows {
        height += match row {
            OverlayRow::Entry { .. } => row_h + gap,
            OverlayRow::Separator => 1.0 + gap,
        };
    }
    (width, height)
}

fn icon_leading<'a>(
    icon: Option<byteui::interaction::icons::IconKind>,
    color: Color,
) -> Option<El<'a>> {
    icon.map(|icon| byteui::interaction::icons::view(icon, byteui::theme::icon_size::row(), color))
}

/// 一条 1px 分隔线,左右内缩 `menu_pad_h`(与 Dozer 同一留白)。
fn separator<'a>() -> El<'a> {
    container(byteui::layout::divider::horizontal())
        .width(Length::Fill)
        .padding(Padding {
            left: byteui::theme::geometry::menu_pad_h(),
            right: byteui::theme::geometry::menu_pad_h(),
            ..Padding::default()
        })
        .into()
}

/// 单个菜单项:透明底 + `base_color` 文字/图标;`msg` 为 `Some` 时挂点击并在
/// hover/pressed 切 `tab_hover` 底(圆角 6,左右各内缩 `menu_hover_inset`)。
/// `None` 为锁定项:不可点、不高亮。
fn item_row<'a>(
    leading: Option<El<'a>>,
    label: String,
    base_color: Color,
    msg: Option<Msg>,
) -> El<'a> {
    let content = match leading {
        Some(leading) => row![
            leading,
            text(label)
                .size(byteui::theme::font::label())
                .color(base_color)
        ],
        None => row![
            text(label)
                .size(byteui::theme::font::label())
                .color(base_color)
        ],
    }
    .spacing(byteui::theme::geometry::menu_gap())
    .align_y(iced::Alignment::Center);
    let inset = byteui::theme::geometry::menu_hover_inset();
    let pad_h = (byteui::theme::geometry::menu_pad_h() - inset).max(0.0);
    let enabled = msg.is_some();
    let btn = button(content)
        .width(Length::Fill)
        .padding([byteui::theme::geometry::menu_pad_v(), pad_h])
        .style(move |_t: &iced_widget::Theme, s: button::Status| {
            let base = button::Style {
                background: None,
                text_color: base_color,
                ..button::Style::default()
            };
            let hovered =
                matches!(s, button::Status::Hovered) || matches!(s, button::Status::Pressed);
            if enabled && hovered {
                button::Style {
                    background: Some(byteui::theme::color::current().tab_hover.into()),
                    text_color: base_color,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 6.0.into(),
                    },
                    ..base
                }
            } else {
                base
            }
        });
    let btn: El<'_> = match msg {
        Some(msg) => btn.on_press(msg).into(),
        None => btn.into(),
    };
    container(btn)
        .width(Length::Fill)
        .padding(Padding {
            left: inset,
            right: inset,
            ..Padding::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
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
