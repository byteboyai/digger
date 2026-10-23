//! Agent 终端的几何换算(S6d-2 Task 6)。
//!
//! 终端块 = 配对里"终端"那一块(列表展开时是 `pair.second`,收起时是整个面板
//! 内容矩形);网格 = 块尺寸减去内边距与页签条高度后,经 `TermStyle::grid_size`
//! 换算。至少 1×1、不超过 `u16::MAX`——退化尺寸不发 0,缩放风暴不发重复网格。

use bytehost_chrome::{PairRects, Rect};
use dozer_panel_agent::display::PaneStyle;
use dozer_panel_term::style::TermStyle;

/// 页签条占据的高度:一行页签按钮 + 与分隔线的间距 + 1px 分隔线。
pub fn tab_bar_height() -> f32 {
    byteui::theme::geometry::tab_button_size() + 4.0 + 1.0
}

/// 终端块矩形:列表收起(或不是配对)时是整块面板内容,否则是配对第二块。
pub fn terminal_block(pane: Rect, list_collapsed: bool, pair: Option<PairRects>) -> Rect {
    match pair {
        Some(p) if !list_collapsed => p.second,
        _ => pane,
    }
}

/// 块 → 网格。内边距按 `PaneStyle` 扣,页签条高度按 [`tab_bar_height`] 扣;
/// 结果钳到 `[1, u16::MAX]`,所以 0/负/NaN 都不会发出 0 尺寸。
pub fn grid(block: Rect, pane: &PaneStyle, tab_bar_h: f32, style: &TermStyle) -> (u16, u16) {
    let width = block.w - pane.padding.left - pane.padding.right;
    let height = block.h - pane.padding.top - pane.padding.bottom - tab_bar_h;
    let (cols, rows) = style.grid_size(width, height);
    (clamp_grid(cols), clamp_grid(rows))
}

fn clamp_grid(value: usize) -> u16 {
    value.clamp(1, u16::MAX as usize) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_widget::core::Padding;

    fn style() -> TermStyle {
        TermStyle {
            font: iced::Font::with_name("JetBrains Mono"),
            font_size: 14.0,
            line_height_factor: 1.2,
        }
    }

    fn pane() -> PaneStyle {
        PaneStyle {
            gap: 4.0,
            padding: Padding::from(8.0),
            background: None,
        }
    }

    #[test]
    fn grid_matches_the_style_formula_for_a_known_block() {
        // 600×400 减去左右 8、上下 8 与页签条 29 → 584×355;
        // cell = 14*0.6 = 8.4,line = 14*1.2 = 16.8。
        assert_eq!(tab_bar_height(), 29.0, "24 + 4 + 1");
        let got = grid(
            Rect::new(0.0, 0.0, 600.0, 400.0),
            &pane(),
            tab_bar_height(),
            &style(),
        );
        assert_eq!(got, (69, 21));
        assert_eq!(style().grid_size(584.0, 355.0), (69, 21));
    }

    #[test]
    fn degenerate_blocks_give_at_least_one_by_one() {
        for (w, h) in [(0.0, 0.0), (-100.0, -100.0), (3.0, 3.0), (10.0, 10.0)] {
            let got = grid(
                Rect::new(0.0, 0.0, w, h),
                &pane(),
                tab_bar_height(),
                &style(),
            );
            assert_eq!(got, (1, 1), "w={w} h={h}");
        }
    }

    #[test]
    fn huge_blocks_clamp_to_u16() {
        let got = grid(
            Rect::new(0.0, 0.0, 1.0e9, 1.0e9),
            &pane(),
            tab_bar_height(),
            &style(),
        );
        assert_eq!(got, (u16::MAX, u16::MAX));
    }

    #[test]
    fn collapsed_list_shows_only_the_terminal_block() {
        // 收起时骨架给出的第一块宽 0、第二块是整块内容;`pair_view` 的收起分支
        // 只渲染第二块,所以终端块 = 整块面板,列表没有可见区域。
        let content = Rect::new(100.0, 20.0, 500.0, 400.0);
        let pair = PairRects {
            first: Rect::new(content.right(), content.y, 0.0, content.h),
            second: content,
            divider: Rect::new(content.right(), content.y, 0.0, content.h),
        };
        assert_eq!(pair.first.w, 0.0);
        assert_eq!(terminal_block(content, true, Some(pair)), content);
    }

    #[test]
    fn collapsed_list_uses_the_whole_panel_and_expanded_uses_the_second_block() {
        let content = Rect::new(100.0, 20.0, 500.0, 400.0);
        let first = Rect::new(600.0, 20.0, 200.0, 400.0);
        let second = Rect::new(100.0, 20.0, 492.0, 400.0);
        let pair = PairRects {
            first,
            second,
            divider: Rect::new(592.0, 20.0, 8.0, 400.0),
        };
        assert_eq!(terminal_block(content, true, Some(pair)), content);
        assert_eq!(terminal_block(content, false, Some(pair)), second);
        assert_eq!(terminal_block(content, false, None), content);
    }
}
