//! Settings stay visually absent until binding and a correctly sized redraw.
use super::Message;
use bytehost_settings::dialog::DialogKey;
use iced::{Size, window};
use iced_widget::canvas;

pub(crate) struct FirstFrame {
    pub key: DialogKey,
    pub id: window::Id,
    pub expected: Size,
}
impl canvas::Program<crate::product::Msg> for FirstFrame {
    type State = bool;
    fn update(
        &self,
        sent: &mut bool,
        event: &iced_core::Event,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Option<canvas::Action<crate::product::Msg>> {
        if !*sent
            && selftest_enabled()
            && matches!(
                event,
                iced_core::Event::Window(window::Event::RedrawRequested(_))
            )
        {
            eprintln!(
                "INFO settings: redraw {:?}, expected {:?}",
                bounds.size(),
                self.expected
            );
        }
        if !*sent
            && matches!(
                event,
                iced_core::Event::Window(window::Event::RedrawRequested(_))
            )
            && (bounds.width - self.expected.width).abs() <= 0.5
            && (bounds.height - self.expected.height).abs() <= 0.5
        {
            *sent = true;
            Some(canvas::Action::publish(crate::product::Msg::Settings(
                Message::FirstFrame(self.key, self.id),
            )))
        } else {
            None
        }
    }
    fn draw(
        &self,
        _: &bool,
        _: &iced::Renderer,
        _: &iced::Theme,
        _: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![]
    }
}

#[cfg(target_os = "macos")]
fn native_window(
    handle: &dyn window::Window,
) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    if objc2::MainThreadMarker::new().is_none() {
        return Err("settings presentation requires the main thread".into());
    }
    let raw = handle.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(appkit) = raw.as_raw() else {
        return Err("settings requires an AppKit window".into());
    };
    // Borrowed only during window::run; window() returns a retained NSWindow.
    let view = unsafe { &*(appkit.ns_view.as_ptr() as *const objc2_app_kit::NSView) };
    view.window()
        .ok_or_else(|| "settings native window is unavailable".into())
}

pub(crate) fn prepare_hidden(handle: &dyn window::Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let window = native_window(handle)?;
        if selftest_enabled() {
            if window.isVisible() {
                return Err("settings selftest: placeholder was visible before preparation".into());
            }
            eprintln!("PASS settings: created hidden");
        }
        window.setAlphaValue(0.);
        window.setAnimationBehavior(objc2_app_kit::NSWindowAnimationBehavior::None);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = handle;
        Err("settings presentation is unsupported on this platform".into())
    }
}

pub(crate) fn reveal(handle: &dyn window::Window) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let window = native_window(handle)?;
        if window.parentWindow().is_none() {
            return Err("settings lost its parent before presentation".into());
        }
        if window.alphaValue() != 0. {
            return Err("settings became opaque before its prepared frame".into());
        }
        // Ordering and alpha restoration share the same main-thread callback;
        // the cancellation permit is checked before either can change.
        window.setAlphaValue(1.);
        window.makeKeyAndOrderFront(None);
        if selftest_enabled() {
            eprintln!("PASS settings: revealed bound window after its correctly sized frame");
        }
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = handle;
        Err("settings presentation is unsupported on this platform".into())
    }
}

pub(crate) fn check_prepared(handle: &dyn window::Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let window = native_window(handle)?;
        let parent = window
            .parentWindow()
            .ok_or_else(|| "settings attachment lost its parent".to_owned())?;
        if window.alphaValue() != 0. || window.frame() != parent.frame() {
            return Err(
                "settings must stay transparent at the parent frame while preparing".into(),
            );
        }
        // A Metal surface needs an ordered window to prepare its drawable.
        // Keep it completely transparent until the correctly sized frame is
        // presented; ordering out here can stall the first GPU frame forever.
        window.orderFront(None);
        if selftest_enabled() {
            eprintln!("PASS settings: attached/resized while fully transparent");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = handle;
        Err("settings presentation is unsupported on this platform".into())
    }
}

pub(crate) fn selftest_enabled() -> bool {
    std::env::var_os("DIGGER_SETTINGS_SELFTEST").is_some()
}

pub(crate) fn validate_snapshot(image: &window::Screenshot, expected: Size) -> Result<(), String> {
    let width = image.size.width as f32 / image.scale_factor;
    let height = image.size.height as f32 / image.scale_factor;
    if (width - expected.width).abs() > 0.5 || (height - expected.height).abs() > 0.5 {
        return Err(format!(
            "wrong first-frame viewport: {width}x{height}, expected {expected:?}"
        ));
    }
    let pixels: &[u8] = image.rgba.as_ref();
    let center = (image.size.height as usize / 2 * image.size.width as usize
        + image.size.width as usize / 2)
        * 4;
    if pixels.len() <= center + 3
        || pixels[..3].iter().any(|p| *p > 3)
        || !(120..=160).contains(&pixels[3])
        || pixels[center + 3] < 250
    {
        return Err("first frame must contain the settings scrim and opaque card, not the main view or an empty surface".into());
    }
    Ok(())
}
