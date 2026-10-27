#!/bin/sh
# Isolated synthetic launcher. Never uses installed applications or the real HOME.
set -eu
exec python3 - "$0" "$@" <<'PY'
import argparse
import os
from pathlib import Path
import shlex
import subprocess
import tempfile

parser = argparse.ArgumentParser(description='Seed a fresh isolated HOME/DIGGER_DATA_DIR under actual TMPDIR, then prepare or launch synthetic conversations.')
parser.add_argument('mode', choices=['prepare', 'gallery', 'digger', 'dozer'])
parser.add_argument('--state', default='list-and-detail', choices=['empty', 'list-only', 'list-and-detail-loading', 'list-and-detail', 'detail-error', 'detail-empty-entries', 'long-detail', 'import-prompt', 'daemon-unavailable', 'agent-filter-open'])
parser.add_argument('--dozer-bin', help='Explicit user-built Dozer binary; never an installed .app')
args = parser.parse_args(__import__('sys').argv[2:])
repo = Path(__import__('sys').argv[1]).resolve().parents[1]
# Honor the repository's native target location; no compile/install is performed.
target = repo / 'target/aarch64-apple-darwin/debug'
gallery = target / 'examples/conversations_gallery'
digger = target / 'digger'
if not os.environ.get('TMPDIR'):
    parser.error('actual TMPDIR is required')
tmp = Path(os.environ['TMPDIR']).resolve(strict=True)
if args.mode == 'dozer':
    if not args.dozer_bin:
        parser.error('dozer requires --dozer-bin pointing to your own built binary')
    binary = Path(args.dozer_bin).resolve(strict=True)
    if any(p.suffix == '.app' for p in binary.parents) or str(binary).startswith('/Applications/'):
        parser.error('installed application bundles are forbidden')
else:
    binary = gallery if args.mode == 'gallery' else digger
if args.mode != 'prepare' and (not binary.is_file() or not os.access(binary, os.X_OK)):
    parser.error(f'build the requested binary first: {binary}')
root = Path(tempfile.mkdtemp(prefix='s7c-conversations-', dir=tmp))
home, data = root / 'home', root / 'data'
env = os.environ.copy()
# Dozer currently uses HOME; discard any inherited product-root override too.
env.pop('DOZER_DATA_DIR', None)
env.update(HOME=str(home), DIGGER_DATA_DIR=str(data), PATH=str(target) + ':/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin')
project = subprocess.check_output([str(repo / 'scripts/seed-conversations-fixture.sh'), str(home)], env=env, text=True).strip()
data.mkdir()
print(f'Isolated root: {root}\nHOME: {home}\nDIGGER_DATA_DIR: {data}\nSynthetic project: {project}', flush=True)
prefix = ['env', '-u', 'DOZER_DATA_DIR', 'HOME=' + str(home), 'DIGGER_DATA_DIR=' + str(data), 'PATH=' + env['PATH']]
print('Gallery command: ' + shlex.join(prefix + [str(gallery), '--state', args.state]), flush=True)
print('Digger command: ' + shlex.join(prefix + [str(digger)]), flush=True)
print('Digger setup: open the printed project; show Conversations; explicitly click history import. No import has run.', flush=True)
if args.mode == 'prepare':
    print('Dozer: provide your own built binary with mode dozer --dozer-bin <path>; operate and compare it yourself.', flush=True)
else:
    command = [str(binary)] + (['--state', args.state] if args.mode == 'gallery' else [])
    os.execve(str(binary), command, env)
PY
