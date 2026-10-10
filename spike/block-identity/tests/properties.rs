//! Property tests for block identity reconciliation (ADR-012 §2). All runs are seeded and deterministic.
//!
//! Hard properties (must hold for every input): ids are unique, reconciliation is deterministic, and committing
//! unchanged text changes nothing. Quality properties are thresholds measured on simulated authoring sessions
//! with known ground truth; they are set just inside the measured values so a regression is caught.

use block_identity_spike::parse::parse_blocks;
use block_identity_spike::reconcile::*;
use block_identity_spike::simulate::*;

const SEEDS: u64 = 10;

fn streams_n(seeds: u64, mix: &Mix, batch: usize, confusable: f64, cfg: &Config) -> Stats {
    let mut total = Stats::default();
    for seed in 1..=seeds {
        total.merge(&stream(seed * 7919, 60, 25, batch, mix, cfg.clone(), confusable));
    }
    total
}

fn streams(mix: &Mix, batch: usize, confusable: f64, cfg: &Config) -> Stats {
    streams_n(SEEDS, mix, batch, confusable, cfg)
}

/// Failures that do not involve a copy-and-edit (which copy is "the original" cannot be known from content).
fn genuine_misattributions(s: &Stats) -> u64 {
    (s.misattributed - s.dup_misattributed) + (s.stolen - s.dup_stolen)
}

#[test]
fn invariants_hold_in_every_scenario() {
    for (name, mix) in [("typing", Mix::typing()), ("editing", Mix::editing()), ("agent", Mix::agent()), ("restructure", Mix::restructure())] {
        for batch in [1, 5, 20] {
            for confusable in [0.0, 0.6] {
                let s = streams_n(5, &mix, batch, confusable, &Config::default());
                assert!(s.violations.is_empty(), "{name} batch={batch} confusable={confusable}: {:?}", &s.violations[..s.violations.len().min(3)]);
                assert!(s.commits > 0 && s.expected > 0);
            }
        }
    }
}

#[test]
fn typing_keeps_every_id_at_every_commit_granularity_up_to_five_edits() {
    for batch in [1, 5] {
        let s = streams(&Mix::typing(), batch, 0.0, &Config::default());
        assert_eq!(s.lost, 0, "typing batch={batch}");
        assert_eq!(s.misattributed + s.stolen, 0, "typing batch={batch}");
        assert_eq!(s.kept, s.expected);
    }
}

#[test]
fn authoring_sessions_keep_the_ids_of_almost_every_block() {
    for (name, mix) in [("editing", Mix::editing()), ("agent", Mix::agent()), ("restructure", Mix::restructure())] {
        for batch in [1, 5, 20] {
            let s = streams(&mix, batch, 0.0, &Config::default());
            // 20 edits between two commits include copy-and-edit combinations whose original cannot be known
            let floor = if batch <= 5 { 0.9997 } else { 0.999 };
            assert!(s.survival() >= floor, "{name} batch={batch}: kept {:.4}% (lost {}, clear {})", 100.0 * s.survival(), s.lost, s.lost_clear);
        }
    }
}

#[test]
fn real_misses_and_misattributions_stay_negligible_outside_copy_and_edit_cases() {
    for (name, mix) in [("typing", Mix::typing()), ("editing", Mix::editing()), ("agent", Mix::agent()), ("restructure", Mix::restructure())] {
        for batch in [1, 5, 20] {
            for confusable in [0.0, 0.6] {
                let s = streams(&mix, batch, confusable, &Config::default());
                // lost although at least half of its text survived, not counting identical-text ambiguity
                assert!(s.lost_clear as f64 <= 1.0 + s.expected as f64 * 0.00005, "{name} batch={batch} confusable={confusable}: {} clear losses of {}", s.lost_clear, s.expected);
                // the dangerous failure: an id attached to the wrong block. Documents where 60% of the paragraphs follow
                // one template share most of their words, so unrelated blocks look alike: that gets a looser bound.
                let rate = if confusable > 0.0 { 0.0003 } else { 0.00005 };
                // (an id that belonged to a block rewritten wholesale in the same commit is not counted: the rewrite
                // destroyed the evidence, so who inherits it is a guess)
                assert!(genuine_misattributions(&s) as f64 <= 2.0 + s.expected as f64 * rate, "{name} batch={batch} confusable={confusable}: {} wrong attachments of {} blocks", genuine_misattributions(&s), s.expected);
            }
        }
    }
}

#[test]
fn a_block_rewritten_wholesale_is_a_new_block_and_never_takes_another_blocks_id() {
    let rewrites = Mix(vec![(Op::RewriteLarge, 1)]);
    for batch in [1, 5] {
        let s = streams(&rewrites, batch, 0.0, &Config::default());
        assert!(s.heavy > 0);
        assert_eq!(s.heavy_misattributed, 0, "batch={batch}");
        assert_eq!(s.misattributed + s.stolen, 0, "batch={batch}");
        assert!((s.heavy_kept as f64) < 0.05 * s.heavy as f64, "a wholesale rewrite should not keep the id: kept {}/{}", s.heavy_kept, s.heavy);
    }
}

#[test]
fn typing_a_heading_marker_or_removing_it_keeps_the_id() {
    let s = streams(&Mix(vec![(Op::KindChange, 1)]), 1, 0.0, &Config::default());
    assert!(s.expected > 100);
    assert!(s.survival() >= 0.995, "kind changes kept {:.2}%", 100.0 * s.survival());
    assert_eq!(s.misattributed + s.stolen, 0);
}

#[test]
fn thousands_of_characters_inserted_earlier_do_not_disturb_any_id() {
    let mut kept = 0;
    let mut total = 0;
    let mut trials = 0;
    for seed in 1..=60 {
        if let Some((k, t)) = insertion_trial(seed * 31, Config::default()) { kept += k; total += t; trials += 1; }
    }
    assert!(trials >= 50);
    assert_eq!(kept, total, "every original block must keep its id when a 30-block section is inserted anywhere");
}

#[test]
fn reformatting_the_whole_document_keeps_every_id() {
    let mut kept = 0;
    let mut total = 0;
    let mut trials = 0;
    for seed in 1..=60 {
        if let Some((k, t)) = format_trial(seed * 17, Config::default()) { kept += k; total += t; trials += 1; }
    }
    assert!(trials >= 40, "only {trials} usable trials");
    assert!(kept as f64 >= 0.999 * total as f64, "formatting kept {kept} of {total}");
}

#[test]
fn split_and_merge_follow_the_configured_policy() {
    for policy in [SplitPolicy::FirstPart, SplitPolicy::LargestPart] {
        let cfg = Config { split: policy, ..Config::default() };
        let (mut n, mut one, mut first, mut largest) = (0, 0, 0, 0);
        for seed in 1..=150 {
            for r in [split_trial(seed * 13, cfg.clone()), merge_trial(seed * 13, cfg.clone())].into_iter().flatten() {
                n += 1;
                one += r.on_exactly_one as u32;
                first += r.on_first as u32;
                largest += r.on_largest as u32;
            }
        }
        assert!(n > 200, "{policy:?}: {n} trials");
        assert!(one as f64 >= 0.98 * n as f64, "{policy:?}: the id survived on exactly one block in {one}/{n}");
        match policy {
            SplitPolicy::FirstPart => assert!(first as f64 >= 0.98 * n as f64, "FirstPart: first in {first}/{n}"),
            // near-ties in size differ by a few characters between the simulator's count and the reconciler's
            SplitPolicy::LargestPart => assert!(largest as f64 >= 0.96 * n as f64, "LargestPart: largest in {largest}/{n}"),
        }
    }
}

#[test]
fn results_are_deterministic() {
    let a = streams(&Mix::editing(), 5, 0.3, &Config::default());
    let b = streams(&Mix::editing(), 5, 0.3, &Config::default());
    assert_eq!((a.kept, a.lost, a.misattributed, a.stolen, a.new_blocks), (b.kept, b.lost, b.misattributed, b.stolen, b.new_blocks));
}

#[test]
fn reconciling_a_large_chapter_with_a_few_edits_is_fast() {
    // 1500 blocks written without per-block validation; then ten of them are edited.
    let mut rng = Rng(0xBEEF);
    let text: String = (0..1500).map(|_| format!("{}\n\n", random_content(&mut rng, 0.2).render().replace('\n', " ").replace("```", "").replace('|', " "))).collect();
    let old: Vec<Tracked> = parse_blocks(&text).into_iter().enumerate().map(|(i, p)| Tracked { id: i as u64, kind: p.kind, text: p.text }).collect();
    let mut edited = parse_blocks(&text);
    for k in 0..10 {
        let b = &mut edited[k * 140];
        b.text.push_str("追加的几个字");
    }
    let t = std::time::Instant::now();
    let mut next = 100_000;
    let out = reconcile(&old, &edited, &Config::default(), || { next += 1; next });
    let took = t.elapsed();
    let same = out.iter().enumerate().filter(|(i, a)| a.id == *i as u64).count();
    assert!(same >= edited.len() - 2, "{same} of {} kept (some blocks may collapse into duplicates)", edited.len());
    assert!(took.as_millis() < 5000, "reconcile took {took:?}");
}
