//! S8b group-chat list gallery. Synthetic state only: two groups, one Running
//! reply, no daemon and no webview. `--check` validates the fixture headlessly.
use digger_desktop::adapters::group_chat_display::GroupChatDisplay;
use digger_desktop::fonts;
use dozer_model::{
    AgentKind, GroupAuthor, GroupInfo, GroupMemberInfo, GroupMessageInfo, GroupMessageStatus,
};
use dozer_panel_group_chat::view::list_pane;
use dozer_panel_group_chat::{Message, WorkspaceState, update};
use iced::widget::{column, container, text};
use iced::{Element, Length, Task};
use iced_widget::core::Border;

const PID: i64 = 7;

struct Gallery {
    state: WorkspaceState,
    display: GroupChatDisplay,
}

fn member(id: i64, group_id: i64, agent: AgentKind, handle: &str) -> GroupMemberInfo {
    GroupMemberInfo {
        id,
        group_id,
        agent,
        handle: handle.into(),
        role_prompt: String::new(),
    }
}

fn fixture() -> Gallery {
    let groups = vec![
        GroupInfo {
            id: 1,
            project_id: PID,
            topic: "S8b 列表与新建群".into(),
            created_ms: 0,
            members: vec![
                member(1, 1, AgentKind::Claude, "reviewer"),
                member(2, 1, AgentKind::Codex, "builder"),
            ],
        },
        GroupInfo {
            id: 2,
            project_id: PID,
            topic: "发布前回归".into(),
            created_ms: 0,
            members: vec![member(3, 2, AgentKind::Claude, "qa")],
        },
    ];
    let mut state = WorkspaceState::default();
    let _ = update(&mut state, Message::GroupsLoaded(PID, Ok(groups)));
    let message = GroupMessageInfo {
        id: 10,
        group_id: 1,
        seq: 1,
        rev: 1,
        author: GroupAuthor::Member { member_id: 1 },
        text: "正在检查列表接线。".into(),
        mentions: vec![],
        status: Some(GroupMessageStatus::Running),
        duration_ms: None,
        created_ms: 0,
        todo_id: None,
    };
    let _ = update(
        &mut state,
        Message::Polled {
            project_id: PID,
            group_id: 1,
            result: Ok((vec![message], 1)),
        },
    );
    Gallery {
        state,
        display: GroupChatDisplay::new(PID),
    }
}

fn app_update(gallery: &mut Gallery, message: Message) -> Task<Message> {
    let _ = update(&mut gallery.state, message);
    Task::none()
}

fn app_view(gallery: &Gallery) -> Element<'_, Message> {
    let list = list_pane(
        &gallery.state,
        &gallery.display,
        PID,
        Length::Fixed(320.0),
        Border::default(),
    );
    let selected = gallery
        .state
        .selected()
        .and_then(|id| gallery.state.groups().iter().find(|g| g.id == id));
    let detail = match selected {
        Some(group) => column![
            text(group.topic.clone()).size(20),
            text(format!("{} 位成员", group.members.len())),
            text("聊天内容即将可用"),
        ]
        .spacing(8),
        None => column![text("选择一个群聊,或点击＋新建")].spacing(8),
    };
    iced::widget::row![
        list,
        container(detail)
            .padding(20)
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .height(Length::Fill)
    .into()
}

fn main() -> iced::Result {
    byteui::theme::color::set_theme(byteui::theme::color::ColorTokens::byteboy2077());
    byteui::theme::font::set_theme(byteui::theme::font::FontTokens::byteboy2077());
    if std::env::args().any(|a| a == "--check") {
        let gallery = fixture();
        assert_eq!(gallery.state.groups().len(), 2);
        assert_eq!(gallery.state.selected(), Some(1));
        assert!(
            gallery
                .state
                .messages()
                .iter()
                .any(|m| { matches!(m.status, Some(GroupMessageStatus::Running)) })
        );
        let _ = app_view(&gallery);
        println!("group_chat_gallery check: 2 groups, selected=1, one Running reply");
        return Ok(());
    }
    iced::application(|| (fixture(), Task::none()), app_update, app_view)
        .font(fonts::JETBRAINS_MONO)
        .font(dozer_panel_term::symbols::SYMBOLS_FONT_DATA)
        .title(|_: &Gallery| "Group chat gallery · S8b".to_string())
        .window_size((980.0, 620.0))
        .run()
}
