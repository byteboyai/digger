#!/usr/bin/env bash
# Same-machine release comparison against the last pre-optimization editor revision.
# Writes raw JSONL and optional frontend samples; never changes product data/configuration.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:?usage: bench-editor-sync.sh absolute-output-directory [baseline-revision]}"
baseline="${2:-88fcf4e}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
bench_tmp="$(mktemp -d)"
trap 'rm -rf "$bench_tmp"' EXIT
python3 - "$root" "$bench_tmp" "$baseline" <<'PY'
import pathlib, shutil, subprocess, sys
root, target, revision = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3]
files = subprocess.check_output(['git','-C',root,'ls-tree','-r','--name-only',revision,'crates/editor/src']).decode().splitlines()
if not files: raise SystemExit('baseline editor source missing')
for f in files:
    p = target / pathlib.Path(f).relative_to('crates/editor')
    p.parent.mkdir(parents=True,exist_ok=True)
    p.write_bytes(subprocess.check_output(['git','-C',root,'show',f'{revision}:{f}']))
(target/'Cargo.toml').write_text('[package]\nname="digger-editor"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\npulldown-cmark={version="0.13",default-features=false}\nsimilar="2"\nserde={version="1",features=["derive"]}\nserde_json="1"\n')
shutil.copy(pathlib.Path(root)/'Cargo.lock',target/'Cargo.lock')
(target/'examples').mkdir()
shutil.copy(pathlib.Path(root)/'crates/editor/examples/sync_bench.rs',target/'examples/sync_bench.rs')
PY
(cd "$bench_tmp"; DIGGER_BENCH_LABEL="$baseline" DIGGER_BENCH_REPLIES_DIR="$out/replies-before" cargo run --release -q --example sync_bench) > "$out/bridge-before.jsonl"
(cd "$root"; DIGGER_BENCH_LABEL=working-tree DIGGER_BENCH_REPLIES_DIR="$out/replies-after" cargo run --release -q -p digger-editor --example sync_bench) > "$out/bridge-after.jsonl"
python3 "$root/scripts/check-editor-performance.py" "$out/bridge-before.jsonl" "$out/bridge-after.jsonl"
# Build .test-dist with npm test before this optional frontend-only measurement.
if [ -f "$root/crates/desktop/web/.test-dist/src/rust-sync.js" ]; then
  (cd "$root/crates/desktop/web"; node bench/apply.mjs "$out/replies-before" "$out/replies-after") > "$out/frontend.jsonl"
fi
