use md_mirror_spike::*;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("md-mirror-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
const BASE: &str = "# 第一章\n\n第一段。天色渐暗。\n\n第二段。风从北边来。\n\n第三段。门没有关。\n";

fn setup(name: &str) -> (Store, i64) {
    let mut s = Store::open(&dir(name)).unwrap();
    let id = s.create_doc("chapters/01.md", BASE).unwrap();
    s.drain().unwrap();
    (s, id)
}
fn read(s: &Store, id: i64) -> String { std::fs::read_to_string(s.file_path(id).unwrap()).unwrap() }
fn write(s: &Store, id: i64, t: &str) { std::fs::write(s.file_path(id).unwrap(), t).unwrap() }

#[test]
fn a_commit_is_mirrored_atomically_and_the_hash_recorded() {
    let (mut s, id) = setup("basic");
    assert_eq!(read(&s, id), BASE);
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
    assert_eq!(read(&s, id), BASE, "the file is behind until the outbox is drained");
    assert_eq!(s.import(id).unwrap(), Import::Unchanged, "a file that is merely behind is not an external edit");
    s.drain().unwrap();
    assert_eq!(read(&s, id), s.text(id).unwrap());
    assert!(s.db.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0)).unwrap() == 0);
}

#[test]
fn a_crash_at_any_point_of_the_write_converges_without_loss() {
    for fault in [None, Some(Fault::AfterTempWritten), Some(Fault::AfterRename)] {
        let (mut s, id) = setup("crash");
        let new = BASE.replace("门没有关", "门没有关好");
        s.commit(id, &new, "App").unwrap();
        s.fault = fault;
        let r = s.drain();
        assert_eq!(r.is_err(), fault.is_some());
        // restart: reopen the database, clean temp files, import then drain
        let root = s.root.clone();
        drop(s);
        let mut s = Store::open(&root).unwrap();
        s.recover().unwrap();
        assert_eq!(s.import(id).unwrap(), Import::Unchanged, "{fault:?}: our own half-written write is not an external edit");
        s.drain().unwrap();
        assert_eq!(read(&s, id), new, "{fault:?}");
        assert_eq!(s.text(id).unwrap(), new);
        assert_eq!(std::fs::read_dir(root.join(".digger/tmp")).unwrap().count(), 0);
    }
}

#[test]
fn an_external_edit_merges_with_a_concurrent_commit() {
    let (mut s, id) = setup("merge");
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap(); // not mirrored yet
    write(&s, id, &BASE.replace("风从北边来", "风从南边来")); // author edits the stale file in another editor
    assert_eq!(s.drain().unwrap()[0].1, Drain::SkippedExternalEditPending, "the mirror must not overwrite the author's edit");
    assert!(matches!(s.import(id).unwrap(), Import::Merged(_)));
    let t = s.text(id).unwrap();
    assert!(t.contains("天色已暗") && t.contains("风从南边来"), "{t}");
    s.drain().unwrap();
    assert_eq!(read(&s, id), t);
}

#[test]
fn overlapping_edits_are_a_conflict_nothing_is_applied_and_the_file_is_kept() {
    let (mut s, id) = setup("conflict");
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
    s.drain().unwrap();
    let before = s.text(id).unwrap();
    // the file (written at the base version) was edited on the same words
    let theirs = BASE.replace("天色渐暗", "天色昏沉");
    // pretend the mirror hash is still the base: the editor started from that file
    s.db.execute("UPDATE mirror SET hash=?", [hash(BASE)]).unwrap();
    write(&s, id, &theirs);
    let Import::Conflict(pid) = s.import(id).unwrap() else { panic!("expected a conflict") };
    assert_eq!(s.text(id).unwrap(), before);
    assert_eq!(read(&s, id), theirs, "the author's file is untouched");
    assert_eq!(s.import(id).unwrap(), Import::Conflict(pid), "importing again does not pile up duplicates");
    assert_eq!(s.pending(id).unwrap().len(), 1);
    s.commit(id, &format!("{before}追加。\n"), "App").unwrap();
    assert_eq!(s.drain().unwrap()[0].1, Drain::SkippedExternalEditPending);
    assert_eq!(read(&s, id), theirs, "still untouched while the conflict is open");
    let v = s.resolve(pid, "# 第一章\n\n第一段。天色昏沉而暗。\n\n第二段。风从北边来。\n\n第三段。门没有关。\n追加。\n").unwrap();
    assert!(v > 1);
    s.drain().unwrap();
    assert_eq!(read(&s, id), s.text(id).unwrap());
}

#[test]
fn a_wholesale_replacement_is_a_working_state_not_an_import() {
    let (mut s, id) = setup("wholesale");
    let before = s.text(id).unwrap();
    s.db.execute("DELETE FROM mirror", []).unwrap(); // e.g. project restored from Git: no recorded base
    write(&s, id, "# 完全不同\n\n另一本书。\n");
    let Import::WorkingState(pid) = s.import(id).unwrap() else { panic!() };
    assert_eq!(s.text(id).unwrap(), before);
    assert_eq!(s.pending(id).unwrap()[0].theirs, "# 完全不同\n\n另一本书。\n");
    assert_eq!(s.pending(id).unwrap()[0].id, pid);
}

#[test]
fn bom_and_crlf_alone_are_not_an_edit() {
    let (mut s, id) = setup("crlf");
    write(&s, id, &format!("\u{feff}{}", BASE.replace('\n', "\r\n")));
    assert_eq!(s.import(id).unwrap(), Import::Unchanged);
    assert_eq!(s.version(id).unwrap(), 1);
    // and a real edit made by such an editor merges
    write(&s, id, &format!("\u{feff}{}", BASE.replace("门没有关", "门没有关上").replace('\n', "\r\n")));
    assert!(matches!(s.import(id).unwrap(), Import::Merged(_)));
    assert!(s.text(id).unwrap().contains("门没有关上") && !s.text(id).unwrap().contains('\r'));
}

#[test]
fn invalid_utf8_and_a_deleted_file_do_nothing_destructive() {
    let (mut s, id) = setup("odd");
    write(&s, id, "x");
    std::fs::write(s.file_path(id).unwrap(), [0xff, 0xfe, 0x00, 0x41]).unwrap();
    assert_eq!(s.import(id).unwrap(), Import::Unreadable);
    assert_eq!(s.text(id).unwrap(), BASE);
    std::fs::remove_file(s.file_path(id).unwrap()).unwrap();
    assert_eq!(s.import(id).unwrap(), Import::Missing);
    s.drain().unwrap();
    assert_eq!(read(&s, id), BASE, "a deleted mirror file is rewritten, not treated as deleting the chapter");
}

/// the author's editor saves in the window between the guard and the replace, the way editors do it
fn save_atomically(path: &std::path::Path, text: &str) {
    let tmp = path.with_extension("md.swp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

#[test]
fn an_edit_saved_between_the_guard_and_the_replace_is_recovered_not_lost() {
    for in_place in [false, true] {
        let (mut s, id) = setup(&format!("race{in_place}"));
        s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
        let theirs = BASE.replace("风从北边来", "风从南边来");
        let t2 = theirs.clone();
        s.hook = Some(Box::new(move |h, path| {
            if h == Hook::BeforeRename { if in_place { std::fs::write(path, &t2).unwrap() } else { save_atomically(path, &t2) } }
        }));
        let r = s.drain().unwrap().remove(0).1;
        s.hook = None;
        assert!(matches!(r, Drain::WrittenAfterRace(Import::Merged(_))), "{in_place}: {r:?}");
        let t = s.text(id).unwrap();
        assert!(t.contains("天色已暗") && t.contains("风从南边来"), "{in_place}: {t}");
        s.drain().unwrap();
        assert_eq!(read(&s, id), t, "{in_place}: the merged text reaches the file");
    }
}

#[test]
fn an_overlapping_edit_in_the_window_is_parked_with_the_authors_text() {
    let (mut s, id) = setup("race-conflict");
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
    let theirs = BASE.replace("天色渐暗", "天色昏沉");
    let t2 = theirs.clone();
    s.hook = Some(Box::new(move |h, path| if h == Hook::BeforeRename { save_atomically(path, &t2) }));
    let r = s.drain().unwrap().remove(0).1;
    assert!(matches!(r, Drain::WrittenAfterRace(Import::Conflict(_))), "{r:?}");
    s.hook = None;
    let p = s.pending(id).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].theirs, theirs, "the author's version is kept");
    assert_eq!(read(&s, id), s.text(id).unwrap(), "the file holds the database text, and nothing is stuck waiting");
    assert_eq!(s.drain().unwrap().iter().filter(|(_, d)| *d == Drain::SkippedExternalEditPending).count(), 0);
}

#[test]
fn a_write_through_an_old_file_handle_after_the_replace_is_caught() {
    use std::io::{Seek, Write};
    let (mut s, id) = setup("late");
    s.commit(id, &BASE.replace("天色渐暗", "天色已暗"), "App").unwrap();
    // an editor holds the file open (in-place saver) and writes after our replace
    let held = std::sync::Arc::new(std::sync::Mutex::new(None::<std::fs::File>));
    let h2 = held.clone();
    let theirs = BASE.replace("门没有关", "门没有关死");
    let t2 = theirs.clone();
    s.hook = Some(Box::new(move |h, path| match h {
        Hook::BeforeRename => *h2.lock().unwrap() = Some(std::fs::OpenOptions::new().write(true).open(path).unwrap()),
        Hook::AfterRename => {
            let mut g = h2.lock().unwrap();
            let f = g.as_mut().unwrap();
            f.set_len(0).unwrap(); f.rewind().unwrap(); f.write_all(t2.as_bytes()).unwrap(); f.sync_all().unwrap();
        }
    }));
    // the late write lands after finish_drain; the immediate check (hard link shares the old inode) already sees it
    let r = s.drain().unwrap().remove(0).1;
    s.hook = None;
    assert!(matches!(r, Drain::WrittenAfterRace(Import::Merged(_))), "{r:?}");
    let t = s.text(id).unwrap();
    assert!(t.contains("天色已暗") && t.contains("门没有关死"), "{t}");
    s.drain().unwrap();
    assert_eq!(read(&s, id), t);
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}
/// add a uniquely marked sentence to a random paragraph (end of it, or as a new paragraph)
fn mark(r: &mut Rng, text: &str, m: &str) -> String {
    let mut paras: Vec<String> = text.split("\n\n").map(String::from).collect();
    let i = r.below(paras.len());
    if r.below(2) == 0 { paras[i] = format!("{}{m}。", paras[i].trim_end_matches('\n')); if i + 1 == paras.len() { paras[i].push('\n') } }
    else { paras.insert(i + 1, format!("{m}。")); }
    paras.join("\n\n")
}

#[test]
fn random_interleavings_never_lose_an_edit_silently() {
    let mut stats = (0u32, 0u32, 0u32, 0u32);
    for seed in 1..=300u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (mut s, id) = setup(&format!("rand{seed}"));
        let mut markers = vec![];
        for step in 0..(5 + r.below(20)) {
            match r.below(7) {
                0 | 1 => { let m = format!("甲{seed}x{step}"); let t = mark(&mut r, &s.text(id).unwrap(), &m); s.commit(id, &t, "App").unwrap(); markers.push(m); }
                2 => { // the author edits the file as it currently is
                    let m = format!("乙{seed}x{step}");
                    let cur = std::fs::read_to_string(s.file_path(id).unwrap()).unwrap();
                    let t = mark(&mut r, &cur, &m);
                    write(&s, id, &t); markers.push(m);
                }
                3 => { let _ = s.drain(); }
                4 => { let _ = s.import(id); }
                6 => { // the author saves in the window of a drain
                    let m = format!("丙{seed}x{step}");
                    let cur = std::fs::read_to_string(s.file_path(id).unwrap()).unwrap();
                    let t = mark(&mut r, &cur, &m);
                    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
                    let f2 = fired.clone();
                    s.hook = Some(Box::new(move |h, path| if h == Hook::BeforeRename { save_atomically(path, &t); f2.set(true); }));
                    let _ = s.drain();
                    s.hook = None;
                    if fired.get() { markers.push(m); } // the author only saved if the drain reached the window
                }
                _ => { // crash during a write, then restart
                    s.fault = Some(if r.below(2) == 0 { Fault::AfterTempWritten } else { Fault::AfterRename });
                    let _ = s.drain();
                    let root = s.root.clone();
                    s = Store::open(&root).unwrap();
                    s.recover().unwrap();
                }
            }
        }
        // quiesce
        for _ in 0..4 { let _ = s.import(id); let _ = s.drain(); }
        let text = s.text(id).unwrap();
        let parked: String = s.pending(id).unwrap().iter().map(|p| format!("{}{}", p.ours, p.theirs)).collect();
        // a file that still disagrees with the database is only allowed while a pending item holds the difference
        let file = read(&s, id);
        if s.pending(id).unwrap().is_empty() { assert_eq!(file, text, "seed {seed}: file and database diverged with nothing pending"); }
        for m in &markers {
            let in_file = file.contains(m.as_str());
            assert!(text.contains(m.as_str()) || parked.contains(m.as_str()) || in_file, "seed {seed}: edit {m} was lost silently\n{text}");
        }
        stats.0 += 1;
        if !s.pending(id).unwrap().is_empty() { stats.1 += 1 }
    }
    eprintln!("{} runs, {} ended with a parked conflict/working state", stats.0, stats.1);
}
