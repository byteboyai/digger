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
    last: Option<(Vec<Parsed>, Vec<(u64, String)>)>,
    /// old block id -> (id of the block that replaced it, plan run that did it): a rewrite gives the block a new id
    aliases: std::collections::HashMap<u64, (u64, u64)>,
    runs: u64,
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
        Doc { text: String::new(), version: 0, tracked: vec![], next_id: 1, last: None, aliases: Default::default(), runs: 0 }
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
        for ((f, t), (_, _, ins)) in spans.iter().zip(changes).rev() {
            self.text.replace_range(*f..*t, ins);
        }
        self.version += 1;
        self.reparse();
        Ok(())
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
        self.edit(self.version, &patch).map_err(|e| AgentError::Edit(format!("{e:?}")))?;
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

    fn reparse(&mut self) {
        let parsed = parse_blocks(&self.text);
        let mut next = self.next_id;
        let assigned = reconcile(&self.tracked, &parsed, &Config::default(), || { next += 1; next - 1 });
        self.next_id = next;
        self.tracked = parsed.iter().zip(&assigned).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        self.last = Some((parsed, assigned.iter().map(|a| (a.id, format!("{:?}", a.how))).collect()));
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
}
