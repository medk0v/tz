"""Exercise the actual workflow CI gate with isolated GitHub API responses."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/deploy-lite.yml"
SHA = "a" * 40


def gate_script():
    lines = WORKFLOW.read_text().splitlines()
    step = lines.index("      - name: Require successful CI for this exact master commit")
    start = lines.index("        run: |", step) + 1
    end = start
    while end < len(lines) and (not lines[end].strip() or lines[end].startswith("          ")):
        end += 1
    return textwrap.dedent("\n".join(lines[start:end]))


def ci_run(**overrides):
    return dict({
        "head_sha": SHA, "head_branch": "master", "event": "push",
        "status": "completed", "conclusion": "success",
        "html_url": "https://github.com/example/repo/actions/runs/123",
    }, **overrides)


class LiteCiGateTests(unittest.TestCase):
    def execute(self, runs, api_error=False):
        with tempfile.TemporaryDirectory() as directory:
            gh = Path(directory) / "gh"
            gh.write_text("#!/bin/sh\n"
                          'if [ "$API_ERROR" = 1 ]; then echo "HTTP 403" >&2; exit 1; fi\n'
                          'printf "%s\\n" "$API_RUNS"\n')
            gh.chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ["PATH"],
                       RELEASE_SHA=SHA, GH_REPO="example/repo",
                       API_RUNS=json.dumps(runs), API_ERROR="1" if api_error else "0")
            return subprocess.run(["bash", "-e", "-o", "pipefail", "-c", gate_script()],
                                  env=env, capture_output=True, text=True, timeout=10)

    def test_allows_completed_success_for_the_exact_master_push(self):
        result = self.execute([ci_run()])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Lite build is allowed", result.stdout)

    def test_running_ci_reports_its_url_and_retry_instruction(self):
        for status in ("queued", "in_progress", "waiting"):
            with self.subTest(status=status):
                result = self.execute([ci_run(status=status, conclusion=None)])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("still queued or running", result.stdout)
                self.assertIn("Re-run failed jobs", result.stdout)
                self.assertIn("/actions/runs/123", result.stdout)

    def test_terminal_unsuccessful_ci_is_blocked(self):
        for conclusion in ("failure", "cancelled", "skipped", "timed_out", "neutral"):
            with self.subTest(conclusion=conclusion):
                result = self.execute([ci_run(conclusion=conclusion)])
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("CI did not succeed", result.stdout)

    def test_missing_ci_is_reported(self):
        result = self.execute([])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No push CI run was found", result.stdout)

    def test_other_commit_branch_or_event_cannot_authorize_deployment(self):
        result = self.execute([ci_run(head_sha="b" * 40), ci_run(head_branch="feature"),
                               ci_run(event="pull_request")])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No push CI run was found", result.stdout)

    def test_api_failure_is_distinct_from_ci_failure(self):
        result = self.execute([], api_error=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Could not read CI runs from GitHub", result.stdout)
        self.assertIn("HTTP 403", result.stderr)
        self.assertNotIn("No push CI run was found", result.stdout)


if __name__ == "__main__":
    unittest.main()
