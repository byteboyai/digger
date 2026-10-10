#[path = "../tests/common.rs"]
mod common;
use common::*;
use text_merge_spike::*;
fn main() {
    let seed: u64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut r = Rng(0xC4A9_0001 + seed * 7919);
    let n = 2 + r.below(8);
    let (base, _) = chapter(&mut r, n);
    let bt = tokens(&base);
    let gap = r.below(6);
    let k = 2 + r.below(10);
    let all = disjoint_edits(&mut r, &bt, k, gap, 0..bt.len());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for e in all.iter().cloned() { if r.chance(0.5) { a.push(e) } else { b.push(e) } }
    let (ours, _theirs, want) = (apply(&bt, &a), apply(&bt, &b), apply(&bt, &all));
    let theirs = _theirs.clone();
    let m = merge_for_test(&base, &ours, &theirs);
    let got = m.resolve(Resolve::Ours);
    println!("clean={} equal_to_union={}", m.is_clean(), got == want);
    let (g, w): (Vec<char>, Vec<char>) = (got.chars().collect(), want.chars().collect());
    let p = g.iter().zip(&w).take_while(|(x, y)| x == y).count();
    println!("first difference at char {p}:\n  got  {:?}\n  want {:?}", g[p.saturating_sub(15)..(p + 25).min(g.len())].iter().collect::<String>(), w[p.saturating_sub(15)..(p + 25).min(w.len())].iter().collect::<String>());
    println!("ours  edits {a:?}\ntheirs edits {b:?}");
    let fmt = |hs: Vec<(std::ops::Range<usize>, String)>| hs.into_iter().map(|(r, t)| format!("{}..{}:{:?}", r.start, r.end, t.chars().take(8).collect::<String>())).collect::<Vec<_>>().join("  ");
    // re-merge of the merged text with each side (the check that sometimes changes the text silently)
    for (name, side) in [("ours", &ours), ("theirs", &_theirs)] {
        if let Some(text) = m.text() {
            let again = merge_for_test(&base, side, &text);
            if let Some(t) = again.text() {
                if t != text {
                    let (x, y): (Vec<char>, Vec<char>) = (t.chars().collect(), text.chars().collect());
                    let p = x.iter().zip(&y).take_while(|(a, b)| a == b).count();
                    println!("RE-MERGE with {name} changed the text at char {p}:\n  got  {:?}\n  want {:?}", x[p.saturating_sub(15)..(p + 25).min(x.len())].iter().collect::<String>(), y[p.saturating_sub(15)..(p + 25).min(y.len())].iter().collect::<String>());
                    let fmt = |hs: Vec<(std::ops::Range<usize>, String)>| hs.into_iter().map(|(r, t)| format!("{}..{}:{:?}", r.start, r.end, t.chars().take(8).collect::<String>())).collect::<Vec<_>>().join("  ");
                    println!("  hunks {name}       : {}\n  hunks merged(base): {}", fmt(debug_hunks(&base, side)), fmt(debug_hunks(&base, &text)));
                }
            }
        }
    }
    println!("hunks ours  : {}\nhunks theirs: {}", fmt(debug_hunks(&base, &ours)), fmt(debug_hunks(&base, &theirs)));
}
