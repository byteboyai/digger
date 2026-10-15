//! 窗口骨架 + 面板生命周期自检(ADR-013 验证项 5 的骨架版):让真实的产品壳自己点图标、收起 / 展开、拖分隔线、缩放窗口、
//! 把编辑器拖到另一条栏、放大、关闭,每一步之后读**真实 NSView** 的可见性、位置、大小和句柄数,与期望比较。
//!
//!   DIGGER_LIFECYCLE_SELFTEST=1 digger        (通常经 scripts/run-editor-selftest.sh lifecycle)
//!
//! 期望值由本文件**独立**算出(只用下面这几个字面量和算术,不调用骨架的几何函数),所以骨架的几何一旦与实际摆放脱节就会失败。
//! 全部通过退出码 0;任何一项失败、或面板没有按时就绪,退出码非 0。

use std::time::{Duration, Instant};

use bytehost_chrome::{Divider, ShellMsg, Side};
use iced::{Size, Task};

use crate::markdown::{BAR_HEIGHT, Probe};
use crate::product::{Msg, ProductShell};
use crate::shell;

pub const ENV: &str = "DIGGER_LIFECYCLE_SELFTEST";

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    /// 动作之间的停顿,什么也不做
    Pause,
    Act,
    Probe,
    Check,
}

/// 期望编辑器 webview 在哪。`pref` 是持久化的左宽偏好(夹取在算期望时独立做)。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Area {
    Left { pref: f64 },
    Right { pref: f64 },
    Maximized,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Visible(Area),
    Hidden,
    Destroyed,
}

enum Action {
    Msg(ShellMsg),
    Resize(f32, f32),
    Close,
}

struct Step {
    name: &'static str,
    actions: Vec<Action>,
    expect: Expect,
}

fn press(panel: bytehost_core::id::PanelId) -> Action {
    Action::Msg(ShellMsg::PanelPress { panel })
}

fn release() -> Action {
    Action::Msg(ShellMsg::PointerReleased)
}

fn steps() -> Vec<Step> {
    let editor = shell::markdown_id;
    let info = shell::info_id;
    vec![
        Step { name: "S1 the editor is shown in the left zone, below its own bar, with the default 640 px zone", actions: vec![], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step { name: "S2 selecting another panel on the same rail hides the editor (it stays alive)", actions: vec![press(info()), release()], expect: Expect::Hidden },
        Step { name: "S3 selecting the editor again shows it at the same place", actions: vec![press(editor()), release()], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step { name: "S4 clicking the active icon collapses the zone and hides the editor", actions: vec![press(editor()), release()], expect: Expect::Hidden },
        Step { name: "S5 clicking it again expands the zone and shows the editor", actions: vec![press(editor()), release()], expect: Expect::Visible(Area::Left { pref: 640.0 }) },
        Step {
            name: "S6 dragging the divider between the zones resizes the editor",
            actions: vec![Action::Msg(ShellMsg::DividerPress(Divider::Zones)), Action::Msg(ShellMsg::CursorMoved { x: 44.0 + 500.0, y: 300.0 }), release()],
            expect: Expect::Visible(Area::Left { pref: 500.0 }),
        },
        Step { name: "S7 enlarging the window moves and resizes it", actions: vec![Action::Resize(1000.0, 760.0)], expect: Expect::Visible(Area::Left { pref: 500.0 }) },
        Step {
            name: "S8 a resize that happens while the editor is hidden is applied when it comes back",
            actions: vec![press(info()), release(), Action::Resize(1100.0, 780.0), press(editor()), release()],
            expect: Expect::Visible(Area::Left { pref: 500.0 }),
        },
        Step { name: "S9 maximizing its zone fills the maximize box", actions: vec![Action::Msg(ShellMsg::Maximize(Side::Left))], expect: Expect::Visible(Area::Maximized) },
        Step {
            name: "S10 dragging the editor icon to the other rail moves it into the right zone",
            actions: vec![Action::Msg(ShellMsg::MaximizeClose), press(info()), release(), press(editor()), Action::Msg(ShellMsg::RailDragMove { side: Side::Right, index: 1 }), release()],
            expect: Expect::Visible(Area::Right { pref: 500.0 }),
        },
        Step { name: "S11 closing destroys the webview and the handle table is empty again", actions: vec![Action::Close], expect: Expect::Destroyed },
    ]
}

/// 状态机:每一步 Act(做动作)→ 等 → Probe(读真实 view)→ 等 → Check(比较)。
pub struct Driver {
    step: usize,
    steps: Vec<Step>,
    started: Instant,
    passes: u32,
    fails: u32,
}

const SETTLE: Duration = Duration::from_millis(700);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

fn after(delay: Duration, phase: Phase) -> Task<Msg> {
    Task::perform(tokio::time::sleep(delay), move |_| Msg::Lifecycle(phase))
}

// ---- 独立的期望值:只用字面量和算术 ----
const RAIL: f64 = 44.0;
const DIVIDER: f64 = 8.0;
const TOP_BAR: f64 = 40.0;
const FOOTBAR: f64 = 22.0;
const MIN_ZONE: f64 = 320.0;
const MAXIMIZE_PADDING: f64 = 40.0;

/// 编辑器 webview 的期望矩形 (x, y, w, h):面板内容矩形(区外框 − 外边距 − 1px 内边距)再减去编辑器自己的顶栏。
fn want(area: Area, window: (f32, f32)) -> (f64, f64, f64, f64) {
    let (w, h) = (window.0 as f64, window.1 as f64);
    let bar = BAR_HEIGHT;
    let zones = (w - 2.0 * RAIL - DIVIDER).max(0.0);
    let left_w = |pref: f64| pref.clamp(MIN_ZONE, (zones - MIN_ZONE).max(MIN_ZONE));
    let body_h = h - TOP_BAR - FOOTBAR;
    let (x, y, cw, ch) = match area {
        // 左区:外边距 上6 左6,内边距 1
        Area::Left { pref } => (RAIL + 6.0 + 1.0, TOP_BAR + 6.0 + 1.0, left_w(pref) - 6.0 - 2.0, body_h - 6.0 - 2.0),
        // 右区:外边距 上6 右6,内边距 1;它的外框从 左图标栏 + 左区 + 分隔线 之后开始
        Area::Right { pref } => (RAIL + left_w(pref) + DIVIDER + 1.0, TOP_BAR + 6.0 + 1.0, (zones - left_w(pref)) - 6.0 - 2.0, body_h - 6.0 - 2.0),
        // 放大盒子:两条图标栏之间、顶栏之下直到窗口底边,四周留 40
        Area::Maximized => (RAIL + MAXIMIZE_PADDING, TOP_BAR + MAXIMIZE_PADDING, w - 2.0 * RAIL - 2.0 * MAXIMIZE_PADDING, h - TOP_BAR - 2.0 * MAXIMIZE_PADDING),
    };
    (x, y + bar, cw, ch - bar)
}

impl Driver {
    pub fn from_env() -> Option<Self> {
        std::env::var_os(ENV).map(|_| Self { step: 0, steps: steps(), started: Instant::now(), passes: 0, fails: 0 })
    }

    pub fn first() -> Task<Msg> {
        Task::done(Msg::Lifecycle(Phase::Act))
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
        for action in &self.steps[self.step].actions {
            let t = match action {
                Action::Msg(m) => Task::done(Msg::Shell(m.clone())),
                Action::Resize(w, h) => iced::window::resize(id, Size::new(*w, *h)),
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
        let step = &self.steps[self.step];
        let (ok, detail) = judge(step.expect, shell.markdown_last_probe(), shell.window_size());
        println!("{}: {}{}", if ok { "PASS" } else { "FAIL" }, step.name, if detail.is_empty() { String::new() } else { format!("\n      {detail}") }); // cli-output
        if ok {
            self.passes += 1
        } else {
            self.fails += 1
        }
        self.step += 1;
        if self.step < self.steps.len() {
            return Task::done(Msg::Lifecycle(Phase::Act));
        }
        println!("SUMMARY shell lifecycle selftest: {} passed, {} failed", self.passes, self.fails); // cli-output
        std::process::exit(i32::from(self.fails > 0));
    }
}

/// 原生 view 的大小是取整的,所以容差取半个像素:够吸收取整,又抓得住 1 像素的几何漂移。
fn near(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    const TOLERANCE: f64 = 0.51;
    (a.0 - b.0).abs() <= TOLERANCE && (a.1 - b.1).abs() <= TOLERANCE && (a.2 - b.2).abs() <= TOLERANCE && (a.3 - b.3).abs() <= TOLERANCE
}

/// 把真实 view 的状态与期望比较。纯函数,可单测。
fn judge(expect: Expect, probe: Option<Probe>, window: (f32, f32)) -> (bool, String) {
    let Some(p) = probe else {
        return (false, "no probe result arrived".into());
    };
    let (ok, want_detail) = match expect {
        Expect::Visible(area) => {
            let want = want(area, window);
            (p.exists && p.visible && p.handles == 1 && p.bounds.is_some_and(|b| near(b, want)), format!("want bounds {want:?}"))
        }
        Expect::Hidden => (p.exists && !p.visible && p.handles == 1, "want hidden but alive".to_owned()),
        Expect::Destroyed => (!p.exists && p.handles == 0, "want destroyed, no handles".to_owned()),
    };
    (ok, if ok { String::new() } else { format!("probe {p:?}, {want_detail}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(exists: bool, visible: bool, bounds: Option<(f64, f64, f64, f64)>, handles: usize) -> Option<Probe> {
        Some(Probe { exists, visible, bounds, handles })
    }

    const WINDOW: (f32, f32) = (1440.0, 900.0);

    #[test]
    fn the_expected_rectangles_are_the_hand_computed_ones() {
        // left zone 640: content x 44+6+1, y 40+6+1, w 640-8, h 900-40-22-8; the editor bar (56) comes off the top
        assert_eq!(want(Area::Left { pref: 640.0 }, WINDOW), (51.0, 103.0, 632.0, 774.0));
        // a narrow window clamps the preference: W=700 ⇒ zones 604, upper max(604-320,320)=320
        assert_eq!(want(Area::Left { pref: 640.0 }, (700.0, 900.0)).2, 320.0 - 8.0);
        // right zone with left 500: zones 1344, right = 844; x = 44+500+8+1
        assert_eq!(want(Area::Right { pref: 500.0 }, WINDOW), (553.0, 103.0, 844.0 - 8.0, 774.0));
        // maximize box: x 84, y 80+56, w 1440-88-80, h 900-40-80-56
        assert_eq!(want(Area::Maximized, WINDOW), (84.0, 136.0, 1272.0, 724.0));
    }

    #[test]
    fn judge_accepts_the_right_state_and_refuses_each_wrong_one() {
        let w = want(Area::Left { pref: 640.0 }, WINDOW);
        let visible = Expect::Visible(Area::Left { pref: 640.0 });
        assert!(judge(visible, probe(true, true, Some(w), 1), WINDOW).0);
        assert!(judge(Expect::Hidden, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(judge(Expect::Destroyed, probe(false, false, None, 0), WINDOW).0);
        // hidden when it should show / wrong place / wrong size / leaked handle / not destroyed / no probe
        assert!(!judge(visible, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some((w.0, w.1 - 56.0, w.2, w.3)), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some((w.0, w.1, w.2 - 40.0, w.3)), 1), WINDOW).0);
        // a one pixel drift is caught, half a pixel of rounding is not
        assert!(!judge(visible, probe(true, true, Some((w.0 + 1.0, w.1, w.2, w.3)), 1), WINDOW).0);
        assert!(judge(visible, probe(true, true, Some((w.0 + 0.4, w.1, w.2 - 0.4, w.3)), 1), WINDOW).0);
        assert!(!judge(visible, probe(true, true, Some(w), 2), WINDOW).0);
        assert!(!judge(Expect::Hidden, probe(true, true, Some(w), 1), WINDOW).0);
        assert!(!judge(Expect::Hidden, probe(false, false, None, 0), WINDOW).0, "hidden is not destroyed");
        assert!(!judge(Expect::Destroyed, probe(true, false, Some(w), 1), WINDOW).0);
        assert!(!judge(visible, None, WINDOW).0);
    }

    #[test]
    fn every_step_has_a_name_and_the_last_one_closes_the_window() {
        let steps = steps();
        assert_eq!(steps.len(), 11);
        assert!(steps.iter().all(|s| !s.name.is_empty()));
        assert!(matches!(steps.last().unwrap().actions.as_slice(), [Action::Close]));
    }
}
