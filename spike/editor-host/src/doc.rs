//! 极简 Writing Domain AST + ProseMirror 投影 + 语义操作 diff(ADR-006 的最小可验证子集)。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Paragraph,
    Heading(u8),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    InsertBlock { id: String, index: usize },
    DeleteBlock { id: String },
    MoveBlock { id: String, to: usize },
    ReplaceBlockContent { id: String, text: String },
    SetBlockAttributes { id: String, kind: Kind },
}

#[derive(Debug, PartialEq)]
pub enum Invalid {
    NotADoc,
    UnsupportedNode(String),
    MissingBlockId(usize),
    DuplicateBlockId(String),
    BadDelta(String),
}

pub fn block_to_pm(b: &Block) -> Value {
    let text = if b.text.is_empty() { json!([]) } else { json!([{"type":"text","text":b.text}]) };
    match b.kind {
        Kind::Paragraph => json!({"type":"paragraph","attrs":{"blockId":b.id},"content":text}),
        Kind::Heading(l) => json!({"type":"heading","attrs":{"blockId":b.id,"level":l},"content":text}),
    }
}

pub fn to_pm(blocks: &[Block]) -> Value {
    json!({"type":"doc","content": blocks.iter().map(block_to_pm).collect::<Vec<_>>()})
}

fn collect_text(node: &Value, out: &mut String, warnings: &mut Vec<String>) {
    match node["type"].as_str() {
        Some("text") => {
            out.push_str(node["text"].as_str().unwrap_or(""));
            if node.get("marks").is_some() {
                warnings.push("marks dropped (spike AST is plain text)".into());
            }
        }
        Some("hardBreak") => out.push('\n'),
        _ => {
            if let Some(children) = node["content"].as_array() {
                for c in children {
                    collect_text(c, out, warnings);
                }
            }
        }
    }
}

fn parse_block(node: &Value, index: usize, warnings: &mut Vec<String>) -> Result<Block, Invalid> {
    let kind = match node["type"].as_str() {
        Some("paragraph") => Kind::Paragraph,
        Some("heading") => Kind::Heading(node["attrs"]["level"].as_u64().unwrap_or(1) as u8),
        other => return Err(Invalid::UnsupportedNode(other.unwrap_or("?").to_string())),
    };
    let id = node["attrs"]["blockId"].as_str().ok_or(Invalid::MissingBlockId(index))?.to_string();
    let mut text = String::new();
    collect_text(node, &mut text, warnings);
    Ok(Block { id, kind, text })
}

/// 解析整篇编辑器文档(load / 报告 / 测试用)。缺失/重复的 blockId、AST 不支持的节点都直接拒绝。
pub fn parse_pm(doc: &Value) -> Result<(Vec<Block>, Vec<String>), Invalid> {
    if doc["type"] != "doc" {
        return Err(Invalid::NotADoc);
    }
    let mut blocks = Vec::new();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();
    for (i, node) in doc["content"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        let b = parse_block(node, i, &mut warnings)?;
        if !seen.insert(b.id.clone()) {
            return Err(Invalid::DuplicateBlockId(b.id));
        }
        blocks.push(b);
    }
    warnings.dedup();
    Ok((blocks, warnings))
}

/// 块级增量:相对宿主基线(= Rust 侧 `history[baseRev]`)只带改动的块。
/// 顺序用"锚点"表达:只有前驱发生变化的块才出现在 `placements`(`after` = 新顺序里的前驱 id,`None` = 首块),
/// 其余块沿用基线(去掉 removed 之后)的前驱关系。原地编辑 → placements 为空;插入/移动 → 每处 1–2 条。
#[derive(Debug, Default, PartialEq)]
pub struct Delta {
    pub changed: Vec<Block>,
    pub removed: Vec<String>,
    pub placements: Vec<(String, Option<String>)>,
}

pub fn parse_delta(p: &Value) -> Result<(Delta, Vec<String>), Invalid> {
    let bad = |m: &str| Invalid::BadDelta(m.to_string());
    let mut warnings = Vec::new();
    let mut changed = Vec::new();
    for (i, n) in p["changed"].as_array().ok_or_else(|| bad("changed must be an array"))?.iter().enumerate() {
        changed.push(parse_block(n, i, &mut warnings)?);
    }
    let removed = p["removed"]
        .as_array()
        .ok_or_else(|| bad("removed must be an array"))?
        .iter()
        .map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("removed must hold strings")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut placements = Vec::new();
    if !p["placements"].is_null() {
        for pl in p["placements"].as_array().ok_or_else(|| bad("placements must be an array"))? {
            let id = pl["id"].as_str().ok_or_else(|| bad("placement needs an id"))?.to_string();
            let after = match &pl["after"] {
                Value::Null => None,
                Value::String(a) => Some(a.clone()),
                _ => return Err(bad("placement.after must be a string or null")),
            };
            placements.push((id, after));
        }
    }
    warnings.dedup();
    Ok((Delta { changed, removed, placements }, warnings))
}

impl Delta {
    /// 与宿主上行增量同形的 JSON(下行消息复用)。
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "changed": self.changed.iter().map(block_to_pm).collect::<Vec<_>>(),
            "removed": self.removed,
        });
        if !self.placements.is_empty() {
            v["placements"] = self.placements.iter().map(|(id, after)| json!({"id": id, "after": after})).collect();
        }
        v
    }
}

/// `apply_delta` 的逆:算出把 `old` 变成 `new` 的最小块级增量,O(n)。
pub fn make_delta(old: &[Block], new: &[Block]) -> Delta {
    let old_map: HashMap<&str, &Block> = old.iter().map(|b| (b.id.as_str(), b)).collect();
    let new_ids: HashSet<&str> = new.iter().map(|b| b.id.as_str()).collect();
    let changed = new.iter().filter(|b| old_map.get(b.id.as_str()).is_none_or(|o| *o != *b)).cloned().collect();
    let removed = old.iter().filter(|b| !new_ids.contains(b.id.as_str())).map(|b| b.id.clone()).collect();
    let mut kept_pred: HashMap<&str, Option<&str>> = HashMap::new();
    let mut prev: Option<&str> = None;
    for b in old.iter().filter(|b| new_ids.contains(b.id.as_str())) {
        kept_pred.insert(b.id.as_str(), prev);
        prev = Some(b.id.as_str());
    }
    let mut placements = Vec::new();
    prev = None;
    for b in new {
        if kept_pred.get(b.id.as_str()) != Some(&prev) {
            placements.push((b.id.clone(), prev.map(str::to_string)));
        }
        prev = Some(b.id.as_str());
    }
    Delta { changed, removed, placements }
}

/// 把增量应用到基线上得到完整候选文档,O(n)。任何不自洽(重复、未知 id、新块无锚点、环、
/// 两个块抢同一个前驱)都拒绝。
pub fn apply_delta(base: &[Block], d: &Delta) -> Result<Vec<Block>, Invalid> {
    let bad = |m: String| Invalid::BadDelta(m);
    let base_map: HashMap<&str, &Block> = base.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut changed: HashMap<&str, &Block> = HashMap::with_capacity(d.changed.len());
    for b in &d.changed {
        if changed.insert(b.id.as_str(), b).is_some() {
            return Err(Invalid::DuplicateBlockId(b.id.clone()));
        }
    }
    let mut removed: HashSet<&str> = HashSet::with_capacity(d.removed.len());
    for r in &d.removed {
        if !base_map.contains_key(r.as_str()) {
            return Err(bad(format!("removed id not in base: {r}")));
        }
        removed.insert(r.as_str());
    }
    // 基线去掉 removed 之后的前驱关系。
    let mut pred: HashMap<&str, Option<&str>> = HashMap::with_capacity(base.len());
    let mut prev: Option<&str> = None;
    for b in base.iter().filter(|b| !removed.contains(b.id.as_str())) {
        pred.insert(b.id.as_str(), prev);
        prev = Some(b.id.as_str());
    }
    let new_ids: Vec<&str> = d.changed.iter().map(|c| c.id.as_str()).filter(|id| !base_map.contains_key(id)).collect();
    let mut placed: HashSet<&str> = HashSet::new();
    for (id, after) in &d.placements {
        if !placed.insert(id.as_str()) {
            return Err(Invalid::DuplicateBlockId(id.clone()));
        }
        let known = pred.contains_key(id.as_str()) || changed.contains_key(id.as_str());
        if !known || removed.contains(id.as_str()) {
            return Err(bad(format!("placement for unknown or removed id: {id}")));
        }
        pred.insert(id.as_str(), after.as_deref());
    }
    for id in &new_ids {
        if !placed.contains(id) {
            return Err(bad(format!("new block without placement: {id}")));
        }
    }
    // 由前驱关系还原顺序:每个前驱(含"首块"=None)至多有一个后继,并且链必须穿过全部块。
    let mut next: HashMap<Option<&str>, &str> = HashMap::with_capacity(pred.len());
    for (id, p) in &pred {
        if let Some(a) = p {
            if !pred.contains_key(a) {
                return Err(bad(format!("placement anchor does not exist: {a}")));
            }
        }
        if next.insert(*p, id).is_some() {
            return Err(bad(format!("two blocks claim the same predecessor: {p:?}")));
        }
    }
    let mut out = Vec::with_capacity(pred.len());
    let mut cur: Option<&str> = None;
    while let Some(id) = next.get(&cur) {
        let b = changed.get(id).or_else(|| base_map.get(id)).ok_or_else(|| bad(format!("unknown id: {id}")))?;
        out.push((*b).clone());
        cur = Some(id);
        if out.len() > pred.len() {
            return Err(bad("cycle in placements".into()));
        }
    }
    if out.len() != pred.len() {
        return Err(bad(format!("placements do not form one chain ({} of {} blocks reachable)", out.len(), pred.len())));
    }
    // 没有改动 placements 的"原地编辑"必须仍然只替换内容,不引入基线之外的块。
    if let Some(c) = d.changed.iter().find(|c| !pred.contains_key(c.id.as_str())) {
        return Err(bad(format!("changed block not part of the document: {}", c.id)));
    }
    Ok(out)
}

/// 线性 diff(O(n)):按 id 建索引,不在循环里做线性查找。
pub fn diff(old: &[Block], new: &[Block]) -> Vec<Op> {
    let old_map: HashMap<&str, &Block> = old.iter().map(|b| (b.id.as_str(), b)).collect();
    let new_map: HashMap<&str, &Block> = new.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut ops = Vec::new();
    for b in old {
        if !new_map.contains_key(b.id.as_str()) {
            ops.push(Op::DeleteBlock { id: b.id.clone() });
        }
    }
    for (i, b) in new.iter().enumerate() {
        match old_map.get(b.id.as_str()) {
            None => {
                ops.push(Op::InsertBlock { id: b.id.clone(), index: i });
                ops.push(Op::ReplaceBlockContent { id: b.id.clone(), text: b.text.clone() });
            }
            Some(o) => {
                if o.kind != b.kind {
                    ops.push(Op::SetBlockAttributes { id: b.id.clone(), kind: b.kind.clone() });
                }
                if o.text != b.text {
                    ops.push(Op::ReplaceBlockContent { id: b.id.clone(), text: b.text.clone() });
                }
            }
        }
    }
    let common_old = old.iter().filter(|b| new_map.contains_key(b.id.as_str())).map(|b| b.id.as_str());
    let common_new = new.iter().enumerate().filter(|(_, b)| old_map.contains_key(b.id.as_str()));
    for (o, (idx, n)) in common_old.zip(common_new) {
        if o != n.id {
            ops.push(Op::MoveBlock { id: n.id.clone(), to: idx });
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, t: &str) -> Block {
        Block { id: id.into(), kind: Kind::Paragraph, text: t.into() }
    }

    #[test]
    fn projection_round_trips() {
        let blocks = vec![Block { id: "h".into(), kind: Kind::Heading(1), text: "标题".into() }, p("a", "正文"), p("e", "")];
        let (back, w) = parse_pm(&to_pm(&blocks)).unwrap();
        assert_eq!(back, blocks);
        assert!(w.is_empty());
    }

    #[test]
    fn rejects_duplicate_missing_and_unsupported() {
        let dup = json!({"type":"doc","content":[
            {"type":"paragraph","attrs":{"blockId":"x"}},{"type":"paragraph","attrs":{"blockId":"x"}}]});
        assert_eq!(parse_pm(&dup).unwrap_err(), Invalid::DuplicateBlockId("x".into()));
        let miss = json!({"type":"doc","content":[{"type":"paragraph","attrs":{"blockId":null}}]});
        assert_eq!(parse_pm(&miss).unwrap_err(), Invalid::MissingBlockId(0));
        let bad = json!({"type":"doc","content":[{"type":"codeBlock","attrs":{"blockId":"c"}}]});
        assert_eq!(parse_pm(&bad).unwrap_err(), Invalid::UnsupportedNode("codeBlock".into()));
    }

    #[test]
    fn diff_yields_semantic_ops() {
        let old = vec![p("a", "1"), p("b", "2"), p("c", "3")];
        let new = vec![p("a", "1!"), p("n", "新"), p("c", "3")];
        let ops = diff(&old, &new);
        assert!(ops.contains(&Op::DeleteBlock { id: "b".into() }));
        assert!(ops.contains(&Op::InsertBlock { id: "n".into(), index: 1 }));
        assert!(ops.contains(&Op::ReplaceBlockContent { id: "a".into(), text: "1!".into() }));
        assert!(!ops.iter().any(|o| matches!(o, Op::MoveBlock { .. })));
    }

    #[test]
    fn diff_detects_move() {
        let old = vec![p("a", "1"), p("b", "2")];
        let new = vec![p("b", "2"), p("a", "1")];
        assert!(diff(&old, &new).iter().any(|o| matches!(o, Op::MoveBlock { .. })));
    }

    fn blocks(n: usize) -> Vec<Block> {
        (0..n).map(|i| p(&format!("id{i}"), &format!("text {i}"))).collect()
    }
    fn d(changed: Vec<Block>, removed: &[&str], placements: &[(&str, Option<&str>)]) -> Delta {
        Delta {
            changed,
            removed: removed.iter().map(|s| s.to_string()).collect(),
            placements: placements.iter().map(|(i, a)| (i.to_string(), a.map(str::to_string))).collect(),
        }
    }
    fn ids(v: &[Block]) -> Vec<&str> {
        v.iter().map(|b| b.id.as_str()).collect()
    }

    #[test]
    fn delta_edit_in_place_needs_no_placements() {
        let base = blocks(3);
        let out = apply_delta(&base, &d(vec![p("id1", "edited")], &[], &[])).unwrap();
        assert_eq!(out[1].text, "edited");
        assert_eq!(out[0], base[0]);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn delta_insert_in_middle_and_at_ends() {
        let base = blocks(3); // id0 id1 id2
        // 在 id0 后插入 n:n 自己 + 被挤后的 id1 两条锚点
        let out = apply_delta(&base, &d(vec![p("n", "new")], &[], &[("n", Some("id0")), ("id1", Some("n"))])).unwrap();
        assert_eq!(ids(&out), ["id0", "n", "id1", "id2"]);
        // 末尾追加:只需 n 一条
        let out = apply_delta(&base, &d(vec![p("n", "new")], &[], &[("n", Some("id2"))])).unwrap();
        assert_eq!(ids(&out), ["id0", "id1", "id2", "n"]);
        // 插到最前:n(首块) + 原首块
        let out = apply_delta(&base, &d(vec![p("n", "new")], &[], &[("n", None), ("id0", Some("n"))])).unwrap();
        assert_eq!(ids(&out), ["n", "id0", "id1", "id2"]);
    }

    #[test]
    fn delta_delete_and_move() {
        let base = blocks(4); // id0..id3
        let out = apply_delta(&base, &d(vec![], &["id1"], &[])).unwrap();
        assert_eq!(ids(&out), ["id0", "id2", "id3"]);
        // 把 id0 移到 id2 之后:id0 新前驱 id2,原后继 id1 的前驱变成 None(首块),id3 的前驱变成 id0
        let out = apply_delta(&base, &d(vec![], &[], &[("id1", None), ("id0", Some("id2")), ("id3", Some("id0"))])).unwrap();
        assert_eq!(ids(&out), ["id1", "id2", "id0", "id3"]);
    }

    #[test]
    fn delta_rejects_inconsistent_input() {
        let base = blocks(3);
        let bad = |r: Result<Vec<Block>, Invalid>| matches!(r, Err(Invalid::BadDelta(_)));
        assert!(bad(apply_delta(&base, &d(vec![p("n", "x")], &[], &[]))), "new block needs a placement");
        assert!(bad(apply_delta(&base, &d(vec![], &["ghost"], &[]))), "removed id unknown");
        assert!(bad(apply_delta(&base, &d(vec![], &[], &[("ghost", None)]))), "placement for unknown id");
        assert!(bad(apply_delta(&base, &d(vec![p("n", "x")], &[], &[("n", Some("ghost"))]))), "anchor does not exist");
        assert!(bad(apply_delta(&base, &d(vec![p("n", "x")], &[], &[("n", Some("id0"))]))), "id0 would have two successors");
        assert!(bad(apply_delta(&base, &d(vec![], &[], &[("id0", Some("id2"))]))), "cycle / broken chain");
        assert!(bad(apply_delta(&base, &d(vec![], &["id0"], &[("id0", None)]))), "placing a removed id");
        assert!(matches!(apply_delta(&base, &d(vec![], &[], &[("id1", None), ("id1", None)])), Err(Invalid::DuplicateBlockId(_))), "duplicate placement");
        assert!(matches!(apply_delta(&base, &d(vec![p("id1", "a"), p("id1", "b")], &[], &[])), Err(Invalid::DuplicateBlockId(_))), "dup changed");
    }

    #[test]
    fn parse_delta_validates_shape() {
        let ok = json!({"changed":[{"type":"paragraph","attrs":{"blockId":"a"},"content":[{"type":"text","text":"x"}]}],"removed":["b"]});
        let (delta, _) = parse_delta(&ok).unwrap();
        assert_eq!(delta.changed[0].text, "x");
        assert!(delta.placements.is_empty());
        assert!(parse_delta(&json!({"changed":"no","removed":[]})).is_err());
        assert!(parse_delta(&json!({"changed":[],"removed":[1]})).is_err());
        assert!(parse_delta(&json!({"changed":[],"removed":[],"placements":[{"id":"a","after":5}]})).is_err());
        let (pl, _) = parse_delta(&json!({"changed":[],"removed":[],"placements":[{"id":"a","after":null},{"id":"b","after":"a"}]})).unwrap();
        assert_eq!(pl.placements, vec![("a".to_string(), None), ("b".to_string(), Some("a".to_string()))]);
        assert_eq!(
            parse_delta(&json!({"changed":[{"type":"codeBlock","attrs":{"blockId":"c"}}],"removed":[]})).unwrap_err(),
            Invalid::UnsupportedNode("codeBlock".into())
        );
    }

    #[test]
    fn apply_delta_scales_linearly() {
        let base = blocks(20_000);
        let t = std::time::Instant::now();
        let out = apply_delta(&base, &d(vec![p("n", "x")], &[], &[("n", Some("id10000")), ("id10001", Some("n"))])).unwrap();
        assert_eq!(out.len(), 20_001);
        assert!(t.elapsed().as_millis() < 500, "apply_delta took {:?}", t.elapsed());
    }

    #[test]
    fn diff_scales_linearly() {
        let old = blocks(20_000);
        let mut new = old.clone();
        new[10_000].text.push('!');
        let t = std::time::Instant::now();
        let ops = diff(&old, &new);
        assert_eq!(ops.len(), 1);
        // 平方实现在 20000 块上要数秒;线性实现远低于此(调试构建也如此)。
        assert!(t.elapsed().as_millis() < 500, "diff took {:?}", t.elapsed());
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    #[test]
    fn make_delta_is_the_inverse_of_apply_delta() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let mut next_id = 0;
        for trial in 0..500 {
            let old = blocks(rng.below(16));
            let mut new = old.clone();
            for _ in 0..rng.below(6) {
                match rng.below(4) {
                    0 if !new.is_empty() => {
                        let i = rng.below(new.len());
                        new.remove(i);
                    }
                    1 if !new.is_empty() => {
                        let i = rng.below(new.len());
                        new[i].text.push('x');
                    }
                    2 => {
                        next_id += 1;
                        let at = rng.below(new.len() + 1);
                        new.insert(at, p(&format!("new{next_id}"), "n"));
                    }
                    _ if new.len() > 1 => {
                        let from = rng.below(new.len());
                        let b = new.remove(from);
                        let to = rng.below(new.len() + 1);
                        new.insert(to, b);
                    }
                    _ => {}
                }
            }
            let delta = make_delta(&old, &new);
            let got = apply_delta(&old, &delta).unwrap_or_else(|e| panic!("trial {trial}: {e:?}"));
            assert_eq!(got, new, "trial {trial}");
            // JSON 往返(下行消息的真实路径)
            let (back, _) = parse_delta(&delta.to_json()).unwrap();
            assert_eq!(apply_delta(&old, &back).unwrap(), new, "trial {trial} via json");
        }
    }

    #[test]
    fn make_delta_is_minimal_for_local_changes() {
        let old = blocks(1000);
        let mut new = old.clone();
        new[500].text.push('!');
        let d = make_delta(&old, &new);
        assert_eq!((d.changed.len(), d.removed.len(), d.placements.len()), (1, 0, 0));
        let mut new = old.clone();
        new.insert(500, p("n", "x"));
        let d = make_delta(&old, &new);
        assert_eq!((d.changed.len(), d.removed.len(), d.placements.len()), (1, 0, 2));
    }
}
