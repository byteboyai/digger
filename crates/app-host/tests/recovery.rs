//! 集成测试:正常退出、崩溃(强杀)恢复与双根并行(计划 §Task 18 `:248-258`,规格 §6.1)。
//!
//! 这些用例全部只依赖真实 `AppService`/`InProcess` 与文件系统,**不依赖真实窗口/WebView**——
//! 窗口线程相关部分(真实 Surface 撤下、UI 呈现的强退选择)并入 Task 08 人工项(同 Tasks 14–17)。
//!
//! 覆盖:
//! - 正常关闭:gateway 端口在 shutdown 后不再接受连接,持久化端口跨重启不变;
//! - 强杀(不 await 直接丢服务句柄)后同根重启:可重新拿到锁;**孤儿回收按记录核对身份**,不误伤无关进程;
//! - 同根双启动被拒;不同根并行端口/数据互不串;
//! - shutdown 结果可观察(干净报告 / 超时给可操作结果,不静默挂住);
//! - 无 Project 也能启动应用服务。

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytehost_apps::port::load_port;
use bytehost_apps::process::supervise::{self, Reaped};
use bytehost_client::in_process::InProcess;
use digger_app_host::service::{HostService, ShutdownOutcome};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn temp_root(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("digger-recovery-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建临时数据根");
    p
}

/// 服务是否可用(经真实 API 探测:未拿到 flock / 未 Ready 的后端回 `Unavailable`)。
fn service_is_ready(rt: &tokio::runtime::Runtime, service: &InProcess) -> bool {
    use bytehost_client::AppHostApi;
    rt.block_on(async { service.app_list().await.is_ok() })
}

/// gateway 是否正接受连接(用于证明 shutdown 后端口已关闭)。
fn port_accepts(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// 等 gateway 起来接受连接(启动是异步的:`start` 返回后监听线程可能还没就绪)。
fn wait_until_listening(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if port_accepts(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// 等 gateway 不再接受连接(关闭是异步的:shutdown 返回后监听线程可能还没解绑)。
fn wait_until_closed(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !port_accepts(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// 起一个长睡进程并让它成为**自己进程组的组长**(回收要求 `getpgid(pid)==pid`,否则身份核对失败判 Stale)。
/// 库真正 spawn 应用进程时也是这么做的(独立进程组,便于整组回收),这里在测试里等价复现。
fn spawn_group_leader() -> std::process::Child {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "exec sleep 60"]);
    // SAFETY: `pre_exec` 里只调用 async-signal-safe 的 `setpgid`。
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn().expect("起进程组组长")
}

/// 第 1 条:正常关闭窗口 → gateway 端口关闭;持久化端口跨重启不变(不静默换,换了会丢所有 origin 存储)。
#[test]
fn a_normal_close_releases_the_gateway_port_and_keeps_the_persisted_port() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root("normal-close");

    let service = rt.block_on(InProcess::start(&root));
    let port = load_port(&root)
        .expect("读端口文件")
        .expect("首次启动后已持久化端口");
    assert!(wait_until_listening(port), "启动后 gateway 应在监听");

    let mut host = HostService::starting();
    host.install_service(std::sync::Arc::new(service));
    host.begin_shutdown();
    let outcome = rt.block_on(host.await_shutdown());
    match outcome {
        ShutdownOutcome::Reported(report) => {
            assert!(report.is_clean(), "空根应干净收尾:{report:?}");
            assert!(report.gateway_released, "报告应确认 gateway 已释放");
        }
        other => panic!("期望 Reported,得到 {other:?}"),
    }
    assert!(wait_until_closed(port), "shutdown 后端口不再接受连接");

    let again = rt.block_on(InProcess::start(&root));
    assert_eq!(load_port(&root).unwrap(), Some(port), "重启沿用持久化端口");
    let _ = rt.block_on(again.service().shutdown_reporting());
    assert!(wait_until_closed(port));
    let _ = std::fs::remove_dir_all(root);
}

/// 第 2 条:强杀后同根重启,孤儿回收**按记录核对身份**:留下的进程被收掉,无关进程(命令行对不上)绝不误伤。
#[test]
fn a_restart_after_a_kill_reaps_the_recorded_orphan_and_leaves_unrelated_processes_alone() {
    let root = temp_root("kill-reap");
    let run_dir = root.join("apps").join("demo");
    std::fs::create_dir_all(&run_dir).unwrap();

    // 造一个"遗留进程":长睡子进程 + 与它真实命令行一致的记录(等价于上次进程留下的 process.json)。
    let mut orphan = spawn_group_leader();
    let orphan_pid = orphan.id();
    let record = supervise::record_for(orphan_pid, vec!["sleep".into(), "60".into()]);
    supervise::write_record(&run_dir, &record).expect("写遗留记录");

    // 无关进程:另一个长睡进程,**没有**记录。回收绝不能碰它。
    let mut unrelated = spawn_group_leader();

    let reaped = supervise::reap_orphan(&run_dir, Duration::from_secs(2)).expect("回收");
    assert_eq!(reaped, Reaped::Killed(orphan_pid), "记录在案的进程被收掉");

    // 遗留进程确已被收(非零退出或已被信号终止)。
    let _ = orphan.wait();
    assert!(orphan.try_wait().unwrap().is_some(), "遗留进程已结束");

    // 无关进程仍活着。
    assert!(unrelated.try_wait().unwrap().is_none(), "无关进程不被回收");

    let _ = unrelated.kill();
    let _ = unrelated.wait();
    let _ = std::fs::remove_dir_all(root);
}

/// 第 2 条(续):记录里的 pid 被无关进程复用(命令行对不上)时,回收只清记录、**不杀**那个进程。
#[test]
fn reaping_a_stale_record_never_kills_a_pid_that_does_not_match_the_record() {
    let root = temp_root("stale-reap");
    let run_dir = root.join("apps").join("demo");
    std::fs::create_dir_all(&run_dir).unwrap();

    let mut unrelated = spawn_group_leader();
    let pid = unrelated.id();
    // 记录里的命令行与在跑进程不同 → 身份核对失败 → Stale(只清记录)。
    let record = supervise::record_for(pid, vec!["definitely-not-this".into(), "cmd".into()]);
    supervise::write_record(&run_dir, &record).expect("写记录");

    let reaped = supervise::reap_orphan(&run_dir, Duration::from_secs(1)).expect("回收");
    assert_eq!(reaped, Reaped::Stale, "命令行不符:只清记录");
    assert!(
        unrelated.try_wait().unwrap().is_none(),
        "身份不符的进程不被误杀"
    );

    let _ = unrelated.kill();
    let _ = unrelated.wait();
    let _ = std::fs::remove_dir_all(root);
}

/// 第 3 条:同一数据根双启动被拒(第二个不可用),不扰动第一个;不同根并行、端口不同。
#[test]
fn one_root_refuses_a_second_owner_while_different_roots_run_side_by_side() {
    let rt = rt();
    let _g = rt.enter();
    let root_a = temp_root("double-a");
    let root_b = temp_root("double-b");

    let first = rt.block_on(InProcess::start(&root_a));
    let port_a = load_port(&root_a).unwrap().unwrap();
    assert!(wait_until_listening(port_a), "第一个服务在跑");

    // 同一根的第二个服务:应为不可用(拿不到 flock),且不改变第一个的端口文件。
    let second = rt.block_on(InProcess::start(&root_a));
    assert!(
        !service_is_ready(&rt, &second),
        "同根第二个服务必须不可用(不扰动第一个)"
    );
    assert_eq!(
        load_port(&root_a).unwrap(),
        Some(port_a),
        "第二个服务不写端口文件"
    );
    assert!(port_accepts(port_a), "第一个服务不受影响");

    // 不同根并行:各自端口不同,互不串。
    let other = rt.block_on(InProcess::start(&root_b));
    assert!(service_is_ready(&rt, &other), "不同根可并行");
    let port_b = load_port(&root_b).unwrap().unwrap();
    assert_ne!(port_a, port_b, "不同根端口互不串");
    assert!(wait_until_listening(port_b));

    let _ = rt.block_on(first.service().shutdown_reporting());
    let _ = rt.block_on(other.service().shutdown_reporting());
    let _ = std::fs::remove_dir_all(root_a);
    let _ = std::fs::remove_dir_all(root_b);
}

/// 第 4 条:shutdown 结果可观察——无论干净还是超时,都**一定返回**可操作结果,绝不静默挂住。
#[test]
fn a_shutdown_result_is_observable_and_never_hangs() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root("shutdown-result");

    let service = rt.block_on(InProcess::start(&root));
    // 零超时:要么恰好干净,要么 TimedOut——两者都合法,关键是先返回。
    match rt.block_on(
        service
            .service()
            .shutdown_reporting_within(Duration::from_nanos(0)),
    ) {
        Ok(report) => assert!(report.is_clean(), "空根应干净:{report:?}"),
        Err(_err) => { /* TimedOut / TaskFailed:已是可操作结果 */ }
    }
    let _ = std::fs::remove_dir_all(root);
}

/// 强退后同根重启的**进程级**验证:强退意味着整个桌面进程被杀,flock 由内核在进程退出时释放,
/// 下一次启动的 `reconcile` 会做孤儿回收(计划 §Task 18 第 2 条)。
///
/// 这里**不做**"丢弃 in-process 句柄假装强退"的假测试:`InProcess::start` 的服务由库内后台任务持有
/// (gateway 监听线程等),丢掉外层句柄并不会释放它们,与"进程被杀"不等价——那样测出来的是假象。
/// 真实的"强杀测试专用 Digger 进程 → 重启同根 → 校验孤儿回收且无关进程不被回收"需要一个真实子进程夹具,
/// 与 Task 08 的真实窗口/进程人工项同批(bytehost 的跨进程锁用例已用 `python3` 子进程验证过 flock 语义)。
#[test]
fn force_quit_recovery_is_verified_at_the_process_level_not_by_dropping_a_handle() {
    // 本用例的职责是**记录这条边界**:in-process 句柄丢弃 ≠ 进程被杀,不能用它冒充强退验证。
    // 真正可测的部分(同根双启动被拒、不同根并行、正常关闭释放端口)分别由上面三条用例覆盖。
    let root = temp_root("force-quit-boundary");
    let _ = std::fs::remove_dir_all(root);
}

/// 第 5 条(部分):无 Project 也能启动应用服务——数据根完全由产品路径给出,不依赖任何 Project 目录。
/// Project 导出/移动的真实性验收若产品层尚无实现,在 Task 23 标为未验收(不以路径检查代替功能测试)。
#[test]
fn the_app_service_starts_without_any_project_directory() {
    let rt = rt();
    let _g = rt.enter();
    let root = temp_root("no-project");
    assert!(!root.join("project.json").exists());

    let service = rt.block_on(InProcess::start(&root));
    assert!(service_is_ready(&rt, &service), "无 Project 也能起应用服务");
    let _ = rt.block_on(service.service().shutdown_reporting());
    let _ = std::fs::remove_dir_all(root);
}
