#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run `just ci` (or another command) so that several worktrees can gate at once.

`run -- CMD...` takes three locks, always in this order, then starts this worktree's own Compose
project, runs CMD with its database in DATABASE_URL, and removes the project whatever CMD's result:

1. the lock `flock /tmp/stepclimb-ci.lock` takes, held shared: a gate run under that exclusive
   wrapper (an older checkout's gate, which uses the fixed project `stepclimb` and port 58080 and
   assumes it runs alone) waits for these runs and they wait for it. A run that is itself inside
   such a wrapper skips this lock, which its wrapper already holds. Only the command form,
   `flock <file> <command>`, is recognized: a wrapper that holds the lock on a descriptor
   (`exec 9>file; flock 9`) is not, and a gate inside it waits for itself;
2. this worktree's lock (`<git dir>/stepclimb-ci.lock`): one run per worktree, since the project
   name, the image tag and the PR stamp are per worktree;
3. one of SC_CI_SLOTS (default 3) slot files in /tmp, so at most that many gates share the
   machine.

The project, `stepclimb-ci-<id>` with `<id>` from the worktree path, is the development Compose
file plus deploy/compose/ci.yaml: PostgreSQL and MinIO on tmpfs, and the server and the mock identity
provider for `just e2e`, each on a port of the run's slot (slot_env). Every database the run touches (migrations, `#[sqlx::test]` databases, whose names
depend only on the test's path, the e2e server) lives in it, so runs from checkouts with different
migrations never meet. CMD finds the PostgreSQL URL in DATABASE_URL and MinIO's, with the bucket
`stepclimb` already made, in SC_CI_MINIO_URL (the storage tests, docs/specs/m1/06-files.md). The run also names the production and ops checks' projects
(`stepclimb-prodcheck-<id>`, `stepclimb-opscheck-<id>` and its `-restore`), gives them its slot's
ports and proxy subnets (slot_env), and sets SC_IMAGE, the server image
tag the Compose files use, to `stepclimb:ci-<id>` (bin/check-image.sh adds `-amd64` and `-arm64`
tags of its own, ci.yaml's mock identity provider `-mock-idp`); it removes every project and those images at the end. Before it starts, it removes what a killed run of this worktree left, every such project
whose worktree no longer exists (Compose labels each container with the directory of its file,
`<worktree>/deploy/compose`), and any project holding its slot's subnets, which only a killed run
that had the same slot can.

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
# Each slot owns a /24 per proxy network, 10.201.(100 + slot) for the production check and
# 10.201.(150 + slot) for the ops check (clear of the checks' own defaults, .73 to .75), and ten
# ports from 20000 + 10 * slot. The ports sit below Linux's ephemeral range (32768 and up): there
# a host socket, such as an outgoing connection's source port, can hold the number, and rootless
# Docker, which picks a free port in its own network namespace, cannot see it.
MAX_SLOTS = 49
SLOT_PORTS = (
    "SC_CI_PG_PORT",
    "SC_CI_PORT",
    "SC_PROD_CHECK_PORT",
    "SC_OPS_CHECK_PORT",
    "SC_OPS_CHECK_HTTP_PORT",
    "SC_OPS_CHECK_HTTPS_PORT",
    "SC_CI_IDP_PORT",
    "SC_CI_MINIO_PORT",
)
POLL_SECONDS = 2.0
COMPOSE_FILES = ("deploy/compose/compose.yaml", "deploy/compose/ci.yaml")
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
    if not 1 <= count <= MAX_SLOTS:
        raise GateError(f"SC_CI_SLOTS must be a whole number from 1 to {MAX_SLOTS}, not {raw!r}")
    return count


def slot_env(slot: int) -> dict[str, str]:
    """The published ports and proxy subnets of one slot, so no two runs share one."""
    ports = {name: str(20000 + 10 * slot + i) for i, name in enumerate(SLOT_PORTS)}
    return {
        **ports,
        "SC_CI_SLOT": str(slot),
        "SC_PROD_CHECK_NETWORK": f"10.201.{100 + slot}",
        "SC_OPS_CHECK_NETWORK": f"10.201.{150 + slot}",
    }


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


DOWN = ("down", "--volumes", "--remove-orphans", "--timeout", "5")
OWN_PROJECT = re.compile(r"^stepclimb-(ci|prodcheck|opscheck)-([0-9a-f]{8})(-restore)?$")
SLOT_LABEL = "stepclimb.ci.slot"  # on the run's own project, deploy/compose/ci.yaml


def remove_project(project: str, env: Mapping[str, str]) -> None:
    """Remove a Compose project's containers, networks and volumes, by name alone.

    An ops check project first loses the echo container bin/check-compose-ops.sh starts on its
    proxy network outside Compose, which would otherwise keep that network in use."""
    match = OWN_PROJECT.match(project)
    if match and match.group(1) == "opscheck" and not match.group(3):
        echo = f"{project}-echo"
        checked(["docker", "rm", "--force", echo], env, f"removing {echo}")
    checked(["docker", "compose", "-p", project, *DOWN], env, f"removing {project}")


def run_projects(env: Mapping[str, str]) -> list[str]:
    ops = env["SC_OPS_CHECK_PROJECT"]
    return [env["SC_CI_PROJECT"], env["SC_PROD_CHECK_PROJECT"], ops, f"{ops}-restore"]


def run_images(image: str) -> list[str]:
    """The server image a run tags, the per-architecture tags bin/check-image.sh adds to it, and
    the mock identity provider's image of `just e2e` (deploy/compose/ci.yaml)."""
    return [image, f"{image}-amd64", f"{image}-arm64", f"{image}-mock-idp"]


def remove_stack(env: Mapping[str, str]) -> None:
    """Remove every project, container and image a run of this worktree creates."""
    for project in run_projects(env):
        remove_project(project, env)
    for image in run_images(env["SC_IMAGE"]):
        remove_image(image, env)


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
        remove_project(project, env)
        match = OWN_PROJECT.match(project)
        assert match is not None  # orphaned_projects returns only names it matched
        for image in run_images(f"stepclimb:ci-{match.group(2)}"):
            remove_image(image, env)
        say(f"removed {project}, left by a worktree that no longer exists")


def subnet_holders(listing: str, subnets: set[str]) -> list[str]:
    """Projects in `docker network inspect` output (`<project>|<subnet> ...`) using one of `subnets`."""
    holders = set()
    for line in listing.splitlines():
        project, _, nets = line.partition("|")
        if project and subnets & set(nets.split()):
            holders.add(project)
    return sorted(holders)


def free_slot(env: Mapping[str, str]) -> None:
    """Remove what a killed run with this slot left on its ports and subnets.

    The slot lock is exclusive, so a gate project labelled with this slot (the PostgreSQL and e2e
    server project) or on this slot's subnets (the check projects) belongs to a run that died. A
    project that is not a gate's is never removed: it fails the run, naming the project."""
    labelled = checked(
        ["docker", "ps", "--all", "--filter", f"label={SLOT_LABEL}={env['SC_CI_SLOT']}"]
        + ["--format", '{{.Label "com.docker.compose.project"}}'],
        env,
        "listing this slot's containers",
    ).split()
    stale = set(labelled) | set(subnet_holders(subnet_listing(env), slot_subnets(env)))
    foreign = sorted(p for p in stale if not OWN_PROJECT.match(p))
    if foreign:
        raise GateError(
            f"{', '.join(foreign)} uses this gate slot's subnet "
            f"({' or '.join(sorted(slot_subnets(env)))}); remove it or lower SC_CI_SLOTS"
        )
    for project in sorted(stale):
        remove_project(project, env)
        say(f"removed {project}, left on this slot by a run that was killed")


def slot_subnets(env: Mapping[str, str]) -> set[str]:
    return {f"{env[name]}.0/24" for name in ("SC_PROD_CHECK_NETWORK", "SC_OPS_CHECK_NETWORK")}


def subnet_listing(env: Mapping[str, str]) -> str:
    networks = checked(
        ["docker", "network", "ls", "--quiet", "--filter", "label=com.docker.compose.project"],
        env,
        "listing Compose networks",
    ).split()
    if not networks:
        return ""
    template = '{{index .Labels "com.docker.compose.project"}}|{{range .IPAM.Config}}{{.Subnet}} {{end}}'
    return checked(
        ["docker", "network", "inspect", "--format", template, *networks], env, "reading subnets"
    )


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


MINIO_URL_VAR = "SC_CI_MINIO_URL"


def start_storage(toplevel: Path, env: Mapping[str, str]) -> str:
    """Start the run's MinIO, whose bucket `stepclimb` deploy/compose/ci.yaml makes, and return its URL."""
    project = env["SC_CI_PROJECT"]
    checked(
        compose_command(toplevel, project, "up", "--detach", "--wait", "minio"),
        env,
        f"starting MinIO in {project}",
    )
    address = checked(
        compose_command(toplevel, project, "port", "minio", "9000"),
        env,
        "finding the MinIO port",
    )
    return f"http://{address}"


def run_env(toplevel: Path, base: Mapping[str, str]) -> dict[str, str]:
    ident = run_id(toplevel)
    return {
        **base,
        "SC_CI_ID": ident,
        "SC_CI_PROJECT": f"stepclimb-ci-{ident}",
        "SC_IMAGE": f"stepclimb:ci-{ident}",
        "SC_PROD_CHECK_PROJECT": f"stepclimb-prodcheck-{ident}",
        "SC_OPS_CHECK_PROJECT": f"stepclimb-opscheck-{ident}",
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


def take_locks(env: Mapping[str, str], toplevel: Path) -> tuple[list[int], int]:
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
    return held, slot


def run_stack(toplevel: Path, env: dict[str, str], command: list[str], signals: Signals) -> int:
    status = 1
    try:
        remove_orphans(env)
        remove_stack(env)  # what a killed run in this worktree left behind
        free_slot(env)
        if signals.received is None:
            env["DATABASE_URL"] = start_database(toplevel, env)
            say(f"database {env['DATABASE_URL']}")
            env[MINIO_URL_VAR] = start_storage(toplevel, env)
            say(f"object storage {env[MINIO_URL_VAR]}")
        status = run_child(command, env, signals)
    finally:
        remove_stack(env)
        say(f"removed {', '.join(run_projects(env))} and {', '.join(run_images(env['SC_IMAGE']))}")
    return 128 + signals.received if signals.received and status == 0 else status


def run(command: list[str]) -> int:
    if not command:
        raise GateError("usage: ci_gate.py run -- COMMAND...")
    toplevel = Path(git("rev-parse", "--show-toplevel"))
    env = run_env(toplevel, os.environ)
    with Signals() as signals:
        try:
            held, slot = take_locks(env, toplevel)
        except Stopped as stop:
            say(f"stopped by signal {stop.signum} while waiting")
            return 128 + stop.signum
        signals.waiting = False
        env.update(slot_env(slot))
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
