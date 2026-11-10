fn rail_pointer(shell: &mut ProductShell, point: iced::Point, event: iced_core::mouse::Event) {
    let id = shell.window.unwrap();
    let messages = {
        let mut view = shell.view_window(id);
        crate::adapters::scale_widget_checks::with_layout(
            &mut view, 1200., |view, tree, node, renderer, _| {
                let mut messages = vec![];
                view.as_widget_mut().update(
                    tree, &iced_core::Event::Mouse(event), iced_core::Layout::new(node),
                    iced_core::mouse::Cursor::Available(point), renderer,
                    &mut iced_core::clipboard::Null, &mut iced_core::Shell::new(&mut messages),
                    &iced_core::Rectangle::with_size(iced::Size::new(1200., 600.)),
                );
                messages
            },
        )
    };
    for message in messages { send(shell, message); }
}

fn rail_button_point(side: Side, index: usize) -> iced::Point {
    use byteui::theme::{geometry as g, region as r};
    let region = match side { Side::Left => r::left_icon_rail(), Side::Right => r::right_icon_rail() };
    iced::Point::new(
        if side == Side::Left { g::icon_rail_width()/2. } else {1200.-g::icon_rail_width()/2.},
        g::top_bar_height()+region.padding.top+index as f32*(g::rail_button_size()+region.gap)+g::rail_button_size()/2.,
    )
}

#[test]
fn rail_clicks_anchor_the_pointer_before_press_and_do_not_start_dragging() {
    use iced_core::mouse::{Event as E, Button};
    for side in [Side::Left, Side::Right] {
        let mut shell = fresh("rail-single-click");
        shell.window=Some(window::Id::unique());
        send(&mut shell, Msg::Shell(ShellMsg::Resized{w:1200.,h:600.}));
        let before=shell.shell.rails().clone();
        let panel=before[side.index()][1].clone();
        let p=rail_button_point(side,1);
        // No prior cursor event: the old subscription path used stale (0,0).
        rail_pointer(&mut shell,p,E::ButtonPressed(Button::Left));
        assert_eq!(shell.shell.selected(side), &panel);
        assert_eq!(shell.shell.rail_drag().expect("press arms gesture").press_pos,(p.x,p.y));
        assert!(!shell.shell.rail_drag_confirmed());
        let jitter=iced::Point::new(p.x+1.,p.y+1.);
        rail_pointer(&mut shell,jitter,E::CursorMoved{position:jitter});
        // Even an adjacent/cross-rail hover intent cannot move before threshold.
        send(&mut shell,Msg::RailDragHover{side,index:0});
        send(&mut shell,Msg::RailDragHover{side:if side==Side::Left {Side::Right}else{Side::Left},index:0});
        assert!(!shell.shell.rail_drag_confirmed());
        assert_eq!(shell.shell.rails(),&before);
        assert!(shell.shell.rail_drag().unwrap().pending_cross.is_none());
        rail_pointer(&mut shell,jitter,E::ButtonReleased(Button::Left));
        assert!(shell.shell.rail_drag().is_none());
        let elsewhere=rail_button_point(side,0);
        rail_pointer(&mut shell,elsewhere,E::CursorMoved{position:elsewhere});
        assert_eq!(shell.shell.rails(),&before,"hover after click cannot reorder");
    }
}

#[test]
fn rail_pointer_motion_still_allows_reordering_and_cross_rail_drop() {
    use iced_core::mouse::{Event as E, Button};
    for side in [Side::Left,Side::Right] {
        let mut shell=fresh("rail-deliberate-drag");
        shell.window=Some(window::Id::unique());
        send(&mut shell,Msg::Shell(ShellMsg::Resized{w:1200.,h:600.}));
        let panel=shell.shell.rail(side)[1].clone();
        let p=rail_button_point(side,1);
        rail_pointer(&mut shell,p,E::ButtonPressed(Button::Left));
        let target=rail_button_point(side,0);
        rail_pointer(&mut shell,target,E::CursorMoved{position:target});
        assert!(shell.shell.rail_drag_confirmed());
        assert_eq!(shell.shell.rail(side)[0],panel);
        let other=if side==Side::Left {Side::Right}else{Side::Left};
        let target=rail_button_point(other,0);
        rail_pointer(&mut shell,target,E::CursorMoved{position:target});
        assert_eq!(shell.shell.rail_drag().unwrap().pending_cross,Some((other,0)));
        rail_pointer(&mut shell,target,E::ButtonReleased(Button::Left));
        assert!(shell.shell.rail_drag().is_none());
        assert_eq!(shell.shell.side_of(&panel),Some(other));
    }
}
