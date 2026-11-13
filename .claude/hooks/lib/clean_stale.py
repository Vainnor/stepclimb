# SPDX-License-Identifier: AGPL-3.0-or-later
"""Delete build output no build has used for N days from every worktree's `target/`.

Run from any checkout of this repository: `just clean-stale [--days N] [--dry-run] [--scratch
[--delete-scratch]]`. Where `.claude/scripts/post-merge-cleanup.sh` clears the whole `target/`
of a merged worktree, this keeps every worktree's `target/` and takes out what has gone stale
inside it: the builds of old commits, features and flags that cargo never deletes itself.
It exits 1 when any delete failed.

Each `target/` of a worktree in `git worktree list` (the main checkout and T3's included) is
judged in turn. One stays whole, and is reported with the reason, when it is a symlink or a
junction, when any process runs an executable from it, has its working directory in it or holds
a file in it open (`ps` and `lsof -n -P`, read again before each `target/`; cargo holds
`.cargo-lock` open for the whole build), or when `ps` or `lsof` is missing, fails or lists no
process. On Windows, which lists executables only, every `target/` stays, as in the post-merge
cleanup. While a profile directory (`target/debug`, `target/release`, `target/<triple>/debug`)
is cleaned, the run holds cargo's own locks in it (`.cargo-lock`, `.cargo-build-lock`,
`.cargo-artifact-lock`), so a build that starts meanwhile waits; a lock already held keeps that
directory.

In a profile directory, a unit is everything cargo built for one package target under one
metadata hash: `.fingerprint/<name>-<hash>`, `build/<name>-<hash>`, and the files in `deps/` and
`examples/` named with that hash. A unit is judged whole: it goes when nothing in it was read or
written within N days, the newest of each file's access and modification time (a directory's
modification time alone, since listing a directory sets its access time) being older than the
cutoff. Its fingerprint goes first, and the first delete that fails stops the unit, so cargo
never finds a fingerprint whose outputs are gone. Cargo reads a unit's fingerprint on every build
that uses it. Under `relatime`, the Linux default and WSL's mount, a read moves the access time
only when it is more than a day old, so what holds is that a unit used within N - 1 days stays;
N is at least 2. On a filesystem mounted `noatime` only the time a unit was built counts, and a
unit used but not rebuilt for N days is deleted and rebuilt once. An `incremental/` crate
directory goes whole when its newest session is older than the cutoff; in one that stays, every
session but the newest that is older than the cutoff goes, with its lock file (rustc names it
after the session without its last `-` part: `s-<time>-<id>-<hash>` locks as
`s-<time>-<id>.lock`). A file with no unit (the copies at the top of the profile directory, an
unknown name) goes when it alone is stale. Cargo's lock files and `.rustc_info.json` stay.
Outside the profile directories, stale entries of `target/tmp/` and `target/nextest/` and stale
`*.tar.zst` nextest archives go. Whatever goes, cargo rebuilds when a build needs it.

With `--scratch`, the run also lists the subdirectories of the agent scratch folder
(`~/.cache/stepclimb-tmp`, or `--scratch-dir`) in which nothing was modified within N days and no
process is using, with their size, and deletes them only with `--delete-scratch`: scratch holds
notes and PR bodies as well as build output. A subdirectory that is or holds a git checkout (a
`.git` file or directory anywhere in it, which covers every worktree of this or any repository)
or a worktree in this repository's `git worktree list` stays, whatever its state: its
uncommitted or unpushed work is for `/cleanup` or its owner to judge. On Windows every
subdirectory stays, for the reason every `target/` does.

`--dry-run` deletes nothing and reports what would go. A run holds the same lock in the git
common directory as the post-merge cleanup, so the two never clear targets at once. The report
names every `target/` with the space freed or the reason it stayed, a total, and the free space
on the repository's drive (and, on WSL, on each Windows drive holding a WSL virtual disk).
"""

import argparse
import datetime
import os
import re
import shutil
import sys
import time

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import post_merge_cleanup as pmc  # noqa: E402

if os.name != "nt":
    import fcntl

DEFAULT_DAYS = 3
# relatime moves a file's access time at most once a day, so a unit read within N - 1 days stays
# only if N is at least 2.
MIN_DAYS = 2
DAY = 86400
CARGO_LOCKS = (".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")
KEEP_NAMES = set(CARGO_LOCKS) | {".rustc_info.json", "CACHEDIR.TAG"}
# Cargo's metadata hash: 16 lowercase hex digits after the last `-` of the stem.
UNIT = re.compile(r"^(?:lib)?(?P<name>.+)-(?P<hash>[0-9a-f]{16})(?:\..*)?$")
UNIT_DIRS = ("deps", "examples")
OUTSIDE_PROFILES = ("tmp", "nextest")


def used_at(path, st):
    """A file's newest access or modification time; a directory's modification time alone,
    since listing a directory (this script, `du`, `ls`) sets its access time."""
    if os.path.isdir(path) and not os.path.islink(path):
        return st.st_mtime
    return max(st.st_atime, st.st_mtime)


def last_use(path):
    """When `path`, or anything under it, was last used (`used_at`); 0 if unreadable.

    Nothing is followed through a symbolic link.
    """
    try:
        newest = used_at(path, os.stat(path, follow_symlinks=False))
    except OSError:
        return 0.0
    if not os.path.isdir(path) or os.path.islink(path):
        return newest
    for root, dirs, files in os.walk(path):
        for name in dirs + files:
            p = os.path.join(root, name)
            try:
                newest = max(newest, used_at(p, os.stat(p, follow_symlinks=False)))
            except OSError:
                continue
    return newest


def last_modified(path):
    """The newest modification time of `path` or anything under it; 0 if unreadable."""
    try:
        newest = os.stat(path, follow_symlinks=False).st_mtime
    except OSError:
        return 0.0
    if os.path.isdir(path) and not os.path.islink(path):
        for root, dirs, files in os.walk(path):
            for name in dirs + files:
                try:
                    newest = max(newest, os.stat(os.path.join(root, name),
                                                 follow_symlinks=False).st_mtime)
                except OSError:
                    continue
    return newest


def entries(path):
    try:
        return sorted(os.scandir(path), key=lambda e: e.name)
    except OSError:
        return []


def profile_dirs(target):
    """Directories under `target` holding a cargo `.fingerprint/`, at most two levels down."""
    found = []
    for entry in entries(target):
        if not entry.is_dir(follow_symlinks=False):
            continue
        if os.path.isdir(os.path.join(entry.path, ".fingerprint")):
            found.append(entry.path)
            continue
        found += [sub.path for sub in entries(entry.path)
                  if sub.is_dir(follow_symlinks=False)
                  and os.path.isdir(os.path.join(sub.path, ".fingerprint"))]
    return found


def units(profile):
    """(units by hash, loose paths) in a profile directory; a unit lists its fingerprint first."""
    by_hash, loose = {}, []
    for entry in entries(os.path.join(profile, ".fingerprint")):
        match = UNIT.match(entry.name)
        if match:
            by_hash.setdefault(match["hash"], []).append(entry.path)
        else:
            loose.append(entry.path)
    for sub in ("build", *UNIT_DIRS):
        for entry in entries(os.path.join(profile, sub)):
            match = UNIT.match(entry.name)
            if match:
                by_hash.setdefault(match["hash"], []).append(entry.path)
            else:
                loose.append(entry.path)
    for entry in entries(profile):
        if entry.is_file(follow_symlinks=False) and entry.name not in KEEP_NAMES:
            loose.append(entry.path)
    return by_hash, loose


def session_lock(session):
    """rustc's lock file for an incremental session: its name without the last `-` part."""
    head, _, _ = os.path.basename(session).rpartition("-")
    return os.path.join(os.path.dirname(session), head + ".lock") if head else None


def stale_sessions(crate, cutoff):
    """Groups to delete in one `incremental/` crate directory that stays: each stale session
    but the newest, then its lock file."""
    sessions = [e.path for e in entries(crate) if e.is_dir(follow_symlinks=False)]
    if not sessions:
        return []
    newest = max(sessions, key=last_modified)
    groups = []
    for session in sessions:
        if session == newest or last_modified(session) >= cutoff:
            continue
        lock = session_lock(session)
        groups.append([session] + ([lock] if lock and os.path.exists(lock) else []))
    return groups


def incremental_plan(profile, cutoff):
    """Groups to delete under `incremental/`: stale crate directories, then stale old sessions."""
    groups = []
    for crate in entries(os.path.join(profile, "incremental")):
        if not crate.is_dir(follow_symlinks=False):
            continue
        has_session = any(e.is_dir(follow_symlinks=False) for e in entries(crate.path))
        if not has_session or last_use(crate.path) < cutoff:
            groups.append([crate.path])
        else:
            groups += stale_sessions(crate.path, cutoff)
    return groups


def profile_plan(profile, cutoff):
    """Groups of paths to delete in one profile directory, each deleted in order until one fails:
    a stale unit whole, fingerprint first, or one stale loose path."""
    by_hash, loose = units(profile)
    groups = [paths for paths in by_hash.values() if max(last_use(p) for p in paths) < cutoff]
    groups += [[p] for p in loose if last_use(p) < cutoff]
    return groups + incremental_plan(profile, cutoff)


def outside_plan(target, cutoff, profiles):
    """Stale paths of `target/` outside the profile directories (tmp, nextest, archives), one
    group each."""
    doomed = []
    for name in OUTSIDE_PROFILES:
        path = os.path.join(target, name)
        if path in profiles:
            continue
        doomed += [e.path for e in entries(path) if last_use(e.path) < cutoff]
    for entry in entries(target):
        if entry.is_file(follow_symlinks=False) and entry.name.endswith(".tar.zst") \
                and last_use(entry.path) < cutoff:
            doomed.append(entry.path)
    return [[p] for p in doomed]


def checkout_inside(path):
    """The first `.git` file or directory under `path` (itself included), or None."""
    for root, dirs, files in os.walk(path):
        if ".git" in dirs or ".git" in files:
            return os.path.join(root, ".git")
    return None


def size_of(path, seen):
    """Bytes on disk under `path`, a hard-linked file counted once across `seen`."""
    try:
        st = os.stat(path, follow_symlinks=False)
    except OSError:
        return 0
    if os.path.isdir(path) and not os.path.islink(path):
        return pmc.disk_bytes(path)
    if st.st_ino and (st.st_dev, st.st_ino) in seen:
        return 0
    seen.add((st.st_dev, st.st_ino))
    return st.st_blocks * 512 if hasattr(st, "st_blocks") else st.st_size


def delete(path):
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path)
    else:
        os.remove(path)


class CargoLocks:
    """Cargo's lock files in one profile directory, held exclusively, or the one already held."""

    def __init__(self, profile):
        self.handles, self.busy = [], None
        for name in CARGO_LOCKS:
            path = os.path.join(profile, name)
            try:
                fd = os.open(path, os.O_RDWR)
            except FileNotFoundError:
                continue
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                os.close(fd)
                self.busy = name
                break
            self.handles.append(fd)

    def release(self):
        for fd in self.handles:
            os.close(fd)  # closing the descriptor drops its flock
        self.handles = []


class CleanStale:
    """One run over every worktree's `target/` and, on request, the scratch folder."""

    def __init__(self, start, days, dry_run):
        self.job = pmc.Cleanup(start, dry_run)
        self.days = days
        self.cutoff = time.time() - days * DAY
        self.dry_run = dry_run
        self.failures = 0

    def say(self, line):
        self.job.say(line)

    def target_reason_to_keep(self, target):
        if pmc.is_link(target):
            return "target is a symlink"
        if os.name == "nt":
            return ("Windows lists only executables, not working directories or open files, "
                    "so no target/ is cleaned there")
        self.job.load_processes()
        if self.job.processes is None:
            return f"could not list processes ({self.job.process_error})"
        pids = self.job.live_processes(target)
        if pids:
            return f"{len(pids)} live processes use it (pids {', '.join(pids)})"
        return None

    def clean_groups(self, groups, seen):
        """Delete each group's paths in order (or only size them in a dry run), stopping a group
        at its first failed delete; (bytes, paths deleted, failure notes)."""
        freed, count, failed = 0, 0, []
        for group in groups:
            for path in group:
                if not os.path.lexists(path):
                    continue
                size = size_of(path, seen)
                if not self.dry_run:
                    try:
                        delete(path)
                    except OSError as error:
                        failed.append(f"failed to delete {self.job.shown(path)}: {error}")
                        break
                freed, count = freed + size, count + 1
        self.failures += len(failed)
        return freed, count, failed

    def clean_profile(self, profile, seen):
        """Clean one profile directory under cargo's locks; (bytes, paths, notes)."""
        locks = CargoLocks(profile)
        try:
            if locks.busy:
                return 0, 0, [f"kept {self.job.shown(profile)}: a build holds {locks.busy}"]
            return self.clean_groups(profile_plan(profile, self.cutoff), seen)
        finally:
            locks.release()

    def clean_target(self, target):
        """Clean one `target/` that passed its checks; the bytes freed."""
        seen, freed, count, notes = set(), 0, 0, []
        profiles = profile_dirs(target)
        for profile in profiles:
            size, n, more = self.clean_profile(profile, seen)
            freed, count, notes = freed + size, count + n, notes + more
        size, n, more = self.clean_groups(outside_plan(target, self.cutoff, profiles), seen)
        freed, count, notes = freed + size, count + n, notes + more
        verb = self.job.would("deleted", "delete")
        self.say(f"{verb} {count} stale paths in {self.job.shown(target)}: {pmc.gib(freed)}")
        for note in notes:
            self.say(f"  {note}")
        return freed

    def target_pass(self):
        total, kept = 0, 0
        for wt in self.job.worktrees():
            # git lists C:/ paths on Windows; normpath gives the separators os.path uses.
            target = os.path.normpath(os.path.join(wt["worktree"], "target"))
            if not (os.path.isdir(target) or pmc.is_link(target)):
                continue
            keep = self.target_reason_to_keep(target)
            if keep:
                kept += 1
                self.say(f"kept {self.job.shown(target)}: {keep}")
                continue
            total += self.clean_target(target)
        self.say(f"target directories: {self.job.would('freed', 'free')} "
                 f"{pmc.gib(total)}, kept {kept} whole")

    def scratch_reason_to_keep(self, path, worktrees):
        """Why a stale scratch subdirectory stays, or None."""
        root = os.path.normcase(os.path.realpath(path))
        for wt in worktrees:
            if wt == root or wt.startswith(root + os.sep):
                return f"holds the worktree {pmc.slashed(wt)}"
        checkout = checkout_inside(path)
        if checkout:
            return f"holds a git checkout ({pmc.slashed(checkout)})"
        pids = self.job.live_processes(path)
        if pids:
            return f"live processes (pids {', '.join(pids)})"
        return None

    def scratch_pass(self, root, delete_scratch):
        """Report, and with `delete_scratch` delete, stale subdirectories of `root`."""
        if not os.path.isdir(root):
            self.say(f"scratch: {pmc.slashed(root)} does not exist")
            return
        if os.name == "nt":
            self.say("scratch: kept everything: Windows lists only executables, not working "
                     "directories or open files, so no scratch folder is deleted there")
            return
        self.job.load_processes()
        if self.job.processes is None:
            self.say(f"scratch: kept everything: could not list processes "
                     f"({self.job.process_error})")
            return
        worktrees = [os.path.normcase(os.path.realpath(wt["worktree"]))
                     for wt in self.job.worktrees()]
        dry = self.dry_run or not delete_scratch
        total, count = 0, 0
        for entry in entries(root):
            if not entry.is_dir(follow_symlinks=False) or last_modified(entry.path) >= self.cutoff:
                continue
            keep = self.scratch_reason_to_keep(entry.path, worktrees)
            if keep:
                self.say(f"scratch: kept {pmc.slashed(entry.path)}: {keep}")
                continue
            size = pmc.disk_bytes(entry.path)
            if not dry:
                try:
                    shutil.rmtree(entry.path)
                except OSError as error:
                    self.say(f"scratch: failed to delete {pmc.slashed(entry.path)}: {error}")
                    self.failures += 1
                    continue
            total, count = total + size, count + 1
            self.say(f"scratch: {'would delete' if dry else 'deleted'} {pmc.slashed(entry.path)}: "
                     f"{pmc.gib(size)}")
        hint = " (--delete-scratch deletes them)" if dry and not self.dry_run else ""
        self.say(f"scratch: {count} subdirectories unmodified for {self.days} days, "
                 f"{pmc.gib(total)}{hint}")

    def execute(self, scratch=None, delete_scratch=False):
        stamp = datetime.datetime.now().isoformat(timespec="seconds")
        mode = " (dry run)" if self.dry_run else ""
        self.say(f"clean-stale{mode}: build output unused for {self.days} days, "
                 f"in {pmc.slashed(self.job.main)} at {stamp}")
        self.target_pass()
        if scratch:
            self.scratch_pass(scratch, delete_scratch)
        self.job.disk_report()
        if self.failures:
            self.say(f"clean-stale: {self.failures} deletes failed")
            return 1
        return 0


def cli(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--days", type=int, default=DEFAULT_DAYS,
                        help=f"unused for this many days is stale (default {DEFAULT_DAYS}, "
                             f"min {MIN_DAYS})")
    parser.add_argument("--dry-run", action="store_true", help="report, delete nothing")
    parser.add_argument("--scratch", action="store_true",
                        help="also list stale subdirectories of the agent scratch folder")
    parser.add_argument("--delete-scratch", action="store_true",
                        help="delete the stale scratch subdirectories --scratch lists")
    parser.add_argument("--scratch-dir",
                        default=os.path.join(os.path.expanduser("~"), ".cache", "stepclimb-tmp"),
                        help="the agent scratch folder (default ~/.cache/stepclimb-tmp)")
    parser.add_argument("--cwd", default=os.getcwd(), help="checkout to start from")
    args = parser.parse_args(argv)
    if args.days < MIN_DAYS:
        parser.error(f"--days must be at least {MIN_DAYS}: under relatime an access time can "
                     "be a day behind the last read")
    if args.delete_scratch and not args.scratch:
        parser.error("--delete-scratch needs --scratch")
    try:
        job = CleanStale(args.cwd, args.days, args.dry_run)
    except RuntimeError as error:
        print(f"clean-stale: {error}", file=sys.stderr)
        return 1
    _lock = pmc.locked(job.job.common)
    return job.execute(args.scratch_dir if args.scratch else None, args.delete_scratch)


if __name__ == "__main__":
    sys.exit(cli(sys.argv[1:]))
