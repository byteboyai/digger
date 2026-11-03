#!/usr/bin/env python3
"""Compare same-machine release sync_bench JSONL files. Usage: before.jsonl after.jsonl."""
import json
import sys


def read(path):
    rows = [json.loads(line) for line in open(path) if line.strip()]
    summaries = {row["blocks"]: row for row in rows if row["kind"] == "summary"}
    for count in (100, 1000, 5000):
        samples = [r for r in rows if r["kind"] == "sample" and r["blocks"] == count]
        if len(samples) < 40 or count not in summaries:
            raise ValueError(f"{path}: missing raw samples / summary for {count}")
    return summaries, rows


before, _ = read(sys.argv[1])
after, rows = read(sys.argv[2])
failures = []
for count in (100, 1000, 5000):
    ratio = before[count]["p95_ns"] / after[count]["p95_ns"]
    remaining = after[count]["max_reply_bytes"] / before[count]["max_reply_bytes"]
    print(f"{count} blocks: p95 speedup={ratio:.2f}x, reply reduction={1-remaining:.4%}")
    if count == 5000:
        if ratio < 2 or remaining > 0.1:
            failures.append("5000-block Rust p95 / reply size threshold")
        if any(r.get("returned_blocks", 999) > 8 for r in rows if r["kind"] == "sample" and r["blocks"] == count):
            failures.append("5000-block local edit returned more than eight blocks")
    if count == 100 and ratio < 1/1.1:
        failures.append("100-block Rust p95 regressed more than 10%")
if failures:
    sys.exit("FAIL: " + ", ".join(failures))
print("PASS: Rust Bridge + JSON thresholds (does not certify a real webview)")
