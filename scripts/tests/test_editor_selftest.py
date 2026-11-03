"""The GUI harness must run the executable from the current build."""
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "run-editor-selftest.sh"
class EditorHarnessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="editor-harness-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        shutil.copy2(SOURCE, self.root / "scripts/run-editor-selftest.sh")
        binary = self.root / "bin"
        binary.mkdir()
        fresh = self.root / "fresh-digger"
        fresh.write_text('#!/bin/sh\necho "SUMMARY current build passed"\n')
        fresh.chmod(0o755)
        stale = self.root / "target/debug/digger"
        stale.parent.mkdir(parents=True)
        stale.write_text('#!/bin/sh\necho "stale executable selected" >&2\nexit 42\n')
        stale.chmod(0o755)
        cargo = binary / "cargo"
        cargo.write_text('#!/usr/bin/env python3\nimport json,os,sys\n'
            'assert "--locked" in sys.argv and "--message-format=json" in sys.argv\n'
            'if os.environ.get("BUILD_FAIL"): sys.exit(101)\n'
            'if not os.environ.get("NO_ARTIFACT"):\n'
            ' print(json.dumps({"reason":"compiler-artifact","target":{"name":"digger","kind":["bin"]},"executable":os.environ["FRESH_EXE"]}))\n')
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=str(binary)+os.pathsep+os.environ["PATH"], FRESH_EXE=str(fresh))

    def run_harness(self):
        return subprocess.run(["bash", "scripts/run-editor-selftest.sh"], cwd=self.root,
            env=self.env, capture_output=True, text=True, timeout=10)

    def test_uses_only_the_executable_from_this_build(self):
        result = self.run_harness()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SUMMARY current build passed", result.stdout)
        self.assertNotIn("stale executable selected", result.stderr)

    def test_timeout_reaps_the_test_process(self):
        fresh = Path(self.env["FRESH_EXE"])
        marker = self.root / "pid"
        fresh.write_text(f'#!/bin/sh\necho $$ > {shlex.quote(str(marker))}\nexec /bin/sleep 30\n')
        self.env["SELFTEST_TIMEOUT"] = "1"
        result = self.run_harness()
        self.assertNotEqual(result.returncode, 0)
        pid = int(marker.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_missing_artifact_fails(self):
        self.env["NO_ARTIFACT"] = "1"
        self.assertNotEqual(self.run_harness().returncode, 0)

    def test_failed_build_cannot_run_a_cached_executable(self):
        self.env["BUILD_FAIL"] = "1"
        result = self.run_harness()
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("SUMMARY", result.stdout)

if __name__ == "__main__":
    unittest.main()
