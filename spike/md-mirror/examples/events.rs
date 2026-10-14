use notify::{RecursiveMode, Watcher};
use std::time::{Duration, Instant};
fn main() {
    let d = std::env::temp_dir().join(format!("evt-{}", std::process::id()));
    std::fs::create_dir_all(d.join("chapters")).unwrap();
    let d = d.canonicalize().unwrap();
    std::fs::create_dir_all(d.join("tmp")).unwrap();
    std::fs::write(d.join("tmp/x.tmp"), "a").unwrap();
    std::fs::rename(d.join("tmp/x.tmp"), d.join("chapters/01.md")).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut w = notify::recommended_watcher(move |e| { let _ = tx.send((Instant::now(), e)); }).unwrap();
    w.watch(&d, RecursiveMode::Recursive).unwrap();
    std::thread::sleep(Duration::from_millis(700));
    while rx.try_recv().is_ok() {}
    let t0 = Instant::now();
    std::fs::write(d.join("chapters/02.md"), "n").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let t1 = Instant::now();
    std::fs::remove_file(d.join("chapters/01.md")).unwrap();
    println!("remove at +{:?}", t1 - t0);
    let end = Instant::now() + Duration::from_secs(4);
    while let Ok((t, e)) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        let e = e.unwrap();
        println!("+{:?} {:?} {}", t - t0, e.kind, e.paths[0].file_name().unwrap().to_string_lossy());
    }
}
