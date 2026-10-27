//! Review shares the pane insets and exported chrome metrics with its iced header.
use bytehost_chrome::Rect;

/// Metrics accessors already include the global UI scale. Normalize once so the
/// caller's explicit scale is applied exactly once (never the device scale).
pub fn review_bounds(block: Rect, scale: f32) -> Rect {
    let scale = scale.max(0.);
    let current_scale = byteui::theme::icon_size::scale().max(f32::EPSILON);
    let style = super::conversations_display::review_pane_style();
    let left = (style.padding.left / current_scale * scale).min(block.w.max(0.));
    let right = style.padding.right / current_scale * scale;
    let chrome = byteui::theme::geometry::preview_chrome_top_px() / current_scale * scale;
    let top = chrome.min(block.h.max(0.));
    Rect {
        x: block.x + left,
        y: block.y + top,
        w: (block.w - left - right).max(1.),
        h: (block.h - top - super::conversations_display::review_bottom_inset(scale)).max(1.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_bounds_matches_dozers_numbers_for_a_known_window() {
        // Dozer 2026-10-26b webview_geometry::content_pane_bounds_for,
        // Side::Right, 1200x800, default shell, Conversations, split .35:
        // zone=(692,456), y=top40+zone margin6+chrome38;
        // x=692+8, w=(456-divider)-list-16, h=800-84-status26.
        // Actual reference probe: /private/tmp/s7c-dozer-geometry.log.
        // Revision: no extra 8px bottom gap (the earlier plan was incorrect).
        // Digger shared chrome adds zone padding1 and uses footbar22;
        // its actual review block is (693,47,291.2,730). The pane reserves
        // status26-footbar22-zone padding1=3 at its bottom. Result is
        // (701,85,275.2,689), each coordinate within1 of actual Dozer.
        let actual = review_bounds(
            Rect {
                x: 693.,
                y: 47.,
                w: 291.2,
                h: 730.,
            },
            1.,
        );
        for (a, b) in [
            (actual.x, 700.),
            (actual.y, 84.),
            (actual.w, 274.4),
            (actual.h, 690.),
        ] {
            assert!((a - b).abs() <= 1., "{actual:?}");
        }
    }

    #[test]
    fn review_bounds_is_never_negative_for_tiny_blocks() {
        for scale in [0., 0.5, 1., 2., 3.] {
            for size in [0., 1., 8., 30., 200.] {
                let bounds = review_bounds(
                    Rect {
                        x: 0.,
                        y: 0.,
                        w: size,
                        h: size,
                    },
                    scale,
                );
                assert!(bounds.w.is_finite() && bounds.h.is_finite());
                assert!(bounds.w >= 1. && bounds.h >= 1.);
                if size == 200. {
                    assert!(bounds.x + bounds.w <= size && bounds.y + bounds.h <= size);
                }
            }
        }
    }

    #[test]
    fn review_bounds_follows_the_scale() {
        let block = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 1000.,
        };
        let one = review_bounds(block, 1.);
        let two = review_bounds(block, 2.);
        assert_eq!(two.x, one.x * 2.);
        assert_eq!(two.y, one.y * 2.);
        assert_eq!(two.w, block.w - (block.w - one.w) * 2.);
        let metrics = byteui::theme::geometry::current();
        let bottom = super::super::conversations_display::review_bottom_inset(2.);
        assert_eq!(two.h, block.h - metrics.preview_chrome_top_px * 2. - bottom);
        // Shared shell zone padding stays in logical pixels as UI scale changes.
        assert!(bottom > super::super::conversations_display::review_bottom_inset(1.));
    }
}
