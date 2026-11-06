// Full product message paths: the project selector and closing flag must not
// change before the writing page's flush and durable recovery have succeeded.
fn writing_fixture() -> (
    tempfile::TempDir,
    ProductShell,
    dozer_model::ProjectInfo,
    dozer_model::ProjectInfo,
) {
    let dir = tempfile::tempdir().unwrap();
    let project = |id, name: &str| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        let mut w = crate::markdown::workbench::Workbench::new();
        w.open_project(path.clone()).unwrap();
        w.create(name, "chapter.md").unwrap();
        dozer_model::ProjectInfo {
            id,
            path: path.to_string_lossy().into_owned(),
            name: name.into(),
            last_active_ms: 1,
            created_ms: 1,
            updated_ms: 1,
        }
    };
    let a = project(1, "A");
    let b = project(2, "B");
    let mut shell = ProductShell::with_shell_path(dir.path().join("shell.json"));
    send(
        &mut shell,
        Msg::Projects(crate::projects::Message::Opened(Ok(Some(a.clone())))),
    );
    shell.markdown.test_edit_text("latest A input");
    (dir, shell, a, b)
}

fn drive_writing_task(shell: &mut ProductShell, task: Task<Msg>) {
    use iced::futures::StreamExt;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut tasks = std::collections::VecDeque::from([task]);
        while let Some(task) = tasks.pop_front() {
            if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        let replayed_host_action = matches!(message, Msg::Markdown(crate::markdown::Message::HostTransitionReady(..)));
                        let next = Product::update(shell, HostMessage::Product(message));
                        if !replayed_host_action {
                            tasks.push_back(next);
                        }
                    }
                }
            }
        }
    });
    shell.markdown.settle_load_for_test();
}

#[test]
fn writing_transition_keeps_project_selection_until_flush_and_recovery_complete() {
    let (_dir, mut shell, a, b) = writing_fixture();
    // Drop the native flush task and deliver the controlled page ACK explicitly.
    send(
        &mut shell,
        Msg::Projects(crate::projects::Message::Opened(Ok(Some(b.clone())))),
    );
    assert_eq!(shell.projects.current_project_id(), Some(a.id));
    assert!(!shell.closing);
    let ack = shell.markdown.test_flush_reply();
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(ack)));
    drive_writing_task(&mut shell, task);
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(crate::markdown::Message::TransitionKeep)));
    drive_writing_task(&mut shell, task);
    assert_eq!(shell.projects.current_project_id(), Some(b.id));
    assert_eq!(
        shell.markdown.current_document().unwrap().document.title,
        "B"
    );
    assert!(shell.pending_writing_transition.is_none());
}

#[test]
fn writing_transition_failure_keeps_both_product_and_editor_in_the_old_project() {
    let (_dir, mut shell, a, b) = writing_fixture();
    let working = PathBuf::from(&a.path).join(".digger/working");
    let _ = std::fs::remove_dir_all(&working);
    std::fs::write(&working, "not a directory").unwrap();
    send(
        &mut shell,
        Msg::Projects(crate::projects::Message::Opened(Ok(Some(b)))),
    );
    let ack = shell.markdown.test_flush_reply();
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(ack)));
    drive_writing_task(&mut shell, task);
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(crate::markdown::Message::TransitionKeep)));
    drive_writing_task(&mut shell, task);
    assert_eq!(shell.projects.current_project_id(), Some(a.id));
    assert_eq!(
        shell.markdown.current_document().unwrap().document.title,
        "A"
    );
    assert!(shell.pending_writing_transition.is_none());
    assert!(!shell.closing);
}

#[test]
fn writing_transition_failed_recovery_prevents_normal_exit() {
    let (_dir, mut shell, a, _b) = writing_fixture();
    let working = PathBuf::from(&a.path).join(".digger/working");
    let _ = std::fs::remove_dir_all(&working);
    std::fs::write(&working, "not a directory").unwrap();
    send(&mut shell, Msg::Exit);
    assert!(
        !shell.closing,
        "exit cannot begin before recovery is durable"
    );
    let ack = shell.markdown.test_flush_reply();
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(ack)));
    drive_writing_task(&mut shell, task);
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(crate::markdown::Message::TransitionKeep)));
    drive_writing_task(&mut shell, task);
    assert!(
        !shell.closing,
        "failed recovery must leave the product running"
    );
    assert!(!shell.shutdown_started);
}

#[test]
fn writing_transition_normal_exit_starts_only_after_the_final_draft_is_durable() {
    let (_dir, mut shell, a, _b) = writing_fixture();
    let id = shell
        .markdown
        .current_document()
        .unwrap()
        .document
        .id
        .clone();
    send(&mut shell, Msg::Exit);
    assert!(!shell.closing);
    let ack = shell.markdown.test_flush_reply();
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(ack)));
    drive_writing_task(&mut shell, task);
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(crate::markdown::Message::TransitionKeep)));
    drive_writing_task(&mut shell, task);
    assert!(shell.closing);
    let recovery =
        digger_store::writing::RecoveryStore::open(std::path::Path::new(&a.path)).unwrap();
    let file = recovery.list_document(&id).unwrap().pop().unwrap();
    let draft: digger_store::writing::RecoveryDraft =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(draft.text, "latest A input");
}

#[test]
fn writing_transition_file_entry_imports_the_selected_path_after_a_deliberate_keep() {
    let (_dir, mut shell, a, _b) = writing_fixture();
    let path = PathBuf::from(&a.path).join("external.md");
    std::fs::write(&path, "# imported from file tree").unwrap();
    send(&mut shell, Msg::EditWritingFile(a.id, path.clone()));
    let ack = shell.markdown.test_flush_reply();
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(ack)));
    drive_writing_task(&mut shell, task);
    assert_eq!(shell.markdown.current_document().unwrap().document.relative_path, "chapter.md");
    let task = Product::update(&mut shell, HostMessage::Product(Msg::Markdown(crate::markdown::Message::TransitionKeep)));
    drive_writing_task(&mut shell, task);
    let current = shell.markdown.current_document().unwrap();
    assert_eq!(current.document.relative_path, "external.md");
    assert_eq!(current.snapshot.text, "# imported from file tree");
    assert!(shell.panel_visible(&shell::markdown_id()));
    send(&mut shell, Msg::EditWritingFile(999, path));
    assert_eq!(shell.markdown.current_document().unwrap().document.relative_path, "external.md");
}
