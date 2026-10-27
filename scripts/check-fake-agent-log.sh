#!/bin/sh
# Verify the command lines the fake agents recorded: every Claude turn must carry the
# read-only tool lists, every Codex turn `--sandbox read-only`, and no turn may carry
# --dangerously-skip-permissions or -y. Exit 0 = ok, 1 = a violation, 2 = nothing to check.
set -eu
log="${1:-${FAKE_AGENT_LOG:-${TMPDIR:-/tmp}/digger-fake-agents.log}}"
if [ ! -s "$log" ]; then
  echo "no recorded turns in $log" >&2
  exit 2
fi
exec python3 - "$log" <<'PY'
import re
import sys

bad = []
turns = 0
for number, line in enumerate(open(sys.argv[1], encoding='utf-8', errors='replace'), 1):
    if not line.startswith('FAKE_AGENT '):
        continue
    turns += 1
    name = line.split()[1]
    args = re.findall(r'arg=\[(.*?)\]', line)
    if any('dangerously' in a or a == '-y' for a in args):
        bad.append(f'line {number}: forbidden permission-skipping flag: {line.strip()}')
    if name == 'claude':
        if '--allowedTools' not in args or '--disallowedTools' not in args:
            bad.append(f'line {number}: claude without the read-only tool lists')
        elif 'Bash' not in args[args.index('--disallowedTools') + 1] or 'Write' not in args[args.index('--disallowedTools') + 1]:
            bad.append(f'line {number}: claude does not disallow Bash/Write')
    elif name == 'codex':
        if not any(a == '--sandbox' for a in args) or args[args.index('--sandbox') + 1] != 'read-only':
            bad.append(f'line {number}: codex without --sandbox read-only')
    else:
        bad.append(f'line {number}: unexpected agent {name}')
if bad:
    print('\n'.join(bad))
    sys.exit(1)
print(f'{turns} recorded turn(s), all read-only')
PY
