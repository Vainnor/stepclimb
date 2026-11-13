# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the /project-status collector's pure parts and its offline run. Run with
`just agents-check`. Nothing here calls GitHub."""

import os
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import project_status as ps  # noqa: E402

SCRIPT = os.path.join(ps.ROOT, ".claude", "scripts", "project-status.sh")


def milestone(title, closed, opened):
    return {"title": title, "closed_issues": closed, "open_issues": opened}


class RepoSlug(unittest.TestCase):
    def test_reads_https_and_ssh_remotes(self):
        for url in ("https://github.com/Vainnor/stepclimb.git", "https://github.com/Vainnor/stepclimb",
                    "git@github.com:Vainnor/stepclimb.git", "ssh://git@github.com/Vainnor/stepclimb.git\n"):
            self.assertEqual(ps.repo_slug(url), "Vainnor/stepclimb", url)

    def test_refuses_other_hosts(self):
        for url in ("https://gitlab.com/a/b.git", "/srv/git/b.git", ""):
            self.assertIsNone(ps.repo_slug(url), url)


class Milestones(unittest.TestCase):
    def test_reads_the_plan_id_at_the_start_of_a_title(self):
        self.assertEqual(ps.milestone_id("M0 Foundations"), "M0")
        self.assertEqual(ps.milestone_id("1.0 Release"), "1.0")
        self.assertIsNone(ps.milestone_id("Backlog"))
        self.assertIsNone(ps.milestone_id("M10 later"))

    def test_orders_rows_as_the_plan_does_and_lists_others_after(self):
        rows = ps.milestone_rows([
            milestone("M6 Managed launch", 0, 9), milestone("Backlog", 1, 2),
            milestone("M0 Foundations", 3, 12), milestone("1.0", 0, 4), milestone("M1 Learning", 0, 20),
        ])
        self.assertEqual([r[0] for r in rows], ["M0", "M1", "1.0", "M6", "-"])
        self.assertEqual(rows[0], ("M0", "M0 Foundations", 3, 12))

    def test_a_second_milestone_with_the_same_id_is_listed_not_merged(self):
        rows = ps.milestone_rows([milestone("M0 Foundations", 1, 1), milestone("M0 duplicate", 2, 2)])
        self.assertEqual([r[1] for r in rows], ["M0 Foundations", "M0 duplicate"])


class Adrs(unittest.TestCase):
    README = "\n".join([
        "| ADR | Title | Status |",
        "| --- | --- | --- |",
        "| [0001](0001-name.md) | Name | Accepted |",
        "| [0019](0019-minio.md) | MinIO image | Proposed |",
        "| [0021](0021-x.md) | X | Superseded by 0022 |",
        "| [0022](0022-y.md) | Y | **Open** |",
        "not a row",
    ])

    def test_reads_rows_and_counts_statuses(self):
        rows = ps.adr_rows(self.README)
        self.assertEqual([r[0] for r in rows], ["0001", "0019", "0021", "0022"])
        counts = ps.count_adr_statuses(rows)
        self.assertEqual((counts["Accepted"], counts["Proposed"], counts["Superseded"], counts["Open"]),
                         (1, 1, 1, 1))


class CodeStats(unittest.TestCase):
    def test_counts_rust_and_typescript_by_unit_and_skips_the_generated_client(self):
        with tempfile.TemporaryDirectory() as root:
            files = {
                "crates/stepclimb-core/src/lib.rs": "pub fn a() {}\n#[test]\nfn t() {}\n",
                "crates/stepclimb-core/tests/db.rs": "#[sqlx::test]\nasync fn t() {}\n",
                "web/src/a.test.tsx": "it('x', () => {});\ntest('y', () => {});\n",
                "packages/ui/src/b.tsx": "export const b = 1;\n",
                "packages/api-client/src/schema.ts": "export interface paths {}\n",
                "docs/a.md": "x\n",
            }
            for path, text in files.items():
                os.makedirs(os.path.dirname(os.path.join(root, path)), exist_ok=True)
                with open(os.path.join(root, path), "w") as f:
                    f.write(text)
            stats, unreadable = ps.code_stats(root, list(files) + ["crates/x/src/gone.rs"])
        self.assertEqual(stats["stepclimb-core"], ("Rust", 5, 2))
        self.assertEqual(stats["web"], ("TypeScript", 2, 2))
        self.assertEqual(stats["packages/ui"], ("TypeScript", 1, 0))
        self.assertNotIn("packages/api-client", stats)
        self.assertEqual(unreadable, ["crates/x/src/gone.rs"])


class Worktrees(unittest.TestCase):
    def test_parses_porcelain(self):
        text = "worktree /r\nHEAD a\nbranch refs/heads/next\n\nworktree /r/.claude/worktrees/x\nHEAD b\ndetached\n"
        self.assertEqual(ps.parse_worktrees(text), [("/r", "next"), ("/r/.claude/worktrees/x", "(detached)")])


class Run(unittest.TestCase):
    def test_runs_without_github_and_names_every_gap(self):
        # A `gh` that always fails stands in for GitHub being unreachable; with no origin remote
        # the GitHub sections are skipped instead. Either way each names its gap.
        with tempfile.TemporaryDirectory() as stub:
            gh = os.path.join(stub, "gh")
            with open(gh, "w") as f:
                f.write("#!/bin/sh\necho 'gh stub: offline' >&2\nexit 1\n")
            os.chmod(gh, 0o755)
            env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
            env["PATH"] = stub + os.pathsep + env.get("PATH", "")
            out = subprocess.run(["bash", SCRIPT], capture_output=True, text=True, env=env,
                                 timeout=120, check=False)
        self.assertEqual(out.returncode, 0, out.stderr)
        for heading in ("## Repo", "## Milestones (GitHub)", "## Pull requests", "## ADRs",
                        "## Code size", "## Worktrees"):
            self.assertIn(heading, out.stdout)
        self.assertNotIn("section failed", out.stdout)


if __name__ == "__main__":
    unittest.main()
