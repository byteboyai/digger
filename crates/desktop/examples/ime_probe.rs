//! 手工 IME 探针入口；运行方式见 docs/superpowers/notes/2026-10-18-ime-probe.md。
#[path = "../src/adapters/agent_ime.rs"]
mod agent_ime;
use iced::Event;
use iced_core::input_method;

#[derive(Default)]
struct State {
    enabled: bool,
    committed: String,
    preedit: Option<String>,
    last_event: String,
}
#[derive(Clone)]
enum Msg {
    Focus,
    Blur,
    Commit(String),
    Preedit(Option<String>),
    ImeEvent(String),
}
fn view(s: &State) -> iced::Element<'_, Msg> {
    // 深色矩形代表终端格;点它获得焦点(启用 IME),点别处取消。
    let grid = iced::widget::button("终端格（点击启用 IME）")
        .width(360)
        .height(80)
        .on_press(Msg::Focus);
    let sink = agent_ime::ImeSink::new(grid.into(), s.enabled, |r| r, Msg::Commit, Msg::Preedit);
    let content = iced::widget::column![
        sink,
        iced::widget::text(format!("已提交文本：{}", s.committed)),
        iced::widget::text(format!(
            "当前预编辑：{}",
            s.preedit.as_deref().unwrap_or("")
        )),
        iced::widget::text(format!("IME 启用：{}", s.enabled)),
        iced::widget::text(format!("最近的 Opened/Closed：{}", s.last_event)),
    ]
    .padding(20)
    .spacing(12);
    iced::widget::mouse_area(
        iced::widget::container(content)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill),
    )
    .on_press(Msg::Blur)
    .into()
}
fn update(s: &mut State, m: Msg) -> iced::Task<Msg> {
    match m {
        Msg::Focus => s.enabled = true,
        Msg::Blur => {
            s.enabled = false;
            s.preedit = None;
        }
        Msg::Commit(v) => s.committed.push_str(&v),
        Msg::Preedit(v) => s.preedit = v,
        Msg::ImeEvent(v) => s.last_event = v,
    };
    iced::Task::none()
}
fn subscription(_: &State) -> iced::Subscription<Msg> {
    iced::event::listen_with(|event, _, _| match event {
        Event::InputMethod(input_method::Event::Opened) => Some(Msg::ImeEvent("Opened".into())),
        Event::InputMethod(input_method::Event::Closed) => Some(Msg::ImeEvent("Closed".into())),
        _ => None,
    })
}
fn main() -> iced::Result {
    iced::application::<State, Msg, iced::Theme, iced::Renderer>(State::default, update, view)
        .subscription(subscription)
        .run()
}
