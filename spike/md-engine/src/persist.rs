//! Block identity in SQLite (ADR-012: these tables live in the project database, next to the text).
//! The text, the ids in order, the forwarding table, the run counter and the Agent history are one transaction.

use crate::sync::{Doc, State};
use rusqlite::{Connection, OptionalExtension, params};

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("
        CREATE TABLE IF NOT EXISTS doc_state(doc_id INTEGER PRIMARY KEY, version INTEGER NOT NULL, text TEXT NOT NULL, next_id INTEGER NOT NULL, runs INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS block(doc_id INTEGER NOT NULL, ord INTEGER NOT NULL, block_id INTEGER NOT NULL, PRIMARY KEY(doc_id, ord));
        CREATE TABLE IF NOT EXISTS block_forward(doc_id INTEGER NOT NULL, old_id INTEGER NOT NULL, new_id INTEGER NOT NULL, run_id INTEGER NOT NULL, PRIMARY KEY(doc_id, old_id));
        CREATE TABLE IF NOT EXISTS block_agent_history(doc_id INTEGER NOT NULL, block_id INTEGER NOT NULL, run_id INTEGER NOT NULL, at_version INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS block_agent_history_block ON block_agent_history(doc_id, block_id);
    ")
}

pub fn save(db: &Connection, doc_id: i64, doc: &mut Doc) -> rusqlite::Result<()> {
    let st = doc.export_state();
    let tx = db.unchecked_transaction()?;
    let r = (|| {
        tx.execute("INSERT INTO doc_state VALUES(?,?,?,?,?) ON CONFLICT(doc_id) DO UPDATE SET version=excluded.version, text=excluded.text, next_id=excluded.next_id, runs=excluded.runs",
            params![doc_id, st.version as i64, st.text, st.next_id as i64, st.runs as i64])?;
        tx.execute("DELETE FROM block WHERE doc_id=?", [doc_id])?;
        for (ord, id) in st.ids.iter().enumerate() { tx.execute("INSERT INTO block VALUES(?,?,?)", params![doc_id, ord as i64, *id as i64])?; }
        tx.execute("DELETE FROM block_forward WHERE doc_id=?", [doc_id])?;
        for (o, n, r) in &st.aliases { tx.execute("INSERT INTO block_forward VALUES(?,?,?,?)", params![doc_id, *o as i64, *n as i64, *r as i64])?; }
        for (b, r, v) in &st.history { tx.execute("INSERT INTO block_agent_history VALUES(?,?,?,?)", params![doc_id, *b as i64, *r as i64, *v as i64])?; }
        Ok(())
    })();
    match r {
        Ok(()) => tx.commit(),
        Err(e) => {
            // keep the unsaved history rows in memory: they were not written
            doc.restore_history(st.history);
            Err(e)
        }
    }
}

/// The document as it was saved, or None if never saved. `text` is the text as it is now.
pub fn load(db: &Connection, doc_id: i64, text: &str) -> rusqlite::Result<Option<Doc>> {
    let Some((version, saved, next_id, runs)) = db.query_row("SELECT version,text,next_id,runs FROM doc_state WHERE doc_id=?", [doc_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))).optional()? else { return Ok(None) };
    let ids: Vec<u64> = db.prepare("SELECT block_id FROM block WHERE doc_id=? ORDER BY ord")?.query_map([doc_id], |r| r.get::<_, i64>(0).map(|x| x as u64))?.collect::<Result<_, _>>()?;
    let aliases: Vec<(u64, u64, u64)> = db.prepare("SELECT old_id,new_id,run_id FROM block_forward WHERE doc_id=?")?.query_map([doc_id], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?.collect::<Result<_, _>>()?;
    let st = State { version: version as u64, text: saved, ids, next_id: next_id as u64, runs: runs as u64, aliases, history: vec![] };
    Ok(Some(Doc::import_state(&st, text)))
}

/// Which runs wrote this block (block_agent_history): (run, version), oldest first.
pub fn agent_history(db: &Connection, doc_id: i64, block_id: u64) -> rusqlite::Result<Vec<(u64, u64)>> {
    db.prepare("SELECT run_id,at_version FROM block_agent_history WHERE doc_id=? AND block_id=? ORDER BY at_version, rowid")?
        .query_map(params![doc_id, block_id as i64], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)))?.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{AgentError, AgentOp, Step, fnv};

    const CH: &str = "# 标题\n\n第一段文字内容足够长,讲的是天色。\n\n第二段文字内容也足够长,讲的是风。\n\n第三段文字内容同样很长,讲的是门。\n";

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }
    fn ids(d: &Doc) -> Vec<String> { d.blocks_message()["blocks"].as_array().unwrap().iter().map(|b| b["id"].as_str().unwrap().to_string()).collect() }
    fn rep(id: &str, text: &str, expect: Option<String>) -> Step { Step { op: AgentOp::ReplaceBlock { id: id.into(), text: text.into() }, expect } }
    fn restart(c: &Connection, d: &Doc) -> Doc { load(c, 1, &d.text).unwrap().unwrap() }

    fn fresh() -> (Connection, Doc, Vec<String>) {
        let c = db();
        let mut d = Doc::new();
        d.load(CH);
        let v = ids(&d);
        (c, d, v)
    }

    #[test]
    fn ids_text_and_counters_survive_a_restart() {
        let (c, mut d, v) = fresh();
        save(&c, 1, &mut d).unwrap();
        let d2 = restart(&c, &d);
        assert_eq!(ids(&d2), v);
        assert_eq!((d2.text.as_str(), d2.version), (d.text.as_str(), d.version));
    }

    #[test]
    fn forwarding_survives_a_restart() {
        let (c, mut d, v) = fresh();
        let seen = fnv(&d.block_text(&v[1]).unwrap());
        d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", Some(seen.clone()))]).unwrap();
        assert_ne!(ids(&d)[1], v[1], "a wholesale rewrite gives the block a new id");
        save(&c, 1, &mut d).unwrap();
        let mut d2 = restart(&c, &d);
        // the old id still reaches the rewritten block; the Agent's old hash is stale, not "no such block"
        let r = d2.agent_plan(&[rep(&v[1], "x", Some(seen))]);
        assert!(matches!(r, Err((0, AgentError::Stale { .. }))), "{r:?}");
        let cur = fnv(&d2.block_text(&v[1]).unwrap());
        assert!(d2.agent_plan(&[rep(&v[1], "海的另一边。", Some(cur))]).is_ok());
        // and without forwarding it would have been refused
        let mut lost = Doc::import_state(&crate::sync::State { aliases: vec![], ..d.clone().export_state() }, &d.text);
        assert!(matches!(lost.agent_plan(&[rep(&v[1], "x", None)]), Err((0, AgentError::NoBlock(_)))));
    }

    #[test]
    fn a_run_from_before_the_restart_is_never_taken_for_the_new_run() {
        let (c, mut d, v) = fresh();
        let seen = fnv(&d.block_text(&v[1]).unwrap());
        d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", None)]).unwrap();
        save(&c, 1, &mut d).unwrap();
        let mut d2 = restart(&c, &d);
        // if the counter restarted at 0 the first new run would equal the run that made the forwarding and skip the check
        let r = d2.agent_plan(&[rep(&v[1], "绕过检查?", Some(seen))]);
        assert!(matches!(r, Err((0, AgentError::Stale { .. }))), "{r:?}");
    }

    #[test]
    fn an_edit_made_while_nothing_was_running_keeps_the_ids() {
        let (c, mut d, v) = fresh();
        save(&c, 1, &mut d).unwrap();
        let edited = d.text.replace("讲的是风", "讲的是大风");
        let d2 = load(&c, 1, &edited).unwrap().unwrap();
        assert_eq!(ids(&d2), v);
        assert_eq!(d2.text, edited);
        assert_eq!(d2.version, d.version + 1);
    }

    #[test]
    fn forwarding_to_a_deleted_block_is_dropped_on_save_without_changing_any_answer() {
        let (c, mut d, v) = fresh();
        d.agent_plan(&[rep(&v[2], "完全不同的第二段,写的是山。", None)]).unwrap();
        let new_id = ids(&d)[2].clone();
        // the author deletes the rewritten block
        let (f, t) = { let m = d.blocks_message(); let b = &m["blocks"][2]; (b["from"].as_u64().unwrap() as usize, b["to"].as_u64().unwrap() as usize + 2) };
        d.edit(d.version, &[(f, t, String::new())]).unwrap();
        assert!(!ids(&d).contains(&new_id));
        assert_eq!(d.clone().export_state().aliases.len(), 0, "dead forwarding is not kept");
        let before = d.agent_plan(&[rep(&v[2], "x", None)]).unwrap_err().1;
        save(&c, 1, &mut d).unwrap();
        let mut d2 = restart(&c, &d);
        assert_eq!(d2.agent_plan(&[rep(&v[2], "x", None)]).unwrap_err().1, before);
    }

    #[test]
    fn the_history_says_which_runs_wrote_a_block() {
        let (c, mut d, v) = fresh();
        d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", None)]).unwrap(); // run 1
        save(&c, 1, &mut d).unwrap();
        let mut d2 = restart(&c, &d);
        let cur = ids(&d2)[1].clone();
        d2.agent_plan(&[rep(&cur, "又改了第一段,写的是山。", None), Step { op: AgentOp::Append { text: "结尾段。".into() }, expect: None }]).unwrap(); // run 2
        save(&c, 1, &mut d2).unwrap();
        let ids2 = ids(&d2);
        let last: u64 = ids2.last().unwrap().parse().unwrap();
        assert_eq!(agent_history(&c, 1, last).unwrap().iter().map(|x| x.0).collect::<Vec<_>>(), vec![2]);
        let mid: u64 = ids2[1].parse().unwrap();
        let h = agent_history(&c, 1, mid).unwrap();
        assert!(h.iter().all(|(run, _)| *run == 2) && !h.is_empty(), "{h:?}");
        // the untouched heading was never written by an Agent
        assert!(agent_history(&c, 1, ids2[0].parse().unwrap()).unwrap().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_state_and_keeps_the_unsaved_history() {
        let (c, mut d, v) = fresh();
        save(&c, 1, &mut d).unwrap();
        let ids_before = ids(&d);
        d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", None)]).unwrap();
        c.execute_batch("CREATE TRIGGER boom BEFORE INSERT ON block_forward BEGIN SELECT RAISE(ABORT, 'disk full'); END;").unwrap();
        assert!(save(&c, 1, &mut d).is_err());
        c.execute_batch("DROP TRIGGER boom;").unwrap();
        let old = load(&c, 1, CH).unwrap().unwrap();
        assert_eq!(ids(&old), ids_before, "all of the previous state, none of the failed one");
        save(&c, 1, &mut d).unwrap(); // retry works and still writes the history
        let n: i64 = c.query_row("SELECT count(*) FROM block_agent_history", [], |r| r.get(0)).unwrap();
        assert!(n >= 1);
    }

    #[test]
    fn restarting_at_random_points_changes_nothing_an_uninterrupted_session_would_do() {
        struct R(u64);
        impl R { fn n(&mut self) -> usize { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as usize } }
        let (mut compared, mut restarts) = (0, 0);
        for seed in 1..=300u64 {
            let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let c = db();
            let text: String = (0..6).map(|i| format!("第{i}段,内容足够长,讲的是故事{i}里发生的事情。\n\n")).collect();
            let mut live = Doc::new(); // restarts
            live.load(&text);
            let mut shadow = live.clone(); // never restarts
            let known: Vec<String> = ids(&live);
            for step in 0..25 {
                match r.n() % 5 {
                    0 | 1 => { // an Agent plan on ids the Agent read at the start: same outcome with or without restarts
                        let id = known[r.n() % known.len()].clone();
                        let cur = shadow.block_text(&id).map(|t| fnv(&t));
                        let expect = if r.n() % 2 == 0 { cur } else { None };
                        let plan = [rep(&id, &format!("改写{seed}-{step},内容也足够长。"), expect)];
                        let (a, b) = (live.agent_plan(&plan), shadow.agent_plan(&plan));
                        assert_eq!(a.as_ref().map(|_| ()).map_err(|e| e.clone()), b.as_ref().map(|_| ()).map_err(|e| e.clone()), "seed {seed} step {step}");
                        compared += 1;
                    }
                    2 => { // the author types
                        let m = shadow.blocks_message();
                        let bl = m["blocks"].as_array().unwrap();
                        let at = bl[r.n() % bl.len()]["from"].as_u64().unwrap() as usize + 1;
                        live.edit(live.version, &[(at, at, "字".into())]).unwrap();
                        shadow.edit(shadow.version, &[(at, at, "字".into())]).unwrap();
                    }
                    3 => { save(&c, 1, &mut live).unwrap(); }
                    _ => { // restart: save, then reload from the database and the current text
                        save(&c, 1, &mut live).unwrap();
                        live = load(&c, 1, &live.text).unwrap().unwrap();
                        restarts += 1;
                    }
                }
                assert_eq!(live.text, shadow.text);
                assert_eq!(ids(&live), ids(&shadow), "seed {seed} step {step}");
            }
        }
        eprintln!("{compared} plans compared across {restarts} restarts: identical outcomes and ids");
        assert!(compared > 1000 && restarts > 1000);
    }
}
