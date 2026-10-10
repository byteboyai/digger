//! Property tests for the token-level three-way merge. Deterministic (seeded). Each generated case knows what the
//! merged text must be (an oracle built from the generating edits), or checks an invariant that needs no oracle.

mod common;
use common::*;
use text_merge_spike::*;

#[test]
fn disjoint_edits_on_both_sides_merge_to_exactly_the_union() {
    let mut r = Rng(0xA11CE);
    let (mut clean, mut total) = (0u32, 0u32);
    for _ in 0..300 {
        let count = 6 + r.below(10);
        let (base, _) = chapter(&mut r, count);
        let bt = tokens(&base);
        let n_all = 8 + r.below(10);
        let all = disjoint_edits(&mut r, &bt, n_all, 3, 0..bt.len());
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for e in all.iter().cloned() {
            if r.chance(0.5) { a.push(e) } else { b.push(e) }
        }
        let (ours, theirs, want) = (apply(&bt, &a), apply(&bt, &b), apply(&bt, &all));
        let m = merge(&base, &ours, &theirs);
        total += 1;
        // The one thing that must never happen: a merge that says "clean" and is not the union of the edits.
        if let Some(text) = m.text() {
            assert_eq!(text, want, "a clean merge must be the union of both sides' edits: {all:?}");
            assert_eq!(merge(&base, &theirs, &ours).text().as_deref(), Some(want.as_str()), "clean merges are symmetric");
            clean += 1;
        }
    }
    // Edits at least three tokens apart merge cleanly. The double-check after each clean merge is deliberately
    // conservative and may turn a merge into a conflict when repeated words make the diff ambiguous: rare.
    assert!(clean as f64 >= 0.98 * total as f64, "{clean} of {total} merged cleanly");
}

#[test]
fn edits_in_different_blocks_merge_cleanly_even_when_blocks_are_adjacent() {
    let mut r = Rng(0xB10C);
    let (mut clean, mut total) = (0, 0);
    for _ in 0..300 {
        let count = 5 + r.below(8);
        let (base, spans) = chapter(&mut r, count);
        let bt = tokens(&base);
        let ranges = token_ranges_of(&base, &spans);
        let i = r.below(ranges.len());
        let mut j = r.below(ranges.len());
        if j == i { j = (i + 1) % ranges.len(); }
        // gap 0 inside a block: the only protection between the two sides is the block boundary itself
        let n_ea = 1 + r.below(4);
        let ea = disjoint_edits(&mut r, &bt, n_ea, 2, ranges[i].clone());
        let n_eb = 1 + r.below(4);
        let eb = disjoint_edits(&mut r, &bt, n_eb, 2, ranges[j].clone());
        let mut all = [ea.clone(), eb.clone()].concat();
        all.sort_by_key(|e| (e.from, e.to));
        let (ours, theirs, want) = (apply(&bt, &ea), apply(&bt, &eb), apply(&bt, &all));
        let m = merge(&base, &ours, &theirs);
        total += 1;
        if let Some(text) = m.text() {
            assert_eq!(text, want, "blocks {i} and {j}: ours {ea:?} theirs {eb:?}");
            clean += 1;
        }
    }
    assert!(clean as f64 >= 0.98 * total as f64, "{clean} of {total} merged cleanly");
}

#[test]
fn deleting_a_whole_block_does_not_disturb_edits_elsewhere() {
    let mut r = Rng(0xDE1E7E);
    let (mut clean, mut total) = (0, 0);
    for _ in 0..200 {
        let count = 6 + r.below(6);
        let (base, spans) = chapter(&mut r, count);
        let bt = tokens(&base);
        let ranges = token_ranges_of(&base, &spans);
        let i = 1 + r.below(ranges.len() - 2);
        // ours deletes block i together with the blank line after it
        let del = Edit { from: ranges[i].start, to: ranges[i + 1].start, text: String::new() };
        let mut j = r.below(ranges.len());
        while (j as i64 - i as i64).abs() <= 1 { j = r.below(ranges.len()); }
        let n_eb = 1 + r.below(3);
        let eb = disjoint_edits(&mut r, &bt, n_eb, 2, ranges[j].clone());
        let (ours, theirs) = (apply(&bt, std::slice::from_ref(&del)), apply(&bt, &eb));
        let mut all = vec![del.clone()];
        all.extend(eb.clone());
        all.sort_by_key(|e| (e.from, e.to));
        let want = apply(&bt, &all);
        if let Some(text) = merge(&base, &ours, &theirs).text() {
            assert_eq!(text, want, "deleted block {i}, edited block {j}");
            clean += 1;
        }
        total += 1;
    }
    assert!(clean as f64 >= 0.98 * total as f64, "{clean} of {total} merged cleanly");
}

#[test]
fn overlapping_different_edits_are_reported_as_conflicts_and_nothing_is_lost() {
    let mut r = Rng(0xC0FFEE);
    for _ in 0..300 {
        let count = 4 + r.below(6);
        let (base, _) = chapter(&mut r, count);
        let bt = tokens(&base);
        let p = r.below(bt.len().saturating_sub(4));
        let (from_a, to_a) = (p, p + 1 + r.below(3));
        // theirs overlaps ours by at least one token
        let (from_b, to_b) = (p + r.below(to_a - p), to_a + r.below(3));
        let (x, y) = (wrap(&mut r), wrap(&mut r));
        if x == y { continue; }
        let ours = apply(&bt, &[Edit { from: from_a, to: to_a, text: x.clone() }]);
        let theirs = apply(&bt, &[Edit { from: from_b, to: to_b.min(bt.len()), text: y.clone() }]);
        let m = merge(&base, &ours, &theirs);
        assert!(m.conflicts() >= 1, "overlapping edits must conflict: {x} vs {y}");
        let both = m.resolve(Resolve::Both);
        assert!(both.contains(&x) && both.contains(&y), "neither side's text may be dropped: {both:?}");
        assert!(m.resolve(Resolve::Ours).contains(&x));
        assert!(m.resolve(Resolve::Theirs).contains(&y));
    }
}

#[test]
fn invariants_that_need_no_oracle() {
    let mut r = Rng(0x1234_5678);
    let (mut clean, mut conflicted, mut remerge_conflicts) = (0, 0, 0);
    for _ in 0..400 {
        let count = 3 + r.below(8);
        let (base, _) = chapter(&mut r, count);
        let bt = tokens(&base);
        // arbitrary (possibly touching or overlapping) edits on both sides
        let n_ea = 1 + r.below(6);
        let ea = disjoint_edits(&mut r, &bt, n_ea, 0, 0..bt.len());
        let n_eb = 1 + r.below(6);
        let eb = disjoint_edits(&mut r, &bt, n_eb, 0, 0..bt.len());
        let (ours, theirs) = (apply(&bt, &ea), apply(&bt, &eb));
        let m = merge(&base, &ours, &theirs);
        assert_eq!(merge(&base, &ours, &ours).text().as_deref(), Some(ours.as_str()), "identical sides");
        assert_eq!(merge(&base, &base, &theirs).text().as_deref(), Some(theirs.as_str()));
        match m.text() {
            Some(text) => {
                clean += 1;
                // symmetric
                assert_eq!(merge(&base, &theirs, &ours).text().as_deref(), Some(text.as_str()), "clean merges are symmetric");
                // The merged text already contains both sides' changes: merging it with either side must change nothing.
                // A diff can cut the same edit differently when words repeat (this generator draws from 12 English
                // words), and then the re-merge reports a conflict. It must NEVER produce a different clean text.
                for side in [&ours, &theirs] {
                    let again = merge(&base, side, &text);
                    match again.text() {
                        Some(t) => assert_eq!(t, text, "a clean re-merge must give the same text"),
                        None => remerge_conflicts += 1,
                    }
                }
            }
            None => {
                conflicted += 1;
                let both = m.resolve(Resolve::Both);
                for s in &m.segments {
                    if let Segment::Conflict { ours, theirs, .. } = s {
                        assert!(both.contains(ours.as_str()) && both.contains(theirs.as_str()));
                    }
                }
            }
        }
    }
    assert!(clean > 50 && conflicted > 50, "the generator should produce both kinds: clean {clean}, conflicted {conflicted}");
    assert!(remerge_conflicts as f64 <= 0.03 * (2 * clean) as f64, "re-merge reported a conflict {remerge_conflicts} times in {} clean merges", 2 * clean);
}

#[test]
fn a_whole_chapter_with_many_scattered_edits_merges_quickly() {
    let mut r = Rng(0xFA57);
    let (base, _) = chapter(&mut r, 400); // about 100k characters
    let bt = tokens(&base);
    let all = disjoint_edits(&mut r, &bt, 120, 6, 0..bt.len());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for e in all.iter().cloned() {
        if r.chance(0.5) { a.push(e) } else { b.push(e) }
    }
    let (ours, theirs, want) = (apply(&bt, &a), apply(&bt, &b), apply(&bt, &all));
    let t = std::time::Instant::now();
    let m = merge(&base, &ours, &theirs);
    let took = t.elapsed();
    assert_eq!(m.text().as_deref(), Some(want.as_str()));
    assert!(took.as_millis() < 3000, "{} chars, {} tokens: {took:?}", base.chars().count(), bt.len());
}

#[test]
fn no_clean_merge_is_ever_wrong_in_a_large_random_campaign() {
    // The failures this guards against appeared once in tens of thousands of random merges; a few hundred cases
    // cannot find them.
    let c = campaign(30_000, 0xC4A9_0001, &|b, o, t| merge(b, o, t));
    assert_eq!(c.wrong, 0, "{:?}", c.first_problem);
    assert_eq!(c.remerge_changed, 0, "{:?}", c.first_problem);
    assert!(c.clean as f64 >= 0.94 * c.total as f64, "only {} of {} merged cleanly", c.clean, c.total);
    assert!((c.remerge_conflict as f64) <= 0.02 * c.remerge_total as f64, "{} of {} re-merges reported a conflict", c.remerge_conflict, c.remerge_total);
}
