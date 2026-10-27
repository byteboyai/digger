"""The fakes never execute prompts, honour the directives, and the log checker is strict."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
FAKES = ROOT / 'crates/desktop/tests/fixtures/fake-agents'
CHECK = ROOT / 'scripts/check-fake-agent-log.sh'
WRAP = ROOT / 'scripts/with-fake-agents.sh'
MAX_REPLY_CHARS = 20_000  # dozerd::group_adapter::MAX_REPLY_CHARS


def run(name, args, prompt=None, log=None, cwd=None):
    env = dict(os.environ, FAKE_AGENT_LOG=str(log))
    return subprocess.run([str(FAKES / name), *args], input=prompt, env=env,
                          capture_output=True, text=True, cwd=cwd, timeout=30)


class FakeAgents(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.log = Path(self.dir.name) / 'log'

    def tearDown(self):
        self.dir.cleanup()

    def test_default_reply_is_deterministic_and_never_executes_the_prompt(self):
        marker = Path(self.dir.name) / 'pwned'
        prompt = f'hi $(touch {marker}) `touch {marker}`; touch {marker}'
        a = run('claude', ['-p', 'x'], prompt, self.log)
        b = run('claude', ['-p', 'x'], prompt, self.log)
        self.assertEqual(a.returncode, 0)
        self.assertEqual(a.stdout, b.stdout)
        self.assertIn('确定性回复', a.stdout)
        self.assertFalse(marker.exists())

    def test_codex_takes_the_prompt_from_the_last_argument_and_writes_the_last_message_file(self):
        out = Path(self.dir.name) / 'last'
        r = run('codex', ['exec', '--sandbox', 'read-only', '--output-last-message', str(out), 'FAKE:long go'], log=self.log)
        self.assertEqual(r.returncode, 0)
        self.assertEqual(out.read_text().strip(), r.stdout.strip())
        self.assertIn('第 449 行', r.stdout)

    def test_replies_stay_within_the_reply_cap_so_nothing_is_truncated(self):
        for directive in ('FAKE:long', 'FAKE:hostile', ''):
            r = run('claude', ['-p', 'x'], directive, self.log)
            self.assertLess(len(r.stdout.strip()), MAX_REPLY_CHARS, directive)

    def test_fail_exits_nonzero_with_the_fixed_error(self):
        r = run('claude', ['-p', 'x'], 'please FAKE:fail', self.log)
        self.assertEqual(r.returncode, 1)
        self.assertIn('fake agent failure', r.stderr)
        self.assertEqual(r.stdout, '')

    def test_hostile_reply_contains_the_untrusted_shapes_only_as_text(self):
        r = run('claude', ['-p', 'x'], 'FAKE:hostile', self.log)
        for needle in ('<script>', 'javascript:', 'https://example.invalid/pixel.png', '```js'):
            self.assertIn(needle, r.stdout)
        self.assertIn('a' * 10_000, r.stdout)

    def test_unknown_name_is_refused(self):
        link = Path(self.dir.name) / 'goose'
        link.symlink_to(FAKES / 'fake-agent.sh')
        r = subprocess.run([str(link)], input='x', capture_output=True, text=True, timeout=30)
        self.assertEqual(r.returncode, 64)

    def test_wrapper_puts_the_fakes_first_on_path(self):
        r = subprocess.run([str(WRAP), 'sh', '-c', 'command -v claude codex'], capture_output=True, text=True, timeout=30)
        self.assertEqual(r.returncode, 0)
        self.assertTrue(all(line.startswith(str(FAKES.resolve())) or line.startswith(str(FAKES)) or '/fake-agents/' in line for line in r.stdout.split()))
        self.assertEqual(subprocess.run([str(WRAP)], capture_output=True).returncode, 2)


class LogChecker(unittest.TestCase):
    def check(self, text):
        with tempfile.NamedTemporaryFile('w', suffix='.log', delete=False) as f:
            f.write(text)
        try:
            return subprocess.run([str(CHECK), f.name], capture_output=True, text=True, timeout=30)
        finally:
            os.unlink(f.name)

    GOOD_CLAUDE = 'FAKE_AGENT claude cwd=/p arg=[-p] arg=[x] arg=[--no-session-persistence] arg=[--allowedTools] arg=[Read,Grep,Glob] arg=[--disallowedTools] arg=[Bash,Edit,Write,NotebookEdit,WebFetch,WebSearch]\n'
    GOOD_CODEX = 'FAKE_AGENT codex cwd=/p arg=[exec] arg=[--ephemeral] arg=[--sandbox] arg=[read-only] arg=[--skip-git-repo-check] arg=[prompt]\n'

    def test_accepts_real_read_only_command_lines(self):
        r = self.check(self.GOOD_CLAUDE + self.GOOD_CODEX)
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertIn('2 recorded turn(s)', r.stdout)

    def test_rejects_permission_skipping_and_missing_read_only_flags(self):
        for text in (
            self.GOOD_CLAUDE.replace('arg=[x]', 'arg=[--dangerously-skip-permissions]'),
            self.GOOD_CODEX.replace('arg=[exec]', 'arg=[-y]'),
            self.GOOD_CLAUDE.replace('arg=[--allowedTools] arg=[Read,Grep,Glob] ', ''),
            self.GOOD_CLAUDE.replace('Bash,Edit,Write,', 'Edit,'),
            self.GOOD_CODEX.replace('arg=[read-only]', 'arg=[workspace-write]'),
            self.GOOD_CODEX.replace('arg=[--sandbox] arg=[read-only] ', ''),
            'FAKE_AGENT goose cwd=/p\n',
        ):
            self.assertEqual(self.check(text).returncode, 1, text)

    def test_an_empty_or_missing_log_is_not_a_pass(self):
        self.assertEqual(self.check('').returncode, 2)
        self.assertEqual(subprocess.run([str(CHECK), '/nonexistent/log'], capture_output=True).returncode, 2)


if __name__ == '__main__':
    unittest.main()
