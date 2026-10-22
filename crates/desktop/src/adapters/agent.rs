//! S6d-1 的 Agent 面板壳；会话/终端由下一期替换。
use iced::widget::{button, column, text};
use iced::{Element, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentShellMsg {
    Retry,
    // 由 4b 的 `Daemon::ensure` 结果构造;本期只在测试里出现。
    #[allow(dead_code)]
    Ensured(Result<(), String>),
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    // 4b 接 daemon 之前的初始态;本期默认直接 Ready。
    #[allow(dead_code)]
    Unknown,
    Checking,
    #[default]
    Ready,
    Unavailable(String),
}
#[derive(Debug, Default)]
pub struct AgentShell {
    pub status: Status,
}
/// 壳当前该显示哪一种内容(视图据此分派,也让分派可测)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body<'a> {
    NoProject,
    Unavailable(&'a str),
    Checking,
    Placeholder,
}
impl AgentShell {
    pub fn body(&self, has_project: bool) -> Body<'_> {
        match (&self.status, has_project) {
            (_, false) => Body::NoProject,
            (Status::Unavailable(reason), _) => Body::Unavailable(reason),
            (Status::Checking, _) => Body::Checking,
            (Status::Ready | Status::Unknown, _) => Body::Placeholder,
        }
    }
    pub fn view(&self, has_project: bool) -> Element<'_, AgentShellMsg> {
        match self.body(has_project) {
            Body::NoProject => column![text("请先打开一个 Project，再使用 Agent 面板。")]
                .padding(20)
                .into(),
            Body::Unavailable(reason) => {
                column![text(reason), button("重试").on_press(AgentShellMsg::Retry)]
                    .spacing(12)
                    .padding(20)
                    .into()
            }
            Body::Checking => column![text("正在连接 Agent 服务…")].padding(20).into(),
            Body::Placeholder => column![text("Agent 面板即将可用")].padding(20).into(),
        }
    }
    pub fn update(
        &mut self,
        msg: AgentShellMsg,
        ensure: impl FnOnce() -> Task<AgentShellMsg>,
    ) -> Task<AgentShellMsg> {
        match msg {
            AgentShellMsg::Retry if self.status == Status::Checking => Task::none(),
            AgentShellMsg::Retry => {
                self.status = Status::Checking;
                ensure()
            }
            AgentShellMsg::Ensured(Ok(())) => {
                self.status = Status::Ready;
                Task::none()
            }
            AgentShellMsg::Ensured(Err(error)) => {
                self.status = Status::Unavailable(error);
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn retry(shell: &mut AgentShell, calls: &Cell<u32>) {
        let _ = shell.update(AgentShellMsg::Retry, || {
            calls.set(calls.get() + 1);
            Task::none()
        });
    }

    #[test]
    fn unavailable_shows_the_reason_and_a_retry_that_checks_once() {
        let mut shell = AgentShell {
            status: Status::Unavailable("daemon 没起来".into()),
        };
        assert_eq!(shell.body(true), Body::Unavailable("daemon 没起来"));
        let calls = Cell::new(0);
        retry(&mut shell, &calls);
        retry(&mut shell, &calls);
        assert_eq!(calls.get(), 1, "the second Retry while Checking is ignored");
        assert_eq!(shell.body(true), Body::Checking);
    }

    #[test]
    fn a_successful_retry_turns_unavailable_into_ready() {
        let mut shell = AgentShell {
            status: Status::Unavailable("x".into()),
        };
        retry(&mut shell, &Cell::new(0));
        let _ = shell.update(AgentShellMsg::Ensured(Ok(())), Task::none);
        assert_eq!(shell.body(true), Body::Placeholder);
        let _ = shell.update(AgentShellMsg::Ensured(Err("又失败了".into())), Task::none);
        assert_eq!(shell.body(true), Body::Unavailable("又失败了"));
    }

    #[test]
    fn no_project_shows_the_empty_state_whatever_the_status() {
        for status in [
            Status::Unknown,
            Status::Checking,
            Status::Ready,
            Status::Unavailable("x".into()),
        ] {
            assert_eq!(AgentShell { status }.body(false), Body::NoProject);
        }
    }

    #[test]
    fn the_default_shell_is_ready_because_this_phase_has_no_daemon_to_ask() {
        assert_eq!(AgentShell::default().body(true), Body::Placeholder);
    }
}
