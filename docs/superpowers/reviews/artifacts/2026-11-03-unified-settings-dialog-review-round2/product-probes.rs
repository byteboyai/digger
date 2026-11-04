// Append inside product.rs mod tests in an isolated copy; no real windows are opened.
    #[tokio::test]
    async fn round2_review_resize_then_never_executes_binding() {
        use iced::futures::StreamExt;
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = called.clone();
        let id = iced::window::Id::unique();
        let task = iced::window::resize::<()>(id, iced::Size::new(800.0, 600.0)).then(move |_| {
            marker.store(true, std::sync::atomic::Ordering::SeqCst);
            Task::done(())
        });
        let mut actions = iced_runtime::task::into_stream(task).unwrap();
        let mut count = 0;
        while let Some(action) = actions.next().await {
            assert!(!matches!(action, iced_runtime::Action::Output(_)));
            count += 1;
        }
        assert_eq!(count, 1);
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
        println!("REPRO: resize emits one effect and no output; then(binding) is never called");
    }

    #[test]
    fn round2_review_destroy_timeout_loses_instance_before_forced_cleanup() {
        use bytehost_settings::dialog::{DialogEffect, Phase};
        let mut shell = fresh("review-round2-timeout");
        shell.settings.open(None);
        let key = shell.settings.key().unwrap();
        let id = iced::window::Id::unique();
        shell.settings.on_native_opened(key, id);
        shell.settings.on_bound(key);
        shell.settings.close();
        let effects = shell.settings.on_destroy_timed_out(key);
        assert!(effects.iter().any(|effect| matches!(effect, DialogEffect::Destroy {..})));
        assert!(shell.settings.window_for(key).is_none());
        assert!(matches!(shell.settings.phase(), Phase::Closed));
        let _ = shell.apply_settings_effects(effects);
        println!("REPRO: forced Destroy is emitted after its native-instance mapping has already been erased");
    }

    #[tokio::test]
    async fn round2_review_actual_created_task_stops_after_resize() {
        use iced::futures::StreamExt;
        use iced_runtime::window::Action as WindowAction;
        let mut shell = fresh("round2-created-task");
        let parent = iced::window::Id::unique();
        let child = iced::window::Id::unique();
        shell.window = Some(parent);
        shell.settings.open(None);
        let key = shell.settings.key().unwrap();
        let task = shell.update_settings(crate::settings::Message::Created(key, child));
        let mut actions = iced_runtime::task::into_stream(task).unwrap();
        match actions.next().await.unwrap() {
            iced_runtime::Action::Window(WindowAction::GetSize(id, reply)) => {
                assert_eq!(id, parent);
                let _ = reply.send(iced::Size::new(1000.0,800.0));
            }
            _ => panic!("expected size request"),
        }
        assert!(matches!(actions.next().await, Some(iced_runtime::Action::Window(WindowAction::Resize(id,_))) if id == child));
        assert!(actions.next().await.is_none(), "no attach_child or Bound action is produced");
        assert!(matches!(shell.settings.phase(), bytehost_settings::dialog::Phase::Opening));
        println!("REPRO: actual Created handler emits GetSize and Resize only, then ends without native binding");
    }

    #[tokio::test]
    async fn round2_review_timeout_task_emits_no_native_close() {
        use iced::futures::StreamExt;
        let mut shell = fresh("round2-timeout-task");
        shell.settings.open(None);
        let key = shell.settings.key().unwrap();
        let child = iced::window::Id::unique();
        shell.settings.on_native_opened(key, child);
        shell.settings.on_bound(key);
        shell.settings.close();
        let task = shell.update_settings(crate::settings::Message::DestroyTimedOut(key));
        let mut actions = iced_runtime::task::into_stream(task).unwrap();
        let mut window_actions = 0;
        while let Some(action) = actions.next().await {
            if matches!(action, iced_runtime::Action::Window(_)) { window_actions += 1; }
        }
        assert_eq!(window_actions, 0);
        println!("REPRO: destroy timeout task schedules another timeout but no forced native close");
    }
