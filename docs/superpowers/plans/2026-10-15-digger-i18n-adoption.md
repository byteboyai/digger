# Digger 接入共享 i18n 契约(bytehost v0.4.0 / byteui v0.4.3)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Digger 脱离本地 `[patch]` worktree,改用已发布的 bytehost `v0.4.0` / byteui `v0.4.3` 和已推送的 Dozer revision `7cdbffd0`,并按新的 i18n 契约适配,使 Digger 重新编译、全部测试通过,用量面板按一个明确的界面语言渲染。

**Architecture:** 新增一个很小的 `UiLocale`(进程内的界面语言持有者:默认 `zh-CN`,可被 `DIGGER_UI_LOCALE` 覆盖,不做运行时切换),把它注入到 `UsagePanel` 的三个出口——库视图的 `locale` 参数、原生 webview 的 `Stamped<LocaleStamp>` 推送、提示文案的 `LocalizedText` 解析。其余是机械的 API 跟进(`panel_head` 多一个 `kind`、折叠按钮的泛型签名、`AppSummary.display_i18n`、`build_app_child` 多一个布尔参数)。**不迁移 Digger 自己的中文硬编码文案**(见「范围外」)。

**Tech Stack:** Rust 2024、iced 0.14、wry、`bytehost-*` `v0.4.0`、`byteui` `v0.4.3`、`dozer-panel-usage` / `dozer-model` / `dozer-services` / `dozer-i18n`(Dozer revision `7cdbffd0178c50891d4c39f8260224e6b76b79e5`)。

**Spec:** 这是对既有契约的消费,不另立新 spec。契约来源(只读):
- bytehost 权威规格 `bytehost/docs/superpowers/specs/2026-10-14-bytehost-app-i18n.md`
- Dozer 的 i18n 设计 `dozer/docs/superpowers/specs/2026-10-12-dozer-i18n-design.md`(Locale 取值、`LocalizedText`、webview envelope 的 `ui_locale` / `locale_revision`)
- Dozer 的参照实现:`dozer/crates/dozer-app/src/host_adapters/usage_panel.rs`(`PanelDisplay` 的 `App` 实现)、`dozer/crates/dozer-app/src/app/app.rs` 的用量推送(`Stamped::new(stamp, desired)`)。

**背景(为什么有这份计划):** 上一次尝试把 Digger 的依赖换成新 tag 时,`digger-desktop` 出现 7 处编译错误(`bytehost v0.3.0` 的 i18n 契约改了公共 API)。已在一次性试验里确认:**只需要下面 Task 1 的这组改动,`cargo build --workspace` 和 `cargo test --workspace`(含真实窗口自检)全部通过**,试验 diff 已还原,没有残留。试验时用了硬编码 `"zh-CN"`;本计划把它换成可测试的 `UiLocale`。

## Global Constraints

- **依赖只用已发布的东西,不留本地路径**:bytehost 全部 7 个依赖 `tag = "v0.4.0"`;byteui `tag = "v0.4.3"`;三个 dozer 依赖加上 `dozer-i18n`,全部 `rev = "7cdbffd0178c50891d4c39f8260224e6b76b79e5"`;删除 `Cargo.toml` 末尾整段开发期 `[patch."https://github.com/byteboyai/bytehost"]` 和 `[patch."https://github.com/byteboyai/byteui"]`。
- **依赖图里只许有一份 `bytehost-core` 和一份 `byteui`**(两份会让 `PanelId` 类型不兼容、byteui 全局主题状态分裂)。以 `Cargo.lock` 里 `name = "bytehost-core"` / `name = "byteui"` 各只出现一次为准。
- **不改 Dozer、bytehost、byteui**。本计划只改 Digger 仓库。
- **不推送、不打 tag**(Digger 没有远端)。
- **持久化文件名与面板 id 不变**:`shell.json`、`digger.usage`。
- **本期不迁移 Digger 自己的文案**,不新增语言设置界面,不做运行时切换语言。
- 注释与现有代码风格一致(中文注释,说"为什么");不留 TODO / 占位。
- 不要碰与本计划无关的未提交文件(开工前 `git status` 记录一次)。
- 提交信息末尾加:`Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`
- 目录约定:`ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy`;Digger 在 `$ROOT/digger`。

## 范围外(明确不做,各自需要单独的计划)

1. **Digger 自己的界面文案**(`apps/*.rs`、`product.rs`、`shell.rs`、`markdown/*` 里约 190 行中文字面量,其中一部分在测试里)。迁移需要先决定:Digger 的词表放哪、`owner` 叫什么(例如 `digger.host`)、是否复用 `dozer-i18n::Resolver`。
2. **跟随系统语言 / 设置界面 / 运行时切换**。Dozer 的系统语言探测在 `dozer-app/src/platform/locale.rs`(依赖 `objc2_foundation`),不是可复用库。
3. **向应用 webview 下发语言**(`build_app_child` 的 `install_i18n_bridge` 本期传 `false`,即应用页面里没有 `window.bytehost.i18n`)。
4. **应用标题本地化**(`AppSummary.display_i18n`)。本期只在测试夹具里补 `None`,界面继续显示 `name`。

> **已知的、被接受的限制:** 在 `DIGGER_UI_LOCALE=en` 下,用量面板(共享库)显示英文,而 Digger 自己的界面仍是中文——这是范围外第 1 条的直接后果,不是缺陷。默认值是 `zh-CN`,所以不设环境变量时界面是一致的中文。

## Review Focus

spec 没细说、但最可能伤到真实使用者的输入或条件;每一条都落到了具体任务的具体测试里:

1. **`DIGGER_UI_LOCALE` 设成不认识的值**(`fr`、空串、`zh-TW`、`C`、乱码):必须回落默认 `zh-CN`,不 panic、不产生空语言。`zh-TW` / `zh-Hant` 不会被偷偷当成简体——共享库的规则是"不自动映射",结果同样回落默认。→ Task 1 `locale::tests::unknown_or_empty_override_falls_back_to_default`、`locale::tests::traditional_chinese_is_not_silently_mapped_to_simplified`。
2. **面板头图标靠稳定身份而不是标题字面量**:旧实现 `match title { "Agent" => Bot, _ => BarChart3 }`,标题一旦本地化("智能体"),图标就错成 BarChart3。→ Task 1 `usage::tests::panel_head_icon_follows_the_kind_not_the_title`。
3. **提示文案的 key 在当前语言里缺失**:必须落到英文 fallback,而不是空串或原始 key。→ Task 1 `usage::tests::notice_with_unknown_key_falls_back_to_english_text`。
4. **提示文案带参数**:`{message}` 必须被替换,且中文模板生效。→ Task 1 `usage::tests::notice_arguments_are_interpolated_in_chinese`。
5. **推送给 webview 的语言上下文形状**:`ui_locale` 是字符串、`locale_revision` 是数字,键名不能错,否则前端拿不到语言。→ Task 1 `locale::tests::stamp_serializes_with_the_envelope_field_names`。
6. **依赖图出现两份 `bytehost-core` / `byteui`**:编译可能照样通过,但运行时主题会错。→ Task 1 Step 8 检查锁文件,Task 2 的真实窗口自检兜底。

---

### Task 0: 分支与基线

**Files:** 无代码改动。

- [ ] **Step 1: 记录基线并建分支**

```bash
ROOT=/Users/chrischiang/Projects/CoralProjects/byteboy
cd $ROOT/digger
git status --short            # 记录既有未提交文件,全程不碰它们
git log --oneline -1
git switch -c feat/i18n-adoption
```

- [ ] **Step 2: 确认基线(旧 pin + `[patch]`)是绿的**

```bash
cd $ROOT/digger
cargo build --workspace 2>&1 | tail -2
cargo test -p digger-desktop --lib adapters 2>&1 | grep -E "^test result|FAILED"
bash scripts/check-host-deps.sh
```

Expected: 构建 `Finished`;`adapters` 下的测试通过;`digger host-deps check: ok`。如果基线不绿,先停下报告,不要往下做。

- [ ] **Step 3: 确认 Dozer 的目标 revision 在远端可取**

```bash
git ls-remote https://github.com/byteboyai/dozer.git main | head -1
git ls-remote --tags https://github.com/byteboyai/bytehost.git v0.4.0 | head -2
git ls-remote --tags https://github.com/byteboyai/byteui.git v0.4.3 | head -2
```

Expected: dozer 的 `main` 不早于 `7cdbffd0`(包含它即可);两个 tag 都能查到。查不到就停下,说明前置发布没完成。

---

### Task 1: 切换依赖并适配 i18n 契约

**Files:**
- Modify: `Cargo.toml`(workspace 依赖、删除 `[patch]`)、`Cargo.lock`(自动)
- Modify: `crates/desktop/Cargo.toml`(加 `dozer-i18n`)
- Modify: `crates/app-host/src/testing.rs`(夹具补 `display_i18n: None`)
- Modify: `crates/desktop/src/apps/mod.rs:1118`(`build_app_child` 多一个布尔参数)
- Create: `crates/desktop/src/locale.rs`
- Modify: `crates/desktop/src/main.rs`(声明 `mod locale;`)
- Modify: `crates/desktop/src/adapters/usage.rs`
- Modify: `crates/desktop/src/adapters/usage_native.rs:147`
- Test: `crates/desktop/src/locale.rs` 与 `crates/desktop/src/adapters/usage.rs` 里的 `#[cfg(test)]` 模块

**Interfaces:**
- Consumes(来自 Dozer `7cdbffd0`,已核对签名):
  - `dozer_i18n::normalize_locale(raw: &str) -> Option<&'static str>`(`en`/`zh-CN` 或 `None`)
  - `dozer_model::{Locale(pub String), LocaleStamp { ui_locale: Locale, locale_revision: u64 }, Stamped<T>::new(stamp, payload), LocalizedText::keyed(owner, key, fallback).with_arg(name, value)}`
  - `dozer_panel_usage::adapter::{PanelHeadKind::{Content, AgentFilter}, UsageNotice { level, text: LocalizedText, key }, NoticeLevel}`
  - `dozer_panel_usage::i18n::resolve(&LocalizedText, locale: &str) -> String`
  - `dozer_panel_usage::view::content_pane(display, collapsed, ws_state, width, outer, locale: &str)`、`list_pane(display, ws_state, width, outer, locale: &str)`
  - `UsageSurface::set_desired(Stamped<UsageViewPayload>)`
- Produces(Task 2 与后续计划使用):
  - `crate::locale::UiLocale`:`UiLocale::resolve(override_value: Option<&str>) -> UiLocale`、`UiLocale::from_env() -> UiLocale`、`fn code(&self) -> &'static str`、`fn stamp(&self) -> dozer_model::LocaleStamp`;常量 `locale::ENV: &str = "DIGGER_UI_LOCALE"`、`locale::DEFAULT: &str = "zh-CN"`
  - `UsagePanel` 内部持有 `locale: UiLocale`(构造函数 `UsagePanel::new(service, scope_path)` 签名**不变**,所以 `product.rs` 不用改)
  - `adapters::usage::head_icon(kind: PanelHeadKind) -> IconKind`、`adapters::usage::render_notices(Vec<UsageNotice>, locale: &str) -> Vec<(String, String)>`
  - `usage_native::Native::sync(&mut self, id, visible, rect, state, stamp: LocaleStamp)`

> **关于 TDD 的说明:** 这一任务里工作区在切换依赖后到全部适配完成前**不能编译**,所以"先看测试失败"的红灯是编译错误本身(Step 3 要看到的就是那 7 处错误)。绿灯在 Step 9。不要把任务拆成中间有一个编不过的提交。

- [ ] **Step 1: 写 `locale.rs`(含测试)**

Create `crates/desktop/src/locale.rs`:

```rust
//! Digger 的界面语言持有者。
//!
//! 本期的语义刻意很小:**进程启动时定一次,之后不变**。默认 `zh-CN`(Digger 自己的
//! 文案目前全是中文,默认中文才不会出现"共享面板英文、其余中文"的混排);开发者可以用
//! `DIGGER_UI_LOCALE` 覆盖,方便验证共享库的英文资源。跟随系统语言、设置界面、运行时
//! 切换都不在本期范围(见 `docs/superpowers/plans/2026-10-15-digger-i18n-adoption.md`)。
//!
//! 规范化规则复用共享的纯库 `dozer-i18n`,不在这里再写一份:`en` / `zh-CN` 之外的值,
//! 以及明确的繁体(`zh-TW` / `zh-Hant`),都按"不受支持"处理,回落默认。

use dozer_model::{Locale, LocaleStamp};

/// 覆盖默认语言的环境变量。
pub const ENV: &str = "DIGGER_UI_LOCALE";

/// 默认界面语言。
pub const DEFAULT: &str = Locale::ZH_CN;

/// 当前进程的界面语言。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiLocale {
    code: &'static str,
    revision: u64,
}

impl UiLocale {
    /// 由一个可选的覆盖值解析;不认识的值回落 [`DEFAULT`]。纯函数,测试不需要碰环境变量。
    pub fn resolve(override_value: Option<&str>) -> Self {
        let code = override_value
            .and_then(dozer_i18n::normalize_locale)
            .unwrap_or(DEFAULT);
        // 本期语言在进程内不会变,显示代恒为 1;以后加运行时切换时,在生效语言
        // 真正变化时递增(webview 端靠它丢弃过期推送)。
        UiLocale { code, revision: 1 }
    }

    /// 读 `DIGGER_UI_LOCALE`。
    pub fn from_env() -> Self {
        Self::resolve(std::env::var(ENV).ok().as_deref())
    }

    /// 规范化后的语言码(`"en"` 或 `"zh-CN"`)。
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 推送给 webview 的语言上下文(envelope 的 `ui_locale` / `locale_revision`)。
    pub fn stamp(&self) -> LocaleStamp {
        LocaleStamp {
            ui_locale: Locale(self.code.to_string()),
            locale_revision: self.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_override_means_the_default() {
        assert_eq!(UiLocale::resolve(None).code(), "zh-CN");
    }

    #[test]
    fn supported_overrides_are_normalized() {
        assert_eq!(UiLocale::resolve(Some("en")).code(), "en");
        assert_eq!(UiLocale::resolve(Some("en_US.UTF-8")).code(), "en");
        assert_eq!(UiLocale::resolve(Some("zh_CN")).code(), "zh-CN");
        assert_eq!(UiLocale::resolve(Some(" ZH-hans ")).code(), "zh-CN");
    }

    #[test]
    fn unknown_or_empty_override_falls_back_to_default() {
        for raw in ["", "   ", "fr", "C", "POSIX", "ja-JP", "🙂", "zh-"] {
            assert_eq!(
                UiLocale::resolve(Some(raw)).code(),
                DEFAULT,
                "覆盖值 {raw:?} 应回落默认"
            );
        }
    }

    #[test]
    fn traditional_chinese_is_not_silently_mapped_to_simplified() {
        // 共享库的规则:繁体不自动映射简体 → 视为不受支持 → 回落默认(而不是 "zh-CN" 之外的什么)。
        // 这里钉住的是"没有产生第三种取值":无论哪条路径,结果只会是 en 或 zh-CN。
        for raw in ["zh-TW", "zh-HK", "zh-Hant", "zh-Hant-TW"] {
            let code = UiLocale::resolve(Some(raw)).code();
            assert!(code == "en" || code == "zh-CN", "{raw:?} → {code:?}");
            assert_eq!(code, DEFAULT, "{raw:?} 不受支持,应回落默认");
        }
    }

    #[test]
    fn stamp_serializes_with_the_envelope_field_names() {
        let json = serde_json::to_value(UiLocale::resolve(Some("en")).stamp()).unwrap();
        assert_eq!(json["ui_locale"], "en");
        assert_eq!(json["locale_revision"], 1);
    }
}
```

- [ ] **Step 2: 声明模块、加依赖**

Edit `crates/desktop/src/main.rs`:在 `mod lifecycle_selftest;` 和 `mod markdown;` 之间加一行(现有列表是字母序):

```rust
mod locale;
```

Edit `crates/desktop/Cargo.toml`:在 `dozer-model.workspace = true` 下一行加:

```toml
dozer-i18n.workspace = true
```

(`serde_json` 已经是 `digger-desktop` 的依赖,`stamp_serializes…` 直接可用。)

- [ ] **Step 3: 切换根 `Cargo.toml`,并确认红灯**

Edit `Cargo.toml`(根):

1. 第 29–39 行的 7 个 `bytehost-*`:`tag = "v0.2.0"` → `tag = "v0.4.0"`。
2. 第 42 行:`byteui` `tag = "v0.4.1"` → `tag = "v0.4.3"`。
3. 三个 dozer 依赖的 `rev` 全部改成 `7cdbffd0178c50891d4c39f8260224e6b76b79e5`,并在它们下面新增:

```toml
dozer-i18n = { git = "https://github.com/byteboyai/dozer", rev = "7cdbffd0178c50891d4c39f8260224e6b76b79e5" }
```

4. 把第 30 行那句旧注释 `# 共享窗口骨架(S1)。开发期靠文件末尾的 [patch] 指向本地 worktree;打 tag 后改成新 tag 并删掉 [patch]。` 改成 `# 共享窗口骨架(S1)。`
5. 把 Dozer 依赖上方注释里"固定到该 revision(dozer 无对应发布 tag,用 rev 钉死)"保留,并把旧 revision 号 `67ab0e6` 的提及改成 `7cdbffd0`(这条注释说明为什么用 rev)。
6. 删除文件末尾从 `# ---- 开发期临时配置(S1 最后一个任务移除)----` 起到文件结束的整段(两个 `[patch]` 表和它们的注释)。

Run:

```bash
cd $ROOT/digger
cargo build -p digger-desktop 2>&1 | grep -E "^error" | sort | uniq -c
```

Expected(红灯,应能看到这几类错误,恰好对应下面的步骤):`E0063 missing field display_i18n`(在 `digger-app-host`)先挡住;修掉 Step 4 后再编译,会看到 `digger-desktop` 的 7 处错误:`E0050 panel_head`、`E0049 list_collapse_button`、`E0277 Vec<(String, LocalizedText)>`、两处 `E0061`(`content_pane` 6 参数 / `list_pane` 5 参数)、`E0308 set_desired`、`E0282 built`。

- [ ] **Step 4: 修 `app-host` 夹具与 `build_app_child`**

Edit `crates/app-host/src/testing.rs`,在 `pub fn summary(...)` 返回的 `AppSummary { ... }` 里,`rollback_note: None,` 下一行加:

```rust
        display_i18n: None,
```

Edit `crates/desktop/src/apps/mod.rs`,在 `bytehost_desktop::build_app_child(` 调用里,`|_slot| {},` 和 `move |built| match built.result {` 之间插入新参数(**第 5 个参数**,位置是 `on_ready` 之后、`wrap` 之前):

```rust
                |_slot| {},
                // 本期不向应用页面下发语言:不装只读语言桥(`window.bytehost.i18n` 不存在)。
                // 要让应用收到语言,需要同时做"应用标题/语言下发"的后续计划。
                false,
                move |built| match built.result {
```

- [ ] **Step 5: 改 `usage_native.rs`,让推送带语言上下文**

Edit `crates/desktop/src/adapters/usage_native.rs`。`sync` 增加最后一个参数 `stamp`,并把 `set_desired` 的载荷包成 `Stamped`:

```rust
    pub fn sync(
        &mut self,
        id: window::Id,
        visible: bool,
        rect: LogicalRect,
        state: &WorkspaceState,
        stamp: dozer_model::LocaleStamp,
    ) -> Task<Event> {
```

```rust
        // 语言上下文并入载荷:业务数据不变、只有语言变时,逐帧去重也会放行一次推送。
        self.surface.set_desired(dozer_model::Stamped::new(
            stamp,
            dozer_panel_usage::current_view_payload(state),
        ));
```

(把原来的 `self.surface.set_desired(dozer_panel_usage::current_view_payload(state));` 两行替换掉。)

- [ ] **Step 6: 写 `usage.rs` 的测试(先于实现)**

在 `crates/desktop/src/adapters/usage.rs` 末尾的 `#[cfg(test)] mod tests { ... }` 里追加(`use super::*;` 已在):

```rust
    use dozer_model::LocalizedText;
    use dozer_panel_usage::adapter::{NoticeLevel, PanelHeadKind, UsageNotice};

    #[test]
    fn panel_head_icon_follows_the_kind_not_the_title() {
        use byteui::interaction::icons::IconKind;
        // 标题本地化之后("智能体"),图标仍由稳定身份决定。
        assert!(matches!(head_icon(PanelHeadKind::AgentFilter), IconKind::Bot));
        assert!(matches!(head_icon(PanelHeadKind::Content), IconKind::BarChart3));
    }

    fn notice(text: LocalizedText) -> UsageNotice {
        UsageNotice { level: NoticeLevel::Error, text, key: None }
    }

    #[test]
    fn notice_arguments_are_interpolated_in_chinese() {
        let text = LocalizedText::keyed(
            "dozer.usage",
            "error.load_failed",
            "Failed to load usage: {message}",
        )
        .with_arg("message", "断线");
        assert_eq!(
            render_notices(vec![notice(text)], "zh-CN"),
            vec![("Error".to_string(), "用量统计失败: 断线".to_string())]
        );
    }

    #[test]
    fn notice_with_unknown_key_falls_back_to_english_text() {
        let text = LocalizedText::keyed("dozer.usage", "no.such.key", "Fallback text");
        assert_eq!(
            render_notices(vec![notice(text)], "zh-CN"),
            vec![("Error".to_string(), "Fallback text".to_string())]
        );
    }
```

- [ ] **Step 7: 改 `usage.rs` 的实现**

Edit `crates/desktop/src/adapters/usage.rs`:

(a) 在 `UsagePanel` 结构体里加字段(放在 `scope_path` 之后),并在 `new` 里初始化:

```rust
    /// 界面语言(进程启动时定,见 [`crate::locale`])。
    locale: crate::locale::UiLocale,
```

```rust
            locale: crate::locale::UiLocale::from_env(),
```

(`new` 的签名与调用点不变,`product.rs` 不用动。)

(b) 在 `impl UsagePanel` 之前加两个纯函数:

```rust
/// 面板头的图标由稳定身份决定,不解析标题字面量(标题随语言变)。
pub(crate) fn head_icon(kind: dozer_panel_usage::adapter::PanelHeadKind) -> byteui::interaction::icons::IconKind {
    use byteui::interaction::icons::IconKind;
    use dozer_panel_usage::adapter::PanelHeadKind;
    match kind {
        PanelHeadKind::AgentFilter => IconKind::Bot,
        PanelHeadKind::Content => IconKind::BarChart3,
    }
}

/// 把库排空的提示按当前语言渲染成 `(级别, 文本)`。key 缺失时库会落到英文 fallback。
pub(crate) fn render_notices(
    notices: Vec<dozer_panel_usage::adapter::UsageNotice>,
    locale: &str,
) -> Vec<(String, String)> {
    notices
        .into_iter()
        .map(|n| (format!("{:?}", n.level), dozer_panel_usage::i18n::resolve(&n.text, locale)))
        .collect()
}
```

(c) `take_notices` 改成:

```rust
    pub fn take_notices(&mut self) -> Vec<(String, String)> {
        render_notices(self.state.take_outbox(), self.locale.code())
    }
```

(d) `sync_native` 把语言上下文传下去:

```rust
        self.native.sync(window, visible, rect, &self.state, self.locale.stamp())
```

(e) `content_view` / `list_view` 给库视图传 `locale`:

```rust
        dozer_panel_usage::view::content_pane(
            self,
            collapsed,
            &self.state,
            Length::Fill,
            Border::default(),
            self.locale.code(),
        )
```

```rust
        dozer_panel_usage::view::list_pane(
            self,
            &self.state,
            Length::Fill,
            Border::default(),
            self.locale.code(),
        )
```

(f) `impl PanelDisplay<Msg> for UsagePanel` 里:

`panel_head` 加 `kind` 参数并改成用 `head_icon`(删掉 `match title { "Agent" => … }`):

```rust
    fn panel_head<'a>(
        &self,
        kind: dozer_panel_usage::adapter::PanelHeadKind,
        title: &'a str,
        _actions: Option<Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer>>,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        use iced::widget::{column, container, row, text};
        let tokens = byteui::theme::color::current();
        let icon = head_icon(kind);
```

(其余 `row![…]` 部分保持原样。)

`list_collapse_button` 的两个提示参数改成库 trait 声明的形态,并转成 `Cow`:

```rust
        tooltip_collapse: impl Into<std::borrow::Cow<'a, str>>,
        tooltip_expand: impl Into<std::borrow::Cow<'a, str>>,
```

```rust
        let (icon, tip): (_, std::borrow::Cow<'a, str>) = if collapsed {
            (IconKind::PanelRightOpen, tooltip_expand.into())
        } else {
            (IconKind::PanelRightClose, tooltip_collapse.into())
        };
```

- [ ] **Step 8: 编译并检查依赖图只有一份**

```bash
cd $ROOT/digger
cargo build --workspace 2>&1 | grep -E "^(error|warning: unused)|Finished"
grep -c 'name = "bytehost-core"' Cargo.lock
grep -c 'name = "byteui"' Cargo.lock
grep -n "\[patch" Cargo.toml || echo "no patch tables"
```

Expected: `Finished`,没有 `error`;两个计数都是 `1`;输出 `no patch tables`。计数不是 1 说明某个依赖仍在引用旧 tag,先找出来(`cargo tree -d | grep -E "bytehost-core|byteui"`),不要继续。

- [ ] **Step 9: 跑新测试与相关测试**

```bash
cd $ROOT/digger
cargo test -p digger-desktop --lib locale:: 2>&1 | grep -E "^test |test result"
cargo test -p digger-desktop --lib adapters:: 2>&1 | grep -E "^test |test result"
cargo test -p digger-app-host 2>&1 | grep -E "test result|FAILED"
```

Expected: `locale::tests` 5 个、`adapters::usage::tests` 原有 3 个 + 新增 3 个全部 `ok`;`digger-app-host` 全过。

- [ ] **Step 10: 全量门禁**

```bash
cd $ROOT/digger
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
bash scripts/check-host-deps.sh
```

Expected: fmt 无输出;clippy 无 error;`digger host-deps check: ok`。fmt 不干净就 `cargo fmt --all` 后只提交本任务触碰的文件。clippy 有既有的警告不是本任务引入的,要先 `git stash` 对比基线再判断,不要顺手改无关代码。

- [ ] **Step 11: 提交**

```bash
cd $ROOT/digger
git add Cargo.toml Cargo.lock crates/desktop/Cargo.toml crates/desktop/src/locale.rs \
  crates/desktop/src/main.rs crates/desktop/src/adapters/usage.rs \
  crates/desktop/src/adapters/usage_native.rs crates/desktop/src/apps/mod.rs \
  crates/app-host/src/testing.rs
git status --short   # 确认没有无关文件被带进来
git commit -m "feat(i18n): adopt bytehost v0.4.0 / byteui v0.4.3 and the shared i18n contract

Drop the dev [patch] worktrees; pin released tags and Dozer 7cdbffd0.
Add UiLocale (default zh-CN, DIGGER_UI_LOCALE override) and thread it into the
Usage panel: library views, Stamped webview pushes, rendered notices. Head
icons now follow PanelHeadKind instead of matching the title string.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 真实窗口验收与文档

**Files:**
- Modify: `README.md`(依赖与语言说明各一句)
- Modify: `docs/superpowers/plans/2026-10-15-digger-i18n-adoption.md`(勾选、记录结果)

**Interfaces:**
- Consumes: Task 1 的全部产出。
- Produces: 一份可复查的验收记录(本文件末尾的「验收记录」小节)。

- [ ] **Step 1: 全量测试(含真实窗口自检)**

真实窗口那几项很慢(约 10 分钟),放后台跑,不要加 `-q` 以外的过滤:

```bash
cd $ROOT/digger
cargo test --workspace --no-fail-fast 2>&1 | grep -E "^test result|FAILED|^error" | sort | uniq -c
```

Expected: 没有 `FAILED`、没有 `error`;所有 `test result` 行都是 `ok`。把输出贴进「验收记录」。

- [ ] **Step 2: 用量配对的真实窗口场景(默认语言)**

```bash
cd $ROOT/digger
scripts/run-editor-selftest.sh usage       # 用量配对(S2):读用量图表 webview 的真实 NSView
scripts/run-editor-selftest.sh lifecycle   # 窗口骨架 + 面板生命周期
echo "exit=$?"
```

Expected: 两个场景都以退出码 0 结束(任何一项失败或没跑完都是非零)。需要图形会话且屏幕亮着、未锁屏。记录命令、退出码和脚本末尾的汇总行。

- [ ] **Step 3: 英文覆盖下启动一次(人工目视)**

```bash
cd $ROOT/digger
DIGGER_UI_LOCALE=en cargo run -p digger-desktop
```

人工检查并记录:打开右侧「用量」面板——① 面板标题为英文 `Usage`、筛选栏标题 `Agent`,且两处图标分别是柱状图和机器人(不随语言错位);② 折叠/展开按钮的提示是英文;③ 图表区域(webview)里的文字是英文。再不带环境变量重启一次,三处都应是中文。这一步是 L4 人工项,做不了就在记录里写 `not-run` 和原因,**不要写成通过**。

- [ ] **Step 4: 文档**

Edit `README.md`。它是文档索引,没有构建章节,所以只在 S2 那一行(`- [S2 用量面板接入…]`)之后加一行索引:

```markdown
- [Digger 接入共享 i18n 契约(bytehost v0.4.0 / byteui v0.4.3)](docs/superpowers/plans/2026-10-15-digger-i18n-adoption.md):依赖改用已发布 tag,不再需要兄弟目录 worktree;界面语言默认 `zh-CN`,可用 `DIGGER_UI_LOCALE=en` 覆盖(目前只影响共享的用量面板)
```

- [ ] **Step 5: 填写验收记录并提交**

把 Step 1–3 的真实输出写进本文件末尾新增的「## 验收记录」小节(命令、退出码、结果、`not-run` 项及原因),并把本计划已完成步骤的复选框勾上。

```bash
cd $ROOT/digger
git add README.md docs/superpowers/plans/2026-10-15-digger-i18n-adoption.md
git commit -m "docs(i18n): record Digger i18n adoption acceptance and update README

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: 收尾核对(不推送)**

```bash
cd $ROOT/digger
git status --short
git log --oneline -3
ls $ROOT/bytehost-chrome $ROOT/byteui-shell >/dev/null 2>&1 && echo "旧 worktree 仍在(可由用户决定何时清理)"
```

不要删除 `bytehost-chrome` / `byteui-shell` 两个 worktree,也不要合并分支;把"可以清理旧 worktree、`feat/i18n-adoption` 待合并"留给用户决定。

---

## Self-Review

1. **Spec coverage:** 7 处编译错误逐一有归属——`E0050`/`E0049`/`E0277`/两处 `E0061`/`E0308` 在 Task 1 Step 5、7;`E0282`(其根因是 `build_app_child` 多一个参数)在 Step 4;另有被它们遮住的 `AppSummary.display_i18n` 在 Step 4。无遗漏的契约项:`install_i18n_bridge` 与 `display_i18n` 明确列入范围外并说明了取值。
2. **Placeholder scan:** 无 TBD/TODO;每个代码步骤都有完整代码;唯一依赖现场确认的是 Step 2 中 `scripts/` 的脚本名与 `serde_json` 是否已是依赖,计划里都给了判断依据与回退做法。
3. **Type consistency:** `UiLocale::code()` 返回 `&'static str`,在 `content_pane(…, locale: &str)`、`render_notices(…, locale: &str)` 处都按 `&str` 使用;`stamp()` 返回 `dozer_model::LocaleStamp`,与 `Native::sync` 的 `stamp` 参数、`Stamped::new(stamp, payload)` 一致;`head_icon` / `render_notices` 在测试与实现里同名同签名。
4. **Review Focus:** 六条各有对应测试或步骤;第 6 条由 Step 8 的锁文件计数和 Task 2 的真实窗口场景兜底。

## 验收记录

(Task 2 完成后填写。)
