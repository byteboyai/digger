#[path = "../tests/common.rs"]
mod common;
use common::*;
use text_merge_spike::*;
fn main() {
    let mut r = Rng(0x1234_5678);
    for case in 0..400 {
        let n = 3 + r.below(8);
        let (base, _) = chapter(&mut r, n);
        let bt = tokens(&base);
        let ka = 1 + r.below(6);
        let ea = disjoint_edits(&mut r, &bt, ka, 0, 0..bt.len());
        let kb = 1 + r.below(6);
        let eb = disjoint_edits(&mut r, &bt, kb, 0, 0..bt.len());
        let (ours, theirs) = (apply(&bt, &ea), apply(&bt, &eb));
        if let Some(text) = merge(&base, &ours, &theirs).text() {
            for (name, side, edits) in [("ours", &ours, &ea), ("theirs", &theirs, &eb)] {
                if let Some(t) = merge(&base, side, &text).text() {
                    if t != text {
                        println!("case {case}: re-merge with {name} gave a different CLEAN text");
                        println!("{name} edits: {edits:?}\nother edits: {:?}", if name == "ours" { &eb } else { &ea });
                        // where the texts differ
                        let (a, b): (Vec<char>, Vec<char>) = (t.chars().collect(), text.chars().collect());
                        let p = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
                        println!("first difference at char {p}: got {:?} want {:?}", a[p.saturating_sub(10)..(p + 20).min(a.len())].iter().collect::<String>(), b[p.saturating_sub(10)..(p + 20).min(b.len())].iter().collect::<String>());
                        println!("units base/{name}/merged: {} {} {}", base.matches("\n\n").count(), side.matches("\n\n").count(), text.matches("\n\n").count());
                        return;
                    }
                }
            }
        }
    }
    println!("no case found");
}
