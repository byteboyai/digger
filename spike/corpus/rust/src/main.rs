//! cargo run --release -- <book.md> [seed]    (data from ../prepare.py)
use block_identity_spike::parse::{Kind, parse_blocks};
use block_identity_spike::reconcile::{Config, Tracked, reconcile};
use std::collections::HashMap;
use std::time::Instant;
use text_merge_spike::merge;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
}

/// sentence spans (byte ranges) of a paragraph: end after 。！？．.!? and any closing quotes
fn sentences(p: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = vec![];
    let (mut start, mut it) = (0, p.char_indices().peekable());
    while let Some((i, c)) = it.next() {
        if "。！？．!?".contains(c) || (c == '.' && p[i + 1..].starts_with(' ')) {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = it.peek() { if "”’\"'」』）)".contains(d) { end = j + d.len_utf8(); it.next(); } else { break } }
            out.push(start..end);
            start = end;
        }
    }
    if start < p.len() && !p[start..].trim().is_empty() { out.push(start..p.len()); }
    out
}

fn kind_name(k: &Kind) -> &'static str {
    match k { Kind::Heading(_) => "heading", Kind::Paragraph => "paragraph", Kind::List => "list", Kind::Quote => "quote", Kind::Code => "code", Kind::Table => "table", Kind::Rule => "rule", Kind::Html => "html", Kind::Footnote => "footnote", Kind::Other => "other" }
}

fn main() {
    let path = std::env::args().nth(1).expect("book.md");
    let seed: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let book = std::fs::read_to_string(&path).expect("read book");
    println!("== {path}: {} chars, {} KB", book.chars().count(), book.len() / 1024);

    // ---- parse -------------------------------------------------------------------------------------------------
    let t = Instant::now();
    let blocks = parse_blocks(&book);
    println!("parse: {} blocks in {:?}", blocks.len(), t.elapsed());
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for b in &blocks { *kinds.entry(kind_name(&b.kind)).or_default() += 1; }
    let mut kv: Vec<_> = kinds.into_iter().collect();
    kv.sort();
    println!("kinds: {kv:?}");

    // paragraphs we can draw from: real paragraph texts of the book
    let paras: Vec<String> = blocks.iter().filter(|b| matches!(b.kind, Kind::Paragraph)).map(|b| b.text.clone()).collect();
    let all_sentences: Vec<String> = paras.iter().flat_map(|p| sentences(p).into_iter().map(move |r| p[r].to_string())).filter(|s| s.chars().count() >= 6).collect();
    println!("{} paragraphs, {} sentences", paras.len(), all_sentences.len());

    text_merge_on_prose(&book, &paras, &all_sentences, seed);
    identity_on_prose(&blocks.iter().map(|b| (b.kind.clone(), b.text.clone())).collect::<Vec<_>>(), &paras, &all_sentences, seed);
}

// ---- E2: three-way merge of realistic prose edits --------------------------------------------------------------

#[derive(Clone, Debug)]
struct SEdit { range: std::ops::Range<usize>, text: String }

fn apply_edits(base: &str, edits: &[SEdit]) -> String {
    let mut e = edits.to_vec();
    e.sort_by_key(|x| x.range.start);
    let mut out = String::new();
    let mut at = 0;
    for x in e { out.push_str(&base[at..x.range.start]); out.push_str(&x.text); at = x.range.end; }
    out.push_str(&base[at..]);
    out
}

/// A realistic edit of one sentence: replace it by another real sentence, delete it, or change a few characters.
fn edit_sentence(r: &mut Rng, s: &str, range: std::ops::Range<usize>, pool: &[String]) -> SEdit {
    let text = match r.below(4) {
        0 => pool[r.below(pool.len())].clone(),
        1 => String::new(),
        2 => format!("{s}{}", pool[r.below(pool.len())]),
        _ => { // change one short word-like piece
            let chars: Vec<char> = s.chars().collect();
            if chars.len() < 4 { pool[r.below(pool.len())].clone() } else {
                let a = r.below(chars.len() - 2);
                let rep: String = pool[r.below(pool.len())].chars().take(2).collect();
                format!("{}{}{}", chars[..a].iter().collect::<String>(), rep, chars[a + 2..].iter().collect::<String>())
            }
        }
    };
    SEdit { range, text }
}

fn text_merge_on_prose(book: &str, _paras: &[String], pool: &[String], seed: u64) {
    // chapters = split at "# " headings
    let mut chapters: Vec<&str> = vec![];
    let mut starts: Vec<usize> = book.match_indices("\n# ").map(|(i, _)| i + 1).collect();
    starts.insert(0, 0);
    starts.dedup();
    for (k, s) in starts.iter().enumerate() { chapters.push(&book[*s..starts.get(k + 1).copied().unwrap_or(book.len())]); }
    chapters.retain(|c| c.len() > 2000);
    let mut r = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
    // categories: 0 different paragraphs, 1 same paragraph far apart, 2 same paragraph adjacent sentences
    let names = ["different paragraphs", "same paragraph, one sentence between", "adjacent sentences"];
    let mut stats = [(0u32, 0u32, 0u32); 3]; // (clean, conflict, wrong)
    let mut first_wrong: Option<String> = None;
    let mut max_ms = 0u128;
    let trials = 4000;
    let mut shown = 0;
    for _ in 0..trials {
        let ch = chapters[r.below(chapters.len())];
        // sentence spans over the whole chapter text with paragraph ids
        let mut spans: Vec<(std::ops::Range<usize>, usize)> = vec![];
        let mut off = 0;
        for (pi, p) in ch.split("\n\n").enumerate() {
            if !p.starts_with("# ") && !p.starts_with("---") { for s in sentences(p) { spans.push((off + s.start..off + s.end, pi)); } }
            off += p.len() + 2;
        }
        if spans.len() < 12 { continue; }
        let cat = r.below(3);
        let a = r.below(spans.len() - 6);
        let b = match cat {
            0 => (a + 1..spans.len()).find(|&j| spans[j].1 != spans[a].1 && j > a + 2).unwrap_or(a + 4),
            1 => a + 2,
            _ => a + 1,
        };
        if b >= spans.len() || (cat == 0 && spans[b].1 == spans[a].1) || (cat == 1 && (spans[b].1 != spans[a].1 || spans[a + 1].0.len() < 24)) || (cat == 2 && spans[b].1 != spans[a].1) { continue; }
        // each side may also carry an unrelated edit far away
        let mk = |r: &mut Rng, i: usize| { let (rg, _) = &spans[i]; edit_sentence(r, &ch[rg.clone()], rg.clone(), pool) };
        let (ea, eb) = (mk(&mut r, a), mk(&mut r, b));
        let (ours, theirs, want) = (apply_edits(ch, &[ea.clone()]), apply_edits(ch, &[eb.clone()]), apply_edits(ch, &[ea, eb]));
        let t = Instant::now();
        let m = if std::env::var("UNGATED").is_ok() { text_merge_spike::merge_for_test(ch, &ours, &theirs) } else { merge(ch, &ours, &theirs) };
        max_ms = max_ms.max(t.elapsed().as_millis());
        match m.text() {
            Some(x) => { stats[cat].0 += 1; if x != want { stats[cat].2 += 1; first_wrong.get_or_insert(format!("category {cat}: sentences {a} and {b}")); } }
            None => {
                stats[cat].1 += 1;
                if let Ok(dir) = std::env::var("DUMP") {
                    if text_merge_spike::merge_for_test(ch, &ours, &theirs).text().as_deref() == Some(want.as_str()) {
                        let n = stats[cat].1;
                        for (k, v) in [("base", ch), ("ours", ours.as_str()), ("theirs", theirs.as_str()), ("want", want.as_str())] { std::fs::write(format!("{dir}/gate{cat}_{n}_{k}.txt"), v).unwrap(); }
                    }
                }
                if cat < 2 && shown < 6 {
                    shown += 1;
                    if shown <= 2 {
                        let toks = text_merge_spike::tokens(ch);
                        for (name, side) in [("ours", &ours), ("theirs", &theirs)] {
                            for (rg, t) in text_merge_spike::debug_hunks(ch, side) {
                                println!("    {name} hunk tokens {:?} base {:?}.. -> {:?}", rg, toks[rg.start..rg.end.min(rg.start + 4)].concat(), t.chars().take(12).collect::<String>());
                            }
                        }
                    }
                    println!("  [false conflict, category {cat}]\n    A {:?} -> {:?}\n    B {:?} -> {:?}", &ch[spans[a].0.clone()], ours_text(&ours, ch, &spans[a].0), &ch[spans[b].0.clone()], ours_text(&theirs, ch, &spans[b].0));
                }
            }
        }
    }
    println!("\n-- three-way merge, one sentence edited on each side (replace / delete / append / change two characters), real chapters");
    for (n, (c, k, w)) in names.iter().zip(stats) {
        println!("  {n:38} clean {c:5}  conflict {k:5}  clean-but-wrong {w}");
    }
    println!("  slowest merge of a whole chapter: {max_ms} ms{}", first_wrong.map(|f| format!("   FIRST WRONG: {f}")).unwrap_or_default());
}

fn ours_text(side: &str, base: &str, r: &std::ops::Range<usize>) -> String {
    // what the side has where the base sentence was (by common prefix/suffix with the base chapter)
    let pre = r.start;
    let suf = base.len() - r.end;
    side.get(pre..side.len().saturating_sub(suf)).unwrap_or("?").to_string()
}

// ---- E3: block identity under realistic editing -----------------------------------------------------------------

#[derive(Clone)]
struct B { truth: u64, kind: Kind, text: String }

fn identity_on_prose(initial: &[(Kind, String)], paras: &[String], pool: &[String], seed: u64) {
    let cfg = Config::default();
    let mut r = Rng(seed ^ 0xD1B5_4A32_D192_ED03);
    let mut next_truth = 1u64;
    let mut model: Vec<B> = initial.iter().map(|(k, t)| { next_truth += 1; B { truth: next_truth - 1, kind: k.clone(), text: t.clone() } }).collect();
    // initial assignment
    let mut next_id = 1u64;
    let parsed0 = parse_blocks(&join(&model));
    let first = reconcile(&[], &parsed0, &cfg, || { next_id += 1; next_id - 1 });
    let mut ids: HashMap<u64, u64> = model.iter().zip(&first).map(|(b, a)| (b.truth, a.id)).collect();
    let mut tracked: Vec<Tracked> = parsed0.iter().zip(&first).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();

    // per kind of operation: (expected-to-keep blocks, kept correctly, kept with a wrong id / twin adoption)
    let mut by_op: HashMap<&str, (u64, u64, u64)> = HashMap::new();
    // classes of block text
    let class = |t: &str, all: &HashMap<String, usize>| -> &'static str {
        if all.get(t).copied().unwrap_or(0) > 1 { "identical twin" } else if t.chars().count() < 12 { "short" } else { "ordinary" }
    };
    let mut by_class: HashMap<&str, (u64, u64)> = HashMap::new();
    let (mut commits, mut reconcile_time) = (0, std::time::Duration::ZERO);
    let mut fresh_adoptions = 0u64; // a brand-new / rewritten block that received an id of an old block
    for batch in [1usize, 5, 20].into_iter().cycle().take(60) {
        let mut expect_same: Vec<(u64, &'static str)> = vec![]; // truth ids that must keep their id
        let mut new_truths: Vec<u64> = vec![];
        for _ in 0..batch {
            if model.len() < 20 { break; }
            let i = r.below(model.len());
            if !matches!(model[i].kind, Kind::Paragraph) { continue; }
            match r.below(7) {
                0 | 1 | 2 => { // edit a sentence in place
                    let sp = sentences(&model[i].text);
                    if sp.is_empty() { continue; }
                    let s = sp[r.below(sp.len())].clone();
                    let e = edit_sentence(&mut r, &model[i].text[s.clone()], s, pool);
                    let new = apply_edits(&model[i].text, &[e]);
                    if new.trim().is_empty() { continue; }
                    model[i].text = new;
                    expect_same.push((model[i].truth, "edit a sentence"));
                }
                3 => { // insert a real paragraph from elsewhere
                    let t = paras[r.below(paras.len())].clone();
                    next_truth += 1;
                    model.insert(i + 1, B { truth: next_truth - 1, kind: Kind::Paragraph, text: t });
                    new_truths.push(next_truth - 1);
                }
                4 if model.len() > 30 => { // delete
                    let b = model.remove(i);
                    ids.remove(&b.truth);
                }
                5 => { // move a paragraph some distance
                    let b = model.remove(i);
                    let j = r.below(model.len());
                    expect_same.push((b.truth, "move"));
                    model.insert(j, b);
                }
                _ => { // rewrite wholesale: another real paragraph under the same slot — a new block, not the old one
                    next_truth += 1;
                    let t = paras[r.below(paras.len())].clone();
                    let old = model[i].truth;
                    model[i] = B { truth: next_truth - 1, kind: Kind::Paragraph, text: t };
                    ids.remove(&old);
                    new_truths.push(next_truth - 1);
                }
            }
        }
        // commit: parse the new text and reconcile
        let text = join(&model);
        let parsed = parse_blocks(&text);
        if parsed.len() != model.len() { // two model paragraphs merged by the parser (e.g. rule next to text): resync, skip scoring
            let n0 = next_id;
            let a = reconcile(&tracked, &parsed, &cfg, || { next_id += 1; next_id - 1 });
            let _ = n0;
            tracked = parsed.iter().zip(&a).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
            model = parsed.iter().zip(&a).map(|(p, a)| { next_truth += 1; ids.insert(next_truth - 1, a.id); B { truth: next_truth - 1, kind: p.kind.clone(), text: p.text.clone() } }).collect();
            continue;
        }
        let t0 = Instant::now();
        let a = reconcile(&tracked, &parsed, &cfg, || { next_id += 1; next_id - 1 });
        reconcile_time += t0.elapsed();
        commits += 1;
        let mut count: HashMap<String, usize> = HashMap::new();
        for b in &model { *count.entry(b.text.clone()).or_default() += 1; }
        let by_truth: HashMap<u64, u64> = model.iter().zip(&a).map(|(b, a)| (b.truth, a.id)).collect();
        for (t, op) in &expect_same {
            let Some(want) = ids.get(t) else { continue };
            let e = by_op.entry(op).or_default();
            e.0 += 1;
            let blk = model.iter().find(|b| b.truth == *t).unwrap();
            let cl = by_class.entry(class(&blk.text, &count)).or_default();
            cl.0 += 1;
            match by_truth.get(t) { Some(got) if got == want => { e.1 += 1; cl.1 += 1 } _ => e.2 += 1 }
        }
        let old_ids: std::collections::HashSet<u64> = ids.values().copied().collect();
        for t in &new_truths { if let Some(got) = by_truth.get(t) { if old_ids.contains(got) { fresh_adoptions += 1; } } }
        for (b, x) in model.iter().zip(&a) { ids.insert(b.truth, x.id); }
        tracked = parsed.iter().zip(&a).map(|(p, a)| Tracked { id: a.id, kind: p.kind.clone(), text: p.text.clone() }).collect();
    }
    println!("\n-- block identity, {} commits of 1/5/20 operations on the whole book ({} blocks); reconcile of the whole book took {:?} on average", commits, model.len(), reconcile_time / commits.max(1));
    let mut ops: Vec<_> = by_op.into_iter().collect();
    ops.sort();
    for (op, (n, ok, bad)) in ops { println!("  {op:18} blocks that must keep their id: {n:5}   kept {ok:5}   wrong or lost {bad}"); }
    let mut cls: Vec<_> = by_class.into_iter().collect();
    cls.sort();
    for (c, (n, ok)) in cls { println!("  class {c:15} {n:5} blocks, {:.3}% kept", 100.0 * ok as f64 / n.max(1) as f64); }
    println!("  new or rewritten blocks that wrongly received an old block's id: {fresh_adoptions}");
}

fn join(model: &[B]) -> String {
    let mut s = String::new();
    for b in model { s.push_str(&b.text); s.push_str("\n\n"); }
    s
}
