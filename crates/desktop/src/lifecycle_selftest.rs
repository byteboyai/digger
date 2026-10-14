//! 面板生命周期自检(ADR-013 验证项 5):让真实的产品壳自己切页签、改窗口大小、关闭,
//! 每一步之后读**真实 NSView** 的可见性、位置、大小和句柄数,与期望比较。
//!
//!   DIGGER_LIFECYCLE_SELFTEST=1 digger        (通常经 scripts/run-editor-selftest.sh lifecycle)
//!
//! 全部通过退出码 0;任何一项失败、或面板没有按时就绪,退出码非 0。

use std::time::{Duration, Instant};

use iced::{Size, Task};

use crate::markdown::{BAR_HEIGHT, Probe};
use crate::product::{ActivePanel, Msg, ProductShell};

pub const ENV: &str = "DIGGER_LIFECYCLE_SELFTEST";

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    /// 动作之间的停顿,什么也不做
    Pause,
    Act,
    Probe,
    Check,
}

#[derive(Clone, Copy)]
enum Action {
    Select(ActivePanel),
    Resize(f32, f32),
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Visible,
    Hidden,
    Destroyed,
}

struct Step {
    name: &'static str,
    actions: &'static [Action],
    expect: Expect,
}

const STEPS: &[Step] = &[
    Step { name: "L1 the webview is shown, inset by the bar, when the editor tab is active", actions: &[], expect: Expect::Visible },
    Step { name: "L2 switching to another panel hides the webview (it stays alive)", actions: &[Action::Select(ActivePanel::Info)], expect: Expect::Hidden },
    Step { name: "L3 it stays hidden while a third panel is active", actions: &[Action::Select(ActivePanel::Apps)], expect: Expect::Hidden },
    Step { name: "L4 switching back shows it again at the same place", actions: &[Action::Select(ActivePanel::Markdown)], expect: Expect::Visible },
    Step { name: "L5 enlarging the window moves and resizes it", actions: &[Action::Resize(1000.0, 760.0)], expect: Expect::Visible },
    Step { name: "L6 shrinking the window moves and resizes it", actions: &[Action::Resize(820.0, 640.0)], expect: Expect::Visible },
    Step {
        name: "L7 a resize that happens while hidden is applied when it comes back",
        actions: &[Action::Select(ActivePanel::Info), Action::Resize(1100.0, 780.0), Action::Select(ActivePanel::Markdown)],
        expect: Expect::Visible,
    },
    Step { name: "L8 closing destroys the webview and the handle table is empty again", actions: &[Action::Close], expect: Expect::Destroyed },
];

/// 状态机:每一步 Act(做动作)→ 等 → Probe(读真实 view)→ 等 → Check(比较)。
pub struct Driver {
    step: usize,
    started: Instant,
    passes: u32,
    fails: u32,
}

const SETTLE: Duration = Duration::from_millis(700);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

fn after(delay: Duration, phase: Phase) -> Task<Msg> {
    Task::perform(tokio::time::sleep(delay), move |_| Msg::Lifecycle(phase))
}

impl Driver {
    pub fn from_env() -> Option<Self> {
        std::env::var_os(ENV).map(|_| Self { step: 0, started: Instant::now(), passes: 0, fails: 0 })
    }

    pub fn first() -> Task<Msg> {
        Task::done(Msg::Lifecycle(Phase::Act))
    }

    /// 面板内容区的期望矩形:与 `ProductShell::sync_native` 的口径独立重写一遍,而不是调用它。
    fn expected(size: Size) -> (f64, f64, f64, f64) {
        let (w, h) = (size.width as f64, size.height as f64);
        (20.0, 180.0 + BAR_HEIGHT, w - 40.0, h - 200.0 - BAR_HEIGHT)
    }

    pub fn advance(&mut self, shell: &mut ProductShell, phase: Phase) -> Task<Msg> {
        match phase {
            Phase::Pause => Task::none(),
            Phase::Act => self.act(shell),
            Phase::Probe => match shell.window_id() {
                Some(id) => shell.markdown_probe(id).chain(after(SETTLE / 2, Phase::Check)),
                None => self.fail_now("no window"),
            },
            Phase::Check => self.check(shell),
        }
    }

    fn act(&mut self, shell: &mut ProductShell) -> Task<Msg> {
        let Some(id) = shell.window_id() else {
            return self.wait_or_die("the window never became ready");
        };
        // 第一步先等页面说过 ready:之后才有「正常显示」可言
        if self.step == 0 && !shell.markdown_ready() {
            return self.wait_or_die("the editor page never became ready");
        }
        // 动作逐个排队,每个之间留一点时间让界面处理完;最后统一等 SETTLE 再读真实 view
        let mut chain: Task<Msg> = Task::none();
        for action in STEPS[self.step].actions {
            let t = match *action {
                Action::Select(panel) => Task::done(Msg::SelectPanel(panel)),
                Action::Resize(w, h) => iced::window::resize(id, Size::new(w, h)),
                Action::Close => Task::done(Msg::Exit),
            };
            chain = chain.chain(t).chain(after(Duration::from_millis(250), Phase::Pause));
        }
        chain.chain(after(SETTLE, Phase::Probe))
    }

    fn wait_or_die(&mut self, why: &str) -> Task<Msg> {
        if self.started.elapsed() > READY_TIMEOUT {
            return self.fail_now(why);
        }
        after(Duration::from_millis(300), Phase::Act)
    }

    fn fail_now(&mut self, why: &str) -> Task<Msg> {
        println!("FAIL: lifecycle selftest could not run: {why}"); // cli-output
        std::process::exit(1);
    }

    fn check(&mut self, shell: &mut ProductShell) -> Task<Msg> {
        let step = &STEPS[self.step];
        let probe = shell.markdown_last_probe();
        let (ok, detail) = judge(step.expect, probe, Self::expected(shell.window_size()));
        println!("{}: {}{}", if ok { "PASS" } else { "FAIL" }, step.name, if detail.is_empty() { String::new() } else { format!("\n      {detail}") }); // cli-output
        if ok { self.passes += 1 } else { self.fails += 1 }
        self.step += 1;
        if self.step < STEPS.len() {
            return Task::done(Msg::Lifecycle(Phase::Act));
        }
        println!("SUMMARY panel lifecycle selftest: {} passed, {} failed", self.passes, self.fails); // cli-output
        std::process::exit(i32::from(self.fails > 0));
    }
}

fn near(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() <= 1.0 && (a.1 - b.1).abs() <= 1.0 && (a.2 - b.2).abs() <= 1.0 && (a.3 - b.3).abs() <= 1.0
}

/// 把真实 view 的状态与期望比较。纯函数,可单测。
fn judge(expect: Expect, probe: Option<Probe>, want: (f64, f64, f64, f64)) -> (bool, String) {
    let Some(p) = probe else {
        return (false, "no probe result arrived".into());
    };
    let detail = format!("probe {p:?}, want bounds {want:?}");
    let ok = match expect {
        Expect::Visible => p.exists && p.visible && p.handles == 1 && p.bounds.is_some_and(|b| near(b, want)),
        Expect::Hidden => p.exists && !p.visible && p.handles == 1,
        Expect::Destroyed => !p.exists && p.handles == 0,
    };
    (ok, if ok { String::new() } else { detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANT: (f64, f64, f64, f64) = (20.0, 236.0, 960.0, 504.0);
    fn probe(exists: bool, visible: bool, bounds: Option<(f64, f64, f64, f64)>, handles: usize) -> Option<Probe> {
        Some(Probe { exists, visible, bounds, handles })
    }

    #[test]
    fn judge_accepts_the_right_state_and_refuses_each_wrong_one() {
        assert!(judge(Expect::Visible, probe(true, true, Some(WANT), 1), WANT).0);
        assert!(judge(Expect::Hidden, probe(true, false, Some(WANT), 1), WANT).0);
        assert!(judge(Expect::Destroyed, probe(false, false, None, 0), WANT).0);
        // visible but hidden / wrong place / wrong size / leaked second handle / destroyed but a handle is left / no probe
        assert!(!judge(Expect::Visible, probe(true, false, Some(WANT), 1), WANT).0);
        assert!(!judge(Expect::Visible, probe(true, true, Some((20.0, 180.0, 960.0, 504.0)), 1), WANT).0);
        assert!(!judge(Expect::Visible, probe(true, true, Some((20.0, 236.0, 900.0, 504.0)), 1), WANT).0);
        assert!(!judge(Expect::Visible, probe(true, true, Some(WANT), 2), WANT).0);
        assert!(!judge(Expect::Hidden, probe(true, true, Some(WANT), 1), WANT).0);
        assert!(!judge(Expect::Hidden, probe(false, false, None, 0), WANT).0, "hidden is not destroyed");
        assert!(!judge(Expect::Destroyed, probe(true, false, Some(WANT), 1), WANT).0);
        assert!(!judge(Expect::Destroyed, probe(false, false, None, 1), WANT).0);
        assert!(!judge(Expect::Visible, None, WANT).0);
    }

    #[test]
    fn the_expected_rectangle_follows_the_window_size() {
        assert_eq!(Driver::expected(Size::new(1000.0, 760.0)), WANT);
    }
}
