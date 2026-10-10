#!/usr/bin/env bash
# 崩溃恢复:进程 A 在日志落盘后 abort,进程 B 用同一数据目录启动并验证恢复流程。
set -u
cd "$(dirname "$0")"
BIN=$(ls target/*/debug/editor-host-spike 2>/dev/null | head -1)
[ -x "$BIN" ] || { echo "build first: cargo build"; exit 1; }
rc=0
for variant in accept discard stale; do
  dir=$(mktemp -d)
  echo "=== crash-a ($variant): killing the process after the journal is written"
  SPIKE_DIR=$dir SELFTEST=crash-a "$BIN" 2>&1 | grep -E "CRASH-A|FAIL"
  echo "    exit status of crashed process: ${PIPESTATUS[0]} (134 = SIGABRT)"
  ls "$dir"
  echo "=== crash-b-$variant: restart on the same data dir"
  SPIKE_DIR=$dir SELFTEST=crash-b-$variant "$BIN" 2>&1 | grep -E "PASS|FAIL|recovery"
  [ "${PIPESTATUS[0]}" = 0 ] || rc=1
  rm -rf "$dir"
done
exit $rc
