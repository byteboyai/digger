// Append these probes inside product.rs mod tests in an isolated source copy.
    #[test]
    fn review_probe_stale_binding_error_closes_current_dialog() {
        use bytehost_settings::dialog::Phase;
        let mut shell = fresh("review-stale-dialog-binding");
        shell.settings.open(None);
        let old = shell.settings.key().unwrap();
        let old_window = iced::window::Id::unique();
        assert!(shell.settings.on_native_opened(old_window));
        shell.settings.on_bound(old);
        shell.settings.close();
        shell.settings.on_native_closed(old_window);
        shell.settings.open(None);
        let current = shell.settings.key().unwrap();
        let current_window = iced::window::Id::unique();
        assert!(shell.settings.on_native_opened(current_window));
        shell.settings.on_bound(current);
        assert_ne!(old.session, current.session);
        assert!(matches!(shell.settings.phase(), Phase::Open));
        let _ = shell.update_settings(crate::settings::Message::Bound(old, Err("synthetic old bind failure".into())));
        assert!(shell.settings.window.is_none());
        assert!(matches!(shell.settings.phase(), Phase::Closed));
        println!("REPRO: stale binding failure closed the current dialog");
    }

    #[test]
    fn review_probe_binding_failure_erases_retryable_phase() {
        use bytehost_settings::dialog::Phase;
        let mut shell = fresh("review-current-dialog-binding");
        shell.settings.open(None);
        let key = shell.settings.key().unwrap();
        shell.settings.on_native_opened(iced::window::Id::unique());
        let _ = shell.update_settings(crate::settings::Message::Bound(key, Err("synthetic current failure".into())));
        assert!(shell.settings.bind_error.is_some());
        assert!(shell.settings.window.is_none());
        assert!(matches!(shell.settings.phase(), Phase::Closed));
        assert!(shell.settings.key().is_none());
        println!("REPRO: binding failure destroyed the retryable Failed state and its only error UI");
    }

    #[test]
    fn review_probe_token_is_exposed_by_product_message_debug() {
        let message = crate::settings::Message::GitToken(bytehost_git_accounts::GitProvider::GitHub, "SYNTHETIC-NOT-A-REAL-TOKEN".into());
        assert!(format!("{message:?}").contains("SYNTHETIC-NOT-A-REAL-TOKEN"));
        println!("REPRO: product Message Debug contains the synthetic token input");
    }
