//! Real file system events (FSEvents on macOS), real writes done the way editors do them.
use md_mirror_spike::watch::Watcher;
use md_mirror_spike::*;
use std::path::PathBuf;
use std::time::Duration;

const BASE: &str = "# 第一章\n\n第一段。天色渐暗。\n\n第二段。风从北边来。\n\n第三段。门没有关。\n";
const QUIET: Duration = Duration::from_millis(150);
const WAIT: Duration = Duration::from_secs(10);

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("md-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn setup(name: &str) -> (Store, Watcher, i64) {
    let mut s = Store::open(&dir(name)).unwrap();
    let id = s.create_doc("chapters/01.md", BASE).unwrap();
    s.drain().unwrap();
    let w = Watcher::start(&s.root).unwrap();
    std::thread::sleep(Duration::from_millis(300)); // let the stream start; also swallows our own first write
    let _ = s.pump(&w, QUIET, Duration::from_millis(400)); // events of the setup write, if any
    (s, w, id)
}
fn path(s: &Store, id: i64) -> PathBuf { s.file_path(id).unwrap() }

#[test]
fn a_save_in_another_editor_is_noticed_and_merged() {
    let (mut s, w, id) = setup("save");
    std::fs::write(path(&s, id), BASE.replace("风从北边来", "风从南边来")).unwrap();
    let p = s.pump(&w, QUIET, WAIT).unwrap();
    assert!(p.imports.iter().any(|(i, r)| *i == id && matches!(r, Import::Merged(_))), "{p:?}");
    assert!(s.text(id).unwrap().contains("风从南边来"));
}

#[test]
fn an_atomic_save_through_a_temp_file_and_rename_is_noticed() {
    let (mut s, w, id) = setup("atomic");
    let p = path(&s, id);
    let tmp = p.with_extension("md.tmp");
    std::fs::write(&tmp, BASE.replace("门没有关", "门没有关好")).unwrap();
    std::fs::rename(&tmp, &p).unwrap();
    let r = s.pump(&w, QUIET, WAIT).unwrap();
    assert!(r.imports.iter().any(|(_, r)| matches!(r, Import::Merged(_))), "{r:?}");
    assert!(s.text(id).unwrap().contains("门没有关好"));
}

#[test]
fn our_own_writes_do_not_loop() {
    let (mut s, w, id) = setup("own");
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
    s.drain().unwrap(); // writes the file: the watcher will report it
    let v = s.version(id).unwrap();
    let mut rounds = 0;
    loop {
        let p = s.pump(&w, QUIET, Duration::from_millis(800)).unwrap();
        if p.paths == 0 { break; }
        rounds += 1;
        assert!(p.imports.iter().all(|(_, r)| *r == Import::Unchanged), "{p:?}");
        assert!(rounds < 4, "events keep coming from our own writes");
    }
    assert_eq!(s.version(id).unwrap(), v, "an echo of our write is not an edit");
}

#[test]
fn a_burst_of_saves_is_one_settled_import_and_the_last_content_wins() {
    let (mut s, w, id) = setup("burst");
    let p = path(&s, id);
    let writer = std::thread::spawn({
        let p = p.clone();
        move || {
            for k in 0..40 { std::fs::write(&p, BASE.replace("门没有关", &format!("门没有关{k}"))).unwrap(); std::thread::sleep(Duration::from_millis(10)); }
        }
    });
    let mut imports = 0;
    let mut merged = 0;
    for _ in 0..6 {
        let r = s.pump(&w, QUIET, Duration::from_millis(1500)).unwrap();
        imports += r.imports.len();
        merged += r.imports.iter().filter(|(_, r)| matches!(r, Import::Merged(_))).count();
        if writer.is_finished() && r.paths == 0 { break; }
    }
    writer.join().unwrap();
    assert!(merged >= 1 && merged <= 6, "40 writes should settle into a few imports, got {merged} ({imports} looks)");
    assert!(s.text(id).unwrap().contains("门没有关39"), "{}", s.text(id).unwrap());
}

#[test]
fn a_truncate_then_write_is_not_imported_half_way() {
    let (mut s, w, id) = setup("trunc");
    let p = path(&s, id);
    let writer = std::thread::spawn(move || {
        std::fs::write(&p, "").unwrap(); // truncated
        std::thread::sleep(Duration::from_millis(60));
        std::fs::write(&p, BASE.replace("风从北边来", "风从南边来")).unwrap();
    });
    let r = s.pump(&w, QUIET, WAIT).unwrap();
    writer.join().unwrap();
    let _ = s.pump(&w, QUIET, Duration::from_millis(500));
    let t = s.text(id).unwrap();
    assert!(t.contains("风从南边来") && t.contains("第三段") && t.contains("第一章"), "{t}\n{r:?}");
    assert!(s.pending(id).unwrap().is_empty());
}

#[test]
fn a_file_cleared_for_good_is_parked_not_applied() {
    let (mut s, w, id) = setup("cleared");
    std::fs::write(path(&s, id), "").unwrap();
    let r = s.pump(&w, QUIET, WAIT).unwrap();
    assert!(r.imports.iter().any(|(_, r)| matches!(r, Import::WorkingState(_))), "{r:?}");
    assert_eq!(s.text(id).unwrap(), BASE);
}

#[test]
fn edits_made_while_the_app_was_closed_are_found_by_the_startup_scan() {
    let (s, w, id) = setup("closed");
    drop(w);
    let root = s.root.clone();
    let p = path(&s, id);
    drop(s);
    std::fs::write(&p, BASE.replace("天色渐暗", "天色昏暗")).unwrap(); // nothing was watching
    let mut s = Store::open(&root).unwrap();
    s.recover().unwrap();
    let r = s.rescan().unwrap();
    assert!(matches!(r[0].1, Import::Merged(_)), "{r:?}");
    assert!(s.text(id).unwrap().contains("天色昏暗"));
}

#[test]
fn a_dropped_event_is_caught_by_the_next_rescan() {
    let (mut s, w, id) = setup("dropped");
    drop(w); // no watcher: the event is never seen
    std::fs::write(path(&s, id), BASE.replace("门没有关", "门没有关紧")).unwrap();
    let r = s.rescan().unwrap();
    assert!(matches!(r[0].1, Import::Merged(_)));
    assert!(s.text(id).unwrap().contains("门没有关紧"));
}

#[test]
fn a_new_markdown_file_is_reported_not_adopted_and_a_deleted_one_is_written_back() {
    let (mut s, w, id) = setup("newdel");
    std::fs::write(s.root.join("chapters/02.md"), "# 新章\n\n正文。\n").unwrap();
    let r = s.pump(&w, QUIET, WAIT).unwrap();
    assert!(r.unknown.iter().any(|p| p.ends_with("chapters/02.md")), "{r:?}");
    std::fs::remove_file(path(&s, id)).unwrap();
    let r = s.pump(&w, QUIET, WAIT).unwrap();
    assert!(r.imports.iter().any(|(_, r)| *r == Import::Missing), "{r:?}");
    assert_eq!(std::fs::read_to_string(path(&s, id)).unwrap(), BASE, "the chapter file is back");
}

#[test]
fn internal_files_never_produce_events() {
    let (mut s, w, _id) = setup("internal");
    std::fs::write(s.root.join(".digger/tmp/x.tmp"), "x").unwrap();
    std::fs::write(s.root.join(".digger/note.md"), "x").unwrap();
    let r = s.pump(&w, QUIET, Duration::from_millis(800)).unwrap();
    assert_eq!(r.paths, 0, "{r:?}");
}
