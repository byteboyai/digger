#!/usr/bin/env bash
# 重建 Markdown 编辑器的网页包 crates/desktop/web/dist/cm.html(提交进 Git,由 digger-desktop 嵌入;ADR-013 §6)。
#
#   scripts/build-editor-web.sh           重建并覆盖 dist/cm.html
#   scripts/build-editor-web.sh --check   重建到临时目录,与已提交的 dist/cm.html 比较;不一致则失败
set -euo pipefail
cd "$(dirname "$0")/../crates/desktop/web"

npm ci --no-audit --no-fund >/dev/null
npx tsc --noEmit

if [ "${1:-}" = "--check" ]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  npx vite build --outDir "$tmp" --emptyOutDir >/dev/null
  if ! cmp -s "$tmp/cm.html" dist/cm.html; then
    echo "已提交的 dist/cm.html 与源码不一致:请运行 scripts/build-editor-web.sh 并提交结果" >&2
    exit 1
  fi
  echo "editor web bundle: ok(已提交的包与源码一致)"
else
  npx vite build >/dev/null
  echo "editor web bundle: 已写入 crates/desktop/web/dist/cm.html"
fi
