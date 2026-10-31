//! Digger 主题偏好:byteui 的 `{"scheme":"dark"|"light"}` 文件格式。
//!
//! 只负责读写与即时应用;共享主题页只产生选择事件。保存失败必须可见并允许重试,
//! 因此这里返回具体错误而不是静默忽略。

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use byteui::theme::color::ColorScheme as Scheme;

/// 第一方网页的主题参数；不向第三方应用注入宿主样式。
pub fn code() -> &'static str {
    match byteui::theme::color::current_snapshot().scheme {
        Scheme::Dark => "dark",
        Scheme::Light => "light",
    }
}

/// 审阅页面沿用共享 CSS 的变量名，通过产品 token 补齐其主题适配。
/// 颜色与 `color-scheme` 取自**同一个** `ThemeSnapshot`，不会出现新颜色配旧方案。
pub fn review_script() -> String {
    let snapshot = byteui::theme::color::current_snapshot();
    let c = snapshot.colors;
    let scheme = match snapshot.scheme {
        Scheme::Dark => "dark",
        Scheme::Light => "light",
    };
    let css_color =
        |v: iced::Color| format!("rgb({:.0} {:.0} {:.0})", v.r * 255., v.g * 255., v.b * 255.);
    let mut css = String::from(":root{");
    for (name, value) in [
        ("bg", c.panel),
        ("gold", c.gold),
        ("cream", c.cream),
        ("cyan", c.cyan),
        ("green", c.green),
        ("red", c.red),
        ("dim", c.dim),
        ("line", c.border),
    ] {
        css.push_str(&format!("--{name}:{};", css_color(value)));
    }
    css.push_str(&format!("color-scheme:{scheme};}}"));
    css.push_str(".msg.AiTurn .bubble .text{color:var(--cream)}");
    css.push_str(&format!(".md-table th,.md-code-block,code,.tool-call-row summary,.tool-call-row .input-json,.tool-result-row .body{{background:{};}}", css_color(c.card)));
    let css = serde_json::to_string(&css).expect("CSS serializes");
    format!(
        "(()=>{{const apply=()=>{{let s=document.getElementById('digger-review-theme');if(!s){{s=document.createElement('style');s.id='digger-review-theme';document.head.append(s);}}s.textContent={css};}};if(document.readyState!=='loading')apply();else document.addEventListener('DOMContentLoaded',apply,{{once:true}});}})();"
    )
}

#[derive(Debug, Serialize, Deserialize)]
struct Persisted {
    scheme: Scheme,
}

/// 从指定路径读取主题。文件不存在时返回 `Ok(None)`(沿用启动默认值);
/// 无法解析时返回错误,由产品回退到默认值并显示提示。
pub fn load_from(path: &Path) -> Result<Option<Scheme>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    serde_json::from_str::<Persisted>(&raw)
        .map(|p| Some(p.scheme))
        .map_err(|e| format!("could not parse {}: {e}", path.display()))
}

/// 写入主题偏好。父目录不存在时创建;任何失败原样返回给调用方展示。
pub fn save_to(path: &Path, scheme: Scheme) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(&Persisted { scheme })
        .map_err(|e| format!("could not serialize theme: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// 启动时读取并应用产品数据根里的主题;失败只回退默认值,不阻断启动。
///
/// 返回可展示的一次性提示(损坏 / 读取失败);成功或文件不存在时返回 `None`。
pub fn init_notice() -> Option<String> {
    match load_from(&crate::settings::theme_path()) {
        Ok(Some(scheme)) => {
            apply(scheme);
            None
        }
        Ok(None) => None,
        Err(reason) => Some(format!(
            "Theme file could not be loaded: {reason}. Using the default theme."
        )),
    }
}

/// 立即应用到进程级 byteui token。主窗、设置窗与所有 iced 视图下一帧自动重绘。
pub fn apply(scheme: Scheme) {
    byteui::theme::color::set_scheme(scheme);
}

/// 保存并即时应用;返回落盘错误供 UI 展示。
pub fn select(scheme: Scheme) -> Option<String> {
    apply(scheme);
    save_to(&crate::settings::theme_path(), scheme).err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_from(&dir.path().join("nope.json")), Ok(None));
    }

    #[test]
    fn round_trips_the_byteui_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/color-theme.json");
        save_to(&path, Scheme::Light).unwrap();
        assert_eq!(load_from(&path), Ok(Some(Scheme::Light)));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"light\""), "byteui 格式: {raw}");
    }

    #[test]
    fn corrupt_file_is_reported_and_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("color-theme.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_from(&path).is_err());
    }

    #[test]
    fn save_failure_is_returned_for_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked");
        std::fs::write(&path, "not a directory").unwrap();
        let result = save_to(&path.join("color-theme.json"), Scheme::Dark);
        assert!(result.is_err(), "在普通文件下面建目录必须失败");
    }
}
