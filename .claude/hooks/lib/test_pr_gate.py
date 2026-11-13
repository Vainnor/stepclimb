# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the pre-PR gate and the stamp `just ci` writes. Run with `just agents-check`."""

import json
import os
import subprocess
import tempfile
import unittest

LIB = os.path.dirname(os.path.abspath(__file__))
HOOKS = os.path.dirname(LIB)
ROOT = os.path.dirname(os.path.dirname(HOOKS))
STAMP_SCRIPT = os.path.join(ROOT, "bin", "ci-stamp.sh")


class PrGate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = self.tmp.name
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com",
                        GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com")
        self.git("init", "-q", "-b", "feat/1/x")
        self.commit("a")

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, *args):
        subprocess.run(["git", "-C", self.repo, *args], check=True, capture_output=True,
                       env=self.env)

    def commit(self, name):
        with open(os.path.join(self.repo, name), "w") as f:
            f.write(name)
        self.git("add", name)
        self.git("commit", "-q", "-m", name)

    def stamp(self, *args):
        return subprocess.run(["bash", STAMP_SCRIPT, *args], cwd=self.repo, capture_output=True,
                              text=True, env=self.env, check=True).stdout

    def head(self):
        return subprocess.run(["git", "-C", self.repo, "rev-parse", "HEAD"], capture_output=True,
                              text=True, env=self.env, check=True).stdout.strip()

    def hook(self, command):
        payload = {"cwd": self.repo, "tool_name": "Bash", "tool_input": {"command": command}}
        out = subprocess.run(["bash", os.path.join(HOOKS, "pr-gate.sh")], input=json.dumps(payload),
                             capture_output=True, text=True, env=self.env, check=False)
        return out.returncode, out.stderr

    def test_blocks_a_pr_with_no_recorded_pass(self):
        code, err = self.hook("gh pr create --base next --fill")
        self.assertEqual(code, 2)
        self.assertIn("no `just ci` pass is recorded", err)

    def test_allows_a_pr_from_the_commit_that_passed(self):
        self.assertIn("recorded", self.stamp())
        for command in ("gh pr create --base next", "git push -u origin HEAD && gh pr create -B next",
                        "gh -R Vainnor/stepclimb pr create", "gh --repo=Vainnor/stepclimb pr create"):
            code, err = self.hook(command)
            self.assertEqual(code, 0, f"{command}: {err}")

    def test_blocks_after_a_new_commit(self):
        self.stamp()
        self.commit("b")
        code, err = self.hook("gh pr create")
        self.assertEqual(code, 2)
        self.assertIn("not HEAD", err)

    def test_blocks_with_uncommitted_changes(self):
        self.stamp()
        with open(os.path.join(self.repo, "a"), "w") as f:
            f.write("changed")
        code, err = self.hook("gh pr create")
        self.assertEqual(code, 2)
        self.assertIn("working tree has changes", err)

    def test_records_the_commit_the_run_started_on(self):
        tested = self.head()
        self.assertIn("recorded", self.stamp(tested))
        code, err = self.hook("gh pr create")
        self.assertEqual(code, 0, err)

    def test_a_commit_during_the_run_records_no_pass(self):
        tested = self.head()
        self.commit("b")
        self.assertIn("not recorded", self.stamp(tested))
        code, _ = self.hook("gh pr create")
        self.assertEqual(code, 2)

    def test_a_run_that_started_dirty_records_no_pass(self):
        tested = self.head()
        self.assertIn("not recorded", self.stamp(tested, "dirty"))
        code, _ = self.hook("gh pr create")
        self.assertEqual(code, 2)
        self.assertIn("recorded", self.stamp(tested, "clean"))

    def test_a_dirty_tree_records_no_pass(self):
        with open(os.path.join(self.repo, "new"), "w") as f:
            f.write("x")
        self.assertIn("not recorded", self.stamp())
        os.remove(os.path.join(self.repo, "new"))
        code, _ = self.hook("gh pr create")
        self.assertEqual(code, 2)

    def test_other_commands_pass(self):
        for command in ("gh pr view 3", "gh pr list", "echo gh pr creates", "git status"):
            code, err = self.hook(command)
            self.assertEqual(code, 0, f"{command}: {err}")

    def test_blocks_input_it_cannot_check(self):
        out = subprocess.run(["bash", os.path.join(HOOKS, "pr-gate.sh")], input="not json",
                             capture_output=True, text=True, check=False)
        self.assertEqual(out.returncode, 2)

    def test_the_ci_recipe_ends_with_the_stamp(self):
        with open(os.path.join(ROOT, "justfile"), encoding="utf-8") as f:
            recipe = f.read().split("\nci:", 1)[1].split("\n\n", 1)[0]
        self.assertIn('tested="$(git rev-parse HEAD)"', recipe)
        self.assertIn("git status --porcelain --untracked-files=normal", recipe)
        self.assertTrue(recipe.rstrip().endswith('bash bin/ci-stamp.sh "$tested" "$tree"'), recipe)

    def test_claude_and_codex_run_the_gate_on_bash(self):
        with open(os.path.join(ROOT, ".claude", "settings.json"), encoding="utf-8") as f:
            entries = json.load(f)["hooks"]["PreToolUse"]
        bash = [h["command"] for e in entries if e["matcher"] == "Bash" for h in e["hooks"]]
        self.assertTrue(any("pr-gate.sh" in c for c in bash))
        with open(os.path.join(ROOT, ".codex", "config.toml"), encoding="utf-8") as f:
            self.assertIn("pr-gate.sh", f.read())


if __name__ == "__main__":
    unittest.main()
