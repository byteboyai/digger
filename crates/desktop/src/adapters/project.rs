//! Project 面板的 **Digger 宿主适配层**(S5a Task 7–11)。
//!
//! 面板本体(状态 / 更新 / 视图)是共享库 `dozer-panel-project`,Dozer 与 Digger 共用唯一实现。
//! 本文件是 Digger 侧的产品适配:
//!
//! - **展示语义**:[`PanelDisplay<Msg>`] 实现(面板头、头部内边距、分割线、locale)。
//! - **数据**:`ProjectService` / `MemoryService` 由 [`digger_store::ProjectHost`] 提供(同一个
//!   Digger 数据根里的 SQLite 文件);Digger 没有 agent 转录,[`Services::transcript`] 用一个
//!   恒 `Unavailable` 的占位——按 spec §11,打开项目不做静默补录,"修复项目"也只跑四个同步步骤。
//! - **宿主动作**:库把"要宿主才做得到的事"交给产品(`OpenLink` / `Pick` / `LinkContextMenu` /
//!   `TextInputMenuOpen` / `DeleteProjectConfirm`),由 [`crate::product`] 的 `Msg::Project` 分支
//!   转成产品自己的动作。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dozer_model::ProjectInfo;
use dozer_panel_project::adapter::PanelDisplay;
use dozer_panel_project::{Message as ProjectMessage, WorkspaceState};
use dozer_services::{BoxFuture, ServiceError, ServiceErrorKind, Services, TranscriptService};
use iced::{Element, Length};
use iced_widget::core::{Border, Padding};

use crate::product::Msg;

/// Project 面板的宿主状态:库的 [`WorkspaceState`] 只对应当前聚焦的项目,所以这里按项目 id
/// 记账——每个项目一份面板状态(描述 / 链接 / 记忆、编辑态、弹窗),切项目不串。
pub struct ProjectPanel {
    /// 最近一次切入的项目 id:库消息不带 id,产品分支按它找到项目。
    focus: Option<i64>,
    states: std::collections::HashMap<i64, WorkspaceState>,
    services: Services,
    handle: Option<tokio::runtime::Handle>,
    tx: tokio::sync::mpsc::UnboundedSender<ProjectMessage>,
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<ProjectMessage>>,
}

/// 转录占位:Digger 没有 agent 转录,任何调用都是 `Unavailable`(面板据此不弹"agent 历史"
/// 步骤,也不做补录——见库 `spawn_scaffold_run` / `spawn_repair_run` 对 `transcript` 的处理)。
struct NoTranscript;

fn no_transcript() -> ServiceError {
    ServiceError::new(ServiceErrorKind::Unavailable, "Digger 没有 agent 转录")
}

impl TranscriptService for NoTranscript {
    fn delete_project_transcripts(&self, _cwd: String) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { Err(no_transcript()) })
    }

    fn backfill_project_transcripts(
        &self,
        _cwd: String,
    ) -> BoxFuture<'_, Result<u32, ServiceError>> {
        Box::pin(async { Err(no_transcript()) })
    }

    fn submit_summary_batch(
        &self,
        _cwd: String,
        _provider: Option<dozer_model::AgentKind>,
        _model: Option<String>,
    ) -> BoxFuture<'_, Result<(i64, u32), ServiceError>> {
        Box::pin(async { Err(no_transcript()) })
    }

    fn get_summary_batch(
        &self,
        _batch_id: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::SummaryBatchInfo>, ServiceError>> {
        Box::pin(async { Err(no_transcript()) })
    }
}

/// 其余能力(Digger 不用的面板)一律占位:Project 面板的 `update` 只碰
/// `project` / `transcript` / `memory`,其余字段不会被调用;占位保证误用时是
/// `Unavailable` 而不是 panic。
pub(crate) struct UnavailableService {
    reason: String,
}

impl UnavailableService {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn error(&self) -> ServiceError {
        ServiceError::new(ServiceErrorKind::Unavailable, self.reason.clone())
    }
}

impl dozer_services::UsageService for UnavailableService {
    fn usage_summary(
        &self,
        _cwd: String,
        _since_ts: Option<u64>,
    ) -> BoxFuture<'_, Result<dozer_services::UsageRows, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::TodoService for UnavailableService {
    fn list_todos(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::TodoInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn list_categories(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::CategoryInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn add_todo(&self, _project_id: i64, _text: String) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn toggle_todo(&self, _id: i64, _done: bool) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn edit_todo_text(&self, _id: i64, _text: String) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn reorder_todo(
        &self,
        _id: i64,
        _after_id: Option<i64>,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn set_todo_plan_date(
        &self,
        _id: i64,
        _plan_date: Option<String>,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn set_todo_status(
        &self,
        _id: i64,
        _status: dozer_model::TodoStoredStatus,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn set_todo_category(
        &self,
        _id: i64,
        _category_id: Option<i64>,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn add_category(
        &self,
        _project_id: i64,
        _parent_id: Option<i64>,
        _name: String,
    ) -> BoxFuture<'_, Result<dozer_model::CategoryInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn rename_category(&self, _id: i64, _name: String) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn delete_category(&self, _id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn reparent_category(
        &self,
        _id: i64,
        _new_parent_id: Option<i64>,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn move_category_sibling(
        &self,
        _id: i64,
        _direction: dozer_model::CategoryMoveDirection,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::ProjectService for UnavailableService {
    fn rename_project(
        &self,
        _id: i64,
        _name: String,
    ) -> BoxFuture<'_, Result<Option<dozer_model::ProjectInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn remove_project(&self, _id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn open_project(
        &self,
        _path: String,
    ) -> BoxFuture<'_, Result<Option<dozer_model::ProjectInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn list_projects(&self) -> BoxFuture<'_, Result<Vec<dozer_model::ProjectInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::MemoryService for UnavailableService {
    fn list_memories(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::MemoryInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn get_memory(
        &self,
        _project_id: i64,
        _id: i64,
    ) -> BoxFuture<'_, Result<dozer_model::MemoryDetail, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn write_memory(
        &self,
        _project_id: i64,
        _title: String,
        _kind: String,
        _description: String,
        _body: String,
        _actor: String,
    ) -> BoxFuture<'_, Result<dozer_model::MemoryDetail, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn delete_memory(
        &self,
        _project_id: i64,
        _id: i64,
        _actor: String,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::GroupChatService for UnavailableService {
    fn list_groups(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::GroupInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn list_group_messages(
        &self,
        _group_id: i64,
        _after_rev: i64,
        _limit: u32,
    ) -> BoxFuture<'_, Result<(Vec<dozer_model::GroupMessageInfo>, i64), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn create_group(
        &self,
        _project_id: i64,
        _topic: String,
    ) -> BoxFuture<'_, Result<dozer_model::GroupInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn delete_group(&self, _group_id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn add_group_member(
        &self,
        _group_id: i64,
        _agent: dozer_model::AgentKind,
        _handle: String,
        _role_prompt: String,
    ) -> BoxFuture<'_, Result<dozer_model::GroupInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn update_group_member(
        &self,
        _member_id: i64,
        _handle: String,
        _role_prompt: String,
    ) -> BoxFuture<'_, Result<dozer_model::GroupInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn remove_group_member(
        &self,
        _member_id: i64,
    ) -> BoxFuture<'_, Result<dozer_model::GroupInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn post_group_message(
        &self,
        _group_id: i64,
        _text: String,
    ) -> BoxFuture<'_, Result<dozer_services::PostedMessages, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn cancel_group(
        &self,
        _group_id: i64,
        _scope: dozer_model::GroupCancelScope,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn retry_group_message(
        &self,
        _message_id: i64,
    ) -> BoxFuture<'_, Result<dozer_model::GroupMessageInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn push_group_message_to_todo(
        &self,
        _message_id: i64,
        _text: String,
    ) -> BoxFuture<'_, Result<dozer_model::TodoInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::ConversationService for UnavailableService {
    fn list_conversations_with_summaries(
        &self,
        _cwd: String,
        _agent: Option<dozer_model::AgentKind>,
        _limit: u32,
        _offset: u32,
    ) -> BoxFuture<'_, Result<dozer_services::ConversationRows, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::CodeHealthService for UnavailableService {
    fn get_code_health_report(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Option<dozer_model::CodeHealthReportInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn list_code_health_reports(
        &self,
        _project_id: i64,
        _limit: u32,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::CodeHealthReportInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn save_code_health_report(
        &self,
        _project_id: i64,
        _info: dozer_model::CodeHealthReportInfo,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::ContextService for UnavailableService {
    fn list_context_items(
        &self,
        _project_id: i64,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::ContextItemInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn add_context_item(
        &self,
        _project_id: i64,
        _entity_kind: String,
        _entity_ref: String,
    ) -> BoxFuture<'_, Result<dozer_model::ContextItemInfo, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn remove_context_item(
        &self,
        _project_id: i64,
        _id: i64,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::EditHistoryService for UnavailableService {
    fn list_file_edit_history(
        &self,
        _project_id: i64,
        _path_filter: Option<String>,
        _limit: u32,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::FileEditHistoryInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn apply_precise_edit(
        &self,
        _edit: dozer_services::PreciseEdit,
    ) -> BoxFuture<'_, Result<dozer_model::MutationOutcome, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::BookmarkService for UnavailableService {
    fn list_bookmarks(
        &self,
        _project_id: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<dozer_model::BookmarkInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn add_bookmark(
        &self,
        _scope: dozer_model::BookmarkScope,
        _project_id: Option<i64>,
        _url: String,
        _title: String,
    ) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn remove_bookmark(&self, _id: i64) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::LocaleService for UnavailableService {
    fn get_locale(&self) -> BoxFuture<'_, Result<dozer_model::LocaleSnapshot, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn set_locale(
        &self,
        _preference: dozer_model::LocalePreference,
        _expected_revision: u64,
        _system_candidates: Vec<String>,
    ) -> BoxFuture<'_, Result<dozer_model::LocaleWriteOutcome, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl dozer_services::DaemonService for UnavailableService {
    fn list_sessions(&self) -> BoxFuture<'_, Result<Vec<dozer_model::SessionInfo>, ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn shutdown_daemon(&self) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
    fn ensure_running(&self) -> BoxFuture<'_, Result<(), ServiceError>> {
        Box::pin(async { Err(self.error()) })
    }
}

impl ProjectPanel {
    /// 构造:注入服务束 + 结果回流通道。运行时句柄由 [`ProductShell::boot`](crate::product::boot)
    /// 在进入 iced 运行时后经 [`ProjectPanel::attach_handle`] 补挂。
    ///
    /// `project = None`(项目数据库打不开)时面板仍可构造:项目/记忆两个能力恒 `Unavailable`,
    /// 注册表自己会显示"项目功能不可用",面板画空盒。
    pub fn new(project: Option<&digger_store::ProjectHost>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let missing = "项目数据库不可用";
        let services = Services {
            usage: Arc::new(UnavailableService::new("Project 面板不用用量服务")),
            todo: Arc::new(UnavailableService::new("Project 面板不用待办服务")),
            project: match project {
                Some(p) => p.projects.clone(),
                None => Arc::new(UnavailableService::new(missing)),
            },
            transcript: Arc::new(NoTranscript),
            memory: match project {
                Some(p) => p.memory.clone(),
                None => Arc::new(UnavailableService::new(missing)),
            },
            group_chat: Arc::new(UnavailableService::new("Project 面板不用群聊服务")),
            conversation: Arc::new(UnavailableService::new("Project 面板不用会话服务")),
            codehealth: Arc::new(UnavailableService::new("Project 面板不用健康服务")),
            context: Arc::new(UnavailableService::new("Project 面板不用上下文服务")),
            edit_history: Arc::new(UnavailableService::new("Project 面板不用编辑历史服务")),
            bookmark: Arc::new(UnavailableService::new("Project 面板不用书签服务")),
            locale: Arc::new(UnavailableService::new("Project 面板不用语言服务")),
            daemon: Arc::new(UnavailableService::new("Project 面板不用 daemon 服务")),
        };
        Self {
            focus: None,
            states: std::collections::HashMap::new(),
            services,
            handle: None,
            tx,
            rx: Some(rx),
        }
    }

    /// 挂上 iced 运行时句柄(仅可在运行时上下文内调用)。
    pub fn attach_handle(&mut self, handle: tokio::runtime::Handle) {
        self.handle = Some(handle);
    }

    /// 取走后台结果接收端(组合根在 `boot` 里接成 iced 流;只该取一次)。
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<ProjectMessage>> {
        self.rx.take()
    }

    /// 项目的面板状态;第一次访问时从磁盘加载描述与链接(与 Dozer 打开项目时同两处读取)。
    fn state_of(&mut self, project: &ProjectInfo) -> &mut WorkspaceState {
        let path = PathBuf::from(&project.path);
        self.states.entry(project.id).or_insert_with(|| {
            WorkspaceState::new(
                dozer_panel_project::meta::load_description(&path),
                dozer_panel_project::links::load_or_discover(&path),
            )
        })
    }

    /// 项目切入(面板变为可见 / 切到另一个项目页签):
    ///
    /// - 记住这次切入的项目(产品消息不带 id,按它路由);
    /// - 写一份缺失的 `README.md`(Dozer 的 `ensure_project_readme` 同一规则);
    /// - 拉一次共享记忆列表(库的 `on_activate`);
    /// - 后台刷新 git 快照与磁盘占用。
    pub fn activate(&mut self, project: &ProjectInfo) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        self.focus = Some(project.id);
        let path = PathBuf::from(&project.path);
        self.ensure_readme(project);
        self.refresh_git(project);
        self.refresh_disk_usage(project);
        let tx = self.tx.clone();
        let ctx = dozer_panel_host::ActivationCtx {
            project_id: project.id,
            project_path: Some(path),
            io: dozer_panel_host::PanelIo::new(self.services.clone(), handle, move |message| {
                // 通道关闭 = 组合根已退出;静默丢弃。
                let _ = tx.send(message);
            }),
        };
        let state = self.state_of(project);
        dozer_panel_project::on_activate(state, &ctx);
    }

    /// 当前聚焦的项目 id(最近一次 `activate` 的那个)。
    pub fn focused_project(&self) -> Option<i64> {
        self.focus
    }

    /// 视图/消息不知道自己是哪个项目的:`AsyncResult` 带着 token 里的项目 id,
    /// 其余库消息一律按当前聚焦项目路由。
    pub fn resolve_project(&self, message: &ProjectMessage) -> Option<i64> {
        message.result_project().or(self.focus)
    }

    /// 与 Dozer 的 `ensure_project_readme` 同一条规则:项目里没有 `README.md` 就写一份
    /// (标题 + 已保存的项目描述)。Digger 一期没有预览栏(S5b),所以不 reveal。
    fn ensure_readme(&self, project: &ProjectInfo) {
        let repo = Path::new(&project.path);
        let readme = repo.join("README.md");
        if readme.exists() {
            return;
        }
        let mut content = format!("# {}\n\n", project.name);
        if let Some(desc) = dozer_panel_project::meta::load_description(repo) {
            content.push_str(&desc);
            content.push('\n');
        }
        if let Err(e) = std::fs::write(&readme, content) {
            eprintln!("digger: 项目 README 未创建: {e}"); // cli-output
        }
    }

    /// 项目里的 git 快照(分支 / 脏标 / remote),后台跑完投回 `GitRefreshed`。
    pub fn refresh_git(&self, project: &ProjectInfo) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let project_id = project.id;
        let repo_path = PathBuf::from(&project.path);
        let tx = self.tx.clone();
        handle.spawn(async move {
            let snapshot = tokio::task::spawn_blocking({
                let repo_path = repo_path.clone();
                move || git_snapshot(&repo_path)
            })
            .await
            .unwrap_or((None, false, Vec::new()));
            let _ = tx.send(ProjectMessage::GitRefreshed(
                project_id, snapshot.0, snapshot.1, snapshot.2,
            ));
        });
    }

    /// 项目的磁盘占用,后台跑完投回 `DiskUsageLoaded`。
    pub fn refresh_disk_usage(&self, project: &ProjectInfo) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let project_id = project.id;
        let repo_path = PathBuf::from(&project.path);
        let tx = self.tx.clone();
        handle.spawn(async move {
            let bytes = tokio::task::spawn_blocking(move || {
                dozer_panel_project::disk::dir_size_excluding(
                    &repo_path,
                    &dozer_panel_project::disk::DISK_USAGE_EXCLUDE,
                )
            })
            .await
            .unwrap_or(0);
            let _ = tx.send(ProjectMessage::DiskUsageLoaded(project_id, bytes));
        });
    }

    /// 处理一条库消息。`project` 是消息所属的项目(注册表里可能已经没有它——被删了——此时丢弃)。
    ///
    /// 库有几种消息由宿主**拦截**后自己落地(`update` 里是 `unreachable!`),这些分支不在这里调库:
    /// `ToolbarHover` / `TextInputMenuOpen` / `OpenLink` / `Pick` / `LinkContextMenu` /
    /// `DeleteProjectConfirm`。悬停动画 Digger 不做,直接丢掉。
    pub fn update(&mut self, project: Option<&ProjectInfo>, message: ProjectMessage) {
        let Some(project) = project else {
            return;
        };
        if intercept(&message) == Intercept::Host {
            // 拦截消息(删除确认/右键菜单)也要落在状态上,所以先建出来。
            let _ = self.state_of(project);
            return;
        }
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let current_name = project.name.clone();
        let repo_path = PathBuf::from(&project.path);
        let services = self.services.clone();
        let tx = self.tx.clone();
        let emit = move |m| {
            let _ = tx.send(m);
        };
        let state = self.state_of(project);
        dozer_panel_project::update(
            state,
            message,
            project.id,
            &current_name,
            &repo_path,
            &services,
            &handle,
            emit,
        );
    }

    /// 名称编辑框失焦:库的 `submit_name_edit` 走一次改名(空名/未改名在库内是 no-op)。
    pub fn submit_name_edit(&mut self, project: &ProjectInfo) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let Some(state) = self.states.get_mut(&project.id) else {
            return;
        };
        let project_id = project.id;
        let current_name = project.name.clone();
        let service = self.services.project.clone();
        let tx = self.tx.clone();
        let emit = move |m| {
            let _ = tx.send(m);
        };
        dozer_panel_project::submit_name_edit(
            state,
            project_id,
            &current_name,
            service,
            &handle,
            emit,
        );
    }

    /// 名称/描述编辑框是否正在编辑。用于"编辑中点了面板别处就提交"的失焦兜底。
    pub fn is_editing(&self, project_id: i64) -> bool {
        self.states
            .get(&project_id)
            .is_some_and(|s| s.name_editing.is_some() || s.description_editing.is_some())
    }

    /// 两个编辑框同时失焦:名称改名、描述写盘。与 Dozer 的 `blur_inputs` 同序。
    pub fn blur_editors(&mut self, project: &ProjectInfo) {
        let Some(state) = self.states.get_mut(&project.id) else {
            return;
        };
        state.submit_description_edit_on_blur(Path::new(&project.path));
        self.submit_name_edit(project);
    }

    /// 排空待发提示。Digger 一期没有 Toast:调用方把文本落到 stderr,不静默吞掉。
    pub fn take_notices(&mut self, project_id: i64) -> Vec<(String, String)> {
        let Some(state) = self.states.get_mut(&project_id) else {
            return Vec::new();
        };
        state
            .take_outbox()
            .into_iter()
            .map(|n| (format!("{:?}", n.level), n.text.fallback))
            .collect()
    }

    /// 面板视图(`width` / `outer` 由骨架的区域几何给出,与 Dozer 的 `project_pane` 同义)。
    pub fn view<'a>(
        &'a self,
        project: Option<&'a ProjectInfo>,
        width: Length,
        outer: Border,
    ) -> Element<'a, Msg> {
        let state = project.and_then(|p| self.states.get(&p.id));
        match state {
            Some(state) => dozer_panel_project::view::view(
                self,
                state,
                project,
                width,
                outer,
                // Digger 不挂 hover 动画表:三颗"＋"按钮按"未悬停"画。
                dozer_panel_project::view::ProjectPaneHover {
                    docs_add: 0.0,
                    remote_add: 0.0,
                    memory_add: 0.0,
                },
                "en",
            ),
            // 还没有状态(尚未切入):给一个空盒子,切入后会立刻建出来。
            None => iced::widget::container(iced::widget::Space::new())
                .width(width)
                .height(Length::Fill)
                .into(),
        }
    }

    /// 「删除项目」三选一卡片(库的 `project_delete_card`);没有待确认的删除时为 `None`。
    pub fn delete_card<'a>(&'a self, project_id: i64) -> Option<Element<'a, Msg>> {
        let state = self.states.get(&project_id)?;
        state.delete_pending?;
        Some(dozer_panel_project::view::project_delete_card(state, "en").map(Msg::Project))
    }

    /// 取走"待确认的删除层级"(库的 `delete_pending`)。产品点确认卡片时先取再关页签。
    pub fn take_delete_pending(
        &mut self,
        project_id: i64,
    ) -> Option<dozer_panel_project::delete::DeleteScope> {
        self.states.get_mut(&project_id)?.delete_pending.take()
    }

    /// 跑一次完整删除(库的 `delete::spawn_delete_project`):先 delist,再按层级删文件系统。
    /// `transcript` 传 `None`(Digger 没有转录):`WithAgentCache` 只删 agent 缓存目录。
    pub fn delete_project(
        &self,
        project: ProjectInfo,
        scope: dozer_panel_project::delete::DeleteScope,
        on_done: impl Fn(i64, bool, Vec<String>) + Send + 'static,
    ) {
        let Some(handle) = self.handle.clone() else {
            on_done(project.id, false, vec!["内部错误:未挂运行时句柄".into()]);
            return;
        };
        dozer_panel_project::delete::spawn_delete_project(
            project.id,
            PathBuf::from(&project.path),
            scope,
            self.services.project.clone(),
            None,
            agent_cache_dir,
            &handle,
            on_done,
        );
    }
}

/// 三家 agent 为项目缓存的目录。规则与 Dozer `dozer_core::agent_paths` 一致(库不依赖它,产品给出):
/// Claude 把路径里所有非字母数字字符换成 `-`(开头 `/` 也换);CodeBuddy 先去掉开头 `/` 再换;
/// opencode 只换 `/`。
fn agent_cache_dir(kind: &'static str, repo: &Path) -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let raw = repo.to_string_lossy();
    match kind {
        "claude" => {
            let key: String = raw
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            home.join(".claude").join("projects").join(key)
        }
        "codebuddy" => {
            let key = raw.trim_start_matches('/').replace('/', "-");
            home.join(".codebuddy").join("projects").join(key)
        }
        "opencode" => home
            .join(".dozer")
            .join("agents")
            .join("opencode")
            .join("projects")
            .join(raw.replace('/', "-")),
        _ => PathBuf::new(),
    }
}

/// 项目的 git 快照:`(当前分支, 是否有改动, 去重后的 remote URL 列表)`。不是仓库时 `(None, false, [])`。
fn git_snapshot(repo_path: &Path) -> (Option<String>, bool, Vec<String>) {
    let repo = bytegit::Repo::discover_workdir(repo_path).ok();
    let branch = repo
        .as_ref()
        .and_then(|repo| repo.head().ok())
        .and_then(|head| head.branch);
    let dirty = repo.as_ref().is_some_and(|repo| {
        repo.is_dirty(bytegit::StatusOptions::default())
            .unwrap_or(false)
    });
    let remotes = bytegit::Repo::discover(repo_path)
        .and_then(|repo| repo.remotes())
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();
    for remote in remotes {
        if seen.insert(remote.url.clone()) {
            urls.push(remote.url);
        }
    }
    (branch, dirty, urls)
}

impl PanelDisplay<Msg> for ProjectPanel {
    fn wrap(&self, message: ProjectMessage) -> Msg {
        // 库消息不带项目 id(除了 `AsyncResult` 的回包 token);产品分支按当前聚焦项目路由,
        // 这里给一个占位 0,由 `resolve_project` 在 `product.rs` 换成聚焦项目。
        Msg::Project(message)
    }

    fn panel_head<'a>(
        &self,
        icon: byteui::interaction::icons::IconKind,
        title: String,
    ) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        let tokens = byteui::theme::color::current();
        iced::widget::column![
            iced::widget::row![
                byteui::interaction::icons::view(
                    icon,
                    byteui::theme::icon_size::row(),
                    tokens.tab_active_border,
                ),
                iced::widget::text(title)
                    .size(byteui::theme::font::title())
                    .color(tokens.tab_active_border),
            ]
            .spacing(8)
            .height(Length::Fixed(byteui::theme::geometry::tab_button_size()))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(4)
        .into()
    }

    fn header_padding(&self) -> Padding {
        // 与 Dozer `workspace.json` 的 `project_pane.padding` 一致(8,随主题缩放)。
        let s = byteui::theme::icon_size::scale();
        Padding::new(8.0 * s)
    }

    fn tab_divider<'a>(&self) -> Element<'a, Msg, iced_widget::Theme, iced_renderer::Renderer> {
        byteui::layout::divider::horizontal()
    }
}

/// 把项目面板的后台结果接收端接成一条持续到进程结束的 iced 流。
pub fn receiver_stream(
    rx: tokio::sync::mpsc::UnboundedReceiver<ProjectMessage>,
) -> impl iced::futures::Stream<Item = ProjectMessage> {
    iced::futures::stream::unfold(Some(rx), |maybe| async move {
        let mut rx = maybe?;
        let next = rx.recv().await?;
        Some((next, Some(rx)))
    })
}

/// 产品侧对一条库消息的处置决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intercept {
    /// 交给产品自己实现(选择器、菜单、确认卡……)。
    Host,
    /// 走 `dozer-panel-project::update`。
    Panel,
}

/// 库的 `update` 里 `unreachable!` 的那些分支必须先由产品处理。
pub fn intercept(message: &ProjectMessage) -> Intercept {
    match message {
        // Digger 一期没有预览栏:点文档在系统默认应用里打开(库不碰系统,产品给动作)。
        ProjectMessage::OpenLink(_) => Intercept::Host,
        ProjectMessage::ToolbarHover(..) => Intercept::Host,
        ProjectMessage::TextInputMenuOpen(..) => Intercept::Host,
        ProjectMessage::Pick(..) => Intercept::Host,
        ProjectMessage::LinkContextMenu { .. } => Intercept::Host,
        ProjectMessage::DeleteProjectConfirm => Intercept::Host,
        _ => Intercept::Panel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> ProjectPanel {
        let dir = std::env::temp_dir().join(format!(
            "digger-project-panel-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let host = digger_store::ProjectHost::open(&dir.join("projects.db")).unwrap();
        ProjectPanel::new(Some(&host))
    }

    #[test]
    fn the_placeholder_services_are_unavailable_not_panics() {
        let p = panel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(p.services.usage.usage_summary("/x".into(), None));
        assert_eq!(
            err.unwrap_err().kind,
            dozer_services::ServiceErrorKind::Unavailable
        );
        let err = rt.block_on(
            p.services
                .transcript
                .delete_project_transcripts("/x".into()),
        );
        assert_eq!(
            err.unwrap_err().kind,
            dozer_services::ServiceErrorKind::Unavailable
        );
    }

    #[test]
    fn every_host_intercepted_message_is_recognised() {
        for m in [
            ProjectMessage::Noop,
            ProjectMessage::TextInputMenuOpen(dozer_panel_project::adapter::TextInputTarget {
                id: iced::widget::Id::new("x"),
                secure: false,
            }),
            ProjectMessage::OpenLink(PathBuf::from("/x")),
            ProjectMessage::Pick(dozer_panel_project::links::LinkTarget::Docs),
            ProjectMessage::LinkContextMenu {
                target: dozer_panel_project::links::LinkTarget::Docs,
                index: 0,
            },
            ProjectMessage::DeleteProjectConfirm,
            ProjectMessage::ToolbarHover(dozer_panel_project::ProjectToolbarTarget::Docs, true),
        ] {
            let expected = if matches!(m, ProjectMessage::Noop) {
                Intercept::Panel
            } else {
                Intercept::Host
            };
            assert_eq!(intercept(&m), expected, "{m:?}");
        }
    }

    #[test]
    fn blurring_the_name_editor_submits_the_pending_rename() {
        let mut p = panel();
        let dir = std::env::temp_dir().join(format!(
            "digger-project-rename-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        p.attach_handle(rt.handle().clone());
        let project = rt
            .block_on(
                p.services
                    .project
                    .open_project(dir.to_string_lossy().into_owned()),
            )
            .unwrap()
            .unwrap();
        assert!(!p.is_editing(project.id));
        p.state_of(&project).name_editing = Some("renamed".into());
        assert!(p.is_editing(project.id));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        p.tx = tx;
        p.blur_editors(&project);
        let event = rt.block_on(async { rx.recv().await });
        let Some(ProjectMessage::NameRenamed(id, Ok(updated))) = event else {
            panic!("expected a rename, got {event:?}");
        };
        assert_eq!(id, project.id);
        assert_eq!(updated.name, "renamed");
        assert!(!p.is_editing(project.id));
    }

    #[test]
    fn the_agent_cache_dirs_match_dozers_naming_rules() {
        let repo = Path::new("/Users/me/Anrong/anrong_finagent");
        let claude = agent_cache_dir("claude", repo);
        assert!(
            claude.ends_with("-Users-me-Anrong-anrong-finagent"),
            "{claude:?}"
        );
        let codebuddy = agent_cache_dir("codebuddy", repo);
        assert!(
            codebuddy.ends_with("Users-me-Anrong-anrong_finagent"),
            "{codebuddy:?}"
        );
        let opencode = agent_cache_dir("opencode", repo);
        assert!(
            opencode.ends_with("projects/-Users-me-Anrong-anrong_finagent"),
            "{opencode:?}"
        );
    }
}
