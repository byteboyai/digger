//! Digger 对共享窗口骨架(`bytehost-chrome`)的适配:面板 id、清单、默认布局、图标与标题、落盘路径。
//!
//! 骨架以 `PanelId` 为键、不理解任何具体面板;这里是产品给它的全部输入。面板 body 的渲染在 `product.rs`
//! (`ShellHost` 实现),因为 body 要读各面板自己的状态。

use std::path::Path;

use bytehost_chrome::{Ctx, Defaults, Loaded, Outcome, PanelMeta, PanelRules, ShellModel, ShellMsg, metrics_from_theme, persist};
use bytehost_core::id::PanelId;
use byteui::interaction::icons::IconKind;

fn id(s: &str) -> PanelId {
    PanelId::new(s).expect("合法 panel id")
}

/// 产品信息 / 状态面板。
pub fn info_id() -> PanelId {
    id("digger.info")
}
/// 应用管理面板。
pub fn apps_id() -> PanelId {
    id("digger.apps")
}
/// 用量面板(共享 `dozer-panel-usage` 库)。
pub fn usage_id() -> PanelId {
    id("digger.usage")
}
/// Markdown 编辑器面板(ADR-013)。
pub fn markdown_id() -> PanelId {
    id("digger.markdown")
}

/// 产品的全部面板。落盘时清单里有而文件里没有的面板,会回到它的默认栏(新版本新增面板不会重置用户布局)。
pub fn all_panels() -> Vec<PanelId> {
    vec![markdown_id(), info_id(), apps_id(), usage_id()]
}

/// 图标栏上的图标与悬停标题。
pub fn meta(panel: &PanelId) -> Option<PanelMeta> {
    let (title, icon) = match panel.as_str() {
        "digger.markdown" => ("编辑器", IconKind::FileText),
        "digger.info" => ("信息", IconKind::SquareActivity),
        "digger.apps" => ("应用", IconKind::LayoutList),
        "digger.usage" => ("用量", IconKind::BarChart3),
        _ => return None,
    };
    Some(PanelMeta { title: title.to_owned(), icon })
}

/// 面板的配对规则。S1 里 Digger 还没有"列表 | 内容"配对的面板(那要等 S2 起的共享面板),所以都是 `None`。
pub fn rules(_panel: &PanelId) -> Option<PanelRules> {
    None
}

/// 默认布局:沿用 Dozer 的分工,左区放内容型面板、右区放辅助型面板。左宽 640 与 Dozer 一致。
pub fn defaults() -> Defaults {
    let (w, h) = byteui::theme::geometry::initial_window_size();
    Defaults {
        rails: [vec![markdown_id(), info_id()], vec![apps_id(), usage_id()]],
        selected: [markdown_id(), usage_id()],
        left_width: 640.0,
        window: (w, h),
        min_window: (byteui::theme::geometry::min_window_width(), byteui::theme::geometry::min_window_height()),
    }
}

/// 读盘:文件不存在、损坏、或布局无法修复时用默认值;更高 schema 版本的文件拒绝覆盖(见 [`Loaded::may_save`])。
pub fn load(path: &Path) -> Loaded {
    let json = std::fs::read_to_string(path).ok();
    persist::load(json.as_deref(), &all_panels(), &defaults(), &metrics_from_theme())
}

/// 把一条骨架消息交给模型,度量与面板规则取产品的。
pub fn update(model: &mut ShellModel, msg: ShellMsg) -> Outcome {
    let metrics = metrics_from_theme();
    model.update(msg, &Ctx { metrics: &metrics, rules: &rules })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Side;

    #[test]
    fn panel_ids_are_the_persisted_wire_names_and_must_not_change() {
        assert_eq!(info_id().as_str(), "digger.info");
        assert_eq!(apps_id().as_str(), "digger.apps");
        assert_eq!(usage_id().as_str(), "digger.usage");
        assert_eq!(markdown_id().as_str(), "digger.markdown");
    }

    #[test]
    fn every_panel_has_an_icon_and_a_title_and_unknown_ids_have_none() {
        for p in all_panels() {
            let m = meta(&p).unwrap_or_else(|| panic!("{p} has no meta"));
            assert!(!m.title.is_empty());
        }
        assert!(meta(&id("digger.nope")).is_none());
    }

    #[test]
    fn the_default_layout_is_legal_and_covers_every_panel_exactly_once() {
        let d = defaults();
        let model = ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).expect("legal default layout");
        let mut seen: Vec<PanelId> = model.rail(Side::Left).iter().chain(model.rail(Side::Right)).cloned().collect();
        seen.sort();
        let mut want = all_panels();
        want.sort();
        assert_eq!(seen, want);
        assert_eq!(model.rail(Side::Left), [markdown_id(), info_id()], "content panels on the left, like Dozer");
        assert_eq!(model.rail(Side::Right), [apps_id(), usage_id()]);
        assert_eq!(model.selected(Side::Left), &markdown_id());
        assert_eq!(model.selected(Side::Right), &usage_id());
        assert_eq!(d.left_width, 640.0);
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_may_be_saved() {
        let dir = std::env::temp_dir().join(format!("digger-shell-missing-{}", std::process::id()));
        let loaded = load(&dir.join("shell.json"));
        assert!(matches!(loaded, Loaded::Fresh(_)));
        assert!(loaded.may_save());
    }

    /// 更高版本的文件退出时不得被覆盖:读到 `Newer` 就不能再写。
    #[test]
    fn a_newer_schema_file_is_left_byte_for_byte_alone() {
        let dir = std::env::temp_dir().join(format!("digger-shell-newer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&path, original).unwrap();
        let loaded = load(&path);
        assert!(!loaded.may_save());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panel_added_in_a_later_version_returns_to_its_default_side_without_resetting_the_user_layout() {
        let dir = std::env::temp_dir().join(format!("digger-shell-added-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        // an older file that predates the usage panel and has the user's own order on the left
        std::fs::write(&path, r#"{ "left": ["digger.info", "digger.markdown"], "right": ["digger.apps"], "left_width": 500.0 }"#).unwrap();
        let model = load(&path).into_model();
        assert_eq!(model.rail(Side::Left), [info_id(), markdown_id()], "the user's order is kept");
        assert_eq!(model.rail(Side::Right), [apps_id(), usage_id()], "the new panel lands on its default side");
        assert_eq!(model.left_width(), 500.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
