//! 浏览器面板的展示接缝:库的视图反过来要的页签、分隔条和区域样式,用 Digger 的令牌与 byteui 部件给。
//!
//! Digger 的收藏夹侧栏分隔条 v1 不可拖(固定比例):库把"开始拖"的消息交给宿主,而 Digger 的壳没有对应的
//! 拖拽通路,所以这里只画一条静态竖线,`on_drag` 不接。
use dozer_panel_browser::{BrowserDisplay, BrowserRegion, TabView};
use iced::Element;

pub struct DiggerBrowserDisplay;

impl BrowserDisplay for DiggerBrowserDisplay {
    fn region(&self) -> BrowserRegion {
        let style = super::conversations_display::review_pane_style();
        BrowserRegion {
            background: style.background,
            padding: style.padding,
            gap: style.gap,
        }
    }

    fn locale(&self) -> &str {
        crate::settings::language::code()
    }

    fn line_height_factor(&self) -> f32 {
        // 与预览 / 文件树同一行距。
        1.2
    }

    fn tab_divider<'a, M: 'a>(&self) -> Element<'a, M, iced::Theme, iced::Renderer> {
        byteui::layout::divider::horizontal()
    }

    fn tab<'a, M: Clone + 'a>(
        &self,
        v: TabView,
        on_select: M,
        on_close: M,
        title_hover: impl Fn(bool) -> M + 'a,
        close_hover: impl Fn(bool) -> M + 'a,
    ) -> Element<'a, M, iced::Theme, iced::Renderer> {
        super::tab_chrome::panel_tab(
            v.title,
            v.active,
            v.hover_t,
            v.close_hover_t,
            on_select,
            on_close,
            v.show_tooltip,
            title_hover,
            close_hover,
        )
    }

    fn bookmarks_divider<'a, M: Clone + 'a>(
        &self,
        _on_drag: M,
    ) -> Element<'a, M, iced::Theme, iced::Renderer> {
        byteui::layout::divider::vertical()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 区域样式取自 Digger 的面板令牌(与会话 / 预览面板同一份),不是库或 Dozer 的。
    #[test]
    fn region_projection_uses_digger_tokens() {
        let style = super::super::conversations_display::review_pane_style();
        let region = DiggerBrowserDisplay.region();
        assert_eq!(region.padding, style.padding);
        assert_eq!(region.gap, style.gap);
        assert_eq!(region.background, style.background);
        assert_eq!(
            region.background,
            Some(byteui::theme::color::current().panel)
        );
        assert_eq!(
            DiggerBrowserDisplay.locale(),
            crate::settings::language::code()
        );
    }

    /// 页签必须走 byteui 的统一页签部件,并把库给的悬停进度与 tooltip 标志原样透传
    /// (元素是不透明的,量不出内部;和预览 / 会话适配器一样,用源码扫描钉住接线)。
    #[test]
    fn tab_uses_byteui_panel_tab_and_carries_hover_and_tooltip() {
        let src = include_str!("browser_display.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for required in [
            "super::tab_chrome::panel_tab(",
            "v.hover_t,",
            "v.close_hover_t,",
            "v.show_tooltip,",
            "title_hover,",
            "close_hover,",
        ] {
            assert!(prod.contains(required), "缺少 {required}");
        }
        // 不许另写一套 MouseArea + on_enter/on_exit 的页签。
        assert!(!prod.contains("on_enter"));
    }
}
