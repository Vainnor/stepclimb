#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run `just ci` (or another command) so that several worktrees can gate at once.

`run -- CMD...` takes three locks, always in this order, then starts this worktree's own Compose
project, runs CMD with its database in DATABASE_URL, and removes the project whatever CMD's result:

1. the lock `flock /tmp/stepclimb-ci.lock` takes, held shared: a gate run under that exclusive
   wrapper (an older checkout's gate, which uses the fixed project `stepclimb` and port 58080 and
   assumes it runs alone) waits for these runs and they wait for it. A run that is itself inside
   such a wrapper skips this lock, which its wrapper already holds;
2. this worktree's lock (`<git dir>/stepclimb-ci.lock`): one run per worktree, since the project
   name, the image tag and the PR stamp are per worktree;
3. one of SC_CI_SLOTS (default 3) slot files in /tmp, so at most that many gates share the
   machine.

The project, `stepclimb-ci-<id>` with `<id>` from the worktree path, is the development Compose
file plus deploy/compose/ci.yaml: PostgreSQL on tmpfs and the server for `just e2e`, each on a port
Docker picks. Every database the run touches (migrations, `#[sqlx::test]` databases, whose names
depend only on the test's path, the e2e server) lives in it, so runs from checkouts with different
migrations never meet. The run also names the production check's project
(`stepclimb-prodcheck-<id>`), and sets SC_IMAGE, the server image tag both Compose files use, to
`stepclimb:ci-<id>`; it removes both projects and the image at the end. Before it starts, it
removes what a killed run of this worktree left, and every `stepclimb-ci-*` or
`stepclimb-prodcheck-*` project whose worktree no longer exists (Compose labels each container
with the directory of its file, `<worktree>/deploy/compose`).

SIGTERM, SIGHUP and SIGINT while waiting for a lock end the run at once. Later they are passed to
CMD (SIGINT reaches it from the terminal already), CMD is not started if it has not been, and the
removal still runs.

`compose -- ARGS...` runs `docker compose` on the current run's project (SC_CI_PROJECT).
`dev-up` starts the shared development PostgreSQL and MinIO (`just up`) under a lock, never
recreating a running container.
"""

from __future__ import annotations

import fcntl
import hashlib
import os
import re
import signal
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


class Stopped(Exception):
    """A signal ended the run while it was waiting for a lock."""

    def __init__(self, signum: int) -> None:
        super().__init__(f"stopped by signal {signum}")
        self.signum = signum


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


Process = tuple[int, str, str | None]  # parent pid, command line, working directory


def process_info(pid: int) -> Process | None:
    out = subprocess.run(
        ["ps", "-o", "ppid=", "-o", "args=", "-p", str(pid)],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    if not out:
        return None
    ppid, _, args = out.partition(" ")
    try:
        cwd: str | None = os.readlink(f"/proc/{pid}/cwd")
    except OSError:  # no /proc (macOS), or the process is gone
        cwd = None
    return int(ppid), args.strip(), cwd


def is_legacy_wrapper(args: str, legacy: Path, cwd: str | None) -> bool:
    """Whether `args` is `flock ... <legacy> ...`, the path written in any form that resolves to it."""
    words = args.split()
    if not words or Path(words[0]).name != "flock":
        return False
    base = cwd or os.getcwd()
    target = os.path.realpath(legacy)
    return any(os.path.realpath(os.path.join(base, word)) == target for word in words[1:])


def inside_legacy_wrapper(
    legacy: Path,
    pid: int,
    lookup: Callable[[int], Process | None] = process_info,
) -> bool:
    """Whether an ancestor of `pid` is `flock <legacy> ...`, which holds the lock exclusively."""
    seen: set[int] = set()
    while pid > 1 and pid not in seen:
        seen.add(pid)
        found = lookup(pid)
        if found is None:
            return False
        pid, args, cwd = found
        if is_legacy_wrapper(args, legacy, cwd):
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


def remove_image(image: str, env: Mapping[str, str]) -> None:
    def exists() -> bool:
        inspect = ["docker", "image", "inspect", image]
        return subprocess.run(inspect, capture_output=True, check=False, env=dict(env)).returncode == 0

    if exists():
        try:
            checked(["docker", "image", "rm", image], env, f"removing the image {image}")
        except GateError:
            if exists():  # another run removing the same image is not a failure
                raise


def remove_stack(toplevel: Path, env: Mapping[str, str]) -> None:
    project = env["SC_CI_PROJECT"]
    down = ["down", "--volumes", "--remove-orphans", "--timeout", "5"]
    checked(compose_command(toplevel, project, "--profile", "stack", *down), env, f"removing {project}")
    production = env["SC_PROD_CHECK_PROJECT"]
    checked(
        ["docker", "compose", "-p", production, "-f", str(toplevel / PRODUCTION_FILE)]
        + ["--profile", "minio", *down],
        env,
        f"removing {production}",
    )
    remove_image(env["SC_IMAGE"], env)


OWN_PROJECT = re.compile(r"^stepclimb-(ci|prodcheck)-([0-9a-f]{8})$")


def orphaned_projects(listing: str, exists: Callable[[str], bool] = os.path.isdir) -> list[str]:
    """Gate projects in `docker ps` output (`<project>\t<file directory>`) whose directory is gone."""
    orphans = set()
    for line in listing.splitlines():
        project, _, directory = line.partition("\t")
        if OWN_PROJECT.match(project) and directory and not exists(directory):
            orphans.add(project)
    return sorted(orphans)


def remove_orphans(env: Mapping[str, str]) -> None:
    template = '{{.Label "com.docker.compose.project"}}\t{{.Label "com.docker.compose.project.working_dir"}}'
    listing = checked(
        ["docker", "ps", "--all", "--filter", "label=com.docker.compose.project", "--format", template],
        env,
        "listing Compose projects",
    )
    for project in orphaned_projects(listing):
        down = ["down", "--volumes", "--remove-orphans", "--timeout", "5"]
        checked(["docker", "compose", "-p", project, *down], env, f"removing {project}")
        match = OWN_PROJECT.match(project)
        assert match is not None  # orphaned_projects returns only names it matched
        remove_image(f"stepclimb:ci-{match.group(2)}", env)
        say(f"removed {project}, left by a worktree that no longer exists")


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


def run_env(toplevel: Path, base: Mapping[str, str]) -> dict[str, str]:
    ident = run_id(toplevel)
    return {
        **base,
        "SC_CI_ID": ident,
        "SC_CI_PROJECT": f"stepclimb-ci-{ident}",
        "SC_IMAGE": f"stepclimb:ci-{ident}",
        "SC_PROD_CHECK_PROJECT": f"stepclimb-prodcheck-{ident}",
    }


class Signals:
    """SIGTERM, SIGHUP and SIGINT for the whole of a run.

    While `waiting` (for a lock) a signal raises Stopped: nothing has started. After that it is
    recorded, SIGTERM and SIGHUP are passed to the running command, and the run checks
    `received` between steps, so the removal always runs.
    """

    HANDLED = (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)

    def __init__(self) -> None:
        self.waiting = True
        self.received: int | None = None
        self.child: subprocess.Popen[bytes] | None = None
        self.previous: dict[int, object] = {}

    def __enter__(self) -> Signals:
        self.previous = {s: signal.signal(s, self.handle) for s in self.HANDLED}
        return self

    def __exit__(self, *_exc: object) -> None:
        for signum, handler in self.previous.items():
            signal.signal(signum, handler)  # type: ignore[arg-type]

    def handle(self, signum: int, _frame: object) -> None:
        if self.waiting:
            raise Stopped(signum)
        self.received = self.received or signum
        if self.child is not None and signum != signal.SIGINT and self.child.poll() is None:
            self.child.send_signal(signum)


def run_child(command: list[str], env: Mapping[str, str], signals: Signals) -> int:
    """Run `command` unless a signal came first; a signal during it is passed on (Signals)."""
    if signals.received is not None:
        return 128 + signals.received
    # Python handlers reset to the default in the child at exec, so it can be stopped as usual.
    signals.child = subprocess.Popen(command, env=dict(env))
    try:
        return signals.child.wait()
    finally:
        signals.child = None


def take_locks(env: Mapping[str, str], toplevel: Path) -> list[int]:
    """The legacy lock (unless inside its wrapper), this worktree's, then a slot: one order for
    every run, so a run under the wrapper and a plain run cannot each hold what the other needs."""
    held: list[int] = []
    try:
        directory = lock_dir(env)
        legacy = directory / LEGACY_LOCK
        if not inside_legacy_wrapper(legacy, os.getpid()):
            waiting = f"a gate run under `flock {legacy}` holds the machine; waiting for it"
            held.append(lock_waiting(legacy, fcntl.LOCK_SH, waiting, say))
        held.append(
            lock_waiting(
                Path(git("rev-parse", "--absolute-git-dir")) / "stepclimb-ci.lock",
                fcntl.LOCK_EX,
                "another gate is running in this worktree; waiting for it",
                say,
            )
        )
        slot, fd = acquire_slot(directory, slot_count(env), say)
        held.append(fd)
    except BaseException:
        for fd in held:
            os.close(fd)
        raise
    say(f"slot {slot} of {slot_count(env)}, project stepclimb-ci-{run_id(toplevel)}")
    return held


def run_stack(toplevel: Path, env: dict[str, str], command: list[str], signals: Signals) -> int:
    status = 1
    try:
        remove_orphans(env)
        remove_stack(toplevel, env)  # what a killed run in this worktree left behind
        if signals.received is None:
            env["DATABASE_URL"] = start_database(toplevel, env)
            say(f"database {env['DATABASE_URL']}")
        status = run_child(command, env, signals)
    finally:
        remove_stack(toplevel, env)
        say(f"removed {env['SC_CI_PROJECT']}, {env['SC_PROD_CHECK_PROJECT']} and {env['SC_IMAGE']}")
    return 128 + signals.received if signals.received and status == 0 else status


def run(command: list[str]) -> int:
    if not command:
        raise GateError("usage: ci_gate.py run -- COMMAND...")
    toplevel = Path(git("rev-parse", "--show-toplevel"))
    env = run_env(toplevel, os.environ)
    with Signals() as signals:
        try:
            held = take_locks(env, toplevel)
        except Stopped as stop:
            say(f"stopped by signal {stop.signum} while waiting")
            return 128 + stop.signum
        signals.waiting = False
        try:
            return run_stack(toplevel, env, command, signals)
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
