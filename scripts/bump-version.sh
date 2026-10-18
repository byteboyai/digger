#!/usr/bin/env bash
#
# bump-version.sh — 发布打包前把 Digger 的版本号最后一位(patch)加 1。
#
# 版本真相源:crates/desktop/Cargo.toml 的 `version`(形如 0.1.0)。
# 同步更新:
#   - crates/desktop/Cargo.toml                       version
#   - Cargo.lock                                      digger-desktop 的 version(离线刷新)
#   - crates/desktop/packaging/macos/Info.plist       CFBundleShortVersionString
#     (对外展示的营销版本号,必须与 Cargo.toml 一致)
#   - crates/desktop/packaging/macos/Info.plist       CFBundleVersion
#     (Apple 的 build 号,纯整数,随发布同步 +1)
#
# 用法: scripts/bump-version.sh [--dry-run]
#   --dry-run  只打印 旧->新,不写文件(用于验证)
set -euo pipefail

DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help) echo "usage: $0 [--dry-run]" >&2; exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 1 ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_TOML="$ROOT_DIR/crates/desktop/Cargo.toml"
INFO_PLIST="$ROOT_DIR/crates/desktop/packaging/macos/Info.plist"

# 只取 [package] 下的 version(行首锚定,避开依赖里的 version)。
CURRENT=$(grep -m1 '^version = ' "$APP_TOML" | sed -E 's/^version = "(.*)"$/\1/')
if [[ -z "$CURRENT" ]]; then
  echo "error: 在 $APP_TOML 找不到 package version" >&2
  exit 1
fi
if [[ ! "$CURRENT" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: 版本格式不是 X.Y.Z: $CURRENT" >&2
  exit 1
fi

IFS='.' read -r -a PARTS <<< "$CURRENT"
# 10# 前缀避免前导零被当成八进制(如 PATCH=09)
NEW="${PARTS[0]}.${PARTS[1]}.$(( 10#${PARTS[2]} + 1 ))"

echo "version: $CURRENT -> $NEW"

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "(dry-run) 未写入任何文件"
  exit 0
fi

CURRENT_ESC=$(printf '%s' "$CURRENT" | sed 's/\./\\./g')
# 只改文件里第一处 version(即 [package] 的那一行)。
sed -i.bak -E "1,/^version = \"$CURRENT_ESC\"/s/^version = \"$CURRENT_ESC\"/version = \"$NEW\"/" "$APP_TOML"
rm -f "$APP_TOML.bak"

# Info.plist 的 <string> 行带前导空白,正则需容错。
sed -i.bak -E '/<key>CFBundleShortVersionString<\/key>/{n;s|^[[:space:]]*<string>[^<]*</string>|  <string>'"$NEW"'</string>|;}' "$INFO_PLIST"

if CURRENT_BUILD=$(sed -nE '/<key>CFBundleVersion<\/key>/{n;s|^[[:space:]]*<string>([0-9]+)</string>|\1|p;}' "$INFO_PLIST") && [[ "$CURRENT_BUILD" =~ ^[0-9]+$ ]]; then
  NEW_BUILD=$(( 10#$CURRENT_BUILD + 1 ))
  echo "CFBundleVersion: $CURRENT_BUILD -> $NEW_BUILD"
  sed -i.bak -E '/<key>CFBundleVersion<\/key>/{n;s|^[[:space:]]*<string>[0-9]+</string>|  <string>'"$NEW_BUILD"'</string>|;}' "$INFO_PLIST"
else
  echo "warn: 未找到纯整数的 CFBundleVersion,跳过 build 号更新" >&2
fi
rm -f "$INFO_PLIST.bak"

# 工作区 crate 的版本记在 Cargo.lock 里;离线刷新,避免之后第一次构建才悄悄改锁文件。
(cd "$ROOT_DIR" && cargo update --workspace --offline >/dev/null 2>&1) \
  || echo "warn: 未能刷新 Cargo.lock,请手动运行 cargo update --workspace --offline" >&2

echo "done."
