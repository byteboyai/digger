//! Markdown text is authoritative in SQLite; `.md` files are a mirror (ADR-012 §5).
//!
//! database -> file: an outbox row is written in the same transaction as the commit; `drain` writes the file
//! atomically (temp file, fsync, rename, fsync dir) and only then records the mirror hash.
//! file -> database: `import` finds the file's base by the recorded mirror hash in the document's history and
//! three-way merges it (text-merge spike). No base, overlap or unreadable bytes never change the database.
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use text_merge_spike::merge;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Spike hash (FNV-1a 64). Production uses SHA-256.
pub fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() { h ^= *b as u64; h = h.wrapping_mul(0x100000001b3); }
    format!("{h:016x}")
}

/// What an external editor may legitimately change without changing the text: a BOM and CRLF line endings.
pub fn normalize(s: &str) -> String { s.strip_prefix('\u{feff}').unwrap_or(s).replace("\r\n", "\n") }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fault { AfterTempWritten, AfterRename }

#[derive(Debug, PartialEq)]
pub enum Drain { Written(i64), AlreadyInSync, SkippedExternalEditPending }

#[derive(Debug, PartialEq)]
pub enum Import {
    Unchanged,
    /// merged into the database as a new version (actor External); the file is rewritten by the outbox
    Merged(i64),
    /// the two sides overlap; nothing applied, file untouched, pending row id
    Conflict(i64),
    /// no usable base (replaced wholesale / history gone): kept as a working state, nothing applied
    WorkingState(i64),
    Unreadable,
    Missing,
}

#[derive(Debug, Clone)]
pub struct Pending { pub id: i64, pub kind: String, pub base: String, pub ours: String, pub theirs: String }

pub struct Store { pub db: Connection, pub root: PathBuf, pub fault: Option<Fault> }

impl Store {
    pub fn open(root: &Path) -> Result<Store> {
        std::fs::create_dir_all(root.join(".digger/tmp"))?;
        let db = Connection::open(root.join(".digger/project.db"))?;
        db.execute_batch("
            PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS doc(id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, text TEXT NOT NULL, version INTEGER NOT NULL, hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS history(doc_id INTEGER NOT NULL, hash TEXT NOT NULL, text TEXT NOT NULL, version INTEGER, actor TEXT NOT NULL, PRIMARY KEY(doc_id, hash));
            CREATE TABLE IF NOT EXISTS mirror(doc_id INTEGER PRIMARY KEY, hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox(doc_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS pending(id INTEGER PRIMARY KEY, doc_id INTEGER NOT NULL, kind TEXT NOT NULL, base TEXT NOT NULL, ours TEXT NOT NULL, theirs TEXT NOT NULL, ours_hash TEXT NOT NULL, theirs_hash TEXT NOT NULL, UNIQUE(doc_id, kind, ours_hash, theirs_hash));
        ")?;
        Ok(Store { db, root: root.into(), fault: None })
    }

    pub fn create_doc(&self, path: &str, text: &str) -> Result<i64> {
        let tx = self.db.unchecked_transaction()?;
        let h = hash(text);
        tx.execute("INSERT INTO doc(path,text,version,hash) VALUES(?,?,1,?)", params![path, text, h])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO history VALUES(?,?,?,1,'App')", params![id, h, text])?;
        tx.execute("INSERT INTO outbox VALUES(?,1)", params![id])?;
        tx.commit()?;
        Ok(id)
    }

    pub fn text(&self, id: i64) -> Result<String> { Ok(self.db.query_row("SELECT text FROM doc WHERE id=?", [id], |r| r.get(0))?) }
    pub fn version(&self, id: i64) -> Result<i64> { Ok(self.db.query_row("SELECT version FROM doc WHERE id=?", [id], |r| r.get(0))?) }
    fn doc(&self, id: i64) -> Result<(String, String, i64, String)> {
        Ok(self.db.query_row("SELECT path,text,version,hash FROM doc WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?)
    }
    pub fn file_path(&self, id: i64) -> Result<PathBuf> { Ok(self.root.join(self.doc(id)?.0)) }
    pub fn pending(&self, id: i64) -> Result<Vec<Pending>> {
        let mut st = self.db.prepare("SELECT id,kind,base,ours,theirs FROM pending WHERE doc_id=? ORDER BY id")?;
        let rows = st.query_map([id], |r| Ok(Pending { id: r.get(0)?, kind: r.get(1)?, base: r.get(2)?, ours: r.get(3)?, theirs: r.get(4)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// One transaction: new text + history + outbox. A failed or late mirror never blocks this.
    pub fn commit(&self, id: i64, text: &str, actor: &str) -> Result<i64> {
        let tx = self.db.unchecked_transaction()?;
        let v = commit_in(&tx, id, text, actor)?;
        tx.commit()?;
        Ok(v)
    }

    /// Write the outbox's documents to their files. Never overwrites an external edit that has not been imported.
    pub fn drain(&mut self) -> Result<Vec<(i64, Drain)>> {
        let ids: Vec<i64> = self.db.prepare("SELECT doc_id FROM outbox ORDER BY doc_id")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        let mut out = vec![];
        for id in ids { let r = self.drain_one(id)?; out.push((id, r)); }
        Ok(out)
    }

    fn drain_one(&mut self, id: i64) -> Result<Drain> {
        let (path, text, version, h) = self.doc(id)?;
        let full = self.root.join(&path);
        let mirror: Option<String> = self.db.query_row("SELECT hash FROM mirror WHERE doc_id=?", [id], |r| r.get(0)).optional()?;
        match std::fs::read(&full) {
            Ok(bytes) => {
                let fh = match String::from_utf8(bytes) { Ok(s) => hash(&normalize(&s)), Err(_) => return Ok(Drain::SkippedExternalEditPending) };
                if fh == h {
                    // a crash after the rename (or an identical external edit): the file is already right
                    self.finish_drain(id, version, &h)?;
                    return Ok(Drain::AlreadyInSync);
                }
                if Some(&fh) != mirror.as_ref() { return Ok(Drain::SkippedExternalEditPending); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(parent) = full.parent() { std::fs::create_dir_all(parent)?; }
        let tmp = self.root.join(format!(".digger/tmp/{id}-{version}.tmp"));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        if self.fault == Some(Fault::AfterTempWritten) { self.fault = None; return Err("injected crash after temp write".into()); }
        std::fs::rename(&tmp, &full)?;
        if let Some(parent) = full.parent() { std::fs::File::open(parent)?.sync_all()?; }
        if self.fault == Some(Fault::AfterRename) { self.fault = None; return Err("injected crash after rename".into()); }
        self.finish_drain(id, version, &h)?;
        Ok(Drain::Written(version))
    }

    /// Record the mirror hash; drop the outbox row only if nothing newer was committed meanwhile.
    fn finish_drain(&self, id: i64, version: i64, h: &str) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute("INSERT INTO mirror VALUES(?,?) ON CONFLICT(doc_id) DO UPDATE SET hash=excluded.hash", params![id, h])?;
        tx.execute("DELETE FROM outbox WHERE doc_id=? AND version<=?", params![id, version])?;
        tx.commit()?;
        Ok(())
    }

    /// Startup recovery: temp files are never trusted; the outbox is whatever is still owed.
    pub fn recover(&mut self) -> Result<()> {
        for e in std::fs::read_dir(self.root.join(".digger/tmp"))? { std::fs::remove_file(e?.path())?; }
        Ok(())
    }

    pub fn import(&mut self, id: i64) -> Result<Import> {
        let (path, ours, _v, cur_hash) = self.doc(id)?;
        let bytes = match std::fs::read(self.root.join(&path)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // a deleted mirror file is not a deletion of the document: write it back
                self.db.execute("INSERT INTO outbox VALUES(?,(SELECT version FROM doc WHERE id=?)) ON CONFLICT(doc_id) DO NOTHING", params![id, id])?;
                return Ok(Import::Missing);
            }
            Err(e) => return Err(e.into()),
        };
        let Ok(raw) = String::from_utf8(bytes) else { return Ok(Import::Unreadable) };
        let theirs = normalize(&raw);
        let th = hash(&theirs);
        let mirror: Option<String> = self.db.query_row("SELECT hash FROM mirror WHERE doc_id=?", [id], |r| r.get(0)).optional()?;
        if th == cur_hash {
            self.db.execute("INSERT INTO mirror VALUES(?,?) ON CONFLICT(doc_id) DO UPDATE SET hash=excluded.hash", params![id, th])?;
            self.db.execute("DELETE FROM outbox WHERE doc_id=?", [id])?;
            return Ok(Import::Unchanged);
        }
        if mirror.as_deref() == Some(th.as_str()) { return Ok(Import::Unchanged); } // the file is just behind the database
        // an external edit. Keep what the author wrote in history first, so a later import can find it as a base.
        let base: Option<String> = match &mirror {
            Some(m) => self.db.query_row("SELECT text FROM history WHERE doc_id=? AND hash=?", params![id, m], |r| r.get(0)).optional()?,
            None => None,
        };
        let ours_hash = cur_hash.clone();
        let Some(base) = base else {
            return Ok(Import::WorkingState(self.park(id, "working", "", &ours, &theirs, &ours_hash, &th)?));
        };
        let m = merge(&base, &ours, &theirs);
        let Some(text) = m.text() else {
            return Ok(Import::Conflict(self.park(id, "conflict", &base, &ours, &theirs, &ours_hash, &th)?));
        };
        let tx = self.db.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO history VALUES(?,?,?,NULL,'External')", params![id, th, theirs])?;
        let v = if text == ours { None } else { Some(commit_in(&tx, id, &text, "External")?) };
        // the file now reflects `theirs`; the outbox rewrites it with the merged text
        tx.execute("INSERT INTO mirror VALUES(?,?) ON CONFLICT(doc_id) DO UPDATE SET hash=excluded.hash", params![id, th])?;
        if v.is_none() { tx.execute("INSERT INTO outbox VALUES(?,(SELECT version FROM doc WHERE id=?)) ON CONFLICT(doc_id) DO NOTHING", params![id, id])?; }
        tx.commit()?;
        Ok(Import::Merged(v.unwrap_or(self.version(id)?)))
    }

    fn park(&self, id: i64, kind: &str, base: &str, ours: &str, theirs: &str, oh: &str, th: &str) -> Result<i64> {
        self.db.execute("INSERT OR IGNORE INTO pending(doc_id,kind,base,ours,theirs,ours_hash,theirs_hash) VALUES(?,?,?,?,?,?,?)", params![id, kind, base, ours, theirs, oh, th])?;
        Ok(self.db.query_row("SELECT id FROM pending WHERE doc_id=? AND kind=? AND ours_hash=? AND theirs_hash=?", params![id, kind, oh, th], |r| r.get(0))?)
    }

    /// The author resolved a conflict / accepted a working state: `text` becomes the new version and replaces the
    /// file (the file's content was consumed by the resolution, so the mirror hash moves to it).
    pub fn resolve(&mut self, pending_id: i64, text: &str) -> Result<i64> {
        let (id, th): (i64, String) = self.db.query_row("SELECT doc_id,theirs_hash FROM pending WHERE id=?", [pending_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let theirs: String = self.db.query_row("SELECT theirs FROM pending WHERE id=?", [pending_id], |r| r.get(0))?;
        let tx = self.db.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO history VALUES(?,?,?,NULL,'External')", params![id, th, theirs])?;
        let v = commit_in(&tx, id, text, "Resolution")?;
        tx.execute("INSERT INTO mirror VALUES(?,?) ON CONFLICT(doc_id) DO UPDATE SET hash=excluded.hash", params![id, th])?;
        tx.execute("DELETE FROM pending WHERE id=?", [pending_id])?;
        tx.commit()?;
        Ok(v)
    }
}

fn commit_in(tx: &rusqlite::Transaction, id: i64, text: &str, actor: &str) -> Result<i64> {
    let h = hash(text);
    let v: i64 = tx.query_row("SELECT version FROM doc WHERE id=?", [id], |r| r.get::<_, i64>(0))? + 1;
    tx.execute("UPDATE doc SET text=?,version=?,hash=? WHERE id=?", params![text, v, h, id])?;
    tx.execute("INSERT INTO history VALUES(?,?,?,?,?) ON CONFLICT(doc_id,hash) DO UPDATE SET version=excluded.version", params![id, h, text, v, actor])?;
    tx.execute("INSERT INTO outbox VALUES(?,?) ON CONFLICT(doc_id) DO UPDATE SET version=excluded.version", params![id, v])?;
    Ok(v)
}
