# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the board mover. Run with `just agents-check`. `gh` is a stub; nothing calls GitHub."""

import json
import os
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import board_move  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
SCRIPT = os.path.join(ROOT, ".claude", "scripts", "board-move.sh")

GH_STUB = r'''#!/usr/bin/env python3
import json, os, sys
stub = os.environ["STUB_DIR"]
with open(os.path.join(stub, "calls.log"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
query = next(a for a in sys.argv if a.startswith("query="))
if "projectsV2" in query:
    print(json.dumps({"data": {"repository": {
        "projectsV2": {"nodes": [{"id": "P1", "title": "Stepclimb", "number": 3}]},
        "issue": {"id": "I7", "projectItems": {"nodes": [{"id": "IT7", "project": {"id": "P1"}}]}}}}}))
elif "fields(" in query:
    print(json.dumps({"data": {"node": {"fields": {"nodes": [
        {}, {"id": "F1", "name": "Status", "options": [
            {"id": "o1", "name": "Todo"}, {"id": "o2", "name": "In progress"},
            {"id": "o3", "name": "In review"}, {"id": "o4", "name": "Done"}]}]}}}}))
elif "updateProjectV2ItemFieldValue" in query:
    print(json.dumps({"data": {"updateProjectV2ItemFieldValue": {"projectV2Item": {"id": "IT7"}}}}))
else:
    sys.exit("gh stub: unexpected query")
'''


class Pure(unittest.TestCase):
    FIELDS = [{}, {"id": "F1", "name": "Status", "options": [{"id": "o2", "name": "In progress"}]}]

    def test_matches_a_status_option_without_regard_to_case(self):
        self.assertEqual(board_move.status_option(self.FIELDS, "in PROGRESS"), ("F1", "o2"))

    def test_names_the_options_when_the_status_is_unknown(self):
        with self.assertRaisesRegex(board_move.BoardError, "has: In progress"):
            board_move.status_option(self.FIELDS, "Blocked")

    def test_refuses_a_project_without_a_status_field(self):
        with self.assertRaisesRegex(board_move.BoardError, "no single-select field named Status"):
            board_move.status_option([{}], "Done")

    def test_reads_the_origin_url(self):
        self.assertEqual(board_move.repo_slug("git@github.com:Vainnor/stepclimb.git"),
                         ("Vainnor", "stepclimb"))
        with self.assertRaises(board_move.BoardError):
            board_move.repo_slug("/srv/git/x.git")


class Script(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.stub = os.path.join(self.tmp.name, "stub")
        self.repo = os.path.join(self.tmp.name, "repo")
        os.mkdir(self.stub)
        with open(os.path.join(self.stub, "gh"), "w") as f:
            f.write(GH_STUB)
        os.chmod(os.path.join(self.stub, "gh"), 0o755)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env.update(STUB_DIR=self.stub, PATH=self.stub + os.pathsep + os.environ["PATH"],
                        GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        subprocess.run(["git", "init", "-q", self.repo], check=True, env=self.env)
        subprocess.run(["git", "-C", self.repo, "remote", "add", "origin",
                        "https://github.com/Vainnor/stepclimb.git"], check=True, env=self.env)

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, *args):
        return subprocess.run(["bash", SCRIPT, *args], cwd=self.repo, capture_output=True,
                              text=True, env=self.env, check=False)

    def test_sets_the_status_field_of_the_issues_item(self):
        out = self.run_script("7", "in review")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("#7 -> in review on project 3 (Stepclimb)", out.stdout)
        with open(os.path.join(self.stub, "calls.log")) as f:
            calls = [json.loads(line) for line in f]
        last = calls[-1]
        self.assertIn("updateProjectV2ItemFieldValue", " ".join(last))
        for expected in ("project=P1", "item=IT7", "field=F1", "option=o3"):
            self.assertIn(expected, last)

    def test_refuses_bad_arguments(self):
        for args in ((), ("7",), ("seven", "Done"), ("7", "Done", "x")):
            self.assertEqual(self.run_script(*args).returncode, 2, args)

    def test_reports_an_unknown_status(self):
        out = self.run_script("7", "Blocked")
        self.assertEqual(out.returncode, 1)
        self.assertIn("no option 'Blocked'", out.stderr)


if __name__ == "__main__":
    unittest.main()
