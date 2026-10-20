#!/usr/bin/env bash
# Builds crates/desktop (bin: digger) and wraps it into a Digger.app bundle
# using crates/desktop/packaging/macos/Info.plist.
#
# Digger is a single-binary app (unlike Dozer, which also ships dozerd /
# dozer-hook / dozer-mcp siblings). Shared Usage assets are embedded at compile
# time from the pinned dozer-panel-usage crate; runtime needs no source checkout.
#
# This produces an UNSIGNED, ad-hoc-signed candidate bundle. Signing /
# notarization need a real Developer ID identity and credentials; without them the
# result must be described as a candidate build, not a distributed release.
#
# Usage: scripts/package-macos.sh [debug|release]   (default: release)
set -euo pipefail

PROFILE="${1:-release}"
case "$PROFILE" in
  debug) CARGO_PROFILE_FLAG=() ;;
  release) CARGO_PROFILE_FLAG=(--release) ;;
  *) echo "usage: $0 [debug|release]" >&2; exit 1 ;;
esac

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGING_DIR="$ROOT_DIR/crates/desktop/packaging/macos"
APP_NAME="Digger"
BIN_NAME="digger"

BUILD_JSON=$(
  cargo build "${CARGO_PROFILE_FLAG[@]}" -p digger-desktop --bin "$BIN_NAME" --message-format=json
)

BIN_PATH=$(
  echo "$BUILD_JSON" | jq -r --arg bin "$BIN_NAME" \
    'select(.reason=="compiler-artifact" and .target.name==$bin and .executable != null) | .executable' \
    | tail -n 1
)
if [ -z "$BIN_PATH" ] || [ ! -x "$BIN_PATH" ]; then
  echo "error: could not locate built '$BIN_NAME' executable" >&2
  exit 1
fi

OUT_DIR="$(dirname "$(dirname "$BIN_PATH")")/bundle/macos"
APP_DIR="$OUT_DIR/$APP_NAME.app"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$BIN_PATH" "$APP_DIR/Contents/MacOS/$BIN_NAME"
cp "$PACKAGING_DIR/Info.plist" "$APP_DIR/Contents/Info.plist"
if [ -f "$PACKAGING_DIR/AppIcon.icns" ]; then
  cp "$PACKAGING_DIR/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
fi

# 预览宿主(dozer-panel-preview)的 web 资源以目录方式交付,不编译期内嵌:
# flyfish 21 MB,内嵌会让二进制涨一大截,且 Dozer 现在就是资源目录。
# 只拷第一批用到的三个目录——json-editor / tabular-host / plantuml-viewer
# 属于能力关闭的类型,宿主根本不会创建(见 P3 计划 D3)。
ASSETS_DIR=$(
  cargo metadata --format-version 1 \
    | jq -r '.packages[] | select(.name=="dozer-panel-preview") | .manifest_path' \
    | head -n 1 | xargs dirname
)/assets
if [ ! -d "$ASSETS_DIR/flyfish" ]; then
  echo "error: preview assets not found at '$ASSETS_DIR'" >&2
  exit 1
fi
for dir in editor flyfish image-annotate; do
  rm -rf "$APP_DIR/Contents/Resources/$dir"
  cp -R "$ASSETS_DIR/$dir" "$APP_DIR/Contents/Resources/$dir"
done
# docx 渲染器第一批用不到(能力关闭),省若干 MB。
# PDF 已开启(N5):`flyfish/vendor/pdf` 必须留在包里。
rm -rf "$APP_DIR/Contents/Resources/flyfish/vendor/docx"

# 宿主按名字读的目录与入口(与 crates/desktop/src/preview_assets.rs 的名单一致):
# dozer:// 应答把 404 静默落成白屏,所以打包时就点名报错。
for dir in editor flyfish image-annotate; do
  if [ ! -d "$APP_DIR/Contents/Resources/$dir" ]; then
    echo "error: preview resource '$dir/' missing from the bundle" >&2
    exit 1
  fi
done
if [ ! -f "$APP_DIR/Contents/Resources/flyfish/host.html" ]; then
  echo "error: preview resource 'flyfish/host.html' missing from the bundle" >&2
  exit 1
fi

# cargo 的链接期只对裸二进制做了 ad-hoc 签名,装进 bundle 后这个签名不覆盖
# Info.plist/资源,且内容一变 ad-hoc identifier 就变;整体重签让本次构建自洽
# (与 Dozer build-macos-app.sh 同因,见该脚本注释)。
codesign --force --deep --sign - "$APP_DIR"

touch "$APP_DIR"

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -f "$APP_DIR"
fi

echo "Bundled: $APP_DIR"
