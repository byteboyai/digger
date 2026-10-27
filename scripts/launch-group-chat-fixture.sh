#!/bin/sh
# Isolated group-chat acceptance launcher: a fresh HOME / DIGGER_DATA_DIR under the
# actual TMPDIR, a synthetic project, the deterministic fake agents on PATH. Never uses
# the real HOME, the installed Digger.app, or the user's resident diggerd.
#   launch-group-chat-fixture.sh prepare   print the isolated root and the commands
#   launch-group-chat-fixture.sh digger    exec the user-built target/…/debug/digger
set -eu
exec python3 - "$0" "$@" <<'PY'
import argparse
import os
from pathlib import Path
import shlex
import sys
import tempfile

parser = argparse.ArgumentParser(description='Isolated group-chat acceptance environment with fake agents.')
parser.add_argument('mode', choices=['prepare', 'digger'])
args = parser.parse_args(sys.argv[2:])
repo = Path(sys.argv[1]).resolve().parents[1]
target = repo / 'target/aarch64-apple-darwin/debug'
digger = target / 'digger'
fakes = repo / 'crates/desktop/tests/fixtures/fake-agents'
if not os.environ.get('TMPDIR'):
    parser.error('actual TMPDIR is required')
tmp = Path(os.environ['TMPDIR']).resolve(strict=True)
if args.mode == 'digger' and (not digger.is_file() or not os.access(digger, os.X_OK)):
    parser.error(f'build the binary first: {digger}')
root = Path(tempfile.mkdtemp(prefix='s8c-group-chat-', dir=tmp))
home, data, project = root / 'home', root / 'data', root / 'home' / 'synthetic-group-chat-project'
for d in (home, data, project):
    d.mkdir(parents=True, exist_ok=True)
(project / 'README.md').write_text('# Synthetic project\n\nOnly for the S8c group-chat acceptance run.\n')
log = root / 'fake-agents.log'
env = {
    'HOME': str(home),
    'DIGGER_DATA_DIR': str(data),
    'FAKE_AGENT_LOG': str(log),
    'PATH': f'{fakes}:{target}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin',
    'TMPDIR': str(tmp),
}
# Keep the rest of the environment (GUI needs it) but never an inherited product root.
prefix = ['env', '-u', 'DOZER_DATA_DIR'] + [f'{k}={v}' for k, v in env.items()]
print(f'Isolated root: {root}\nSynthetic project: {project}\nFake agent log: {log}', flush=True)
print('Digger command: ' + shlex.join(prefix + [str(digger)]), flush=True)
print(f'After the run: scripts/check-fake-agent-log.sh {log}', flush=True)
if args.mode == 'digger':
    os.execv('/usr/bin/env', prefix + [str(digger)])
PY
