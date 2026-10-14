//! A real file watcher for the mirror (ADR-012 §5, file -> database). Events only say "look at this path": the
//! hash comparison in `Store::import` decides whether there is anything to do, so duplicate, late or spurious
//! events (including the ones our own writes cause) are harmless. Because OS event streams can drop or coalesce
//! events, a full `rescan` is the safety net, run at startup and after an overflow.
use crate::{Drain, Import, Result, Store};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Watcher {
    _w: RecommendedWatcher,
    rx: Receiver<PathBuf>,
    overflow: Arc<AtomicBool>,
    root: PathBuf,
}

#[derive(Debug, Default)]
pub struct Pumped {
    pub paths: usize,
    pub imports: Vec<(i64, Import)>,
    /// markdown files under the root that belong to no known document (a new chapter, created outside)
    pub unknown: Vec<PathBuf>,
    pub rescanned: bool,
    pub drained: Vec<(i64, Drain)>,
}

impl Watcher {
    pub fn start(root: &Path) -> Result<Watcher> {
        let root = root.canonicalize()?; // FSEvents reports canonical paths (/private/var, not /var)
        let (tx, rx) = channel();
        let overflow = Arc::new(AtomicBool::new(false));
        let (ov, internal) = (overflow.clone(), root.join(".digger"));
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(ev) => {
                if ev.need_rescan() { ov.store(true, Ordering::SeqCst); }
                if matches!(ev.kind, EventKind::Access(_)) { return; }
                for p in ev.paths { if !p.starts_with(&internal) { let _ = tx.send(p); } }
            }
            Err(_) => ov.store(true, Ordering::SeqCst),
        })?;
        w.watch(&root, RecursiveMode::Recursive)?;
        Ok(Watcher { _w: w, rx, overflow, root })
    }
}

impl Store {
    pub fn doc_id_for(&self, rel: &str) -> Result<Option<i64>> {
        use rusqlite::OptionalExtension;
        Ok(self.db.query_row("SELECT id FROM doc WHERE path=?", [rel], |r| r.get(0)).optional()?)
    }

    fn all_docs(&self) -> Result<Vec<i64>> {
        Ok(self.db.prepare("SELECT id FROM doc ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?)
    }

    /// Look at every document's file. Used at startup (edits made while the app was closed) and after an overflow.
    pub fn rescan(&mut self) -> Result<Vec<(i64, Import)>> {
        let mut out = vec![];
        for id in self.all_docs()? { out.push((id, self.import_when_stable(id, Duration::from_millis(30))?)); }
        Ok(out)
    }

    /// An editor may be in the middle of writing (truncate, then write): only import a file that reads the same twice.
    fn import_when_stable(&mut self, id: i64, gap: Duration) -> Result<Import> {
        let path = self.file_path(id)?;
        let mut last = std::fs::read(&path).ok();
        for _ in 0..50 {
            std::thread::sleep(gap);
            let now = std::fs::read(&path).ok();
            if now == last { break; }
            last = now;
        }
        self.import(id)
    }

    /// Wait up to `max_wait` for a change, let the burst settle (`quiet` without new events), import every touched
    /// document, then write whatever the outbox owes.
    pub fn pump(&mut self, w: &Watcher, quiet: Duration, max_wait: Duration) -> Result<Pumped> {
        let mut touched: BTreeSet<PathBuf> = BTreeSet::new();
        match w.rx.recv_timeout(max_wait) {
            Ok(p) => { touched.insert(p); }
            Err(RecvTimeoutError::Timeout) => return Ok(Pumped::default()),
            Err(RecvTimeoutError::Disconnected) => return Err("watcher stopped".into()),
        }
        let mut deadline = Instant::now() + quiet;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match w.rx.recv_timeout(left) {
                Ok(p) => { touched.insert(p); deadline = Instant::now() + quiet; }
                Err(_) => break,
            }
        }
        let mut out = Pumped { paths: touched.len(), ..Default::default() };
        if w.overflow.swap(false, Ordering::SeqCst) {
            out.rescanned = true;
            out.imports = self.rescan()?;
        } else {
            for p in touched {
                let Ok(rel) = p.strip_prefix(&w.root) else { continue };
                if p.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                match self.doc_id_for(&rel.to_string_lossy())? {
                    Some(id) => { let r = self.import_when_stable(id, quiet / 2)?; out.imports.push((id, r)); }
                    None => if p.exists() { out.unknown.push(p) },
                }
            }
        }
        out.drained = self.drain()?;
        Ok(out)
    }
}
