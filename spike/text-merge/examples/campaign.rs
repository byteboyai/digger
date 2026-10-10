//! Looks for the one failure that matters: a merge that says "clean" and is wrong (a change silently lost or doubled).
//!   cargo run --release --example campaign [cases]            the merge as shipped (with the verification gate)
//!   UNGATED=1 cargo run --release --example campaign [cases]  without the gate
#[path = "../tests/common.rs"]
mod common;
use text_merge_spike::*;

fn main() {
    let cases: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(20000);
    let c = if std::env::var("UNGATED").is_ok() { common::campaign(cases, 0xC4A9_0001, &|b, o, t| merge_for_test(b, o, t)) } else { common::campaign(cases, 0xC4A9_0001, &|b, o, t| merge(b, o, t)) };
    println!("{} random merges (edits chosen so the union is the one right answer): {} clean, {} reported conflicts", c.total, c.clean, c.conflicts);
    println!("clean but WRONG (differs from the union of the edits): {}", c.wrong);
    println!("re-merge of a merged text, arbitrary edits: changed it silently {} times; reported a conflict {} times (of {})", c.remerge_changed, c.remerge_conflict, c.remerge_total);
    if let Some(f) = c.first_problem { println!("first problem: {f}"); }
}
