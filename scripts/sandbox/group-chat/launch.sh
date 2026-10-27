#!/usr/bin/env bash
# S8b user-acceptance sandbox: isolated HOME, synthetic gallery, no daemon.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SANDBOX="${S8B_SANDBOX:-$(mktemp -d "${TMPDIR:-/tmp}/digger-s8b.XXXXXX")}"
mkdir -p "$SANDBOX/home" "$SANDBOX/tmp"
export HOME="$SANDBOX/home"
export TMPDIR="$SANDBOX/tmp"
export XDG_DATA_HOME="$SANDBOX/home/.local/share"
export XDG_CONFIG_HOME="$SANDBOX/home/.config"
cd "$ROOT"
case "${1:-run}" in
  check)
    cargo run -q -p digger-desktop --example group_chat_gallery -- --check
    ;;
  run)
    cargo run -q -p digger-desktop --example group_chat_gallery
    ;;
  *)
    echo "usage: $0 [run|check]" >&2
    exit 2
    ;;
esac
