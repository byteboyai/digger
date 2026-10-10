//! 持久化的增量历史(ADR-001 / 技术架构 §7 的 spike 版本):追加式日志 + 周期性检查点。
//!
//! 文件布局(`<dir>/`):
//!   meta.json   {"format":1,"binding":{...}}      创建时写一次
//!   log.jsonl   每行 `<crc32 8位十六进制> <json>\n`
//!               第一条总是检查点;之后每个版本 N 一条 `delta`(相对 N-1 的块级增量),
//!               N 是 `every` 的倍数时紧跟一条 `checkpoint`(完整块列表)。
//!
//! 任意旧版本 = 最近的检查点 + 若干条增量,重建结果放进小缓存。提交只追加一行并 fsync,
//! 不再整篇重写;内存里只有"索引 + 缓存",而不是每个版本一份完整副本。
//! 崩溃安全:启动时校验 CRC,文件末尾被截断的半条记录会被丢弃并截断文件;中间的损坏是错误,不静默跳过。

use crate::bridge::Binding;
use crate::doc::{self, Block, Delta};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const DEFAULT_CHECKPOINT_EVERY: u64 = 32;
const CACHE_SIZE: usize = 8;

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0xEDB8_8320 & (!(c & 1)).wrapping_add(1));
        }
    }
    !c
}

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn line(json: &Value) -> String {
    let body = json.to_string();
    format!("{:08x} {body}\n", crc32(body.as_bytes()))
}

#[derive(Clone, Copy)]
struct Loc {
    off: u64,
    len: u32, // 含换行
}

pub struct History {
    dir: PathBuf,
    log: PathBuf,
    file: File,
    reader: File,
    deltas: BTreeMap<u64, Loc>,
    checkpoints: BTreeMap<u64, Loc>,
    head: u64,
    size: u64,
    every: u64,
    cache: Mutex<VecDeque<(u64, Arc<Vec<Block>>)>>,
    pub binding: Binding,
}

impl History {
    /// 新建一份历史,以 `rev` 的完整块列表作为第一个检查点。
    pub fn create(dir: &Path, binding: Binding, rev: u64, blocks: &[Block], every: u64) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let log = dir.join("log.jsonl");
        if log.exists() {
            return Err(bad("history already exists"));
        }
        write_durable(&dir.join("meta.json"), json!({"format": 1, "binding": binding}).to_string().as_bytes())?;
        let rec = line(&json!({"rev": rev, "kind": "checkpoint", "blocks": blocks}));
        write_durable(&log, rec.as_bytes())?;
        Self::open_with(dir, every)
    }

    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::open_with(dir, DEFAULT_CHECKPOINT_EVERY)
    }

    pub fn exists(dir: &Path) -> bool {
        dir.join("log.jsonl").exists() && dir.join("meta.json").exists()
    }

    fn open_with(dir: &Path, every: u64) -> io::Result<Self> {
        let meta: Value = serde_json::from_slice(&std::fs::read(dir.join("meta.json"))?).map_err(|e| bad(format!("meta.json: {e}")))?;
        if meta["format"] != 1 {
            return Err(bad("unsupported history format"));
        }
        let binding: Binding = serde_json::from_value(meta["binding"].clone()).map_err(|e| bad(format!("meta binding: {e}")))?;
        let log = dir.join("log.jsonl");
        let mut deltas = BTreeMap::new();
        let mut checkpoints = BTreeMap::new();
        let mut head = 0u64;
        let mut valid_end = 0u64;
        {
            let mut r = BufReader::new(File::open(&log)?);
            let mut off = 0u64;
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let n = r.read_until(b'\n', &mut buf)?;
                if n == 0 {
                    break;
                }
                let parsed = parse_line(&buf);
                let at_tail = r.fill_buf()?.is_empty();
                match parsed {
                    Ok(v) => {
                        let loc = Loc { off, len: n as u32 };
                        let rev = v["rev"].as_u64().ok_or_else(|| bad(format!("record at {off} has no rev")))?;
                        match v["kind"].as_str() {
                            Some("checkpoint") => {
                                if checkpoints.is_empty() {
                                    head = rev;
                                } else if rev != head {
                                    return Err(bad(format!("checkpoint {rev} does not follow delta {head}")));
                                }
                                checkpoints.insert(rev, loc);
                            }
                            Some("delta") => {
                                if checkpoints.is_empty() {
                                    return Err(bad("log must start with a checkpoint"));
                                }
                                if rev != head + 1 {
                                    return Err(bad(format!("delta {rev} after {head}: revisions must be consecutive")));
                                }
                                deltas.insert(rev, loc);
                                head = rev;
                            }
                            other => return Err(bad(format!("unknown record kind {other:?} at {off}"))),
                        }
                        off += n as u64;
                        valid_end = off;
                    }
                    Err(e) if at_tail => {
                        // 崩溃留下的半条记录:丢弃,稍后截断文件。
                        let _ = e;
                        break;
                    }
                    Err(e) => return Err(bad(format!("corrupt record at byte {off}: {e}"))),
                }
            }
        }
        if checkpoints.is_empty() {
            return Err(bad("history log has no checkpoint"));
        }
        let file = OpenOptions::new().append(true).open(&log)?;
        if file.metadata()?.len() != valid_end {
            file.set_len(valid_end)?; // 截掉半条记录
            file.sync_all()?;
        }
        let reader = File::open(&log)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            log,
            file,
            reader,
            deltas,
            checkpoints,
            head,
            size: valid_end,
            every: every.max(1),
            cache: Mutex::new(VecDeque::new()),
            binding,
        })
    }

    pub fn head(&self) -> u64 {
        self.head
    }
    pub fn oldest(&self) -> u64 {
        *self.checkpoints.keys().next().unwrap()
    }
    pub fn file_len(&self) -> u64 {
        self.size
    }
    pub fn contains(&self, rev: u64) -> bool {
        rev >= self.oldest() && rev <= self.head
    }

    fn read_record(&self, loc: Loc) -> Option<Value> {
        let mut buf = vec![0u8; loc.len as usize];
        self.reader.read_exact_at(&mut buf, loc.off).ok()?;
        parse_line(&buf).ok()
    }

    /// 重建版本 `rev`:最近的检查点 + 其后的增量。过旧(已压缩)或不存在返回 None。
    pub fn blocks_at(&self, rev: u64) -> Option<Arc<Vec<Block>>> {
        if !self.contains(rev) {
            return None;
        }
        if let Some((_, b)) = self.cache.lock().unwrap().iter().find(|(r, _)| *r == rev) {
            return Some(b.clone());
        }
        let (&cp_rev, &cp_loc) = self.checkpoints.range(..=rev).next_back()?;
        let cp = self.read_record(cp_loc)?;
        let mut blocks: Vec<Block> = serde_json::from_value(cp["blocks"].clone()).ok()?;
        for r in cp_rev + 1..=rev {
            let rec = self.read_record(*self.deltas.get(&r)?)?;
            let (delta, _) = doc::parse_delta(&rec["delta"]).ok()?;
            blocks = doc::apply_delta(&blocks, &delta).ok()?;
        }
        let out = Arc::new(blocks);
        let mut c = self.cache.lock().unwrap();
        c.push_back((rev, out.clone()));
        if c.len() > CACHE_SIZE {
            c.pop_front();
        }
        Some(out)
    }

    /// 追加版本 `rev`(必须是 head+1):一条增量,以及每 `every` 个版本一条检查点;一次写入、一次 fsync。
    pub fn append(&mut self, rev: u64, prev: &[Block], new: &[Block], label: &str) -> io::Result<()> {
        if rev != self.head + 1 {
            return Err(bad(format!("append {rev} but head is {}", self.head)));
        }
        let delta: Delta = doc::make_delta(prev, new);
        let l1 = line(&json!({"rev": rev, "kind": "delta", "label": label, "delta": delta.to_json()}));
        let mut buf = l1.clone();
        let cp = rev % self.every == 0;
        if cp {
            buf.push_str(&line(&json!({"rev": rev, "kind": "checkpoint", "blocks": new})));
        }
        let res = self.file.write_all(buf.as_bytes()).and_then(|_| self.file.sync_data());
        if let Err(e) = res {
            let _ = self.file.set_len(self.size); // 不让半条记录留在文件中间
            return Err(e);
        }
        self.deltas.insert(rev, Loc { off: self.size, len: l1.len() as u32 });
        if cp {
            self.checkpoints.insert(rev, Loc { off: self.size + l1.len() as u64, len: (buf.len() - l1.len()) as u32 });
        }
        self.size += buf.len() as u64;
        self.head = rev;
        let mut c = self.cache.lock().unwrap();
        c.push_back((rev, Arc::new(new.to_vec())));
        if c.len() > CACHE_SIZE {
            c.pop_front();
        }
        Ok(())
    }

    /// 丢弃早于 `floor` 的历史:新日志从 ≤ floor 的最近检查点开始,拷贝其后的记录,原子替换。
    /// 返回新的最早可重建版本。调用方负责保证没有仍被引用的更旧基线(宿主基线、恢复日志)。
    pub fn compact(&mut self, floor: u64) -> io::Result<u64> {
        let (&cp_rev, _) = self.checkpoints.range(..=floor).next_back().ok_or_else(|| bad("floor is older than the history"))?;
        let first = self.blocks_at(cp_rev).ok_or_else(|| bad("cannot rebuild the checkpoint"))?;
        let tmp = self.dir.join("log.jsonl.compact");
        let mut out = File::create(&tmp)?;
        out.write_all(line(&json!({"rev": cp_rev, "kind": "checkpoint", "blocks": &*first})).as_bytes())?;
        // 其后的记录按原顺序拷贝(原始字节,CRC 不变)。
        let mut recs: Vec<Loc> = self.deltas.range(cp_rev + 1..).map(|(_, l)| *l).chain(self.checkpoints.range(cp_rev + 1..).map(|(_, l)| *l)).collect();
        recs.sort_by_key(|l| l.off);
        for l in recs {
            let mut buf = vec![0u8; l.len as usize];
            self.reader.read_exact_at(&mut buf, l.off)?;
            out.write_all(&buf)?;
        }
        out.sync_all()?;
        std::fs::rename(&tmp, &self.log)?;
        let every = self.every;
        *self = Self::open_with(&self.dir.clone(), every)?;
        Ok(self.oldest())
    }

    /// 完整性自检:每个检查点必须等于"上一个检查点 + 中间所有增量"的结果。
    pub fn verify(&self) -> Result<(), String> {
        let mut prev: Option<(u64, Vec<Block>)> = None;
        for (&rev, &loc) in &self.checkpoints {
            let rec = self.read_record(loc).ok_or(format!("unreadable checkpoint {rev}"))?;
            let blocks: Vec<Block> = serde_json::from_value(rec["blocks"].clone()).map_err(|e| format!("checkpoint {rev}: {e}"))?;
            if let Some((prev_rev, mut cur)) = prev.take() {
                for r in prev_rev + 1..=rev {
                    let d = self.read_record(*self.deltas.get(&r).ok_or(format!("missing delta {r}"))?).ok_or(format!("unreadable delta {r}"))?;
                    let (delta, _) = doc::parse_delta(&d["delta"]).map_err(|e| format!("delta {r}: {e:?}"))?;
                    cur = doc::apply_delta(&cur, &delta).map_err(|e| format!("apply delta {r}: {e:?}"))?;
                }
                if cur != blocks {
                    return Err(format!("checkpoint {rev} differs from the replayed deltas"));
                }
            }
            prev = Some((rev, blocks));
        }
        Ok(())
    }
}

fn parse_line(buf: &[u8]) -> Result<Value, String> {
    let body = buf.strip_suffix(b"\n").ok_or("no trailing newline")?;
    let sp = body.iter().position(|&b| b == b' ').ok_or("no checksum")?;
    let crc = u32::from_str_radix(std::str::from_utf8(&body[..sp]).map_err(|e| e.to_string())?, 16).map_err(|e| e.to_string())?;
    let json = &body[sp + 1..];
    if crc32(json) != crc {
        return Err("checksum mismatch".into());
    }
    serde_json::from_slice(json).map_err(|e| e.to_string())
}

fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Kind;

    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!("digger-history-{}", uuid::Uuid::new_v4()))
    }
    fn binding() -> Binding {
        Binding { project_id: "p".into(), panel_id: "pn".into(), document_id: "d".into() }
    }
    fn blk(id: &str, t: &str) -> Block {
        Block { id: id.into(), kind: Kind::Paragraph, text: t.into() }
    }
    fn base(n: usize) -> Vec<Block> {
        (0..n).map(|i| blk(&format!("b{i}"), &format!("text {i}"))).collect()
    }
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }
    fn mutate(b: &mut Vec<Block>, rng: &mut Rng, counter: &mut u32) {
        for _ in 0..1 + rng.below(3) {
            match rng.below(4) {
                0 if b.len() > 2 => {
                    let i = rng.below(b.len());
                    b.remove(i);
                }
                1 => {
                    let i = rng.below(b.len());
                    b[i].text.push('字');
                }
                2 => {
                    *counter += 1;
                    let at = rng.below(b.len() + 1);
                    b.insert(at, blk(&format!("n{counter}"), "新"));
                }
                _ if b.len() > 1 => {
                    let f = rng.below(b.len());
                    let x = b.remove(f);
                    let to = rng.below(b.len() + 1);
                    b.insert(to, x);
                }
                _ => {}
            }
        }
    }
    /// 建一份 `revs` 个版本的历史,同时在内存里留一份"每版完整副本"当参照。
    fn build(d: &Path, n_blocks: usize, revs: u64, every: u64) -> (History, Vec<Vec<Block>>) {
        let mut b = base(n_blocks);
        let mut h = History::create(d, binding(), 1, &b, every).unwrap();
        let mut all = vec![b.clone()]; // all[rev-1]
        let (mut rng, mut c) = (Rng(0x1234_5678_9ABC_DEF1), 0);
        for rev in 2..=revs {
            let prev = b.clone();
            mutate(&mut b, &mut rng, &mut c);
            h.append(rev, &prev, &b, "t").unwrap();
            all.push(b.clone());
        }
        (h, all)
    }

    #[test]
    fn every_revision_reconstructs_exactly() {
        let d = dir();
        let (h, all) = build(&d, 40, 100, 8);
        assert_eq!(h.head(), 100);
        for (i, want) in all.iter().enumerate() {
            assert_eq!(&*h.blocks_at(i as u64 + 1).unwrap(), want, "rev {}", i + 1);
        }
        h.verify().unwrap();
        assert!(h.blocks_at(0).is_none() && h.blocks_at(101).is_none());
    }

    #[test]
    fn reopen_from_disk_gives_the_same_history() {
        let d = dir();
        let (h, all) = build(&d, 30, 70, 10);
        drop(h);
        let h2 = History::open(&d).unwrap();
        assert_eq!((h2.head(), h2.oldest()), (70, 1));
        for (i, want) in all.iter().enumerate() {
            assert_eq!(&*h2.blocks_at(i as u64 + 1).unwrap(), want);
        }
        assert_eq!(h2.binding, binding());
        h2.verify().unwrap();
    }

    #[test]
    fn appending_after_reopen_continues_the_log() {
        let d = dir();
        let (h, all) = build(&d, 10, 12, 4);
        drop(h);
        let mut h = History::open_with(&d, 4).unwrap();
        let mut next = all.last().unwrap().clone();
        next[0].text.push('!');
        h.append(13, all.last().unwrap(), &next, "t").unwrap();
        drop(h);
        let h = History::open_with(&d, 4).unwrap();
        assert_eq!(&*h.blocks_at(13).unwrap(), &next);
        h.verify().unwrap();
    }

    #[test]
    fn torn_tail_is_discarded_and_the_log_stays_appendable() {
        let d = dir();
        let (h, all) = build(&d, 10, 6, 100);
        let len = h.file_len();
        drop(h);
        // 崩溃:最后一条记录只写了一半。
        let f = OpenOptions::new().write(true).open(d.join("log.jsonl")).unwrap();
        f.set_len(len - 7).unwrap();
        drop(f);
        let mut h = History::open_with(&d, 100).unwrap();
        assert_eq!(h.head(), 5, "rev 6 was torn and is gone");
        assert_eq!(&*h.blocks_at(5).unwrap(), &all[4]);
        assert!(std::fs::metadata(d.join("log.jsonl")).unwrap().len() < len - 7, "tail truncated on open");
        // 可继续追加,且重新打开后一致
        h.append(6, &all[4], &all[5], "t").unwrap();
        drop(h);
        let h = History::open_with(&d, 100).unwrap();
        assert_eq!(&*h.blocks_at(6).unwrap(), &all[5]);
        h.verify().unwrap();
    }

    #[test]
    fn corruption_in_the_middle_is_an_error_not_silently_skipped() {
        let d = dir();
        let (h, _) = build(&d, 10, 8, 100);
        drop(h);
        let p = d.join("log.jsonl");
        let mut bytes = std::fs::read(&p).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x55;
        std::fs::write(&p, bytes).unwrap();
        let e = History::open(&d).err().expect("must fail");
        assert!(e.to_string().contains("corrupt"), "{e}");
    }

    #[test]
    fn non_consecutive_or_missing_checkpoint_is_rejected() {
        let d = dir();
        let b = base(3);
        let mut h = History::create(&d, binding(), 1, &b, 10).unwrap();
        assert!(h.append(3, &b, &b, "gap").is_err());
        drop(h);
        let d2 = dir();
        std::fs::create_dir_all(&d2).unwrap();
        std::fs::write(d2.join("meta.json"), json!({"format":1,"binding":binding()}).to_string()).unwrap();
        std::fs::write(d2.join("log.jsonl"), line(&json!({"rev":2,"kind":"delta","delta":{"changed":[],"removed":[]}}))).unwrap();
        assert!(History::open(&d2).is_err());
    }

    #[test]
    fn compaction_keeps_recent_revisions_and_drops_old_ones() {
        let d = dir();
        let (mut h, all) = build(&d, 20, 90, 8);
        let before = h.file_len();
        let oldest = h.compact(60).unwrap();
        assert_eq!(oldest, 56, "starts at the checkpoint at or below the floor");
        assert!(h.file_len() < before);
        assert!(h.blocks_at(55).is_none());
        for rev in 56..=90 {
            assert_eq!(&*h.blocks_at(rev).unwrap(), &all[rev as usize - 1], "rev {rev}");
        }
        h.verify().unwrap();
        // 压缩后重新打开、继续追加
        drop(h);
        let mut h = History::open_with(&d, 8).unwrap();
        assert_eq!((h.oldest(), h.head()), (56, 90));
        let mut n = all.last().unwrap().clone();
        n.pop();
        h.append(91, all.last().unwrap(), &n, "t").unwrap();
        assert_eq!(&*h.blocks_at(91).unwrap(), &n);
    }

    #[test]
    fn log_is_far_smaller_than_full_copies_per_revision() {
        let d = dir();
        let (n_blocks, revs) = (2000usize, 200u64);
        let (h, all) = build(&d, n_blocks, revs, DEFAULT_CHECKPOINT_EVERY);
        let naive: usize = all.iter().map(|v| serde_json::to_vec(v).unwrap().len()).sum();
        let log = h.file_len() as usize;
        println!("history: {n_blocks} blocks × {revs} revs → log {} KB vs {} KB as full copies ({:.1}%)", log / 1024, naive / 1024, 100.0 * log as f64 / naive as f64);
        assert!(log * 5 < naive, "log {log} B should be well under a fifth of {naive} B");
    }

    #[test]
    fn a_torn_checkpoint_keeps_the_delta_that_precedes_it() {
        let d = dir();
        let (h, all) = build(&d, 12, 6, 3); // rev 6 = 2×3 → delta 6 then checkpoint 6 are written together
        let len = h.file_len();
        drop(h);
        let f = OpenOptions::new().write(true).open(d.join("log.jsonl")).unwrap();
        f.set_len(len - 10).unwrap(); // 只切掉检查点记录的尾部
        drop(f);
        let h = History::open_with(&d, 3).unwrap();
        assert_eq!(h.head(), 6, "the intact delta for rev 6 survives");
        assert_eq!(&*h.blocks_at(6).unwrap(), &all[5], "rebuilt from checkpoint 3 + deltas");
        h.verify().unwrap();
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -- --ignored --nocapture history_bench"]
    fn history_bench() {
        let (n_blocks, revs, every) = (6000usize, 400u64, DEFAULT_CHECKPOINT_EVERY);
        let d = dir();
        let mut b = (0..n_blocks).map(|i| blk(&format!("b{i}"), &format!("{i}. 夜色渐深，城市的灯火一盏盏亮起，他推开门，雨声涌了进来，所有的往事像潮水一样漫过脚踝。"))).collect::<Vec<_>>();
        let mut h = History::create(&d, binding(), 1, &b, every).unwrap();
        let (mut rng, mut c) = (Rng(0xDEAD_BEEF_1234_5679), 0);
        let mut append_ms = Vec::new();
        for rev in 2..=revs {
            let prev = b.clone();
            mutate(&mut b, &mut rng, &mut c);
            let t = std::time::Instant::now();
            h.append(rev, &prev, &b, "t").unwrap();
            append_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
        append_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let full = serde_json::to_vec(&b).unwrap().len();
        println!("bench: {n_blocks} blocks, {revs} revs, checkpoint every {every}; one full copy = {} KB", full / 1024);
        println!("bench: log {} KB vs {} MB as full copies per revision", h.file_len() / 1024, full * revs as usize / 1024 / 1024);
        println!("bench: append median {:.2} ms, p99 {:.2} ms (includes fsync; checkpoints are the slow ones)", append_ms[append_ms.len() / 2], append_ms[append_ms.len() * 99 / 100]);
        drop(h);
        let t = std::time::Instant::now();
        let h = History::open(&d).unwrap();
        println!("bench: reopen {revs} revisions: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        // 最坏情况的冷重建:紧挨在检查点之前的版本(需要回放 every-1 条增量)
        let worst = (revs / every) * every - 1;
        let t = std::time::Instant::now();
        let _ = h.blocks_at(worst).unwrap();
        println!("bench: cold rebuild of rev {worst} (replays {} deltas): {:.1} ms", every - 1, t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        let _ = h.blocks_at(worst).unwrap();
        println!("bench: cached: {:.3} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        h.verify().unwrap();
        println!("bench: verify whole history: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    }
}
