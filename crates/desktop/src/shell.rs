//! Digger 对共享窗口骨架(`bytehost-chrome`)的适配:面板 id、清单、默认布局、图标与标题、落盘路径。
//!
//! 骨架以 `PanelId` 为键、不理解任何具体面板;这里是产品给它的全部输入。面板 body 的渲染在 `product.rs`
//! (`ShellHost` 实现),因为 body 要读各面板自己的状态。

use std::path::Path;

use bytehost_chrome::{
    Ctx, Defaults, Loaded, Outcome, PairRects, PaneRules, PanelMeta, PanelRules, ShellModel,
    ShellMsg, Side, metrics_from_theme, persist,
};
use bytehost_core::id::PanelId;
use bytehost_desktop::PaneSpec;
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
/// 项目面板(共享 `dozer-panel-project` 库):当前项目的信息、描述、文档链接与共享记忆。
pub fn project_id() -> PanelId {
    id("digger.project")
}

/// 产品的全部面板。落盘时清单里有而文件里没有的面板,会回到它的默认栏(新版本新增面板不会重置用户布局)。
pub fn all_panels() -> Vec<PanelId> {
    vec![
        markdown_id(),
        info_id(),
        apps_id(),
        usage_id(),
        project_id(),
    ]
}

/// 图标栏上的图标与悬停标题。
pub fn meta(panel: &PanelId) -> Option<PanelMeta> {
    let (title, icon) = match panel.as_str() {
        "digger.markdown" => ("编辑器", IconKind::FileText),
        "digger.info" => ("信息", IconKind::SquareActivity),
        "digger.apps" => ("应用", IconKind::LayoutList),
        "digger.usage" => ("用量", IconKind::BarChart3),
        "digger.project" => ("项目", IconKind::Briefcase),
        _ => return None,
    };
    Some(PanelMeta {
        title: title.to_owned(),
        icon,
    })
}

/// 配对里"列表"那一块的最小宽度:再窄就收起,免得底部按钮行被挤爆。
/// 与 Dozer `extensions/project/view.rs` 的 `footer_min_width()` 同一公式(两颗按钮 + 间距)。
pub fn list_min_width() -> f32 {
    let icon = byteui::theme::icon_size::row();
    let label_px = byteui::theme::font::label() as f32;
    let button_w = icon + 6.0 + label_px * 4.0 + 16.0;
    button_w * 2.0 + 6.0 + 16.0
}

/// 面板的配对规则。S2 起用量是"内容 | agent 列表"配对,与 Dozer 的用量一致:列表是第一块,默认渲染在内容之后
/// (所以在右栏时是"内容 | 列表",拖到左栏镜像成"列表 | 内容"),可拖窄收起。其余面板仍是单栏。
pub fn rules(panel: &PanelId) -> Option<PanelRules> {
    match panel.as_str() {
        // 一期单栏:配对(预览)留给 S5b。
        "digger.usage" => Some(PanelRules {
            default_side: Side::Right,
            pane: Some(PaneRules {
                spec: PaneSpec::new(false, true),
                default_split: byteui::theme::geometry::default_split_ratio(),
                min_first_px: list_min_width(),
            }),
        }),
        _ => None,
    }
}

/// 某侧当前面板的配对矩形(度量与规则取产品的);该侧面板不是配对面板时为 `None`。
pub fn pair_rects(model: &ShellModel, side: Side) -> Option<PairRects> {
    let metrics = metrics_from_theme();
    model.pair_rects(
        side,
        &Ctx {
            metrics: &metrics,
            rules: &rules,
        },
    )
}

/// 默认布局:沿用 Dozer 的分工,左区放内容型面板、右区放辅助型面板。左宽 640 与 Dozer 一致。
pub fn defaults() -> Defaults {
    let (w, h) = byteui::theme::geometry::initial_window_size();
    Defaults {
        rails: [
            vec![project_id(), markdown_id(), info_id()],
            vec![apps_id(), usage_id()],
        ],
        selected: [markdown_id(), usage_id()],
        left_width: 640.0,
        window: (w, h),
        min_window: (
            byteui::theme::geometry::min_window_width(),
            byteui::theme::geometry::min_window_height(),
        ),
    }
}

/// 读盘:文件不存在、损坏、或布局无法修复时用默认值;更高 schema 版本的文件拒绝覆盖(见 [`Loaded::may_save`])。
pub fn load(path: &Path) -> Loaded {
    load_scoped(path, None)
}

/// 按项目读盘:`project` 指向该项目自己的布局文件时读它,否则读全局 `path`(两者都没有就用默认值)。
///
/// 项目文件不存在、损坏或版本更高时**不**回落全局布局:项目有文件才算"该项目的布局";文件在但不可用
/// 按骨架自己的规则处理(损坏 → 默认;版本更高 → 不覆盖)。全新项目(没有文件)以全局布局为初值,
/// 首次保存才写项目文件,全局文件字节不动。
pub fn load_scoped(path: &Path, project: Option<&Path>) -> Loaded {
    let Some(project_path) = project.filter(|p| p.exists()) else {
        return load_file(path);
    };
    // 项目的布局文件存在但解析失败:这是该项目的布局,不能悄悄借用全局那份。
    load_file(project_path)
}

fn load_file(path: &Path) -> Loaded {
    let json = std::fs::read_to_string(path)
        .ok()
        .map(|j| project_first_on_left(&j));
    persist::load(
        json.as_deref(),
        &all_panels(),
        &defaults(),
        &metrics_from_theme(),
    )
}

/// 旧布局文件里没有 Project 面板:读盘前把它放到左栏最前面(S5a D5)。骨架对"清单里有而文件里没有"
/// 的面板只会追加到默认栏末尾,所以这一步在宿主做。文件已经提到它、或解析不了,原样返回。
fn project_first_on_left(json: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_owned();
    };
    let wire = project_id();
    let mentions = |side: &str| {
        value[side]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|i| i.as_str() == Some(wire.as_str())))
    };
    if mentions("left") || mentions("right") {
        return json.to_owned();
    }
    let Some(left) = value.get_mut("left").and_then(|l| l.as_array_mut()) else {
        return json.to_owned();
    };
    left.insert(0, serde_json::Value::String(wire.as_str().to_owned()));
    value.to_string()
}

/// 只把窗口大小写进全局布局文件,其余字段保持原样。文件还不存在时整份写(它之后作为新项目的初始布局)。
/// 文件在但解析不了:不碰它(损坏的文件由读盘路径按默认值处理,这里不替用户改写)。
pub fn save_window_size(path: &Path, model: &ShellModel) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return persist::save_atomic(path, model);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    let (w, h) = model.window();
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    object.insert("window_width".into(), serde_json::json!(w));
    object.insert("window_height".into(), serde_json::json!(h));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&value)?)?;
    std::fs::rename(&tmp, path)
}

/// 把一条骨架消息交给模型,度量与面板规则取产品的。
pub fn update(model: &mut ShellModel, msg: ShellMsg) -> Outcome {
    let metrics = metrics_from_theme();
    model.update(
        msg,
        &Ctx {
            metrics: &metrics,
            rules: &rules,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytehost_chrome::Divider;

    #[test]
    fn panel_ids_are_the_persisted_wire_names_and_must_not_change() {
        assert_eq!(info_id().as_str(), "digger.info");
        assert_eq!(apps_id().as_str(), "digger.apps");
        assert_eq!(usage_id().as_str(), "digger.usage");
        assert_eq!(markdown_id().as_str(), "digger.markdown");
        assert_eq!(project_id().as_str(), "digger.project");
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
        let model = ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window)
            .expect("legal default layout");
        let mut seen: Vec<PanelId> = model
            .rail(Side::Left)
            .iter()
            .chain(model.rail(Side::Right))
            .cloned()
            .collect();
        seen.sort();
        let mut want = all_panels();
        want.sort();
        assert_eq!(seen, want);
        assert_eq!(
            model.rail(Side::Left),
            [project_id(), markdown_id(), info_id()],
            "project first, then the content panels, like Dozer"
        );
        assert_eq!(model.rail(Side::Right), [apps_id(), usage_id()]);
        assert_eq!(model.selected(Side::Left), &markdown_id());
        assert_eq!(model.selected(Side::Right), &usage_id());
        assert_eq!(d.left_width, 640.0);
    }

    #[test]
    fn a_layout_that_already_places_the_project_panel_is_not_reordered() {
        let dir = std::env::temp_dir().join(format!("digger-shell-placed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        std::fs::write(
            &path,
            r#"{ "left": ["digger.info", "digger.markdown"], "right": ["digger.apps", "digger.usage", "digger.project"] }"#,
        )
        .unwrap();
        let model = load(&path).into_model();
        assert_eq!(model.rail(Side::Left), [info_id(), markdown_id()]);
        assert_eq!(
            model.rail(Side::Right),
            [apps_id(), usage_id(), project_id()]
        );
        let _ = std::fs::remove_dir_all(&dir);
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
    fn a_panel_added_in_a_later_version_returns_to_its_default_side_without_resetting_the_user_layout()
     {
        let dir = std::env::temp_dir().join(format!("digger-shell-added-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shell.json");
        // an older file that predates the usage panel and has the user's own order on the left
        std::fs::write(&path, r#"{ "left": ["digger.info", "digger.markdown"], "right": ["digger.apps"], "left_width": 500.0 }"#).unwrap();
        let model = load(&path).into_model();
        assert_eq!(
            model.rail(Side::Left),
            [project_id(), info_id(), markdown_id()],
            "the user's order is kept, the project panel lands first"
        );
        assert_eq!(
            model.rail(Side::Right),
            [apps_id(), usage_id()],
            "the new panels land on their default side"
        );
        assert_eq!(model.left_width(), 500.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_usage_panel_is_a_pair_and_it_matches_dozers_usage_pane() {
        let r = rules(&usage_id()).expect("usage has rules");
        assert_eq!(
            r.default_side,
            Side::Right,
            "usage lives on the right by default, like Dozer"
        );
        let pane = r.pane.expect("usage is a pair");
        // 列表是第一块(默认渲染在内容之后),且可拖窄收起
        assert_eq!(pane.spec, bytehost_desktop::PaneSpec::new(false, true));
        assert_eq!(
            pane.default_split,
            byteui::theme::geometry::default_split_ratio()
        );
        assert_eq!(pane.min_first_px, list_min_width());
        for p in [markdown_id(), info_id(), apps_id()] {
            assert!(rules(&p).is_none(), "{p} stays a single-column panel in S2");
        }
    }

    #[test]
    fn the_list_min_width_is_dozers_footer_formula_at_the_default_theme() {
        // 默认主题(缩放 1.0):行图标 14、标签字号 13。
        // 按钮宽 = 14 + 6 + 13*4 + 16 = 88;最小宽 = 88*2 + 6 + 16 = 198。
        assert_eq!(list_min_width(), 198.0);
    }

    #[test]
    fn pair_rects_for_usage_split_the_right_zone_with_the_list_second_and_the_content_first() {
        let d = defaults();
        let model = ShellModel::new(
            d.rails.clone(),
            d.selected.clone(),
            d.left_width,
            (1200.0, 800.0),
        )
        .unwrap();
        let p = pair_rects(&model, Side::Right).expect("the right side shows usage");
        // zones = 1200-88-8 = 1104,左区 640 ⇒ 右区 464;右区内容:x 44+640+8+1=693,宽 464-6-2=456;
        // 配对宽 456-8=448,列表占 0.35 ⇒ 156.8,内容 291.2
        assert!(
            (p.second.x - 693.0).abs() < 1e-3,
            "content first, on the left"
        );
        assert!((p.second.w - 291.2).abs() < 1e-3);
        assert!((p.first.w - 156.8).abs() < 1e-3);
        assert!(
            p.first.x > p.second.x,
            "the list is to the right of the content"
        );
        assert!(
            pair_rects(&model, Side::Left).is_none(),
            "the editor is not a pair"
        );
    }

    // ---- 布局按项目一份(Task 9) ----

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("digger-shell-scoped-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn drag_left_width_to(model: &mut ShellModel, x: f32) {
        update(model, ShellMsg::DividerPress(Divider::Zones));
        update(model, ShellMsg::CursorMoved { x, y: 300.0 });
        update(model, ShellMsg::PointerReleased);
    }

    #[test]
    fn with_no_project_the_global_layout_is_used() {
        let dir = tmp_dir("global");
        let global = dir.join("shell.json");
        let mut saved = defaults_layout();
        drag_left_width_to(&mut saved, 544.0);
        persist::save_atomic(&global, &saved).unwrap();
        let loaded = load_scoped(&global, None);
        assert!(matches!(loaded, Loaded::Restored(_)));
        assert_eq!(loaded.model().left_width(), 500.0, "x minus the rail");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn defaults_layout() -> ShellModel {
        let d = defaults();
        ShellModel::new(d.rails.clone(), d.selected.clone(), d.left_width, d.window).unwrap()
    }

    #[test]
    fn first_save_for_a_project_writes_its_own_file_and_leaves_the_global_one_untouched() {
        let dir = tmp_dir("first-save");
        let global = dir.join("shell.json");
        let project = dir.join("shell-projects").join("7.json");
        let mut g = defaults_layout();
        drag_left_width_to(&mut g, 544.0);
        persist::save_atomic(&global, &g).unwrap();
        let global_bytes = std::fs::read(&global).unwrap();

        // 项目还没有自己的文件:以全局布局为初值(不复制文件)。
        let loaded = load_scoped(&global, Some(&project));
        assert_eq!(loaded.model().left_width(), 500.0);
        assert!(!project.exists(), "loading never writes the project file");

        // 第一次保存写项目文件;全局文件字节不动。
        let mut own = loaded.into_model();
        drag_left_width_to(&mut own, 744.0);
        persist::save_atomic(&project, &own).unwrap();
        assert_eq!(std::fs::read(&global).unwrap(), global_bytes);
        let again = load_scoped(&global, Some(&project));
        assert_eq!(again.model().left_width(), 700.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_between_projects_swaps_the_layout() {
        let dir = tmp_dir("switch");
        let global = dir.join("shell.json");
        let a_path = dir.join("a.json");
        let b_path = dir.join("b.json");
        // A 的左宽改成 500,并保存;全局写另一份宽度 400。
        let mut a = defaults_layout();
        drag_left_width_to(&mut a, 544.0);
        persist::save_atomic(&a_path, &a).unwrap();
        let mut g = defaults_layout();
        drag_left_width_to(&mut g, 444.0);
        persist::save_atomic(&global, &g).unwrap();

        assert_eq!(
            load_scoped(&global, Some(&a_path)).model().left_width(),
            500.0
        );
        // B 没有文件:拿到全局的初值,而不是 A 的。
        let b = load_scoped(&global, Some(&b_path));
        assert_eq!(b.model().left_width(), 400.0);
        // 切回 A 仍是 A 的。
        assert_eq!(
            load_scoped(&global, Some(&a_path)).model().left_width(),
            500.0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_schema_project_file_is_never_overwritten() {
        let dir = tmp_dir("newer-project");
        let global = dir.join("shell.json");
        let project = dir.join("future.json");
        let original = r#"{ "schema_version": 999, "left": ["digger.info"], "right": [] }"#;
        std::fs::write(&project, original).unwrap();
        let loaded = load_scoped(&global, Some(&project));
        assert!(matches!(loaded, Loaded::Newer { .. }));
        assert!(!loaded.may_save());
        assert_eq!(std::fs::read_to_string(&project).unwrap(), original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_the_window_size_leaves_every_other_global_field_alone() {
        let dir = tmp_dir("window-size");
        let global = dir.join("shell.json");
        std::fs::write(
            &global,
            r#"{ "left": ["digger.info"], "right": ["digger.apps"], "left_width": 500.0, "window_width": 1.0, "window_height": 2.0 }"#,
        )
        .unwrap();
        let model = defaults_layout();
        save_window_size(&global, &model).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&global).unwrap()).unwrap();
        assert_eq!(v["left"], serde_json::json!(["digger.info"]));
        assert_eq!(v["left_width"], serde_json::json!(500.0));
        assert_eq!(v["window_width"], serde_json::json!(model.window().0));
        // 没有文件时整份写出。
        let fresh = dir.join("fresh.json");
        save_window_size(&fresh, &model).unwrap();
        assert!(fresh.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_global_layout_seeds_a_project_without_losing_it() {
        let dir = tmp_dir("legacy");
        let global = dir.join("shell.json");
        let project = dir.join("p.json");
        // 旧版本的全局文件(只有部分字段)。
        std::fs::write(
            &global,
            r#"{ "left": ["digger.info", "digger.markdown"], "right": ["digger.apps"], "left_width": 500.0 }"#,
        )
        .unwrap();
        let global_bytes = std::fs::read(&global).unwrap();
        let mut first = load_scoped(&global, Some(&project)).into_model();
        assert_eq!(first.left_width(), 500.0, "the user's width is kept");
        assert_eq!(
            first.rail(Side::Left),
            [project_id(), info_id(), markdown_id()]
        );
        assert_eq!(first.rail(Side::Right), [apps_id(), usage_id()]);
        drag_left_width_to(&mut first, 744.0);
        persist::save_atomic(&project, &first).unwrap();
        assert_eq!(std::fs::read(&global).unwrap(), global_bytes);
        let rel = load_scoped(&global, Some(&project));
        assert_eq!(rel.model().left_width(), 700.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removed_projects_layout_file_is_ignored() {
        let dir = tmp_dir("removed");
        let global = dir.join("shell.json");
        let mut g = defaults_layout();
        drag_left_width_to(&mut g, 444.0);
        persist::save_atomic(&global, &g).unwrap();
        // 注册表里已经没有 id=7:调用方传 None,读到的就是全局布局。
        let loaded = load_scoped(&global, None);
        assert_eq!(loaded.model().left_width(), 400.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
