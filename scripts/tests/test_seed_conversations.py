"""Safety regression: rejection must leave every existing byte untouched."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'seed-conversations-fixture.sh'

class SeedSafety(unittest.TestCase):
    def run_seed(self, *args):
        return subprocess.run([str(SCRIPT), *map(str, args)], capture_output=True, text=True)

    def test_missing_argument(self):
        self.assertNotEqual(self.run_seed().returncode, 0)

    def test_rejections_do_not_write(self):
        tmp = Path(os.environ['TMPDIR']).resolve()
        with tempfile.TemporaryDirectory(dir=tmp) as safe, tempfile.TemporaryDirectory(dir='/private/tmp') as outside:
            safe, outside = Path(safe), Path(outside)
            sentinel = outside / 'sentinel'
            sentinel.write_bytes(b'untouched\x00fixture')
            (safe / 'escape').symlink_to(outside, target_is_directory=True)
            (safe / 'inside').mkdir()
            (safe / 'inside-link').symlink_to(safe / 'inside', target_is_directory=True)
            (safe / 'home').mkdir()
            (safe / 'home' / '.claude').symlink_to(outside, target_is_directory=True)
            for target in [tmp, Path.home(), outside / 'home', safe / 'escape' / 'home', safe / 'inside-link' / 'home', safe / 'home', Path(str(tmp) + '-lookalike') / 'home']:
                with self.subTest(target=target):
                    before = sorted(str(p) for p in outside.rglob('*'))
                    self.assertNotEqual(self.run_seed(target).returncode, 0)
                    self.assertEqual(sentinel.read_bytes(), b'untouched\x00fixture')
                    self.assertEqual(before, sorted(str(p) for p in outside.rglob('*')))
            self.assertFalse((outside / 'home').exists())
            self.assertFalse((safe / 'inside' / 'home').exists())

    def test_valid_seed_rewrites_only_valid_json(self):
        with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as root:
            home = (Path(root) / 'home').resolve()
            result = self.run_seed(home)
            self.assertEqual(result.returncode, 0, result.stderr)
            project = Path(result.stdout.strip())
            self.assertTrue(project.is_relative_to(home))
            key = ''.join(c if c.isascii() and c.isalnum() else '-' for c in str(project))
            files = list((home / '.claude/projects' / key).glob('*.jsonl'))
            self.assertEqual(len(files), 7)
            malformed = 0
            for fixture in files:
                for line in fixture.read_text().splitlines():
                    try:
                        record = json.loads(line)
                    except json.JSONDecodeError:
                        malformed += 1
                        continue
                    self.assertEqual(record['cwd'], str(project))
            self.assertEqual(malformed, 2)

if __name__ == '__main__':
    unittest.main()
