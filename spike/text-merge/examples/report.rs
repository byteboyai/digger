//! Rates: spurious conflicts as edits get closer, and how often the re-merge invariant fails.
#[path = "../tests/common.rs"]
mod common;
use common::*;
use text_merge_spike::*;

fn main() {
    println!("edits on both sides, 400 chapters per row; 'conflict' = the merge reports at least one conflict");
    for gap in [0usize, 1, 2, 3, 5, 8] {
        let mut r = Rng(0x5eed ^ gap as u64);
        let (mut conflicts, mut total, mut wrong) = (0, 0, 0);
        for _ in 0..400 {
            let n = 3 + r.below(8);
            let (base, _) = chapter(&mut r, n);
            let bt = tokens(&base);
            let k = 4 + r.below(6);
            let all = disjoint_edits(&mut r, &bt, k, gap, 0..bt.len());
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for e in all.iter().cloned() { if r.chance(0.5) { a.push(e) } else { b.push(e) } }
            let (ours, theirs, want) = (apply(&bt, &a), apply(&bt, &b), apply(&bt, &all));
            let m = merge(&base, &ours, &theirs);
            total += 1;
            if !m.is_clean() { conflicts += 1 } else if m.text().as_deref() != Some(want.as_str()) { wrong += 1 }
        }
        println!("  at least {gap} untouched tokens between edits: {conflicts}/{total} conflicts ({:.1}%), merged text differs from the union of the edits in {wrong}", 100.0 * conflicts as f64 / total as f64);
    }
    println!("re-merge invariant: merge(base, ours, merged) == merged, by distance between the edits");
    for gap in [0usize, 1, 2, 3, 5] {
        let mut r = Rng(0x1234_5678 ^ gap as u64);
        let (mut clean, mut broken) = (0, 0);
        for _ in 0..1500 {
            let n = 3 + r.below(8);
            let (base, _) = chapter(&mut r, n);
            let bt = tokens(&base);
            let k = 2 + r.below(8);
            let all = disjoint_edits(&mut r, &bt, k, gap, 0..bt.len());
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for e in all.iter().cloned() { if r.chance(0.5) { a.push(e) } else { b.push(e) } }
            let (ours, theirs) = (apply(&bt, &a), apply(&bt, &b));
            if let Some(text) = merge(&base, &ours, &theirs).text() {
                clean += 1;
                let x = merge(&base, &ours, &text).text();
                let y = merge(&base, &theirs, &text).text();
                if x.as_deref() != Some(text.as_str()) || y.as_deref() != Some(text.as_str()) { broken += 1 }
            }
        }
        println!("  at least {gap} untouched tokens between edits: fails in {broken} of {clean} clean merges ({:.2}%)", 100.0 * broken as f64 / clean.max(1) as f64);
    }
}
