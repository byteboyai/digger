//! 界面语言偏好独立存放在 Digger 数据根；共享视图只读取已成功保存的语言。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

// 低位是语言，其余位是显示代数。一次读取即可得到一致的 webview stamp。
static CURRENT: AtomicU64 = AtomicU64::new(1);

pub fn code() -> &'static str {
    if CURRENT.load(Ordering::Relaxed) & 1 == 1 {
        "zh-CN"
    } else {
        "en"
    }
}

pub fn stamp() -> dozer_model::LocaleStamp {
    let value = CURRENT.load(Ordering::Relaxed);
    dozer_model::LocaleStamp {
        ui_locale: dozer_model::Locale(if value & 1 == 1 { "zh-CN" } else { "en" }.into()),
        locale_revision: value >> 1,
    }
}

fn resolve(preference: Preference, system: &str) -> &'static str {
    match preference {
        Preference::English => "en",
        Preference::SimplifiedChinese => "zh-CN",
        Preference::System => {
            let system = system.to_ascii_lowercase().replace('_', "-");
            if system == "zh"
                || system.starts_with("zh-hans")
                || system.starts_with("zh-cn")
                || system.starts_with("zh-sg")
            {
                "zh-CN"
            } else {
                "en"
            }
        }
    }
}

fn system_language() -> String {
    #[cfg(target_os = "macos")]
    {
        let languages = objc2_foundation::NSLocale::preferredLanguages();
        if languages.count() > 0 {
            return languages.objectAtIndex(0).to_string();
        }
    }
    std::env::var("LC_ALL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_else(|| "en".into())
}

pub fn apply(preference: Preference) {
    let chinese = u64::from(resolve(preference, &system_language()) == "zh-CN");
    let _ = CURRENT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
        Some(((old >> 1) + 1) << 1 | chinese)
    });
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    preference: Preference,
}

pub fn load_from(path: &Path) -> Result<Preference, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<Persisted>(&raw)
            .map(|p| p.preference)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preference::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_to(path: &Path, preference: Preference) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_vec_pretty(&Persisted { preference }).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

pub fn path() -> std::path::PathBuf {
    crate::paths::config_dir().join("language.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_missing_file_follows_system() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/language.json");
        assert_eq!(load_from(&path), Ok(Preference::System));
        for preference in [
            Preference::System,
            Preference::English,
            Preference::SimplifiedChinese,
        ] {
            save_to(&path, preference).unwrap();
            assert_eq!(load_from(&path), Ok(preference));
        }
    }

    #[test]
    fn invalid_file_and_failed_save_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked");
        std::fs::write(&path, "invalid").unwrap();
        assert!(load_from(&path).is_err());
        assert!(save_to(&path.join("language.json"), Preference::English).is_err());
    }

    #[test]
    fn system_language_resolves_supported_languages_and_explicit_choices_win() {
        for system in ["zh-Hans-CN", "zh_CN.UTF-8", "zh-SG", "zh"] {
            assert_eq!(resolve(Preference::System, system), "zh-CN");
            assert_eq!(resolve(Preference::English, system), "en");
        }
        for system in ["en-US", "fr-FR", "zh-Hant-TW", "", "C"] {
            assert_eq!(resolve(Preference::System, system), "en");
            assert_eq!(resolve(Preference::SimplifiedChinese, system), "zh-CN");
        }
    }
}
