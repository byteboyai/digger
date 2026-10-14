#!/usr/bin/env bash
# 在产品二进制上跑编辑器自检(ADR-013 验证项 4):用 harness 页面启动真实的 digger,
# 走生产的 webview 构建器与生产的桥,自动跑 T1–T23,任何一项失败(或没跑完)都以非零退出。
# 需要图形会话(开一个真实窗口)。
set -euo pipefail
cd "$(dirname "$0")/.."

data="$(mktemp -d)"
trap 'rm -rf "$data"' EXIT
export DIGGER_DATA_DIR="$data" DIGGER_EDITOR_SELFTEST=1
cargo build -q -p digger-desktop --bin digger
bin="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')"
exe="$( (ls -t "$bin"/*/debug/digger "$bin"/debug/digger 2>/dev/null || true) | head -1)"
[ -x "$exe" ] || { echo "找不到 digger 二进制(在 $bin 下)" >&2; exit 1; }

out="$data/selftest.log"
set +e
( "$exe" >"$out" 2>&1 ) &
pid=$!
( sleep "${SELFTEST_TIMEOUT:-180}"; kill "$pid" 2>/dev/null ) &
guard=$!
wait "$pid"
code=$?
kill "$guard" 2>/dev/null
set -e
cat "$out"
if [ "$code" -ne 0 ]; then
  echo "editor selftest: 失败(退出码 $code;被超时终止时为 143)" >&2
  exit 1
fi
echo "editor selftest: ok"
