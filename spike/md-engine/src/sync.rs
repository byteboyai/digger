//! The Rust side of the block range sync: Rust owns the text. The editor sends text patches in UTF-16 positions;
//! Rust applies them, re-parses, keeps block ids stable with the reconciler, and answers with the block list.

use block_identity_spike::parse::{Kind, Parsed, parse_blocks};
use block_identity_spike::reconcile::{Config, Tracked, reconcile};
use serde_json::{Value, json};

pub struct Doc {
    pub text: String,
    pub version: u64,
    tracked: Vec<Tracked>,
    next_id: u64,
    last: Option<(Vec<Parsed>, Vec<(u64, String)>)>,
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
        Doc { text: String::new(), version: 0, tracked: vec![], next_id: 1, last: None }
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

    fn reparse(&mut self) {
        let parsed = parse_blocks(&self.text);
        let mut next = self.next_id;
        let assigned = reconcile(&self.tracked, &parsed, &Config::default(), || { next += 1; next - 1 });
        self.next_id = next;
        self.tracked = parsed.iter().zip(&assigned).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        self.last = Some((parsed, assigned.iter().map(|a| (a.id, format!("{:?}", a.how))).collect()));
    }

    /// The `blocks` message for the editor: ranges in UTF-16 positions.
    pub fn blocks_message(&self) -> Value {
        let (parsed, hows) = self.last.as_ref().expect("a document was loaded");
        // one pass over the text to convert the (sorted) byte offsets to UTF-16 positions
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
        let blocks: Vec<Value> = parsed
            .iter()
            .zip(hows)
            .map(|(p, (id, how))| {
                let (kind, level) = kind_name(&p.kind);
                let title: Option<String> = level.map(|_| heading_title(&p.text));
                json!({"id": id.to_string(), "kind": kind, "level": level, "title": title, "how": how,
                       "from": pos16[&p.start], "to": pos16[&p.end], "head": p.text.chars().take(16).collect::<String>()})
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
}
