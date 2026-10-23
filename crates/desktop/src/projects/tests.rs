use super::*;
use crate::paths;

fn info(id: i64, path: &Path, name: &str) -> ProjectInfo {
    ProjectInfo {
        id,
        path: path.to_string_lossy().into_owned(),
        name: name.into(),
        last_active_ms: id as u64,
        created_ms: 1,
        updated_ms: id as u64,
    }
}

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// 一次成功的注册表读取(含目录是否还在的检查,与生产里后台任务用同一个 `Listing::scan`)。
fn listed(projects: Vec<ProjectInfo>) -> Message {
    Message::Listed(Ok(Listing::scan(projects)))
}

fn opened(state: &mut ProjectsState, info: ProjectInfo) {
    let _ = state.update(Message::Opened(Ok(Some(info))));
}

#[test]
fn a_fresh_data_dir_has_no_tabs_and_creates_the_registry_file() {
    let d = dir();
    let state = ProjectsState::open_in(d.path());
    assert_eq!(state.current_project_id(), None);
    assert!(state.notice().is_none());
    assert!(d.path().join(paths::PROJECTS_DB_FILE).exists());
}

#[test]
fn opening_a_project_adds_and_activates_its_tab_and_survives_a_restart() {
    let d = dir();
    let proj = d.path().join("book");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "book"));
    assert_eq!(state.current_project_id(), Some(1));

    let mut again = ProjectsState::open_in(d.path());
    assert_eq!(
        again.current_project_id(),
        None,
        "快照还没读回来,页签先不画"
    );
    let _ = again.update(listed(vec![info(1, &proj, "book")]));
    assert_eq!(
        again.current_project_id(),
        Some(1),
        "页签与当前项目随重启恢复"
    );
}

#[test]
fn a_registry_read_returning_without_a_tabbed_project_drops_that_tab() {
    let d = dir();
    let proj = d.path().join("gone");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "gone"));
    let _ = state.update(listed(vec![]));
    assert_eq!(state.current_project_id(), None);
    let reloaded = ProjectsState::open_in(d.path());
    assert!(
        reloaded.tabs.open().is_empty(),
        "取消登记后的页签也从落盘里消失"
    );
}

#[test]
fn a_failed_registry_read_keeps_the_tabs_and_only_warns() {
    let d = dir();
    let proj = d.path().join("keep");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "keep"));
    let _ = state.update(Message::Listed(Err("boom".into())));
    assert_eq!(state.current_project_id(), Some(1));
    assert!(state.notice().unwrap().contains("boom"));
}

#[test]
fn a_project_whose_directory_is_gone_stays_as_a_greyed_tab() {
    let d = dir();
    let proj = d.path().join("vanished");
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "vanished"));
    let _ = state.update(listed(vec![info(1, &proj, "vanished")]));
    assert_eq!(state.current_project_id(), Some(1), "不自动删除");
    assert!(state.missing.contains(&1));
}

#[test]
fn closing_a_tab_keeps_the_project_registered_and_picks_the_left_neighbour() {
    let d = dir();
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &a, "a"));
    opened(&mut state, info(2, &b, "b"));
    let _ = state.update(Message::Close(2));
    assert_eq!(state.current_project_id(), Some(1));
    assert_eq!(state.registry.len(), 2, "关页签不取消登记");
}

#[test]
fn opening_a_non_directory_only_shows_a_notice() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::Opened(Ok(None)));
    assert!(state.notice().unwrap().contains("文件夹"));
    assert_eq!(state.current_project_id(), None);
}

#[test]
fn an_unopenable_registry_disables_projects_and_never_falls_back_to_a_temp_database() {
    let d = dir();
    // 把数据库路径占成一个目录:SQLite 打不开。
    std::fs::create_dir(d.path().join(paths::PROJECTS_DB_FILE)).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    assert!(state.notice().unwrap().contains("不可用"));
    let _ = state.update(Message::FolderPicked(Some(d.path().to_path_buf())));
    assert_eq!(state.current_project_id(), None);
}

#[test]
fn a_newer_schema_tabs_file_is_never_overwritten() {
    let d = dir();
    let file = d.path().join(paths::PROJECT_TABS_FILE);
    std::fs::write(&file, r#"{"schema":99,"open":[1],"active":1}"#).unwrap();
    let proj = d.path().join("p");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(5, &proj, "p"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        r#"{"schema":99,"open":[1],"active":1}"#
    );
}

#[test]
fn cancelling_the_folder_picker_changes_nothing() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::FolderPicked(None));
    assert_eq!(state.current_project_id(), None);
    assert!(state.notice().is_none());
}

#[test]
fn scan_marks_exactly_the_projects_whose_directory_is_gone() {
    let d = dir();
    let here = d.path().join("here");
    std::fs::create_dir(&here).unwrap();
    let gone = d.path().join("gone");
    let listing = Listing::scan(vec![info(1, &here, "here"), info(2, &gone, "gone")]);
    assert_eq!(listing.missing, HashSet::from([2]));
    assert_eq!(listing.projects.len(), 2);
}

#[test]
fn reopening_a_project_whose_directory_came_back_clears_its_grey_state_at_once() {
    let d = dir();
    let proj = d.path().join("back");
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "back"));
    let _ = state.update(listed(vec![info(1, &proj, "back")]));
    assert!(state.missing.contains(&1));

    std::fs::create_dir(&proj).unwrap();
    opened(&mut state, info(1, &proj, "back"));
    assert!(!state.missing.contains(&1), "不必等后面的 Listed");
}

#[test]
fn a_list_failure_notice_clears_on_the_next_successful_read() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::Listed(Err("boom".into())));
    assert!(state.notice().unwrap().contains("boom"));
    let _ = state.update(listed(vec![]));
    assert!(state.notice().is_none());
}

#[test]
fn a_read_success_does_not_clear_an_unrelated_open_notice_but_a_new_pick_does() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::Opened(Ok(None)));
    let _ = state.update(listed(vec![]));
    assert!(state.notice().unwrap().contains("文件夹"));
    let _ = state.update(Message::PickFolder);
    assert!(
        state.notice().is_none(),
        "开始新一次选择就清掉上一次的打开提示"
    );
}

#[test]
fn an_unavailable_registry_notice_is_never_cleared_by_other_events() {
    let d = dir();
    std::fs::create_dir(d.path().join(paths::PROJECTS_DB_FILE)).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(listed(vec![]));
    let _ = state.update(Message::PickFolder);
    assert!(state.notice().unwrap().contains("不可用"));
}

#[test]
fn a_read_that_changes_nothing_does_not_rewrite_the_tabs_file() {
    let d = dir();
    let proj = d.path().join("same");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "same"));
    let file = d.path().join(paths::PROJECT_TABS_FILE);
    assert!(file.exists());
    std::fs::remove_file(&file).unwrap();
    let _ = state.update(listed(vec![info(1, &proj, "same")]));
    assert!(!file.exists(), "状态没变就不该写盘");
    let _ = state.update(Message::Close(1));
    assert!(file.exists(), "状态变了才写");
}

#[test]
fn a_long_notice_is_shortened_for_the_top_bar_on_a_char_boundary() {
    assert_eq!(shorten("short", 10), "short");
    let long = "错".repeat(100);
    let cut = shorten(&long, 20);
    assert_eq!(cut.chars().count(), 20);
    assert!(cut.ends_with('…'));
}

// ---- 顶栏拖窗保护(Task 10) ----

#[test]
fn hovering_a_tab_marks_a_topbar_control_hovered_and_leaving_clears_it() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let proj = d.path().join("book");
    std::fs::create_dir(&proj).unwrap();
    opened(&mut state, info(1, &proj, "book"));
    assert!(!state.topbar_control_hovered());

    let _ = state.update(Message::HoverEnter(1));
    assert!(
        state.topbar_control_hovered(),
        "a tab is a topbar control: pressing it must reach iced, not drag the window"
    );
    let _ = state.update(Message::HoverLeave(1));
    assert!(
        !state.topbar_control_hovered(),
        "leaving the tab gives the blank topbar back to the native drag"
    );
}

#[test]
fn open_with_migrates_the_legacy_registry_into_the_given_db_and_enables_wal() {
    let d = dir();
    let legacy = d.path().join("projects.db");
    let db = d.path().join("state").join("digger.db");
    {
        let c = rusqlite::Connection::open(&legacy).unwrap();
        c.execute_batch(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, last_active_ms INTEGER NOT NULL, created_ms INTEGER NOT NULL DEFAULT 0);
             INSERT INTO projects VALUES (3, '/old/book', 'Old book', 10, 5);",
        )
        .unwrap();
    }
    let state = ProjectsState::open_with(d.path(), &db, &legacy);
    assert!(state.notice().is_none(), "{:?}", state.notice());
    let host = state.project_host().expect("host");
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let listed = rt.block_on(host.projects.list_projects()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, 3);
    assert_eq!(digger_store::journal_mode(&db).unwrap(), "wal");
    assert!(!legacy.exists());
    assert!(d.path().join("projects.db.migrated").exists());
}

#[test]
fn a_failed_migration_shows_unavailable_instead_of_falling_back() {
    let d = dir();
    let legacy = d.path().join("projects.db");
    std::fs::write(&legacy, b"not a sqlite file").unwrap();
    let state = ProjectsState::open_with(d.path(), &d.path().join("digger.db"), &legacy);
    assert!(state.project_host().is_none());
    assert!(state.notice().is_some());
}

#[test]
fn production_registry_path_is_the_daemons_db_path() {
    assert_eq!(
        digger_paths::projects_db_path(),
        digger_paths::product().db_path()
    );
    assert_eq!(
        digger_paths::projects_db_path().parent(),
        Some(digger_paths::state_dir().as_path()),
        "the registry lives in the state dir, the same place diggerd opens it"
    );
}
