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
        if case != 340 { continue; }
        let (ours, theirs) = (apply(&bt, &ea), apply(&bt, &eb));
        let mut all = [ea.clone(), eb.clone()].concat();
        all.sort_by_key(|e| (e.from, e.to));
        let overlap = all.windows(2).any(|w| w[0].to > w[1].from);
        println!("edits overlap each other: {overlap}");
        let want = apply(&bt, &all);
        let m = merge(&base, &ours, &theirs);
        println!("clean: {}  equals the union of both edit sets: {}", m.is_clean(), m.text().as_deref() == Some(want.as_str()));
        println!("base tokens around 171: {:?}", &bt[165..176]);
        println!("theirs delete tokens {:?}; ours edits near: {:?}", eb.iter().map(|e| (e.from, e.to)).collect::<Vec<_>>(), ea.iter().map(|e| (e.from, e.to)).collect::<Vec<_>>());
        let merged = m.text().unwrap();
        let again = merge(&base, &theirs, &merged);
        let seg = |name: &str, t: &str| { let i = t.find("return return").unwrap_or(0); println!("{name:>8}: {:?}", t[t.char_indices().map(|(k, _)| k).filter(|&k| k >= i.saturating_sub(40)).next().unwrap_or(0)..].chars().take(70).collect::<String>()); };
        seg("base", &base); seg("ours", &ours); seg("theirs", &theirs); seg("merged", &merged);
        let bt2 = tokens(&base);
        let near = |hs: Vec<(std::ops::Range<usize>, String)>| hs.into_iter().filter(|(r, _)| r.start >= 80 && r.start <= 130).collect::<Vec<_>>();
        println!("theirs hunks near 171: {:?}", near(debug_hunks(&base, &theirs)));
        println!("merged hunks near 171: {:?}", near(debug_hunks(&base, &merged)));
        println!("base tokens 84..122: {:?}", &bt2[84..122]);
        println!("re-merge clean: {} ; conflicts: {}", again.is_clean(), again.conflicts());
        seg("remerge", &again.resolve(Resolve::Ours));
        println!("ours  has 'reader' near there: {}", ours.contains("return return reader system"));
        println!("theirs has it: {}  union has it: {}  merged has it: {}", theirs.contains("return return reader system"), want.contains("return return reader system"), m.text().unwrap_or_default().contains("return return reader system"));
    }
}
