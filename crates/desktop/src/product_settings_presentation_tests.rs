#[tokio::test]
async fn settings_window_is_identified_and_hidden_before_the_created_callback() {
    use iced::futures::StreamExt;
    use iced_runtime::window::Action as W;
    let mut shell = fresh("settings-first-frame-reservation");
    shell.window = Some(window::Id::unique());
    let task = shell.open_settings(Some(crate::settings::language_page_id()));
    let id = shell
        .settings
        .window
        .expect("identity reserved synchronously");
    assert!(shell.is_settings_window(id));
    let mut actions = iced_runtime::task::into_stream(task).unwrap();
    match actions.next().await.unwrap() {
        iced_runtime::Action::Window(W::Open(opened, settings, _)) => {
            assert_eq!(opened, id);
            assert!(!settings.visible);
            assert!(settings.transparent);
        }
        _ => panic!("expected a hidden settings window open"),
    }
    // Window routing must already choose the overlay before Created arrives.
    let mut view = shell.view_window(id);
    let (_, _, texts) = crate::adapters::scale_widget_checks::layout(&mut view, 1200.);
    assert!(texts.0.iter().any(|(t,_)|t==&crate::settings::shared_text("settings.language.title","Language")));
}

#[tokio::test]
async fn settings_binding_waits_for_a_sized_redraw_before_reveal() {
    use iced::futures::StreamExt;
    use iced_core::{Event, Layout, Rectangle, Size, mouse};
    use iced_runtime::window::Action as W;
    let mut shell = fresh("settings-first-frame-order");
    shell.window = Some(window::Id::unique());
    let _ = shell.on_shell(ShellMsg::Resized { w: 1200., h: 600. });
    shell.settings.open(None);
    let key = shell.settings.key().unwrap();
    let id = window::Id::unique();
    assert!(shell.settings.on_native_opened(key, id));
    assert!(
        iced_runtime::task::into_stream(
            shell.update_settings(crate::settings::Message::FirstFrame(key, id))
        )
        .is_none()
    );
    assert!(
        iced_runtime::task::into_stream(
            shell.update_settings(crate::settings::Message::Bound(key, Ok(())))
        )
        .is_none()
    );
    assert!(matches!(
        shell.settings.phase(),
        bytehost_settings::dialog::Phase::Opening
    ));
    let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
    let mut frames = vec![];
    for width in [800., 1200.] {
        let mut view = shell.view_window(id);
        crate::adapters::scale_widget_checks::with_layout(
            &mut view,
            width,
            |view, tree, node, renderer, _| {
                view.as_widget_mut().update(
                    tree,
                    &redraw,
                    Layout::new(node),
                    mouse::Cursor::Unavailable,
                    renderer,
                    &mut iced_core::clipboard::Null,
                    &mut iced_core::Shell::new(&mut frames),
                    &Rectangle::with_size(Size::new(width, 600.)),
                );
            },
        );
        if width == 800. {
            assert!(frames.is_empty(), "placeholder dimensions cannot reveal");
        }
    }
    let frame = frames
        .into_iter()
        .find_map(|m| match m {
            Msg::Settings(crate::settings::Message::FirstFrame(k, w)) => Some((k, w)),
            _ => None,
        })
        .expect("correctly sized overlay requests presentation");
    assert_eq!(frame, (key, id));
    let mut actions = iced_runtime::task::into_stream(
        shell.update_settings(crate::settings::Message::FirstFrame(key, id)),
    )
    .unwrap();

    assert!(
        matches!(actions.next().await,Some(iced_runtime::Action::Window(W::Run(w,_))) if w==id)
    );
    assert!(
        iced_runtime::task::into_stream(
            shell.update_settings(crate::settings::Message::FirstFrame(key, id))
        )
        .is_none(),
        "duplicate frame must not show/focus again"
    );
}

#[test]
fn closing_settings_cancels_pending_and_already_queued_presentations() {
    use crate::settings::Message as S;
    for already_queued in [false, true] {
        let mut shell = fresh("settings-cancel-frame");
        shell.settings.open(None);
        let key = shell.settings.key().unwrap();
        let id = window::Id::unique();
        shell.settings.on_native_opened(key, id);
        let _ = shell.update_settings(S::Bound(key, Ok(())));
        let permit = shell.settings.presentation_permit.clone().unwrap();
        if already_queued {
            let _ = shell.update_settings(S::FirstFrame(key, id));
        }
        let _ = shell.close_settings();
        assert!(!permit.load(std::sync::atomic::Ordering::Acquire));
        assert!(
            iced_runtime::task::into_stream(shell.update_settings(S::FirstFrame(key, id)))
                .is_none()
        );
        assert!(
            iced_runtime::task::into_stream(shell.update_settings(S::Presented(key, id, Ok(true))))
                .is_none()
        );
    }
}

#[test]
fn old_settings_frames_cannot_reveal_a_replacement_or_paint_the_main_view() {
    use crate::settings::Message as S;
    let mut shell = fresh("settings-stale-frame");
    shell.window = Some(window::Id::unique());
    shell.settings.open(None);
    let old = shell.settings.key().unwrap();
    let old_id = window::Id::unique();
    shell.settings.on_native_opened(old, old_id);
    let _ = shell.update_settings(S::Bound(old, Ok(())));
    shell.settings.close();
    shell.settings.on_destroy_timed_out(old);
    shell.settings.open(None);
    let key = shell.settings.key().unwrap();
    let id = window::Id::unique();
    shell.settings.on_native_opened(key, id);
    let _ = shell.update_settings(S::Bound(key, Ok(())));
    assert!(
        iced_runtime::task::into_stream(shell.update_settings(S::FirstFrame(old, old_id)))
            .is_none()
    );
    assert_eq!(shell.settings.pending_frame, Some((key, id)));
    let mut view = shell.view_window(old_id);
    assert!(
        crate::adapters::scale_widget_checks::layout(&mut view, 1200.)
            .2
            .0
            .is_empty()
    );
}
