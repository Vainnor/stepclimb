# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the post-merge cleanup and its hook. Run with `just agents-check`.

Every test builds throwaway repositories under a temporary directory: an `origin` bare repo and a
main checkout with agent worktrees under `.claude/worktrees/`. `gh`, `ps`, `lsof` and `docker`
are stubs on PATH that read and log to files in the test's stub directory, so no test touches
GitHub, Docker or this repository's worktrees.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import time
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import post_merge_cleanup  # noqa: E402

LIB = os.path.dirname(os.path.abspath(__file__))
HOOKS = os.path.dirname(LIB)
ROOT = os.path.dirname(os.path.dirname(HOOKS))
SCRIPT = os.path.join(ROOT, ".claude", "scripts", "post-merge-cleanup.sh")
GITHUB_URL = "https://github.com/example-org/stepclimb.git"

GH_STUB = r'''#!/usr/bin/env python3
import json, os, sys
stub = os.environ["STUB_DIR"]
with open(os.path.join(stub, "gh.log"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\n")
if os.path.exists(os.path.join(stub, "gh-fails")):
    sys.exit("gh: could not reach api.github.com")
with open(os.path.join(stub, "prs.json")) as f:
    prs = json.load(f)
args = sys.argv[1:]
if args[:2] == ["pr", "list"]:
    print(json.dumps(prs))
elif args[:2] == ["pr", "view"]:
    sel = args[2]
    if sel.startswith("-"):  # no PR named: gh uses the current branch of its directory
        import subprocess
        sel = subprocess.run(["git", "branch", "--show-current"], capture_output=True,
                             text=True).stdout.strip()
    match = [p for p in prs if str(p["number"]) == sel or p["headRefName"] == sel]
    if not match:
        sys.exit("no pull requests found")
    print(json.dumps(match[0]))
else:
    sys.exit("gh stub: unexpected " + " ".join(args))
'''

# `ps` and `lsof` print the process table the test wrote; `ps` fails when asked to. On Windows
# the cleanup asks PowerShell instead, and its stub prints each pid with the first word of its
# command from the same table, the executable.
PS_STUB = r'''#!/usr/bin/env python3
import os, sys
stub = os.environ["STUB_DIR"]
if os.path.exists(os.path.join(stub, "ps-fails")):
    sys.exit("ps: operation not permitted")
table = os.path.join(stub, "ps.txt")
if os.path.exists(table):
    sys.stdout.write(open(table).read())
'''

LSOF_STUB = r'''#!/usr/bin/env python3
import os, sys
stub = os.environ["STUB_DIR"]
if os.path.exists(os.path.join(stub, "lsof-missing")):
    print("lsof: command not found", file=sys.stderr)
    sys.exit(127)
table = os.path.join(stub, "lsof.txt")
text = open(table).read() if os.path.exists(table) else ""
# Real lsof always lists at least the process that asked, so an empty table lists an init.
sys.stdout.write(text or "p1\nn/\n")
'''

POWERSHELL_STUB = r'''#!/usr/bin/env python3
import os, sys
stub = os.environ["STUB_DIR"]
if os.path.exists(os.path.join(stub, "ps-fails")):
    sys.exit("powershell: operation not permitted")
table = os.path.join(stub, "ps.txt")
for line in open(table).read().splitlines() if os.path.exists(table) else []:
    fields = line.split(None, 2)
    if len(fields) >= 2:
        print(fields[0] + "\t" + fields[1])
'''

# The program that lists processes, as the report names it.
LISTER = "powershell" if os.name == "nt" else "ps"



# Records every call: the cleanup must never run docker.
DOCKER_STUB = r'''#!/usr/bin/env python3
import os, sys
with open(os.path.join(os.environ["STUB_DIR"], "docker.log"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\n")
'''


def install_stub(directory, name, body):
    """An executable script `name` in `directory`. Windows runs no extensionless script, so there
    a `name.bat` beside it runs it with this Python, and the cleanup finds the `.bat` on PATH."""
    path = os.path.join(directory, name)
    with open(path, "w") as f:
        f.write(body)
    os.chmod(path, 0o755)
    if os.name == "nt":
        with open(path + ".bat", "w") as f:
            f.write(f'@"{sys.executable}" -X utf8 "%~dp0{name}" %*\n')


def sh(cwd, *args):
    out = subprocess.run(args, cwd=cwd, capture_output=True, text=True, check=False)
    if out.returncode != 0:
        raise AssertionError(f"{args} failed: {out.stderr}")
    return out.stdout.strip()


class Repo(unittest.TestCase):
    """A main checkout on `next` with an origin, stubs on PATH, and helpers to add worktrees."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = os.path.realpath(self.tmp.name)
        self.origin = os.path.join(base, "origin.git")
        self.main = os.path.join(base, "main")
        self.stub = os.path.join(base, "stub")
        os.mkdir(self.stub)
        for name, body in (("gh", GH_STUB), ("docker", DOCKER_STUB),
                           ("ps", PS_STUB), ("lsof", LSOF_STUB),
                           ("powershell", POWERSHELL_STUB)):
            install_stub(self.stub, name, body)
        # GIT_DIR, GIT_INDEX_FILE and the like from a calling git hook would aim every git
        # command here at the real repository.
        clean = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        self.env = dict(
            clean, STUB_DIR=self.stub, PATH=self.stub + os.pathsep + os.environ["PATH"],
            GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
            GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com",
            GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com",
        )
        self.prs = []
        self.write_prs()
        self.git(base, "init", "-q", "--bare", "-b", "next", self.origin)
        self.git(base, "init", "-q", "-b", "next", self.main)
        self.git(self.main, "remote", "add", "origin", GITHUB_URL)
        self.git(self.main, "config", f"url.{self.origin}.insteadOf", GITHUB_URL)
        with open(os.path.join(self.main, ".gitignore"), "w") as f:
            f.write("/target\n**/target\n.env\n")
        self.git(self.main, "add", ".gitignore")
        self.commit(self.main, "seed")
        self.git(self.main, "push", "-q", "origin", "next")
        self.git(self.main, "branch", "main")
        self.git(self.main, "fetch", "-q", "origin")

    def tearDown(self):
        self.tmp.cleanup()

    def git(self, cwd, *args):
        out = subprocess.run(
            ["git", *args], cwd=cwd, capture_output=True, text=True, env=self.env, check=False,
        )
        if out.returncode != 0:
            raise AssertionError(f"git {args} failed: {out.stderr}")
        return out.stdout.strip()

    def commit(self, cwd, name):
        with open(os.path.join(cwd, name), "w") as f:
            f.write(name)
        self.git(cwd, "add", name)
        self.git(cwd, "commit", "-q", "-m", name)
        return self.git(cwd, "rev-parse", "HEAD")

    def write_prs(self):
        with open(os.path.join(self.stub, "prs.json"), "w") as f:
            json.dump(self.prs, f)

    def pr(self, number, branch, state, head):
        self.prs.append({
            "number": number, "state": state, "headRefName": branch, "headRefOid": head,
            "url": f"https://github.com/example-org/stepclimb/pull/{number}",
        })
        self.write_prs()

    def worktree(self, name, branch, pr_state=None, number=1, path=None):
        """An agent worktree on `branch` with one pushed commit and its harness branch.

        `path` puts it elsewhere, as T3 does under `~/.t3/worktrees/`.
        """
        path = path or os.path.join(self.main, ".claude", "worktrees", name)
        self.git(self.main, "branch", f"worktree-{name}", "origin/next")
        self.git(self.main, "worktree", "add", "-q", "-b", branch, path, "origin/next")
        head = self.commit(path, f"{name}.txt")
        self.git(path, "push", "-q", "origin", branch)
        if pr_state:
            self.pr(number, branch, pr_state, head)
        return path

    def run_cleanup(self, *args, cwd=None):
        out = subprocess.run(
            ["bash", SCRIPT, *args], cwd=cwd or self.main, capture_output=True, text=True,
            env=self.env, check=False, timeout=120,
        )
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        return out.stdout

    def branches(self):
        return set(self.git(self.main, "for-each-ref", "--format=%(refname:short)", "refs/heads/")
                   .split())



class Worktrees(Repo):
    def test_removes_a_merged_worktree_with_its_branches(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(path), report)
        self.assertNotIn("feat/x/a", self.branches())
        self.assertNotIn("worktree-agent-a", self.branches())
        self.assertFalse(os.path.exists(os.path.join(self.stub, "docker.log")), "docker was run")
        self.assertIn("removed worktree .claude/worktrees/agent-a (feat/x/a: PR #1 merged)", report)

    def test_removes_a_worktree_whose_pr_was_closed(self):
        path = self.worktree("agent-a", "feat/x/a", "CLOSED")
        self.run_cleanup()
        self.assertFalse(os.path.exists(path))

    def test_keeps_a_dirty_worktree(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        with open(os.path.join(path, "new.txt"), "w") as f:
            f.write("work")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("kept worktree .claude/worktrees/agent-a: dirty", report)
        self.assertIn("feat/x/a", self.branches())

    def test_keeps_unpushed_commits(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        self.commit(path, "after-merge.txt")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("feat/x/a: unpushed commits", report)

    def test_keeps_an_open_pr(self):
        path = self.worktree("agent-a", "feat/x/a", "OPEN")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("feat/x/a: PR #1 is open", report)

    def test_keeps_a_branch_with_an_open_pr_beside_a_closed_one(self):
        path = self.worktree("agent-a", "feat/x/a", "CLOSED")
        self.pr(2, "feat/x/a", "OPEN", self.git(path, "rev-parse", "HEAD"))
        self.run_cleanup()
        self.assertTrue(os.path.exists(path))

    def test_keeps_a_branch_with_no_pr(self):
        # A running agent that has not opened its PR yet.
        path = self.worktree("agent-a", "feat/x/a")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("feat/x/a: no PR", report)

    def test_keeps_a_locked_worktree_with_an_open_pr_and_its_lock(self):
        # The lock's pid is dead, so only the open PR keeps it, and the lock must survive.
        path = self.worktree("agent-a", "feat/x/a", "OPEN")
        gone = subprocess.Popen(["true"])
        gone.wait()
        reason = f"claude agent (pid {gone.pid})"
        self.git(self.main, "worktree", "lock", "--reason", reason, path)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("agent-a: feat/x/a: PR #1 is open", report)
        self.assertIn(f"locked {reason}", self.git(self.main, "worktree", "list", "--porcelain"))

    def test_keeps_a_merged_worktree_whose_lock_names_no_pid(self):
        # A manual `git worktree lock` is a person saying keep it.
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        self.git(self.main, "worktree", "lock", "--reason", "owner: keep for bisect", path)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("agent-a: locked (no pid): owner: keep for bisect", report)
        self.assertIn("locked owner: keep for bisect",
                      self.git(self.main, "worktree", "list", "--porcelain"))

    def test_keeps_a_merged_worktree_whose_lock_pid_is_alive(self):
        # An idle agent has no process in its worktree; only its lock says it is still running.
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        reason = f"claude agent agent-a (pid {os.getpid()} start Wed Oct 29 15:23:52 2026)"
        self.git(self.main, "worktree", "lock", "--reason", reason, path)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn(f"kept worktree .claude/worktrees/agent-a: locked by live pid {os.getpid()}",
                      report)
        self.assertIn(f"locked {reason}", self.git(self.main, "worktree", "list", "--porcelain"))

    def test_unlocks_and_removes_a_merged_worktree_whose_lock_pid_is_dead(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        gone = subprocess.Popen(["true"])
        gone.wait()
        reason = f"claude agent agent-a (pid {gone.pid} start Wed Oct 29 15:23:52 2026)"
        self.git(self.main, "worktree", "lock", "--reason", reason, path)
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(path), report)
        self.assertIn(f"unlocked its stale lock: {reason}", report)

    def test_keeps_a_worktree_whose_lock_pid_is_too_large_to_check(self):
        # os.kill raises OverflowError past the C long range; the run must go on and keep it.
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        reason = "claude agent agent-a (pid 99999999999999999999 start x)"
        self.git(self.main, "worktree", "lock", "--reason", reason, path)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("agent-a: locked by live pid 99999999999999999999", report)
        self.assertIn("git worktree prune", report)  # the run finished

    def test_keeps_worktrees_on_next_and_main(self):
        # Release PRs have next as their head, so `next` shows up with a merged PR.
        self.git(self.main, "switch", "-q", "-c", "feat/elsewhere")
        on_next = os.path.join(self.main, ".claude", "worktrees", "agent-n")
        on_main = os.path.join(self.main, ".claude", "worktrees", "agent-m")
        self.git(self.main, "worktree", "add", "-q", on_next, "next")
        self.git(self.main, "worktree", "add", "-q", on_main, "main")
        self.git(self.main, "push", "-q", "origin", "main")
        head = self.git(self.main, "rev-parse", "next")
        self.pr(2, "next", "MERGED", head)
        self.pr(3, "main", "MERGED", head)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(on_next) and os.path.exists(on_main), report)
        self.assertIn("agent-n: it has next checked out", report)
        self.assertIn("agent-m: it has main checked out", report)
        self.assertLessEqual({"main", "next"}, self.branches())

    def test_keeps_a_worktree_with_ignored_files_other_than_target(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        os.makedirs(os.path.join(path, "target", "debug"))
        open(os.path.join(path, "target", "debug", "bin"), "w").close()
        with open(os.path.join(path, ".env"), "w") as f:
            f.write("TOKEN=x\n")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        # The ignored-files reason names .env alone: target/ is left out of it.
        self.assertIn("kept worktree .claude/worktrees/agent-a: ignored files that removal would "
                      "delete: .env\n", report)

    def test_names_a_worktree_relpath_cannot_reach_by_its_full_path(self):
        # On Windows, os.path.relpath raises ValueError for a path on another drive than the
        # main checkout; it is made to raise here so the case runs on every OS.
        merged = self.worktree("agent-a", "feat/x/a", "MERGED")
        dirty = self.worktree("agent-b", "feat/x/b", "OPEN", number=2)
        open(os.path.join(dirty, "scratch.txt"), "w").close()
        driver = (
            "import sys; sys.path.insert(0, sys.argv[1]); import post_merge_cleanup as p\n"
            "c = p.Cleanup(sys.argv[2], False); real = p.os.path.relpath\n"
            "def relpath(path, start=None):\n"
            "    if start == c.main:\n"
            "        raise ValueError('path is on mount D:, start on mount C:')\n"
            "    return real(path, start)\n"
            "p.os.path.relpath = relpath\n"
            "sys.exit(c.execute())\n"
        )
        out = subprocess.run(
            [sys.executable, "-B", "-c", driver, LIB, self.main],
            capture_output=True, text=True, env=self.env, check=False, timeout=120,
        )
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        self.assertFalse(os.path.exists(merged), out.stdout)
        self.assertIn(f"removed worktree {shown(merged)} (feat/x/a: PR #1 merged)", out.stdout)
        self.assertIn(f"kept worktree {shown(dirty)}: dirty", out.stdout)

    def test_removes_a_worktree_whose_only_ignored_files_are_target_dirs(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        for sub in ("target/debug", "crates/a/target/release"):
            os.makedirs(os.path.join(path, sub))
            open(os.path.join(path, sub, "bin"), "w").close()
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(path), report)

    def test_removes_a_detached_worktree_in_next_and_keeps_one_ahead(self):
        inside = os.path.join(self.main, ".claude", "worktrees", "agent-d")
        ahead = os.path.join(self.main, ".claude", "worktrees", "agent-e")
        self.git(self.main, "worktree", "add", "-q", "--detach", inside, "origin/next")
        self.git(self.main, "worktree", "add", "-q", "--detach", ahead, "origin/next")
        self.commit(ahead, "local.txt")
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(inside), report)
        self.assertIn("kept worktree .claude/worktrees/agent-e: detached HEAD is not in", report)

    def test_never_removes_the_worktree_it_started_in(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        report = self.run_cleanup(cwd=path)
        self.assertTrue(os.path.exists(path))
        self.assertIn("the cleanup was started from it", report)

    def test_keeps_everything_when_gh_fails(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        self.git(self.main, "branch", "feat/old", "origin/next")
        open(os.path.join(self.stub, "gh-fails"), "w").close()
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("feat/old", self.branches())
        self.assertIn("PR state unknown (gh failed: gh: could not reach api.github.com)", report)

    def test_dry_run_changes_nothing(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        gone = subprocess.Popen(["true"])
        gone.wait()
        self.git(self.main, "worktree", "lock", "--reason", f"claude agent (pid {gone.pid})", path)
        before = self.branches()
        report = self.run_cleanup("--dry-run")
        self.assertTrue(os.path.exists(path))
        self.assertEqual(self.branches(), before)
        self.assertIn("locked", self.git(self.main, "worktree", "list", "--porcelain"))
        self.assertIn("would remove worktree .claude/worktrees/agent-a", report)

    def test_keep_worktrees_leaves_a_merged_worktree(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        report = self.run_cleanup("--keep-worktrees")
        self.assertTrue(os.path.exists(path), report)
        self.assertTrue({"feat/x/a", "worktree-agent-a"} <= self.branches(), report)

    def test_a_second_run_finds_nothing_left(self):
        self.worktree("agent-a", "feat/x/a", "MERGED")
        self.run_cleanup()
        report = self.run_cleanup()
        self.assertNotIn("remove", report.replace("worktree prune", ""))


class WindowsPid(unittest.TestCase):
    def test_only_a_pid_that_names_no_process_counts_as_dead(self):
        # Win32 errors: 87 invalid parameter, 5 access denied, 8 not enough memory.
        self.assertFalse(post_merge_cleanup.open_failure_means_alive(87))
        for error in (5, 8, 0):
            self.assertTrue(post_merge_cleanup.open_failure_means_alive(error), error)


class RunLock(Repo):
    def test_a_run_waits_while_another_holds_the_lock(self):
        common = self.git(self.main, "rev-parse", "--path-format=absolute", "--git-common-dir")
        held = post_merge_cleanup.locked(common)
        try:
            run = subprocess.Popen(
                ["bash", SCRIPT, "--dry-run"], cwd=self.main, env=self.env, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            time.sleep(3)
            self.assertIsNone(run.poll(), "the run went ahead while the lock was held")
        finally:
            held.close()
        out, err = run.communicate(timeout=120)
        self.assertEqual(run.returncode, 0, err)
        self.assertIn("post-merge cleanup (dry run)", out)


class Processes(Repo):
    """A worktree something still runs from stays; nothing is killed."""

    def table(self, ps="", lsof=""):
        for name, text in (("ps.txt", ps), ("lsof.txt", lsof)):
            with open(os.path.join(self.stub, name), "w") as f:
                f.write(text)

    def test_keeps_a_worktree_running_an_executable_from_inside_it(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        exe = os.path.join(path, "target", "debug", "deps", "pi_layout-0123")
        self.table(ps=f"  411 /sbin/launchd\n 4242 {exe} --exact pi_layout\n")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("kept worktree .claude/worktrees/agent-a: 1 live processes (pids 4242)", report)

    @unittest.skipIf(os.name == "nt", "Windows has no documented way to read another process's "
                     "working directory, so the cleanup checks only executables there")
    def test_keeps_a_worktree_a_process_has_as_its_working_directory(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        self.table(ps=" 5150 /bin/zsh -l\n 5151 cargo test\n",
                   lsof=f"p5150\nfcwd\nn{path}\np5151\nfcwd\nn{path}/crates\n")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("2 live processes (pids 5150, 5151)", report)

    def test_a_sibling_path_with_the_same_prefix_does_not_count(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        self.table(ps=f" 7 {path}2/target/debug/x\n", lsof=f"p7\nfcwd\nn{path}-other\n")
        self.run_cleanup()
        self.assertFalse(os.path.exists(path))

    def test_keeps_every_worktree_when_ps_fails(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        open(os.path.join(self.stub, "ps-fails"), "w").close()
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn(
            f"could not list processes ({LISTER} failed: {LISTER}: operation not permitted)", report,
        )

    @unittest.skipIf(os.name == "nt", "Windows lists processes with PowerShell, not lsof")
    def test_without_lsof_every_worktree_stays(self):
        # cargo and rustc run from ~/.cargo, so the executable check alone misses a build.
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        open(os.path.join(self.stub, "lsof-missing"), "w").close()
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(path))
        self.assertIn("agent-a: could not list processes (lsof failed (exit 127): "
                      "lsof: command not found)", report)


def build_output(path):
    """A `target/` in `path` with two files and a hard link; returns the directory."""
    target = os.path.join(path, "target")
    os.makedirs(os.path.join(target, "debug", "deps"))
    with open(os.path.join(target, "debug", "deps", "lib-0123.rlib"), "wb") as f:
        f.write(b"x" * 100_000)
    os.link(os.path.join(target, "debug", "deps", "lib-0123.rlib"),
            os.path.join(target, "debug", "lib.rlib"))
    return target


def shown(path):
    """`path` as the report prints it, with `/` separators on every OS."""
    return os.path.normpath(path).replace(os.sep, "/")


# Windows checks only executables, so the cleanup keeps every target/ there.
CLEARS = unittest.skipIf(os.name == "nt", "no target/ is cleared on Windows")


class Targets(Repo):
    """The `target/` of a merged worktree goes in every mode; its checkout and the rest stay."""

    def outside(self, name):
        """A path for a worktree outside `.claude/worktrees/`, as T3 makes them."""
        return os.path.join(os.path.dirname(self.main), "t3", name)

    def table(self, ps="", lsof=""):
        for name, text in (("ps.txt", ps), ("lsof.txt", lsof)):
            with open(os.path.join(self.stub, name), "w") as f:
                f.write(text)

    @CLEARS
    def test_clears_the_target_of_a_merged_worktree_outside_claude_worktrees(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(target), report)
        self.assertTrue(os.path.exists(os.path.join(path, "t3-a.txt")), report)
        self.assertIn(f"cleared {shown(target)} (feat/x/t: PR #1 merged): ", report)
        self.assertRegex(report, r"target directories: cleared 1 \(0\.0 GiB\), kept 0")

    def test_keeps_the_target_of_a_worktree_with_an_open_pr(self):
        path = self.worktree("t3-a", "feat/x/t", "OPEN", path=self.outside("t3-a"))
        target = build_output(path)
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: feat/x/t: PR #1 is open", report)

    def test_keeps_the_target_of_a_worktree_with_no_pr(self):
        path = self.worktree("t3-a", "t3/work", path=self.outside("t3-a"))
        target = build_output(path)
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: t3/work: no PR", report)

    def test_keeps_the_target_of_a_worktree_a_process_runs_from(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        self.table(ps=f" 4242 {target}/debug/deps/pi_layout-0123 --exact pi_layout\n")
        report = self.run_cleanup()
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: 1 live processes (pids 4242)", report)

    @unittest.skipIf(os.name == "nt", "Windows lists no open files, only executables")
    def test_keeps_the_target_of_a_worktree_a_process_holds_a_file_in(self):
        # cargo run from elsewhere with --manifest-path: neither its executable nor its working
        # directory is in the worktree, only the files it has open.
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        self.table(ps=" 5150 /usr/bin/cargo build\n",
                   lsof=f"p5150\nn/home\nn{target}/debug/.cargo-lock\n")
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: 1 live processes (pids 5150)", report)

    def test_keeps_the_target_of_a_worktree_locked_by_a_live_pid(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        self.git(self.main, "worktree", "lock", "--reason", f"agent (pid {os.getpid()})", path)
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: locked by live pid {os.getpid()}", report)

    def test_never_touches_the_main_checkout(self):
        # Started from another worktree, with the main checkout on a merged branch, so only the
        # main-checkout rule keeps its target.
        other = self.worktree("t3-a", "feat/x/t", "OPEN", path=self.outside("t3-a"))
        self.git(self.main, "switch", "-q", "-c", "feat/merged-main")
        self.pr(2, "feat/merged-main", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        target = build_output(self.main)
        for args in ((), ("--keep-worktrees",), ("--targets-only",)):
            report = self.run_cleanup(*args, cwd=other)
            self.assertTrue(os.path.exists(target), report)
            self.assertIn("kept target: it is the main checkout", report)

    @CLEARS
    def test_keep_worktrees_keeps_the_checkout_and_clears_its_target(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        target = build_output(path)
        report = self.run_cleanup("--keep-worktrees")
        self.assertTrue(os.path.exists(os.path.join(path, "agent-a.txt")), report)
        self.assertFalse(os.path.exists(target), report)
        self.assertIn("kept worktree .claude/worktrees/agent-a: worktrees are removed only by "
                      "hand", report)
        self.assertIn("cleared .claude/worktrees/agent-a/target (feat/x/a: PR #1 merged)", report)

    @CLEARS
    def test_targets_only_touches_nothing_else(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        target = build_output(path)
        self.git(self.main, "branch", "feat/merged", "origin/next")
        self.pr(3, "feat/merged", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        before = self.branches()
        report = self.run_cleanup("--targets-only")
        self.assertFalse(os.path.exists(target), report)
        self.assertTrue(os.path.exists(path), report)
        self.assertEqual(self.branches(), before, report)
        self.assertNotIn("worktree prune", report)
        self.assertIn("post-merge cleanup (targets only)", report)

    def test_targets_only_refuses_a_pr(self):
        out = subprocess.run(["bash", SCRIPT, "--targets-only", "--pr", "7"], cwd=self.main,
                             capture_output=True, text=True, env=self.env, check=False)
        self.assertEqual(out.returncode, 2)
        self.assertIn("takes no --pr", out.stderr)

    @CLEARS
    def test_dry_run_reports_and_keeps_the_target(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        report = self.run_cleanup("--dry-run", "--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"would clear {shown(target)} (feat/x/t: PR #1 merged): ", report)
        self.assertIn("target directories: would clear 1", report)

    def test_a_removed_worktree_is_not_reported_twice(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED")
        build_output(path)
        report = self.run_cleanup()
        self.assertFalse(os.path.exists(path), report)
        self.assertNotIn("agent-a/target", report)

    @CLEARS
    def test_detached_head_at_a_merged_pr_head_is_cleared_and_others_kept(self):
        merged = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        head = self.git(merged, "rev-parse", "HEAD")
        self.git(merged, "switch", "-q", "--detach")
        loose = self.outside("t3-b")
        self.git(self.main, "worktree", "add", "-q", "--detach", loose, "origin/next")
        merged_target, loose_target = build_output(merged), build_output(loose)
        report = self.run_cleanup("--targets-only")
        self.assertFalse(os.path.exists(merged_target), report)
        self.assertIn(f"cleared {shown(merged_target)} (detached HEAD is feat/x/t's head: "
                      "PR #1 merged)", report)
        self.assertTrue(os.path.exists(loose_target), report)
        self.assertIn(f"kept {shown(loose_target)}: detached HEAD is no PR's head commit", report)
        self.assertEqual(self.git(merged, "rev-parse", "HEAD"), head)

    def test_keeps_the_target_of_a_worktree_on_next(self):
        self.git(self.main, "switch", "-q", "-c", "feat/elsewhere")
        on_next = self.outside("t3-n")
        self.git(self.main, "worktree", "add", "-q", on_next, "next")
        self.pr(2, "next", "MERGED", self.git(self.main, "rev-parse", "next"))
        target = build_output(on_next)
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target), report)
        self.assertIn(f"kept {shown(target)}: it has next checked out", report)

    @unittest.skipIf(os.name == "nt", "Windows lists processes with PowerShell, not lsof")
    def test_keeps_every_target_when_lsof_is_missing(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        open(os.path.join(self.stub, "lsof-missing"), "w").close()
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: could not list processes (lsof failed (exit 127): "
                      "lsof: command not found)", report)

    @unittest.skipUnless(os.name == "nt", "Windows only")
    def test_keeps_every_target_on_windows(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target))
        self.assertIn(f"kept {shown(target)}: Windows lists only executables", report)

    def test_keeps_every_target_when_gh_fails(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        detached = self.outside("t3-d")
        self.git(self.main, "worktree", "add", "-q", "--detach", detached, "origin/next")
        targets = build_output(path), build_output(detached)
        open(os.path.join(self.stub, "gh-fails"), "w").close()
        report = self.run_cleanup("--targets-only")
        for target in targets:
            self.assertTrue(os.path.exists(target), report)
        self.assertIn(f"kept {shown(targets[0])}: feat/x/t: PR state unknown (gh failed: "
                      "gh: could not reach api.github.com)", report)
        self.assertIn(f"kept {shown(targets[1])}: PR state unknown (gh failed: ", report)

    def test_keeps_a_detached_head_whose_pr_branch_has_an_open_pr(self):
        # The closed PR's head is checked out detached, but its branch was reopened as PR #2.
        path = self.worktree("t3-a", "feat/x/t", "CLOSED", path=self.outside("t3-a"))
        head = self.git(path, "rev-parse", "HEAD")
        self.git(path, "switch", "-q", "--detach")
        self.pr(2, "feat/x/t", "OPEN", head)
        target = build_output(path)
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.exists(target), report)
        self.assertIn(f"kept {shown(target)}: detached HEAD is feat/x/t's head: "
                      "PR #2 is open", report)

    @unittest.skipIf(os.name == "nt", "a symlink needs developer mode on Windows")
    def test_keeps_a_target_that_is_a_symlink(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        elsewhere = build_output(os.path.join(os.path.dirname(self.main), "shared"))
        os.symlink(elsewhere, os.path.join(path, "target"))
        report = self.run_cleanup("--targets-only")
        self.assertTrue(os.path.islink(os.path.join(path, "target")), report)
        self.assertTrue(os.path.exists(os.path.join(elsewhere, "debug", "lib.rlib")), report)
        self.assertIn(f"kept {shown(path)}/target: target is a symlink", report)

    @unittest.skipIf(os.name == "nt" or os.geteuid() == 0,
                     "needs POSIX permissions that bind the user running the test")
    def test_a_failed_delete_counts_as_kept(self):
        path = self.worktree("t3-a", "feat/x/t", "MERGED", path=self.outside("t3-a"))
        target = build_output(path)
        locked = os.path.join(target, "debug")
        os.chmod(locked, 0o500)  # its files cannot be unlinked
        try:
            report = self.run_cleanup("--targets-only")
        finally:
            os.chmod(locked, 0o700)
        self.assertTrue(os.path.exists(target), report)
        self.assertIn(f"failed to clear {shown(target)} (feat/x/t: PR #1 merged): ", report)
        self.assertIn("target directories: cleared 0 (0.0 GiB), kept 1", report)

    def test_reports_free_space_on_the_repository_filesystem(self):
        report = self.run_cleanup("--targets-only")
        self.assertRegex(report,
                         rf"disk: {re.escape(shown(self.main))} filesystem: [\d.]+ GiB free of ")


class DiskReport(unittest.TestCase):
    def test_warns_under_ten_percent_free(self):
        line = post_merge_cleanup.disk_line
        self.assertEqual(line("C:", 54 * 2**30, 1862 * 2**30),
                         "disk: C:: 54.0 GiB free of 1862.0 GiB (3%); WARNING: under 10% free")
        self.assertNotIn("WARNING", line("/", 10 * 2**30, 100 * 2**30))
        self.assertIn("WARNING", line("/", 10 * 2**30 - 1, 100 * 2**30))

    def test_finds_the_windows_drives_that_hold_a_wsl_virtual_disk(self):
        with tempfile.TemporaryDirectory() as root:
            def disk(*parts):
                os.makedirs(os.path.join(root, *parts[:-1]), exist_ok=True)
                open(os.path.join(root, *parts), "w").close()

            self.assertEqual(post_merge_cleanup.wsl_disk_mounts(root), [])
            disk("d", "Users", "u", "AppData", "Local", "Packages", "Canonical", "LocalState",
                 "ext4.vhdx")
            self.assertEqual(post_merge_cleanup.wsl_disk_mounts(root), [os.path.join(root, "d")])
            disk("c", "Users", "u", "AppData", "Local", "wsl", "{0a}", "ext4.vhdx")
            disk("l", "Users", "v", "AppData", "Local", "wsl", "{0b}", "ext4.vhdx")
            os.makedirs(os.path.join(root, "wslg", "Users"))
            self.assertEqual(post_merge_cleanup.wsl_disk_mounts(root),
                             [os.path.join(root, "c"), os.path.join(root, "l")])

    def test_counts_a_hard_linked_file_once(self):
        with tempfile.TemporaryDirectory() as root:
            target = build_output(root)
            one = os.lstat(os.path.join(target, "debug", "lib.rlib"))
            size = one.st_blocks * 512 if hasattr(one, "st_blocks") else one.st_size
            self.assertEqual(post_merge_cleanup.disk_bytes(target), size)


class Branches(Repo):
    def test_deletes_merged_branches_and_keeps_the_rest(self):
        self.git(self.main, "branch", "feat/merged", "origin/next")
        self.pr(3, "feat/merged", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        self.git(self.main, "branch", "feat/open", "origin/next")
        self.pr(4, "feat/open", "OPEN", self.git(self.main, "rev-parse", "HEAD"))
        self.git(self.main, "branch", "feat/no-pr", "origin/next")
        self.git(self.main, "switch", "-q", "-c", "feat/unpushed")
        self.pr(5, "feat/unpushed", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        self.commit(self.main, "local.txt")
        self.git(self.main, "switch", "-q", "next")
        self.pr(6, "main", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        report = self.run_cleanup()
        self.assertEqual(
            self.branches(), {"main", "next", "feat/open", "feat/no-pr", "feat/unpushed"}, report,
        )
        self.assertIn("kept branch feat/unpushed: unpushed commits", report)

    def test_keeps_the_current_branch(self):
        self.git(self.main, "switch", "-q", "-c", "feat/merged")
        self.pr(3, "feat/merged", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        report = self.run_cleanup()
        self.assertIn("feat/merged", self.branches())
        self.assertIn("kept branch feat/merged: checked out where the cleanup started", report)


    def test_delete_branch_refuses_main_and_next(self):
        self.git(self.main, "switch", "-q", "-c", "feat/elsewhere")
        driver = (
            "import sys; sys.path.insert(0, sys.argv[1]); import post_merge_cleanup as p\n"
            "c = p.Cleanup(sys.argv[2], False)\n"
            "print(c.delete_branch('next'), c.delete_branch('main'))\n"
        )
        out = subprocess.run(
            [sys.executable, "-B", "-c", driver, LIB, self.main],
            capture_output=True, text=True, env=self.env, check=False,
        )
        self.assertIn("False False", out.stdout, out.stderr)
        self.assertLessEqual({"main", "next"}, self.branches())


class MergeMatcher(unittest.TestCase):
    def test_fires_on_gh_pr_merge(self):
        cases = {
            "gh pr merge 12 --squash --delete-branch": [("12", "", None)],
            "gh pr merge --squash": [("", "", None)],
            "cd x && gh pr merge -t 'a b' 7": [("7", "", "x")],
            "gh pr merge 9 -R example-org/stepclimb --squash": [
                ("9", "example-org/stepclimb", None),
            ],
            "GH_PAGER= /opt/bin/gh pr merge https://github.com/o/r/pull/3": [
                ("https://github.com/o/r/pull/3", "", None),
            ],
            # Separators inside quotes belong to the word, not the command line.
            "gh pr merge 9 -t 'fix; a && b | c' --squash": [("9", "", None)],
            "git status\ngh pr merge 3": [("3", "", None)],
            "bash -c 'gh pr merge 5 --squash'": [("5", "", None)],
            "time gh pr merge 6": [("6", "", None)],
            "echo $(gh pr merge 7)": [("7", "", None)],
            'out="$(gh pr merge 8 --squash)"': [("8", "", None)],
            "(gh pr merge 4)": [("4", "", None)],
            "cd .. ; cd repo && gh pr merge": [("", "", "repo")],
        }
        for command, expected in cases.items():
            self.assertEqual(post_merge_cleanup.merge_invocations(command), expected, command)

    def test_does_not_fire_on_other_commands(self):
        for command in (
            "gh pr view 12", "gh pr list --state merged", "gh pr create --base next",
            "echo gh pr merge 12", "git merge next", "gh pr", "rg 'gh pr merge' .claude",
            "echo 'x; gh pr merge 1'", "git commit -m 'run gh pr merge later'",
        ):
            self.assertEqual(post_merge_cleanup.merge_invocations(command), [], command)


class Hook(Repo):
    def hook(self, command, cwd=None):
        payload = {"cwd": cwd or self.main, "tool_name": "Bash",
                   "tool_input": {"command": command}, "hook_event_name": "PostToolUse"}
        out = subprocess.run(
            ["bash", os.path.join(HOOKS, "post-merge-cleanup.sh")], input=json.dumps(payload),
            capture_output=True, text=True, env=self.env, cwd=cwd or self.main, check=False,
            timeout=30,
        )
        self.assertEqual(out.returncode, 0, out.stderr)
        return out.stdout

    def log(self):
        return os.path.join(self.main, ".git", post_merge_cleanup.LOG_NAME)

    def wait_for_report(self, runs=1):
        """The log once `runs` background runs have finished (each ends with the prune line)."""
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if os.path.exists(self.log()):
                with open(self.log()) as f:
                    text = f.read()
                if text.count("git worktree prune") >= runs:
                    return text
            time.sleep(0.1)
        self.fail("the background cleanup did not finish within 60 s")

    def test_a_merge_cleans_branches_in_the_background_and_keeps_every_worktree(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        self.git(self.main, "branch", "feat/merged", "origin/next")
        self.pr(3, "feat/merged", "MERGED", self.git(self.main, "rev-parse", "HEAD"))
        out = self.hook("gh pr merge 7 --squash")
        context = json.loads(out)["hookSpecificOutput"]["additionalContext"]
        self.assertIn(self.log(), context)
        report = self.wait_for_report()
        self.assertTrue(os.path.exists(path), report)
        self.assertIn("kept worktree .claude/worktrees/agent-a: worktrees are removed only by "
                      "hand (/cleanup)", report)
        self.assertTrue({"feat/x/a", "worktree-agent-a"} <= self.branches(), report)
        self.assertNotIn("feat/merged", self.branches(), report)
        self.assertIn("deleted remote branch origin/feat/x/a (PR #7 merged)", report)
        self.assertEqual(self.git(self.main, "ls-remote", "origin", "refs/heads/feat/x/a"), "")

    def test_leaves_a_remote_branch_that_moved_or_whose_pr_is_not_merged(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        self.commit(path, "later.txt")
        self.git(path, "push", "-q", "origin", "feat/x/a")
        self.worktree("agent-b", "feat/x/b", "OPEN", number=8)
        self.hook("gh pr merge 7 --squash; gh pr merge 8 --auto")
        text = self.wait_for_report(runs=2)
        self.assertIn("remote branch feat/x/a left: it has moved past PR #7's head", text)
        self.assertIn("remote branch left: PR #8 is open, not merged", text)
        self.assertIn("feat/x/b", self.git(self.main, "ls-remote", "origin"))

    def test_other_commands_start_nothing(self):
        self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        self.assertEqual(self.hook("gh pr view 7"), "")
        self.assertFalse(os.path.exists(self.log()))

    def test_bad_input_is_reported_not_fatal(self):
        out = subprocess.run(
            ["bash", os.path.join(HOOKS, "post-merge-cleanup.sh")], input="not json merge",
            capture_output=True, text=True, env=self.env, check=False,
        )
        self.assertEqual(out.returncode, 0)
        self.assertIn("did not start", json.loads(out.stdout)["hookSpecificOutput"]["additionalContext"])

    def test_a_command_without_merge_never_starts_python(self):
        marker = os.path.join(self.stub, "python-ran")
        with open(os.path.join(self.stub, "python3"), "w") as f:
            f.write(f"#!/bin/sh\ntouch '{marker}'\n")
        os.chmod(os.path.join(self.stub, "python3"), 0o755)
        self.hook("ls -la")
        self.assertFalse(os.path.exists(marker))
        self.hook("gh pr merge 7")
        self.assertTrue(os.path.exists(marker))

    def test_outside_a_checkout_it_writes_no_log(self):
        outside = os.path.join(os.path.dirname(self.main), "plain")
        os.mkdir(outside)
        self.assertEqual(self.hook("gh pr merge 7", cwd=outside), "")
        self.assertEqual(os.listdir(outside), [])

    def test_a_merge_after_cd_resolves_the_pr_there(self):
        path = self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        self.hook("cd .claude/worktrees/agent-a && gh pr merge --squash")
        report = self.wait_for_report()
        self.assertIn("deleted remote branch origin/feat/x/a (PR #7 merged)", report)
        self.assertTrue(os.path.exists(path), report)

    def test_a_merge_after_an_unresolvable_cd_leaves_the_remote_branch(self):
        self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        self.hook("cd $SOMEWHERE && gh pr merge 7 --squash")
        report = self.wait_for_report()
        self.assertIn("`cd $SOMEWHERE`, so which PR it merged is not known", report)
        self.assertIn("feat/x/a", self.git(self.main, "ls-remote", "origin"))

    def test_the_remote_delete_fails_if_the_branch_moved_after_it_was_read(self):
        # ls-remote is made to report the PR head while origin has moved on: the lease must
        # refuse the delete.
        path = self.worktree("agent-a", "feat/x/a", "MERGED", number=7)
        pr_head = self.git(path, "rev-parse", "HEAD")
        self.commit(path, "later.txt")
        self.git(path, "push", "-q", "origin", "feat/x/a")
        moved = self.git(path, "rev-parse", "HEAD")
        driver = (
            "import sys; sys.path.insert(0, sys.argv[1]); import post_merge_cleanup as p\n"
            "c = p.Cleanup(sys.argv[2], False); real = c.git\n"
            "def git(*a, **k):\n"
            "    if a[0] == 'ls-remote':\n"
            "        return p.subprocess.CompletedProcess(a, 0, sys.argv[3] + '\\tref\\n', '')\n"
            "    return real(*a, **k)\n"
            "c.git = git; c.delete_origin_ref('feat/x/a', sys.argv[3], 7)\n"
        )
        out = subprocess.run(
            [sys.executable, "-B", "-c", driver, LIB, self.main, pr_head],
            capture_output=True, text=True, env=self.env, check=False,
        )
        self.assertIn("failed to delete", out.stdout, out.stderr)
        self.assertIn(moved, self.git(self.main, "ls-remote", "origin", "refs/heads/feat/x/a"))


class Registration(unittest.TestCase):
    def test_claude_runs_it_after_bash(self):
        with open(os.path.join(ROOT, ".claude", "settings.json"), encoding="utf-8") as f:
            entries = json.load(f)["hooks"]["PostToolUse"]
        found = [e["matcher"] for e in entries
                 if any("post-merge-cleanup.sh" in h["command"] for h in e["hooks"])]
        self.assertEqual(found, ["Bash"])

    def test_codex_runs_it_after_bash(self):
        with open(os.path.join(ROOT, ".codex", "config.toml"), encoding="utf-8") as f:
            config = f.read()
        head = config[:config.index("post-merge-cleanup.sh")]
        events = re.findall(r"^\[\[hooks\.(\w+)\]\]$", head, re.M)
        self.assertEqual(events[-1], "PostToolUse")
        self.assertIn('matcher = "^Bash$"', head[head.rindex("[[hooks.PostToolUse]]"):])

    def test_cleanup_command_runs_the_script(self):
        with open(os.path.join(ROOT, ".claude", "commands", "cleanup.md"), encoding="utf-8") as f:
            self.assertIn(".claude/scripts/post-merge-cleanup.sh", f.read())


if __name__ == "__main__":
    unittest.main()
