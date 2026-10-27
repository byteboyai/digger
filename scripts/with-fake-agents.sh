#!/bin/sh
# Run a command with the deterministic fake `claude` / `codex` first on PATH, so a
# group-chat turn never reaches a real model. Usage: with-fake-agents.sh <cmd> [args…]
# The fakes append every command line they receive to $FAKE_AGENT_LOG
# (default: $TMPDIR/digger-fake-agents.log); check it with check-fake-agent-log.sh.
set -eu
if [ "$#" -eq 0 ]; then
  echo "usage: $0 <command> [args…]" >&2
  exit 2
fi
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
fakes="$here/../crates/desktop/tests/fixtures/fake-agents"
PATH="$fakes:$PATH"
export PATH
exec "$@"
