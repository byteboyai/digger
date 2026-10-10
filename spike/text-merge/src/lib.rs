//! Three-way text merge for Markdown chapters (ADR-012 §4).
//!
//! Prose paragraphs are single long lines, so a line-based merge would report a conflict for two edits in the same
//! paragraph. Here the unit is a token (a word, a CJK character, punctuation, whitespace, an emoji sequence), so edits
//! to different places of one paragraph merge, and only edits that overlap or touch each other conflict.

use similar::{Algorithm, DiffOp, capture_diff_slices};
use unicode_segmentation::UnicodeSegmentation;

/// A run of the merged text: either settled, or a place where both sides changed the same base text differently.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Clean(String),
    Conflict { base: String, ours: String, theirs: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolve {
    Ours,
    Theirs,
    /// keep both: ours then theirs, one after the other
    Both,
}

impl Merge {
    pub fn is_clean(&self) -> bool {
        self.segments.iter().all(|s| matches!(s, Segment::Clean(_)))
    }
    pub fn conflicts(&self) -> usize {
        self.segments.iter().filter(|s| matches!(s, Segment::Conflict { .. })).count()
    }
    /// The text if there is nothing to decide.
    pub fn text(&self) -> Option<String> {
        self.is_clean().then(|| self.resolve(Resolve::Ours))
    }
    pub fn resolve(&self, how: Resolve) -> String {
        let mut out = String::new();
        for s in &self.segments {
            match s {
                Segment::Clean(t) => out.push_str(t),
                Segment::Conflict { ours, theirs, .. } => match how {
                    Resolve::Ours => out.push_str(ours),
                    Resolve::Theirs => out.push_str(theirs),
                    Resolve::Both => {
                        out.push_str(ours);
                        out.push_str(theirs);
                    }
                },
            }
        }
        out
    }
}

/// UAX #29 word boundaries: a word, a punctuation mark, a whitespace run, a single Han/Kana character, a whole emoji
/// sequence. Newlines are their own tokens so blank lines between blocks stay intact.
pub fn tokens(text: &str) -> Vec<&str> {
    text.split_word_bounds().collect()
}

#[derive(Debug, Clone)]
struct Hunk {
    /// range of base tokens that is replaced
    b: std::ops::Range<usize>,
    /// the replacement text
    text: String,
}

/// The same deletion or insertion can be written at several places when tokens repeat ("reader reader": delete the
/// first or the second?). Two diffs of nearly the same text may pick different places, and then the merge would see
/// two independent edits and apply both. Slide every pure deletion and pure insertion as far left as it goes (never
/// past the previous hunk) so that equal changes are written the same way.
fn canonicalize(hunks: &mut [Hunk], base: &[&str]) {
    let mut lower = 0;
    for h in hunks.iter_mut() {
        if h.text.is_empty() && h.b.end > h.b.start {
            while h.b.start > lower && base[h.b.start - 1] == base[h.b.end - 1] {
                h.b = h.b.start - 1..h.b.end - 1;
            }
        } else if h.b.start == h.b.end && !h.text.is_empty() {
            let mut toks: Vec<String> = tokens(&h.text).into_iter().map(String::from).collect();
            let mut at = h.b.start;
            while at > lower && toks.last().is_some_and(|l| l == base[at - 1]) {
                let last = toks.pop().unwrap();
                toks.insert(0, last);
                at -= 1;
            }
            h.text = toks.concat();
            h.b = at..at;
        }
        lower = lower.max(h.b.end);
    }
}

/// Turns the diff operations into hunks. The positions of insertions are taken from a cursor over the base that
/// advances with every operation, NOT from the `old_index` field of `Insert`: `similar` does not always set that
/// field to the place where the insertion happens (it can point back into a span that was deleted just before, which
/// was this crate's worst bug: an unchanged token next to the insertion was swallowed and a change silently lost).
fn ops_to_hunks<T: ?Sized + AsRef<str>>(ops: Vec<DiffOp>, other: &[&T], offset: usize) -> Vec<Hunk> {
    let join = |from: usize, len: usize| -> String { other[from..from + len].iter().map(|t| t.as_ref()).collect() };
    let mut out = Vec::new();
    let mut at = 0usize; // base position after the operations seen so far
    for op in ops {
        match op {
            DiffOp::Equal { old_index, len, .. } => at = old_index + len,
            DiffOp::Delete { old_index, old_len, .. } => {
                out.push(Hunk { b: offset + old_index..offset + old_index + old_len, text: String::new() });
                at = old_index + old_len;
            }
            DiffOp::Insert { new_index, new_len, .. } => out.push(Hunk { b: offset + at..offset + at, text: join(new_index, new_len) }),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                out.push(Hunk { b: offset + old_index..offset + old_index + old_len, text: join(new_index, new_len) });
                at = old_index + old_len;
            }
        }
    }
    out
}

/// Token-level hunks between two token sequences (used inside changed blocks only).
fn token_hunks(base: &[&str], other: &[&str], base_offset: usize) -> Vec<Hunk> {
    ops_to_hunks(capture_diff_slices(Algorithm::Myers, base, other), other, base_offset)
}

/// Split after every run of blank lines: a unit is one block together with the blank lines that follow it.
fn units(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] == b'\n' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            let mut j = i;
            while j < bytes.len() && bytes[j] == b'\n' {
                j += 1;
            }
            out.push(&text[start..j]);
            start = j;
            i = j;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        out.push(&text[start..]);
    }
    out
}

/// Block-aware hunks: first compare the sequences of blocks, so a block that is inserted, deleted or moved is one hunk
/// and can never be "matched" against similar-looking characters elsewhere in the document; only blocks that were
/// changed in place (or replaced) are compared token by token, and only against each other.
fn hunks(base_text: &str, other_text: &str, base_tokens: &[&str]) -> Vec<Hunk> {
    let (bu, ou) = (units(base_text), units(other_text));
    // token index at the start of every base unit
    let mut tok_at = Vec::with_capacity(bu.len() + 1);
    let mut n = 0;
    for u in &bu {
        tok_at.push(n);
        n += tokens(u).len();
    }
    tok_at.push(n);
    debug_assert_eq!(n, base_tokens.len());
    let mut out = Vec::new();
    let mut at = 0usize; // base UNIT position after the operations seen so far
    for op in capture_diff_slices(Algorithm::Myers, &bu, &ou) {
        match op {
            DiffOp::Equal { old_index, len, .. } => at = old_index + len,
            DiffOp::Delete { old_index, old_len, .. } => {
                out.push(Hunk { b: tok_at[old_index]..tok_at[old_index + old_len], text: String::new() });
                at = old_index + old_len;
            }
            DiffOp::Insert { new_index, new_len, .. } => out.push(Hunk { b: tok_at[at]..tok_at[at], text: ou[new_index..new_index + new_len].concat() }),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let old_tokens = &base_tokens[tok_at[old_index]..tok_at[old_index + old_len]];
                let new_text = ou[new_index..new_index + new_len].concat();
                let new_tokens = tokens(&new_text);
                out.extend(token_hunks(old_tokens, &new_tokens, tok_at[old_index]));
                at = old_index + old_len;
            }
        }
    }
    canonicalize(&mut out, base_tokens);
    out
}

/// A diff may cut the same edit into different hunks on the two sides (one big insertion against an insertion, a few
/// characters matched to the base, and another insertion). Over a range that is too small the results then differ.
/// Absorb the hunks that follow, on both sides, a few at a time; if the two sides come out equal over the larger
/// range, or one of them left the range as it was, there is no conflict.
fn settle(bt: &[&str], ho: &[Hunk], ht: &[Hunk], (i0, j0): (usize, usize), (mut i, mut j): (usize, usize), start: usize, mut end: usize) -> Option<(String, usize, usize, usize)> {
    const WINDOW: usize = 48;
    const MAX_ABSORBED: usize = 12;
    for _ in 0..MAX_ABSORBED {
        let next_o = ho.get(i).filter(|h| h.b.start <= end + WINDOW);
        let next_t = ht.get(j).filter(|h| h.b.start <= end + WINDOW);
        match (next_o, next_t) {
            (None, None) => return None,
            (Some(a), Some(b)) => {
                if a.b.start <= b.b.start { end = end.max(a.b.end); i += 1 } else { end = end.max(b.b.end); j += 1 }
            }
            (Some(a), None) => { end = end.max(a.b.end); i += 1 }
            (None, Some(b)) => { end = end.max(b.b.end); j += 1 }
        }
        // absorb everything that now overlaps the grown range
        loop {
            let mut grew = false;
            if let Some(a) = ho.get(i).filter(|h| h.b.start <= end) { end = end.max(a.b.end); i += 1; grew = true; }
            if let Some(b) = ht.get(j).filter(|h| h.b.start <= end) { end = end.max(b.b.end); j += 1; grew = true; }
            if !grew { break; }
        }
        let base_part = bt[start..end].concat();
        let (o, t) = (side_text(bt, &ho[i0..i], start, end), side_text(bt, &ht[j0..j], start, end));
        if o == t || t == base_part { return Some((o, i, j, end)); }
        if o == base_part { return Some((t, i, j, end)); }
    }
    None
}

#[doc(hidden)]
pub fn debug_hunks(base: &str, other: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let bt = tokens(base);
    hunks(base, other, &bt).into_iter().map(|h| (h.b, h.text)).collect()
}

/// What `side`'s hunks (all inside `lo..hi` of the base) make of that stretch of the base.
fn side_text(base: &[&str], side: &[Hunk], lo: usize, hi: usize) -> String {
    let mut out = String::new();
    let mut at = lo;
    for h in side {
        out.push_str(&base[at..h.b.start].concat());
        out.push_str(&h.text);
        at = h.b.end;
    }
    out.push_str(&base[at..hi].concat());
    out
}

/// Three-way merge. A clean answer is double-checked before it is returned: the merged text already contains both
/// sides' changes, so merging it with either side against the same base must give it back unchanged. If it does
/// not (a diff cut some edit ambiguously), the result is not trusted and is reported as a conflict over the whole
/// text instead. Losing a change silently is the one failure a merge must not have.
pub fn merge(base: &str, ours: &str, theirs: &str) -> Merge {
    let m = merge_unchecked(base, ours, theirs);
    if let Some(text) = m.text() {
        let trusted = [ours, theirs].into_iter().all(|side| side == text || merge_unchecked(base, side, &text).text().as_deref() == Some(text.as_str()));
        if !trusted {
            return Merge { segments: vec![Segment::Conflict { base: base.to_string(), ours: ours.to_string(), theirs: theirs.to_string() }] };
        }
    }
    m
}

/// The merge without the verification gate (for experiments).
#[doc(hidden)]
pub fn merge_for_test(base: &str, ours: &str, theirs: &str) -> Merge {
    merge_unchecked(base, ours, theirs)
}

fn merge_unchecked(base: &str, ours: &str, theirs: &str) -> Merge {
    if ours == theirs {
        return Merge { segments: vec![Segment::Clean(ours.to_string())] };
    }
    if base == ours {
        return Merge { segments: vec![Segment::Clean(theirs.to_string())] };
    }
    if base == theirs {
        return Merge { segments: vec![Segment::Clean(ours.to_string())] };
    }
    let (bt, ot, tt) = (tokens(base), tokens(ours), tokens(theirs));
    let (ho, ht) = (hunks(base, ours, &bt), hunks(base, theirs, &bt));
    let mut segments: Vec<Segment> = Vec::new();
    let mut push_clean = |segments: &mut Vec<Segment>, s: String| {
        if s.is_empty() {
            return;
        }
        if let Some(Segment::Clean(last)) = segments.last_mut() {
            last.push_str(&s);
        } else {
            segments.push(Segment::Clean(s));
        }
    };
    let (mut i, mut j, mut at) = (0usize, 0usize, 0usize);
    while i < ho.len() || j < ht.len() {
        // the next cluster starts at the earliest hunk and grows over everything that overlaps or touches it
        let start = match (ho.get(i), ht.get(j)) {
            (Some(a), Some(b)) => a.b.start.min(b.b.start),
            (Some(a), None) => a.b.start,
            (None, Some(b)) => b.b.start,
            (None, None) => unreachable!(),
        };
        let mut end = start;
        let (i0, j0) = (i, j);
        loop {
            let mut grew = false;
            if let Some(a) = ho.get(i) {
                if a.b.start <= end {
                    end = end.max(a.b.end);
                    i += 1;
                    grew = true;
                }
            }
            if let Some(b) = ht.get(j) {
                if b.b.start <= end {
                    end = end.max(b.b.end);
                    j += 1;
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        push_clean(&mut segments, bt[at..start].concat());
        let (co, ct) = (&ho[i0..i], &ht[j0..j]);
        let base_part = bt[start..end].concat();
        match (co.is_empty(), ct.is_empty()) {
            (false, true) => push_clean(&mut segments, side_text(&bt, co, start, end)),
            (true, false) => push_clean(&mut segments, side_text(&bt, ct, start, end)),
            _ => {
                let (o, t) = (side_text(&bt, co, start, end), side_text(&bt, ct, start, end));
                if o == t || t == base_part {
                    push_clean(&mut segments, o);
                } else if o == base_part {
                    push_clean(&mut segments, t);
                } else if let Some((text, ni, nj, nend)) = settle(&bt, &ho, &ht, (i0, j0), (i, j), start, end) {
                    // the two sides spelled the same change with differently cut hunks: nothing to decide
                    push_clean(&mut segments, text);
                    i = ni;
                    j = nj;
                    end = nend;
                } else {
                    segments.push(Segment::Conflict { base: base_part, ours: o, theirs: t });
                }
            }
        }
        at = end;
    }
    push_clean(&mut segments, bt[at..].concat());
    Merge { segments }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_in_different_sentences_of_one_paragraph_merge() {
        let base = "第一句话在这里。第二句话在这里。第三句话在这里。";
        let ours = "第一句话在这里，改了。第二句话在这里。第三句话在这里。";
        let theirs = "第一句话在这里。第二句话在这里。第三句话在那里。";
        let m = merge(base, ours, theirs);
        assert_eq!(m.text().as_deref(), Some("第一句话在这里，改了。第二句话在这里。第三句话在那里。"));
    }

    #[test]
    fn overlapping_edits_conflict_and_are_never_silently_dropped() {
        let m = merge("甲乙丙丁戊", "甲乙X丁戊", "甲乙Y丁戊");
        assert_eq!(m.conflicts(), 1);
        assert_eq!(m.resolve(Resolve::Ours), "甲乙X丁戊");
        assert_eq!(m.resolve(Resolve::Theirs), "甲乙Y丁戊");
        assert_eq!(m.resolve(Resolve::Both), "甲乙XY丁戊");
        assert!(m.text().is_none());
    }

    #[test]
    fn identical_changes_on_both_sides_are_not_a_conflict() {
        assert_eq!(merge("a b c", "a X c", "a X c").text().as_deref(), Some("a X c"));
        assert_eq!(merge("a b c d", "a X c d", "a b c Y").text().as_deref(), Some("a X c Y"));
    }

    #[test]
    fn one_side_unchanged_is_the_other_side() {
        assert_eq!(merge("base", "base", "theirs").text().as_deref(), Some("theirs"));
        assert_eq!(merge("base", "ours", "base").text().as_deref(), Some("ours"));
    }

    #[test]
    fn blocks_are_independent_even_when_each_is_a_single_long_line() {
        let base = "# 题\n\n甲段落内容在这里。\n\n乙段落内容在这里。\n";
        let ours = "# 题\n\n甲段落内容在这里，加了一句。\n\n乙段落内容在这里。\n";
        let theirs = "# 题\n\n甲段落内容在这里。\n\n乙段落内容在那里。\n";
        assert_eq!(merge(base, ours, theirs).text().as_deref(), Some("# 题\n\n甲段落内容在这里，加了一句。\n\n乙段落内容在那里。\n"));
    }

    #[test]
    fn emoji_sequences_and_combining_marks_are_not_split() {
        let t = tokens("a👨‍👩‍👧b é");
        assert!(t.contains(&"👨‍👩‍👧"), "{t:?}");
        let m = merge("x 👨‍👩‍👧 y", "x 👨‍👩‍👧 y!", "w 👨‍👩‍👧 y");
        assert_eq!(m.text().as_deref(), Some("w 👨‍👩‍👧 y!"));
    }

    #[test]
    fn deleting_a_block_while_the_other_side_edits_another_block() {
        let base = "甲段内容。\n\n乙段内容。\n\n丙段内容。\n";
        let ours = "甲段内容。\n\n丙段内容。\n"; // deletes 乙
        let theirs = "甲段内容。\n\n乙段内容。\n\n丙段内容改。\n";
        assert_eq!(merge(base, ours, theirs).text().as_deref(), Some("甲段内容。\n\n丙段内容改。\n"));
    }

    // ---- regressions: each of these was a real bug found by the random campaign -------------------------------

    #[test]
    fn an_insertion_after_matched_tokens_is_not_pulled_back_into_a_deleted_span() {
        // `similar` reported the last insertion with an old_index inside the span deleted just before it; the token
        // matched in between ("的") was swallowed and the other side's change was lost
        let base = "一，中等的主";
        let ours = "一【外，的声情】的主";
        let theirs = "一，中等的主!";
        assert_eq!(merge(base, ours, theirs).text().as_deref(), Some("一【外，的声情】的主!"));
        assert_eq!(merge(base, theirs, ours).text().as_deref(), Some("一【外，的声情】的主!"));
    }

    #[test]
    fn deleting_one_of_two_equal_neighbours_is_the_same_change_whichever_one_is_named() {
        // base "reader reader": ours deletes the first and edits elsewhere, theirs deletes the second
        let base = "alpha beta gamma. return return return reader reader system. delta epsilon.";
        let ours = "ALPHA beta gamma. return return return reader system. delta epsilon.";
        let theirs = "alpha beta gamma. return return return reader system. delta epsilon!";
        let want = "ALPHA beta gamma. return return return reader system. delta epsilon!";
        assert_eq!(merge(base, ours, theirs).text().as_deref(), Some(want));
        // both delete "a reader" but name different ones: it is one deletion, not two
        let theirs2 = "alpha beta gamma. return return return reader system. delta epsilon.";
        assert_eq!(merge(base, ours, theirs2).text().as_deref(), Some(ours));
    }

    #[test]
    fn a_clean_answer_must_survive_being_merged_again() {
        // the merged text contains both sides' changes: merging it with either side must give it back
        let base = "第一句。第二句。第三句。第四句。第五句。";
        let ours = "第一句改。第二句。第三句。第四句。第五句。";
        let theirs = "第一句。第二句。第三句。第四句。第五句改。";
        let m = merge(base, ours, theirs).text().unwrap();
        assert_eq!(merge(base, ours, &m).text().as_deref(), Some(m.as_str()));
        assert_eq!(merge(base, theirs, &m).text().as_deref(), Some(m.as_str()));
    }
}
