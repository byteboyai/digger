// Included in product::tests so the PR usage_route filter runs the full product path.
type UsageReply =
    tokio::sync::oneshot::Sender<Result<dozer_services::UsageRows, dozer_services::ServiceError>>;

struct RoutedUsageService(tokio::sync::mpsc::UnboundedSender<(String, UsageReply)>);

impl dozer_services::UsageService for RoutedUsageService {
    fn usage_summary(
        &self,
        cwd: String,
        _since_ts: Option<u64>,
    ) -> dozer_services::BoxFuture<
        '_,
        Result<dozer_services::UsageRows, dozer_services::ServiceError>,
    > {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.0.send((cwd, tx)).unwrap();
        Box::pin(async move { rx.await.expect("test must reply to every query") })
    }
}

async fn usage_route_recv<T>(rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("usage route did not produce the expected event")
        .expect("usage route channel closed")
}

fn usage_route_rows(title: &str) -> dozer_services::UsageRows {
    vec![(
        dozer_model::ConversationSummary {
            conversation_id: title.into(),
            agent: dozer_model::AgentKind::Claude,
            file_path: format!("/{title}.jsonl"),
            title: title.into(),
            first_ts: 0,
            last_ts: 0,
            turn_count: 1,
        },
        dozer_model::UsagePayload {
            turns: 1,
            tokens_in: 1,
            ..Default::default()
        },
    )]
}

#[tokio::test]
async fn usage_route_project_messages_invalidate_hidden_queries_and_closed_lifecycles() {
    use crate::projects::Message as P;

    let dir = test_temp_dir("usage-route-project-messages");
    let project = |id, name: &str| {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        dozer_model::ProjectInfo {
            id,
            path: path.to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        }
    };
    let a = project(1, "a");
    let b = project(2, "b");
    // B has its own saved Agent layout. New projects may inherit the global
    // layout, so make the hidden-panel precondition explicit on disk.
    let defaults = shell::defaults();
    let b_layout = ShellModel::new(
        defaults.rails,
        defaults.selected,
        defaults.left_width,
        defaults.window,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("shell-projects")).unwrap();
    bytehost_chrome::persist::save_atomic(&project_path_in(&dir, b.id), &b_layout).unwrap();
    let mut shell = ProductShell::with_shell_path(dir.join("shell.json"));
    let (tx, mut queries) = tokio::sync::mpsc::unbounded_channel();
    shell.usage = crate::adapters::usage::UsagePanel::new(Arc::new(RoutedUsageService(tx)));
    shell.usage.attach_handle(tokio::runtime::Handle::current());
    let mut results = shell.usage.take_receiver().unwrap();

    // Enter through the same top-level messages as the running product. No direct
    // set_scope/reset calls: removing sync_usage_scope must make this test fail.
    let dispatch = |shell: &mut ProductShell, message| {
        let _ = update_host(shell, HostMessage::Product(message));
    };
    dispatch(&mut shell, Msg::Projects(P::Opened(Ok(Some(a.clone())))));
    dispatch(
        &mut shell,
        Msg::Shell(ShellMsg::ShowPanel(shell::usage_id())),
    );
    assert!(shell.panel_visible(&shell::usage_id()));
    let (path, old_a) = usage_route_recv(&mut queries).await;
    assert_eq!(path, a.path);
    let a_identity = shell.usage.scope_identity().unwrap();

    // Hide Usage via the actual rail selection, then open B. Its saved layout
    // keeps Usage hidden, but its scope must change immediately nonetheless.
    dispatch(
        &mut shell,
        Msg::Shell(ShellMsg::ShowPanel(shell::agent_id())),
    );
    assert!(!shell.panel_visible(&shell::usage_id()));
    dispatch(&mut shell, Msg::Projects(P::Opened(Ok(Some(b.clone())))));
    assert!(!shell.panel_visible(&shell::usage_id()));
    let b_identity = shell.usage.scope_identity().unwrap();
    assert_eq!(b_identity.0, PathBuf::from(&b.path));
    assert_ne!(a_identity.1, b_identity.1);
    assert!(queries.try_recv().is_err(), "hidden B must not query yet");
    old_a.send(Ok(usage_route_rows("stale-a"))).unwrap();
    dispatch(
        &mut shell,
        Msg::UsageResult(usage_route_recv(&mut results).await),
    );
    assert!(shell.usage.row_titles().is_empty());

    dispatch(
        &mut shell,
        Msg::Shell(ShellMsg::ShowPanel(shell::usage_id())),
    );
    assert!(shell.panel_visible(&shell::usage_id()));
    let (path, current_b) = usage_route_recv(&mut queries).await;
    assert_eq!(path, b.path, "show must query the actual current project");
    current_b.send(Ok(usage_route_rows("current-b"))).unwrap();
    let completed_b = usage_route_recv(&mut results).await;
    dispatch(&mut shell, Msg::UsageResult(completed_b.clone()));
    assert_eq!(shell.usage.row_titles(), ["current-b"]);

    // Actual tab activation invalidates B and restores A's hidden layout.
    dispatch(&mut shell, Msg::Projects(P::Activate(a.id)));
    assert!(!shell.panel_visible(&shell::usage_id()));
    assert_eq!(
        shell.usage.scope_identity().unwrap().0,
        PathBuf::from(&a.path)
    );
    assert!(shell.usage.row_titles().is_empty());
    dispatch(&mut shell, Msg::UsageResult(completed_b.clone()));
    assert!(shell.usage.row_titles().is_empty());

    // Close the background tab first, then the active tab, leaving no project.
    dispatch(&mut shell, Msg::Projects(P::Close(b.id)));
    dispatch(&mut shell, Msg::Projects(P::Close(a.id)));
    assert_eq!(shell.projects.current_project_id(), None);
    assert!(shell.usage.scope_identity().is_none());
    dispatch(&mut shell, Msg::UsageResult(completed_b.clone()));
    assert!(shell.usage.row_titles().is_empty());

    // Reopen the same B path through the project message, not a manually assigned
    // scope; B's saved visible layout starts a new query and a new lifecycle.
    dispatch(&mut shell, Msg::Projects(P::Opened(Ok(Some(b.clone())))));
    assert!(shell.panel_visible(&shell::usage_id()));
    let reopened_identity = shell.usage.scope_identity().unwrap();
    assert_eq!(reopened_identity.0, b_identity.0);
    assert_ne!(reopened_identity.1, b_identity.1);
    let (path, reopened_b) = usage_route_recv(&mut queries).await;
    assert_eq!(path, b.path);
    dispatch(&mut shell, Msg::UsageResult(completed_b));
    assert!(shell.usage.row_titles().is_empty());
    reopened_b.send(Ok(usage_route_rows("reopened-b"))).unwrap();
    dispatch(
        &mut shell,
        Msg::UsageResult(usage_route_recv(&mut results).await),
    );
    assert_eq!(shell.usage.row_titles(), ["reopened-b"]);
    assert!(queries.try_recv().is_err());
    drop(shell);
    std::fs::remove_dir_all(dir).unwrap();
}
