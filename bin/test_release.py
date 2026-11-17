# SPDX-License-Identifier: AGPL-3.0-or-later
"""The release recipe (bin/release.py, cliff.toml; docs/specs/m0/01-repo-and-ci.md B15, T7).
Each repository test runs the script in a scratch clone (`work`) whose `origin` is a local bare
repository. Merged PRs, the release merge and the tag are pushed from a second clone (`other`),
so `work` sees them only through the script's own fetch. Nothing reaches the network; git-cliff
must be on PATH (the Nix dev shell)."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import release as r  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "bin" / "release.py"
# The files the script reads from the checkout being released.
CHECKOUT_FILES = ("cliff.toml", ".github/release-pr-template.md", ".claude/hooks/lib/attribution.sh")
CARGO = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "{}"\nedition = "2024"\n'
# Stands in for `just`: `release-version <v>` rewrites the workspace version, as the real recipe
# does for every manifest, then writes the notes the way the real recipe does.
FAKE_JUST = f"""#!/usr/bin/env python3
import re, subprocess, sys
assert sys.argv[1] == "release-version", sys.argv
text = open("Cargo.toml").read()
open("Cargo.toml", "w").write(re.sub(r'^version = "[^"]*"', f'version = "{{sys.argv[2]}}"', text, flags=re.M))
sys.exit(subprocess.run([sys.executable, "-B", "{SCRIPT}", "notes", sys.argv[2]]).returncode)
"""
FAILING_JUST = "#!/usr/bin/env python3\nopen('Cargo.toml', 'a').write('# half done\\n')\nraise SystemExit(1)\n"


class Precedence(unittest.TestCase):
    def test_follows_the_semver_specification_example(self) -> None:
        # Semantic Versioning 2.0.0, section 11, item 4: this list is in ascending order.
        ordered = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta",
                   "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0"]
        self.assertEqual(sorted(reversed(ordered), key=r.precedence), ordered)

    def test_orders_numbers_numerically(self) -> None:
        self.assertLess(r.precedence("0.9.0"), r.precedence("0.10.0"))
        self.assertLess(r.precedence("0.1.0-test.2"), r.precedence("0.1.0-test.10"))
        self.assertLess(r.precedence("0.0.0"), r.precedence("0.1.0-test.1"))


class Changelog(unittest.TestCase):
    SECTION = "## v0.2.0\n\n### Fixes\n\n- Two\n"
    OLD = r.CHANGELOG_HEADER + "\n## v0.1.0\n\n### Features\n\n- One\n"

    def test_starts_a_file_with_the_header(self) -> None:
        text = r.add_section(None, "0.1.0", "## v0.1.0\n\n- One\n")
        self.assertTrue(text.startswith("# Changelog\n"))
        self.assertTrue(text.endswith("\n## v0.1.0\n\n- One\n"))

    def test_puts_the_new_section_above_the_newest(self) -> None:
        text = r.add_section(self.OLD, "0.2.0", self.SECTION)
        self.assertLess(text.index("## v0.2.0"), text.index("## v0.1.0"))
        self.assertTrue(text.startswith(r.CHANGELOG_HEADER))

    def test_refuses_a_second_section_for_a_version(self) -> None:
        with self.assertRaisesRegex(r.ReleaseError, "already has a section for v0.1.0"):
            r.add_section(self.OLD, "0.1.0", "## v0.1.0\n\n- Again\n")

    def test_reads_one_section_without_its_heading(self) -> None:
        text = r.add_section(self.OLD, "0.2.0", self.SECTION)
        self.assertEqual(r.release_section(text, "0.2.0"), "### Fixes\n\n- Two\n")
        self.assertEqual(r.release_section(text, "0.1.0"), "### Features\n\n- One\n")

    def test_refuses_a_missing_or_empty_section(self) -> None:
        with self.assertRaisesRegex(r.ReleaseError, "no section for v0.3.0"):
            r.release_section(self.OLD, "0.3.0")
        with self.assertRaisesRegex(r.ReleaseError, "section for v0.2.0 is empty"):
            r.release_section("## v0.2.0\n\n## v0.1.0\n\n- One\n", "0.2.0")

    def test_tags_need_a_v_and_a_version(self) -> None:
        self.assertEqual(r.tag_version("v0.1.0-test.1"), "0.1.0-test.1")
        for tag in ("0.1.0", "v0.1", "v01.0.0", "release-1", "v0.1.0-rc.01", "v1.0.0-00"):
            with self.assertRaises(r.ReleaseError, msg=tag):
                r.tag_version(tag)


class Scratch(unittest.TestCase):
    """A bare `origin` with `main` and `next`, `work` (where the script runs) and `other` (where
    everyone else's pushes come from)."""

    def setUp(self) -> None:
        # Resolved, because git reports worktree paths through symlinks such as macOS's /tmp.
        self.tmp = Path(tempfile.mkdtemp(prefix="release-")).resolve()
        self.addCleanup(shutil.rmtree, self.tmp)
        config = self.tmp / "gitconfig"
        config.write_text("[user]\n\tname = Test\n\temail = test@example.invalid\n"
                          "[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n")
        patcher = mock.patch.dict(os.environ, {"GIT_CONFIG_GLOBAL": str(config), "GIT_CONFIG_NOSYSTEM": "1"})
        patcher.start()
        self.addCleanup(patcher.stop)
        self.just = self.fake("just", FAKE_JUST)
        self.origin = self.tmp / "origin.git"
        self.git(self.tmp, "init", "-q", "--bare", "-b", "next", str(self.origin))
        self.other = self.tmp / "other"
        self.git(self.tmp, "clone", "-q", str(self.origin), str(self.other))
        for path in CHECKOUT_FILES:
            (self.other / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / path, self.other / path)
        (self.other / "Cargo.toml").write_text(CARGO.format("0.0.0"))
        (self.other / ".gitignore").write_text("/target\n")
        self.commit(self.other, "Initial commit", ".")
        self.git(self.other, "push", "-q", "origin", "HEAD:next", "HEAD:main")
        self.work = self.tmp / "work"
        self.git(self.tmp, "clone", "-q", str(self.origin), str(self.work))

    def fake(self, name: str, text: str) -> Path:
        path = self.tmp / name
        path.write_text(text)
        path.chmod(0o755)
        return path

    def git(self, cwd: Path, *args: str) -> str:
        return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True,
                              text=True).stdout.strip()

    def commit(self, repo: Path, message: str, *paths: str) -> None:
        if paths:
            self.git(repo, "add", *paths)
        self.git(repo, "commit", "-q", "--allow-empty", "-m", message)

    def land(self, *messages: str) -> None:
        """Squash-merged PRs: commits on next, pushed from `other`."""
        self.git(self.other, "switch", "-q", "next")
        self.git(self.other, "pull", "-q", "--ff-only")
        for message in messages:
            self.commit(self.other, message)
        self.git(self.other, "push", "-q", "origin", "next")

    def run_release(self, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, "-B", str(SCRIPT), *args], cwd=cwd or self.work,
                              capture_output=True, text=True)

    def prepare(self, version: str, just: Path | None = None) -> subprocess.CompletedProcess[str]:
        return self.run_release("prepare", version, str(just or self.just))

    def state(self, repo: Path | None = None) -> tuple[str, str, str]:
        """HEAD, the local branches and tags, and the status: what a refusal must leave alone."""
        repo = repo or self.work
        refs = self.git(repo, "for-each-ref", "--format=%(refname) %(objectname)",
                        "refs/heads", "refs/tags")
        return (self.git(repo, "rev-parse", "HEAD"), refs,
                self.git(repo, "status", "--porcelain", "--untracked-files=all"))

    def origin_refs(self) -> str:
        return self.git(self.origin, "for-each-ref", "--format=%(refname) %(objectname)")

    def notes(self, version: str) -> str:
        return r.release_section((self.work / "CHANGELOG.md").read_text(), version)

    def squash_prep_into_next(self, version: str) -> None:
        """The person pushes the prepared branch; GitHub squash-merges it into next with the
        branch's commit messages as the body (the repository's COMMIT_MESSAGES setting)."""
        branch = self.git(self.work, "branch", "--show-current")
        self.git(self.work, "push", "-q", "origin", branch)
        self.git(self.other, "fetch", "-q", "origin")
        self.git(self.other, "switch", "-q", "next")
        self.git(self.other, "pull", "-q", "--ff-only")
        body = self.git(self.other, "log", "--reverse", "--format=* %s%n%n%b", f"next..origin/{branch}")
        self.git(self.other, "merge", "-q", "--squash", f"origin/{branch}")
        self.git(self.other, "commit", "-q", "-m", f"chore(release): prepare v{version} (#90)", "-m", body)
        self.git(self.other, "push", "-q", "origin", "next")

    def merge_next_into_main(self, version: str, tag: bool = True) -> None:
        """The release PR's merge commit on main, and the annotated tag on it."""
        self.git(self.other, "fetch", "-q", "origin")
        self.git(self.other, "switch", "-q", "--detach", "origin/main")
        self.git(self.other, "merge", "-q", "--no-ff", "origin/next", "-m", "Merge pull request #91 from org/next")
        self.git(self.other, "push", "-q", "origin", "HEAD:main")
        if tag:
            self.git(self.other, "tag", "-a", f"v{version}", "-m", f"Stepclimb v{version}")
            self.git(self.other, "push", "-q", "origin", f"v{version}")

    def release_into_main(self, version: str) -> None:
        self.squash_prep_into_next(version)
        self.merge_next_into_main(version)


class Prepare(Scratch):
    def test_branches_bumps_and_writes_grouped_notes(self) -> None:
        self.land("feat(server): add the probe (#1)", "fix(web): stop the flicker (#2)",
                  "Merge branch 'x'", "feat(core)!: rename the table (#3)",
                  "chore(release): prepare v0.0.9 (#4)", "docs: explain the gate (#5)")
        before = self.origin_refs()
        result = self.prepare("0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        base = self.git(self.work, "rev-parse", "origin/next")
        self.assertEqual(base, self.git(self.other, "rev-parse", "next"), "prepare must fetch next")
        self.assertEqual(self.git(self.work, "branch", "--show-current"),
                         f"chore/{base[:7]}/release-v0.1.0")
        self.assertEqual(self.git(self.work, "log", "--format=%s", f"{base}..HEAD").splitlines(),
                         ["chore(release): add the v0.1.0 release notes",
                          "chore(release): set the version to 0.1.0"])
        self.assertEqual(self.git(self.work, "show", "--name-only", "--format=", "HEAD~1"), "Cargo.toml")
        self.assertIn(f"The v0.1.0 notes cover origin/next up to {base}.",
                      self.git(self.work, "log", "-1", "--format=%b"))
        self.assertIn('version = "0.1.0"', (self.work / "Cargo.toml").read_text())
        self.assertEqual(self.notes("0.1.0"), "### Breaking changes\n\n- **core:** Rename the table (#3)\n\n"
                                              "### Features\n\n- **server:** Add the probe (#1)\n\n"
                                              "### Fixes\n\n- **web:** Stop the flicker (#2)\n\n"
                                              "### Documentation\n\n- Explain the gate (#5)\n")
        body = (self.work / "target/release/v0.1.0-pr.md").read_text()
        self.assertIn("as Stepclimb v0.1.0.", body)
        self.assertIn("- **server:** Add the probe (#1)", body)
        self.assertNotIn("$", body)
        self.assertEqual(self.git(self.work, "status", "--porcelain"), "")
        self.assertEqual(self.origin_refs(), before, "prepare must push nothing")
        self.assertIn("Nothing was pushed", result.stdout)
        self.assertIn('gh pr create --base main --head next --title "chore(release): v0.1.0"', result.stdout)

    def test_reverts_and_breaking_footers_reach_the_notes(self) -> None:
        self.land("feat(server): add the probe (#1)", 'Revert "feat(server): add the probe (#1)" (#2)',
                  "fix(core): move the key (#3)\n\nBREAKING CHANGE: the key moved\nsecond line of the footer")
        self.assertEqual(self.prepare("0.1.0").returncode, 0)
        self.assertEqual(self.notes("0.1.0"), "### Breaking changes\n\n- **core:** Move the key (#3). The key moved\n\n"
                                              "### Features\n\n- **server:** Add the probe (#1)\n\n"
                                              "### Reverts\n\n- Revert \"feat(server): add the probe (#1)\" (#2)\n")

    def test_second_release_lists_only_what_came_after_the_first(self) -> None:
        self.land("feat(server): first feature (#1)")
        self.assertEqual(self.prepare("0.1.0").returncode, 0)
        self.release_into_main("0.1.0")
        self.land("fix(server): later fix (#2)")
        result = self.prepare("0.2.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.notes("0.2.0"), "### Fixes\n\n- **server:** Later fix (#2)\n")
        self.assertEqual(self.notes("0.1.0"), "### Features\n\n- **server:** First feature (#1)\n")
        changelog = (self.work / "CHANGELOG.md").read_text()
        self.assertLess(changelog.index("## v0.2.0"), changelog.index("## v0.1.0"))

    def test_a_fixed_hotfix_on_main_does_not_block(self) -> None:
        self.hotfix_on_main()
        self.git(self.other, "switch", "-q", "next")
        (self.other / "HOTFIX").write_text("x")
        self.commit(self.other, "fix: bring the hotfix into next (#7)", "HOTFIX")
        self.git(self.other, "push", "-q", "origin", "next")
        result = self.prepare("0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)

    def hotfix_on_main(self) -> None:
        self.land("feat: one (#1)")
        self.git(self.other, "switch", "-q", "--detach", "origin/main")
        (self.other / "HOTFIX").write_text("x")
        self.commit(self.other, "fix: on main only", "HOTFIX")
        self.git(self.other, "push", "-q", "origin", "HEAD:main")

    def assert_refused(self, version: str, message: str, just: Path | None = None) -> None:
        before, origin = self.state(), self.origin_refs()
        result = self.prepare(version, just)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(message, result.stderr)
        self.assertEqual(self.state(), before, "a refusal must leave the checkout as it was")
        self.assertEqual(self.origin_refs(), origin)

    def test_refuses_a_malformed_version(self) -> None:
        self.land("feat: one (#1)")
        self.assert_refused("v0.1.0", "is not a version like 0.1.0")
        self.assert_refused("0.1.0-rc.01", "is not a version like 0.1.0")

    def test_refuses_a_version_not_newer_than_next(self) -> None:
        (self.other / "Cargo.toml").write_text(CARGO.format("0.1.0"))
        self.commit(self.other, "chore(release): set the version to 0.1.0", "Cargo.toml")
        self.land("feat: one (#1)")
        self.assert_refused("0.1.0", "0.1.0 is not newer than 0.1.0")
        self.assert_refused("0.1.0-rc.1", "0.1.0-rc.1 is not newer than 0.1.0")

    def test_refuses_an_existing_tag(self) -> None:
        self.land("feat: one (#1)")
        self.git(self.other, "tag", "-a", "v0.1.0", "-m", "v0.1.0")
        self.git(self.other, "push", "-q", "origin", "v0.1.0")
        self.git(self.work, "fetch", "-q", "--tags", "origin")
        self.assert_refused("0.1.0", "the tag v0.1.0 already exists")

    def test_refuses_a_version_next_already_has_notes_for(self) -> None:
        (self.other / "CHANGELOG.md").write_text(r.CHANGELOG_HEADER + "\n## v0.1.0\n\n- Old\n")
        self.commit(self.other, "docs: notes by hand (#1)", "CHANGELOG.md")
        self.land("feat: one (#2)")
        self.assert_refused("0.1.0", "CHANGELOG.md on origin/next already has a section for v0.1.0")

    def test_refuses_a_dirty_tree(self) -> None:
        self.land("feat: one (#1)")
        (self.work / "stray.txt").write_text("x")
        self.assert_refused("0.1.0", "the working tree has changes")

    def test_refuses_when_main_has_changes_next_lacks(self) -> None:
        self.hotfix_on_main()
        self.assert_refused("0.1.0", "origin/main has changes origin/next does not (HOTFIX)")

    def test_refuses_when_nothing_is_left_to_release(self) -> None:
        self.land("chore(release): prepare v0.0.1 (#1)", "Merge branch 'x'")
        self.assert_refused("0.1.0", "no Conventional Commit to release")

    def test_refuses_notes_that_carry_attribution(self) -> None:
        self.land("feat(server): add the probe \U0001F916 (#1)")
        self.assert_refused("0.1.0", "the notes carry AI attribution")

    def test_refuses_an_existing_branch(self) -> None:
        self.land("feat: one (#1)")
        self.git(self.work, "fetch", "-q", "origin")
        base = self.git(self.work, "rev-parse", "origin/next")
        self.git(self.work, "branch", f"chore/{base[:7]}/release-v0.1.0", base)
        self.assert_refused("0.1.0", f"the branch chore/{base[:7]}/release-v0.1.0 already exists")

    def test_a_failure_after_branching_names_the_way_back(self) -> None:
        self.land("feat: one (#1)")
        before = self.state()
        result = self.prepare("0.1.0", self.fake("failing-just", FAILING_JUST))
        self.assertEqual(result.returncode, 1)
        base = self.git(self.work, "rev-parse", "origin/next")
        hint = f"git reset --hard && git switch - && git branch -D chore/{base[:7]}/release-v0.1.0"
        self.assertIn(hint, result.stderr)
        subprocess.run(["bash", "-c", hint], cwd=self.work, check=True, capture_output=True)
        self.assertEqual(self.state(), before)


class Release(Scratch):
    """`verify` and `finish`, the two halves of `just release` around `just ci`."""

    TAG = "v0.1.0-test.1"

    def setUp(self) -> None:
        super().setUp()
        self.land("feat(server): add the probe (#1)")
        self.assertEqual(self.prepare(self.TAG[1:]).returncode, 0)
        self.release_into_main(self.TAG[1:])
        self.tagged = self.checkout_tag(self.TAG)

    def checkout_tag(self, tag: str) -> Path:
        """A worktree of `tag` from `work`, which fetches only the tag, not main."""
        self.git(self.work, "fetch", "-q", "origin", f"refs/tags/{tag}:refs/tags/{tag}")
        path = self.tmp / f"tagged-{tag}"
        self.git(self.work, "worktree", "add", "-q", "--detach", str(path), tag)
        return path

    def tag_in_other(self, tag: str, commit: str) -> None:
        self.git(self.other, "tag", "-a", tag, "-m", tag, commit)
        self.git(self.other, "push", "-q", "origin", tag)

    def assert_refused(self, tag: str, message: str, cwd: Path | None = None) -> None:
        cwd = cwd or self.tagged
        before = self.state(cwd)
        result = self.run_release("verify", tag, cwd=cwd)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(message, result.stderr)
        self.assertEqual(self.state(cwd), before)
        self.assertFalse((cwd / f"target/release/{tag}-notes.md").exists())

    def stamp(self, commit: str) -> None:
        git_dir = Path(self.git(self.tagged, "rev-parse", "--absolute-git-dir"))
        (git_dir / "stepclimb-ci-pass").write_text(commit + "\n")

    def test_verify_writes_the_notes_of_the_tagged_version(self) -> None:
        self.assertNotEqual(self.git(self.work, "rev-parse", "origin/main"),
                            self.git(self.other, "rev-parse", "origin/main"), "work must not know main yet")
        result = self.run_release("verify", self.TAG, cwd=self.tagged)
        self.assertEqual(result.returncode, 0, result.stderr)
        notes = (self.tagged / f"target/release/{self.TAG}-notes.md").read_text()
        self.assertEqual(notes, "### Features\n\n- **server:** Add the probe (#1)\n")

    def test_verify_refuses_a_tag_that_differs_from_the_workspace_version(self) -> None:
        self.tag_in_other("v0.1.0-test.2", "origin/main")
        tagged = self.checkout_tag("v0.1.0-test.2")
        self.assert_refused("v0.1.0-test.2", "does not match the workspace version 0.1.0-test.1", tagged)

    def test_verify_refuses_a_lightweight_tag(self) -> None:
        self.git(self.work, "tag", "v0.1.0-test.3", f"{self.TAG}^{{commit}}")
        self.assert_refused("v0.1.0-test.3", "is a lightweight tag")

    def test_verify_refuses_when_head_is_not_the_tag(self) -> None:
        self.git(self.work, "switch", "-q", "--detach", "origin/next")
        self.assert_refused(self.TAG, f"HEAD is not {self.TAG}", cwd=self.work)

    def test_verify_refuses_a_tag_off_mains_first_parent_line(self) -> None:
        # next's tip carries the version and is in main's history, but only as the merge's
        # second parent.
        self.git(self.other, "fetch", "-q", "origin")
        self.git(self.other, "tag", "-d", self.TAG)
        self.git(self.other, "push", "-q", "origin", f":refs/tags/{self.TAG}")
        self.git(self.work, "tag", "-d", self.TAG)
        self.tag_in_other(self.TAG, "origin/next")
        self.git(self.work, "fetch", "-q", "origin", f"refs/tags/{self.TAG}:refs/tags/{self.TAG}")
        self.git(self.tagged, "checkout", "-q", "--detach", self.TAG)
        self.assert_refused(self.TAG, f"{self.TAG} is not on origin/main's first-parent history")

    def test_verify_refuses_a_tag_that_is_not_a_merge(self) -> None:
        self.git(self.other, "fetch", "-q", "origin")
        self.git(self.other, "switch", "-q", "--detach", "origin/main")
        (self.other / "Cargo.toml").write_text(CARGO.format("0.1.0-test.4"))
        self.commit(self.other, "chore(release): set the version to 0.1.0-test.4", "Cargo.toml")
        self.git(self.other, "push", "-q", "origin", "HEAD:main")
        self.tag_in_other("v0.1.0-test.4", "HEAD")
        tagged = self.checkout_tag("v0.1.0-test.4")
        self.assert_refused("v0.1.0-test.4", "v0.1.0-test.4 is not a merge commit", tagged)

    def test_verify_refuses_when_next_moved_after_the_notes(self) -> None:
        self.land("feat(server): second (#5)")
        self.assertEqual(self.prepare("0.2.0").returncode, 0)
        self.squash_prep_into_next("0.2.0")
        self.land("fix(server): merged mid-release (#6)")
        self.merge_next_into_main("0.2.0")
        tagged = self.checkout_tag("v0.2.0")
        self.assert_refused("v0.2.0", "next gained commits after the v0.2.0 notes were written", tagged)

    def test_verify_refuses_notes_edited_by_hand(self) -> None:
        self.land("feat(server): second (#5)")
        self.assertEqual(self.prepare("0.2.0").returncode, 0)
        changelog = self.work / "CHANGELOG.md"
        changelog.write_text(changelog.read_text().replace("Second (#5)", "Second, reworded (#5)"))
        self.git(self.work, "commit", "-q", "-am", "chore(release): reword the notes")
        self.release_into_main("0.2.0")
        tagged = self.checkout_tag("v0.2.0")
        self.assert_refused("v0.2.0", "section differs from what git-cliff writes", tagged)

    def test_verify_refuses_a_tag_without_release_notes(self) -> None:
        self.land("feat(server): second (#5)")
        self.assertEqual(self.prepare("0.2.0").returncode, 0)
        self.git(self.work, "rm", "-q", "CHANGELOG.md")
        self.git(self.work, "commit", "-q", "-m", "chore(release): drop the notes")
        self.release_into_main("0.2.0")
        tagged = self.checkout_tag("v0.2.0")
        self.assert_refused("v0.2.0", "v0.2.0 has no CHANGELOG.md", tagged)

    def test_verify_refuses_a_dirty_tree(self) -> None:
        (self.tagged / "stray.txt").write_text("x")
        self.assert_refused(self.TAG, "the working tree has changes")

    def test_finish_refuses_without_a_pass_on_the_tag(self) -> None:
        self.assertEqual(self.run_release("verify", self.TAG, cwd=self.tagged).returncode, 0)
        result = self.run_release("finish", self.TAG, cwd=self.tagged)
        self.assertEqual(result.returncode, 1)
        self.assertIn("just ci has not passed", result.stderr)
        self.stamp(self.git(self.work, "rev-parse", "origin/next"))
        result = self.run_release("finish", self.TAG, cwd=self.tagged)
        self.assertEqual(result.returncode, 1)
        self.assertIn("just ci has not passed", result.stderr)
        self.assertNotIn("gh release create", result.stdout)

    def test_finish_checks_the_pass_and_publishes_nothing(self) -> None:
        self.assertEqual(self.run_release("verify", self.TAG, cwd=self.tagged).returncode, 0)
        self.stamp(self.git(self.tagged, "rev-parse", "HEAD"))
        before = self.origin_refs()
        result = self.run_release("finish", self.TAG, cwd=self.tagged)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"just ci passed on {self.TAG}", result.stdout)
        self.assertNotIn("gh release create", result.stdout)
        self.assertEqual(self.origin_refs(), before)

    def test_finish_refuses_without_the_notes(self) -> None:
        self.stamp(self.git(self.tagged, "rev-parse", "HEAD"))
        result = self.run_release("finish", self.TAG, cwd=self.tagged)
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"{self.TAG}-notes.md is missing", result.stderr)

if __name__ == "__main__":
    unittest.main()
