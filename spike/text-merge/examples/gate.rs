//! cargo run --release --example gate -- /tmp/gatecases/gate0_1   (files <prefix>_{base,ours,theirs}.txt)
use text_merge_spike::*;
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let rd = |k: &str| std::fs::read_to_string(format!("{p}_{k}.txt")).unwrap();
    let (base, ours, theirs) = (rd("base"), rd("ours"), rd("theirs"));
    let bt = tokens(&base);
    let Some(merged) = merge_for_test(&base, &ours, &theirs).text() else { println!("ungated merge conflicts"); for (n, a) in [("ours", &ours), ("theirs", &theirs)] { for (r, t) in debug_hunks(&base, a) { println!("  {n}: {:?} -> {:?}", r, t.chars().take(20).collect::<String>()); } } return; };
    let show = |name: &str, a: &str, b: &str| {
        for (r, t) in debug_hunks(a, b) {
            let ctx: String = bt[r.start.saturating_sub(2)..r.end.min(bt.len()).min(r.start + 3)].concat();
            println!("  {name}: base tokens {:?} ({:?}…) -> {:?}", r, ctx.chars().take(14).collect::<String>(), { let c: Vec<char> = t.chars().collect(); format!("{}…{} (len {})", c.iter().take(6).collect::<String>(), c[c.len().saturating_sub(6)..].iter().collect::<String>(), c.len()) });
        }
    };
    println!("base->ours"); show("ours", &base, &ours);
    println!("base->theirs"); show("theirs", &base, &theirs);
    println!("base->merged"); show("merged", &base, &merged);
    for (name, side) in [("ours", &ours), ("theirs", &theirs)] {
        let r = merge_for_test(&base, side, &merged);
        println!("re-merge of {name} with merged: clean={} equal={}", r.is_clean(), r.text().as_deref() == Some(merged.as_str()));
    }
}
