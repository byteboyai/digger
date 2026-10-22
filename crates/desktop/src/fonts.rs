pub const CODE_FONT_FAMILY: &str = "JetBrains Mono";
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono[wght].ttf");
pub fn code_font() -> iced::Font {
    iced::Font::with_name(CODE_FONT_FAMILY)
}
pub fn terminal_style() -> dozer_panel_term::style::TermStyle {
    dozer_panel_term::style::TermStyle {
        font: code_font(),
        font_size: 14.0,
        line_height_factor: 1.2,
    }
}
pub fn sanitize_font_db() {
    let mut fs = iced_wgpu::graphics::text::font_system()
        .write()
        .expect("font system");
    let ids: Vec<_> = fs
        .raw()
        .db()
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.contains("GB18030"))
        })
        .map(|face| face.id)
        .collect();
    let db = fs.raw().db_mut();
    for id in ids {
        db.remove_face(id);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_font_is_jetbrains_mono() {
        assert_eq!(
            code_font().family,
            iced::font::Family::Name("JetBrains Mono")
        );
    }
    #[test]
    fn embedded_font_bytes_are_a_truetype_font_that_names_itself_jetbrains_mono() {
        assert!(
            JETBRAINS_MONO.starts_with(&[0, 1, 0, 0]) || JETBRAINS_MONO.starts_with(b"true"),
            "not a TrueType header"
        );
        let ascii = b"JetBrains Mono";
        let utf16: Vec<u8> = "JetBrains Mono"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        let has = |needle: &[u8]| JETBRAINS_MONO.windows(needle.len()).any(|w| w == needle);
        assert!(has(ascii) || has(&utf16));
    }
    #[test]
    fn terminal_style_matches_dozers_terminal_json() {
        let s = terminal_style();
        assert_eq!((s.font_size, s.line_height_factor), (14.0, 1.2));
        // 与 Dozer 同公式(dozer-panel-term::TermStyle::grid_size,UI scale 1.0):
        // 600 / (14 × 0.6) = 71.4 → 71 列;400 / (14 × 1.2) = 23.8 → 23 行。
        assert_eq!(s.grid_size(600.0, 400.0), (71, 23));
    }
    #[test]
    fn sanitize_font_db_is_idempotent() {
        sanitize_font_db();
        sanitize_font_db();
    }
}
