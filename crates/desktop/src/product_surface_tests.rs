// Paint the production shell and all nine panel bodies with the software backend.
// This checks output pixels, not the presence of a radius field in source code.
#[test]
fn every_panel_keeps_the_outer_corners_clear_of_square_child_backgrounds() {
    use iced_core::{Layout, Pixels, Rectangle, Size, layout, renderer, widget};
    if std::env::var("DIGGER_SURFACE_SCHEME").as_deref() == Ok("light") {
        byteui::theme::color::set_scheme(byteui::theme::color::ColorScheme::Light);
    }
    let mut shell = fresh("panel-corner-pixels");
    let root = test_temp_dir("panel-corner-pixels-project");
    std::fs::write(root.join("chapter.md"), "# Chapter\n\nA paragraph\n").unwrap();
    send(
        &mut shell,
        Msg::Projects(crate::projects::Message::Opened(Ok(Some(
            dozer_model::ProjectInfo {
                id: 1,
                path: root.to_string_lossy().into_owned(),
                name: "Corner fixture".into(),
                last_active_ms: 1,
                created_ms: 1,
                updated_ms: 1,
            },
        )))),
    );
    shell.agent.shell.status = crate::adapters::agent::Status::Ready;
    shell.seed_usage_sample();
    let metrics = metrics_from_theme();
    let size = Size::new(1200., 800.);
    let _ = shell.on_shell(ShellMsg::Resized {
        w: size.width,
        h: size.height,
    });
    for panel in shell::all_panels() {
        let _ = shell.on_shell(ShellMsg::ShowPanel(panel.clone()));
        let original_side = shell.shell.side_of(&panel).unwrap();
        for mirrored in [false, true] {
            if mirrored {
                let _ = shell.on_shell(ShellMsg::PanelPress {
                    panel: panel.clone(),
                });
                let _ = shell.on_shell(ShellMsg::RailDragMove {
                    side: original_side.other(),
                    index: 0,
                });
                let _ = shell.on_shell(ShellMsg::PointerReleased);
            }
            let _ = shell.on_shell(ShellMsg::ShowPanel(panel.clone()));
            let side = shell.shell.side_of(&panel).unwrap();
            assert_eq!(
                side,
                if mirrored {
                    original_side.other()
                } else {
                    original_side
                }
            );
            let collapsed_cases = if shell::rules(&panel).and_then(|r| r.pane).is_some() {
                vec![false, true]
            } else {
                vec![false]
            };
            for collapsed in collapsed_cases {
                if collapsed {
                    let _ = shell.on_shell(ShellMsg::TogglePane(panel.clone()));
                }
                for maximized in [false, true] {
                    let _ = shell.on_shell(if maximized {
                        ShellMsg::Maximize(side)
                    } else {
                        ShellMsg::MaximizeClose
                    });
                    let rects = shell.shell.rects(&metrics);
                    let frame = if maximized {
                        rects.content(side).unwrap()
                    } else {
                        let zone = if side == Side::Left {
                            byteui::theme::region::left_zone()
                        } else {
                            byteui::theme::region::right_zone()
                        };
                        let z = rects.zone(side).unwrap();
                        Rect::new(
                            z.x + zone.margin.left,
                            z.y + zone.margin.top,
                            z.w - zone.margin.left - zone.margin.right,
                            z.h - zone.margin.top - zone.margin.bottom,
                        )
                    };
                    let mut view = bytehost_chrome::view(&shell.shell, &shell, &metrics);
                    let mut renderer = iced_renderer::fallback::Renderer::Secondary(
                        iced_tiny_skia::Renderer::new(iced::Font::DEFAULT, Pixels(14.)),
                    );
                    let mut tree = widget::Tree::new(view.as_widget());
                    let node = view.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, size),
                    );
                    view.as_widget().draw(
                        &tree,
                        &mut renderer,
                        &iced::Theme::Dark,
                        &renderer::Style {
                            text_color: iced::Color::WHITE,
                        },
                        Layout::new(&node),
                        iced::mouse::Cursor::Unavailable,
                        &Rectangle::with_size(size),
                    );
                    let mut pixels = tiny_skia::Pixmap::new(1200, 800).unwrap();
                    let mut mask = tiny_skia::Mask::new(1200, 800).unwrap();
                    let iced_renderer::fallback::Renderer::Secondary(mut renderer) = renderer
                    else {
                        unreachable!()
                    };
                    renderer.draw(
                        &mut pixels.as_mut(),
                        &mut mask,
                        &iced_tiny_skia::graphics::Viewport::with_physical_size(
                            Size::new(1200, 800),
                            1.,
                        ),
                        &[Rectangle::with_size(size)],
                        iced::Color::BLACK,
                    );
                    // iced_tiny_skia targets a BGRA surface; PNG and pixel assertions use RGBA.
                    for pixel in pixels.data_mut().chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    if !maximized {
                        let expected = byteui::theme::region::window_background().into_rgba8();
                        for (x, y) in [
                            (frame.x + 1., frame.y + 1.),
                            (frame.right() - 2., frame.y + 1.),
                            (frame.x + 1., frame.bottom() - 2.),
                            (frame.right() - 2., frame.bottom() - 2.),
                        ] {
                            let pixel = pixels.pixel(x as u32, y as u32).unwrap().demultiply();
                            let actual = [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()];
                            assert!(
                                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                                "{} {side:?} corner ({x},{y}) = {actual:?}, expected {expected:?}",
                                panel.as_str()
                            );
                        }
                    }
                    if let Ok(out) = std::env::var("DIGGER_SURFACE_ARTIFACTS") {
                        std::fs::create_dir_all(&out).unwrap();
                        pixels
                            .save_png(std::path::Path::new(&out).join(format!(
                                "{}-{}-{}-{}.png",
                                panel.as_str(),
                                if maximized { "maximized" } else { "normal" },
                                if mirrored { "mirrored" } else { "default" },
                                if collapsed { "collapsed" } else { "expanded" }
                            )))
                            .unwrap();
                    }
                }
                let _ = shell.on_shell(ShellMsg::MaximizeClose);
            }
            if shell.pane_collapsed(&panel) {
                let _ = shell.on_shell(ShellMsg::TogglePane(panel.clone()));
            }
        }
    }
}

#[test]
fn maximized_terminal_grid_and_pair_divider_use_the_inner_frame() {
    let mut shell = fresh("maximized-surface-geometry");
    let _ = shell.on_shell(ShellMsg::ShowPanel(shell::agent_id()));
    let side = shell.shell.side_of(&shell::agent_id()).unwrap();
    let _ = shell.on_shell(ShellMsg::Maximize(side));
    let _ = shell.on_shell(ShellMsg::TogglePane(shell::agent_id()));
    let raw = shell
        .shell
        .rects(&metrics_from_theme())
        .content(side)
        .unwrap();
    let inner = crate::panel_surface::content_rect(raw, true);
    assert_eq!(shell.current_agent_block(), Some(inner));
    let _ = shell.on_shell(ShellMsg::TogglePane(shell::agent_id()));
    let _ = shell.on_shell(ShellMsg::DividerPress(Divider::Pair(side)));
    let target = inner.x + (inner.w - metrics_from_theme().divider_width) * 0.65;
    let _ = shell.on_shell(ShellMsg::CursorMoved {
        x: target,
        y: inner.y + 100.,
    });
    let pair = shell::pair_rects(&shell.shell, side).unwrap();
    assert!(
        (pair.divider.x - target).abs() < 0.1,
        "{:?}, target {target}",
        pair.divider
    );
}
