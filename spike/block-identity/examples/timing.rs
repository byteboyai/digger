use block_identity_spike::parse::parse_blocks;
use block_identity_spike::reconcile::*;
use block_identity_spike::simulate::*;
fn main() {
    for n in [500usize, 1500, 6000, 20000] {
        let mut rng = Rng(0xBEEF);
        let text: String = (0..n).map(|_| format!("{}\n\n", random_content(&mut rng, 0.2).render().replace('\n', " ").replace("```", "").replace('|', " "))).collect();
        let t = std::time::Instant::now();
        let parsed = parse_blocks(&text);
        let parse_ms = t.elapsed().as_secs_f64() * 1e3;
        let old: Vec<Tracked> = parsed.iter().enumerate().map(|(i, p)| Tracked { id: i as u64, kind: p.kind.clone(), text: p.text.clone() }).collect();
        // typing: ten scattered small edits
        let mut edited = parsed.clone();
        for k in 0..10 { let i = (k * 977) % edited.len(); edited[i].text.push_str("追加"); }
        let t = std::time::Instant::now();
        let mut next = 10_000_000;
        let a = reconcile(&old, &edited, &Config::default(), || { next += 1; next });
        let ms_small = t.elapsed().as_secs_f64() * 1e3;
        // restructure: a section of 200 blocks moved, 50 blocks edited, 50 deleted
        let mut r2 = parsed.clone();
        let moved: Vec<_> = r2.drain(n / 4..n / 4 + 200.min(n / 4)).collect();
        let at = r2.len() * 3 / 4;
        for (k, m) in moved.into_iter().enumerate() { r2.insert(at + k, m); }
        for k in 0..50 { let i = (k * 31) % r2.len(); r2[i].text.push_str("改"); }
        let t = std::time::Instant::now();
        let b = reconcile(&old, &r2, &Config::default(), || { next += 1; next });
        let ms_big = t.elapsed().as_secs_f64() * 1e3;
        println!("{n:>6} blocks: parse {parse_ms:>7.1} ms | 10 small edits: reconcile {ms_small:>7.1} ms ({} fresh) | move 200 + edit 50: {ms_big:>7.1} ms ({} fresh)", a.iter().filter(|x| x.how == How::Fresh).count(), b.iter().filter(|x| x.how == How::Fresh).count());
    }
}
