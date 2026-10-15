use super::*;
use crate::paths;

fn info(id: i64, path: &Path, name: &str) -> ProjectInfo {
    ProjectInfo { id, path: path.to_string_lossy().into_owned(), name: name.into(), last_active_ms: id as u64, created_ms: 1, updated_ms: id as u64 }
}

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
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
    assert_eq!(again.current_project_id(), None, "快照还没读回来,页签先不画");
    let _ = again.update(Message::Listed(Ok(vec![info(1, &proj, "book")])));
    assert_eq!(again.current_project_id(), Some(1), "页签与当前项目随重启恢复");
}

#[test]
fn a_registry_read_returning_without_a_tabbed_project_drops_that_tab() {
    let d = dir();
    let proj = d.path().join("gone");
    std::fs::create_dir(&proj).unwrap();
    let mut state = ProjectsState::open_in(d.path());
    opened(&mut state, info(1, &proj, "gone"));
    let _ = state.update(Message::Listed(Ok(vec![])));
    assert_eq!(state.current_project_id(), None);
    let reloaded = ProjectsState::open_in(d.path());
    assert!(reloaded.tabs.open().is_empty(), "取消登记后的页签也从落盘里消失");
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
    let _ = state.update(Message::Listed(Ok(vec![info(1, &proj, "vanished")])));
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
    assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"{"schema":99,"open":[1],"active":1}"#);
}

#[test]
fn cancelling_the_folder_picker_changes_nothing() {
    let d = dir();
    let mut state = ProjectsState::open_in(d.path());
    let _ = state.update(Message::FolderPicked(None));
    assert_eq!(state.current_project_id(), None);
    assert!(state.notice().is_none());
}
