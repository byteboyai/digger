//! Digger 的**应用宿主适配层**(Task 13)。
//!
//! 占住"产品组合根"与共享平台之间的一层:把 [`bytehost_panel`] 里与界面框架无关的状态机
//! (应用面板 / 安装审批 / 日志查看器)接到一个**真实的异步执行器**上——生产用进程内嵌入的
//! [`bytehost_client::in_process::InProcess`](独占一个 [`bytehost_apps::service::AppService`]),
//! 测试用 [`testing::FakeHost`]。不属于任何界面框架:不依赖 iced/wry/byteui。
//!
//! 职责(规格 §5.3 / §6.1):
//! - **执行器**([`effects::Executor`]):把状态机吐出的 `Effect` 翻成对 [`AppHostApi`] 的
//!   异步调用,结果包成带回信身份的消息回喂状态机。启动应用服务异步执行,不阻塞窗口。
//! - **订阅闭环**([`subscription`]):`Changed`/`Resync` 合并刷新,`Disconnected` 按共享退避
//!   规则重订阅并回退到轮询。
//! - **身份与失效**([`request`]):列表 / launch URL / 日志结果都带 scope 与 request 身份;
//!   卸载、面板关闭、generation 递增使旧的 UI 结果失效——但**写操作仍由服务完成**,不受影响。
//! - **单一服务 + 取消令牌**([`service::HostService`]):一个数据根一个服务所有者;退出时先
//!   停新操作、撤 Surface、停订阅,再 `shutdown_reporting` 等收尾。
//!
//! 依赖方向(规格 §4):`digger-app-host` 属于宿主适配层白名单,允许依赖 `bytehost-*`;
//! 业务层(domain/store/runtime)不得依赖本 crate,也不得直接依赖 `bytehost-*`(见
//! `scripts/check-host-deps.sh`)。

pub mod effects;
pub mod request;
pub mod service;
pub mod subscription;
pub mod testing;

use bytehost_apps::id::AppId;
use bytehost_panel::key::AppKey;

/// Digger 面板状态机用的应用键。
///
/// 与 Dozer 用"进程内槽号"不同,Digger 没有 webview 槽表:直接以 [`AppId`] 为键。
/// [`AppId`] 不是 `Copy`(内含 `String`),所以这里用一个可 `Copy` 的包装——它是 id 字符串
/// 的廉价句柄,靠一个进程内驻留表(interning)把 `&'static str` 借出去。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppKeyId(u32);

impl AppKeyId {
    /// 换回拥有所有权的 [`AppId`](id 已由宿主校验过)。
    pub fn into_app_id(self) -> AppId {
        AppId::new(interning::resolve(self)).expect("interned AppId was validated on insert")
    }
}

impl AppKey for AppKeyId {
    fn from_app_id(id: &AppId) -> Option<Self> {
        Some(interning::intern(id.as_str()))
    }

    fn app_id(&self) -> &str {
        interning::resolve(*self)
    }
}

/// 进程内 id 驻留表:`AppKeyId` → `&'static str`。
///
/// 面板状态机的 `AppKey` 要求 `Copy + 'static`,而 `AppId` 持有一段 `String`。这里把每次见到的
/// id 泄漏成一个 `&'static str` 并登记一个稳定序号——对已安装应用的集合(几十个、生命周期内基本不变)
/// 而言,cost 可忽略,却让 `AppKeyId` 成为真正的 `Copy` 键。同一个 id 只登记一次。
mod interning {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    use super::AppKeyId;

    struct Table {
        by_id: HashMap<String, AppKeyId>,
        by_key: Vec<&'static str>,
    }

    fn table() -> &'static Mutex<Table> {
        static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
        TABLE.get_or_init(|| {
            Mutex::new(Table {
                by_id: HashMap::new(),
                by_key: Vec::new(),
            })
        })
    }

    pub(super) fn intern(id: &str) -> AppKeyId {
        let mut t = table().lock().expect("app key table poisoned");
        if let Some(key) = t.by_id.get(id) {
            return *key;
        }
        // 泄漏一次:应用集合在进程生命周期内基本不变,换取 `Copy` 键。
        let leaked: &'static str = Box::leak(id.to_owned().into_boxed_str());
        let key = AppKeyId(u32::try_from(t.by_key.len()).expect("too many interned app ids"));
        t.by_key.push(leaked);
        t.by_id.insert(id.to_owned(), key);
        key
    }

    pub(super) fn resolve(key: AppKeyId) -> &'static str {
        let t = table().lock().expect("app key table poisoned");
        t.by_key[usize::try_from(key.0).expect("AppKeyId fits usize")]
    }
}
