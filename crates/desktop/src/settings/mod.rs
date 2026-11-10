//! Digger 设置窗口:产品状态、消息与打开目标。
//!
//! 共享层(`bytehost-settings` / `bytehost-settings-ui`)负责页面身份、导航状态、
//! 对话框生命周期、几何与视图;本模块只做产品适配:注册 Theme / Language / Git / Apps、
//! 记住窗口句柄、把共享事件翻成产品消息。
//!
//! 设置窗从 2026-11-03 起走统一的父所属 overlay:窗口由 iced 创建,再由
//! `bytehost-desktop::dialog` 挂成主窗的 AppKit 子窗口;生命周期由共享
//! `DialogController` 决定,不再由产品散落维护重开 / 失败 / 关闭策略。
//!
//! 语言偏好由产品本地持久化，高级页尚未注册。

pub mod language;
pub(crate) mod presentation;
pub mod theme;
pub mod window;

use bytehost_git_accounts::{
    AccountSnapshot, Admission, CommitOutcome, ConnectedAccount, GitAccountError, GitProvider,
    RequestTicket, SecretToken,
};
use bytehost_settings::dialog::{
    DialogController, DialogEffect, DialogEvent, DialogFailure, DialogKey, DialogKind, Phase,
};
use bytehost_settings::git::{GitIntent, GitPageState};
use bytehost_settings::{PageId, PageRegistry, SettingsState};
use iced::window as iced_window;
use std::sync::Arc;

use crate::paths;

/// 主窗口在共享对话框身份里的 owner 编号(Digger 只有一个主窗)。
pub const SETTINGS_OWNER: u64 = 0;

/// 产品设置消息。由 `crate::product::Msg::Settings` 承载。
#[derive(Debug, Clone)]
pub enum Message {
    /// 打开设置;`target` 不可用时由注册表回退。
    Open(Option<PageId>),
    Close,
    /// 原生窗口创建完成;携带创建时的会话身份,迟到结果不能覆盖新窗口。
    Created(DialogKey, iced_window::Id),
    /// 原生父子绑定结果;携带创建时的会话身份,迟到结果不能影响新会话。
    Bound(DialogKey, Result<(), String>),
    FirstFrame(DialogKey, iced_window::Id),
    Presented(DialogKey, iced_window::Id, Result<bool, String>),
    SelftestSnapshot(DialogKey, iced_window::Id, iced_window::Screenshot),
    /// 父窗移动 / 缩放后的重新同步结果。失败只更新可见错误,不改变会话相位。
    SyncResult(DialogKey, Option<String>),
    /// 销毁回包在限定时间内没有到达:强制结束,避免卡在 Closing。
    DestroyTimedOut(DialogKey),
    /// 创建 / 绑定失败后的显式重试。
    Retry,
    /// 选中某页(导航点击或外部直达)。
    Select(PageId),
    /// 主题页选择;产品负责即时应用与落盘。
    ThemeSelected(crate::settings::theme::Scheme),
    LanguageSelected(language::Preference),
    /// 设置窗口收到 Esc。
    Escape(iced_window::Id),
    /// Git 页的类型化意图(连接 / 取消 / 断开 / 重试 / 打开 PAT 页)。
    Git(GitIntent),
    /// 敏感 Token 输入。`SecretToken` 的 Debug 打码、Drop 清零。
    GitToken(GitProvider, SecretToken),
    /// 权威账户事实查询回包。
    GitQueryLoaded(RequestTicket, Result<AccountSnapshot, GitAccountError>),
    /// 连接回包。
    GitConnectLoaded(
        GitProvider,
        RequestTicket,
        Result<(ConnectedAccount, CommitOutcome), GitAccountError>,
    ),
    /// 断开回包。
    GitDisconnectLoaded(
        GitProvider,
        RequestTicket,
        Result<CommitOutcome, GitAccountError>,
    ),
}

/// 设置窗口的产品状态。
pub struct State {
    /// 当前设置窗口;`None` = 未打开。
    pub window: Option<iced_window::Id>,
    /// `window` 所属的对话框身份。销毁 / 回包必须按身份定位实例,
    /// 不能用"当前 window"代替。
    pub window_key: Option<DialogKey>,
    pub(crate) pending_frame: Option<(DialogKey, iced_window::Id)>,
    pub(crate) presentation_permit: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) selftest_cycles: u8,
    // Timed-out sessions relinquish the active slot, but keep resource ownership
    // until the actual native Closed event. Old cleanup cannot target a new session.
    retired_windows: std::collections::HashMap<DialogKey, iced_window::Id>,
    /// 共享导航状态(当前页 / 兼容代数)。对话框身份以 `dialog` 为唯一权威。
    pub core: SettingsState,
    /// 共享对话框生命周期(Closed / Opening / Open / Closing / Failed)。
    pub dialog: DialogController,
    /// 共享 Git 页状态(输入、账户、请求身份)。
    pub git: GitPageState,
    /// 创建 / 绑定 / 同步失败提示;重试成功后清空。
    pub bind_error: Option<String>,
    /// 长寿命提交准入:同一产品内所有请求(包括每次新建的 service)共享。
    admission: Arc<Admission>,
    registry: PageRegistry,
    /// 主题保存失败原因;显示在主题页并保留到下次成功保存。
    pub theme_error: Option<String>,
    /// 启动读盘失败等一次性提示(与保存失败共用一行展示)。
    pub theme_notice: Option<String>,
    pub language: bytehost_settings::SelectionState<language::Preference>,
    pub language_notice: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        // 顺序即导航顺序。
        let registry = PageRegistry::new(vec![
            bytehost_settings::PageDescriptor::built_in(theme_page_id(), "Theme"),
            bytehost_settings::PageDescriptor::built_in(language_page_id(), "Language"),
            bytehost_settings::PageDescriptor::built_in(git_page_id(), "Git"),
            bytehost_settings::PageDescriptor::built_in(apps_page_id(), "Apps"),
        ])
        .expect("内置设置页注册必须有效");
        Self {
            window: None,
            window_key: None,
            pending_frame: None,
            presentation_permit: None,
            selftest_cycles: 0,
            retired_windows: std::collections::HashMap::new(),
            core: SettingsState::new(),
            dialog: DialogController::new(SETTINGS_OWNER, DialogKind::Settings),
            git: GitPageState::new(1),
            bind_error: None,
            admission: Arc::new(Admission::new()),
            registry,
            theme_error: None,
            theme_notice: None,
            language: bytehost_settings::SelectionState::default(),
            language_notice: None,
        }
    }

    /// 启动时读取并应用主题;失败只记录提示,不阻断启动。
    pub fn load_theme(&mut self) {
        self.theme_notice = theme::init_notice();
    }

    pub fn load_language(&mut self) {
        match language::load_from(&language::path()) {
            Ok(preference) => {
                self.language.applied(preference);
                language::apply(preference);
            }
            Err(reason) => {
                language::apply(language::Preference::default());
                self.language_notice = Some(reason);
            }
        }
    }

    pub fn select_language_at(
        &mut self,
        preference: language::Preference,
        path: &std::path::Path,
    ) -> bool {
        if !self.language.select(preference) {
            return false;
        }
        match language::save_to(path, preference) {
            Ok(()) => {
                self.language.applied(preference);
                self.language_notice = None;
                true
            }
            Err(reason) => {
                self.language.failed(reason);
                false
            }
        }
    }

    pub fn registry(&self) -> &PageRegistry {
        &self.registry
    }

    /// 唯一权威的窗口代数:共享对话框会话号(首次打开后为 1)。
    pub fn generation(&self) -> u64 {
        self.dialog.key().map(|key| key.session).unwrap_or(0)
    }

    pub fn phase(&self) -> &Phase {
        self.dialog.phase()
    }

    pub fn key(&self) -> Option<DialogKey> {
        self.dialog.key()
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

    /// 打开并选中目标页。返回共享控制器要求的原生命令。
    pub fn open(&mut self, target: Option<&PageId>) -> Vec<DialogEffect> {
        let effects = {
            self.core.open(&self.registry, target);
            self.bind_error = None;
            self.dialog.open()
        };
        self.sync_git_owner(&effects);
        effects
    }

    /// 只切页;窗口不存在时返回 `false`。
    pub fn select(&mut self, id: &PageId) -> bool {
        self.core.select(&self.registry, id)
    }

    /// 关闭设置状态并返回原生命令。重复关闭幂等。
    pub fn close(&mut self) -> Vec<DialogEffect> {
        self.cancel_presentation();
        self.core.close();
        self.admission.cancel_owner(self.git.owner());
        self.git.close();
        self.bind_error = None;
        self.dialog.close()
    }

    /// 父窗退出:即使创建回包永远不到也必须结束。
    pub fn parent_closed(&mut self) -> Vec<DialogEffect> {
        self.cancel_presentation();
        self.core.close();
        self.admission.cancel_owner(self.git.owner());
        self.git.close();
        self.bind_error = None;
        self.dialog.parent_closed()
    }

    /// 创建失败后的显式重试。
    pub fn retry(&mut self) -> Vec<DialogEffect> {
        self.cancel_presentation();
        let effects = self.dialog.retry();
        self.bind_error = None;
        self.sync_git_owner(&effects);
        effects
    }

    /// 驱动报告原生窗口已创建。只有创建时的完整身份仍是当前会话、且实例槽
    /// 为空(或就是同一个实例)时才接受;迟到 / 旧 attempt 的结果返回
    /// `false`,产品必须销毁该窗口,不能让它覆盖当前实例。
    pub fn on_native_opened(&mut self, key: DialogKey, id: iced_window::Id) -> bool {
        if self.dialog.key() != Some(key)
            || !matches!(self.dialog.phase(), Phase::Opening | Phase::Closing)
        {
            return false;
        }
        match self.window {
            None => {
                self.window = Some(id);
                self.window_key = Some(key);
                true
            }
            Some(current) => current == id && self.window_key == Some(key),
        }
    }

    fn cancel_presentation(&mut self) {
        self.pending_frame = None;
        if let Some(permit) = self.presentation_permit.take() {
            permit.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    /// Binding alone is not presentation readiness. A redraw of the bound,
    /// correctly sized settings view must precede ordering the window on-screen.
    pub(crate) fn prepare_presentation(&mut self, key: DialogKey) -> bool {
        if self.dialog.key() != Some(key)
            || self.window_key != Some(key)
            || !matches!(self.phase(), Phase::Opening)
        {
            return false;
        }
        let Some(id) = self.window else {
            return false;
        };
        self.cancel_presentation();
        self.presentation_permit = Some(Arc::new(std::sync::atomic::AtomicBool::new(true)));
        self.pending_frame = Some((key, id));
        true
    }

    pub(crate) fn accept_first_frame(&mut self, key: DialogKey, id: iced_window::Id) -> bool {
        if self.pending_frame != Some((key, id))
            || self.window != Some(id)
            || self.window_key != Some(key)
            || self.key() != Some(key)
            || !matches!(self.phase(), Phase::Opening)
        {
            return false;
        }
        self.pending_frame = None;
        true
    }

    pub(crate) fn presentation_failed(
        &mut self,
        key: DialogKey,
        id: iced_window::Id,
        detail: String,
    ) -> bool {
        if self.key() != Some(key)
            || self.window != Some(id)
            || self.window_key != Some(key)
            || !matches!(self.phase(), Phase::Open)
        {
            return false;
        }
        self.cancel_presentation();
        self.bind_error = Some(detail.clone());
        self.dialog
            .fail_current(DialogFailure::new("present", detail));
        true
    }

    /// 原生父子绑定成功:此时才算进入 Open,失败窗口不会假装已打开。
    pub fn on_bound(&mut self, key: DialogKey) -> Vec<DialogEffect> {
        if self.dialog.key() != Some(key) || self.window_key != Some(key) {
            return Vec::new();
        }
        if matches!(self.dialog.phase(), Phase::Opening) {
            self.dialog.handle(DialogEvent::Opened { key, window: 0 })
        } else {
            Vec::new()
        }
    }

    /// 驱动报告当前 attempt 的创建 / 绑定失败。旧会话、旧 attempt 或已经
    /// 进入 Open 之后的迟到失败都返回 `false`;调用方不得据此清理当前窗口。
    pub fn on_bind_failed(&mut self, key: DialogKey, detail: impl Into<String>) -> bool {
        if self.dialog.key() != Some(key) || !matches!(self.dialog.phase(), Phase::Opening) {
            return false;
        }
        self.cancel_presentation();
        let detail = detail.into();
        self.bind_error = Some(detail.clone());
        self.dialog.fail_current(DialogFailure::new("bind", detail));
        true
    }

    /// 父窗同步失败:保留可重试的会话与当前实例,只更新可见错误。
    pub fn on_sync_failed(&mut self, key: DialogKey, detail: impl Into<String>) -> bool {
        if self.dialog.key() != Some(key) {
            return false;
        }
        self.bind_error = Some(detail.into());
        true
    }

    /// 仅查看属于 `key` 的实例,不改变映射。销毁命令先关闭它,等原生
    /// `Closed` 回包确认后再清槽位。
    pub fn window_for(&self, key: DialogKey) -> Option<iced_window::Id> {
        if self.window_key == Some(key) {
            self.window
        } else {
            self.retired_windows.get(&key).copied()
        }
    }

    /// 取走属于 `key` 的实例。旧身份不会拿到当前窗口。
    pub fn take_window_for(&mut self, key: DialogKey) -> Option<iced_window::Id> {
        if self.window_key != Some(key) {
            return None;
        }
        self.cancel_presentation();
        self.window_key = None;
        self.window.take()
    }

    /// 驱动确认 `key` 对应的原生实例已经销毁。只有该实例被定位并取走之后
    /// 才能回报 Closed,不能用"当前窗口关闭"代替实例身份校验。
    pub fn confirm_closed(&mut self, key: DialogKey) -> Vec<DialogEffect> {
        if self.dialog.key() != Some(key) {
            return Vec::new();
        }
        let effects = self.dialog.handle(DialogEvent::Closed { key, window: 0 });
        self.sync_git_owner(&effects);
        effects
    }

    /// 驱动报告原生窗口已销毁。只有当前实例的 `id` 才能推进控制器;
    /// 旧实例的关闭回包只清理自己的槽位。
    pub fn owns_window(&self, id: iced_window::Id) -> bool {
        self.window == Some(id) || self.retired_windows.values().any(|old| *old == id)
    }

    pub fn on_native_closed(&mut self, id: iced_window::Id) -> Vec<DialogEffect> {
        if let Some(key) = self
            .retired_windows
            .iter()
            .find_map(|(key, old)| (*old == id).then_some(*key))
        {
            self.retired_windows.remove(&key);
            return Vec::new();
        }
        if self.window != Some(id) {
            return Vec::new();
        }
        self.cancel_presentation();
        let key = self.window_key;
        self.window = None;
        self.window_key = None;
        match key {
            Some(key) => self.confirm_closed(key),
            None => Vec::new(),
        }
    }

    /// 有界销毁:回包 / 原生句柄迟迟不到时强制回到 Closed。
    pub fn on_destroy_timed_out(&mut self, key: DialogKey) -> Vec<DialogEffect> {
        if self.dialog.key() != Some(key) || !matches!(self.dialog.phase(), Phase::Closing) {
            return Vec::new();
        }
        if let Some(id) = self.take_window_for(key) {
            self.retired_windows.insert(key, id);
        }
        let reopen = self.core.is_open();
        let mut effects = self.dialog.destroy_timed_out();
        if reopen {
            effects.extend(self.dialog.open());
        }
        self.sync_git_owner(&effects);
        effects
    }

    /// 所有 product / service 实例共享的提交准入。
    pub fn admission(&self) -> Arc<Admission> {
        Arc::clone(&self.admission)
    }

    /// 当前页内容是否是可渲染的已注册页。注册表为空时返回 `None`。
    pub fn page(&self) -> Option<&PageId> {
        self.core.current()
    }

    fn sync_git_owner(&mut self, effects: &[DialogEffect]) {
        for effect in effects {
            if let DialogEffect::Create(key) = effect {
                // 每个新会话一个独立请求 owner:旧窗口的迟到回包不能改新会话,
                // 旧 owner 的在途请求也不能再进入持久化。
                // 同一会话的 attempt 重试保留输入 / 账户展示,不清空非敏感状态。
                if self.git.owner() != key.session {
                    self.admission.cancel_owner(self.git.owner());
                    self.git = GitPageState::new(key.session);
                }
            }
        }
    }
}

/// 共享视图的英文来自库内 fallback，中文映射由产品提供。
pub fn shared_text(key: &str, fallback: &str) -> String {
    shared_text_for(language::code(), key, fallback)
}

fn shared_text_for(locale: &str, key: &str, fallback: &str) -> String {
    if locale != "zh-CN" {
        return fallback.to_owned();
    }
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
        "settings.git.title" => "Git 账户",
        "settings.git.loading" => "正在读取账户…",
        "settings.git.not_connected" => "未连接",
        "settings.git.unreconciled" => "凭据已保存，账户信息待恢复。请重新连接或断开。",
        "settings.git.reconnect" => "重新连接",
        "settings.git.connect" => "连接",
        "settings.git.connected" => "已连接",
        "settings.git.disconnect" => "断开",
        "settings.git.confirm" => "确认",
        "settings.git.cancel" => "取消",
        "settings.git.validating" => "校验中…",
        "settings.git.retry" => "重试",
        "settings.git.pat_placeholder" => "个人访问令牌(PAT)",
        "settings.git.generate_pat" => "没有 PAT?点此生成",
        "settings.git.error" => "账户操作未完成,请检查后重试。",
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

/// `bytehost.language` 的校验 ID。
pub fn language_page_id() -> PageId {
    PageId::new(bytehost_settings::LANGUAGE_ID).expect("static built-in id")
}

/// `bytehost.git` 的校验 ID。
pub fn git_page_id() -> PageId {
    PageId::new(bytehost_settings::GIT_ID).expect("static built-in id")
}

/// `bytehost.apps` 的校验 ID。
pub fn apps_page_id() -> PageId {
    PageId::new(bytehost_settings::APPS_ID).expect("static built-in id")
}

/// 主题文件的绝对路径(产品数据根)。
pub fn theme_path() -> std::path::PathBuf {
    paths::color_theme_path()
}

/// Digger 的 Git 账户元数据文件(独立于 Dozer)。
pub fn git_accounts_path() -> std::path::PathBuf {
    paths::config_dir().join("git_accounts.json")
}

/// Digger 的 Keychain 服务名;不得读取 Dozer 的 `dozer-git`。
pub const GIT_KEYCHAIN_SERVICE: &str = "digger-git";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_can_be_opened_directly_and_selected_after_reopening() {
        let mut state = State::new();
        let language = language_page_id();
        state.open(Some(&language));
        assert_eq!(state.current(), Some(&language));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("language.json");
        assert!(state.select_language_at(language::Preference::English, &path));
        state.close();
        state.open(Some(&language));
        assert_eq!(state.language.confirmed(), &language::Preference::English);
        assert_eq!(
            language::load_from(&path),
            Ok(language::Preference::English)
        );
    }

    #[test]
    fn failed_language_save_keeps_confirmed_value_and_can_be_retried() {
        let mut state = State::new();
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, "file").unwrap();
        assert!(!state.select_language_at(
            language::Preference::English,
            &blocked.join("language.json")
        ));
        assert_eq!(state.language.confirmed(), &language::Preference::System);
        assert!(matches!(
            state.language.status(),
            bytehost_settings::SaveStatus::Failed(_)
        ));
        let path = dir.path().join("language.json");
        assert!(state.select_language_at(language::Preference::English, &path));
        assert_eq!(
            state.language.status(),
            &bytehost_settings::SaveStatus::Idle
        );
    }

    #[test]
    fn settings_text_follows_the_selected_language() {
        assert_eq!(
            shared_text_for("en", "settings.language.title", "Language"),
            "Language"
        );
        assert_eq!(
            shared_text_for("zh-CN", "settings.language.title", "Language"),
            "语言"
        );
        assert_eq!(shared_text_for("zh-CN", "unknown", "Fallback"), "Fallback");
    }

    #[test]
    fn token_input_message_debug_is_redacted() {
        let message = Message::GitToken(
            GitProvider::GitHub,
            SecretToken::new("ghp_marker_should_not_print"),
        );
        let debug = format!("{message:?}");
        assert!(!debug.contains("ghp_marker_should_not_print"));
        assert!(debug.contains("***"));
    }

    #[test]
    fn registers_language_between_theme_and_git() {
        let state = State::new();
        let ids: Vec<_> = state
            .registry()
            .pages()
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "bytehost.theme",
                "bytehost.language",
                "bytehost.git",
                "bytehost.apps"
            ]
        );
    }

    #[test]
    fn reopens_do_not_create_a_second_session_or_bump_generation() {
        let mut state = State::new();
        let first = state.open(None);
        assert!(matches!(first.as_slice(), [DialogEffect::Create(_)]));
        let key = state.key().unwrap();
        assert!(state.on_native_opened(key, iced_window::Id::unique()));
        let effects = state.on_bound(key);
        assert!(effects.is_empty());
        let generation = state.generation();
        assert!(!state.open(Some(&apps_page_id())).is_empty() || state.key() == Some(key));
        assert_eq!(state.generation(), generation);
        assert_eq!(state.current().unwrap().as_str(), "bytehost.apps");
    }

    #[test]
    fn unregistered_targets_fall_back_to_theme() {
        let mut state = State::new();
        state.open(Some(&PageId::new("digger.editor").unwrap()));
        assert_eq!(state.current().unwrap().as_str(), "bytehost.theme");
    }

    #[test]
    fn reopening_after_close_uses_a_new_session_and_clears_git_input() {
        let mut state = State::new();
        state.open(None);
        let first = state.generation();
        let id = iced_window::Id::unique();
        let key = state.key().unwrap();
        assert!(state.on_native_opened(key, id));
        state.on_bound(key);
        state.git.begin_edit(GitProvider::GitHub);
        state.git.set_token(GitProvider::GitHub, "secret");
        state.on_native_closed(id);
        assert!(!state.is_open());
        let effects = state.open(None);
        assert!(matches!(effects.as_slice(), [DialogEffect::Create(_)]));
        assert!(state.generation() > first);
        assert_eq!(state.git.token(GitProvider::GitHub), "");
    }

    #[test]
    fn bind_failure_is_visible_and_retryable() {
        let mut state = State::new();
        state.open(None);
        let key = state.key().unwrap();
        let id = iced_window::Id::unique();
        assert!(state.on_native_opened(key, id));
        assert!(state.on_bind_failed(key, "no parent"));
        assert_eq!(state.take_window_for(key), Some(id));
        assert!(matches!(state.phase(), Phase::Failed(_)));
        assert_eq!(state.bind_error.as_deref(), Some("no parent"));
        let effects = state.retry();
        assert!(matches!(effects.as_slice(), [DialogEffect::Create(_)]));
        assert!(state.bind_error.is_none());
    }

    #[test]
    fn stale_bind_results_cannot_open_or_fail_a_new_session() {
        let mut state = State::new();
        state.open(None);
        let first = state.key().unwrap();
        let id = iced_window::Id::unique();
        assert!(state.on_native_opened(first, id));
        state.close();
        state.on_native_closed(id);

        state.open(None);
        let second = state.key().unwrap();
        assert_ne!(first.session, second.session);
        assert!(matches!(state.phase(), Phase::Opening));
        let second_window = iced_window::Id::unique();
        assert!(state.on_native_opened(second, second_window));
        assert!(state.on_bound(second).is_empty());

        // 旧会话的失败 / 成功 / 关闭都不能影响新句柄。
        assert!(!state.on_bind_failed(first, "old failure"));
        assert!(state.bind_error.is_none());
        assert_eq!(state.window, Some(second_window));
        assert!(matches!(state.phase(), Phase::Open));
        assert!(state.on_bound(first).is_empty());
        assert!(state.on_native_closed(id).is_empty());
        assert_eq!(state.window, Some(second_window));
        assert!(matches!(state.phase(), Phase::Open));
    }

    #[test]
    fn late_failure_after_open_does_not_destroy_the_visible_dialog() {
        let mut state = State::new();
        state.open(None);
        let key = state.key().unwrap();
        let id = iced_window::Id::unique();
        assert!(state.on_native_opened(key, id));
        assert!(state.on_bound(key).is_empty());
        assert!(matches!(state.phase(), Phase::Open));

        assert!(!state.on_bind_failed(key, "late"));
        assert!(matches!(state.phase(), Phase::Open));
        assert_eq!(state.window, Some(id));
        assert!(state.bind_error.is_none());
    }

    #[test]
    fn new_session_gets_a_fresh_git_request_owner() {
        let mut state = State::new();
        state.open(None);
        let key = state.key().unwrap();
        let id = iced_window::Id::unique();
        assert!(state.on_native_opened(key, id));
        let first_owner = state.git.owner();
        state.close();
        state.on_native_closed(id);
        state.open(None);
        assert_ne!(state.git.owner(), first_owner);
    }

    #[test]
    fn cancel_and_close_revoke_service_level_commit_admission() {
        let mut state = State::new();
        state.open(None);
        state.git.begin_edit(GitProvider::GitHub);
        state.git.set_token(GitProvider::GitHub, "tok");
        let ticket = state.git.begin_connect(GitProvider::GitHub).unwrap();
        let admission = state.admission();
        admission.register(ticket);
        assert!(admission.admit(ticket));

        assert_eq!(state.git.cancel(GitProvider::GitHub), Some(ticket));
        admission.cancel(ticket);
        assert!(!admission.admit(ticket));
    }

    #[test]
    fn closing_the_dialog_revokes_every_in_flight_request_owner() {
        let mut state = State::new();
        state.open(None);
        let owner = state.git.owner();
        state.git.begin_edit(GitProvider::GitHub);
        state.git.set_token(GitProvider::GitHub, "tok");
        let ticket = state.git.begin_connect(GitProvider::GitHub).unwrap();
        let admission = state.admission();
        admission.register(ticket);

        state.close();
        assert!(!admission.admit(ticket));
        assert_eq!(state.git.owner(), owner);
    }

    #[test]
    fn destroy_without_a_reply_is_bounded_by_timeout() {
        let mut state = State::new();
        state.open(None);
        let key = state.key().unwrap();
        state.close();
        assert!(matches!(state.phase(), Phase::Closing));
        let effects = state.on_destroy_timed_out(key);
        assert!(matches!(state.phase(), Phase::Closed));
        assert!(state.key().is_none());
        assert!(
            effects.iter().all(|effect| !matches!(
                effect,
                DialogEffect::Create(_) | DialogEffect::Activate(_)
            ))
        );
    }

    #[test]
    fn closing_a_stale_instance_does_not_close_the_current_session() {
        let mut state = State::new();
        state.open(None);
        let first = state.key().unwrap();
        let first_window = iced_window::Id::unique();
        assert!(state.on_native_opened(first, first_window));
        state.close();
        // 旧实例的销毁回包到得比新会话还晚。
        state.on_native_closed(first_window);

        state.open(None);
        let second = state.key().unwrap();
        let second_window = iced_window::Id::unique();
        assert!(state.on_native_opened(second, second_window));
        state.on_bound(second);

        // 再投递一次旧实例的关闭:不能清掉新句柄或新相位。
        assert!(state.on_native_closed(first_window).is_empty());
        assert_eq!(state.window, Some(second_window));
        assert!(matches!(state.phase(), Phase::Open));
        assert_eq!(state.key(), Some(second));
    }
}
