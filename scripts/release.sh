#!/usr/bin/env bash
#
# release.sh — Digger 发布候选构建:门禁 → 升版本 → 打包 → 压缩。
#
# 流程对应 Dozer 的 build-macos-app.sh(release 先 bump-version、再过门禁、再装 bundle),
# 这里把它们串成一个入口。产物是 ad-hoc 签名的候选包:没有 Developer ID 证书与公证
# 凭据时,不能称为"已发布版本"(见 package-macos.sh 的说明)。
#
# 本脚本只改工作区里的版本号文件,**不提交、不打 tag、不推送**(Digger 没有 remote)。
# 版本号改动留在工作区,由你审阅后自己提交。
#
# 用法: scripts/release.sh [--no-bump] [--skip-tests] [--allow-dirty] [--dry-run]
#   --no-bump      不升版本号(重打同一版本)
#   --skip-tests   跳过 cargo test --workspace(编辑器属性测试约 7 分钟)
#   --allow-dirty  允许工作区有未提交改动(默认拒绝,避免把别人的半成品打进发布包)
#   --dry-run      只跑门禁并打印将要做的版本变更,不改文件、不打包
set -euo pipefail

BUMP=1; TESTS=1; ALLOW_DIRTY=0; DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --no-bump) BUMP=0 ;;
    --skip-tests) TESTS=0 ;;
    --allow-dirty) ALLOW_DIRTY=1 ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
step() { printf '\n==> %s\n' "$*"; }

step "工作区状态"
if [[ "$ALLOW_DIRTY" -eq 0 && -n "$(git status --porcelain)" ]]; then
  echo "error: 工作区有未提交改动(用 --allow-dirty 跳过):" >&2
  git status --short >&2
  exit 1
fi
echo "branch: $(git branch --show-current)  head: $(git rev-parse --short HEAD)"

step "门禁:格式 / clippy"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

step "门禁:依赖(共享库只有一份、宿主依赖边界、外壳区域)"
bash scripts/check-shared-deps.sh
bash scripts/check-host-deps.sh
bash scripts/check-host-deps.sh --self-test
bash scripts/check-shell-regions.sh

if [[ "$TESTS" -eq 1 ]]; then
  step "门禁:全量测试"
  cargo test --workspace
else
  echo "(跳过全量测试)"
fi

if [[ "$BUMP" -eq 1 ]]; then
  step "版本号 patch +1"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    bash scripts/bump-version.sh --dry-run
  else
    bash scripts/bump-version.sh
  fi
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo
  echo "(dry-run) 门禁通过;未改文件、未打包"
  exit 0
fi

VERSION=$(grep -m1 '^version = ' crates/desktop/Cargo.toml | sed -E 's/^version = "(.*)"$/\1/')

step "打包 Digger $VERSION(release)"
OUT=$(bash scripts/package-macos.sh release | tee /dev/stderr | sed -n 's/^Bundled: //p' | tail -n 1)
if [[ -z "$OUT" || ! -d "$OUT" ]]; then
  echo "error: 没有拿到 bundle 路径" >&2
  exit 1
fi

step "校验 bundle"
codesign --verify --deep --strict "$OUT"
for bin in digger diggerd digger-hook digger-mcp; do
  if [[ ! -x "$OUT/Contents/MacOS/$bin" ]]; then
    echo "error: bundle 缺少可执行文件 $bin" >&2
    exit 1
  fi
done
PLIST_VER=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$OUT/Contents/Info.plist")
if [[ "$PLIST_VER" != "$VERSION" ]]; then
  echo "error: bundle 版本 $PLIST_VER 与 Cargo.toml $VERSION 不一致" >&2
  exit 1
fi

step "压缩"
ZIP="$(dirname "$OUT")/Digger-$VERSION-macos-$(uname -m).zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$OUT" "$ZIP"
shasum -a 256 "$ZIP"

cat <<MSG

候选包: $ZIP
状态:   ad-hoc 签名,未公证(没有 Developer ID 与公证凭据时只是候选构建)
下一步: 审阅 git diff(版本号 / Cargo.lock),自行提交;本脚本不提交、不打 tag、不推送。
MSG
