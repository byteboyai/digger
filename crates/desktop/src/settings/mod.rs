//! Digger 设置窗口:产品状态、消息与打开目标。
//!
//! 共享层(`bytehost-settings` / `bytehost-settings-ui`)负责页面身份、导航状态与视图;
//! 本模块只做产品适配:注册 Theme / Apps、记住窗口句柄、把共享事件翻成产品消息。
//!
//! 本期**不注册**语言 / Git / 高级页:Digger 还没有对应的产品服务与文案能力。

pub mod theme;
pub mod window;

use bytehost_settings::{PageId, PageRegistry, SettingsState};
use iced::window as iced_window;

use crate::paths;

/// 产品设置消息。由 `crate::product::Msg::Settings` 承载。
#[derive(Debug, Clone)]
pub enum Message {
    /// 打开设置;`target` 不可用时由注册表回退。
    Open(Option<PageId>),
    Close,
    /// 原生窗口创建完成(记录句柄;重复/迟到结果不覆盖新窗口)。
    Opened(iced_window::Id),
    /// 选中某页(导航点击或外部直达)。
    Select(PageId),
    /// 主题页选择;产品负责即时应用与落盘。
    ThemeSelected(crate::settings::theme::Scheme),
    /// 设置窗口收到 Esc。
    Escape(iced_window::Id),
}

/// 设置窗口的产品状态。
pub struct State {
    /// 当前设置窗口;`None` = 未打开。
    pub window: Option<iced_window::Id>,
    /// 共享导航状态(打开 / 当前页 / 窗口代数)。
    pub core: SettingsState,
    registry: PageRegistry,
    /// 主题保存失败原因;显示在主题页并保留到下次成功保存。
    pub theme_error: Option<String>,
    /// 启动读盘失败等一次性提示(与保存失败共用一行展示)。
    pub theme_notice: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        // 首批 Theme / Apps;顺序即导航顺序。
        let registry = PageRegistry::new(vec![
            bytehost_settings::PageDescriptor::built_in(theme_page_id(), "Theme"),
            bytehost_settings::PageDescriptor::built_in(apps_page_id(), "Apps"),
        ])
        .expect("内置设置页注册必须有效");
        Self {
            window: None,
            core: SettingsState::new(),
            registry,
            theme_error: None,
            theme_notice: None,
        }
    }

    /// 启动时读取并应用主题;失败只记录提示,不阻断启动。
    pub fn load_theme(&mut self) {
        self.theme_notice = theme::init_notice();
    }

    pub fn registry(&self) -> &PageRegistry {
        &self.registry
    }

    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }

    pub fn current(&self) -> Option<&PageId> {
        self.core.current()
    }

    /// 已注册页 ID(供产品入口直接构造目标)。
    pub fn has_page(&self, id: &PageId) -> bool {
        self.registry.contains(id)
    }

    /// 打开并选中目标页。返回 `true` 表示需要创建原生窗口。
    pub fn open(&mut self, target: Option<&PageId>) -> bool {
        // 重复打开时共享状态只切页;窗口由调用方 `gain_focus`。
        self.core.open(&self.registry, target)
    }

    /// 只切页;窗口不存在时返回 `false`。
    pub fn select(&mut self, id: &PageId) -> bool {
        self.core.select(&self.registry, id)
    }

    /// 关闭设置状态(不负责原生命令)。
    pub fn close(&mut self) {
        self.core.close();
        self.window = None;
    }

    /// 当前页内容是否是可渲染的已注册页。注册表为空时返回 `None`。
    pub fn page(&self) -> Option<&PageId> {
        self.core.current()
    }
}

/// 共享设置视图的文案:Digger 本期没有 i18n,这里给常用键中文映射,未命中落英文。
pub fn shared_text(key: &str, fallback: &str) -> String {
    let chinese = match key {
        "settings.title" => "设置",
        "settings.close" => "关闭",
        "settings.empty" => "没有可用的设置页。",
        "settings.theme.title" => "主题",
        "settings.theme.dark" => "深色 · ByteBoy2077",
        "settings.theme.light" => "浅色 · ByteBoy2077-Light",
        "settings.language.title" => "语言",
        "settings.language.follow_system" => "跟随系统",
        "settings.language.english" => "English",
        "settings.language.simplified_chinese" => "简体中文",
        "settings.language.saving" => "正在保存…",
        "settings.language.save_failed" => "保存失败,请重试。",
        "settings.language.conflict" => "语言已在别处更改,请重新选择。",
        "settings.language.unsupported" => "当前服务不支持语言设置。",
        "settings.apps.title" => "应用",
        "settings.apps.runtime_note" => "运行时(随宿主提供的受管版本优先于系统版本)",
        "settings.apps.installed_heading" => "已安装",
        "settings.apps.detecting" => "正在探测…",
        "settings.apps.reading" => "读取中…",
        "settings.apps.no_apps" => "还没有安装任何应用",
        "settings.apps.reading_logs" => "读取日志…",
        "settings.apps.logs_truncated" => "仅显示末尾若干行",
        "settings.apps.logs_stale" => "刷新失败,显示的是上一次的内容",
        "settings.apps.uninstall" => "卸载",
        "settings.apps.stop" => "停止",
        "settings.apps.logs" => "日志",
        "settings.apps.hide_logs" => "收起日志",
        "settings.apps.processing" => "处理中…",
        "settings.apps.rollback_to" => "回滚到",
        "settings.apps.confirm_rollback" => "确认回滚",
        "settings.apps.keep_data" => "只删程序(保留数据)",
        "settings.apps.delete_data_too" => "连数据一起删",
        "settings.apps.got_it" => "知道了",
        "settings.apps.install_app_ellipsis" => "安装应用…",
        "settings.apps.source_dir" => "本机目录",
        "settings.apps.source_archive" => "本机压缩包",
        "settings.apps.source_url" => "网络 URL",
        "settings.apps.fetch_plan" => "获取计划",
        "settings.apps.pick_dir_hint" => {
            "请在弹出的对话框里选择应用目录(目录里要有 manifest.toml)…"
        }
        "settings.apps.pick_archive_hint" => "请在弹出的对话框里选择压缩包(.zip / .tar.gz / .tgz)…",
        "settings.apps.checking_app" => "正在检查应用…",
        "settings.apps.installing" => "正在安装…",
        "settings.apps.url_hint" => "https 压缩包地址(只允许静态应用)",
        "settings.apps.sha_hint" => "期望 sha256(可选;填了就钉死,不填则审批卡展示实际算出的哈希)",
        "settings.apps.sha_placeholder" => "留空 = 不钉死",
        "settings.apps.install_ellipsis" => "安装…",
        "settings.apps.approve_and_install" => "批准并安装",
        "settings.apps.download_and_install" => "下载并安装",
        "settings.apps.will_do" => "将要做什么:",
        "settings.apps.will_run" => "安装并运行会执行:",
        "settings.apps.permissions_heading" => "请求的权限:",
        "settings.apps.no_permissions" => "没有请求任何权限",
        "settings.apps.source_heading" => "来源:",
        "settings.apps.from_network_untrusted" => "来自网络,不可信",
        "settings.apps.static_only_csp" => "仅静态应用,运行在严格 CSP 下",
        "settings.apps.uninstall_title" => "卸载应用",
        "settings.apps.uninstall_body" => {
            "「只删程序」保留 data/ 与 logs/,重装后数据还在;「连数据一起删」会清掉整个应用目录。"
        }
        "settings.apps.runtime_uninstall_body" => {
            "依赖它的应用下次启动会因缺少运行时失败;重新安装可恢复。"
        }
        "settings.apps.install_runtime_title" => "安装运行时",
        "settings.apps.rollback_confirm" => {
            "回滚不会连数据一起回滚,旧版本可能读不了新版本写的数据。回滚吗?"
        }
        "common.cancel" => "取消",
        _ => return fallback.to_owned(),
    };
    chinese.to_owned()
}

/// `bytehost.theme` 的校验 ID。
pub fn theme_page_id() -> PageId {
    PageId::new(bytehost_settings::THEME_ID).expect("static built-in id")
}

/// `bytehost.apps` 的校验 ID。
pub fn apps_page_id() -> PageId {
    PageId::new(bytehost_settings::APPS_ID).expect("static built-in id")
}

/// 主题文件的绝对路径(产品数据根)。
pub fn theme_path() -> std::path::PathBuf {
    paths::color_theme_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_batch_registers_theme_then_apps_only() {
        let state = State::new();
        let ids: Vec<_> = state
            .registry()
            .pages()
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(ids, ["bytehost.theme", "bytehost.apps"]);
    }

    #[test]
    fn reopens_do_not_create_a_second_window_or_bump_generation() {
        let mut state = State::new();
        assert!(state.open(None));
        let generation = state.core.window_generation();
        assert!(!state.open(Some(&apps_page_id())));
        assert_eq!(state.core.window_generation(), generation);
        assert_eq!(state.current().unwrap().as_str(), "bytehost.apps");
    }

    #[test]
    fn unregistered_targets_fall_back_to_theme() {
        let mut state = State::new();
        state.open(Some(&PageId::new("digger.editor").unwrap()));
        assert_eq!(state.current().unwrap().as_str(), "bytehost.theme");
    }

    #[test]
    fn reopening_after_close_uses_a_new_window_generation() {
        let mut state = State::new();
        state.open(None);
        let first = state.core.window_generation();
        state.window = Some(iced_window::Id::unique());
        state.close();
        assert!(!state.is_open());
        assert!(state.window.is_none());
        state.open(None);
        assert!(state.core.window_generation() > first);
    }
}
