//! 受控端口重置的**顺序契约**(Task 17 / 规格 §6.1)。
//!
//! Digger **不直接编辑 `gateway.json`**——改端口只有一条受控路径:先停服务(释放同根 flock),
//! 再用 bytehost 提供的 [`AppService::reset_port`]/[`AppService::clear_port`] 管理接口。顺序错一步
//! 就会失败或被拒:
//!
//! 1. **停服务**:`HostService::begin_shutdown` + `await_shutdown`(锁在 shutdown 后释放);
//! 2. **撤所有应用 Surface**(webview 使用旧 origin,必须销毁);
//! 3. **改端口**:`AppService::reset_port(root, requested)`(服务在跑时会返回 `ServiceRunning`);
//! 4. **重启 + 重订阅**:重新 `AppService::start(root)`,把新后端装回,订阅继续。
//!
//! origin 会从 `http://<id>.localhost:<旧端口>` 变成 `http://<id>.localhost:<新端口>`:已打开的应用
//! 页面必须重开(旧 origin 的会话/服务可能失效),这正是第 2 步要撤 Surface、第 4 步之后由产品重取
//! launch URL 的原因。本模块把这条顺序固化成**一个纯函数**(可单测),真正的异步停/起由组合根驱动
//! (窗口线程不能阻塞等锁)。
//!
//! 为什么单独一层:端口重置是**唯一**允许改 gateway.json 的口子,写错会破坏注册表与已存数据;把
//! "先停后改、改完再起"的顺序集中到这里,任何人接设置页都只需按 [`PortResetPlan`] 执行。

use std::path::Path;

use bytehost_apps::service::{AppService, PortResetError};

/// 一次端口重置的**顺序计划**:组合根据此一步步执行(每一步都可能失败,失败即中止并保留旧端口)。
///
/// 顺序不可调换:先停服务(否则 `reset_port` 返回 `ServiceRunning`)、撤 Surface(否则旧 origin 的
/// webview 悬着)、改端口(此时服务已停、锁已释放)、再起服务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortResetPlan {
    /// 已持久化/正在使用的旧端口(用于告诉用户 origin 会怎么变)。
    pub old_port: Option<u16>,
    /// 用户请求的新端口。
    pub requested: u16,
}

impl PortResetPlan {
    pub fn new(old_port: Option<u16>, requested: u16) -> Self {
        Self {
            old_port,
            requested,
        }
    }

    /// 第 3 步:执行受控改端口(**必须在服务已停、flock 已释放之后**调用)。
    ///
    /// 直接转发到 [`AppService::reset_port`];服务仍在跑会得到 [`PortResetError::ServiceRunning`]。
    /// 失败时**原 `gateway.json` 原样保留**,调用方仍可用旧端口重启。
    pub fn apply(&self, root: &Path) -> Result<u16, PortResetError> {
        AppService::reset_port(root, self.requested)
    }

    /// 恢复"端口文件损坏/不可用":忘掉已持久化的端口(下次 `start` 重新随机选并绑定)。
    /// 同样要求服务已停。用于 [`PortResetError::Io`] 后的受控恢复,不暴露任意写。
    pub fn clear(root: &Path) -> Result<(), PortResetError> {
        AppService::clear_port(root)
    }

    /// 新端口下某应用的 origin(给用户看"origin 变化影响";应用 id 不变,只有端口变)。
    ///
    /// 与 `bytehost_webview` 的 origin 形状一致:`http://<app-id>.localhost:<port>`。
    pub fn origin_after(&self, app_id: &str) -> String {
        format!("http://{app_id}.localhost:{}", self.requested)
    }

    /// 旧端口下某应用的 origin(对比展示用);没有旧端口时 `None`。
    pub fn origin_before(&self, app_id: &str) -> Option<String> {
        self.old_port
            .map(|p| format!("http://{app_id}.localhost:{p}"))
    }

    /// 端口是否确实会改变(相同端口无需重置流程)。
    pub fn changes_port(&self) -> bool {
        self.old_port != Some(self.requested)
    }
}

/// 端口重置流程在其生命周期中停在哪一步(供设置页展示进度/失败原因)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortResetStage {
    /// 第 1 步:停服务。
    Stopping,
    /// 第 2 步:撤所有应用 Surface。
    TearingDownSurfaces,
    /// 第 3 步:改端口。
    Applying,
    /// 第 4 步:重启并重订阅。
    Restarting,
    /// 完成:新端口生效。
    Done(u16),
    /// 失败:带上可操作原因(旧端口可继续用)。
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_after_reflects_the_requested_port() {
        let plan = PortResetPlan::new(Some(5000), 6000);
        assert_eq!(plan.origin_after("notes"), "http://notes.localhost:6000");
        assert_eq!(
            plan.origin_before("notes").as_deref(),
            Some("http://notes.localhost:5000")
        );
    }

    #[test]
    fn the_missing_old_port_has_no_before_origin() {
        let plan = PortResetPlan::new(None, 6000);
        assert!(plan.origin_before("notes").is_none());
        assert_eq!(plan.origin_after("notes"), "http://notes.localhost:6000");
    }

    #[test]
    fn requesting_the_same_port_is_not_a_change() {
        assert!(!PortResetPlan::new(Some(6000), 6000).changes_port());
        assert!(PortResetPlan::new(Some(5000), 6000).changes_port());
        assert!(PortResetPlan::new(None, 6000).changes_port());
    }

    /// `apply` 直接转发到 Task 12 的 `AppService::reset_port`:越界端口在**任何**锁检查之前就被拒,
    /// 因此这个用例不需要真的起服务就能证明"我们调的是那条受控接口、且它的入参校验生效"。
    #[test]
    fn apply_forwards_to_the_managed_reset_api() {
        let dir = std::env::temp_dir().join(format!("digger-port-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let plan = PortResetPlan::new(None, 1); // 远低于 PORT_MIN
        assert!(matches!(
            plan.apply(&dir),
            Err(PortResetError::OutOfRange(1))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
