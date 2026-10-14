//! The Rust side of the block range sync: Rust owns the text. The editor sends text patches in UTF-16 positions;
//! Rust applies them, re-parses, keeps block ids stable with the reconciler, and answers with the block list.

use block_identity_spike::parse::{Kind, Parsed, parse_blocks};
use block_identity_spike::reconcile::{Config, Tracked, reconcile};
use serde_json::{Value, json};

pub enum AgentOp {
    ReplaceBlock { id: String, text: String },
    InsertAfter { id: String, text: String },
    Append { text: String },
}

/// Why an Agent step was not applied. Every one of these is a normal outcome the runtime reports to the Agent.
#[derive(Debug, PartialEq, Clone)]
pub enum AgentError {
    /// the block is gone (deleted, or merged into another) and no forwarding exists
    NoBlock(String),
    /// the block exists but its text is no longer what the Agent read: the author changed it. Carries the current text.
    Stale { id: String, current: String },
    Edit(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::NoBlock(id) => write!(f, "no block with id {id}"),
            AgentError::Stale { id, .. } => write!(f, "block {id} changed since the Agent read it"),
            AgentError::Edit(e) => write!(f, "{e}"),
        }
    }
}

/// One step of an Agent plan. `expect` is the hash (`fnv`) of the block text the Agent based the step on.
pub struct Step {
    pub op: AgentOp,
    pub expect: Option<String>,
}

#[derive(Clone)]
pub struct Doc {
    pub text: String,
    pub version: u64,
    tracked: Vec<Tracked>,
    next_id: u64,
    last: Option<(Vec<Parsed>, Vec<(u64, &'static str)>)>,
    /// old block id -> (id of the block that replaced it, plan run that did it): a rewrite gives the block a new id
    aliases: std::collections::HashMap<u64, (u64, u64)>,
    runs: u64,
    /// re-parse only the blocks around an edit (falls back to the whole document whenever it cannot prove the result equal)
    pub incremental: bool,
    pub stats: ReparseStats,
    /// (block id, run, version) of every block an Agent step wrote, not yet saved (block_agent_history)
    history: Vec<(u64, u64, u64)>,
}

/// Everything about block identity that cannot be recovered from the text (ADR-012): saved with the text in one
/// transaction so the two never disagree.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub version: u64,
    pub text: String,
    /// block ids in document order, for the blocks of `text`
    pub ids: Vec<u64>,
    pub next_id: u64,
    pub runs: u64,
    /// (old id, new id, run)
    pub aliases: Vec<(u64, u64, u64)>,
    /// rows for block_agent_history that were not saved yet
    pub history: Vec<(u64, u64, u64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReparseStats {
    pub incremental: u64,
    pub full: u64,
    /// total bytes handed to the parser
    pub parsed_bytes: u64,
}

#[derive(Debug, PartialEq)]
pub enum EditError {
    StaleBase { have: u64, got: u64 },
    BadPosition(usize),
    Malformed(&'static str),
}

/// UTF-16 position → byte offset (None if it falls inside a surrogate pair or past the end).
pub fn byte_offset(text: &str, utf16: usize) -> Option<usize> {
    let mut u = 0;
    for (i, c) in text.char_indices() {
        if u == utf16 {
            return Some(i);
        }
        u += c.len_utf16();
        if u > utf16 {
            return None;
        }
    }
    (u == utf16).then_some(text.len())
}

/// FNV-1a 64 over the UTF-8 bytes, as 16 hex digits (the page computes the same to check both sides hold the same text).
pub fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn how_name(h: block_identity_spike::reconcile::How) -> &'static str {
    use block_identity_spike::reconcile::How::*;
    match h { Exact => "Exact", Moved => "Moved", Similar => "Similar", Split => "Split", Merged => "Merged", Fresh => "Fresh" }
}

fn kind_name(k: &Kind) -> (&'static str, Option<u8>) {
    match k {
        Kind::Heading(l) => ("heading", Some(*l)),
        Kind::Paragraph => ("paragraph", None),
        Kind::List => ("list", None),
        Kind::Quote => ("quote", None),
        Kind::Code => ("code", None),
        Kind::Table => ("table", None),
        Kind::Rule => ("rule", None),
        Kind::Html => ("html", None),
        Kind::Footnote => ("footnote", None),
        Kind::Other => ("other", None),
    }
}

impl Doc {
    pub fn new() -> Doc {
        Doc { text: String::new(), version: 0, tracked: vec![], next_id: 1, last: None, aliases: Default::default(), runs: 0, incremental: true, stats: ReparseStats::default(), history: vec![] }
    }

    /// A new document: every block gets a fresh id.
    pub fn load(&mut self, text: &str) {
        self.text = text.to_string();
        self.tracked.clear();
        self.version += 1;
        self.reparse();
    }

    /// Apply patches given in the coordinates of the document the editor had at `base` (UTF-16, sorted, disjoint).
    pub fn edit(&mut self, base: u64, changes: &[(usize, usize, String)]) -> Result<(), EditError> {
        if base != self.version {
            return Err(EditError::StaleBase { have: self.version, got: base });
        }
        let mut spans = Vec::with_capacity(changes.len());
        let mut prev_end = 0;
        for (from, to, _) in changes {
            if *from < prev_end || to < from {
                return Err(EditError::Malformed("changes must be sorted and disjoint"));
            }
            prev_end = *to;
            let f = byte_offset(&self.text, *from).ok_or(EditError::BadPosition(*from))?;
            let t = byte_offset(&self.text, *to).ok_or(EditError::BadPosition(*to))?;
            spans.push((f, t));
        }
        let old_len = self.text.len();
        for ((f, t), (_, _, ins)) in spans.iter().zip(changes).rev() {
            self.text.replace_range(*f..*t, ins);
        }
        self.version += 1;
        let delta: isize = self.text.len() as isize - old_len as isize;
        let region = (spans.first().map(|x| x.0).unwrap_or(0), spans.last().map(|x| x.1).unwrap_or(0));
        if !(self.incremental && self.reparse_window(region, delta)) {
            self.reparse();
        }
        Ok(())
    }

    /// The state to persist together with the text. Forwarding whose target is gone is dropped: it could only ever
    /// have answered "no such block".
    pub fn export_state(&mut self) -> State {
        let ids: Vec<u64> = self.last.as_ref().map(|l| l.1.iter().map(|(i, _)| *i).collect()).unwrap_or_default();
        let live: std::collections::HashSet<u64> = ids.iter().copied().collect();
        let mut keep: Vec<u64> = vec![];
        for &old in self.aliases.keys() {
            let mut cur = old;
            for _ in 0..64 {
                if live.contains(&cur) { keep.push(old); break; }
                match self.aliases.get(&cur) { Some((n, _)) => cur = *n, None => break }
            }
        }
        self.aliases.retain(|k, _| keep.contains(k));
        let mut aliases: Vec<(u64, u64, u64)> = self.aliases.iter().map(|(o, (n, r))| (*o, *n, *r)).collect();
        aliases.sort_unstable();
        State { version: self.version, text: self.text.clone(), ids, next_id: self.next_id, runs: self.runs, aliases, history: std::mem::take(&mut self.history) }
    }

    /// Put history rows back after a save that did not happen.
    pub fn restore_history(&mut self, mut rows: Vec<(u64, u64, u64)>) {
        rows.append(&mut self.history);
        self.history = rows;
    }

    /// Rebuild a Doc from saved state and the text as it is now. If the text is what was saved, every id is
    /// restored verbatim; if it changed while nothing was running (an external edit), the saved text and ids are the
    /// reconciler's "old" side, so unchanged blocks keep their ids.
    pub fn import_state(st: &State, text: &str) -> Doc {
        let mut d = Doc::new();
        d.next_id = st.next_id;
        d.runs = st.runs;
        d.version = st.version;
        d.aliases = st.aliases.iter().map(|(o, n, r)| (*o, (*n, *r))).collect();
        let saved = parse_blocks(&st.text);
        if saved.len() != st.ids.len() {
            d.load(text); // inconsistent state: nothing to carry over except the counters
            return d;
        }
        d.tracked = saved.iter().zip(&st.ids).map(|(p, id)| Tracked { id: *id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        d.text = st.text.clone();
        if text == st.text {
            d.last = Some((saved, st.ids.iter().map(|i| (*i, "Exact")).collect()));
        } else {
            d.text = text.to_string();
            d.version += 1;
            d.reparse();
        }
        d
    }

    /// Resolve a block id the Agent holds to a block of the CURRENT version: the id itself, or the block that replaced
    /// it (followed through forwarding entries). Returns the block index and whether every hop was made by `run`.
    fn resolve(&self, id: &str, run: u64) -> Result<(usize, bool), AgentError> {
        let ids: Vec<u64> = self.last.as_ref().ok_or_else(|| AgentError::Edit("no document".into()))?.1.iter().map(|(i, _)| *i).collect();
        let mut cur: u64 = id.parse().map_err(|_| AgentError::NoBlock(id.into()))?;
        let (mut own, mut hops) = (true, 0);
        for _ in 0..64 {
            if let Some(i) = ids.iter().position(|x| *x == cur) { return Ok((i, own && hops > 0)); }
            hops += 1;
            let (next, by) = *self.aliases.get(&cur).ok_or_else(|| AgentError::NoBlock(id.into()))?;
            own &= by == run;
            cur = next;
        }
        Err(AgentError::NoBlock(id.into()))
    }

    pub fn block_text(&self, id: &str) -> Option<String> {
        let (i, _) = self.resolve(id, u64::MAX).ok()?;
        Some(self.last.as_ref()?.0[i].text.clone())
    }

    /// An Agent operation addressed by block id, without a precondition (kept for the manual test page).
    pub fn agent(&mut self, op: &AgentOp) -> Result<Vec<(usize, usize, String)>, String> {
        self.runs += 1;
        let run = self.runs;
        self.agent_step(op, None, run).map_err(|e| e.to_string())
    }

    /// One step, resolved against the current version at the moment it runs. `replace_block` also needs the block to
    /// still hold the text the Agent read (`expect`), unless the block is the Agent's own earlier rewrite in this run;
    /// otherwise the author's change would be overwritten. `insert_after` and `append` depend only on position.
    pub fn agent_step(&mut self, op: &AgentOp, expect: Option<&str>, run: u64) -> Result<Vec<(usize, usize, String)>, AgentError> {
        let ranges = self.ranges16();
        let end16 = self.text.encode_utf16().count();
        let mut alias_from: Option<(u64, usize)> = None; // (old id, byte start of the block) for the forwarding entry
        let patch = match op {
            AgentOp::ReplaceBlock { id, text } => {
                let (i, own) = self.resolve(id, run)?;
                let (parsed, ids) = self.last.as_ref().unwrap();
                if let Some(want) = expect {
                    if !own && fnv(&parsed[i].text) != want {
                        return Err(AgentError::Stale { id: id.clone(), current: parsed[i].text.clone() });
                    }
                }
                alias_from = Some((ids[i].0, parsed[i].start));
                let (from, to) = ranges[i];
                vec![(from, to, text.clone())]
            }
            AgentOp::InsertAfter { id, text } => {
                let (i, _) = self.resolve(id, run)?;
                let to = ranges[i].1;
                vec![(to, to, format!("\n\n{text}"))]
            }
            AgentOp::Append { text } => vec![(end16, end16, format!("{}{text}\n", if self.text.ends_with("\n\n") { "" } else if self.text.ends_with('\n') { "\n" } else { "\n\n" }))],
        };
        let written: Vec<(usize, usize)> = patch.iter().filter(|(_, _, ins)| !ins.is_empty()).filter_map(|(f, _, ins)| byte_offset(&self.text, *f).map(|b| (b, b + ins.len()))).collect();
        self.edit(self.version, &patch).map_err(|e| AgentError::Edit(format!("{e:?}")))?;
        for (a, b) in written {
            // the blocks that start inside the text the Agent wrote
            let (parsed, ids) = self.last.as_ref().unwrap();
            let v = self.version;
            let new: Vec<(u64, u64, u64)> = parsed.iter().zip(ids).filter(|(p, _)| p.start >= a && p.start < b).map(|(_, (id, _))| (*id, run, v)).collect();
            self.history.extend(new);
        }
        if let Some((old, start)) = alias_from {
            // the text before the block is unchanged, so the rewrite begins at the same byte offset
            let (parsed, ids) = self.last.as_ref().unwrap();
            if let Some(j) = parsed.iter().position(|p| p.start == start) {
                if ids[j].0 != old { self.aliases.insert(old, (ids[j].0, run)); }
            }
        }
        Ok(patch)
    }

    /// A plan is all or nothing: if a step cannot be resolved or is stale, the document is left exactly as it was.
    /// Each step is resolved when it runs, so a step may name a block an earlier step of the same plan rewrote.
    /// Returns one patch per step; each is in the coordinates of the version before it.
    pub fn agent_plan(&mut self, steps: &[Step]) -> Result<Vec<Vec<(usize, usize, String)>>, (usize, AgentError)> {
        let saved = self.clone();
        self.runs += 1;
        let run = self.runs;
        let mut patches = vec![];
        for (n, st) in steps.iter().enumerate() {
            match self.agent_step(&st.op, st.expect.as_deref(), run) {
                Ok(p) => patches.push(p),
                Err(e) => { *self = saved; return Err((n, e)); }
            }
        }
        Ok(patches)
    }

    /// The `update` message for the editor after an Agent patch: the patch, and the resulting version and blocks.
    pub fn update_message(&self, patch: &[(usize, usize, String)], origin: &str) -> Value {
        let mut m = self.blocks_message();
        m["t"] = json!("update");
        m["origin"] = json!(origin);
        m["changes"] = patch.iter().map(|(f, t, i)| json!({"from": f, "to": t, "insert": i})).collect();
        m
    }

    /// Re-parse only the blocks around the edit: the edited blocks plus one neighbour on each side, taken as a
    /// window of text. The result is used only if the window parses back to exactly the same neighbours (same
    /// kind, text and position) — then nothing outside it can have changed — and is spliced into the old block list.
    /// `region` is the edited byte range in the OLD text, `delta` the change in length. Returns false to fall back.
    fn reparse_window(&mut self, region: (usize, usize), delta: isize) -> bool {
        let Some((old, _)) = self.last.as_ref() else { return false };
        let n = old.len();
        if n < 8 { return false; }
        // first block that ends at or after the edit, last block that starts at or before its end
        let first = old.partition_point(|p| p.end < region.0);
        let last = old.partition_point(|p| p.start <= region.1).saturating_sub(1);
        // the neighbours must come back unchanged, at the same place; typing right after a paragraph can turn it into a
        // continuation of that paragraph, so a failed check retries with a wider window before giving up
        let old_len = (self.text.len() as isize - delta) as usize;
        let mut found = None;
        for pad in [1usize, 2, 4] {
            let i = first.saturating_sub(pad);
            let j = (last.max(first) + pad).min(n - 1);
            if j - i > 64 { return false; } // a big edit: the window would be most of the document anyway
            let ws = if i == 0 { 0 } else { old[i].start };
            let we_old = if j == n - 1 { old_len } else { old[j].end };
            let we = (we_old as isize + delta) as usize;
            if we < ws || we > self.text.len() || !self.text.is_char_boundary(ws) || !self.text.is_char_boundary(we) { return false; }
            let mut win = parse_blocks(&self.text[ws..we]);
            self.stats.parsed_bytes += (we - ws) as u64;
            for p in &mut win { p.start += ws; p.end += ws; }
            if i > 0 {
                let (a, b) = (&old[i], win.first());
                if !b.is_some_and(|b| b.kind == a.kind && b.text == a.text && b.start == a.start) { continue; }
            }
            if j < n - 1 {
                let (a, b) = (&old[j], win.last());
                if !b.is_some_and(|b| b.kind == a.kind && b.text == a.text && b.end as isize == a.end as isize + delta) { continue; }
            }
            found = Some((i, j, win));
            break;
        }
        let Some((i, j, win)) = found else { return false };
        self.stats.incremental += 1;
        let (mut parsed, mut hows) = self.last.take().unwrap();
        let mut next = self.next_id;
        let assigned = reconcile(&self.tracked[i..=j], &win, &Config::default(), || { next += 1; next - 1 });
        self.next_id = next;
        // splice in place: only the window is replaced, the rest keeps its strings (offsets after it just shift)
        let new_tracked: Vec<Tracked> = win.iter().zip(&assigned).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        let new_hows: Vec<(u64, &'static str)> = assigned.iter().map(|a| (a.id, how_name(a.how))).collect();
        let added = win.len();
        parsed.splice(i..=j, win);
        for p in &mut parsed[i + added..] {
            p.start = (p.start as isize + delta) as usize;
            p.end = (p.end as isize + delta) as usize;
        }
        for h in &mut hows[j + 1..] { h.1 = "Exact"; }
        hows.splice(i..=j, new_hows);
        self.tracked.splice(i..=j, new_tracked);
        let new_hows = hows;
        self.last = Some((parsed, new_hows));
        true
    }

    fn reparse(&mut self) {
        self.stats.full += 1;
        self.stats.parsed_bytes += self.text.len() as u64;
        let parsed = parse_blocks(&self.text);
        let mut next = self.next_id;
        let assigned = reconcile(&self.tracked, &parsed, &Config::default(), || { next += 1; next - 1 });
        self.next_id = next;
        self.tracked = parsed.iter().zip(&assigned).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        self.last = Some((parsed, assigned.iter().map(|a| (a.id, how_name(a.how))).collect()));
    }

    /// (from, to) of every block in UTF-16 positions, in document order.
    fn ranges16(&self) -> Vec<(usize, usize)> {
        let (parsed, _) = self.last.as_ref().expect("a document was loaded");
        let mut offsets: Vec<usize> = parsed.iter().flat_map(|p| [p.start, p.end]).collect();
        offsets.sort_unstable();
        offsets.dedup();
        let mut pos16 = std::collections::HashMap::with_capacity(offsets.len());
        let (mut u, mut k) = (0usize, 0usize);
        for (i, c) in self.text.char_indices().chain(std::iter::once((self.text.len(), '\0'))) {
            while k < offsets.len() && offsets[k] <= i {
                pos16.insert(offsets[k], u);
                k += 1;
            }
            u += c.len_utf16();
        }
        parsed.iter().map(|p| (pos16[&p.start], pos16[&p.end])).collect()
    }

    /// The `blocks` message for the editor: ranges in UTF-16 positions.
    pub fn blocks_message(&self) -> Value {
        let (parsed, hows) = self.last.as_ref().expect("a document was loaded");
        let ranges = self.ranges16();
        let blocks: Vec<Value> = parsed
            .iter()
            .zip(hows)
            .zip(&ranges)
            .map(|((p, (id, how)), (from, to))| {
                let (kind, level) = kind_name(&p.kind);
                let title: Option<String> = level.map(|_| heading_title(&p.text));
                json!({"id": id.to_string(), "kind": kind, "level": level, "title": title, "how": how,
                       "from": from, "to": to, "head": p.text.chars().take(16).collect::<String>()})
            })
            .collect();
        json!({"t": "blocks", "version": self.version, "hash": fnv(&self.text), "len16": self.text.encode_utf16().count(), "blocks": blocks})
    }
}

/// The visible text of a heading (markers and setext underline removed).
fn heading_title(src: &str) -> String {
    let first = src.lines().next().unwrap_or("");
    first.trim_start_matches('#').trim().trim_end_matches('#').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(d: &Doc) -> Vec<String> {
        d.blocks_message()["blocks"].as_array().unwrap().iter().map(|b| b["id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn utf16_positions_and_surrogate_pairs() {
        let t = "a字😀b";
        assert_eq!(byte_offset(t, 0), Some(0));
        assert_eq!(byte_offset(t, 1), Some(1));
        assert_eq!(byte_offset(t, 2), Some(4));
        assert_eq!(byte_offset(t, 3), None, "inside the surrogate pair of 😀");
        assert_eq!(byte_offset(t, 4), Some(8));
        assert_eq!(byte_offset(t, 5), Some(9));
        assert_eq!(byte_offset(t, 6), None);
    }

    #[test]
    fn block_ranges_are_in_utf16_units() {
        let mut d = Doc::new();
        d.load("😀 开头\n\n- 项\n\n第三段\n");
        let m = d.blocks_message();
        let b = m["blocks"].as_array().unwrap();
        assert_eq!((b[0]["from"].as_u64(), b[0]["to"].as_u64()), (Some(0), Some(5)), "😀 counts as two units");
        assert_eq!(b[1]["from"].as_u64(), Some(7));
        assert_eq!(b[2]["head"], "第三段");
        let units: Vec<u16> = d.text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&units[b[2]["from"].as_u64().unwrap() as usize..b[2]["to"].as_u64().unwrap() as usize]).unwrap(), "第三段");
    }

    #[test]
    fn edits_keep_ids_and_reject_stale_bases() {
        let mut d = Doc::new();
        d.load("# 标题\n\n第一段文字内容足够长。\n\n第二段文字内容也足够长。\n");
        let before = ids(&d);
        d.edit(1, &[(10, 10, "新增".into())]).unwrap();
        assert_eq!(ids(&d), before);
        assert_eq!(d.version, 2);
        assert_eq!(d.edit(1, &[(0, 0, "x".into())]), Err(EditError::StaleBase { have: 2, got: 1 }));
        // two disjoint changes in one patch, in the coordinates of the base document
        d.edit(2, &[(0, 0, "# ".into()), (d.text.encode_utf16().count(), d.text.encode_utf16().count(), "尾".into())]).unwrap();
        assert!(d.text.starts_with("# # 标题"));
        assert!(d.text.ends_with("尾"));
        assert_eq!(d.edit(3, &[(5, 3, String::new())]), Err(EditError::Malformed("changes must be sorted and disjoint")));
    }

    #[test]
    fn agent_operations_address_blocks_by_id_and_report_their_patch() {
        let mut d = Doc::new();
        d.load("# 题\n\n甲段落内容在这里。\n\n乙段落内容也在这里。\n");
        let m = d.blocks_message();
        let id_a = m["blocks"][1]["id"].as_str().unwrap().to_string();
        let id_b = m["blocks"][2]["id"].as_str().unwrap().to_string();
        let patch = d.agent(&AgentOp::ReplaceBlock { id: id_a.clone(), text: "改写后的甲段落😀。".into() }).unwrap();
        assert_eq!(d.version, 2);
        assert_eq!(patch.len(), 1);
        assert!(d.text.contains("改写后的甲段落😀。") && d.text.contains("乙段落"));
        // ids of the untouched blocks are unchanged
        let m2 = d.blocks_message();
        assert_eq!(m2["blocks"][2]["id"], id_b.as_str());
        let up = d.update_message(&patch, "agent");
        assert_eq!((up["t"].as_str(), up["version"].as_u64(), up["changes"].as_array().map(|c| c.len())), (Some("update"), Some(2), Some(1)));
        d.agent(&AgentOp::InsertAfter { id: id_b.clone(), text: "新增的一段。".into() }).unwrap();
        d.agent(&AgentOp::Append { text: "结尾。".into() }).unwrap();
        assert!(d.text.ends_with("新增的一段。\n\n结尾。\n"), "{:?}", d.text);
        assert!(d.agent(&AgentOp::ReplaceBlock { id: "nope".into(), text: "x".into() }).is_err());
        assert_eq!(d.version, 4, "a failed operation does not advance the version");
    }

    // ---- the Agent runtime resolves ids at execution time -------------------------------------------------------

    const CH: &str = "# 标题\n\n第一段文字内容足够长,讲的是天色。\n\n第二段文字内容也足够长,讲的是风。\n\n第三段文字内容同样很长,讲的是门。\n";

    fn loaded() -> (Doc, Vec<String>) {
        let mut d = Doc::new();
        d.load(CH);
        let v = ids(&d);
        (d, v)
    }
    fn rep(id: &str, text: &str, expect: Option<String>) -> Step { Step { op: AgentOp::ReplaceBlock { id: id.into(), text: text.into() }, expect } }

    #[test]
    fn a_second_step_may_name_a_block_the_first_step_rewrote() {
        let (mut d, v) = loaded();
        let seen = fnv(&d.block_text(&v[1]).unwrap());
        // a wholesale rewrite gives the block a new id; the plan still holds the old one
        let r = d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", Some(seen.clone())), rep(&v[1], "再改一次第一段,写的是山。", Some(seen))]);
        assert!(r.is_ok(), "{r:?}");
        assert!(d.text.contains("再改一次第一段,写的是山。") && !d.text.contains("写的是海"));
        assert_eq!(d.text.matches("写的是山").count(), 1);
    }

    #[test]
    fn a_block_the_author_changed_is_not_overwritten() {
        let (mut d, v) = loaded();
        let seen = fnv(&d.block_text(&v[2]).unwrap());
        d.edit(d.version, &[(4 + 2 + 5, 4 + 2 + 5, "(作者加的)".into())]).ok(); // some edit in block 1
        let i = d.text.find("第二段").unwrap();
        let at = d.text[..i].encode_utf16().count() + 3;
        d.edit(d.version, &[(at, at, "【作者】".into())]).unwrap();
        let before = (d.text.clone(), d.version);
        let r = d.agent_plan(&[rep(&v[2], "Agent 的改写。", Some(seen.clone()))]);
        match r {
            Err((0, AgentError::Stale { current, .. })) => assert!(current.contains("【作者】")),
            other => panic!("{other:?}"),
        }
        assert_eq!((d.text.clone(), d.version), before, "nothing changed");
        // an explicit step without a precondition is allowed (the caller chose to overwrite)
        assert!(d.agent_plan(&[rep(&v[2], "Agent 的改写。", None)]).is_ok());
    }

    #[test]
    fn a_failed_step_rolls_the_whole_plan_back() {
        let (mut d, v) = loaded();
        let before = (d.text.clone(), d.version);
        let r = d.agent_plan(&[rep(&v[1], "新的第一段内容,足够长。", None), rep("9999", "x", None)]);
        assert_eq!(r, Err((1, AgentError::NoBlock("9999".into()))));
        assert_eq!((d.text.clone(), d.version), before);
        // the ids still resolve after the rollback, and the forwarding made by the rolled-back step is gone
        assert!(d.agent_plan(&[rep(&v[1], "新的第一段内容,足够长。", None)]).is_ok());
    }

    #[test]
    fn a_deleted_block_is_a_normal_outcome() {
        let (mut d, v) = loaded();
        let i = d.text.find("第二段").unwrap();
        let j = d.text.find("第三段").unwrap();
        let (f, t) = (d.text[..i].encode_utf16().count(), d.text[..j].encode_utf16().count());
        d.edit(d.version, &[(f, t, String::new())]).unwrap();
        assert_eq!(d.agent_plan(&[Step { op: AgentOp::InsertAfter { id: v[2].clone(), text: "x".into() }, expect: None }]).unwrap_err().1, AgentError::NoBlock(v[2].clone()));
    }

    #[test]
    fn forwarding_from_an_earlier_run_does_not_bypass_the_precondition() {
        let (mut d, v) = loaded();
        let seen = fnv(&d.block_text(&v[1]).unwrap());
        d.agent_plan(&[rep(&v[1], "完全不同的第一段,写的是海。", None)]).unwrap();
        // a later run still holding the old id and the old text hash: the block is someone else's rewrite now
        let r = d.agent_plan(&[rep(&v[1], "又一次改写。", Some(seen))]);
        assert!(matches!(r, Err((0, AgentError::Stale { .. }))), "{r:?}");
    }

    struct R(u64);
    impl R {
        fn next(&mut self) -> usize { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as usize }
    }

    #[test]
    fn the_author_and_the_agent_race_for_many_documents_and_nothing_is_overwritten() {
        let (mut ok, mut stale, mut gone) = (0, 0, 0);
        for seed in 1..=1500u64 {
            let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let n = 3 + r.next() % 6;
            let text: String = (0..n).map(|i| format!("第{i}段,内容足够长,讲的是故事{i}里发生的事情。\n\n")).collect();
            let mut d = Doc::new();
            d.load(&text);
            let v = ids(&d);
            // what the Agent read
            let snap: Vec<(String, String)> = v.iter().map(|id| (id.clone(), d.block_text(id).unwrap())).collect();
            // the author keeps working: edits, deletions, new paragraphs
            let mut authored = vec![];
            for k in 0..(r.next() % 4) {
                let mark = format!("【作者{seed}-{k}】");
                let blocks = d.blocks_message()["blocks"].as_array().unwrap().clone();
                let b = &blocks[r.next() % blocks.len()];
                let (from, to) = (b["from"].as_u64().unwrap() as usize, b["to"].as_u64().unwrap() as usize);
                match r.next() % 3 {
                    0 => { d.edit(d.version, &[(from + 2, from + 2, mark.clone())]).unwrap(); authored.push(mark); }
                    1 if blocks.len() > 2 => { d.edit(d.version, &[(from, (to + 2).min(d.text.encode_utf16().count()), String::new())]).unwrap(); }
                    _ => { d.edit(d.version, &[(to, to, format!("\n\n{mark}。"))]).unwrap(); authored.push(mark); }
                }
            }
            // the Agent plan: rewrites of 1-3 blocks it read, with preconditions
            let k = 1 + r.next() % 3;
            let mut steps = vec![];
            let mut targets = vec![];
            let mut used = vec![];
            for j in 0..k {
                let at = r.next() % snap.len();
                if used.contains(&at) { continue; }
                used.push(at);
                let (id, t) = &snap[at];
                targets.push((id.clone(), t.clone()));
                steps.push(rep(id, &format!("Agent改写{seed}-{j}。"), Some(fnv(t))));
            }
            let before = d.text.clone();
            match d.agent_plan(&steps) {
                Ok(_) => {
                    ok += 1;
                    // every author mark that was present survives, and every rewrite is there
                    for m in &authored { assert_eq!(d.text.contains(m.as_str()), before.contains(m.as_str()), "seed {seed}: an author edit was overwritten"); }
                    for st in &steps { if let AgentOp::ReplaceBlock { text, .. } = &st.op { assert!(d.text.contains(text.as_str()), "seed {seed}: {text} missing"); } }
                    // an Ok plan only ever replaced text that was unchanged since the Agent read it (or its own rewrite)
                    for (_, t) in &targets { assert!(before.contains(t.as_str()) || steps.len() > 1, "seed {seed}"); }
                }
                Err((_, AgentError::Stale { .. })) => { stale += 1; assert_eq!(d.text, before, "seed {seed}"); }
                Err((_, AgentError::NoBlock(_))) => { gone += 1; assert_eq!(d.text, before, "seed {seed}"); }
                Err((_, e)) => panic!("seed {seed}: {e:?}"),
            }
        }
        eprintln!("1500 races: {ok} plans applied, {stale} refused as stale, {gone} refused because the block was gone");
        assert!(ok > 200 && stale > 50 && gone > 50);
    }

    // ---- incremental re-parse must be indistinguishable from a full re-parse ------------------------------------

    /// Blocks must be identical. Ids may differ only where the full reconciler reused the id of a far-away look-alike
    /// (a template twin), which the window cannot see; ids must stay unique either way. Returns whether ids differed.
    fn same(a: &Doc, b: &Doc) -> Result<bool, String> {
        let mut differ = false;
        let (pa, ia) = a.last.as_ref().unwrap();
        let (pb, ib) = b.last.as_ref().unwrap();
        if pa.len() != pb.len() { return Err(format!("{} blocks vs {}", pa.len(), pb.len())); }
        for (k, (x, y)) in pa.iter().zip(pb).enumerate() {
            if (x.start, x.end, &x.kind, &x.text) != (y.start, y.end, &y.kind, &y.text) { return Err(format!("block {k} differs: {:?} vs {:?}", (x.start, x.end), (y.start, y.end))); }
            differ |= ia[k].0 != ib[k].0;
        }
        let mut seen = std::collections::HashSet::new();
        if !ia.iter().all(|(id, _)| seen.insert(*id)) { return Err("duplicate block id after incremental re-parse".into()); }
        Ok(differ)
    }

    const PIECES: &[&str] = &["```\n", "~~~", "---\n", "===\n", "- ", "1. ", "> ", "| a | b |\n|---|---|\n", "|", "# ", "\n\n", "\n", " ", "    ", "[^a]: ", "[^a]", "<div>\n", "</div>\n\n", "*", "**", "`", "字", "😀", "。", "abc ", "[x]: http://e.com\n", "\t"];

    fn sample_doc(r: &mut R, n: usize) -> String {
        let mut t = String::new();
        for i in 0..n {
            match r.next() % 10 {
                0 => t.push_str(&format!("# 标题{i}\n\n")),
                1 => t.push_str(&format!("- 项目{i}\n- 项目{i}b\n\n")),
                2 => t.push_str(&format!("```\ncode {i}\n\n```\n\n")),
                3 => t.push_str(&format!("> 引用{i}\n\n")),
                4 => t.push_str(&format!("| a | b |\n|---|---|\n| {i} | x |\n\n")),
                5 => t.push_str(&format!("标题{i}\n---\n\n")),
                6 => t.push_str(&format!("脚注引用[^{i}]。\n\n[^{i}]: 脚注内容{i}。\n\n")),
                7 => t.push_str("---\n\n"),
                _ => t.push_str(&format!("第{i}段。内容足够长,讲的是故事{i}里的事情。\n\n")),
            }
        }
        t
    }

    #[test]
    fn incremental_reparse_equals_full_reparse_under_adversarial_edits() {
        let (mut inc, mut fallbacks, mut total, mut id_diff) = (0u64, 0u64, 0u64, 0u64);
        for seed in 1..=400u64 {
            let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut a = Doc::new();
            let n = 12 + r.next() % 30; a.load(&sample_doc(&mut r, n));
            let mut b = a.clone();
            b.incremental = false;
            for step in 0..40 {
                // random edit: insert a tricky piece, or delete a short span, at a random character boundary
                let chars: Vec<usize> = a.text.char_indices().map(|(i, _)| i).chain(std::iter::once(a.text.len())).collect();
                let at = chars[r.next() % chars.len()];
                let (from, to, ins) = if r.next() % 3 == 0 {
                    let e = chars[(chars.iter().position(|c| *c == at).unwrap() + r.next() % 6).min(chars.len() - 1)];
                    (at, e, String::new())
                } else { (at, at, PIECES[r.next() % PIECES.len()].to_string()) };
                let (f16, t16) = (a.text[..from].encode_utf16().count(), a.text[..to].encode_utf16().count());
                let (before_i, before_f) = (a.stats.incremental, a.stats.full);
                a.edit(a.version, &[(f16, t16, ins.clone())]).unwrap();
                b.edit(b.version, &[(f16, t16, ins.clone())]).unwrap();
                assert_eq!(a.text, b.text);
                match same(&a, &b) {
                    Err(e) => panic!("seed {seed} step {step}: {e}\nedit {from}..{to} {ins:?}\n{:?}", a.text),
                    Ok(true) => { id_diff += 1; b = a.clone(); b.incremental = false; } // resynchronise the shadow's ids
                    Ok(false) => {}
                }
                inc += a.stats.incremental - before_i;
                fallbacks += a.stats.full - before_f;
                total += 1;
            }
        }
        eprintln!("{total} adversarial edits: {inc} re-parsed a window, {fallbacks} fell back to the whole document; ids differed from a full re-parse in {id_diff}");
        assert!(inc as f64 > 0.5 * total as f64);
        assert!((id_diff as f64) < 0.02 * total as f64);
    }

    #[test]
    fn multi_span_patches_also_agree() {
        for seed in 1..=300u64 {
            let mut r = R(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
            let mut a = Doc::new();
            a.load(&sample_doc(&mut r, 20));
            let mut b = a.clone();
            b.incremental = false;
            let len16 = a.text.encode_utf16().count();
            let mut cuts: Vec<usize> = (0..4).map(|_| r.next() % (len16 + 1)).collect();
            cuts.sort_unstable();
            let mut ch = vec![];
            let mut prev = 0;
            for w in cuts.chunks(2) { if w[0] >= prev && w[0] <= w[1] { ch.push((w[0], w[1], PIECES[r.next() % PIECES.len()].to_string())); prev = w[1]; } }
            let (ra, rb) = (a.edit(a.version, &ch), b.edit(b.version, &ch));
            assert_eq!(ra, rb);
            if ra.is_ok() { same(&a, &b).unwrap_or_else(|e| panic!("seed {seed}: {e} {ch:?}")); }
        }
    }

    #[test]
    fn typing_in_a_book_parses_a_few_blocks_not_the_book() {
        let mut r = R(7);
        let mut a = Doc::new();
        let mut t = String::new();
        for c in 0..1000 { t.push_str(&sample_doc(&mut r, 15).replace("标题", &format!("章{c}标题"))); }
        a.load(&t);
        let mut b = a.clone();
        b.incremental = false;
        let blocks = a.last.as_ref().unwrap().0.len();
        let (mut ta, mut tb) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        let base = (a.stats.clone(), b.stats.clone());
        for k in 0..100 {
            let mut cut = a.text.len() / 2 + k * 3; while !a.text.is_char_boundary(cut) { cut += 1; }
            let at = a.text[..cut].encode_utf16().count();
            let s0 = std::time::Instant::now(); a.edit(a.version, &[(at, at, "字".into())]).unwrap(); ta += s0.elapsed();
            let s1 = std::time::Instant::now(); b.edit(b.version, &[(at, at, "字".into())]).unwrap(); tb += s1.elapsed();
        }
        same(&a, &b).unwrap();
        let (da, db) = (a.stats.parsed_bytes - base.0.parsed_bytes, b.stats.parsed_bytes - base.1.parsed_bytes);
        eprintln!("{} blocks, {} KB: 100 keystrokes — incremental {:?} total ({} bytes parsed, {} whole-document fallbacks), full {:?} total ({} bytes parsed)", blocks, a.text.len() / 1024, ta, da, a.stats.full - base.0.full, tb, db);
        assert!(a.stats.full - base.0.full <= 5);
        assert!(da * 20 < db, "incremental parsed {da} bytes vs {db}");
    }
}
