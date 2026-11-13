# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `just clean-stale` (clean_stale.py). Run with `just agents-check`.

Each test builds a throwaway repository with worktrees (the post-merge cleanup tests' `Repo`),
lays out a fake cargo `target/` with file times set back, and runs the script with `ps` and
`lsof` stubs on PATH.
"""

import os
import subprocess
import sys
import time
import unittest
from unittest import mock

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import clean_stale  # noqa: E402
from test_post_merge_cleanup import Repo  # noqa: E402

# Cargo's lock files in a profile directory, spelled out so a change to the script's list shows.
CARGO_LOCKS = (".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")
SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "clean_stale.py")
OLD = time.time() - 10 * clean_stale.DAY
HASH_OLD = "0123456789abcdef"
HASH_NEW = "fedcba9876543210"
# Real-shaped incremental names: a crate directory, sessions, and the lock rustc gives a session
# (its name without the last `-` part).
INC_OLD = "old_crate-1abc2def3ghi4"
INC_LIVE = "live_crate-3qyk4rvk3pb4m"
SESSION_OLD = "s-ho0fn2ij0n-115nhot-7k9vqa1yh3777bkzw6qxs5d3r"
SESSION_OLD_LOCK = "s-ho0fn2ij0n-115nhot.lock"
SESSION_NEW = "s-ho0gpa2l9d-035q4fk-5aa8fmo2wecqg8lpxjj78u4d4"
SESSION_NEW_LOCK = "s-ho0gpa2l9d-035q4fk.lock"


def touch(path, when=None, body=b"x" * 4096):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(body)
    if when is not None:
        os.utime(path, (when, when))


def age_tree(path, when):
    """Set every file and directory under `path` to `when`, deepest first."""
    for root, dirs, files in os.walk(path, topdown=False):
        for name in dirs + files:
            os.utime(os.path.join(root, name), (when, when))
    os.utime(path, (when, when))


def fake_target(worktree):
    """A `target/debug` with one old unit, one fresh unit, incremental crates and a stale tmp."""
    debug = os.path.join(worktree, "target", "debug")
    for name in CARGO_LOCKS:
        touch(os.path.join(debug, name), OLD, b"")
    for h, when in ((HASH_OLD, OLD), (HASH_NEW, None)):
        touch(os.path.join(debug, ".fingerprint", f"foo-{h}", "lib-foo"), when)
        touch(os.path.join(debug, "build", f"foo-{h}", "out", "gen.rs"), when)
        touch(os.path.join(debug, "deps", f"libfoo-{h}.rlib"), when)
        touch(os.path.join(debug, "deps", f"foo-{h}.d"), when)
        touch(os.path.join(debug, "deps", f"foo_test-{h}"), when)
    touch(os.path.join(debug, "foo"), OLD)  # an uplifted copy
    inc = os.path.join(debug, "incremental")
    touch(os.path.join(inc, INC_OLD, SESSION_OLD, "query-cache.bin"), OLD)
    touch(os.path.join(inc, INC_OLD, SESSION_OLD_LOCK), OLD, b"")
    touch(os.path.join(inc, INC_LIVE, SESSION_OLD, "query-cache.bin"), OLD)
    touch(os.path.join(inc, INC_LIVE, SESSION_OLD_LOCK), OLD, b"")
    touch(os.path.join(inc, INC_LIVE, SESSION_NEW, "query-cache.bin"))
    touch(os.path.join(inc, INC_LIVE, SESSION_NEW_LOCK), None, b"")
    for h in (HASH_OLD,):
        age_tree(os.path.join(debug, ".fingerprint", f"foo-{h}"), OLD)
        age_tree(os.path.join(debug, "build", f"foo-{h}"), OLD)
    age_tree(os.path.join(inc, INC_OLD), OLD)
    age_tree(os.path.join(inc, INC_LIVE, SESSION_OLD), OLD)
    touch(os.path.join(worktree, "target", "tmp", "scratch", "f"), OLD)
    age_tree(os.path.join(worktree, "target", "tmp", "scratch"), OLD)
    touch(os.path.join(worktree, "target", "nextest-archive.tar.zst"), OLD)
    return debug


class CleanStale(Repo):

    def run_stale(self, *args, cwd=None, code=0):
        out = subprocess.run(
            [sys.executable, "-X", "utf8", "-B", SCRIPT, *args], cwd=cwd or self.main,
            capture_output=True, text=True, env=self.env, check=False, timeout=120,
        )
        self.assertEqual(out.returncode, code, out.stdout + out.stderr)
        return out.stdout

    def table(self, ps="", lsof=""):
        for name, text in (("ps.txt", ps), ("lsof.txt", lsof)):
            with open(os.path.join(self.stub, name), "w") as f:
                f.write(text)

    def ex(self, path):
        return os.path.exists(path)

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_deletes_stale_units_and_keeps_fresh_ones(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        report = self.run_stale()
        for gone in (f".fingerprint/foo-{HASH_OLD}", f"build/foo-{HASH_OLD}",
                     f"deps/libfoo-{HASH_OLD}.rlib", f"deps/foo-{HASH_OLD}.d",
                     f"deps/foo_test-{HASH_OLD}", "foo", f"incremental/{INC_OLD}",
                     f"incremental/{INC_LIVE}/{SESSION_OLD}",
                     f"incremental/{INC_LIVE}/{SESSION_OLD_LOCK}"):
            self.assertFalse(self.ex(os.path.join(debug, gone)), gone)
        for kept in (f".fingerprint/foo-{HASH_NEW}", f"build/foo-{HASH_NEW}",
                     f"deps/libfoo-{HASH_NEW}.rlib", f"deps/foo_test-{HASH_NEW}",
                     f"incremental/{INC_LIVE}/{SESSION_NEW}",
                     f"incremental/{INC_LIVE}/{SESSION_NEW_LOCK}", *CARGO_LOCKS):
            self.assertTrue(self.ex(os.path.join(debug, kept)), kept)
        self.assertFalse(self.ex(os.path.join(wt, "target", "tmp", "scratch")))
        self.assertFalse(self.ex(os.path.join(wt, "target", "nextest-archive.tar.zst")))
        self.assertIn("deleted 11 stale paths in .claude/worktrees/agent-a/target", report)
        self.assertIn("target directories: freed", report)

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_a_unit_read_recently_stays(self):
        """A unit built long ago but whose fingerprint a build just read is in use."""
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        fp = os.path.join(debug, ".fingerprint", f"foo-{HASH_OLD}", "lib-foo")
        os.utime(fp, (time.time(), OLD))
        self.run_stale()
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertTrue(self.ex(fp))

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_dry_run_deletes_nothing(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        report = self.run_stale("--dry-run")
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertTrue(self.ex(os.path.join(debug, "incremental", INC_OLD)))
        self.assertIn("would delete 11 stale paths", report)
        self.assertIn("target directories: would free", report)

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_days_moves_the_cutoff(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        self.run_stale("--days", "30")
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_a_target_a_process_uses_stays_whole(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        self.table(ps=" 5150 cargo nextest run\n",
                   lsof=f"p5150\nn/home\nn{debug}/.cargo-lock\n")
        report = self.run_stale()
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertIn("kept .claude/worktrees/agent-a/target: 1 live processes use it "
                      "(pids 5150)", report)

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_without_lsof_every_target_stays(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        open(os.path.join(self.stub, "lsof-missing"), "w").close()
        report = self.run_stale()
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertIn("could not list processes (lsof failed (exit 127)", report)

    @unittest.skipIf(os.name == "nt", "flock is POSIX; every target/ stays on Windows")
    def test_a_profile_whose_cargo_lock_is_held_stays(self):
        import fcntl
        for i, lock in enumerate(CARGO_LOCKS):
            with self.subTest(lock=lock):
                wt = self.worktree(f"agent-{i}", f"feat/{i}")
                debug = fake_target(wt)
                fd = os.open(os.path.join(debug, lock), os.O_RDWR)
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX)
                    report = self.run_stale()
                finally:
                    os.close(fd)
                self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
                self.assertIn(f"kept .claude/worktrees/agent-{i}/target/debug: a build holds "
                              f"{lock}", report)

    @unittest.skipIf(os.name == "nt", "flock is POSIX; every target/ stays on Windows")
    def test_cargo_locks_are_held_while_deleting(self):
        """A build that starts during the run must wait: every lock is held at each delete."""
        import fcntl
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        real_delete, free, probed = clean_stale.delete, [], []

        def probe(path):
            if path.startswith(debug + os.sep):
                probed.append(path)
                for lock in CARGO_LOCKS:
                    fd = os.open(os.path.join(debug, lock), os.O_RDWR)
                    try:
                        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        free.append(lock)
                    except OSError:
                        pass
                    finally:
                        os.close(fd)
            real_delete(path)

        with mock.patch.dict(os.environ, self.env, clear=True), \
                mock.patch.object(clean_stale, "delete", probe), \
                mock.patch("sys.stdout"):
            code = clean_stale.cli(["--cwd", self.main])
        self.assertEqual(code, 0)
        self.assertFalse(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertGreater(len(probed), 0)
        self.assertEqual(free, [])

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_a_unit_is_judged_whole(self):
        """An old fingerprint stays while a file of its unit is fresh."""
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        touch(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rmeta"))
        self.run_stale()
        for kept in (f".fingerprint/foo-{HASH_OLD}", f"build/foo-{HASH_OLD}",
                     f"deps/libfoo-{HASH_OLD}.rlib", f"deps/foo_test-{HASH_OLD}"):
            self.assertTrue(self.ex(os.path.join(debug, kept)), kept)

    @unittest.skipIf(os.name == "nt" or os.geteuid() == 0,
                     "POSIX directory modes; root deletes regardless")
    def test_a_failed_fingerprint_delete_keeps_the_unit_and_fails_the_run(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        fingerprints = os.path.join(debug, ".fingerprint")
        os.chmod(fingerprints, 0o555)
        try:
            report = self.run_stale(code=1)
        finally:
            os.chmod(fingerprints, 0o755)
        for kept in (f".fingerprint/foo-{HASH_OLD}", f"build/foo-{HASH_OLD}",
                     f"deps/libfoo-{HASH_OLD}.rlib"):
            self.assertTrue(self.ex(os.path.join(debug, kept)), kept)
        self.assertFalse(self.ex(os.path.join(debug, "incremental", INC_OLD)))
        self.assertIn("failed to delete .claude/worktrees/agent-a/target/debug/.fingerprint/"
                      f"foo-{HASH_OLD}", report)
        self.assertIn("clean-stale: 1 deletes failed", report)

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_the_main_checkout_and_outside_worktrees_are_cleaned(self):
        debug = fake_target(self.main)
        outside = self.worktree("t3", "feat/t3", path=os.path.join(self.tmp.name, "t3"))
        debug_t3 = fake_target(outside)
        self.run_stale()
        for d in (debug, debug_t3):
            self.assertFalse(self.ex(os.path.join(d, "deps", f"libfoo-{HASH_OLD}.rlib")))
            self.assertTrue(self.ex(os.path.join(d, "deps", f"libfoo-{HASH_NEW}.rlib")))

    @unittest.skipIf(os.name == "nt", "every target/ stays on Windows")
    def test_a_symlinked_target_stays(self):
        wt = self.worktree("agent-a", "feat/a")
        real = os.path.join(self.tmp.name, "elsewhere")
        fake_target(real)
        os.symlink(os.path.join(real, "target"), os.path.join(wt, "target"))
        report = self.run_stale()
        self.assertIn("kept .claude/worktrees/agent-a/target: target is a symlink", report)
        self.assertTrue(self.ex(os.path.join(real, "target", "debug", "deps",
                                             f"libfoo-{HASH_OLD}.rlib")))

    @unittest.skipUnless(os.name == "nt", "Windows lists executables only")
    def test_on_windows_every_target_stays(self):
        wt = self.worktree("agent-a", "feat/a")
        debug = fake_target(wt)
        report = self.run_stale()
        self.assertTrue(self.ex(os.path.join(debug, "deps", f"libfoo-{HASH_OLD}.rlib")))
        self.assertIn("no target/ is cleaned there", report)

    def scratch(self):
        root = os.path.join(self.tmp.name, "scratch")
        touch(os.path.join(root, "old-agent", "target", "big"), OLD)
        age_tree(os.path.join(root, "old-agent"), OLD)
        touch(os.path.join(root, "new-agent", "pr-body.md"))
        touch(os.path.join(root, "loose-note.md"), OLD)
        return root

    @unittest.skipIf(os.name == "nt", "every scratch folder stays on Windows")
    def test_scratch_is_listed_but_not_deleted_by_default(self):
        root = self.scratch()
        report = self.run_stale("--scratch", "--scratch-dir", root)
        self.assertTrue(self.ex(os.path.join(root, "old-agent")))
        old = clean_stale.pmc.slashed(os.path.join(root, "old-agent"))
        self.assertIn(f"scratch: would delete {old}", report)
        self.assertIn("(--delete-scratch deletes them)", report)
        self.assertNotIn("new-agent", report)

    @unittest.skipIf(os.name == "nt", "process checks there list executables only")
    def test_delete_scratch_deletes_stale_subdirectories_only(self):
        root = self.scratch()
        self.run_stale("--scratch", "--delete-scratch", "--scratch-dir", root)
        self.assertFalse(self.ex(os.path.join(root, "old-agent")))
        self.assertTrue(self.ex(os.path.join(root, "new-agent")))
        self.assertTrue(self.ex(os.path.join(root, "loose-note.md")))

    @unittest.skipIf(os.name == "nt", "process checks there list executables only")
    def test_a_scratch_directory_in_use_stays(self):
        root = self.scratch()
        old = os.path.join(root, "old-agent")
        self.table(ps=" 7 bash\n", lsof=f"p7\nfcwd\nn{old}\n")
        report = self.run_stale("--scratch", "--delete-scratch", "--scratch-dir", root)
        self.assertTrue(self.ex(old))
        shown = clean_stale.pmc.slashed(old)
        self.assertIn(f"scratch: kept {shown}: live processes (pids 7)", report)

    @unittest.skipIf(os.name == "nt", "every scratch folder stays on Windows")
    def test_a_stale_worktree_under_scratch_stays(self):
        root = self.scratch()
        holder = os.path.join(root, "measure")
        wt = self.worktree("measure", "feat/m", path=os.path.join(holder, "wt"))
        touch(os.path.join(wt, "uncommitted.txt"))
        age_tree(holder, OLD)
        report = self.run_stale("--scratch", "--delete-scratch", "--scratch-dir", root)
        self.assertTrue(self.ex(os.path.join(wt, "uncommitted.txt")))
        shown = clean_stale.pmc.slashed(os.path.realpath(wt))
        self.assertIn(f"scratch: kept {clean_stale.pmc.slashed(holder)}: holds the worktree "
                      f"{shown}", report)
        self.assertFalse(self.ex(os.path.join(root, "old-agent")))

    @unittest.skipIf(os.name == "nt", "every scratch folder stays on Windows")
    def test_a_stale_git_checkout_under_scratch_stays(self):
        root = self.scratch()
        clone = os.path.join(root, "spike", "repo")
        self.git(root, "init", "-q", clone)
        age_tree(os.path.join(root, "spike"), OLD)
        report = self.run_stale("--scratch", "--delete-scratch", "--scratch-dir", root)
        self.assertTrue(self.ex(os.path.join(clone, ".git")))
        self.assertIn(f"scratch: kept {clean_stale.pmc.slashed(os.path.join(root, 'spike'))}: "
                      "holds a git checkout", report)

    @unittest.skipUnless(os.name == "nt", "Windows lists executables only")
    def test_on_windows_every_scratch_folder_stays(self):
        root = self.scratch()
        report = self.run_stale("--scratch", "--delete-scratch", "--scratch-dir", root)
        self.assertTrue(self.ex(os.path.join(root, "old-agent")))
        self.assertIn("no scratch folder is deleted there", report)

    def test_rejects_days_under_two_and_delete_scratch_alone(self):
        for args in (["--days", "1"], ["--delete-scratch"]):
            out = subprocess.run([sys.executable, "-B", SCRIPT, *args], cwd=self.main,
                                 capture_output=True, text=True, env=self.env, check=False)
            self.assertEqual(out.returncode, 2, args)


if __name__ == "__main__":
    unittest.main()
