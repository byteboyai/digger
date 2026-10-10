//! 极简 Writing Domain AST + ProseMirror 投影 + 语义操作 diff(ADR-006 的最小可验证子集)。

use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Paragraph,
    Heading(u8),
}

#[derive(Debug, Clone, PartialEq)]
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
}

pub fn to_pm(blocks: &[Block]) -> Value {
    let content: Vec<Value> = blocks
        .iter()
        .map(|b| {
            let text = if b.text.is_empty() { json!([]) } else { json!([{"type":"text","text":b.text}]) };
            match b.kind {
                Kind::Paragraph => json!({"type":"paragraph","attrs":{"blockId":b.id},"content":text}),
                Kind::Heading(l) => json!({"type":"heading","attrs":{"blockId":b.id,"level":l},"content":text}),
            }
        })
        .collect();
    json!({"type":"doc","content":content})
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

/// 解析编辑器候选文档。缺失/重复的 blockId、AST 不支持的节点都直接拒绝(ADR-006:不进入正式 AST)。
pub fn parse_pm(doc: &Value) -> Result<(Vec<Block>, Vec<String>), Invalid> {
    if doc["type"] != "doc" {
        return Err(Invalid::NotADoc);
    }
    let mut blocks = Vec::new();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();
    for (i, node) in doc["content"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        let kind = match node["type"].as_str() {
            Some("paragraph") => Kind::Paragraph,
            Some("heading") => Kind::Heading(node["attrs"]["level"].as_u64().unwrap_or(1) as u8),
            other => return Err(Invalid::UnsupportedNode(other.unwrap_or("?").to_string())),
        };
        let id = node["attrs"]["blockId"].as_str().ok_or(Invalid::MissingBlockId(i))?.to_string();
        if !seen.insert(id.clone()) {
            return Err(Invalid::DuplicateBlockId(id));
        }
        let mut text = String::new();
        collect_text(node, &mut text, &mut warnings);
        blocks.push(Block { id, kind, text });
    }
    warnings.dedup();
    Ok((blocks, warnings))
}

pub fn diff(old: &[Block], new: &[Block]) -> Vec<Op> {
    let old_ids: HashSet<&str> = old.iter().map(|b| b.id.as_str()).collect();
    let new_ids: HashSet<&str> = new.iter().map(|b| b.id.as_str()).collect();
    let mut ops = Vec::new();
    for b in old {
        if !new_ids.contains(b.id.as_str()) {
            ops.push(Op::DeleteBlock { id: b.id.clone() });
        }
    }
    for (i, b) in new.iter().enumerate() {
        if !old_ids.contains(b.id.as_str()) {
            ops.push(Op::InsertBlock { id: b.id.clone(), index: i });
            ops.push(Op::ReplaceBlockContent { id: b.id.clone(), text: b.text.clone() });
        }
    }
    for b in new {
        if let Some(o) = old.iter().find(|o| o.id == b.id) {
            if o.kind != b.kind {
                ops.push(Op::SetBlockAttributes { id: b.id.clone(), kind: b.kind.clone() });
            }
            if o.text != b.text {
                ops.push(Op::ReplaceBlockContent { id: b.id.clone(), text: b.text.clone() });
            }
        }
    }
    let common_old: Vec<&str> = old.iter().filter(|b| new_ids.contains(b.id.as_str())).map(|b| b.id.as_str()).collect();
    let common_new: Vec<(usize, &str)> =
        new.iter().enumerate().filter(|(_, b)| old_ids.contains(b.id.as_str())).map(|(i, b)| (i, b.id.as_str())).collect();
    for (k, (idx, id)) in common_new.iter().enumerate() {
        if common_old[k] != *id {
            ops.push(Op::MoveBlock { id: id.to_string(), to: *idx });
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
}
