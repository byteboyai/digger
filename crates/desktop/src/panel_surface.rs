//! One outer contour per shell zone. Pair seams remain square.
//! iced's container clip is rectangular, so square child backgrounds need corner
//! cutouts. Native children cannot be covered by iced and use safe bounds instead.
use bytehost_chrome::{BodyArea, Rect, Side};
use iced::{Color, Element, Length, Point, Rectangle};
use iced_widget::{canvas, container, stack};

pub fn radius(maximized: bool) -> f32 {
    if maximized {
        byteui::theme::region::maximize_overlay()
            .border
            .radius
            .top_left
    } else {
        let zone = byteui::theme::region::left_zone();
        (zone.border.unwrap_or_default().radius.top_left - zone.padding.left).max(0.)
    }
}

/// Maximized panels reserve the frame's radius on all sides; normal panels keep
/// their existing geometry and trim only painted corners.
pub fn content_rect(rect: Rect, maximized: bool) -> Rect {
    if !maximized {
        return rect;
    }
    let r = radius(true)
        .min(rect.w.max(0.) / 2.)
        .min(rect.h.max(0.) / 2.);
    Rect::new(
        rect.x + r,
        rect.y + r,
        (rect.w - 2. * r).max(0.),
        (rect.h - 2. * r).max(0.),
    )
}

pub fn wrap<'a, M: 'a>(content: Element<'a, M>, maximized: bool) -> Element<'a, M> {
    if maximized {
        let style = byteui::theme::region::maximize_overlay();
        return container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(radius(true))
            .style(move |_| container::Style {
                background: Some(byteui::theme::color::current().bg.into()),
                border: style.border,
                ..Default::default()
            })
            .into();
    }
    stack![
        content,
        canvas::Canvas::new(Corners {
            radius: radius(false),
            outside: byteui::theme::region::window_background(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
    ]
    .into()
}

struct Corners {
    radius: f32,
    outside: Color,
}
impl<M> canvas::Program<M> for Corners {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Align to the outer zone contour, including its one-pixel padding.
        let inset = byteui::theme::region::left_zone().padding.left;
        let r = (self.radius + inset)
            .min((bounds.width + 2. * inset) / 2.)
            .min((bounds.height + 2. * inset) / 2.)
            .max(0.);
        // Cubic quarter-circle, reflected into each corner; no internal seams.
        let c = r * (1. - 0.552_284_8);
        for (x, y, sx, sy) in [
            (-inset, -inset, 1., 1.),
            (bounds.width + inset, -inset, -1., 1.),
            (-inset, bounds.height + inset, 1., -1.),
            (bounds.width + inset, bounds.height + inset, -1., -1.),
        ] {
            let p = |a, b| Point::new(x + sx * a, y + sy * b);
            let path = canvas::Path::new(|b| {
                b.move_to(p(0., 0.));
                b.line_to(p(r, 0.));
                b.bezier_curve_to(p(c, 0.), p(0., c), p(0., r));
                b.close();
            });
            frame.fill(&path, self.outside);
        }
        vec![frame.into_geometry()]
    }
}

/// A rectangular native child must stay inside the rounded contour. Keep its
/// header/side insets; only reserve a corner where it actually touches an edge.
pub fn native_rect(rect: Rect, body: Rect, radius: f32) -> Rect {
    let r = radius
        .max(0.)
        .min(body.w.max(0.) / 2.)
        .min(body.h.max(0.) / 2.);
    let touches_side = rect.x < body.x + r || rect.right() > body.right() - r;
    let top = rect.y.max(body.y + if touches_side { r } else { 0. });
    let bottom = rect
        .bottom()
        .min(body.bottom() - if touches_side { r } else { 0. });
    Rect::new(rect.x, top, rect.w, (bottom - top).max(0.))
}

pub fn native_radius(maximized: bool) -> f32 {
    if maximized { 0. } else { radius(false) }
}

pub fn area(
    mut area: BodyArea,
    maximized: bool,
    pair: Option<bytehost_chrome::PairRects>,
) -> BodyArea {
    if maximized {
        area.rect = content_rect(area.rect, true);
        area.pair = pair;
    }
    area
}

pub fn is_maximized(model: &bytehost_chrome::ShellModel, side: Side) -> bool {
    model.maximized() == Some(side)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_children_never_cover_the_rounded_corner_cutouts() {
        let body = Rect::new(20., 30., 300., 200.);
        for radius in [0., 7., 10., 16.] {
            for inset in [0., 3., 8., 20.] {
                for side in [0., 1., 2.] {
                    let x = body.x + inset + side * 80.;
                    let width = if side == 0. {
                        body.w - 2. * inset
                    } else {
                        130. - inset
                    };
                    let page = native_rect(
                        Rect::new(x, body.y + 60., width, body.h - 60.),
                        body,
                        radius,
                    );
                    for (x, y) in [
                        (page.x, page.y),
                        (page.right(), page.y),
                        (page.x, page.bottom()),
                        (page.right(), page.bottom()),
                    ] {
                        let cx = x.clamp(body.x + radius, body.right() - radius);
                        let cy = y.clamp(body.y + radius, body.bottom() - radius);
                        assert!(
                            (x - cx).powi(2) + (y - cy).powi(2) <= radius.powi(2) + 0.001,
                            "{page:?} radius {radius}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn existing_safe_native_insets_and_normal_body_geometry_are_preserved() {
        let body = Rect::new(20., 30., 300., 200.);
        let page = Rect::new(28., 80., 284., 150.);
        assert_eq!(native_rect(page, body, 7.), page);
        assert_eq!(content_rect(body, false), body);
        assert_eq!(content_rect(body, true), Rect::new(30., 40., 280., 180.));
    }
    #[test]
    fn tiny_native_bounds_and_frames_are_finite_and_nonnegative() {
        for size in [0., 1., 5., 20.] {
            let body = Rect::new(0., 0., size, size);
            let frame = content_rect(body, true);
            let page = native_rect(body, body, 10.);
            for rect in [frame, page] {
                assert!(rect.x.is_finite() && rect.y.is_finite() && rect.w >= 0. && rect.h >= 0.);
                assert!(rect.right() <= body.right() && rect.bottom() <= body.bottom());
            }
        }
    }
}
