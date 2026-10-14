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

/// Token ranges of the sentences of a block: a sentence ends after . 。 ！ ？ ! ? ; and the closing quotes and spaces
/// that follow. Ranges cover the tokens exactly, so concatenating them gives the block back.
fn sentence_ranges(toks: &[&str]) -> Vec<std::ops::Range<usize>> {
    let ends = |t: &str| matches!(t, "." | "。" | "．" | "！" | "？" | "!" | "?" | ";" | "；" | "…");
    let closing = |t: &str| matches!(t, "”" | "’" | "\"" | "'" | "」" | "』" | "）" | ")" | "]" | "】");
    let mut out = vec![];
    let (mut start, mut i) = (0, 0);
    while i < toks.len() {
        if ends(toks[i]) {
            let mut j = i + 1;
            while j < toks.len() && closing(toks[j]) { j += 1; }
            // spaces after the sentence belong to it; line breaks (the end of the block) do not
            while j < toks.len() && toks[j].chars().all(char::is_whitespace) && !toks[j].contains('\n') { j += 1; }
            out.push(start..j);
            start = j;
            i = j;
        } else {
            i += 1;
        }
    }
    if start < toks.len() { out.push(start..toks.len()); }
    out
}

/// Sentence-level hunks inside one changed block: a sentence that was replaced by an unrelated one is ONE hunk (and
/// not a dozen small ones around the words the two sentences happen to share); only sentences that are versions of
/// each other are compared token by token.
fn sentence_level(base: &[&str], other: &[&str], offset: usize) -> Vec<Hunk> {
    let (bs, os) = (sentence_ranges(base), sentence_ranges(other));
    if bs.len() <= 1 && os.len() <= 1 { return token_hunks(base, other, offset); }
    let bu: Vec<String> = bs.iter().map(|r| base[r.clone()].concat()).collect();
    let ou: Vec<String> = os.iter().map(|r| other[r.clone()].concat()).collect();
    let pos = |at: usize| offset + if at < bs.len() { bs[at].start } else { base.len() };
    let mut out = Vec::new();
    let mut at = 0usize;
    for op in capture_diff_slices(Algorithm::Myers, &bu, &ou) {
        match op {
            DiffOp::Equal { old_index, len, .. } => at = old_index + len,
            DiffOp::Delete { old_index, old_len, .. } => {
                out.push(Hunk { b: pos(old_index)..pos(old_index + old_len), text: String::new() });
                at = old_index + old_len;
            }
            DiffOp::Insert { new_index, new_len, .. } => out.push(Hunk { b: pos(at)..pos(at), text: ou[new_index..new_index + new_len].concat() }),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let (ob, nb) = (bs[old_index].start..bs[old_index + old_len - 1].end, os[new_index].start..os[new_index + new_len - 1].end);
                let whole = |out: &mut Vec<Hunk>| out.extend(token_hunks(&base[ob.clone()], &other[nb.clone()], offset + ob.start));
                if old_len == 1 && new_len == 1 {
                    // one sentence against one sentence: versions of each other, or a rewrite
                    let (a, b) = (&base[ob.clone()], &other[nb.clone()]);
                    if dice(a, b) >= 0.25 { whole(&mut out) } else { out.push(Hunk { b: offset + ob.start..offset + ob.end, text: other[nb.clone()].concat() }) }
                } else if old_len * new_len > 256 {
                    whole(&mut out)
                } else {
                    let olds: Vec<Vec<&str>> = (old_index..old_index + old_len).map(|k| base[bs[k].clone()].to_vec()).collect();
                    let news: Vec<Vec<&str>> = (new_index..new_index + new_len).map(|k| other[os[k].clone()].to_vec()).collect();
                    match align_units(&olds, &news) {
                        None => whole(&mut out),
                        Some(steps) => {
                            let (mut oi, mut nj) = (0, 0);
                            for step in steps {
                                match step {
                                    Align::Pair => {
                                        let o = &bs[old_index + oi];
                                        if dice(&olds[oi], &news[nj]) >= 0.25 { out.extend(token_hunks(&olds[oi], &news[nj], offset + o.start)) } else { out.push(Hunk { b: offset + o.start..offset + o.end, text: news[nj].concat() }) }
                                        oi += 1;
                                        nj += 1;
                                    }
                                    Align::Delete => { out.push(Hunk { b: pos(old_index + oi)..pos(old_index + oi + 1), text: String::new() }); oi += 1; }
                                    Align::Insert => { out.push(Hunk { b: pos(old_index + oi)..pos(old_index + oi), text: news[nj].concat() }); nj += 1; }
                                }
                            }
                        }
                    }
                }
                at = old_index + old_len;
            }
        }
    }
    out
}

fn dice(a: &[&str], b: &[&str]) -> f32 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let mut counts: std::collections::HashMap<&str, i32> = std::collections::HashMap::new();
    for t in a { *counts.entry(t).or_default() += 1; }
    let mut common = 0;
    for t in b { if let Some(c) = counts.get_mut(t) { if *c > 0 { *c -= 1; common += 1; } } }
    2.0 * common as f32 / (a.len() + b.len()) as f32
}

enum Align { Pair, Delete, Insert }

/// Monotone alignment of two short lists of blocks: pair blocks that are versions of each other (token Dice of at
/// least 0.4, maximising the total similarity); everything else is a deletion or an insertion.
fn align_units(olds: &[Vec<&str>], news: &[Vec<&str>]) -> Option<Vec<Align>> {
    use std::collections::HashMap;
    let sim = |a: &[&str], b: &[&str]| -> f32 {
        if a.is_empty() || b.is_empty() { return 0.0; }
        let mut counts: HashMap<&str, i32> = HashMap::new();
        for t in a { *counts.entry(t).or_default() += 1; }
        let mut common = 0;
        for t in b { if let Some(c) = counts.get_mut(t) { if *c > 0 { *c -= 1; common += 1; } } }
        2.0 * common as f32 / (a.len() + b.len()) as f32
    };
    let (m, n) = (olds.len(), news.len());
    let mut score = vec![vec![0.0f32; n + 1]; m + 1];
    let mut sims = vec![vec![0.0f32; n]; m];
    for i in 0..m { for j in 0..n { sims[i][j] = sim(&olds[i], &news[j]); } }
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            let mut best = score[i + 1][j].max(score[i][j + 1]);
            if sims[i][j] >= 0.4 { best = best.max(score[i + 1][j + 1] + sims[i][j]); }
            score[i][j] = best;
        }
    }
    let (mut i, mut j, mut out) = (0, 0, vec![]);
    while i < m || j < n {
        if i < m && j < n && sims[i][j] >= 0.4 && (score[i][j] - (score[i + 1][j + 1] + sims[i][j])).abs() < 1e-6 { out.push(Align::Pair); i += 1; j += 1; }
        else if i < m && (j >= n || (score[i][j] - score[i + 1][j]).abs() < 1e-6) { out.push(Align::Delete); i += 1; }
        else { out.push(Align::Insert); j += 1; }
    }
    // between two pairs, as many blocks deleted as inserted: they are rewrites of each other, pair them in order
    let mut fixed = Vec::with_capacity(out.len());
    let mut k = 0;
    while k < out.len() {
        if matches!(out[k], Align::Pair) { fixed.push(Align::Pair); k += 1; continue; }
        let end = (k..out.len()).find(|&e| matches!(out[e], Align::Pair)).unwrap_or(out.len());
        let dels = out[k..end].iter().filter(|a| matches!(a, Align::Delete)).count();
        let ins = (end - k) - dels;
        if dels == ins { fixed.extend((0..dels).map(|_| Align::Pair)); }
        else { fixed.extend(out[k..end].iter().map(|a| match a { Align::Delete => Align::Delete, _ => Align::Insert })); }
        k = end;
    }
    // A block that is only deleted but whose text is largely present in the other side's blocks was merged into them;
    // one that is only inserted but largely present in the old blocks was split off from them. Those are not
    // independent deletions and insertions: leave them to the diff over the concatenated blocks.
    // measured on pairs of neighbouring tokens: single CJK characters are shared by unrelated sentences all the time
    let contained = |unit: &[&str], others: &[Vec<&str>]| -> f32 {
        let mut counts: HashMap<(&str, &str), i32> = HashMap::new();
        for o in others { for w in o.windows(2) { *counts.entry((w[0], w[1])).or_default() += 1; } }
        let (mut hit, mut total) = (0, 0);
        for w in unit.windows(2) { total += 1; if let Some(c) = counts.get_mut(&(w[0], w[1])) { if *c > 0 { *c -= 1; hit += 1; } } }
        hit as f32 / total.max(1) as f32
    };
    let (mut oi, mut nj) = (0, 0);
    for a in &fixed {
        match a {
            Align::Pair => { oi += 1; nj += 1; }
            Align::Delete => { if contained(&olds[oi], news) >= 0.6 { return None; } oi += 1; }
            Align::Insert => { if contained(&news[nj], olds) >= 0.6 { return None; } nj += 1; }
        }
    }
    Some(fixed)
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
                let olds: Vec<Vec<&str>> = if old_len == 1 && new_len == 1 || old_len * new_len > 256 { vec![] } else { (old_index..old_index + old_len).map(|k| tokens(bu[k])).collect() };
                let news: Vec<Vec<&str>> = if olds.is_empty() { vec![] } else { (new_index..new_index + new_len).map(|k| tokens(ou[k])).collect() };
                let aligned = if olds.is_empty() { None } else { align_units(&olds, &news) };
                if aligned.is_none() {
                    let old_tokens = &base_tokens[tok_at[old_index]..tok_at[old_index + old_len]];
                    let new_text = ou[new_index..new_index + new_len].concat();
                    let new_tokens = tokens(&new_text);
                    out.extend(sentence_level(old_tokens, &new_tokens, tok_at[old_index]));
                } else {
                    // several blocks on each side (a block changed and its neighbour was deleted, ...): pair up the
                    // ones that are versions of each other and treat the rest as deleted or inserted, so each block
                    // is diffed against its own counterpart and not against the concatenation of its neighbours
                    let mut oi = 0;
                    let mut nj = 0;
                    for step in aligned.unwrap() {
                        match step {
                            Align::Pair => {
                                out.extend(sentence_level(&olds[oi], &news[nj], tok_at[old_index + oi]));
                                oi += 1;
                                nj += 1;
                            }
                            Align::Delete => {
                                out.push(Hunk { b: tok_at[old_index + oi]..tok_at[old_index + oi + 1], text: String::new() });
                                oi += 1;
                            }
                            Align::Insert => {
                                out.push(Hunk { b: tok_at[old_index + oi]..tok_at[old_index + oi], text: ou[new_index + nj].to_string() });
                                nj += 1;
                            }
                        }
                    }
                }
                at = old_index + old_len;
            }
        }
    }
    coalesce(&mut out);
    canonicalize(&mut out, base_tokens);
    coalesce(&mut out);
    out
}

/// Hunks that touch in the base are one change. A diff can report them separately (two insertions at the same
/// place), and then the same change looks different depending on what else changed nearby.
fn coalesce(hunks: &mut Vec<Hunk>) {
    let mut out: Vec<Hunk> = Vec::with_capacity(hunks.len());
    for h in hunks.drain(..) {
        match out.last_mut() {
            Some(last) if last.b.end == h.b.start => {
                last.b.end = h.b.end;
                last.text.push_str(&h.text);
            }
            _ => out.push(h),
        }
    }
    *hunks = out;
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

/// Hunks that touch, or have at most one real (non-blank) token between them, are compared together. A diff may cut
/// one change into pieces with a stray matched token between them (a comma that also occurs inside an inserted
/// sentence), and the same change then sits at slightly different places depending on what else changed.
fn near(bt: &[&str], end: usize, start: usize) -> bool {
    if start <= end { return true; }
    if start - end > 8 { return false; }
    bt[end..start.min(bt.len())].iter().filter(|t| !t.trim().is_empty()).count() <= 1
}

/// `ins` is a pure insertion that can be written, sliding right over equal tokens, at the exact place where `del`
/// (a pure deletion starting with blank lines) begins, and no later hunk of its own side is in the way.
fn insertion_before_deletion(ins: &Hunk, del: &Hunk, next_same_side: Option<&Hunk>, bt: &[&str]) -> Option<(usize, String)> {
    if !ins.b.is_empty() || ins.text.is_empty() || !is_pure_deletion_after_blank(del, bt) || ins.b.start > del.b.start { return None; }
    let mut toks: Vec<String> = tokens(&ins.text).into_iter().map(String::from).collect();
    let mut pos = ins.b.start;
    while pos < del.b.start && pos < bt.len() && toks.first().is_some_and(|f| f == bt[pos]) {
        let first = toks.remove(0);
        toks.push(first);
        pos += 1;
    }
    if pos != del.b.start || next_same_side.is_some_and(|n| n.b.start < pos) { return None; }
    Some((pos, toks.concat()))
}

fn is_pure_deletion_after_blank(h: &Hunk, bt: &[&str]) -> bool {
    h.text.is_empty() && !h.b.is_empty() && bt.get(h.b.start).is_some_and(|t| t.matches('\n').count() >= 2 && t.chars().all(char::is_whitespace))
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
    let bt = tokens(base);
    let (ho, ht) = (hunks(base, ours, &bt), hunks(base, theirs, &bt));
    let mut segments: Vec<Segment> = Vec::new();
    let push_clean = |segments: &mut Vec<Segment>, s: String| {
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
        // An insertion at the end of a block's text (as far right as it can be written) and the other side deleting
        // the NEXT block (its hunk starts with the blank lines in between) do not touch: nothing the insertion could
        // belong to is deleted. The insertion goes first and the deletion is handled in the next round.
        if let (Some(a), Some(b)) = (ho.get(i), ht.get(j)) {
            if let Some((pos, text)) = insertion_before_deletion(a, b, ho.get(i + 1), &bt) {
                push_clean(&mut segments, bt[at..pos].concat());
                push_clean(&mut segments, text);
                at = pos;
                i += 1;
                continue;
            }
            if let Some((pos, text)) = insertion_before_deletion(b, a, ht.get(j + 1), &bt) {
                push_clean(&mut segments, bt[at..pos].concat());
                push_clean(&mut segments, text);
                at = pos;
                j += 1;
                continue;
            }
        }
        let mut end = start;
        let mut by_insertion = false; // the hunk that last set `end` is a pure insertion
        let (i0, j0) = (i, j);
        loop {
            let mut grew = false;
            // touching hunks always belong together; a pure insertion also pulls in a hunk one real token away
            let joins = |h: &Hunk, end: usize, by_insertion: bool| h.b.start <= end || ((h.b.is_empty() || by_insertion) && near(&bt, end, h.b.start));
            if let Some(a) = ho.get(i) {
                if joins(a, end, by_insertion) {
                    if a.b.end >= end { by_insertion = a.b.is_empty(); }
                    end = end.max(a.b.end);
                    i += 1;
                    grew = true;
                }
            }
            if let Some(b) = ht.get(j) {
                if joins(b, end, by_insertion) {
                    if b.b.end >= end { by_insertion = b.b.is_empty(); }
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

    // ---- found by running the merge on real prose (Dream of the Red Chamber): no synthetic case produced them -----

    #[test]
    fn an_edit_in_one_paragraph_and_the_deletion_of_the_next_paragraph_do_not_conflict() {
        let base = "# 回目\n\n宝玉笑道:“起来吃饭去,就开戏了。”\n\n　　吃了饭点戏时,贾母一定先叫宝钗点。\n\n　　末段另有别的话说。\n";
        let ours = base.replace("就开戏了。”", "就开戏了。”你愛看那一出?我先去点。”");
        let theirs = base.replace("　　吃了饭点戏时,贾母一定先叫宝钗点。\n\n", "");
        let want = ours.replace("　　吃了饭点戏时,贾母一定先叫宝钗点。\n\n", "");
        assert_eq!(merge(base, &ours, &theirs).text().as_deref(), Some(want.as_str()));
        assert_eq!(merge(base, &theirs, &ours).text().as_deref(), Some(want.as_str()));
    }

    #[test]
    fn an_appended_sentence_that_reuses_a_comma_of_the_old_text_still_merges_with_a_far_edit() {
        // the appended text contains tokens (，) that the diff may match against the following old text, cutting one
        // insertion into two; the change must look the same whether or not another edit is nearby
        let base = "你是个明白人,何必作此形像自苦。何况你又多病,还不自己保养。你听我说,这事不必再提。他们都在外头候着。";
        let ours = base.replace("自苦。", "自苦。今儿偏偏的来了个刘姥姥,我自己多事,为他跑了半日,这会子又被姨太太看见了。");
        let theirs = base.replace("保养。", "保养。我想府上待下人最宽的,为什么还有这事。");
        let want = base.replace("自苦。", "自苦。今儿偏偏的来了个刘姥姥,我自己多事,为他跑了半日,这会子又被姨太太看见了。").replace("保养。", "保养。我想府上待下人最宽的,为什么还有这事。");
        assert_eq!(merge(base, &ours, &theirs).text().as_deref(), Some(want.as_str()));
        let m = merge(base, &ours, &want).text();
        assert_eq!(m.as_deref(), Some(want.as_str()), "re-merging the answer with a side gives the answer");
    }
}
