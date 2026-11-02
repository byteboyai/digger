//! Keeps block ids stable across a Commit (ADR-012 §2).
//!
//! Input: the blocks of the previous version (with their ids) and the blocks parsed from the new text.
//! Output: an id for every new block. Passes, in order:
//!  1. exact content hash along the longest common subsequence (unchanged blocks, wherever they are);
//!  2. exact hash out of order (moved blocks);
//!  3. inside each gap between anchors: split / merge detection, then a monotone similarity alignment;
//!  4. a global greedy similarity pass for blocks that were both edited and moved;
//!  5. everything left is a new block with a fresh id.

use crate::blocks::parse::{Kind, Parsed};
use similar::{Algorithm, DiffOp, capture_diff_slices};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

pub type BlockId = u64;

#[derive(Clone, Debug)]
pub struct Tracked {
    pub id: BlockId,
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum How {
    Exact,
    Moved,
    Similar,
    Split,
    Merged,
    Fresh,
}

#[derive(Clone, Copy, Debug)]
pub struct Assigned {
    pub id: BlockId,
    pub how: How,
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPolicy {
    /// The first part of a split (or the first block of a merge) keeps the id.
    FirstPart,
    /// The part that holds most of the original text keeps the id; ties go to the first.
    LargestPart,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Similarity needed to pair two blocks of the same kind inside a gap.
    pub low: f32,
    /// Similarity needed for the global pass (edited and moved) and for split/merge detection.
    pub high: f32,
    /// Similarity needed when the kind changed (paragraph typed into a heading).
    pub kind_change: f32,
    /// Blocks with fewer significant characters must match exactly (ignoring syntax).
    pub min_chars: usize,
    /// Blocks with fewer than this many significant characters are compared by character edit ratio.
    pub short_chars: usize,
    pub short_ratio: f32,
    pub split: SplitPolicy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            low: 0.5,
            high: 0.6,
            kind_change: 0.8,
            min_chars: 4,
            short_chars: 12,
            short_ratio: 0.6,
            split: SplitPolicy::LargestPart,
        }
    }
}

struct Sig {
    norm: Vec<char>,
    grams: HashMap<u64, usize>,
    total: usize,
    /// sentences / lines with syntax stripped, as a multiset: "half of the sentences are untouched" is strong
    /// evidence even when the rest was rewritten and the character statistics no longer agree
    units: HashMap<String, usize>,
    unit_count: usize,
}

fn normalize(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Comparison tokens: character bigrams for CJK text, whole words for alphabetic text. Bigrams of Latin letters
/// are far too common ("th", "er") for two unrelated English paragraphs to look different.
fn tokens(text: &str) -> (HashMap<u64, usize>, usize) {
    let mut grams: HashMap<u64, usize> = HashMap::new();
    let mut total = 0;
    let h = |parts: &[u64]| -> u64 {
        let mut st = std::collections::hash_map::DefaultHasher::new();
        parts.hash(&mut st);
        st.finish()
    };
    let mut word = String::new();
    let mut prev_cjk: Option<char> = None;
    let flush = |word: &mut String, grams: &mut HashMap<u64, usize>, total: &mut usize| {
        if !word.is_empty() {
            let mut st = std::collections::hash_map::DefaultHasher::new();
            word.hash(&mut st);
            *grams.entry(st.finish() ^ 0x9E37_79B9).or_default() += 1;
            *total += 1;
            word.clear();
        }
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            word.extend(c.to_lowercase());
            prev_cjk = None;
        } else if c.is_alphanumeric() {
            flush(&mut word, &mut grams, &mut total);
            if let Some(p) = prev_cjk {
                *grams.entry(h(&[p as u64, c as u64])).or_default() += 1;
                total += 1;
            }
            prev_cjk = Some(c);
        } else {
            flush(&mut word, &mut grams, &mut total);
            prev_cjk = None;
        }
    }
    flush(&mut word, &mut grams, &mut total);
    (grams, total)
}

fn sig(text: &str) -> Sig {
    let norm = normalize(text);
    let (grams, total) = tokens(text);
    let mut units: HashMap<String, usize> = HashMap::new();
    let mut unit_count = 0;
    for line in text.lines() {
        for u in line.split_inclusive(['。', '！', '？', '.', '!', '?']) {
            let n: String = normalize(u).into_iter().collect();
            if n.chars().count() >= 3 {
                *units.entry(n).or_default() += 1;
                unit_count += 1;
            }
        }
    }
    Sig {
        norm,
        grams,
        total,
        units,
        unit_count,
    }
}

/// Shared sentences/lines over the larger count.
fn unit_overlap(a: &Sig, b: &Sig) -> f32 {
    let max = a.unit_count.max(b.unit_count);
    if max < 2 {
        return 0.0; // a single unit gives no extra evidence beyond the character similarity
    }
    let inter: usize = a
        .units
        .iter()
        .map(|(u, &n)| n.min(*b.units.get(u).unwrap_or(&0)))
        .sum();
    inter as f32 / max as f32
}

/// Work budget for the character edit ratio. Bigram statistics stay O(n); the character diff is
/// O(n*m) in the worst case, so very long blocks fall back to the statistical score (and are counted)
/// instead of letting one pair consume the whole reconcile.
const MAX_CHAR_DIFF_CHARS: usize = 32_768;
const MAX_CHAR_DIFF_WORK: usize = 4_000_000;

fn char_diff_within_budget(a: &Sig, b: &Sig) -> bool {
    a.norm.len() <= MAX_CHAR_DIFF_CHARS
        && b.norm.len() <= MAX_CHAR_DIFF_CHARS
        && a.norm.len().saturating_mul(b.norm.len()) <= MAX_CHAR_DIFF_WORK
}

fn char_ratio(a: &Sig, b: &Sig) -> f32 {
    let (x, y): (String, String) = (a.norm.iter().collect(), b.norm.iter().collect());
    similar::TextDiff::from_chars(&x, &y).ratio()
}

/// Diagnostics for the reconciliation budget. `budget_degraded` counts pairs that skipped the
/// character diff because they were too large; those pairs were still compared by bigram statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconcileStats {
    pub char_diff_calls: u64,
    pub budget_degraded: u64,
}

fn dice(a: &Sig, b: &Sig) -> f32 {
    if a.norm.is_empty() && b.norm.is_empty() {
        return 1.0;
    }
    if a.total == 0 || b.total == 0 {
        return if a.norm == b.norm { 1.0 } else { 0.0 };
    }
    let (small, big) = if a.grams.len() <= b.grams.len() {
        (a, b)
    } else {
        (b, a)
    };
    let inter: usize = small
        .grams
        .iter()
        .map(|(g, &n)| n.min(*big.grams.get(g).unwrap_or(&0)) as usize)
        .sum();
    2.0 * inter as f32 / (a.total + b.total) as f32
}

fn hash_of(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

struct Ctx<'a> {
    old: &'a [Tracked],
    new: &'a [Parsed],
    osig: Vec<Sig>,
    nsig: Vec<Sig>,
    cfg: &'a Config,
    stats: Cell<ReconcileStats>,
}

impl Ctx<'_> {
    /// Similarity if the pair is allowed to match at all, else None.
    fn sim(&self, o: usize, n: usize, threshold: f32) -> Option<f32> {
        let (a, b) = (&self.osig[o], &self.nsig[n]);
        let shortest = a.norm.len().min(b.norm.len());
        let same_kind = self.old[o].kind == self.new[n].kind;
        let mut need = if same_kind {
            threshold
        } else {
            threshold.max(self.cfg.kind_change)
        };
        let s = if shortest < self.cfg.min_chars {
            // too little text to say anything: only an identical text (syntax aside) is a match
            if a.norm == b.norm { 1.0 } else { return None }
        } else if shortest < self.cfg.short_chars {
            // bigram statistics are meaningless on a handful of characters: use the character edit ratio
            need = need.max(self.cfg.short_ratio);
            self.char_ratio(a, b).unwrap_or(0.0)
        } else {
            let d = dice(a, b).max(unit_overlap(a, b));
            // Bigram statistics are stricter than an edit ratio; when they say "somewhat similar", ask the
            // character edit ratio too (only for plausible pairs, so the cost stays small).
            if d < need && d >= 0.25 {
                d.max(self.char_ratio(a, b).unwrap_or(d))
            } else {
                d
            }
        };
        (s >= need).then_some(s)
    }

    fn char_ratio(&self, a: &Sig, b: &Sig) -> Option<f32> {
        if !char_diff_within_budget(a, b) {
            let mut stats = self.stats.get();
            stats.budget_degraded += 1;
            self.stats.set(stats);
            return None;
        }
        let mut stats = self.stats.get();
        stats.char_diff_calls += 1;
        self.stats.set(stats);
        Some(char_ratio(a, b))
    }
}

pub fn reconcile(
    old: &[Tracked],
    new: &[Parsed],
    cfg: &Config,
    fresh: impl FnMut() -> BlockId,
) -> Vec<Assigned> {
    reconcile_with_stats(old, new, cfg, fresh).0
}

/// Same as [`reconcile`], with diagnostics for budget decisions.
pub fn reconcile_with_stats(
    old: &[Tracked],
    new: &[Parsed],
    cfg: &Config,
    mut fresh: impl FnMut() -> BlockId,
) -> (Vec<Assigned>, ReconcileStats) {
    let cx = Ctx {
        old,
        new,
        osig: old.iter().map(|b| sig(&b.text)).collect(),
        nsig: new.iter().map(|b| sig(&b.text)).collect(),
        cfg,
        stats: Cell::new(ReconcileStats::default()),
    };
    let mut out: Vec<Option<Assigned>> = vec![None; new.len()];
    let mut old_used = vec![false; old.len()];
    let mut src: Vec<Option<usize>> = vec![None; new.len()];
    let give = |out: &mut Vec<Option<Assigned>>,
                used: &mut Vec<bool>,
                o: usize,
                n: usize,
                how: How,
                conf: f32| {
        debug_assert!(!used[o] && out[n].is_none());
        used[o] = true;
        out[n] = Some(Assigned {
            id: old[o].id,
            how,
            confidence: conf,
        });
    };

    // 1. exact hashes along the longest common subsequence.
    let oh: Vec<u64> = old.iter().map(|b| hash_of(&b.text)).collect();
    let nh: Vec<u64> = new.iter().map(|b| hash_of(&b.text)).collect();
    for op in capture_diff_slices(Algorithm::Myers, &oh, &nh) {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        {
            for k in 0..len {
                give(
                    &mut out,
                    &mut old_used,
                    old_index + k,
                    new_index + k,
                    How::Exact,
                    1.0,
                );
                src[new_index + k] = Some(old_index + k);
            }
        }
    }

    // 2. exact hashes out of order = moved blocks (duplicates are consumed in document order).
    let mut by_hash: HashMap<u64, VecDeque<usize>> = HashMap::new();
    for (i, h) in oh.iter().enumerate() {
        if !old_used[i] {
            by_hash.entry(*h).or_default().push_back(i);
        }
    }
    for n in 0..new.len() {
        if out[n].is_none()
            && let Some(o) = by_hash.get_mut(&nh[n]).and_then(|q| q.pop_front())
        {
            give(&mut out, &mut old_used, o, n, How::Moved, 1.0);
            src[n] = Some(o);
        }
    }

    // 2b. identical blocks cannot be told apart by content: choose by what surrounds them.
    refine_duplicates(&oh, &nh, &mut out, &mut old_used, &mut src, old);

    // Anchors: the exact matches that keep the document order (longest increasing run); the rest are moves.
    let pairs: Vec<(usize, usize)> = (0..new.len())
        .filter_map(|n| src[n].map(|o| (o, n)))
        .collect();
    let chain = lis(&pairs.iter().map(|p| p.0).collect::<Vec<_>>());
    let anchors: Vec<(usize, usize)> = chain.iter().map(|&i| pairs[i]).collect();
    let in_chain: std::collections::HashSet<usize> = anchors.iter().map(|a| a.1).collect();
    for &(_, n) in &pairs {
        if let Some(a) = out[n].as_mut() {
            a.how = if in_chain.contains(&n) {
                How::Exact
            } else {
                How::Moved
            };
        }
    }

    // 3. gaps between anchors.
    let mut bounds = vec![(-1i64, -1i64)];
    bounds.extend(anchors.iter().map(|&(o, n)| (o as i64, n as i64)));
    bounds.push((old.len() as i64, new.len() as i64));
    for w in bounds.windows(2) {
        let os: Vec<usize> = ((w[0].0 + 1) as usize..w[1].0 as usize)
            .filter(|&i| !old_used[i])
            .collect();
        let ns: Vec<usize> = ((w[0].1 + 1) as usize..w[1].1 as usize)
            .filter(|&i| out[i].is_none())
            .collect();
        if os.is_empty() || ns.is_empty() {
            continue;
        }
        if split_or_merge(&cx, &os, &ns, &mut out, &mut old_used) {
            continue;
        }
        align(&cx, &os, &ns, &mut out, &mut old_used);
    }

    // 4. edited and moved: best similarity first, across the whole document.
    let lo: Vec<usize> = (0..old.len()).filter(|&i| !old_used[i]).collect();
    let ln: Vec<usize> = (0..new.len()).filter(|&i| out[i].is_none()).collect();
    if !lo.is_empty()
        && !ln.is_empty()
        && lo
            .len()
            .checked_mul(ln.len())
            .is_some_and(|n| n <= 1_000_000)
    {
        let mut cands: Vec<(f32, usize, usize)> = Vec::new();
        for &o in &lo {
            for &n in &ln {
                if let Some(s) = cx.sim(o, n, cfg.high) {
                    cands.push((s, o, n));
                }
            }
        }
        cands.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap()
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        for (s, o, n) in cands {
            if !old_used[o] && out[n].is_none() {
                give(&mut out, &mut old_used, o, n, How::Similar, s);
            }
        }
    }

    // 5. new blocks.
    let stats = cx.stats.get();
    let out = out
        .into_iter()
        .map(|a| {
            a.unwrap_or_else(|| Assigned {
                id: fresh(),
                how: How::Fresh,
                confidence: 1.0,
            })
        })
        .collect();
    (out, stats)
}

/// Indices of a longest strictly increasing subsequence of `a`.
fn lis(a: &[usize]) -> Vec<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; a.len()];
    for i in 0..a.len() {
        let pos = tails.partition_point(|&t| a[t] < a[i]);
        if pos > 0 {
            prev[i] = tails[pos - 1];
        }
        if pos == tails.len() {
            tails.push(i)
        } else {
            tails[pos] = i
        }
    }
    let mut out = Vec::new();
    let mut i = tails.last().copied().unwrap_or(usize::MAX);
    while i != usize::MAX {
        out.push(i);
        i = prev[i];
    }
    out.reverse();
    out
}

/// For each group of identical blocks (same hash) that occurs more than once on either side, redo the pairing
/// of old to new by how many of the immediate neighbours also match, then by closeness of rank.
fn refine_duplicates(
    oh: &[u64],
    nh: &[u64],
    out: &mut [Option<Assigned>],
    used: &mut [bool],
    src: &mut [Option<usize>],
    old: &[Tracked],
) {
    let mut groups: HashMap<u64, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (i, h) in oh.iter().enumerate() {
        groups.entry(*h).or_default().0.push(i);
    }
    for (i, h) in nh.iter().enumerate() {
        groups.entry(*h).or_default().1.push(i);
    }
    let mut keys: Vec<u64> = groups
        .iter()
        .filter(|(_, (o, n))| {
            !o.is_empty()
                && !n.is_empty()
                && (o.len() > 1 || n.len() > 1)
                && o.len().checked_mul(n.len()).is_some_and(|n| n <= 100_000)
        })
        .map(|(h, _)| *h)
        .collect();
    keys.sort();
    for h in keys {
        let (os, ns) = &groups[&h];
        for &n in ns {
            if let Some(o) = src[n]
                && oh[o] == h
            {
                used[o] = false;
                out[n] = None;
                src[n] = None;
            }
        }
        let ctx = |o: usize, n: usize| -> u32 {
            let side = |a: Option<usize>, b: Option<usize>, ha: &[u64], hb: &[u64]| {
                matches!((a, b), (Some(x), Some(y)) if ha[x] == hb[y]) as u32
            };
            2 * (side(o.checked_sub(1), n.checked_sub(1), oh, nh)
                + side(
                    (o + 1 < oh.len()).then_some(o + 1),
                    (n + 1 < nh.len()).then_some(n + 1),
                    oh,
                    nh,
                ))
        };
        let mut cands: Vec<(u32, usize, usize, usize)> = Vec::new();
        for (ro, &o) in os.iter().enumerate() {
            for (rn, &n) in ns.iter().enumerate() {
                cands.push((ctx(o, n), ro.abs_diff(rn), o, n));
            }
        }
        cands.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
                .then(a.3.cmp(&b.3))
        });
        for (_, _, o, n) in cands {
            if !used[o] && out[n].is_none() {
                used[o] = true;
                out[n] = Some(Assigned {
                    id: old[o].id,
                    how: How::Exact,
                    confidence: 1.0,
                });
                src[n] = Some(o);
            }
        }
    }
}

fn concat_sig(cx: &Ctx, idx: &[usize], old_side: bool) -> Sig {
    let mut s = String::new();
    for &i in idx {
        s.push_str(if old_side {
            &cx.old[i].text
        } else {
            &cx.new[i].text
        });
        s.push('\n');
    }
    sig(&s)
}

/// Tokens of `a` that also occur in `b` (multiset intersection size).
fn overlap(a: &Sig, b: &Sig) -> usize {
    let (small, big) = if a.grams.len() <= b.grams.len() {
        (a, b)
    } else {
        (b, a)
    };
    small
        .grams
        .iter()
        .map(|(g, &n)| n.min(*big.grams.get(g).unwrap_or(&0)) as usize)
        .sum()
}

/// A part counts as a fragment of the other side when most of its own tokens come from it.
const FRAGMENT: f32 = 0.6;

/// One old block that became several (split), or several that became one (merge). It only counts when at least
/// two parts are real fragments of the other side AND together they still add up to it; a block that merely sits
/// next to an unrelated new block is an ordinary edit, not a split.
fn split_or_merge(
    cx: &Ctx,
    os: &[usize],
    ns: &[usize],
    out: &mut [Option<Assigned>],
    used: &mut [bool],
) -> bool {
    // "Largest" means the most text, in characters, so the answer does not depend on the script used.
    let choose = |parts: &[(usize, usize)], size: &dyn Fn(usize) -> usize| -> usize {
        match cx.cfg.split {
            SplitPolicy::FirstPart => parts[0].0,
            SplitPolicy::LargestPart => {
                parts
                    .iter()
                    .fold(
                        parts[0],
                        |best, &p| if size(p.0) > size(best.0) { p } else { best },
                    )
                    .0
            }
        }
    };
    if os.len() == 1 && (2..=4).contains(&ns.len()) {
        let o = os[0];
        let frags: Vec<(usize, usize)> = ns
            .iter()
            .map(|&n| (n, overlap(&cx.osig[o], &cx.nsig[n])))
            .filter(|&(n, ov)| {
                cx.nsig[n].total > 0 && ov as f32 / cx.nsig[n].total as f32 >= FRAGMENT
            })
            .collect();
        if frags.len() >= 2 {
            let fr: Vec<usize> = frags.iter().map(|f| f.0).collect();
            if dice(&cx.osig[o], &concat_sig(cx, &fr, false)) >= cx.cfg.high {
                let n = choose(&frags, &|n| cx.nsig[n].norm.len());
                used[o] = true;
                out[n] = Some(Assigned {
                    id: cx.old[o].id,
                    how: How::Split,
                    confidence: 0.8,
                });
                return true;
            }
        }
    }
    if ns.len() == 1 && (2..=4).contains(&os.len()) {
        let n = ns[0];
        let frags: Vec<(usize, usize)> = os
            .iter()
            .map(|&o| (o, overlap(&cx.osig[o], &cx.nsig[n])))
            .filter(|&(o, ov)| {
                cx.osig[o].total > 0 && ov as f32 / cx.osig[o].total as f32 >= FRAGMENT
            })
            .collect();
        if frags.len() >= 2 {
            let fr: Vec<usize> = frags.iter().map(|f| f.0).collect();
            if dice(&concat_sig(cx, &fr, true), &cx.nsig[n]) >= cx.cfg.high {
                let o = choose(&frags, &|o| cx.osig[o].norm.len());
                used[o] = true;
                out[n] = Some(Assigned {
                    id: cx.old[o].id,
                    how: How::Merged,
                    confidence: 0.8,
                });
                return true;
            }
        }
    }
    false
}

/// Order-preserving alignment of the unmatched blocks in one gap that maximizes total similarity.
fn align(cx: &Ctx, os: &[usize], ns: &[usize], out: &mut [Option<Assigned>], used: &mut [bool]) {
    let (a, b) = (os.len(), ns.len());
    if a * b > 250_000 {
        return; // left to the global pass
    }
    let mut s = vec![vec![None::<f32>; b]; a];
    for i in 0..a {
        for j in 0..b {
            s[i][j] = cx.sim(os[i], ns[j], cx.cfg.low);
        }
    }
    let mut best = vec![vec![0f32; b + 1]; a + 1];
    for i in 1..=a {
        for j in 1..=b {
            let diag = s[i - 1][j - 1]
                .map(|v| best[i - 1][j - 1] + v)
                .unwrap_or(f32::MIN);
            best[i][j] = best[i - 1][j].max(best[i][j - 1]).max(diag);
        }
    }
    let (mut i, mut j) = (a, b);
    while i > 0 && j > 0 {
        let diag = s[i - 1][j - 1].map(|v| best[i - 1][j - 1] + v);
        if diag.is_some_and(|d| (d - best[i][j]).abs() < 1e-6) {
            let (o, n) = (os[i - 1], ns[j - 1]);
            used[o] = true;
            out[n] = Some(Assigned {
                id: cx.old[o].id,
                how: How::Similar,
                confidence: s[i - 1][j - 1].unwrap(),
            });
            i -= 1;
            j -= 1;
        } else if best[i - 1][j] >= best[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::parse::parse_blocks;

    fn track(text: &str, first_id: u64) -> Vec<Tracked> {
        parse_blocks(text)
            .into_iter()
            .enumerate()
            .map(|(i, p)| Tracked {
                id: first_id + i as u64,
                kind: p.kind,
                text: p.text,
            })
            .collect()
    }
    fn run(old: &[Tracked], new_text: &str) -> Vec<Assigned> {
        let mut next = 1000;
        reconcile(old, &parse_blocks(new_text), &Config::default(), || {
            next += 1;
            next
        })
    }
    fn ids(a: &[Assigned]) -> Vec<u64> {
        a.iter().map(|x| x.id).collect()
    }

    const BASE: &str = "# 第一章 开端\n\n夜色渐深，城市的灯火一盏盏亮起。他推开门，雨声涌了进来。\n\n第二段讲述了他回到旧居之后发现的事情，一切都与记忆里不同。\n\n结尾段落收束了全章的情绪，留下一个开放的问题。\n";

    #[test]
    fn repeated_tokens_beyond_u16_do_not_overflow() {
        for n in [65_535usize, 65_536, 70_000] {
            for text in ["a ".repeat(n), "中".repeat(n)] {
                let parsed = parse_blocks(&text);
                assert_eq!(parsed.len(), 1, "one paragraph");
                let id = reconcile(&[], &parsed, &Config::default(), || 7);
                assert_eq!(ids(&id), [7]);

                let old = track(&text, 1);
                let mut edited = parsed.clone();
                edited[0].text.push('尾');
                let (assigned, _stats) =
                    reconcile_with_stats(&old, &edited, &Config::default(), || 8);
                assert_eq!(ids(&assigned), [1], "the huge block keeps its id");
            }
        }
    }

    #[test]
    fn a_long_character_diff_is_bounded_and_still_keeps_ids_unique() {
        // 40k characters on both sides: the statistical score is only 0.3, so the reconciler would
        // normally ask the character diff for a second opinion. The work budget must skip it instead.
        let old = track(&"甲".repeat(40_000), 1);
        let new = parse_blocks(&format!("{}{}", "甲".repeat(12_000), "乙".repeat(28_000)));
        let (assigned, stats) = reconcile_with_stats(&old, &new, &Config::default(), || 99);
        assert_eq!(assigned.len(), 1);
        assert!(stats.budget_degraded >= 1, "the oversized pair degrades");
        assert_eq!(stats.char_diff_calls, 0);
        assert!(assigned[0].id == 1 || assigned[0].id == 99);
    }

    #[test]
    fn unchanged_text_keeps_every_id() {
        let old = track(BASE, 1);
        let r = run(&old, BASE);
        assert_eq!(ids(&r), [1, 2, 3, 4]);
        assert!(r.iter().all(|a| a.how == How::Exact));
    }

    #[test]
    fn a_small_edit_keeps_the_id_and_a_large_insertion_before_it_does_not_matter() {
        let old = track(BASE, 1);
        let edited = BASE.replace("留下一个开放的问题", "留下一个开放的问题吗");
        assert_eq!(ids(&run(&old, &edited)), [1, 2, 3, 4]);
        let mut big = String::from("# 序章\n\n");
        for i in 0..40 {
            big.push_str(&format!(
                "序章里的第{i}段，讲述与本章无关的另一件事情，字数不少。\n\n"
            ));
        }
        let r = run(&old, &format!("{big}{BASE}"));
        assert_eq!(
            &ids(&r)[r.len() - 4..],
            [1, 2, 3, 4],
            "the original blocks keep their ids after 41 new blocks"
        );
        assert!(r[..r.len() - 4].iter().all(|a| a.how == How::Fresh));
    }

    #[test]
    fn moved_and_deleted_blocks() {
        let old = track(BASE, 1);
        let blocks = parse_blocks(BASE);
        let moved = format!(
            "{}\n\n{}\n\n{}\n\n{}\n",
            blocks[0].text, blocks[3].text, blocks[1].text, blocks[2].text
        );
        assert_eq!(ids(&run(&old, &moved)), [1, 4, 2, 3]);
        let deleted = format!("{}\n\n{}\n", blocks[0].text, blocks[2].text);
        assert_eq!(ids(&run(&old, &deleted)), [1, 3]);
    }

    #[test]
    fn typing_a_heading_marker_keeps_the_id() {
        let old = track(BASE, 1);
        let r = run(&old, &BASE.replace("第二段讲述", "## 第二段讲述"));
        assert_eq!(ids(&r), [1, 2, 3, 4]);
    }

    #[test]
    fn unrelated_new_text_never_steals_an_id() {
        let old = track(BASE, 1);
        let r = run(
            &old,
            "# 完全不同\n\nA completely different English paragraph about databases.\n\n另一段毫无关系的中文内容，讨论的是烹饪与旅行。\n",
        );
        assert!(r.iter().all(|a| a.how == How::Fresh), "{r:?}");
    }

    #[test]
    fn split_and_merge_follow_the_policy() {
        let old = track(BASE, 1);
        let p = parse_blocks(BASE)[1].text.clone();
        let (a, b) = p.split_once("。他").unwrap();
        let split = BASE.replace(&p, &format!("{a}。\n\n他{b}"));
        let mut next = 1000;
        let largest = reconcile(&old, &parse_blocks(&split), &Config::default(), || {
            next += 1;
            next
        });
        assert_eq!(
            largest.iter().filter(|x| x.id == 2).count(),
            1,
            "the id survives on exactly one part"
        );
        let cfg = Config {
            split: SplitPolicy::FirstPart,
            ..Config::default()
        };
        let first = reconcile(&old, &parse_blocks(&split), &cfg, || {
            next += 1;
            next
        });
        assert_eq!(first[1].id, 2, "first part keeps the id");
        // merge the last two paragraphs
        let blocks = parse_blocks(BASE);
        let merged = format!(
            "{}\n\n{}\n\n{} {}\n",
            blocks[0].text, blocks[1].text, blocks[2].text, blocks[3].text
        );
        let r = reconcile(&old, &parse_blocks(&merged), &cfg, || {
            next += 1;
            next
        });
        assert_eq!(
            ids(&r),
            [1, 2, 3],
            "merged block keeps the first id under FirstPart"
        );
    }

    #[test]
    fn short_blocks_do_not_match_by_chance() {
        let old = track("# 第一章\n\n正文内容足够长，用来占位的一段话。\n", 1);
        let r = run(&old, "# 第二章\n\n正文内容足够长，用来占位的一段话。\n");
        assert_eq!(
            r[0].how,
            How::Fresh,
            "第一章 vs 第二章 share no bigram and are too short to guess"
        );
        assert_eq!(r[1].id, 2);
    }

    #[test]
    fn a_short_block_survives_a_small_edit() {
        let old = track(
            "# 总览\n\n> 的屋行好次开，西那。\n\n正文内容足够长，用来占位的一段话，这里再多写一些文字。\n",
            1,
        );
        let r = run(
            &old,
            "# 总览\n\n> 的次开，西那。\n\n正文内容足够长，用来占位的一段话，这里再多写一些文字。\n",
        );
        assert_eq!(ids(&r), [1, 2, 3]);
    }

    #[test]
    fn half_untouched_sentences_keep_the_id_even_when_the_rest_is_rewritten() {
        let old = track(
            "# 题\n\n第一句话是这样写的，保持不变。第二句话也保持不变，没有任何改动。第三句话原本很长很长，写了许多许多内容在里面，现在它要被整个换掉了。\n",
            1,
        );
        let r = run(
            &old,
            "# 题\n\n第一句话是这样写的，保持不变。第二句话也保持不变，没有任何改动。This is an entirely new third sentence written in English, of a similar length.\n",
        );
        assert_eq!(ids(&r), [1, 2]);
    }

    #[test]
    fn identical_blocks_are_assigned_by_their_surroundings() {
        let old = track(
            "甲段落内容。\n\n重复的一段话，出现在两个地方。\n\n乙段落内容。\n",
            1,
        );
        // a copy of the repeated paragraph is pasted ABOVE the original: the original keeps its neighbours
        let r = run(
            &old,
            "重复的一段话，出现在两个地方。\n\n甲段落内容。\n\n重复的一段话，出现在两个地方。\n\n乙段落内容。\n",
        );
        assert_eq!(
            r[2].id, 2,
            "the block still between 甲 and 乙 is the original"
        );
        assert_eq!(r[0].how, How::Fresh, "the pasted copy is new");
    }

    #[test]
    fn an_edited_block_next_to_an_unrelated_new_block_is_not_a_split() {
        let old = track(
            "# 题\n\n这一段文字比较长，讲述了许多许多内容，我们会在这里改动其中的一个字，其余全部保持原样。\n\n结尾段落，保持不变，用来固定位置。\n",
            1,
        );
        // the paragraph gets one character edit, and a short unrelated English quote appears right after it
        let r = run(
            &old,
            "# 题\n\n这一段文字比较长，讲述了许多许多内容，我们会在这里改动其中的一个字啊，其余全部保持原样。\n\n> An unrelated quotation about databases and nothing else.\n\n结尾段落，保持不变，用来固定位置。\n",
        );
        assert_eq!(r[1].id, 2, "the edited paragraph keeps its id");
        assert_eq!(r[2].how, How::Fresh, "the quote is new");
    }
}
