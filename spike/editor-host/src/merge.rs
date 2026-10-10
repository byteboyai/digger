//! 块级三方合并(base = 宿主编辑所基于的正式版本,local = 宿主候选,official = 当前正式版本)。
//! 已知限制:不合并"移动"——结果顺序以 official 为骨架,local 新增块插在其本地前驱之后。

use crate::doc::Block;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolution {
    Mine,
    Official,
}

#[derive(Debug, PartialEq)]
pub struct Merged {
    pub blocks: Vec<Block>,
    /// 两边都改了且不一致的块 id。非空时 `blocks` 只是"按 official 偏置"的占位,调用方不得提交。
    pub conflicts: Vec<String>,
}

fn index(v: &[Block]) -> HashMap<&str, &Block> {
    v.iter().map(|b| (b.id.as_str(), b)).collect()
}

/// 返回 Ok(最终值) 或 Err(()) 表示冲突。
fn decide<'a>(b: Option<&'a Block>, l: Option<&'a Block>, o: Option<&'a Block>) -> Result<Option<&'a Block>, ()> {
    if l == o || l == b {
        Ok(o)
    } else if o == b {
        Ok(l)
    } else {
        Err(())
    }
}

pub fn merge(base: &[Block], local: &[Block], official: &[Block], res: Option<Resolution>) -> Merged {
    let (bi, li, oi) = (index(base), index(local), index(official));
    let mut conflicts: Vec<String> = Vec::new();
    let mut out: Vec<Block> = Vec::new();

    let mut resolve = |id: &str, b, l, o| -> Option<Block> {
        match decide(b, l, o) {
            Ok(v) => v.cloned(),
            Err(()) => match res {
                Some(Resolution::Mine) => l.cloned(),
                Some(Resolution::Official) => o.cloned(),
                None => {
                    conflicts.push(id.to_string());
                    o.cloned()
                }
            },
        }
    };

    for ob in official {
        let id = ob.id.as_str();
        if let Some(v) = resolve(id, bi.get(id).copied(), li.get(id).copied(), Some(ob)) {
            out.push(v);
        }
    }
    // official 已删除、local 仍有的块:local 没改则一并删除,改了则冲突。
    for lb in local {
        let id = lb.id.as_str();
        if oi.contains_key(id) {
            continue;
        }
        if let Some(b) = bi.get(id).copied() {
            if let Some(v) = resolve(id, Some(b), Some(lb), None) {
                out.push(v);
            }
        }
    }
    // local 新增的块:插在其本地前驱之后。
    let mut prev: Option<&str> = None;
    for lb in local {
        let id = lb.id.as_str();
        if !bi.contains_key(id) && !oi.contains_key(id) {
            let at = prev.and_then(|p| out.iter().position(|b| b.id == p)).map(|i| i + 1).unwrap_or(0);
            out.insert(at.min(out.len()), lb.clone());
        }
        prev = Some(id);
    }
    // 在 official 与 local 都已无该块、base 有的块自然不在结果里(两边都删 = 一致)。
    conflicts.sort();
    conflicts.dedup();
    Merged { blocks: out, conflicts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Kind;

    fn p(id: &str, t: &str) -> Block {
        Block { id: id.into(), kind: Kind::Paragraph, text: t.into() }
    }
    fn texts(m: &Merged) -> Vec<String> {
        m.blocks.iter().map(|b| format!("{}:{}", b.id, b.text)).collect()
    }

    #[test]
    fn disjoint_edits_merge() {
        let base = vec![p("a", "1"), p("b", "2")];
        let local = vec![p("a", "1L"), p("b", "2")];
        let off = vec![p("a", "1"), p("b", "2A")];
        let m = merge(&base, &local, &off, None);
        assert!(m.conflicts.is_empty());
        assert_eq!(texts(&m), ["a:1L", "b:2A"]);
    }

    #[test]
    fn same_block_different_edits_conflict_and_resolve() {
        let base = vec![p("a", "1")];
        let local = vec![p("a", "mine")];
        let off = vec![p("a", "theirs")];
        let m = merge(&base, &local, &off, None);
        assert_eq!(m.conflicts, ["a"]);
        assert_eq!(texts(&merge(&base, &local, &off, Some(Resolution::Mine))), ["a:mine"]);
        assert_eq!(texts(&merge(&base, &local, &off, Some(Resolution::Official))), ["a:theirs"]);
    }

    #[test]
    fn identical_edits_agree() {
        let base = vec![p("a", "1")];
        let both = vec![p("a", "same")];
        assert!(merge(&base, &both, &both, None).conflicts.is_empty());
    }

    #[test]
    fn delete_vs_edit_conflicts_but_delete_vs_untouched_deletes() {
        let base = vec![p("a", "1"), p("b", "2")];
        // local 删 a,official 改 a → 冲突
        let m = merge(&base, &[p("b", "2")], &[p("a", "1!"), p("b", "2")], None);
        assert_eq!(m.conflicts, ["a"]);
        // local 删 a,official 没动 a → 删除
        let m = merge(&base, &[p("b", "2")], &base, None);
        assert!(m.conflicts.is_empty());
        assert_eq!(texts(&m), ["b:2"]);
        // official 删 a,local 没动 a → 删除;local 改了 a → 冲突
        let m = merge(&base, &base, &[p("b", "2")], None);
        assert_eq!(texts(&m), ["b:2"]);
        let m = merge(&base, &[p("a", "1L"), p("b", "2")], &[p("b", "2")], None);
        assert_eq!(m.conflicts, ["a"]);
    }

    #[test]
    fn local_insert_lands_after_its_predecessor() {
        let base = vec![p("a", "1"), p("b", "2")];
        let local = vec![p("a", "1"), p("n", "new"), p("b", "2")];
        let off = vec![p("a", "1"), p("b", "2A"), p("c", "3")];
        let m = merge(&base, &local, &off, None);
        assert!(m.conflicts.is_empty());
        assert_eq!(texts(&m), ["a:1", "n:new", "b:2A", "c:3"]);
    }
}
