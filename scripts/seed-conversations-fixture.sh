#!/bin/sh
# Synthetic fixture installation only. No daemon, import, or model calls.
set -eu
if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo 'usage: seed-conversations-fixture.sh <isolated-HOME-under-TMPDIR>' >&2
  exit 2
fi
exec python3 - "$1" "$(CDPATH= cd -- "$(dirname -- "$0")/../crates/desktop/tests/fixtures/conversations" && pwd)" <<'PY'
import json
import os
from pathlib import Path
import pwd
import sys


def refuse(reason):
    sys.exit('refusing fixture seed: ' + reason)

if not os.environ.get('TMPDIR'):
    refuse('TMPDIR must identify the actual temporary root')
root = Path(os.environ['TMPDIR']).resolve(strict=True)
raw = Path(sys.argv[1]).absolute()
home = raw.resolve()
if home == root or not home.is_relative_to(root):
    refuse('HOME must be a strict descendant of actual TMPDIR')
real_home = Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()
if home == real_home or home.is_relative_to(real_home):
    refuse('real user HOME is forbidden')
# Check the lexical path below the system temp alias, and every existing child.
# Refuse before mkdir/write, including .claude/project/file symlink escapes.
for parent in [raw, *raw.parents]:
    if parent == root or parent.resolve() == root:
        break
    if parent.is_symlink():
        refuse('symlink in HOME path')
if home.exists():
    if not home.is_dir():
        refuse('HOME is not a directory')
    if any(p.is_symlink() for p in home.rglob('*')):
        refuse('symlink inside isolated HOME')
project = home / 'synthetic-conversations-project'
# Exact core::agent_paths mapping: ASCII alphanumeric unchanged, ALL else '-'.
key = ''.join(c if c.isascii() and c.isalnum() else '-' for c in str(project))
destination = home / '.claude' / 'projects' / key
sources = sorted(Path(sys.argv[2]).glob('*.jsonl'))
if len(sources) != 7:
    refuse('expected seven tracked synthetic fixtures')
# Prepare all payloads before touching the destination.
payloads = []
for source in sources:
    lines = []
    for line in source.read_text().splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            lines.append(line)  # Intentionally broken lines survive byte-for-byte.
        else:
            record['cwd'] = str(project)
            lines.append(json.dumps(record, ensure_ascii=False, separators=(',', ':')))
    payloads.append((source.name, '\n'.join(lines) + '\n'))
project.mkdir(parents=True, exist_ok=True)
destination.mkdir(parents=True, exist_ok=True)
for name, payload in payloads:
    (destination / name).write_text(payload)
print(project)
PY
