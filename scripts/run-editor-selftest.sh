#!/usr/bin/env bash
#   scripts/run-editor-selftest.sh              harness:T1–T23 + T17b(验证项 4)
#   scripts/run-editor-selftest.sh lifecycle    窗口骨架 + 面板生命周期:点图标 / 收起 / 拖分隔线 / 缩放 / 放大 /
#                                               跨栏移动 / 关闭,读真实 NSView(ADR-013 验证项 5;S1 骨架版)
#   scripts/run-editor-selftest.sh usage        用量配对(S2):无数据 / 拼分栏 / 收起 / 拖分隔线 / 镜像 / 关闭,
#                                               读用量图表 webview 的真实 NSView
#   scripts/run-editor-selftest.sh settings     设置窗隐藏创建 / 绑定 / 首帧后显示,连续打开关闭三次
#
# 在产品二进制上跑编辑器自检(ADR-013 验证项 4):用 harness 页面启动真实的 digger,
# 走生产的 webview 构建器与生产的桥,自动跑 T1–T23,任何一项失败(或没跑完)都以非零退出。
# 需要图形会话(开一个真实窗口)。harness 模式还要求屏幕亮着且未锁:屏幕息屏 / 锁屏时页面的动画帧会被暂停,
# harness 会卡在 T6;lifecycle 模式只读 NSView 状态,不依赖动画帧。
set -euo pipefail
cd "$(dirname "$0")/.."

data="$(mktemp -d)"
trap 'rm -rf "$data"' EXIT
export DIGGER_DATA_DIR="$data"
case "${1:-harness}" in
  harness) export DIGGER_EDITOR_SELFTEST=1 ;;
  lifecycle) export DIGGER_LIFECYCLE_SELFTEST=1 ;;
  usage) export DIGGER_LIFECYCLE_SELFTEST=usage ;;
  settings) export DIGGER_SETTINGS_SELFTEST=1 ;;
  *) echo "用法: $0 [harness|lifecycle|usage|settings]" >&2; exit 2 ;;
esac
# Select only the executable emitted by this successful build, never a cached mtime winner.
cargo build --locked -q -p digger-desktop --bin digger --message-format=json > "$data/build.jsonl"
exe="$(python3 - "$data/build.jsonl" <<'PYBUILD'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
paths = {row["executable"] for row in rows
         if row.get("reason") == "compiler-artifact"
         and row.get("target", {}).get("name") == "digger"
         and "bin" in row.get("target", {}).get("kind", [])
         and row.get("executable")}
if len(paths) != 1:
    raise SystemExit("expected exactly one digger executable from this build")
print(paths.pop())
PYBUILD
)"
[ -x "$exe" ] || { echo "本次构建的 digger 不可执行: $exe" >&2; exit 1; }

out="$data/selftest.log"
set +e
python3 - "$exe" "$out" "${SELFTEST_TIMEOUT:-180}" <<'PYRUN'
import subprocess, sys
with open(sys.argv[2], "w") as output:
    try:
        result = subprocess.run([sys.argv[1]], stdout=output, stderr=subprocess.STDOUT,
                                timeout=float(sys.argv[3]))
        code = result.returncode
    except subprocess.TimeoutExpired:
        code = 124
raise SystemExit(code if code >= 0 else 128 - code)
PYRUN
code=$?
set -e
cat "$out"
# 进程正常退出但没打出 SUMMARY = 自检没跑完(例如提前被关掉),同样算失败
if ! grep -q '^SUMMARY' "$out"; then
  echo "editor selftest: 失败(没有 SUMMARY 行:自检没有跑完)" >&2
  echo "  harness 卡在 T6 多半是屏幕息屏或锁屏(页面的动画帧被暂停):唤醒并解锁屏幕后重试" >&2
  exit 1
fi
if [ "$code" -ne 0 ]; then
  echo "editor selftest: 失败(退出码 $code;被超时终止时为 124)" >&2
  exit 1
fi
echo "editor selftest: ok"
