# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of bin/ci_gate.py: the slot semaphore, the legacy `flock` lock, names and teardown."""

from __future__ import annotations

import fcntl
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ci_gate  # noqa: E402


class SlotCountTest(unittest.TestCase):
    def test_default_is_three(self) -> None:
        self.assertEqual(ci_gate.slot_count({}), 3)

    def test_env_overrides(self) -> None:
        self.assertEqual(ci_gate.slot_count({"SC_CI_SLOTS": "4"}), 4)

    def test_rejects_zero_and_words(self) -> None:
        for raw in ("0", "-1", "three", ""):
            with self.subTest(raw=raw), self.assertRaises(ci_gate.GateError):
                ci_gate.slot_count({"SC_CI_SLOTS": raw})


class NamesTest(unittest.TestCase):
    def test_id_is_stable_per_path_and_differs_between_paths(self) -> None:
        a, b = Path("/work/stepclimb"), Path("/work/stepclimb/.claude/worktrees/x")
        self.assertEqual(ci_gate.run_id(a), ci_gate.run_id(a))
        self.assertNotEqual(ci_gate.run_id(a), ci_gate.run_id(b))
        self.assertRegex(ci_gate.run_id(a), r"^[0-9a-f]{8}$")

    def test_projects_and_image_carry_the_id(self) -> None:
        env = ci_gate.run_env(Path("/work/stepclimb"), {"PATH": "/bin"})
        ident = env["SC_CI_ID"]
        self.assertEqual(env["SC_CI_PROJECT"], f"stepclimb-ci-{ident}")
        self.assertEqual(env["SC_PROD_CHECK_PROJECT"], f"stepclimb-prodcheck-{ident}")
        self.assertEqual(env["SC_IMAGE"], f"stepclimb:ci-{ident}")
        self.assertEqual(env["PATH"], "/bin")

    def test_compose_uses_the_project_and_both_files(self) -> None:
        command = ci_gate.compose_command(Path("/r"), "stepclimb-ci-1", "up")
        self.assertEqual(
            command,
            [
                "docker",
                "compose",
                "-p",
                "stepclimb-ci-1",
                "-f",
                "/r/deploy/compose/compose.yaml",
                "-f",
                "/r/deploy/compose/ci.yaml",
                "up",
            ],
        )


class LockTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        self.fds: list[int] = []
        self.addCleanup(lambda: [os.close(fd) for fd in self.fds])

    def hold(self, name: str, operation: int = fcntl.LOCK_EX) -> int:
        fd = ci_gate.try_lock(self.dir / name, operation)
        assert fd is not None, f"{name} was already held"
        self.fds.append(fd)
        return fd


class SlotTest(LockTestCase):
    def test_takes_a_free_slot_without_waiting(self) -> None:
        self.hold("stepclimb-ci-slot.1")
        self.hold("stepclimb-ci-slot.2")
        said: list[str] = []
        slot, fd = ci_gate.acquire_slot(self.dir, 3, said.append, sleep=self.fail)
        self.fds.append(fd)
        self.assertEqual((slot, said), (3, []))

    def test_waits_while_every_slot_is_held(self) -> None:
        first = self.hold("stepclimb-ci-slot.1")
        self.hold("stepclimb-ci-slot.2")
        said: list[str] = []
        sleeps: list[float] = []

        def sleep(seconds: float) -> None:
            sleeps.append(seconds)
            if len(sleeps) == 2:  # the first slot's run ends during the second wait
                self.fds.remove(first)
                os.close(first)

        slot, fd = ci_gate.acquire_slot(self.dir, 2, said.append, sleep=sleep)
        self.fds.append(fd)
        self.assertEqual(slot, 1)
        self.assertEqual(len(sleeps), 2)
        self.assertEqual(said, ["all 2 gate slots are in use (SC_CI_SLOTS); waiting for one"])

    def test_two_gates_never_share_a_slot(self) -> None:
        _, fd = ci_gate.acquire_slot(self.dir, 1, self.fail, sleep=self.fail)
        self.fds.append(fd)

        def sleep(_seconds: float) -> None:
            raise InterruptedError

        with self.assertRaises(InterruptedError):
            ci_gate.acquire_slot(self.dir, 1, lambda _message: None, sleep=sleep)

    def test_a_held_slot_is_not_handed_out_twice(self) -> None:
        slot, fd = ci_gate.acquire_slot(self.dir, 1, self.fail, sleep=self.fail)
        self.fds.append(fd)
        self.assertEqual(slot, 1)
        self.assertIsNone(ci_gate.try_lock(self.dir / "stepclimb-ci-slot.1", fcntl.LOCK_EX))


class LegacyLockTest(LockTestCase):
    def test_new_gates_share_the_legacy_lock(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_SH)
        self.assertIsNotNone(self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_SH))

    def test_a_legacy_wrapper_excludes_new_gates_and_waits_for_them(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_EX)
        self.assertIsNone(ci_gate.try_lock(self.dir / ci_gate.LEGACY_LOCK, fcntl.LOCK_SH))

    def test_a_running_new_gate_holds_off_a_legacy_wrapper(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_SH)
        self.assertIsNone(ci_gate.try_lock(self.dir / ci_gate.LEGACY_LOCK, fcntl.LOCK_EX))

    @unittest.skipIf(shutil.which("flock") is None, "needs util-linux flock")
    def test_flock_command_and_the_gate_see_each_others_locks(self) -> None:
        legacy = self.dir / ci_gate.LEGACY_LOCK
        holder = subprocess.Popen(["flock", str(legacy), "sleep", "30"])
        self.addCleanup(holder.wait)
        self.addCleanup(holder.kill)
        for _ in range(100):
            fd = ci_gate.try_lock(legacy, fcntl.LOCK_SH)
            if fd is None:
                break
            os.close(fd)
            subprocess.run(["sleep", "0.05"], check=True)
        self.assertIsNone(ci_gate.try_lock(legacy, fcntl.LOCK_SH))

    @unittest.skipIf(shutil.which("flock") is None, "needs util-linux flock")
    def test_detects_a_real_flock_ancestor(self) -> None:
        legacy = self.dir / ci_gate.LEGACY_LOCK
        probe = (
            "import os, sys; sys.path.insert(0, sys.argv[1]); import ci_gate; "
            "from pathlib import Path; "
            "print(ci_gate.inside_legacy_wrapper(Path(sys.argv[2]), os.getpid()))"
        )
        here = str(Path(__file__).resolve().parent)
        args = [sys.executable, "-B", "-c", probe, here, str(legacy)]
        wrapped = subprocess.run(
            ["flock", str(legacy), "sh", "-c", '"$@"', "sh", *args],
            capture_output=True,
            text=True,
            check=True,
        )
        bare = subprocess.run(args, capture_output=True, text=True, check=True)
        self.assertEqual((wrapped.stdout.strip(), bare.stdout.strip()), ("True", "False"))


class TakeLocksTest(LockTestCase):
    """`take_locks` in a child process, as a second gate on the machine would run it."""

    def start(self, git_dir: Path, slots: int = 2, wrapper: tuple[str, ...] = ()) -> subprocess.Popen[str]:
        script = "\n".join(
            [
                "import sys",
                "from pathlib import Path",
                "from unittest import mock",
                "sys.path.insert(0, sys.argv[1])",
                "import ci_gate",
                "env = {'SC_CI_LOCK_DIR': sys.argv[2], 'SC_CI_SLOTS': sys.argv[4]}",
                "with mock.patch.object(ci_gate, 'git', return_value=sys.argv[3]):",
                "    ci_gate.take_locks(env, Path('/work'))",
                "print('locked', flush=True)",
            ]
        )
        here = str(Path(__file__).resolve().parent)
        git_dir.mkdir(exist_ok=True)
        child = subprocess.Popen(
            [*wrapper, sys.executable, "-B", "-c", script, here, str(self.dir), str(git_dir), str(slots)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        self.addCleanup(child.wait)
        self.addCleanup(self.kill, child)
        return child

    @staticmethod
    def kill(child: subprocess.Popen[str]) -> None:
        # The whole group: under a wrapper, the gate is the wrapper's child.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def outcome(self, child: subprocess.Popen[str]) -> tuple[str, str]:
        try:
            out, err = child.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            self.kill(child)
            out, err = child.communicate()
            return "blocked", err
        return out.strip(), err

    def test_runs_beside_another_new_gate(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_SH)
        self.hold("stepclimb-ci-slot.1")
        self.assertEqual(self.outcome(self.start(self.dir / "a"))[0], "locked")

    def test_waits_for_a_legacy_wrapper(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_EX)
        state, err = self.outcome(self.start(self.dir / "a"))
        self.assertEqual(state, "blocked")
        self.assertIn("holds the machine; waiting", err)

    @unittest.skipIf(shutil.which("flock") is None, "needs util-linux flock")
    def test_runs_inside_a_legacy_wrapper_without_waiting_for_it(self) -> None:
        wrapper = ("flock", str(self.dir / ci_gate.LEGACY_LOCK))
        self.assertEqual(self.outcome(self.start(self.dir / "a", wrapper=wrapper))[0], "locked")

    def test_waits_for_a_gate_in_the_same_worktree(self) -> None:
        (self.dir / "a").mkdir()
        self.hold("a/stepclimb-ci.lock")
        state, err = self.outcome(self.start(self.dir / "a"))
        self.assertEqual(state, "blocked")
        self.assertIn("another gate is running in this worktree", err)

    def test_waits_when_every_slot_is_taken(self) -> None:
        self.hold("stepclimb-ci-slot.1")
        self.hold("stepclimb-ci-slot.2")
        state, err = self.outcome(self.start(self.dir / "a", slots=2))
        self.assertEqual(state, "blocked")
        self.assertIn("all 2 gate slots are in use", err)


class AncestorTest(unittest.TestCase):
    legacy = Path("/tmp/stepclimb-ci.lock")

    def test_wrapper_args(self) -> None:
        yes = "/usr/bin/flock /tmp/stepclimb-ci.lock nix develop -c just ci"
        self.assertTrue(ci_gate.is_legacy_wrapper(yes, self.legacy))
        self.assertFalse(ci_gate.is_legacy_wrapper("flock /tmp/other.lock just ci", self.legacy))
        self.assertFalse(ci_gate.is_legacy_wrapper("vim /tmp/stepclimb-ci.lock", self.legacy))
        self.assertFalse(ci_gate.is_legacy_wrapper("", self.legacy))

    def test_walks_up_to_the_wrapper(self) -> None:
        tree = {
            40: (30, "python3 bin/ci_gate.py run -- just _ci-checks"),
            30: (20, "just ci"),
            20: (10, "flock /tmp/stepclimb-ci.lock nix develop -c just ci"),
            10: (1, "fish"),
        }
        self.assertTrue(ci_gate.inside_legacy_wrapper(self.legacy, 40, tree.get))
        self.assertFalse(ci_gate.inside_legacy_wrapper(self.legacy, 30, {30: (1, "x")}.get))
        self.assertFalse(ci_gate.inside_legacy_wrapper(self.legacy, 40, {}.get))


class RunTeardownTest(unittest.TestCase):
    """`run` removes the stack before and after the command, whatever the command does."""

    def setUp(self) -> None:
        patches = {
            "git": mock.patch.object(ci_gate, "git", return_value="/work/stepclimb"),
            "take_locks": mock.patch.object(ci_gate, "take_locks", return_value=[]),
            "remove_stack": mock.patch.object(ci_gate, "remove_stack"),
            "start_database": mock.patch.object(
                ci_gate, "start_database", return_value="postgres://db"
            ),
            "say": mock.patch.object(ci_gate, "say"),
        }
        self.mocks = {name: patch.start() for name, patch in patches.items()}
        self.addCleanup(mock.patch.stopall)

    def test_failing_command_is_torn_down_and_its_status_kept(self) -> None:
        status = ci_gate.run([sys.executable, "-B", "-c", "raise SystemExit(7)"])
        self.assertEqual(status, 7)
        self.assertEqual(self.mocks["remove_stack"].call_count, 2)

    def test_command_sees_the_run_database_and_a_free_port(self) -> None:
        probe = "\n".join(
            [
                "import os, socket",
                "assert os.environ['DATABASE_URL'] == 'postgres://db'",
                "socket.socket().bind(('127.0.0.1', int(os.environ['SC_PROD_CHECK_PORT'])))",
            ]
        )
        self.assertEqual(ci_gate.run([sys.executable, "-B", "-c", probe]), 0)

    def test_a_failed_database_start_is_torn_down(self) -> None:
        self.mocks["start_database"].side_effect = ci_gate.GateError("no docker")
        with self.assertRaises(ci_gate.GateError):
            ci_gate.run(["true"])
        self.assertEqual(self.mocks["remove_stack"].call_count, 2)

    def test_main_reports_a_setup_failure_as_exit_2(self) -> None:
        self.mocks["start_database"].side_effect = ci_gate.GateError("no docker")
        self.assertEqual(ci_gate.main(["run", "--", "true"]), 2)


if __name__ == "__main__":
    unittest.main()
