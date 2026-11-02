//! Repeatable full-path benchmark for the editor bridge.
//!
//!   cargo run -q --release -p digger-editor --example sync_bench
//!
//! Fixed corpus sizes: 100 / 1000 / 5000 blocks, mixed Chinese / English / emoji. For each size it
//! reports Doc load, one local edit (incremental parse), and full `blocks` + JSON serialisation.
//! These are Debug/Release numbers for the same machine; do not use them as CI latency gates.

use digger_editor::doc::Doc;
use std::time::{Duration, Instant};

fn corpus(blocks: usize) -> String {
    let mut out = String::new();
    for i in 0..blocks {
        match i % 5 {
            0 => out.push_str(&format!("# 第{i}章 Chapter {i}\n\n")),
            1 => out.push_str(&format!(
                "第{i}段正文，讲述第 {i} 个故事，包含中英文 mixed text and emoji 😀。\n\n"
            )),
            2 => out.push_str(&format!("- 项目 {i} alpha\n- 项目 {i} beta\n\n")),
            3 => out.push_str(&format!(
                "> 引用第 {i} 段，the quick brown fox jumps over the lazy dog.\n\n"
            )),
            _ => out.push_str(&format!("```\ncode block {i}\nlet x = {i};\n```\n\n")),
        }
    }
    out
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn main() {
    for blocks in [100usize, 1_000, 5_000] {
        let text = corpus(blocks);
        let runs = 12;
        let mut load = Vec::with_capacity(runs);
        let mut edit = Vec::with_capacity(runs);
        let mut message = Vec::with_capacity(runs);
        let mut bytes = 0usize;
        for _ in 0..runs {
            let t = Instant::now();
            let mut doc = Doc::new();
            doc.load(&text);
            load.push(t.elapsed());

            // type one character in the middle of the document (UTF-16 coordinate)
            let at = doc.len16() / 2;
            let t = Instant::now();
            doc.edit(doc.version, &[(at, at, "字".to_string())])
                .unwrap();
            edit.push(t.elapsed());

            let t = Instant::now();
            let json = doc.blocks_message().to_json();
            message.push(t.elapsed());
            bytes = json.len();
        }
        println!(
            "blocks={blocks:5} bytes={} len16={} blocks_in_doc={} | load={:?} edit={:?} blocks+json={:?}",
            text.len(),
            text.encode_utf16().count(),
            blocks,
            median(load),
            median(edit),
            median(message)
        );
        println!("  message_bytes={bytes}");
    }
}
