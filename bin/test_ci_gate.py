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
import time
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

    def test_rejects_zero_words_and_more_than_the_subnets_allow(self) -> None:
        for raw in ("0", "-1", "three", "", "50"):
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
        self.assertEqual(env["SC_OPS_CHECK_PROJECT"], f"stepclimb-opscheck-{ident}")
        self.assertEqual(env["SC_IMAGE"], f"stepclimb:ci-{ident}")
        self.assertEqual(env["PATH"], "/bin")

    def test_every_slot_has_its_own_subnets_clear_of_the_defaults(self) -> None:
        seen: set[str] = set()
        for slot in range(1, ci_gate.MAX_SLOTS + 1):
            env = ci_gate.slot_env(slot)
            seen.update((env["SC_PROD_CHECK_NETWORK"], env["SC_OPS_CHECK_NETWORK"]))
        self.assertEqual(len(seen), 2 * ci_gate.MAX_SLOTS)
        self.assertTrue(seen.isdisjoint({"10.201.73", "10.201.74", "10.201.75"}))
        self.assertTrue(all(0 < int(net.rsplit(".", 1)[1]) < 255 for net in seen))

    def test_every_slot_has_its_own_ports_below_the_ephemeral_range(self) -> None:
        seen: list[int] = []
        for slot in range(1, ci_gate.MAX_SLOTS + 1):
            env = ci_gate.slot_env(slot)
            seen.extend(int(env[name]) for name in ci_gate.SLOT_PORTS)
        self.assertEqual(len(set(seen)), len(ci_gate.SLOT_PORTS) * ci_gate.MAX_SLOTS)
        self.assertTrue(all(1024 < port < 32768 for port in seen), (min(seen), max(seen)))

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
        for pipe in (child.stdout, child.stderr):
            self.addCleanup(pipe.close)  # type: ignore[union-attr]
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

    def outcome(self, child: subprocess.Popen[str], timeout: float = 3) -> tuple[str, str]:
        try:
            out, err = child.communicate(timeout=timeout)
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

    def test_holds_nothing_while_it_waits_for_the_legacy_lock(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_EX)
        child = self.start(self.dir / "a")
        time.sleep(1)
        self.assertEqual(child.poll(), None)
        fd = ci_gate.try_lock(self.dir / "a" / "stepclimb-ci.lock", fcntl.LOCK_EX)
        self.assertIsNotNone(fd, "the worktree lock was taken before the legacy lock")
        os.close(fd)  # type: ignore[arg-type]

    @unittest.skipIf(shutil.which("flock") is None, "needs util-linux flock")
    def test_a_wrapped_gate_and_a_plain_gate_in_one_worktree_both_finish(self) -> None:
        legacy = self.dir / ci_gate.LEGACY_LOCK
        # The wrapper takes the legacy lock at once; its gate starts a second later, after the
        # plain gate below has started.
        wrapper = ("flock", str(legacy), "sh", "-c", 'sleep 1; exec "$@"', "sh")
        wrapped = self.start(self.dir / "a", wrapper=wrapper)
        for _ in range(100):
            fd = ci_gate.try_lock(legacy, fcntl.LOCK_SH)
            if fd is None:
                break
            os.close(fd)
            time.sleep(0.02)
        plain = self.start(self.dir / "a")
        self.assertEqual(self.outcome(wrapped, timeout=10)[0], "locked")
        self.assertEqual(self.outcome(plain, timeout=10)[0], "locked")

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
        def wrapper(args: str, cwd: str | None = "/") -> bool:
            return ci_gate.is_legacy_wrapper(args, self.legacy, cwd)

        self.assertTrue(wrapper("/usr/bin/flock /tmp/stepclimb-ci.lock nix develop -c just ci"))
        self.assertTrue(wrapper("flock -w 600 /tmp//stepclimb-ci.lock just ci"))
        self.assertTrue(wrapper("flock /tmp/./stepclimb-ci.lock just ci"))
        self.assertTrue(wrapper("flock stepclimb-ci.lock just ci", cwd="/tmp"))
        self.assertFalse(wrapper("flock stepclimb-ci.lock just ci", cwd="/home"))
        self.assertFalse(wrapper("flock /tmp/other.lock just ci"))
        self.assertFalse(wrapper("vim /tmp/stepclimb-ci.lock"))
        self.assertFalse(wrapper(""))

    def test_walks_up_to_the_wrapper(self) -> None:
        tree = {
            40: (30, "python3 bin/ci_gate.py run -- just _ci-checks", "/w"),
            30: (20, "just ci", "/w"),
            20: (10, "flock /tmp/stepclimb-ci.lock nix develop -c just ci", "/w"),
            10: (1, "fish", "/w"),
        }
        self.assertTrue(ci_gate.inside_legacy_wrapper(self.legacy, 40, tree.get))
        self.assertFalse(ci_gate.inside_legacy_wrapper(self.legacy, 30, {30: (1, "x", None)}.get))
        self.assertFalse(ci_gate.inside_legacy_wrapper(self.legacy, 40, {}.get))


class OrphanTest(unittest.TestCase):
    def test_only_gate_projects_whose_worktree_is_gone(self) -> None:
        listing = "\n".join(
            [
                "stepclimb-ci-0123abcd\t/gone/a/deploy/compose",
                "stepclimb-ci-0123abcd\t/gone/a/deploy/compose",
                "stepclimb-prodcheck-0123abcd\t/gone/a/deploy/compose",
                "stepclimb-opscheck-0123abcd-restore\t/gone/a/deploy/compose",
                "stepclimb-ci-89abcdef\t/here/b/deploy/compose",
                "stepclimb\t/gone/c/deploy/compose",
                "stepclimb-ci-xyz\t/gone/d/deploy/compose",
                "stepclimb-ci-00000000\t",
                "",
            ]
        )
        orphans = ci_gate.orphaned_projects(listing, exists=lambda d: d.startswith("/here/"))
        self.assertEqual(
            orphans,
            [
                "stepclimb-ci-0123abcd",
                "stepclimb-opscheck-0123abcd-restore",
                "stepclimb-prodcheck-0123abcd",
            ],
        )

    def test_removes_each_orphan_and_its_images(self) -> None:
        listing = "stepclimb-opscheck-0123abcd-restore\t/gone/a/deploy/compose\n"
        with (
            mock.patch.object(ci_gate, "checked", return_value=listing) as checked,
            mock.patch.object(ci_gate, "remove_image") as remove_image,
            mock.patch.object(ci_gate, "say"),
        ):
            ci_gate.remove_orphans({})
        self.assertEqual(
            checked.call_args_list[1].args[0],
            ["docker", "compose", "-p", "stepclimb-opscheck-0123abcd-restore", "down", "--volumes"]
            + ["--remove-orphans", "--timeout", "5"],
        )
        images = ["stepclimb:ci-0123abcd", "stepclimb:ci-0123abcd-amd64", "stepclimb:ci-0123abcd-arm64"]
        self.assertEqual([c.args for c in remove_image.call_args_list], [(i, {}) for i in images])


class SubnetTest(unittest.TestCase):
    def test_finds_projects_on_this_slots_subnets_only(self) -> None:
        listing = "\n".join(
            [
                "stepclimb-prodcheck-0123abcd|10.201.101.0/24 ",
                "stepclimb-opscheck-0123abcd|10.201.151.0/24 ",
                "stepclimb-prodcheck-89abcdef|10.201.102.0/24 ",
                "stepclimb|172.19.0.0/16 ",
                "|10.201.101.0/24",
            ]
        )
        ours = {"10.201.101.0/24", "10.201.151.0/24"}
        self.assertEqual(
            ci_gate.subnet_holders(listing, ours),
            ["stepclimb-opscheck-0123abcd", "stepclimb-prodcheck-0123abcd"],
        )



class FreeSlotTest(unittest.TestCase):
    env = {
        "SC_CI_SLOT": "2",
        "SC_PROD_CHECK_NETWORK": "10.201.102",
        "SC_OPS_CHECK_NETWORK": "10.201.152",
    }

    def free(self, labelled: str, networks: str) -> tuple[mock.Mock, list[list[str]]]:
        listings = {"ps": labelled, "ls": "n1" if networks else "", "inspect": networks}
        commands: list[list[str]] = []

        def checked(command: list[str], *_: object) -> str:
            commands.append(command)
            return listings[command[2] if command[1] == "network" else command[1]]

        with (
            mock.patch.object(ci_gate, "checked", side_effect=checked),
            mock.patch.object(ci_gate, "remove_project") as remove_project,
            mock.patch.object(ci_gate, "say"),
        ):
            ci_gate.free_slot(self.env)
        return remove_project, commands

    def test_removes_another_worktrees_leftover_on_this_slot(self) -> None:
        leftover = "stepclimb-ci-89abcdef\nstepclimb-ci-89abcdef\n"
        remove_project, commands = self.free(leftover, "")
        self.assertIn("label=stepclimb.ci.slot=2", commands[0])
        remove_project.assert_called_once_with("stepclimb-ci-89abcdef", self.env)

    def test_removes_check_projects_on_this_slots_subnets(self) -> None:
        networks = "stepclimb-opscheck-89abcdef|10.201.152.0/24 \nother|10.9.0.0/16 "
        remove_project, _ = self.free("", networks)
        remove_project.assert_called_once_with("stepclimb-opscheck-89abcdef", self.env)

    def test_never_removes_a_project_that_is_not_a_gates(self) -> None:
        networks = "homelab|10.201.102.0/24 \nstepclimb-opscheck-89abcdef|10.201.152.0/24 "
        with self.assertRaisesRegex(ci_gate.GateError, "homelab uses this gate slot's subnet"):
            self.free("", networks)
        with (
            mock.patch.object(ci_gate, "checked", side_effect=["", "n1", networks]),
            mock.patch.object(ci_gate, "remove_project") as remove_project,
            self.assertRaises(ci_gate.GateError),
        ):
            ci_gate.free_slot(self.env)
        remove_project.assert_not_called()


class RemoveProjectTest(unittest.TestCase):
    def commands(self, project: str) -> list[list[str]]:
        with mock.patch.object(ci_gate, "checked", return_value="") as checked:
            ci_gate.remove_project(project, {})
        return [c.args[0] for c in checked.call_args_list]

    def test_an_ops_project_loses_its_echo_container_first(self) -> None:
        down = ["down", "--volumes", "--remove-orphans", "--timeout", "5"]
        self.assertEqual(
            self.commands("stepclimb-opscheck-0123abcd"),
            [
                ["docker", "rm", "--force", "stepclimb-opscheck-0123abcd-echo"],
                ["docker", "compose", "-p", "stepclimb-opscheck-0123abcd", *down],
            ],
        )
        self.assertEqual(len(self.commands("stepclimb-ci-0123abcd")), 1)
        self.assertEqual(len(self.commands("stepclimb-opscheck-0123abcd-restore")), 1)


class StackCommandsTest(unittest.TestCase):
    env = {
        "SC_CI_PROJECT": "stepclimb-ci-0123abcd",
        "SC_PROD_CHECK_PROJECT": "stepclimb-prodcheck-0123abcd",
        "SC_OPS_CHECK_PROJECT": "stepclimb-opscheck-0123abcd",
        "SC_IMAGE": "stepclimb:ci-0123abcd",
    }
    down = ["down", "--volumes", "--remove-orphans", "--timeout", "5"]

    def test_remove_stack_removes_every_project_with_volumes_and_the_images(self) -> None:
        with (
            mock.patch.object(ci_gate, "checked", return_value="") as checked,
            mock.patch.object(ci_gate, "remove_image") as remove_image,
        ):
            ci_gate.remove_stack(self.env)
        commands = [c.args[0] for c in checked.call_args_list]
        projects = [
            "stepclimb-ci-0123abcd",
            "stepclimb-prodcheck-0123abcd",
            "stepclimb-opscheck-0123abcd",
            "stepclimb-opscheck-0123abcd-restore",
        ]
        down = [["docker", "compose", "-p", project, *self.down] for project in projects]
        echo = ["docker", "rm", "--force", "stepclimb-opscheck-0123abcd-echo"]
        self.assertEqual(commands, [*down[:2], echo, *down[2:]])
        images = ["stepclimb:ci-0123abcd", "stepclimb:ci-0123abcd-amd64", "stepclimb:ci-0123abcd-arm64"]
        self.assertEqual([c.args for c in remove_image.call_args_list], [(i, self.env) for i in images])

    def test_start_database_uses_the_runs_own_postgres(self) -> None:
        answers = iter(["", "127.0.0.1:40001"])
        with mock.patch.object(ci_gate, "checked", side_effect=lambda *_: next(answers)) as checked:
            url = ci_gate.start_database(Path("/r"), self.env)
        self.assertEqual(url, "postgres://stepclimb:stepclimb-local@127.0.0.1:40001/stepclimb")
        commands = [c.args[0] for c in checked.call_args_list]
        project = "stepclimb-ci-0123abcd"
        self.assertEqual(
            commands,
            [
                ci_gate.compose_command(Path("/r"), project, "up", "--detach", "--wait", "postgres"),
                ci_gate.compose_command(Path("/r"), project, "port", "postgres", "5432"),
            ],
        )
        self.assertNotIn("55432", url)

    def test_a_vanished_image_is_not_a_failure(self) -> None:
        inspected = iter([0, 1])  # present, then gone after another run removed it
        with (
            mock.patch.object(
                ci_gate.subprocess,
                "run",
                side_effect=lambda *_a, **_k: mock.Mock(returncode=next(inspected)),
            ),
            mock.patch.object(ci_gate, "checked", side_effect=ci_gate.GateError("no such image")),
        ):
            ci_gate.remove_image("stepclimb:ci-0123abcd", {})


class RunTeardownTest(unittest.TestCase):
    """`run` removes the stack before and after the command, whatever the command does."""

    def setUp(self) -> None:
        patches = {
            "git": mock.patch.object(ci_gate, "git", return_value="/work/stepclimb"),
            "take_locks": mock.patch.object(ci_gate, "take_locks", return_value=([], 1)),
            "remove_orphans": mock.patch.object(ci_gate, "remove_orphans"),
            "free_slot": mock.patch.object(ci_gate, "free_slot"),
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
        self.mocks["remove_orphans"].assert_called_once()

    def test_command_sees_the_run_database_and_its_slots_subnets(self) -> None:
        probe = "\n".join(
            [
                "import os",
                "assert os.environ['DATABASE_URL'] == 'postgres://db'",
                "assert os.environ['SC_PROD_CHECK_NETWORK'] == '10.201.101'",
                "assert os.environ['SC_OPS_CHECK_NETWORK'] == '10.201.151'",
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


# A gate process with Docker replaced by lines in a log: run by SignalTest, which signals it.
HARNESS = """
import contextlib, os, sys, time
from pathlib import Path
from unittest import mock
sys.path.insert(0, sys.argv[1])
import ci_gate
log = Path(sys.argv[2])
def note(text):
    with log.open("a") as f:
        f.write(text + "\\n")
def orphans(*_):
    note("orphans")
    time.sleep(float(os.environ.get("ORPHAN_SECONDS", "0")))
def start(*_):
    note("start")
    time.sleep(float(os.environ.get("START_SECONDS", "0")))
    return "postgres://db"
with contextlib.ExitStack() as patches:
    patch = lambda name, **kw: patches.enter_context(mock.patch.object(ci_gate, name, **kw))
    patch("git", return_value=sys.argv[3])
    if not os.environ.get("REAL_LOCKS"):
        patch("take_locks", return_value=([], 1))
    patch("remove_orphans", side_effect=orphans)
    patch("free_slot")
    patch("remove_stack", side_effect=lambda *_: note("remove"))
    patch("start_database", side_effect=start)
    sys.exit(ci_gate.main(["run", "--", *sys.argv[4:]]))
"""

# The command the gate runs: notes that it started and which signal reached it, then exits with
# 128 + the signal, or 0 for SIGHUP when CHILD_HUP_STATUS says so.
CHILD = """
import signal, sys, time
from pathlib import Path
log = Path(sys.argv[1])
def got(signum, _):
    with log.open("a") as f:
        f.write(f"child got {signum}\\n")
    sys.exit(0 if signum == signal.SIGHUP else 128 + signum)
signal.signal(signal.SIGTERM, got)
signal.signal(signal.SIGHUP, got)
with log.open("a") as f:
    f.write("child running\\n")
time.sleep(30)
"""


class SignalTest(LockTestCase):
    def gate(self, extra_env: dict[str, str] | None = None) -> subprocess.Popen[str]:
        self.log = self.dir / "log"
        self.log.touch()
        git_dir = self.dir / "git"
        git_dir.mkdir(exist_ok=True)
        here = str(Path(__file__).resolve().parent)
        child = [sys.executable, "-B", "-c", CHILD, str(self.log)]
        env = {**os.environ, "SC_CI_LOCK_DIR": str(self.dir), **(extra_env or {})}
        self.err = self.dir / "err"
        with self.err.open("w") as err:
            gate = subprocess.Popen(
                [sys.executable, "-B", "-c", HARNESS, here, str(self.log), str(git_dir), *child],
                env=env,
                stderr=err,
                text=True,
            )
        self.addCleanup(gate.wait)
        self.addCleanup(lambda: gate.poll() is None and gate.kill())
        return gate

    def wait_for(self, line: str) -> None:
        for _ in range(200):
            if line in self.log.read_text().splitlines():
                return
            time.sleep(0.05)
        self.fail(f"{line!r} never appeared in {self.log.read_text()!r}")

    def lines(self) -> list[str]:
        return self.log.read_text().splitlines()

    def test_sigterm_reaches_the_command_and_the_stack_is_removed(self) -> None:
        gate = self.gate()
        self.wait_for("child running")
        gate.send_signal(signal.SIGTERM)
        self.assertEqual(gate.wait(timeout=20), 128 + signal.SIGTERM)
        self.assertEqual(
            self.lines(), ["orphans", "remove", "start", "child running", f"child got {signal.SIGTERM}", "remove"]
        )

    def test_sigterm_during_setup_skips_the_command_and_still_removes(self) -> None:
        gate = self.gate({"START_SECONDS": "1"})
        self.wait_for("start")
        gate.send_signal(signal.SIGTERM)
        self.assertEqual(gate.wait(timeout=20), 128 + signal.SIGTERM)
        self.assertEqual(self.lines(), ["orphans", "remove", "start", "remove"])

    def test_a_signal_during_cleanup_skips_the_database_and_the_command(self) -> None:
        gate = self.gate({"ORPHAN_SECONDS": "1"})
        self.wait_for("orphans")
        gate.send_signal(signal.SIGTERM)
        self.assertEqual(gate.wait(timeout=20), 128 + signal.SIGTERM)
        self.assertEqual(self.lines(), ["orphans", "remove", "remove"])

    def test_a_command_that_exits_0_after_a_signal_still_fails_the_run(self) -> None:
        gate = self.gate()
        self.wait_for("child running")
        gate.send_signal(signal.SIGHUP)
        self.assertEqual(gate.wait(timeout=20), 128 + signal.SIGHUP)
        self.assertIn(f"child got {signal.SIGHUP}", self.lines())

    def test_sigterm_while_waiting_for_a_lock_ends_the_run(self) -> None:
        self.hold(ci_gate.LEGACY_LOCK, fcntl.LOCK_EX)
        gate = self.gate({"REAL_LOCKS": "1"})
        for _ in range(200):
            if "holds the machine; waiting" in self.err.read_text():
                break
            time.sleep(0.05)
        else:
            self.fail(f"the gate never waited: {self.err.read_text()!r}")
        gate.send_signal(signal.SIGTERM)
        self.assertEqual(gate.wait(timeout=20), 128 + signal.SIGTERM)
        self.assertEqual(self.lines(), [])


if __name__ == "__main__":
    unittest.main()
