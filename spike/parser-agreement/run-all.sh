#!/usr/bin/env bash
# Rust (pulldown-cmark) vs Lezer (CodeMirror 6) top-level block agreement, end to end. Outputs overwrite corpus/ and out/.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p corpus out
(cd rust && cargo build --release -q)
BIN=$PWD/$(ls rust/target/*/release/parser-agreement-rust | head -1)
cd node
[ -d node_modules ] || npm install --silent
node extract.mjs
node edge.mjs
"$BIN" gen 300 12345 > ../corpus/generated.jsonl
for c in commonmark-spec gfm-extensions generated workspace edge; do
  "$BIN" blocks ../corpus/$c.jsonl > ../out/$c.rust.jsonl
  "$BIN" blocks-comrak ../corpus/$c.jsonl > ../out/$c.comrak.jsonl
  node lezer.mjs ../corpus/$c.jsonl --footnotes > ../out/$c.lezer.jsonl
done
node edits.mjs                      # also writes corpus/edited.jsonl
"$BIN" blocks ../corpus/edited.jsonl > ../out/edited.rust.jsonl
"$BIN" blocks-comrak ../corpus/edited.jsonl > ../out/edited.comrak.jsonl
node lezer.mjs ../corpus/edited.jsonl --footnotes > ../out/edited.lezer.jsonl
node compare.mjs commonmark-spec gfm-extensions generated workspace edge edited
echo; node conformance.mjs
echo; node check.mjs
