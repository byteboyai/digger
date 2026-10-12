//! 单一应用服务实例与生命周期(规格 §6.1 / Task 13 第 5 条)。
//!
//! [`HostService`] 独占一个数据根的应用宿主。它**泛型于后端** `A: AppHostApi`——因为
//! `AppHostApi` 用 RPITIT 返回 `impl Future`,不是 dyn-safe 的,不能放进 `Box<dyn>`。生产里
//! `A = InProcess`(进程内嵌入的真实服务),测试里 `A = FakeHost`。同一个 [`HostService`] 与
//! [`Executor`](crate::effects::Executor) 于是不区分两种后端。
//!
//! 后端句柄放在 `Arc<RwLock<A>>` 之后,使得"启动占位 → 真正服务"的替换不需要改变类型:
//! 构造时装入不可用占位,后台启动完成后经 [`HostService::install_service`] 就地替换。
//!
//! 退出顺序(**先停新操作,再收尾**):
//! 1. [`HostService::begin_shutdown`] 置取消令牌 —— 之后任何派发一律被拒;
//! 2. 组合根撤 Surface(销毁 webview,不再接受业务输入)—— 宿主层做;
//! 3. 停订阅(丢弃 `AppChange` 接收端);
//! 4. [`HostService::await_shutdown`] —— `await` 服务 `shutdown_reporting`,拿可操作的退出结果。
//!    **失败不静默**:报告由组合根呈现为可操作错误。
//!
//! 数据根只允许一个所有者:互斥由 [`AppService`] 的 flock 保证(Task 12);第二个同根服务进入
//! `Unavailable`,不会扰动第一个。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytehost_apps::service::{AppService, ShutdownError, ShutdownReport};
use bytehost_client::AppHostApi;
use bytehost_client::in_process::InProcess;
use tokio::sync::RwLock;

/// 一次退出尝试的结果。
#[derive(Debug)]
pub enum ShutdownOutcome {
    /// 服务正常收尾(报告里仍可能列出没停下的应用——那是"部分失败",要呈现)。
    Reported(ShutdownReport),
    /// 退出没能干净完成(超时或内部任务失败);组合根据此显示可操作错误。
    Failed(ShutdownError),
    /// 服务从未成功启动过(没有可收尾的 `AppService`)。
    NeverStarted,
}

/// Digger 独占的应用服务宿主。生产里 `A = InProcess`。
pub struct HostService<A: AppHostApi + 'static> {
    api: Arc<RwLock<A>>,
    /// 真正启动的底层服务(用于 `shutdown_reporting`);启动完成前 / 测试后端下是 `None`。
    service: Option<Arc<AppService>>,
    /// 取消令牌:置位后拒绝一切新操作(派发前核对)。
    cancelled: Arc<AtomicBool>,
}

impl HostService<InProcess> {
    /// 以"尚未就绪"的占位开始:窗口先画出来,服务后台启动。
    pub fn starting() -> Self {
        Self::with_api(InProcess::unavailable("应用服务正在启动"), None)
    }

    /// 以不可用原因开始(启动直接失败,或产品决定不启用应用功能)。
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::with_api(InProcess::unavailable(reason), None)
    }

    /// 启动完成:记录底层服务句柄(供 shutdown 用),并在锁空闲时就地把后端换成真服务。
    ///
    /// 生产路径里 genesis 任务已在 tokio 工作线程用 `write().await` 换过后端(窗口线程不能阻塞
    /// 等锁),这里的写入是等价的后备;若此刻恰有并发读(把锁占住),跳过即可——后端已是真服务。
    /// **绝不用 `try_write().expect`**:有并发读时那会 panic。
    pub fn install_service(&mut self, in_process: Arc<InProcess>) {
        self.service = Some(in_process.service().clone());
        if let Ok(mut guard) = self.api.try_write() {
            *guard = InProcess::new(in_process.service().clone());
        }
    }
}

impl<A: AppHostApi + 'static> HostService<A> {
    /// 用任意后端构造(测试传 `FakeHost`);`service` 只在生产(真实 `AppService`)时给出。
    pub fn with_api(api: A, service: Option<Arc<AppService>>) -> Self {
        Self::with_api_lock(Arc::new(RwLock::new(api)), service)
    }

    /// 直接以共享的后端句柄构造(测试里 `Arc<FakeHost>` 常被多处引用)。
    pub fn with_api_lock(api: Arc<RwLock<A>>, service: Option<Arc<AppService>>) -> Self {
        Self {
            api,
            service,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 当前后端的共享句柄(执行器据此派发;读锁短持有,不跨 await)。
    pub fn api(&self) -> Arc<RwLock<A>> {
        self.api.clone()
    }

    /// 取消令牌的句柄。
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken {
            flag: self.cancelled.clone(),
        }
    }

    /// 是否已开始退出。
    pub fn is_shutting_down(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 退出第 1 步:置取消令牌。**此后派发一律拒绝**,新操作不再发起。
    pub fn begin_shutdown(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// 退出第 4 步:`await` 服务收尾,拿到可操作的退出结果。
    ///
    /// 服务从未启动过 → [`ShutdownOutcome::NeverStarted`];否则 `await` `shutdown_reporting`
    /// (内部有 `SHUTDOWN_TIMEOUT`,超时也会返回,不会永远挂住)。
    pub async fn await_shutdown(self) -> ShutdownOutcome {
        let Some(service) = self.service else {
            return ShutdownOutcome::NeverStarted;
        };
        match service.shutdown_reporting().await {
            Ok(report) => ShutdownOutcome::Reported(report),
            Err(err) => ShutdownOutcome::Failed(err),
        }
    }
}

/// 取消令牌:可克隆、跨线程;置位后 [`CancelToken::is_cancelled`] 恒真。
#[derive(Debug, Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn a_never_started_service_reports_never_started() {
        let host = HostService::starting();
        let outcome = rt().block_on(host.await_shutdown());
        assert!(matches!(outcome, ShutdownOutcome::NeverStarted));
    }

    #[test]
    fn begin_shutdown_flips_the_cancel_token() {
        let host = HostService::starting();
        let token = host.cancel_token();
        assert!(!token.is_cancelled());
        host.begin_shutdown();
        assert!(token.is_cancelled());
        assert!(host.is_shutting_down());
    }

    #[test]
    fn a_fake_backend_is_usable_generically() {
        let fake = FakeHost::new();
        fake.on_list(vec![]);
        let host = HostService::with_api(fake, None);
        let api = host.api();
        let apps = rt()
            .block_on(async { api.read().await.app_list().await })
            .unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn the_placeholder_api_is_unavailable_until_a_service_is_installed() {
        let host = HostService::starting();
        let api = host.api();
        let err = rt()
            .block_on(async { api.read().await.app_list().await })
            .expect_err("占位不可用");
        use bytehost_apps::proto::AppErrorKind;
        assert_eq!(err.kind(), Some(AppErrorKind::Unavailable));
    }
}
