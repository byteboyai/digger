//! `blocks <corpus.jsonl>`  → prints one JSON line per document with its top-level blocks (Rust parser).
//! `gen <count> <seed>`     → prints random documents (same generator as the block-identity simulator).
use block_identity_spike::parse::{Kind, parse_blocks};
use block_identity_spike::simulate::{Rng, random_content};
use comrak::nodes::NodeValue;
use comrak::{Arena, Options, parse_document};
use serde_json::{Value, json};
use std::io::BufRead;

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
    text.as_bytes()[..at.min(text.len())].iter().filter(|&&b| b == b'\n').count()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("blocks") => {
            let file = std::fs::File::open(&args[2]).expect("open corpus");
            for line in std::io::BufReader::new(file).lines() {
                let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
                let text = v["text"].as_str().unwrap();
                let blocks: Vec<Value> = parse_blocks(text)
                    .iter()
                    .map(|b| {
                        let trimmed_end = b.text.trim_end().len();
                        json!({"class": class(&b.kind), "startLine": line_of(text, b.start), "endLine": line_of(text, b.start + trimmed_end.saturating_sub(1)), "start": b.start, "end": b.start + trimmed_end})
                    })
                    .collect();
                println!("{}", json!({"id": v["id"], "blocks": blocks}));
            }
        }
        Some("blocks-comrak") => {
            let file = std::fs::File::open(&args[2]).expect("open corpus");
            let mut opts = Options::default();
            opts.extension.table = true;
            opts.extension.strikethrough = true;
            opts.extension.tasklist = true;
            opts.extension.footnotes = true;
            for line in std::io::BufReader::new(file).lines() {
                let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
                let text = v["text"].as_str().unwrap();
                let arena = Arena::new();
                let root = parse_document(&arena, text, &opts);
                let blocks: Vec<Value> = root
                    .children()
                    .filter_map(|n| {
                        let d = n.data.borrow();
                        let class = match &d.value {
                            NodeValue::Paragraph => "paragraph",
                            NodeValue::Heading(_) => "heading",
                            NodeValue::List(_) => "list",
                            NodeValue::BlockQuote => "quote",
                            NodeValue::CodeBlock(_) => "code",
                            NodeValue::HtmlBlock(_) => "html",
                            NodeValue::Table(_) => "table",
                            NodeValue::ThematicBreak => "rule",
                            NodeValue::FootnoteDefinition(_) => "footnote",
                            _ => "other",
                        };
                        let lines: Vec<&str> = text.split('\n').collect();
                        let sl = d.sourcepos.start.line.saturating_sub(1);
                        // comrak's end position may include trailing blank lines; the other parsers report the last content line
                        let mut el = d.sourcepos.end.line.saturating_sub(1).min(lines.len().saturating_sub(1));
                        while el > sl && lines[el].trim().is_empty() { el -= 1; }
                        Some(json!({"class": class, "startLine": sl, "endLine": el}))
                    })
                    .collect();
                println!("{}", json!({"id": v["id"], "blocks": blocks}));
            }
        }
        Some("gen") => {
            let count: u64 = args[2].parse().unwrap();
            let seed: u64 = args[3].parse().unwrap();
            for i in 0..count {
                let mut rng = Rng((seed + i * 7919) | 1);
                let n = 10 + rng.below(60);
                let mut text = String::new();
                for _ in 0..n {
                    text.push_str(&random_content(&mut rng, 0.1).render());
                    text.push_str(if rng.chance(0.15) { "\n\n\n" } else { "\n\n" });
                }
                println!("{}", json!({"id": format!("gen-{i}"), "source": "generated", "text": text}));
            }
        }
        _ => eprintln!("usage: blocks <corpus.jsonl> | gen <count> <seed>"),
    }
}
