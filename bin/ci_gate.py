#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run `just ci` (or another command) so that several worktrees can gate at once.

`run -- CMD...` takes three locks, then starts this worktree's own Compose project, runs CMD with
its database in DATABASE_URL, and removes the project whatever CMD's result:

1. this worktree's lock (`<git dir>/stepclimb-ci.lock`): one run per worktree, since the project
   name, the image tag and the PR stamp are per worktree;
2. the lock `flock /tmp/stepclimb-ci.lock` takes, held shared: a gate run under that exclusive
   wrapper (an older checkout's gate, which uses the fixed project `stepclimb` and port 58080 and
   assumes it runs alone) waits for these runs and they wait for it. A run that is itself inside
   such a wrapper skips this lock, which its wrapper already holds;
3. one of SC_CI_SLOTS (default 3) slot files in /tmp, so at most that many gates share the
   machine.

The project, `stepclimb-ci-<id>` with `<id>` from the worktree path, is the development Compose
file plus deploy/compose/ci.yaml: PostgreSQL on tmpfs on a free port, and the server on a free
port for `just e2e`. Every database the run touches (migrations, `#[sqlx::test]` databases, whose
names depend only on the test's path, the e2e server) lives in it, so runs from checkouts with
different migrations never meet. The run also names the production check's project
(`stepclimb-prodcheck-<id>`) and port (a free one), and sets SC_IMAGE, the server image tag both
Compose files use, to `stepclimb:ci-<id>`; it removes both projects and the image at the end.

`compose -- ARGS...` runs `docker compose` on the current run's project (SC_CI_PROJECT).
`dev-up` starts the shared development PostgreSQL and MinIO (`just up`) under a lock, never
recreating a running container.
"""

from __future__ import annotations

import fcntl
import hashlib
import os
import signal
import socket
import subprocess
import sys
import time
from collections.abc import Callable, Mapping
from pathlib import Path

LEGACY_LOCK = "stepclimb-ci.lock"
DEV_UP_LOCK = "stepclimb-dev-up.lock"
DEFAULT_SLOTS = 3
POLL_SECONDS = 2.0
COMPOSE_FILES = ("deploy/compose/compose.yaml", "deploy/compose/ci.yaml")
PRODUCTION_FILE = "deploy/compose/compose.production.yaml"
DB_USER_PASSWORD = "stepclimb:stepclimb-local"  # deploy/compose/compose.yaml


class GateError(Exception):
    """A setup or teardown step failed; the message says which."""


def lock_dir(env: Mapping[str, str]) -> Path:
    # /tmp, not TMPDIR: `nix develop` gives every shell its own TMPDIR, and the slots must be
    # machine-wide.
    return Path(env.get("SC_CI_LOCK_DIR", "/tmp"))


def slot_count(env: Mapping[str, str]) -> int:
    raw = env.get("SC_CI_SLOTS", str(DEFAULT_SLOTS))
    try:
        count = int(raw)
    except ValueError:
        count = 0
    if count < 1:
        raise GateError(f"SC_CI_SLOTS must be a whole number of at least 1, not {raw!r}")
    return count


def run_id(toplevel: Path) -> str:
    return hashlib.sha256(str(toplevel.resolve()).encode()).hexdigest()[:8]


def open_lock(path: Path) -> int:
    return os.open(path, os.O_RDWR | os.O_CREAT, 0o666)


def try_lock(path: Path, operation: int) -> int | None:
    """The descriptor holding `operation` (LOCK_EX or LOCK_SH) on `path`, or None if it is taken."""
    fd = open_lock(path)
    try:
        fcntl.flock(fd, operation | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        return None
    return fd


def lock_waiting(path: Path, operation: int, waiting: str, say: Callable[[str], None]) -> int:
    fd = try_lock(path, operation)
    if fd is not None:
        return fd
    say(waiting)
    fd = open_lock(path)
    fcntl.flock(fd, operation)
    return fd


def acquire_slot(
    directory: Path,
    count: int,
    say: Callable[[str], None],
    sleep: Callable[[float], None] = time.sleep,
) -> tuple[int, int]:
    """Block until one of `count` slots is free; return (slot number, descriptor holding it)."""
    announced = False
    while True:
        for slot in range(1, count + 1):
            fd = try_lock(directory / f"stepclimb-ci-slot.{slot}", fcntl.LOCK_EX)
            if fd is not None:
                return slot, fd
        if not announced:
            say(f"all {count} gate slots are in use (SC_CI_SLOTS); waiting for one")
            announced = True
        sleep(POLL_SECONDS)


def parent_and_args(pid: int) -> tuple[int, str] | None:
    out = subprocess.run(
        ["ps", "-o", "ppid=", "-o", "args=", "-p", str(pid)],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    if not out:
        return None
    ppid, _, args = out.partition(" ")
    return int(ppid), args.strip()


def is_legacy_wrapper(args: str, legacy: Path) -> bool:
    words = args.split()
    return bool(words) and Path(words[0]).name == "flock" and str(legacy) in words


def inside_legacy_wrapper(
    legacy: Path,
    pid: int,
    lookup: Callable[[int], tuple[int, str] | None] = parent_and_args,
) -> bool:
    """Whether an ancestor of `pid` is `flock <legacy> ...`, which holds the lock exclusively."""
    seen: set[int] = set()
    while pid > 1 and pid not in seen:
        seen.add(pid)
        found = lookup(pid)
        if found is None:
            return False
        pid, args = found
        if is_legacy_wrapper(args, legacy):
            return True
    return False


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout.strip()


def say(message: str) -> None:
    print(f"ci-gate: {message}", file=sys.stderr, flush=True)


def compose_command(toplevel: Path, project: str, *args: str) -> list[str]:
    files = [arg for name in COMPOSE_FILES for arg in ("-f", str(toplevel / name))]
    return ["docker", "compose", "-p", project, *files, *args]


def checked(command: list[str], env: Mapping[str, str], what: str) -> str:
    result = subprocess.run(command, env=dict(env), capture_output=True, text=True, check=False)
    if result.returncode != 0:
        sys.stderr.write(result.stdout + result.stderr)
        raise GateError(f"{what} failed (exit {result.returncode}): {' '.join(command)}")
    return result.stdout.strip()


def remove_stack(toplevel: Path, env: Mapping[str, str]) -> None:
    project, image = env["SC_CI_PROJECT"], env["SC_IMAGE"]
    down = compose_command(toplevel, project, "--profile", "stack", "down", "--volumes")
    checked([*down, "--remove-orphans", "--timeout", "5"], env, f"removing {project}")
    production = env["SC_PROD_CHECK_PROJECT"]
    down = ["docker", "compose", "-p", production, "-f", str(toplevel / PRODUCTION_FILE)]
    checked(
        [*down, "--profile", "minio", "down", "--volumes", "--remove-orphans", "--timeout", "5"],
        env,
        f"removing {production}",
    )
    exists = subprocess.run(
        ["docker", "image", "inspect", image], capture_output=True, check=False, env=dict(env)
    )
    if exists.returncode == 0:
        checked(["docker", "image", "rm", image], env, f"removing the image {image}")


def start_database(toplevel: Path, env: Mapping[str, str]) -> str:
    """Start the run's PostgreSQL and return its URL."""
    project = env["SC_CI_PROJECT"]
    checked(
        compose_command(toplevel, project, "up", "--detach", "--wait", "postgres"),
        env,
        f"starting PostgreSQL in {project}",
    )
    address = checked(
        compose_command(toplevel, project, "port", "postgres", "5432"),
        env,
        "finding the PostgreSQL port",
    )
    return f"postgres://{DB_USER_PASSWORD}@{address}/stepclimb"


def free_port() -> int:
    """A port nothing listens on now; the production check publishes its server there."""
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def run_env(toplevel: Path, base: Mapping[str, str]) -> dict[str, str]:
    ident = run_id(toplevel)
    return {
        **base,
        "SC_CI_ID": ident,
        "SC_CI_PROJECT": f"stepclimb-ci-{ident}",
        "SC_IMAGE": f"stepclimb:ci-{ident}",
        "SC_PROD_CHECK_PROJECT": f"stepclimb-prodcheck-{ident}",
    }


def run_child(command: list[str], env: Mapping[str, str]) -> int:
    """Run `command`, passing SIGTERM and SIGHUP on so that this process can still clean up."""
    child = subprocess.Popen(command, env=dict(env))

    def forward(signum: int, _frame: object) -> None:
        child.send_signal(signum)

    previous = {s: signal.signal(s, forward) for s in (signal.SIGTERM, signal.SIGHUP)}
    # Ctrl-C reaches the child through the terminal's process group; this process waits for it.
    previous[signal.SIGINT] = signal.signal(signal.SIGINT, signal.SIG_IGN)
    try:
        return child.wait()
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def take_locks(env: Mapping[str, str], toplevel: Path) -> list[int]:
    held = [
        lock_waiting(
            Path(git("rev-parse", "--absolute-git-dir")) / "stepclimb-ci.lock",
            fcntl.LOCK_EX,
            "another gate is running in this worktree; waiting for it",
            say,
        )
    ]
    directory = lock_dir(env)
    legacy = directory / LEGACY_LOCK
    if not inside_legacy_wrapper(legacy, os.getpid()):
        waiting = f"a gate run under `flock {legacy}` holds the machine; waiting for it"
        held.append(lock_waiting(legacy, fcntl.LOCK_SH, waiting, say))
    slot, fd = acquire_slot(directory, slot_count(env), say)
    held.append(fd)
    say(f"slot {slot} of {slot_count(env)}, project stepclimb-ci-{run_id(toplevel)}")
    return held


def run(command: list[str]) -> int:
    if not command:
        raise GateError("usage: ci_gate.py run -- COMMAND...")
    toplevel = Path(git("rev-parse", "--show-toplevel"))
    env = run_env(toplevel, os.environ)
    held = take_locks(env, toplevel)
    try:
        remove_stack(toplevel, env)  # what a killed run in this worktree left behind
        status = 1
        try:
            env["SC_PROD_CHECK_PORT"] = str(free_port())
            env["DATABASE_URL"] = start_database(toplevel, env)
            say(f"database {env['DATABASE_URL']}")
            status = run_child(command, env)
        finally:
            remove_stack(toplevel, env)
            say(f"removed {env['SC_CI_PROJECT']}, {env['SC_PROD_CHECK_PROJECT']} and {env['SC_IMAGE']}")
        return status
    finally:
        for fd in held:
            os.close(fd)


def compose(args: list[str]) -> int:
    project = os.environ.get("SC_CI_PROJECT")
    if not project:
        raise GateError("SC_CI_PROJECT is not set; run this inside `ci_gate.py run`")
    toplevel = Path(git("rev-parse", "--show-toplevel"))
    return subprocess.run(compose_command(toplevel, project, *args), check=False).returncode


def dev_up() -> int:
    toplevel = Path(git("rev-parse", "--show-toplevel"))
    fd = lock_waiting(
        lock_dir(os.environ) / DEV_UP_LOCK,
        fcntl.LOCK_EX,
        "another `just up` is starting the development stack; waiting",
        say,
    )
    try:
        command = ["docker", "compose", "-f", str(toplevel / COMPOSE_FILES[0])]
        up = ["up", "--detach", "--wait", "--no-recreate", "postgres", "minio"]
        return subprocess.run([*command, *up], check=False).returncode
    finally:
        os.close(fd)


def main(argv: list[str]) -> int:
    if len(argv) >= 1 and argv[0] == "dev-up":
        return dev_up()
    if len(argv) >= 2 and argv[0] in ("run", "compose") and argv[1] == "--":
        try:
            return run(argv[2:]) if argv[0] == "run" else compose(argv[2:])
        except GateError as error:
            say(str(error))
            return 2
    say("usage: ci_gate.py run -- COMMAND... | compose -- ARGS... | dev-up")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
