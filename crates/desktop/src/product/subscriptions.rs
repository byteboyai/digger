//! iced subscriptions: the pure construction of the window / panel / service streams.
//!
//! Moved out of the composition root so the boot wiring and the state owner stay readable; behavior is
//! unchanged (same identities, same filters, same routing).

use super::*;

pub fn subscription(shell: &ProductShell) -> Subscription<Msg> {
    let events = iced::event::listen_with(|evt, _status, id| {
        if let iced::Event::Window(window::Event::CloseRequested) = evt {
            return Some(Msg::CloseRequested(id));
        }
        if crate::is_secondary_window(id) {
            return None;
        }
        bytehost_chrome::map_event(&evt).map(Msg::Shell)
    });
    if shell.closing {
        return events;
    }
    // 终端输入:键盘只在未被控件消费时投;鼠标按下带最近一次光标位置;
    // 拖入文件走窗口事件。`listen_with` 只收函数指针,光标与浮层状态由
    // `view` 每帧写进进程级原子,供订阅工作线程读取。
    let agent_input = iced::event::listen_with(agent_event);
    // 左键按下 / 松开与修饰键:文件树的单击 / 拖拽 / 多选靠它们(库对这些消息的 `update` 不处理)。
    let pointer = iced::event::listen_with(|evt, _status, id| {
        if crate::is_secondary_window(id) {
            return None;
        }
        match evt {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                Some(Msg::LeftButton(true))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Msg::LeftButton(false))
            }
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Msg::Modifiers(modifiers))
            }
            _ => None,
        }
    });
    let mut subs = vec![
        events,
        pointer,
        agent_input,
        iced::event::listen_with(window_state_event),
        iced::time::every(std::time::Duration::from_secs(1)).map(|_| Msg::Tick),
    ];
    // 页签拖拽中:在页签行外松手也要结束拖拽(行内松手由页签行自己的 `on_release` 管),
    // 否则之后光标滑过页签行会接着重排并落盘。只在拖拽中订阅。
    if shell.projects.is_dragging() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left
                ))
            )
            .then_some(Msg::Projects(crate::projects::Message::DragEnd))
        }));
    }
    // 待办面板可见时每秒轮询一次(收起整个面板即停,省电)。
    if todo_poll_active(shell) {
        subs.push(iced::time::every(crate::adapters::todo::POLL_INTERVAL).map(|_| Msg::TodoPoll));
    }
    if group_chat_poll_active(shell) {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Msg::GroupChatTick),
        );
    }
    // 名称编辑框的真焦点态只有渲染时(Operation)读得到:用一个 16ms 的订阅在帧与帧之间
    // 把它取回来(同 Dozer 的 `CaptureNameEditFocus`,这里借订阅当逐帧钩子)。
    if let Some(project) = shell.projects.current_project().map(|p| p.id)
        && shell.panel_visible(&shell::project_id())
    {
        subs.push(iced::Subscription::run_with(
            NameFocusProbe(project),
            |probe| {
                let project = probe.0;
                iced::stream::channel(
                    1,
                    move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                        use iced::futures::SinkExt;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                            if out.send(Msg::ProjectFocusFrame(project)).await.is_err() {
                                break;
                            }
                        }
                    },
                )
            },
        ));
    }
    if let Some(pid) = shell.conversation_search_project() {
        subs.push(Subscription::run_with(
            ConversationSearchFocusProbe(pid),
            |probe| {
                let pid = probe.0;
                iced::stream::channel(
                    1,
                    move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                        use iced::futures::SinkExt;
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                            if out
                                .send(Msg::ConversationSearchFocusFrame(pid))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    },
                )
            },
        ));
    }
    if let Some(pid) = shell.browser_project() {
        subs.push(Subscription::run_with(BrowserFocusProbe(pid), |probe| {
            let pid = probe.0;
            iced::stream::channel(
                1,
                move |mut out: iced::futures::channel::mpsc::Sender<Msg>| async move {
                    use iced::futures::SinkExt;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                        if out.send(Msg::BrowserFocusFrame(pid)).await.is_err() {
                            break;
                        }
                    }
                },
            )
        }));
    }
    #[cfg(any(not(target_os = "macos"), test))]
    if let Some(pid) = shell.conversation_search_project()
        && shell
            .conversations
            .state(pid)
            .is_some_and(|state| state.agent_picker_open())
    {
        subs.push(
            iced::event::listen_with(|event, _, _| {
                matches!(
                    event,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(())
            })
            .with(pid)
            .map(|(pid, ())| {
                Msg::Conversations(pid, dozer_panel_conversations::Message::AgentFilterClose)
            }),
        );
    }
    // 新建群弹窗:Esc 取消(背景点击不关闭,保留草稿)。
    if let Some(pid) = shell.projects.current_project().map(|p| p.id)
        && shell
            .group_chat
            .state(pid)
            .is_some_and(|state| state.new_group_dialog_open())
    {
        subs.push(
            iced::event::listen_with(|evt, _status, _id| {
                matches!(
                    evt,
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    })
                )
                .then_some(())
            })
            .with(pid)
            .map(|(pid, ())| {
                Msg::GroupChat(dozer_panel_group_chat::Message::NewGroupDialogCancel(pid))
            }),
        );
    }

    // 右键菜单打开时订阅 Esc:关掉浮层(点空白走浮层自己的透明按下区)。
    if shell.files.has_modal() || shell.files.menu().is_some() {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
            .then_some(Msg::FilesMenuDismiss)
        }));
    }
    // Agent 覆盖层(选择器 / 溢出下拉)或关页签确认打开时 Esc 关最上层。
    let agent_confirm = shell
        .projects
        .current_project()
        .and_then(|p| shell.agent.states.get(&p.id))
        .is_some_and(|state| state.pending_close_tab.is_some());
    if shell.agent.overlay_is_open() || agent_confirm {
        subs.push(iced::event::listen_with(|evt, _status, _id| {
            matches!(
                evt,
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            )
            .then_some(Msg::AgentEscape)
        }));
    }
    if shell.shell.animating() {
        subs.push(
            iced::time::every(std::time::Duration::from_millis(16))
                .map(|_| Msg::Shell(ShellMsg::Tick)),
        );
    }
    Subscription::batch(subs)
}
