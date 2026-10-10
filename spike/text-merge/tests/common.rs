//! Shared generator code for the property tests and the report example.
#![allow(dead_code)]

use text_merge_spike::*;

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}

const POOL: &str = "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可她里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长知民样现分将外但些与高意进把法此实回二理美点月明其种声全工话儿者向情部正名定女问力机给等几很业最间新什打便位因重被走电四第门相次东政海口使教西再平真听世气信北少关并内加化由却代军产入先山无雨夜城灯门屋窗河桥路风云光影梦海岛森林星月故事记忆旧居离别归途沉默";
const WORDS: &[&str] = &["system", "chapter", "memory", "river", "design", "story", "window", "light", "quiet", "return", "reader", "draft"];

pub fn cjk_sentence(r: &mut Rng) -> String {
    let chars: Vec<char> = POOL.chars().collect();
    let n = 6 + r.below(14);
    let mut s: String = (0..n).map(|i| { let c = chars[r.below(chars.len())]; if i == n / 2 && r.chance(0.5) { '，' } else { c } }).collect();
    s.push('。');
    s
}
pub fn en_sentence(r: &mut Rng) -> String {
    let n = 5 + r.below(7);
    let w: Vec<&str> = (0..n).map(|_| WORDS[r.below(WORDS.len())]).collect();
    format!("{}.", w.join(" "))
}
/// A chapter: paragraphs that are each ONE long line (as prose is), plus headings and a list.
pub fn chapter(r: &mut Rng, paragraphs: usize) -> (String, Vec<std::ops::Range<usize>>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    for i in 0..paragraphs {
        let start = text.len();
        match r.below(10) {
            0 => text.push_str(&format!("## {}", cjk_sentence(r).trim_end_matches('。'))),
            1 => text.push_str(&format!("- {}\n- {}", cjk_sentence(r), cjk_sentence(r))),
            _ => {
                let k = 2 + r.below(4);
                let s: Vec<String> = (0..k).map(|_| if r.chance(0.2) { en_sentence(r) } else { cjk_sentence(r) }).collect();
                text.push_str(&s.join(if r.chance(0.5) { "" } else { " " }));
            }
        }
        spans.push(start..text.len());
        text.push_str(if i + 1 < paragraphs { "\n\n" } else { "\n" });
    }
    (text, spans)
}

#[derive(Clone, Debug)]
pub struct Edit {
    /// token range of the base
    pub from: usize,
    pub to: usize,
    pub text: String,
}
pub fn wrap(r: &mut Rng) -> String {
    // surrounded by punctuation so the edit cannot fuse with a neighbouring word and change token boundaries
    let w = if r.chance(0.5) { cjk_sentence(r).trim_end_matches('。').to_string() } else { WORDS[r.below(WORDS.len())].to_string() };
    format!("【{w}】")
}
pub fn apply(tokens: &[&str], edits: &[Edit]) -> String {
    let mut out = String::new();
    let mut at = 0;
    for e in edits {
        out.push_str(&tokens[at..e.from].concat());
        out.push_str(&e.text);
        at = e.to;
    }
    out.push_str(&tokens[at..].concat());
    out
}
/// `n` edits at random places of `limit` tokens, at least `gap` untouched tokens apart; sorted.
pub fn disjoint_edits(r: &mut Rng, tokens: &[&str], n: usize, gap: usize, within: std::ops::Range<usize>) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    let mut tries = 0;
    while edits.len() < n && tries < n * 30 && within.len() > 4 {
        tries += 1;
        let from = within.start + r.below(within.len() - 1);
        let kind = r.below(3);
        let len = if kind == 0 { 0 } else { 1 + r.below(3) };
        let to = (from + len).min(within.end);
        let text = if kind == 2 { String::new() } else { wrap(r) };
        if kind == 2 && to == from {
            continue;
        }
        // two edits at one position (even identical ones) are one change to a merge, not two
        let clash = edits.iter().any(|e| (from < e.to + gap && e.from < to + gap) || e.from == from);
        if !clash {
            edits.push(Edit { from, to, text });
            edits.sort_by_key(|e| (e.from, e.to));
        }
    }
    edits
}

pub fn token_ranges_of(text: &str, spans: &[std::ops::Range<usize>]) -> Vec<std::ops::Range<usize>> {
    // map byte spans of paragraphs to token index ranges
    let toks = tokens(text);
    let mut offsets = Vec::with_capacity(toks.len() + 1);
    let mut o = 0;
    for t in &toks {
        offsets.push(o);
        o += t.len();
    }
    offsets.push(o);
    spans.iter().map(|s| offsets.partition_point(|&x| x < s.start)..offsets.partition_point(|&x| x < s.end)).collect()
}



/// Like `disjoint_edits`, but never deletes tokens next to an identical token ("a a" -> delete the first or the
/// second?): there the two sides can describe the SAME net change with different edits, and the union of the
/// generating edits stops being the one right answer.
pub fn unambiguous_edits(r: &mut Rng, tokens: &[&str], n: usize, gap: usize, within: std::ops::Range<usize>) -> Vec<Edit> {
    disjoint_edits(r, tokens, n * 2, gap, within)
        .into_iter()
        .filter(|e| e.text.is_empty() && e.to > e.from && (e.from > 0 && tokens[e.from - 1] == tokens[e.to - 1] || e.to < tokens.len() && tokens[e.from] == tokens[e.to]) == false || !e.text.is_empty())
        .take(n)
        .collect()
}

#[derive(Default, Debug)]
pub struct Campaign {
    pub total: u64,
    pub clean: u64,
    pub conflicts: u64,
    /// a clean merge that is not the union of the edits (edits chosen so the union is the one right answer)
    pub wrong: u64,
    /// merging a merged text with a side that is already contained in it: changed the text, or reported a conflict
    pub remerge_changed: u64,
    pub remerge_conflict: u64,
    pub remerge_total: u64,
    pub first_problem: Option<String>,
}

/// `cases` random merges. Phase 1 uses unambiguous edits and an oracle; phase 2 uses arbitrary edits (including
/// ones where equal tokens make the diff ambiguous) and checks the re-merge invariant, which needs no oracle.
pub fn campaign(cases: u64, base_seed: u64, merge: &dyn Fn(&str, &str, &str) -> text_merge_spike::Merge) -> Campaign {
    use text_merge_spike::tokens;
    let mut c = Campaign::default();
    for seed in 0..cases {
        let mut r = Rng(base_seed + seed * 7919);
        let n = 2 + r.below(8);
        let (base, _) = chapter(&mut r, n);
        let bt = tokens(&base);
        let gap = r.below(6);
        let k = 2 + r.below(10);
        let all = unambiguous_edits(&mut r, &bt, k, gap, 0..bt.len());
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for e in all.iter().cloned() { if r.chance(0.5) { a.push(e) } else { b.push(e) } }
        let (ours, theirs, want) = (apply(&bt, &a), apply(&bt, &b), apply(&bt, &all));
        c.total += 1;
        match merge(&base, &ours, &theirs).text() {
            Some(t) => {
                c.clean += 1;
                if t != want {
                    c.wrong += 1;
                    c.first_problem.get_or_insert(format!("seed {seed}: clean merge is not the union of the edits\n ours {a:?}\n theirs {b:?}"));
                }
            }
            None => c.conflicts += 1,
        }
        // phase 2: any edits
        let (ka, kb) = (1 + r.below(6), 1 + r.below(6));
        let ea = disjoint_edits(&mut r, &bt, ka, 0, 0..bt.len());
        let eb = disjoint_edits(&mut r, &bt, kb, 0, 0..bt.len());
        let (ours, theirs) = (apply(&bt, &ea), apply(&bt, &eb));
        if let Some(text) = merge(&base, &ours, &theirs).text() {
            for side in [&ours, &theirs] {
                c.remerge_total += 1;
                match merge(&base, side, &text).text() {
                    Some(x) if x != text => {
                        c.remerge_changed += 1;
                        c.first_problem.get_or_insert(format!("seed {seed}: a clean re-merge changed the text"));
                    }
                    None => c.remerge_conflict += 1,
                    _ => {}
                }
            }
        }
    }
    c
}
