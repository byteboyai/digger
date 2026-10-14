//! A document model with ground truth: every block has a true identity that follows it through edits,
//! so the reconciler's answer can be scored. Operations are applied to structured content, so syntax stays
//! valid; the document is rendered to Markdown text, parsed again, and reconciled as a Commit would.

use digger_editor::blocks::parse::parse_blocks;
use digger_editor::blocks::reconcile::{Assigned, BlockId, Config, How, Tracked, reconcile};
use std::collections::{HashMap, HashSet};

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
    fn zipf(&mut self, n: usize) -> usize {
        let u = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        ((u * u) * n as f64) as usize % n
    }
}

const POOL: &str = "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可她里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长知民样现分将外但些与高意进把法此实回二理美点月明其种声全工话儿者向情部正名定女问力机给等几很业最间新什打便位因重被走电四第门相次东政海口使教西再平真听世气信北少关并内加化由却代军产入先山无雨夜城灯门屋窗河桥路风云光影梦海岛森林星月故事记忆旧居离别归途沉默";
const FUNCTION_WORDS: &[&str] = &["the", "of", "and", "to", "in", "a", "that", "is", "was", "for", "it", "with", "as", "his", "on", "be", "at", "by", "had", "not"];
const ONSET: &[&str] = &["b", "c", "d", "f", "g", "h", "l", "m", "n", "p", "r", "s", "t", "v", "w", "st", "tr", "pl", "ch", "sh"];
const VOWEL: &[&str] = &["a", "e", "i", "o", "u", "ai", "ea", "ou", "io"];
const CODA: &[&str] = &["", "", "n", "r", "s", "t", "l", "nd", "st", "ng"];
fn en_word(r: &mut Rng) -> String {
    if r.chance(0.35) { return FUNCTION_WORDS[r.below(FUNCTION_WORDS.len())].to_string(); }
    let mut w = String::new();
    for _ in 0..1 + r.below(3) { w.push_str(ONSET[r.below(ONSET.len())]); w.push_str(VOWEL[r.below(VOWEL.len())]); }
    w.push_str(CODA[r.below(CODA.len())]);
    w
}

fn word(r: &mut Rng) -> String {
    let chars: Vec<char> = POOL.chars().collect();
    let n = 1 + r.below(3);
    (0..n).map(|_| chars[r.zipf(chars.len())]).collect()
}
pub fn sentence(r: &mut Rng) -> String {
    if r.chance(0.12) {
        let n = 6 + r.below(8);
        let w: Vec<String> = (0..n).map(|_| en_word(r)).collect();
        return format!("{}.", w.join(" "));
    }
    let n = 5 + r.below(9);
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&word(r));
        if i + 1 < n && r.chance(0.18) {
            s.push('，');
        }
    }
    s.push('。');
    s
}
/// A sentence from a small family of near-identical templates, to provoke look-alike blocks.
fn templated(r: &mut Rng) -> String {
    let step = 1 + r.below(3);
    format!("第{step}步：检查{}并确认{}，然后继续处理{}。", word(r), word(r), word(r))
}

#[derive(Clone, Debug)]
pub enum Content {
    Heading { level: u8, text: String, setext: bool },
    Para { sentences: Vec<String>, wrap: bool },
    List { ordered: bool, items: Vec<String>, marker: u8 },
    Quote { sentences: Vec<String> },
    Code { body: String },
    Table { rows: Vec<[String; 2]> },
    Rule,
    Image { alt: String },
}

impl Content {
    pub fn render(&self) -> String {
        match self {
            Content::Heading { level, text, setext } if *setext && *level <= 2 => format!("{text}\n{}", if *level == 1 { "=====" } else { "-----" }),
            Content::Heading { level, text, .. } => format!("{} {text}", "#".repeat(*level as usize)),
            Content::Para { sentences, wrap } => sentences.join(if *wrap { "\n" } else { " " }),
            Content::List { ordered, items, marker } => items
                .iter()
                .enumerate()
                .map(|(i, t)| if *ordered { format!("{}{} {t}", i + 1, if *marker % 2 == 0 { "." } else { ")" }) } else { format!("{} {t}", ["-", "*", "+"][*marker as usize % 3]) })
                .collect::<Vec<_>>()
                .join("\n"),
            Content::Quote { sentences } => sentences.iter().map(|s| format!("> {s}")).collect::<Vec<_>>().join("\n"),
            Content::Code { body } => format!("```\n{body}\n```"),
            Content::Table { rows } => {
                let mut s = String::from("| 列A | 列B |\n|-----|-----|");
                for [a, b] in rows {
                    s.push_str(&format!("\n| {a} | {b} |"));
                }
                s
            }
            Content::Rule => "---".into(),
            Content::Image { alt } => format!("![{alt}](assets/{}.png)", alt.chars().count()),
        }
    }
    fn texts_mut(&mut self) -> Vec<&mut String> {
        match self {
            Content::Heading { text, .. } => vec![text],
            Content::Para { sentences, .. } | Content::Quote { sentences } => sentences.iter_mut().collect(),
            Content::List { items, .. } => items.iter_mut().collect(),
            Content::Table { rows } => rows.iter_mut().flat_map(|r| r.iter_mut()).collect(),
            Content::Code { body } => vec![body],
            _ => vec![],
        }
    }
}

pub fn random_content(r: &mut Rng, confusable: f64) -> Content {
    let mk = |r: &mut Rng| if r.chance(confusable) { templated(r) } else { sentence(r) };
    match r.below(100) {
        0..=49 => Content::Para { sentences: (0..2 + r.below(4)).map(|_| mk(r)).collect(), wrap: r.chance(0.2) },
        50..=61 => Content::Heading { level: 1 + r.below(3) as u8, text: mk(r).trim_end_matches(['。', '.']).to_string(), setext: r.chance(0.2) },
        62..=73 => Content::List { ordered: r.chance(0.4), items: (0..2 + r.below(4)).map(|_| mk(r)).collect(), marker: r.below(3) as u8 },
        74..=79 => Content::Quote { sentences: (0..1 + r.below(3)).map(|_| mk(r)).collect() },
        80..=85 => Content::Code { body: (0..2 + r.below(4)).map(|_| format!("let {} = {};", en_word(&mut *r), r.below(100))).collect::<Vec<_>>().join("\n") },
        86..=89 => Content::Table { rows: (0..2 + r.below(3)).map(|_| [word(r), word(r)]).collect() },
        90..=92 => Content::Rule,
        _ => Content::Image { alt: format!("{}{}", word(r), word(r)) },
    }
}

#[derive(Clone, Debug)]
pub struct TrueBlock {
    pub tid: u64,
    pub c: Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    TypeSmall,
    EditMedium,
    RewriteLarge,
    InsertBlock,
    InsertSection,
    DeleteBlock,
    MoveBlock,
    MoveAndEdit,
    KindChange,
    Duplicate,
    Reflow,
}

#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub commits: u64,
    /// blocks that existed before, were not rewritten wholesale, and so must keep their id
    pub expected: u64,
    pub kept: u64,
    pub lost: u64,
    /// lost although at least half of its text survived (a real miss) / although less than half did (a rewrite)
    pub lost_clear: u64,
    pub lost_weak: u64,
    pub lost_dup: u64,
    /// an expected block got the id of a *different* block (the dangerous failure)
    pub misattributed: u64,
    pub heavy: u64,
    pub heavy_kept: u64,
    pub heavy_misattributed: u64,
    pub new_blocks: u64,
    /// a new block took an id that still belongs to a live block
    pub stolen: u64,
    /// failures that involve a block whose text is identical to another block's: which copy is "the original" cannot be known
    pub dup_misattributed: u64,
    pub dup_stolen: u64,
    pub samples: Vec<String>,
    /// a new block took the id of a block that was deleted (an unrelated block inherits its history)
    pub inherited: u64,
    pub violations: Vec<String>,
    pub how: HashMap<String, u64>,
    pub op_counts: HashMap<String, u64>,
}

impl Stats {
    pub fn merge(&mut self, o: &Stats) {
        self.commits += o.commits;
        self.expected += o.expected;
        self.kept += o.kept;
        self.lost += o.lost;
        self.lost_clear += o.lost_clear;
        self.lost_weak += o.lost_weak;
        self.lost_dup += o.lost_dup;
        self.misattributed += o.misattributed;
        self.heavy += o.heavy;
        self.heavy_kept += o.heavy_kept;
        self.heavy_misattributed += o.heavy_misattributed;
        self.new_blocks += o.new_blocks;
        self.stolen += o.stolen;
        self.dup_misattributed += o.dup_misattributed;
        self.dup_stolen += o.dup_stolen;
        for x in &o.samples { if self.samples.len() < 12 { self.samples.push(x.clone()); } }
        self.inherited += o.inherited;
        self.violations.extend(o.violations.iter().cloned());
        for (k, v) in &o.how { *self.how.entry(k.clone()).or_default() += v; }
        for (k, v) in &o.op_counts { *self.op_counts.entry(k.clone()).or_default() += v; }
    }
    pub fn survival(&self) -> f64 { self.kept as f64 / self.expected.max(1) as f64 }
}

pub struct Sim {
    pub rng: Rng,
    pub blocks: Vec<TrueBlock>,
    pub cfg: Config,
    pub confusable: f64,
    next_tid: u64,
    next_id: u64,
    tracked: Vec<Tracked>,
    snapshot: HashMap<u64, BlockId>,
    heavy: HashSet<u64>,
    batch_ops: Vec<String>,
    pub stats: Stats,
}

/// Independent estimate of how much of a block's text survived an edit: the larger of the character edit ratio
/// and the fraction of unchanged sentences/lines.
fn keep_fraction(old: &str, new: &str) -> f64 {
    let ratio = similar::TextDiff::from_chars(old, new).ratio() as f64;
    let units = |t: &str| -> Vec<String> { t.lines().flat_map(|l| l.split_inclusive(['。', '.']).map(|u| u.trim().to_string()).collect::<Vec<_>>()).filter(|u| u.chars().count() >= 3).collect() };
    let (a, b) = (units(old), units(new));
    let shared = a.iter().filter(|u| b.contains(u)).count();
    let frac = shared as f64 / a.len().max(b.len()).max(1) as f64;
    ratio.max(frac)
}

fn short(s: &str) -> String { s.chars().take(70).collect::<String>().replace('\n', "⏎") }

fn parsed_text(parsed: &[digger_editor::blocks::parse::Parsed], blocks: &[TrueBlock], tid: u64) -> Option<String> {
    blocks.iter().position(|b| b.tid == tid).map(|i| parsed[i].text.clone())
}

fn join(blocks: &[TrueBlock]) -> String {
    let mut s = blocks.iter().map(|b| b.c.render()).collect::<Vec<_>>().join("\n\n");
    s.push('\n');
    s
}

impl Sim {
    /// A fresh document of about `n` blocks, committed once so every block has an id.
    pub fn new(seed: u64, n: usize, cfg: Config, confusable: f64) -> Sim {
        let mut s = Sim { rng: Rng(seed | 1), blocks: vec![], cfg, confusable, next_tid: 1, next_id: 1, tracked: vec![], snapshot: HashMap::new(), heavy: HashSet::new(), batch_ops: vec![], stats: Stats::default() };
        while s.blocks.len() < n {
            let c = random_content(&mut s.rng, confusable);
            s.blocks.push(TrueBlock { tid: s.next_tid, c });
            s.next_tid += 1;
            if parse_blocks(&join(&s.blocks)).len() != s.blocks.len() {
                s.blocks.pop(); // two lists or quotes merged: drop the block that caused it
            }
        }
        s.commit_inner(false).expect("the generated document parses to its own blocks");
        s
    }

    pub fn text(&self) -> String { join(&self.blocks) }
    pub fn block_ids(&self) -> Vec<BlockId> { self.tracked.iter().map(|t| t.id).collect() }
    pub fn tid_ids(&self) -> &HashMap<u64, BlockId> { &self.snapshot }

    fn fresh_tid(&mut self) -> u64 { self.next_tid += 1; self.next_tid - 1 }

    fn valid(&self, blocks: &[TrueBlock]) -> bool { parse_blocks(&join(blocks)).len() == blocks.len() }

    fn pick_text_block(&mut self) -> Option<usize> {
        let idx: Vec<usize> = (0..self.blocks.len()).filter(|&i| !matches!(self.blocks[i].c, Content::Rule | Content::Image { .. } | Content::Code { .. })).collect();
        if idx.is_empty() { None } else { Some(idx[self.rng.below(idx.len())]) }
    }

    fn type_small(&mut self, i: usize) {
        let chars: Vec<char> = POOL.chars().collect();
        let mut b = self.blocks[i].clone();
        let r = &mut self.rng;
        let mut texts = b.c.texts_mut();
        if texts.is_empty() { return; }
        let k = r.below(texts.len());
        let t = &mut *texts[k];
        let cs: Vec<char> = t.chars().collect();
        // never touch the final punctuation or syntax: edit inside the text
        let lo = 0usize;
        let hi = cs.len().saturating_sub(2).max(1);
        let at = lo + r.below(hi);
        let mut v = cs.clone();
        if r.chance(0.6) || v.len() < 8 {
            for _ in 0..1 + r.below(3) { v.insert(at.min(v.len()), chars[r.zipf(chars.len())]); }
        } else {
            for _ in 0..1 + r.below(3) { if v.len() > 6 && at < v.len() { v.remove(at); } }
        }
        *t = v.into_iter().collect();
        self.blocks[i] = b;
    }

    fn edit_medium(&mut self, i: usize) {
        let confusable = self.confusable;
        let mut b = self.blocks[i].clone();
        let r = &mut self.rng;
        match &mut b.c {
            Content::Para { sentences, .. } | Content::Quote { sentences } if sentences.len() >= 3 => {
                let k = r.below(sentences.len());
                sentences[k] = if r.chance(confusable) { templated(r) } else { sentence(r) };
            }
            Content::List { items, .. } if items.len() >= 3 => {
                let k = r.below(items.len());
                items[k] = sentence(r);
            }
            _ => { self.type_small(i); return; }
        }
        self.blocks[i] = b;
    }

    fn rewrite_large(&mut self, i: usize) {
        let mut b = self.blocks[i].clone();
        let r = &mut self.rng;
        match &mut b.c {
            Content::Para { sentences, .. } | Content::Quote { sentences } => { for s in sentences.iter_mut() { *s = sentence(r); } }
            Content::List { items, .. } => { for s in items.iter_mut() { *s = sentence(r); } }
            Content::Heading { text, .. } => *text = sentence(r).trim_end_matches('。').to_string(),
            _ => return,
        }
        self.heavy.insert(b.tid);
        self.blocks[i] = b;
    }

    fn reflow(&mut self, i: usize) {
        match &mut self.blocks[i].c {
            Content::Para { wrap, .. } => *wrap = !*wrap,
            Content::List { marker, .. } => *marker = (*marker + 1) % 3,
            Content::Heading { setext, level, .. } if *level <= 2 => *setext = !*setext,
            _ => {}
        }
    }

    /// Apply one operation; returns false (and changes nothing) if it would break the block structure.
    pub fn apply(&mut self, op: Op) -> bool {
        let before = self.blocks.clone();
        let before_heavy = self.heavy.clone();
        let n = self.blocks.len();
        let ok = match op {
            Op::TypeSmall => self.pick_text_block().map(|i| self.type_small(i)).is_some(),
            Op::EditMedium => self.pick_text_block().map(|i| self.edit_medium(i)).is_some(),
            Op::RewriteLarge => self.pick_text_block().map(|i| self.rewrite_large(i)).is_some(),
            Op::InsertBlock | Op::InsertSection => {
                let count = if op == Op::InsertSection { 5 + self.rng.below(26) } else { 1 };
                let at = self.rng.below(n + 1);
                for k in 0..count {
                    let c = random_content(&mut self.rng, self.confusable);
                    let tid = self.fresh_tid();
                    self.blocks.insert((at + k).min(self.blocks.len()), TrueBlock { tid, c });
                }
                true
            }
            Op::DeleteBlock if n > 3 => { let i = self.rng.below(n); self.blocks.remove(i); true }
            Op::MoveBlock | Op::MoveAndEdit if n > 3 => {
                let i = self.rng.below(n);
                let b = self.blocks.remove(i);
                let at = self.rng.below(self.blocks.len() + 1);
                self.blocks.insert(at, b);
                if op == Op::MoveAndEdit { if matches!(self.blocks[at].c, Content::Para { .. } | Content::List { .. } | Content::Quote { .. }) { self.type_small(at); } }
                true
            }
            Op::KindChange => {
                let idx: Vec<usize> = (0..n).filter(|&i| matches!(self.blocks[i].c, Content::Para { .. } | Content::Heading { .. })).collect();
                if idx.is_empty() { false } else {
                    let i = idx[self.rng.below(idx.len())];
                    let new = match &self.blocks[i].c {
                        Content::Para { sentences, .. } => Content::Heading { level: 2, text: sentences.join(" ").trim_end_matches(['。', '.']).to_string(), setext: false },
                        Content::Heading { text, .. } => Content::Para { sentences: vec![format!("{text}。")], wrap: false },
                        _ => unreachable!(),
                    };
                    self.blocks[i].c = new;
                    true
                }
            }
            Op::Duplicate => {
                let i = self.rng.below(n);
                let c = self.blocks[i].c.clone();
                let tid = self.fresh_tid();
                let at = self.rng.below(n + 1);
                self.blocks.insert(at, TrueBlock { tid, c });
                true
            }
            Op::Reflow => { let i = self.rng.below(n); self.reflow(i); true }
            _ => false,
        };
        if !ok || !self.valid(&self.blocks) {
            self.blocks = before;
            self.heavy = before_heavy;
            return false;
        }
        *self.stats.op_counts.entry(format!("{op:?}")).or_default() += 1;
        self.batch_ops.push(format!("{op:?}"));
        true
    }

    /// Apply `Reflow` to every block that supports it (a "format document" pass).
    pub fn format_all(&mut self) {
        for i in 0..self.blocks.len() { self.reflow(i); }
        if !self.valid(&self.blocks) { for i in 0..self.blocks.len() { self.reflow(i); } }
    }

    /// Insert a section of `count` new blocks (thousands of characters) at `at`.
    pub fn insert_section_at(&mut self, at: usize, count: usize) {
        for k in 0..count {
            let c = random_content(&mut self.rng, self.confusable);
            let tid = self.fresh_tid();
            self.blocks.insert(at + k, TrueBlock { tid, c });
        }
        if !self.valid(&self.blocks) { for k in (0..count).rev() { self.blocks.remove(at + k); } }
    }

    /// Split paragraph `i` into two at a sentence boundary; returns (first tid, second tid, chars first, chars second).
    pub fn split_paragraph(&mut self, i: usize) -> Option<(u64, u64, usize, usize)> {
        let Content::Para { sentences, wrap } = self.blocks[i].c.clone() else { return None };
        if sentences.len() < 2 { return None; }
        let k = 1 + self.rng.below(sentences.len() - 1);
        let (a, b) = sentences.split_at(k);
        let tid2 = self.fresh_tid();
        let first = self.blocks[i].tid;
        let (la, lb) = (a.iter().map(|s| s.chars().count()).sum(), b.iter().map(|s| s.chars().count()).sum());
        self.blocks[i].c = Content::Para { sentences: a.to_vec(), wrap };
        self.blocks.insert(i + 1, TrueBlock { tid: tid2, c: Content::Para { sentences: b.to_vec(), wrap } });
        Some((first, tid2, la, lb))
    }

    /// Merge paragraphs `i` and `i+1` into one (the first keeps its tid).
    pub fn merge_paragraphs(&mut self, i: usize) -> Option<(u64, u64)> {
        let (Content::Para { sentences: a, wrap }, Content::Para { sentences: b, .. }) = (self.blocks[i].c.clone(), self.blocks[i + 1].c.clone()) else { return None };
        let (t1, t2) = (self.blocks[i].tid, self.blocks[i + 1].tid);
        let mut s = a;
        s.extend(b);
        self.blocks[i].c = Content::Para { sentences: s, wrap };
        self.blocks.remove(i + 1);
        Some((t1, t2))
    }

    pub fn commit(&mut self) -> Result<(), String> { self.commit_inner(true) }

    fn commit_inner(&mut self, score: bool) -> Result<(), String> {
        let text = join(&self.blocks);
        let parsed = parse_blocks(&text);
        if parsed.len() != self.blocks.len() {
            return Err(format!("parsed {} blocks, expected {}", parsed.len(), self.blocks.len()));
        }
        let mut next = self.next_id;
        let assigned: Vec<Assigned> = reconcile(&self.tracked, &parsed, &self.cfg, || { next += 1; next - 1 });
        // determinism: the same inputs give the same ids
        let mut next2 = self.next_id;
        let again = reconcile(&self.tracked, &parsed, &self.cfg, || { next2 += 1; next2 - 1 });
        if assigned.iter().map(|a| a.id).ne(again.iter().map(|a| a.id)) {
            self.stats.violations.push("reconcile is not deterministic".into());
        }
        // uniqueness
        let mut seen = HashSet::new();
        for a in &assigned {
            if !seen.insert(a.id) { self.stats.violations.push(format!("duplicate id {}", a.id)); }
        }
        if score {
            self.stats.commits += 1;
            let old_ids: HashSet<BlockId> = self.tracked.iter().map(|t| t.id).collect();
            let owner: HashMap<BlockId, u64> = self.snapshot.iter().map(|(t, id)| (*id, *t)).collect();
            let alive: HashSet<u64> = self.blocks.iter().map(|b| b.tid).collect();
            let mut count: HashMap<&str, u32> = HashMap::new();
            for p in &parsed { *count.entry(p.text.as_str()).or_default() += 1; }
            for t in &self.tracked { *count.entry(t.text.as_str()).or_default() += 1; }
            let ops = self.batch_ops.join(",");
            let copied = self.batch_ops.iter().any(|o| o == "Duplicate");
            let heavy_ids: HashSet<BlockId> = self.snapshot.iter().filter(|(t, _)| self.heavy.contains(t)).map(|(_, id)| *id).collect();
            for (b, a) in self.blocks.iter().zip(&assigned) {
                *self.stats.how.entry(format!("{:?}", a.how)).or_default() += 1;
                match self.snapshot.get(&b.tid) {
                    Some(&want) if !self.heavy.contains(&b.tid) => {
                        self.stats.expected += 1;
                        if a.id == want { self.stats.kept += 1; }
                        else if old_ids.contains(&a.id) {
                            self.stats.misattributed += 1;
                            let dup = copied || heavy_ids.contains(&a.id) || parsed_text(&parsed, &self.blocks, b.tid).is_some_and(|t| count.get(t.as_str()).copied().unwrap_or(0) > 2);
                            if dup { self.stats.dup_misattributed += 1; }
                            else if self.stats.samples.len() < 12 { self.stats.samples.push(format!("MISATTRIBUTED [{ops}] how={:?} block={:?}", a.how, short(&b.c.render()))); }
                        } else {
                            self.stats.lost += 1;
                            let before_full = self.tracked.iter().find(|t| t.id == want).map(|t| t.text.clone()).unwrap_or_default();
                            let now = b.c.render();
                            let keep = keep_fraction(&before_full, &now);
                            let dup = copied || count.get(parsed[self.blocks.iter().position(|x| x.tid == b.tid).unwrap()].text.as_str()).copied().unwrap_or(0) > 2;
                            if dup { self.stats.lost_dup += 1; } else if keep >= 0.5 { self.stats.lost_clear += 1; } else { self.stats.lost_weak += 1; }
                            let holder = assigned.iter().position(|x| x.id == want).map(|j| short(&parsed[j].text));
                            if !dup && keep >= 0.9 && std::env::var("DIAG").is_ok() {
                                let j = assigned.iter().position(|x| x.id == want);
                                eprintln!("DIAG lost block (kept {:.0}%): ops=[{ops}]\n  old text = {before_full:?}\n  new text = {now:?}\n  holder idx {j:?} how={:?}\n  holder old-text = {:?}\n  holder new-text = {:?}\n  all hows of this commit: {:?}",
                                    keep * 100.0, j.map(|j| assigned[j].how), self.tracked.iter().find(|t| t.id == want).map(|t| t.text.clone()), j.map(|j| parsed[j].text.clone()),
                                    assigned.iter().map(|a| format!("{:?}", a.how)).collect::<Vec<_>>().join(","));
                                std::process::exit(3);
                            }
                            if !dup && keep >= 0.5 && self.stats.samples.len() < 12 {
                                self.stats.samples.push(format!("LOST (kept {:.0}% of its text) [{ops}] {:?}\n        was {:?}\n        now {:?}\n        id went to {holder:?}; copies of this text: old {} new {}",
                                    keep * 100.0, a.how, short(&before_full), short(&now), self.tracked.iter().filter(|t| t.text == now).count(), parsed.iter().filter(|t| t.text == now).count()));
                            }
                        }
                    }
                    Some(&want) => {
                        self.stats.heavy += 1;
                        if a.id == want { self.stats.heavy_kept += 1; }
                        else if old_ids.contains(&a.id) { self.stats.heavy_misattributed += 1; }
                    }
                    None => {
                        self.stats.new_blocks += 1;
                        if old_ids.contains(&a.id) {
                            if owner.get(&a.id).is_some_and(|t| alive.contains(t)) {
                                self.stats.stolen += 1;
                                let dup = copied || heavy_ids.contains(&a.id) || parsed_text(&parsed, &self.blocks, b.tid).is_some_and(|t| count.get(t.as_str()).copied().unwrap_or(0) > 2);
                                if dup { self.stats.dup_stolen += 1; }
                                else if self.stats.samples.len() < 12 { self.stats.samples.push(format!("STOLEN [{ops}] how={:?} block={:?}", a.how, short(&b.c.render()))); }
                            } else { self.stats.inherited += 1; }
                        }
                    }
                }
            }
        }
        self.next_id = next;
        self.tracked = parsed.iter().zip(&assigned).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
        self.snapshot = self.blocks.iter().zip(&assigned).map(|(b, a)| (b.tid, a.id)).collect();
        self.heavy.clear();
        self.batch_ops.clear();
        // idempotence: reconciling the same text again changes nothing
        let rerun = reconcile(&self.tracked, &parsed, &self.cfg, || 0);
        if rerun.iter().zip(&self.tracked).any(|(r, t)| r.id != t.id || r.how != How::Exact) {
            self.stats.violations.push("committing unchanged text changed an id".into());
        }
        Ok(())
    }
}

pub struct Mix(pub Vec<(Op, u32)>);

impl Mix {
    pub fn typing() -> Mix { Mix(vec![(Op::TypeSmall, 80), (Op::EditMedium, 8), (Op::InsertBlock, 7), (Op::DeleteBlock, 3), (Op::Reflow, 2)]) }
    pub fn editing() -> Mix {
        Mix(vec![(Op::TypeSmall, 30), (Op::EditMedium, 15), (Op::InsertBlock, 10), (Op::DeleteBlock, 6), (Op::MoveBlock, 8), (Op::MoveAndEdit, 6), (Op::KindChange, 5), (Op::Duplicate, 4), (Op::Reflow, 8), (Op::InsertSection, 3)])
    }
    pub fn agent() -> Mix {
        Mix(vec![(Op::RewriteLarge, 25), (Op::EditMedium, 25), (Op::InsertSection, 10), (Op::MoveBlock, 10), (Op::DeleteBlock, 10), (Op::KindChange, 10), (Op::InsertBlock, 10)])
    }
    pub fn restructure() -> Mix {
        Mix(vec![(Op::MoveBlock, 30), (Op::MoveAndEdit, 25), (Op::InsertSection, 15), (Op::DeleteBlock, 15), (Op::Duplicate, 15)])
    }
    pub fn pick(&self, r: &mut Rng) -> Op {
        let total: u32 = self.0.iter().map(|(_, w)| w).sum();
        let mut x = r.below(total as usize) as u32;
        for (op, w) in &self.0 {
            if x < *w { return *op; }
            x -= w;
        }
        self.0[0].0
    }
}

/// `commits` commits of `batch` operations each, on a document of `n` blocks.
pub fn stream(seed: u64, n: usize, commits: usize, batch: usize, mix: &Mix, cfg: Config, confusable: f64) -> Stats {
    let mut sim = Sim::new(seed, n, cfg, confusable);
    for _ in 0..commits {
        let mut applied = 0;
        let mut tries = 0;
        while applied < batch && tries < batch * 8 {
            tries += 1;
            let op = mix.pick(&mut sim.rng);
            if sim.apply(op) { applied += 1; }
        }
        if let Err(e) = sim.commit() { sim.stats.violations.push(e); break; }
    }
    sim.stats
}

// ---------------------------------------------------------------------------------------------------------
// Dedicated scenarios

#[derive(Debug, Clone, Copy)]
pub struct SplitOutcome {
    /// the old id appears on exactly one of the resulting blocks
    pub on_exactly_one: bool,
    pub on_first: bool,
    pub on_largest: bool,
}

fn paragraph_indices(sim: &Sim, min_sentences: usize) -> Vec<usize> {
    (0..sim.blocks.len()).filter(|&i| matches!(&sim.blocks[i].c, Content::Para { sentences, .. } if sentences.len() >= min_sentences)).collect()
}

/// Split one paragraph at a sentence boundary in a 40-block chapter and see where its id went.
pub fn split_trial(seed: u64, cfg: Config) -> Option<SplitOutcome> {
    let mut sim = Sim::new(seed, 40, cfg, 0.0);
    let cands = paragraph_indices(&sim, 2);
    if cands.is_empty() { return None; }
    let i = cands[sim.rng.below(cands.len())];
    let old = sim.block_ids()[i];
    let (_, _, la, lb) = sim.split_paragraph(i)?;
    if sim.commit().is_err() { return None; }
    let ids = sim.block_ids();
    let holders: Vec<usize> = [i, i + 1].into_iter().filter(|&k| ids[k] == old).collect();
    Some(SplitOutcome { on_exactly_one: holders.len() == 1, on_first: holders == [i], on_largest: holders == [if la >= lb { i } else { i + 1 }] })
}

/// Merge two adjacent paragraphs.
pub fn merge_trial(seed: u64, cfg: Config) -> Option<SplitOutcome> {
    let mut sim = Sim::new(seed, 40, cfg, 0.0);
    let adj: Vec<usize> = (0..sim.blocks.len().saturating_sub(1)).filter(|&i| matches!((&sim.blocks[i].c, &sim.blocks[i + 1].c), (Content::Para { .. }, Content::Para { .. }))).collect();
    if adj.is_empty() { return None; }
    let i = adj[sim.rng.below(adj.len())];
    let (id1, id2) = (sim.block_ids()[i], sim.block_ids()[i + 1]);
    let (l1, l2) = match (&sim.blocks[i].c, &sim.blocks[i + 1].c) {
        (Content::Para { sentences: a, .. }, Content::Para { sentences: b, .. }) => (a.iter().map(|s| s.chars().count()).sum::<usize>(), b.iter().map(|s| s.chars().count()).sum::<usize>()),
        _ => unreachable!(),
    };
    sim.merge_paragraphs(i)?;
    if sim.commit().is_err() { return None; }
    let got = sim.block_ids()[i];
    Some(SplitOutcome { on_exactly_one: got == id1 || got == id2, on_first: got == id1, on_largest: got == if l1 >= l2 { id1 } else { id2 } })
}

/// Insert a 30-block section (thousands of characters) at a random place; do all original blocks keep their ids?
pub fn insertion_trial(seed: u64, cfg: Config) -> Option<(usize, usize)> {
    let mut sim = Sim::new(seed, 60, cfg, 0.0);
    let before = sim.tid_ids().clone();
    let at = sim.rng.below(sim.blocks.len() + 1);
    let n_before = sim.blocks.len();
    sim.insert_section_at(at, 30);
    if sim.blocks.len() == n_before || sim.commit().is_err() { return None; }
    let after = sim.tid_ids();
    let kept = before.iter().filter(|(t, id)| after.get(t) == Some(id)).count();
    Some((kept, before.len()))
}

/// "Format document": every block is rewritten in another but equivalent Markdown style.
pub fn format_trial(seed: u64, cfg: Config) -> Option<(usize, usize)> {
    let mut sim = Sim::new(seed, 60, cfg, 0.0);
    let before = sim.tid_ids().clone();
    sim.format_all();
    if sim.commit().is_err() { return None; }
    let after = sim.tid_ids();
    let kept = before.iter().filter(|(t, id)| after.get(t) == Some(id)).count();
    Some((kept, before.len()))
}
