//! 预览页签栏的 **Digger 部件**(S5b-2 P4 Task 6)。
//!
//! 页签、溢出入口与溢出下拉的渲染已迁到 `byteui::interaction::panel_tabs`(X1,与 Dozer 共用;
//! 原先这里抄过一份 Dozer 的 `chrome/tab_widget.rs`)。本文件只剩 Digger 自己的接线:
//!
//! - 不带 Dozer 的悬停动画表:Digger 的 D2 约定是「无 hover 动画」——`hover_t` 仍按接口透传,
//!   调用方恒传 0(见 [`crate::adapters::preview_display`]),于是页签的底色与文字色都取静止态;
//!   交互反馈只剩 `tabs::tab_core` 自带的指针光标与 `button::Status` 驱动的关闭按钮变色。
//! - 页签的布局数学(窗口翻页、溢出判定)在库里,这里只画单个页签 / 溢出入口 / 下拉,不参与任何
//!   宽度决策;宽度估算 [`tab_display_width`] 是库反过来要的那一份(估算而非测量,CJK 按全宽计)。
//! - 预览模式切换按钮 [`mode_button`] 是 Digger 自己的(Dozer 的同类按钮依赖 Dozer 国际化)。
//!
//! 提示气泡现在是 byteui 的统一样式(PANEL 底、描边圆角 6、CREAM 12px),与 Dozer 一致;原先这里
//! 用的是 `container::rounded_box`,外观不同。

use byteui::interaction::icons;
use byteui::interaction::panel_tabs::{self, PanelTabArgs, TabOverflowEntry, TabOverflowMenuArgs};
use iced_widget::core::Element;

/// 文本显示宽度的基础单元和:CJK 字符按全宽计 2,其余按半宽计 1。
pub fn text_width_units(s: &str) -> f32 {
    s.chars()
        .map(|c| if (c as u32) > 0x2E80 { 2.0 } else { 1.0 })
        .sum()
}

/// 预览页签的估算显示宽(逻辑像素):名称 + 关闭 × + 胶囊内边距的粗估。
/// 不追求精确——估偏几像素只会让翻页边界差一个页签。宽度随标题实际长度
/// 增长(无上限),与渲染侧按内容伸缩对齐。
pub fn tab_display_width(title: &str) -> f32 {
    text_width_units(title) * 8.0 + 18.0 + 12.0
}

/// 一个横向页签。`on_select`/`on_close` 由库的 `TabArgs` 给出,已经是产品顶层消息;两个 `hover`
/// 闭包在 Digger 恒产出同一个 `Noop`(见模块文档)。渲染委托给 byteui 的 `panel_tab`,无前缀/后缀。
#[allow(clippy::too_many_arguments)] // 参数逐项对应库的 `TabArgs`,不再另立结构体。
pub fn panel_tab<'a, M, F1, F2>(
    title: String,
    active: bool,
    hover_t: f32,
    close_hover_t: f32,
    on_select: M,
    on_close: M,
    show_tooltip: bool,
    title_hover: F1,
    close_hover: F2,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
    F1: Fn(bool) -> M + 'a,
    F2: Fn(bool) -> M + 'a,
{
    panel_tabs::panel_tab(PanelTabArgs {
        title,
        active,
        hover_t,
        close_hover_t,
        prefix: None,
        suffix: None,
        on_select,
        on_close,
        show_tooltip,
        title_hover,
        close_hover,
    })
}

/// 溢出下拉入口:`ChevronsDown` 标准图标按钮。页签组非空时总显示(即使当前没有横向溢出,下拉也列出整组
/// 页签);空组返回 `None`。
pub fn overflow_button<'a, M: Clone + 'a>(
    tab_count: usize,
    hover_t: f32,
    on_press: M,
    on_hover: impl Fn(bool) -> M + 'a,
) -> Option<Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>> {
    panel_tabs::tab_overflow_button(tab_count, hover_t, on_press, on_hover)
}

/// 页签栏右侧的模式切换按钮(预览/代码、网格/原文、树/文本三种)。
pub fn mode_button<'a, M: Clone + 'a>(
    kind: icons::IconKind,
    tooltip: impl Into<std::borrow::Cow<'a, str>>,
    hover_t: f32,
    on_press: M,
    on_hover: impl Fn(bool) -> M + 'a,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer> {
    icons::icon_button_entry(
        kind,
        byteui::theme::icon_size::row(),
        false,
        false,
        hover_t,
        false,
        byteui::theme::icon_size::row() + 6.0,
        true,
        on_press,
        on_hover,
        tooltip,
    )
}

/// 溢出下拉的一行(对应组里某个页签)。
pub struct OverflowEntry {
    pub index: usize,
    pub title: String,
    pub active: bool,
    /// `Blank` 占位(fallback 页)固定存在、不可关闭,这一行不画 ×。
    pub closable: bool,
    pub hover_t: f32,
}

/// 溢出下拉:每行一个页签,选中行标题前画 CREAM 的 `>`(未选中行留空,文字左对齐);关闭 × 恒可点
/// (下拉本就是"主动点开来看"的场景)。返回的元素带一层全窗透明遮罩接「点外部关闭」,调用方必须把它
/// 提到视图顶层 `stack` 里(锚点与窗口尺寸都是全窗坐标)。渲染委托给 byteui 的 `tab_overflow_menu`。
pub fn overflow_menu<'a, M, FSel, FClose, FRH>(
    entries: Vec<OverflowEntry>,
    anchor: (f32, f32),
    window_size: (f32, f32),
    on_select: FSel,
    on_close: FClose,
    on_dismiss: M,
    on_row_hover: FRH,
) -> Element<'a, M, iced_widget::Theme, iced_renderer::Renderer>
where
    M: Clone + 'a,
    FSel: Fn(usize) -> M + 'a,
    FClose: Fn(usize) -> M + 'a,
    FRH: Fn(usize, bool) -> M + 'a,
{
    panel_tabs::tab_overflow_menu(TabOverflowMenuArgs {
        entries: entries
            .into_iter()
            .map(|e| TabOverflowEntry {
                index: e.index,
                prefix: None,
                title: e.title,
                active: e.active,
                closable: e.closable,
                hover_t: e.hover_t,
            })
            .collect(),
        anchor,
        window_size,
        on_select,
        on_close,
        on_dismiss,
        on_row_hover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_display_width_counts_cjk_as_full_width() {
        // "ab" = 2 个半宽单元;"中文" = 2 个全宽单元 = 4。
        let ascii = tab_display_width("ab");
        let cjk = tab_display_width("中文");
        assert_eq!(text_width_units("ab"), 2.0);
        assert_eq!(text_width_units("中文"), 4.0);
        assert_eq!(cjk - ascii, 2.0 * 8.0);
        // 空标题只剩 × 与内边距。
        assert_eq!(tab_display_width(""), 30.0);
    }

    #[test]
    fn tab_display_width_grows_with_the_title() {
        assert!(tab_display_width("a_very_long_file_name.rs") > tab_display_width("a.rs"));
    }

    #[test]
    fn overflow_entries_mark_the_blank_tab_as_not_closable() {
        // `closable` 由库的 `OverflowEntry` 原样带过来;这里钉住 Digger 的
        // 下拉行数据形状:Blank 占位是第 0 项且不画 ×。
        let entries = [
            OverflowEntry {
                index: 0,
                title: String::new(),
                active: false,
                closable: false,
                hover_t: 0.0,
            },
            OverflowEntry {
                index: 1,
                title: "a.rs".to_string(),
                active: true,
                closable: true,
                hover_t: 0.0,
            },
        ];
        assert!(!entries[0].closable);
        assert!(entries[1].closable);
        // 空组不出溢出入口;非空组出。
        assert!(
            overflow_button::<bytehost_chrome::ShellMsg>(
                0,
                0.0,
                bytehost_chrome::ShellMsg::Noop,
                |_| bytehost_chrome::ShellMsg::Noop,
            )
            .is_none()
        );
        assert!(
            overflow_button::<bytehost_chrome::ShellMsg>(
                1,
                0.0,
                bytehost_chrome::ShellMsg::Noop,
                |_| bytehost_chrome::ShellMsg::Noop,
            )
            .is_some()
        );
    }
}

#[cfg(test)]
mod structure_tests {
    use super::*;

    #[test]
    fn the_tab_widget_tree_shape_does_not_depend_on_show_tooltip() {
        // 到期才把页签包进 `Tooltip`,控件树结构翻转,里面 `MouseArea` 的状态被重置成"未悬停":
        // 下一帧重发 enter(重置计时,气泡闪一下就没),翻转那一帧光标若已离开则 exit 永不触发
        // (悬停底色与 × 卡住)。结构必须恒定,延迟交给 `Tooltip::delay`。
        let shape = |show: bool| {
            let el = panel_tab(
                "notes.txt".to_string(),
                false,
                0.0,
                0.0,
                (),
                (),
                show,
                |_| (),
                |_| (),
            );
            (el.as_widget().tag(), el.as_widget().children().len())
        };
        assert_eq!(shape(true), shape(false));
    }
}
