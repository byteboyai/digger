//! Group-chat detail column: the native chat page has no native panel header, so it
//! keeps clear of the zone's rounded corners the same way Dozer's
//! `group_chat_content_pane_bounds_for` does (`chrome_top = 0` branch).
use bytehost_chrome::Rect;

/// Zone corner radius left over after the zone padding, as in Dozer.
fn corner_radius() -> f32 {
    let zone = byteui::theme::region::right_zone();
    zone.border
        .map(|b| (b.radius.bottom_left - zone.padding.bottom).max(0.))
        .unwrap_or(0.)
}

/// Height of the strip kept for the iced expand button when the list is collapsed
/// (Dozer has no such button; the chat would otherwise cover it).
pub fn collapsed_strip_height(scale: f32) -> f32 {
    let current = byteui::theme::icon_size::scale().max(f32::EPSILON);
    (byteui::theme::geometry::tab_button_size() / current + 8.) * scale.max(0.)
}

/// `block` is the shared shell's content block of the pair (zone padding already
/// excluded). Metrics accessors include the global UI scale; normalise once so the
/// caller's `scale` is applied exactly once.
pub fn chat_bounds(block: Rect, scale: f32, list_collapsed: bool) -> Rect {
    let scale = scale.max(0.);
    let current = byteui::theme::icon_size::scale().max(f32::EPSILON);
    let corner = corner_radius() / current * scale;
    let geometry = byteui::theme::geometry::current();
    // Dozer measures to the status bar, the shared shell to the (shorter) footbar.
    let status_gap = (geometry.status_bar_height - geometry.footbar_height) / current * scale;
    let strip = if list_collapsed {
        collapsed_strip_height(scale)
    } else {
        0.
    };
    let top = (corner + strip).min(block.h.max(0.));
    Rect {
        x: block.x,
        y: block.y + top,
        w: block.w.max(1.),
        h: (block.h - top - corner - status_gap).max(1.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> Rect {
        Rect {
            x: 693.,
            y: 47.,
            w: 291.2,
            h: 730.,
        }
    }

    #[test]
    fn chat_bounds_matches_dozers_numbers_for_a_known_window() {
        // Dozer c76302fc `group_chat_content_pane_bounds_for(Side::Right, 1200, 800,
        // default shell, content_desired, list_visible)`, run in a locked source snapshot
        // (/private/tmp/s7c-dozer-reference, probe `s8c_reference_numbers`, HOME isolated):
        //   list visible: (693.0, 54.0, 291.19998, 712.0)
        //   list collapsed: (693.0, 54.0, 456.0, 712.0), zone=(692,456), split .35,
        //   top 40, status 26.
        // Digger's real block for the same window is (693,47,291.2,730) (footbar 22 and
        // zone padding 1); expected result is within 1px of Dozer on every edge.
        let _ = byteui::theme::region::right_zone();
        let actual = chat_bounds(block(), 1., false);
        for (a, b) in [
            (actual.x, 693.),
            (actual.y, 54.),
            (actual.w, 291.2),
            (actual.h, 712.),
        ] {
            assert!((a - b).abs() <= 1., "{actual:?}");
        }
    }

    #[test]
    fn collapsed_list_keeps_a_strip_for_the_expand_button_and_the_same_bottom() {
        let open = chat_bounds(block(), 1., false);
        let collapsed = chat_bounds(block(), 1., true);
        assert!(collapsed.y > open.y);
        assert!((collapsed.y + collapsed.h - (open.y + open.h)).abs() < 0.01);
        assert!(collapsed.y - open.y >= byteui::theme::icon_size::row());
    }

    #[test]
    fn chat_bounds_is_never_negative_for_tiny_blocks() {
        for scale in [0., 0.5, 1., 2., 3.] {
            for size in [0., 1., 8., 30., 200.] {
                for collapsed in [false, true] {
                    let b = chat_bounds(
                        Rect {
                            x: 0.,
                            y: 0.,
                            w: size,
                            h: size,
                        },
                        scale,
                        collapsed,
                    );
                    assert!(b.w.is_finite() && b.h.is_finite());
                    assert!(b.w >= 1. && b.h >= 1.);
                }
            }
        }
    }

    #[test]
    fn chat_bounds_follows_the_scale() {
        let b = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 1000.,
        };
        let one = chat_bounds(b, 1., false);
        let two = chat_bounds(b, 2., false);
        assert!((two.y - one.y * 2.).abs() < 0.01);
        assert!((two.h - (b.h - (b.h - one.h) * 2.)).abs() < 0.01);
        assert_eq!(two.x, one.x);
        assert_eq!(two.w, one.w);
    }
}
