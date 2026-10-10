//! Prints identity-survival metrics for edit scenarios.
//!   cargo run --release --bin report [seeds]            default scenarios
//!   cargo run --release --bin report [seeds] sweep      thresholds sweep
use block_identity_spike::reconcile::{Config, SplitPolicy};
use block_identity_spike::simulate::*;

fn row(name: &str, s: &Stats) {
    println!(
        "{name:<30} kept {:>7.3}% | lost {:>4} (clear {:>3}, identical-text {:>3}) | misattributed {:>4} (identical {:>4}) | stolen {:>4} (identical {:>4}) of {:>6} new | violations {}",
        100.0 * s.survival(), s.lost, s.lost_clear, s.lost_dup, s.misattributed, s.dup_misattributed, s.stolen, s.dup_stolen, s.new_blocks, s.violations.len()
    );
}

pub fn run(name: &str, mix: &Mix, batch: usize, confusable: f64, cfg: &Config, seeds: u64, print: bool) -> Stats {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) as u64;
    let parts: Vec<Stats> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let (cfg, mix) = (cfg.clone(), Mix(mix.0.clone()));
                sc.spawn(move || {
                    let mut total = Stats::default();
                    for seed in (1..=seeds).filter(|s| s % threads == t) {
                        total.merge(&stream(seed * 7919, 60, 40, batch, &mix, cfg.clone(), confusable));
                    }
                    total
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut total = Stats::default();
    for p in &parts { total.merge(p); }
    if print { row(name, &total); }
    total
}

fn scenarios(cfg: &Config, seeds: u64, print: bool) -> Stats {
    let mut all = Stats::default();
    for (label, mix) in [("typing", Mix::typing()), ("editing", Mix::editing()), ("agent", Mix::agent()), ("restructure", Mix::restructure())] {
        for batch in [1, 5, 20] {
            all.merge(&run(&format!("{label} batch={batch}"), &mix, batch, 0.0, cfg, seeds, print));
        }
    }
    if print { println!("-- look-alike blocks (templated paragraphs, 60%)"); }
    for (label, mix) in [("typing", Mix::typing()), ("editing", Mix::editing()), ("agent", Mix::agent()), ("restructure", Mix::restructure())] {
        for batch in [1, 5, 20] {
            all.merge(&run(&format!("confusable {label} batch={batch}"), &mix, batch, 0.6, cfg, seeds, print));
        }
    }
    all
}

fn main() {
    let seeds: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(24);
    if std::env::args().nth(2).as_deref() == Some("sweep") {
        println!("{:>5} {:>5} {:>5} | {}", "low", "high", "kc", "all scenarios combined");
        for low in [0.4, 0.5, 0.6] {
            for high in [0.6, 0.7, 0.8] {
                for kc in [0.8, 0.9] {
                    let cfg = Config { low, high, kind_change: kc, ..Config::default() };
                    let s = scenarios(&cfg, seeds, false);
                    println!("{low:>5} {high:>5} {kc:>5} | kept {:>7.3}% lost-clear {:>4} misattributed-not-identical {:>3} stolen-not-identical {:>3}", 100.0 * s.survival(), s.lost_clear, s.misattributed - s.dup_misattributed, s.stolen - s.dup_stolen);
                }
            }
        }
        return;
    }
    println!("== streams ({seeds} documents × 40 commits, default config)");
    let all = scenarios(&Config::default(), seeds, true);
    println!("== sampled real misses (not identical-text)");
    for s in &all.samples { println!("  {s}"); }
    let _ = SplitPolicy::FirstPart;
}
