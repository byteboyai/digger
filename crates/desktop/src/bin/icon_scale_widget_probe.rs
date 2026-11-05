//! Minimal icon-scale widget probe for the conversations panel.
//!
//! The parent scale test forks this binary once per scale instead of forking
//! the full libtest harness. Everything here is synchronous and headless: no
//! Tokio runtime, no `dozer-client`, no daemon socket and no filesystem
//! access. Exits 2 only for a missing/invalid `BYTEUI_ICON_SCALE`.

use std::process::ExitCode;

use byteui::theme::icon_size;
use digger_desktop::adapters::scale_widget_checks;

fn main() -> ExitCode {
    let raw = match std::env::var_os("BYTEUI_ICON_SCALE") {
        Some(raw) => raw.to_string_lossy().into_owned(),
        None => {
            eprintln!("icon_scale_widget_probe: BYTEUI_ICON_SCALE is required (e.g. 0.5, 1, 2)");
            return ExitCode::from(2);
        }
    };
    let Some(expected) = raw.parse::<f32>().ok().and_then(icon_size::normalize_scale) else {
        eprintln!(
            "icon_scale_widget_probe: invalid BYTEUI_ICON_SCALE {raw:?}; expected a positive finite number"
        );
        return ExitCode::from(2);
    };

    // Do not call `init_scale`: it reads a persisted file. The scale is the
    // only global this process needs, and it is set directly from the env.
    icon_size::set_scale(expected);
    let mut panel = scale_widget_checks::stub_panel();
    match scale_widget_checks::check_all(&mut panel, expected) {
        Ok(()) => {
            println!("icon_scale_widget_probe: ok at scale {expected}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("icon_scale_widget_probe: scale {expected}: {error}");
            ExitCode::FAILURE
        }
    }
}
