//! 夹具生成器:读 JSONL(`{"id","text"}`),用**生产**解析器输出每段文本的顶层块(类别 + 起止行)。
//! 编辑器页面的 harness T10 拿它当基准,检查页面里的 Lezer 解析器与 Rust 一致(ADR-013 验证项 7)。
//!
//!   blocks-fixture <input.jsonl>

use std::io::BufRead;

use digger_editor::blocks::parse::{Kind, parse_blocks};
use serde_json::{Value, json};

fn class(k: &Kind) -> &'static str {
    match k {
        Kind::Heading(_) => "heading",
        Kind::Paragraph => "paragraph",
        Kind::List => "list",
        Kind::Quote => "quote",
        Kind::Code => "code",
        Kind::Table => "table",
        Kind::Rule => "rule",
        Kind::Html => "html",
        Kind::Footnote => "footnote",
        Kind::Other => "other",
    }
}

/// 0-based line number of byte offset `at` (lines end at '\n').
fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: blocks-fixture <input.jsonl>");
    let file = std::fs::File::open(path).expect("open input");
    for line in std::io::BufReader::new(file).lines() {
        let v: Value = serde_json::from_str(&line.expect("read line")).expect("json line");
        let text = v["text"].as_str().expect("text");
        let blocks: Vec<Value> = parse_blocks(text)
            .iter()
            .map(|b| {
                let trimmed_end = b.text.trim_end().len();
                json!({"class": class(&b.kind), "startLine": line_of(text, b.start), "endLine": line_of(text, b.start + trimmed_end.saturating_sub(1))})
            })
            .collect();
        println!("{}", json!({"id": v["id"], "blocks": blocks}));
    }
}
