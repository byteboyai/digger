//! Production Bridge + JSON benchmark. Fixed corpus; JSONL raw samples and p50/p95.
//! Run in release, outside other workloads. DIGGER_BENCH_LABEL identifies the revision.
use digger_editor::bridge::{Bridge, Handled, Mode};
use digger_editor::doc::Doc;
use serde_json::json;
use std::time::Instant;

fn corpus(blocks: usize) -> String {
    let mut text = String::new();
    for i in 0..blocks {
        text.push_str(&match i % 5 {
            0 => format!("# 第{i}章 Chapter {i}\n\n"),
            1 => format!("第{i}段正文，中英文 mixed text and emoji 😀。\n\n"),
            2 => format!("- 项目 {i} alpha\n- 项目 {i} beta\n\n"),
            3 => format!("> 引用第 {i} 段，the quick brown fox.\n\n"),
            _ => format!("```\ncode block {i}\nlet x = {i};\n```\n\n"),
        });
    }
    text
}
fn percentile(values: &mut [u64], p: usize) -> u64 {
    values.sort_unstable();
    values[(values.len() * p).div_ceil(100) - 1]
}
fn main() {
    let label = std::env::var("DIGGER_BENCH_LABEL").unwrap_or_else(|_| "working-tree".into());
    for blocks in [100usize, 1000, 5000] {
        let text = corpus(blocks);
        let mut samples = Vec::new();
        let mut sizes = Vec::new();
        for sample in 0..44 {
            let mut bridge = Bridge::new(&text, Mode::Normal, "bench");
            let heading = (blocks / 4 * (1 + sample % 3)) / 5 * 5;
            let at_byte = text.find(&format!("# 第{heading}章")).unwrap() + 2;
            let at = text[..at_byte].encode_utf16().count();
            let load_start = Instant::now();
            let mut doc = Doc::new();
            doc.load(&text);
            let doc_load_ns = load_start.elapsed().as_nanos() as u64;
            let edit_start = Instant::now();
            doc.edit(doc.version, &[(at, at, "字".into())]).unwrap();
            let doc_edit_ns = edit_start.elapsed().as_nanos() as u64;
            let raw = json!({"n":"bench", "t":"edit", "request_id":1, "base":bridge.doc().version,
                "changes":[{"from":at,"to":at,"insert":"字"}]})
            .to_string();
            let initial = if std::env::var_os("DIGGER_BENCH_REPLIES_DIR").is_some() && sample == 3 {
                Some(bridge.doc().blocks_message().to_json())
            } else {
                None
            };
            let start = Instant::now();
            let Handled::Reply(reply) = bridge.handle(&raw) else {
                panic!("not a reply")
            };
            let json = reply.to_json();
            let nanos = start.elapsed().as_nanos() as u64;
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(
                value["t"] == "blocks" || value["t"] == "incremental",
                "{json}"
            );
            if let Some(initial) = initial {
                let dir = std::env::var("DIGGER_BENCH_REPLIES_DIR").unwrap();
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(format!("{dir}/{blocks}.json"), json!({"text":text,"initial":serde_json::from_str::<serde_json::Value>(&initial).unwrap(),"reply":value}).to_string()).unwrap();
            }
            if sample < 3 {
                continue;
            }
            samples.push(nanos);
            sizes.push(json.len());
            println!(
                "{}",
                json!({"kind":"sample","label":label,"blocks":blocks,"sample":sample-3,
                "ns":nanos,"doc_load_ns":doc_load_ns,"doc_edit_ns":doc_edit_ns,"reply_bytes":json.len(),"reply_type":value["t"],
                "returned_blocks":value["blocks"].as_array().unwrap().len()})
            );
        }
        let p50 = percentile(&mut samples, 50);
        let p95 = percentile(&mut samples, 95);
        println!(
            "{}",
            json!({"kind":"summary","label":label,"blocks":blocks,"samples":samples.len(),
            "p50_ns":p50,"p95_ns":p95,"max_reply_bytes":sizes.iter().max().unwrap()})
        );
    }
}
