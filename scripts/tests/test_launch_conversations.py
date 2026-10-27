"""A parent's data-root override must never survive the isolated launcher."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'launch-conversations-fixture.sh'
GALLERY = SCRIPT.parents[1] / 'target/aarch64-apple-darwin/debug/examples/conversations_gallery'

class LaunchIsolation(unittest.TestCase):
    def test_unknown_state_fails_before_gui_or_temp_setup(self):
        result = subprocess.run([str(GALLERY), '--state', 'not-a-state'], env=dict(os.environ, TMPDIR='/nonexistent-synthetic-root'), capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('Unknown gallery state', result.stderr)

    def test_gui_rejects_outside_data_override_before_window_start(self):
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as home, tempfile.TemporaryDirectory(dir='/private/tmp') as outside:
            sentinel = Path(outside) / 'sentinel'
            sentinel.write_bytes(b'untouched')
            result = subprocess.run([str(GALLERY), '--state', 'empty'], env=dict(os.environ, HOME=home, DIGGER_DATA_DIR=outside), capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('DIGGER_DATA_DIR must be a strict descendant', result.stderr)
            self.assertEqual(sentinel.read_bytes(), b'untouched')
            self.assertEqual(sorted(p.name for p in Path(outside).iterdir()), ['sentinel'])

    def test_inherited_data_override_is_replaced_in_printed_and_exec_environment(self):
        with tempfile.TemporaryDirectory(dir='/private/tmp') as outside:
            outside = Path(outside)
            sentinel = outside / 'sentinel'
            sentinel.write_bytes(b'real-user-data-must-stay-untouched\x00')
            env = dict(os.environ, DIGGER_DATA_DIR=str(outside), DOZER_DATA_DIR=str(outside))
            result = subprocess.run([str(SCRIPT), 'prepare'], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            root = Path(next(line.removeprefix('Isolated root: ') for line in result.stdout.splitlines() if line.startswith('Isolated root: ')))
            self.assertTrue(root.is_relative_to(Path(os.environ['TMPDIR']).resolve()))
            command = shlex.split(next(line.removeprefix('Gallery command: ') for line in result.stdout.splitlines() if line.startswith('Gallery command: ')))
            prefix = command[:next(i for i, arg in enumerate(command) if arg.endswith('/examples/conversations_gallery'))]
            executed = subprocess.check_output(prefix + ['/usr/bin/env'], env=env, text=True)
            values = dict(line.split('=', 1) for line in executed.splitlines() if '=' in line)
            self.assertEqual(values['DIGGER_DATA_DIR'], str(root / 'data'))
            self.assertEqual(values['HOME'], str(root / 'home'))
            # Build the example before running this test: this queries the real
            # digger_paths resolver in a headless child, not a Python imitation.
            checked = subprocess.run(command + ['--check'], env=env, capture_output=True, text=True)
            self.assertEqual(checked.returncode, 0, checked.stderr)
            self.assertEqual(json.loads(checked.stdout)['config_dir'], str(root / 'data'))
            self.assertNotEqual(values.get('DOZER_DATA_DIR'), str(outside))
            self.assertEqual(sentinel.read_bytes(), b'real-user-data-must-stay-untouched\x00')
            self.assertEqual(sorted(p.name for p in outside.iterdir()), ['sentinel'])
            # Exercise the actual execve branch with an explicitly supplied tiny
            # user-built stand-in. This is not a GUI application.
            stand_in = root / 'user-built-dozer-probe'
            stand_in.write_text('#!/bin/sh\n/usr/bin/env\n')
            stand_in.chmod(0o700)
            launched = subprocess.run([str(SCRIPT), 'dozer', '--dozer-bin', str(stand_in)], env=env, capture_output=True, text=True)
            self.assertEqual(launched.returncode, 0, launched.stderr)
            actual = dict(line.split('=', 1) for line in launched.stdout.splitlines() if line.startswith(('HOME=', 'DIGGER_DATA_DIR=', 'DOZER_DATA_DIR=')))
            self.assertTrue(Path(actual['DIGGER_DATA_DIR']).is_relative_to(Path(os.environ['TMPDIR']).resolve()))
            self.assertNotEqual(actual.get('DOZER_DATA_DIR'), str(outside))
            self.assertEqual(sentinel.read_bytes(), b'real-user-data-must-stay-untouched\x00')

if __name__ == '__main__':
    unittest.main()
