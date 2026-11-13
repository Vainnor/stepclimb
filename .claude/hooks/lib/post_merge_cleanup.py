# SPDX-License-Identifier: AGPL-3.0-or-later
"""Remove merged agent worktrees and merged branches; clear merged `target/`.

Run from any checkout of this repository: `.claude/scripts/post-merge-cleanup.sh [--dry-run]`.
`/cleanup` runs it by hand; this file is the one implementation. The PostToolUse(Bash) hook
`.claude/hooks/post-merge-cleanup.sh` runs it in the background after a `gh pr merge` (`--hook`)
with `--keep-worktrees`: the automatic run never removes a worktree or its branches,
because an agent that looks finished may still resume in it. It deletes
merged local branches and the merged PR's remote branch, clears merged worktrees' `target/`
directories, and prunes; worktrees go only by hand. `--targets-only` clears `target/`
directories and reports disk space, and does nothing else.

What goes, and only when every check passes:
- a worktree under `<main checkout>/.claude/worktrees/` whose branch has a merged or closed PR and
  no open one, or whose detached HEAD is in `origin/next`. It must not be on `main` or `next`,
  must not be the checkout the script was started from, and must not be locked by a live
  process: a lock whose pid is dead is stale and is unlocked, and a lock naming no pid is a
  person's and keeps it. The harness's lock reason names the Claude Code session's pid, not the
  agent's, so a worktree still locked while that session runs is kept, and leftovers during a
  session are expected. Evidence points at the harness unlocking a worktree when its agent
  finishes (one agent worktree here lost its lock mid-session while the others kept theirs),
  which makes it cleanable. No process may run an executable from inside it, have its working
  directory there or hold a file there open (`ps` and `lsof -n -P`; on Windows, executables only,
  from Win32_Process; nothing is ever killed). When `ps` or `lsof` is missing, fails, times out
  or lists no process, every worktree and `target/` stays, with that named as the reason. It must have no changed, untracked or ignored file
  other than `target/` directories, and a HEAD already in `origin/next`, `origin/<branch>` or the
  PR's head commit. With it go its branch,
  and the harness's `worktree-<dir name>` branch;
- in every mode, the `target/` directory at the top of any other worktree in `git worktree list`,
  wherever it lives (T3's `~/.t3/worktrees/` included). The checkout stays; cargo rebuilds
  `target/`. The worktree must not be the main checkout (the first entry `git worktree list`
  prints), must not be on `main` or `next`, must not be the checkout the script was started
  from, must not be locked by a live pid or by hand, and no process may use it, as above, by a
  process table read again just before each `target/` is judged. A `target/` that is a symlink
  or a junction stays. Its branch must have a merged or closed PR and no open one; a detached
  HEAD must be the head commit of such a PR, with no open PR on that PR's branch. Changed files
  and unpushed commits do not keep it, since `target/` holds only build output. On Windows,
  where only executables are checked, every `target/` stays until a working-directory check
  exists there. The report names each one cleared with the space it held (allocated blocks, a
  hard-linked file once) and each one kept with the reason, a failed delete among them;
- any other local branch with a merged or closed PR, no open one, and no commit outside
  `origin/next`, `origin/<branch>` or a PR head, except `main`, `next`, the current branch and
  branches checked out in a worktree that stays;
- with `--pr`, the merged PR's branch on `origin`, only while it still points at the PR's head
  (`git push --force-with-lease` makes the check and the delete one step).

With `--keep-worktrees` every worktree under `.claude/worktrees/` stays, with its branch,
reported as kept; its `target/` is still cleared when it passes the checks above.

Everything else stays and is reported with the reason. Remote branches are otherwise untouched.
Every run ends with the free space on the repository's filesystem and, on WSL, on each Windows
drive holding a WSL virtual disk (an `ext4.vhdx` under a user's `AppData/Local/wsl/`, or failing
that a Store distro's `AppData/Local/Packages/*/LocalState/`), warning for any under 10 % free.
Which of the virtual disks is this distro's is not known from inside it, so every such drive is
reported. `--dry-run` changes nothing, but it still runs `git fetch origin` (without `--prune`).
`--targets-only` does not fetch. A lock in the git common directory serializes concurrent runs,
so running it twice is the same as running it once.
"""

import argparse
import datetime
import errno
import glob
import json
import os
import re
import shlex
import shutil
import subprocess
import sys

if os.name == "nt":
    import ctypes
    import msvcrt
    from ctypes import wintypes
else:
    import fcntl

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib

PROTECTED = {"main", "next"}
LOG_NAME = "stepclimb-post-merge-cleanup.log"
LOCK_NAME = "stepclimb-post-merge-cleanup.lock"
# A drive with less free space than this share of its size is reported with a warning.
LOW_FREE_SHARE = 0.10
# Where WSL keeps a distro's virtual disk on a Windows drive, below the drive's mount: the
# `wsl --install` location first, then the Microsoft Store one.
WSL_DISK_PATTERNS = (
    ("Users", "*", "AppData", "Local", "wsl", "*", "ext4.vhdx"),
    ("Users", "*", "AppData", "Local", "Packages", "*", "LocalState", "ext4.vhdx"),
)
# More PRs than this and the oldest branches look PR-less, which keeps them: the safe direction.
PR_LIST_LIMIT = "1000"
# `gh pr merge` options that take a value, so the PR argument is found after them.
GH_MERGE_VALUE_OPTS = {
    "-b", "--body", "-F", "--body-file", "-t", "--subject", "--match-head-commit",
    "-A", "--author-email", "-R", "--repo",
}


def run(args, cwd, timeout=300):
    """Run a command; a missing program or a timeout is a failed result, never an exception.

    On Windows the program is looked up through PATHEXT, as a shell would, so a `.cmd` or `.bat`
    program runs; CreateProcess alone finds only `.exe`.
    """
    if os.name == "nt":
        args = [shutil.which(args[0]) or args[0], *args[1:]]
    try:
        return subprocess.run(
            args, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return subprocess.CompletedProcess(args, 127, "", str(error))


# --- Matching `gh pr merge` in a shell command ----------------------------------------------


def simple_commands(command):
    """Split a shell command line into simple commands, each a list of words.

    Quotes are honored before splitting, so `;`, `&`, `|`, parentheses and newlines inside a
    quoted string stay in their word. A line shlex cannot parse falls back to whitespace words.
    """
    lex = shlex.shlex(command, posix=True, punctuation_chars=";&|()\n")
    lex.whitespace = " \t\r"
    lex.whitespace_split = True
    lex.commenters = "#"
    try:
        words = list(lex)
    except ValueError:
        words = command.split()
    commands, current = [], []
    for word in words:
        if word and all(c in ";&|()\n" for c in word):
            commands.append(current)
            current = []
        else:
            current.append(word)
    commands.append(current)
    return [c for c in commands if c]


# Words that run the command after them: `time gh pr merge 1` merges.
COMMAND_PREFIXES = {"time", "command", "exec", "nohup", "env", "builtin"}
SHELLS = {"sh", "bash", "zsh", "fish", "dash"}


def strip_prefixes(words):
    while words and (words[0] in COMMAND_PREFIXES or words[0] == "$"
                     or re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[0])):
        words = words[1:]
    return words


def gh_merge_args(words):
    """(selector, repo) when `words` run `gh pr merge`, else None."""
    if len(words) < 3 or os.path.basename(words[0]) != "gh":
        return None
    rest, repo = words[1:], ""
    while len(rest) > 1 and rest[0] in ("-R", "--repo"):
        repo, rest = rest[1], rest[2:]
    if rest[:2] != ["pr", "merge"]:
        return None
    selector, i, args = "", 0, rest[2:]
    while i < len(args):
        arg = args[i]
        if arg in GH_MERGE_VALUE_OPTS:
            if arg in ("-R", "--repo") and i + 1 < len(args):
                repo = args[i + 1]
            i += 2
            continue
        if arg.startswith(("--repo=", "-R=")):
            repo = arg.split("=", 1)[1]
        elif not arg.startswith("-"):
            selector = selector or arg
        i += 1
    return selector, repo


def merge_invocations(command, depth=0):
    """The `gh pr merge` calls in a shell command line, as (selector, repo, cd) triples.

    The selector is "" when the command names no PR (gh then merges the current branch's PR).
    `cd` is the last `cd` target before the call in the same line, or None. `bash -c "..."` and
    `$(...)` inside a word are searched too.
    """
    found, cd = [], None
    for raw in simple_commands(command):
        if depth < 3:
            for word in raw:
                for inner in re.findall(r"\$\(([^()]*)\)", word):
                    found += [(s, r, c if c is not None else cd)
                              for s, r, c in merge_invocations(inner, depth + 1)]
        words = strip_prefixes(raw)
        if not words:
            continue
        if words[0] == "cd":
            cd = words[1] if len(words) > 1 else "~"
            continue
        if depth < 3 and words[0] in SHELLS and "-c" in words[:-1]:
            inner = words[words.index("-c") + 1]
            found += [(s, r, c if c is not None else cd)
                      for s, r, c in merge_invocations(inner, depth + 1)]
            continue
        merge = gh_merge_args(words)
        if merge:
            found.append((*merge, cd))
    return found


# --- Repository state -----------------------------------------------------------------------


def is_target_dir(path):
    """True for a cargo `target/` directory as `git status --ignored` lists it."""
    return path.endswith("/") and path.rstrip("/").split("/")[-1] == "target"


def lock_pid(reason):
    """The pid in a worktree lock reason, as the harness writes it: `... (pid 79142 start ...)`."""
    match = re.search(r"\bpid (\d+)\b", reason)
    return int(match.group(1)) if match and int(match.group(1)) > 0 else None


def pid_alive(pid):
    """True while process `pid` exists, including one this user may not signal.

    A pid too large for the OS (`OverflowError`) cannot be checked, so it counts as alive and
    the worktree stays.
    """
    if os.name == "nt":
        return windows_pid_alive(pid)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except (PermissionError, OverflowError):
        return True
    return True


# Every process as "<pid>\t<executable path>", UTF-8. The path is empty where Windows withholds it.
WINDOWS_PROCESSES = (
    "[Console]::OutputEncoding = [Text.Encoding]::UTF8; "
    "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId)`t$($_.ExecutablePath)\" }"
)

# Windows process access rights and wait results (Win32 API).
PROCESS_SYNCHRONIZE_AND_QUERY = 0x00100000 | 0x1000  # SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
ERROR_INVALID_PARAMETER = 87
WAIT_TIMEOUT = 0x102


def open_failure_means_alive(error):
    """Whether an OpenProcess failure with Win32 error `error` leaves the process counted alive."""
    return error != ERROR_INVALID_PARAMETER


def windows_pid_alive(pid):
    """`pid_alive` on Windows, where `os.kill(pid, 0)` would terminate the process.

    OpenProcess fails with an invalid parameter for a pid that names no process (dead); any
    other failure, access denied for a live process this user may not open among them, counts as
    alive, so the worktree stays. A pid above the 32-bit range cannot be checked, so it counts as
    alive too.
    """
    if pid > 0xFFFFFFFF:
        return True
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    handle = kernel32.OpenProcess(PROCESS_SYNCHRONIZE_AND_QUERY, False, pid)
    if not handle:
        return open_failure_means_alive(ctypes.get_last_error())
    try:
        return kernel32.WaitForSingleObject(handle, 0) == WAIT_TIMEOUT
    finally:
        kernel32.CloseHandle(handle)


# --- Disk space -----------------------------------------------------------------------------


def disk_bytes(path):
    """The space the files under `path` take on disk, a hard-linked file counted once.

    POSIX counts allocated blocks; Windows, which reports none, counts file sizes. Windows leaves
    st_ino and st_dev zero in a directory entry's stat, so there each file is stat'ed by path to
    read its file index. Nothing is followed through a symbolic link, and a file that vanishes
    during the walk is skipped.
    """
    total, seen, stack = 0, set(), [path]
    while stack:
        try:
            entries = list(os.scandir(stack.pop()))
        except OSError:
            continue
        for entry in entries:
            try:
                if entry.is_dir(follow_symlinks=False):
                    stack.append(entry.path)
                    continue
                st = os.lstat(entry.path) if os.name == "nt" else entry.stat(follow_symlinks=False)
            except OSError:
                continue
            if st.st_ino and (st.st_dev, st.st_ino) in seen:
                continue
            seen.add((st.st_dev, st.st_ino))
            total += st.st_blocks * 512 if hasattr(st, "st_blocks") else st.st_size
    return total


def is_link(path):
    """True for a symbolic link or, on Windows, a junction: a `target/` the cleanup never follows."""
    return os.path.islink(path) or getattr(os.path, "isjunction", lambda _: False)(path)


def slashed(path):
    """`path` as the report prints it: normalized, with `/` separators on every OS."""
    return os.path.normpath(path).replace(os.sep, "/")


def gib(size):
    return f"{size / 2**30:.1f} GiB"


def disk_line(label, free, total):
    """One line of the disk report; a warning when under LOW_FREE_SHARE of the drive is free."""
    share = free / total if total else 0.0
    line = f"disk: {label}: {gib(free)} free of {gib(total)} ({share:.0%})"
    if share < LOW_FREE_SHARE:
        line += f"; WARNING: under {LOW_FREE_SHARE:.0%} free"
    return line


def on_wsl():
    """True inside a WSL distro, whose kernel release names Microsoft."""
    try:
        with open("/proc/sys/kernel/osrelease") as f:
            return "microsoft" in f.read().lower()
    except OSError:
        return False


def wsl_disk_mounts(root="/mnt"):
    """The mounts under `root` of Windows drives (`/mnt/c`) that hold a WSL virtual disk."""
    for pattern in WSL_DISK_PATTERNS:
        disks = glob.glob(os.path.join(glob.escape(root), "?", *pattern))
        if disks:
            drives = {os.path.relpath(d, root).replace(os.sep, "/").split("/")[0] for d in disks}
            return [os.path.join(root, d) for d in sorted(drives)]
    return []


class Cleanup:
    """One run: the repository, what it found, and what it did."""

    def __init__(self, start, dry_run, keep_worktrees=False):
        self.dry_run = dry_run
        self.keep_worktrees = keep_worktrees
        self.lines = []
        top = run(["git", "rev-parse", "--show-toplevel"], start)
        common = run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], start)
        listed = run(["git", "worktree", "list", "--porcelain"], start)
        first = listed.stdout.splitlines()[:1]
        if top.returncode != 0 or common.returncode != 0 or listed.returncode != 0 \
                or not first or not first[0].startswith("worktree "):
            raise RuntimeError(f"{start} is not inside a git checkout")
        self.start = os.path.realpath(top.stdout.strip())
        self.common = os.path.realpath(common.stdout.strip())
        # git lists the main worktree first; its directory need not be the common dir's parent.
        self.main = os.path.realpath(first[0].removeprefix("worktree "))
        self.worktree_root = os.path.join(self.main, ".claude", "worktrees")
        self.current_branch = run(["git", "branch", "--show-current"], start).stdout.strip()
        self.prs = None
        self.pr_error = ""
        self.processes = None
        self.process_error = ""

    def say(self, line):
        self.lines.append(line)
        print(line, flush=True)

    def git(self, *args, cwd=None):
        return run(["git", *args], cwd or self.main)

    def would(self, done, todo):
        return f"would {todo}" if self.dry_run else done

    # PRs and commits

    def load_prs(self):
        """Every PR by head branch name, from one `gh pr list`; None when gh failed."""
        out = run(
            ["gh", "pr", "list", "--state", "all", "--limit", PR_LIST_LIMIT,
             "--json", "number,state,headRefName,headRefOid,url"],
            self.main, timeout=60,
        )
        if out.returncode != 0:
            self.pr_error = ((out.stderr or out.stdout).strip().splitlines() or ["no output"])[-1]
            return
        try:
            listed = json.loads(out.stdout)
        except ValueError:
            self.pr_error = "gh printed something that is not JSON"
            return
        self.prs = {}
        for pr in listed:
            self.prs.setdefault(pr["headRefName"], []).append(pr)

    def has_commit(self, oid):
        return bool(oid) and self.git("cat-file", "-e", f"{oid}^{{commit}}").returncode == 0

    def contains(self, ref, oid):
        """True when commit `oid` is reachable from `ref`."""
        return self.git("merge-base", "--is-ancestor", oid, ref).returncode == 0

    def pushed(self, oid, branch, prs):
        """True when `oid` is in origin/next, origin/<branch> or one of the PRs' head commits."""
        refs = ["refs/remotes/origin/next"]
        if branch and self.git("rev-parse", "-q", "--verify", f"refs/remotes/origin/{branch}").returncode == 0:
            refs.append(f"refs/remotes/origin/{branch}")
        refs += [pr["headRefOid"] for pr in prs if self.has_commit(pr.get("headRefOid"))]
        return any(self.contains(ref, oid) for ref in refs)

    def pr_verdict(self, branch):
        """(prs, reason to keep or None) for a branch, judged by its PRs alone."""
        if self.prs is None:
            return [], f"PR state unknown (gh failed: {self.pr_error})"
        prs = self.prs.get(branch, [])
        if not prs:
            return prs, "no PR"
        open_ = [f"#{p['number']}" for p in prs if p["state"] == "OPEN"]
        if open_:
            return prs, f"PR {', '.join(open_)} is open"
        return prs, None

    @staticmethod
    def describe(prs):
        return ", ".join(f"PR #{p['number']} {p['state'].lower()}" for p in prs)

    # Worktrees

    def worktrees(self):
        out = self.git("worktree", "list", "--porcelain")
        entries, entry = [], {}
        for line in out.stdout.splitlines() + [""]:
            if not line:
                if entry:
                    entries.append(entry)
                entry = {}
                continue
            key, _, value = line.partition(" ")
            entry[key] = value
        return entries

    # Processes

    def load_processes(self):
        """Snapshot every process as (pid, executable, [cwd and open files]); None on failure.

        Each call reads the table again; the note on what could not be checked is said once.
        """
        self.processes, self.process_error = None, ""
        if os.name == "nt":
            self.load_windows_processes()
        else:
            self.load_posix_processes()

    def note_once(self, line):
        if line not in self.lines:
            self.say(line)

    def load_posix_processes(self):
        """The process table from `ps` and `lsof`; None when either failed.

        The executable is the first word of `ps`'s command column. `lsof` adds each process's
        working directory and open files. Without them a build in a worktree goes unseen (cargo
        and rustc run from ~/.cargo), so a missing, failed or timed-out lsof, or one that lists
        no process (it always lists at least this one), leaves the table None and every worktree
        and `target/` kept.
        """
        out = run(["ps", "-axo", "pid=,command="], self.main, 60)
        if out.returncode != 0:
            self.process_error = f"ps failed: {out.stderr.strip()}"
            return
        files, pid = {}, None
        # -n and -P skip host and port name lookups, which can stall on a network timeout.
        listed = run(["lsof", "-w", "-n", "-P", "-F", "pn"], self.main, 60)
        for line in listed.stdout.splitlines():
            if line.startswith("p"):
                pid = line[1:]
                files.setdefault(pid, [])
            elif line.startswith("n") and pid:
                files[pid].append(line[1:])
        if listed.returncode != 0 or not files:
            why = (listed.stderr.strip().splitlines() or ["it listed no process"])[-1]
            self.process_error = f"lsof failed (exit {listed.returncode}): {why}"
            return
        self.processes = []
        for line in out.stdout.splitlines():
            fields = line.split(None, 2)
            if len(fields) >= 2:
                self.processes.append((fields[0], fields[1], files.get(fields[0], [])))

    def load_windows_processes(self):
        """The process table from Win32_Process through PowerShell; None when that failed.

        Windows has no documented way to read another process's working directory, so only
        executables are checked, and the report says so. That is too little to delete a
        `target/` under a build, so `target_verdict` keeps every one on Windows.
        """
        out = run(["powershell", "-NoProfile", "-NonInteractive", "-Command", WINDOWS_PROCESSES],
                  self.main, 60)
        if out.returncode != 0:
            self.process_error = f"powershell failed: {out.stderr.strip()}"
            return
        self.note_once("Windows lists no working directories; only executables were checked")
        self.processes = []
        for line in out.stdout.splitlines():
            pid, _, exe = line.partition("\t")
            if pid.strip():
                self.processes.append((pid.strip(), exe.strip(), []))

    def live_processes(self, path):
        """Pids whose executable, working directory or an open file is inside `path`."""
        # normcase: Windows paths ignore case and take either slash; elsewhere it changes nothing.
        roots = {os.path.normcase(os.path.abspath(path)), os.path.normcase(os.path.realpath(path))}

        def inside(p):
            p = os.path.normcase(p)
            return bool(p) and any(p == r or p.startswith(r + os.sep) for r in roots)

        return [pid for pid, exe, files in self.processes
                if inside(exe) or any(inside(f) for f in files)]

    def worktree_verdict(self, wt):
        """(remove?, reason, prs) for one worktree under .claude/worktrees/."""
        path = wt["worktree"]
        branch = wt.get("branch", "").removeprefix("refs/heads/")
        keep = self.local_reason_to_keep(wt, path, branch)
        if keep:
            return False, keep, []
        head = wt.get("HEAD", "")
        if not branch:
            if self.contains("refs/remotes/origin/next", head):
                return True, "detached HEAD is in origin/next", []
            return False, "detached HEAD is not in origin/next", []
        prs, keep = self.pr_verdict(branch)
        if keep:
            return False, f"{branch}: {keep}", prs
        if not self.pushed(head, branch, prs):
            return False, f"{branch}: unpushed commits ({self.describe(prs)})", prs
        return True, f"{branch}: {self.describe(prs)}", prs

    def local_reason_to_keep(self, wt, path, branch):
        """Why this machine's state keeps the worktree, whatever its PR says; None if nothing."""
        return self.use_reason_to_keep(wt, path, branch) or self.status_reason_to_keep(path)

    def use_reason_to_keep(self, wt, path, branch):
        """Why the worktree may still be in use: its branch, a lock, a process; None if nothing."""
        if branch in PROTECTED:
            return f"it has {branch} checked out"
        if os.path.realpath(path) == self.start:
            return "the cleanup was started from it"
        if "prunable" in wt or not os.path.isdir(path):
            return "its directory is missing (git worktree prune clears it)"
        if "locked" in wt:
            pid = lock_pid(wt["locked"])
            if pid is None:
                return f"locked (no pid): {wt['locked'] or '(no reason)'}"
            if pid_alive(pid):
                return f"locked by live pid {pid}"
        if self.processes is None:
            return f"could not list processes ({self.process_error})"
        pids = self.live_processes(path)
        if pids:
            return f"{len(pids)} live processes (pids {', '.join(pids)})"
        return None

    def status_reason_to_keep(self, path):
        """Changed, untracked or ignored files that `git worktree remove` would delete.

        `--ignored=matching` lists `crates/a/target/` itself; the default mode would fold it into
        `crates/`, which would hide whether anything but build output is there.
        """
        status =self.git("status", "--porcelain", "--ignored=matching", cwd=path)
        if status.returncode != 0:
            return "git status failed"
        lines = status.stdout.splitlines()
        changed = [line for line in lines if not line.startswith("!! ")]
        if changed:
            return f"dirty: {len(changed)} changed or untracked"
        ignored = [line[3:] for line in lines if line.startswith("!! ")]
        kept = [p for p in ignored if not is_target_dir(p)]
        if kept:
            shown = ", ".join(kept[:5]) + (f" and {len(kept) - 5} more" if len(kept) > 5 else "")
            return f"ignored files that removal would delete: {shown}"
        return None

    def delete_branch(self, branch, indent=""):
        if branch in PROTECTED:
            self.say(f"{indent}kept branch {branch}: protected")
            return False
        if not self.dry_run:
            out = self.git("branch", "-D", branch)
            if out.returncode != 0:
                self.say(f"{indent}failed to delete branch {branch}: {out.stderr.strip()}")
                return False
        self.say(f"{indent}{self.would('deleted', 'delete')} branch {branch}")
        return True

    def remove_worktree(self, wt, reason):
        """Remove one worktree that passed its checks; returns the branches it took with it."""
        path = wt["worktree"]
        rel = self.shown(path)
        if not self.dry_run:
            if "locked" in wt:
                self.git("worktree", "unlock", path)
            out = self.git("worktree", "remove", path)
            if out.returncode != 0:
                if "locked" in wt:
                    self.git("worktree", "lock", "--reason", wt["locked"], path)
                self.say(f"kept worktree {rel}: git worktree remove failed: {out.stderr.strip()}")
                return set()
        self.say(f"{self.would('removed', 'remove')} worktree {rel} ({reason})")
        if "locked" in wt:
            self.say(f"  {self.would('unlocked', 'unlock')} its stale lock: {wt['locked'] or '(no reason)'}")
        gone = set()
        branch = wt.get("branch", "").removeprefix("refs/heads/")
        if branch and self.delete_branch(branch, "  "):
            gone.add(branch)
        harness = "worktree-" + os.path.basename(path)
        tip = self.git("rev-parse", "-q", "--verify", f"refs/heads/{harness}").stdout.strip()
        if tip and harness != branch:
            if self.contains("refs/remotes/origin/next", tip) or self.contains(wt["HEAD"], tip):
                if self.delete_branch(harness, "  "):
                    gone.add(harness)
            else:
                self.say(f"  kept branch {harness}: commits outside origin/next")
        return gone

    # Build output

    def shown(self, path):
        """`path` relative to the main checkout when inside it, else absolute; `/` separators."""
        try:
            rel = os.path.relpath(path, self.main)
        except ValueError:  # another drive on Windows
            rel = os.pardir
        if rel == os.pardir or rel.startswith(os.pardir + os.sep):
            rel = path
        return slashed(rel)

    def target_verdict(self, wt, target):
        """(reason to keep or None, why it may go) for `target`, the `target/` of worktree `wt`."""
        path = wt["worktree"]
        if os.path.realpath(path) == os.path.realpath(self.main):
            return "it is the main checkout", ""
        if is_link(target):
            return "target is a symlink", ""
        branch = wt.get("branch", "").removeprefix("refs/heads/")
        keep = self.use_reason_to_keep(wt, path, branch)
        if keep:
            return keep, ""
        keep, why = self.merged_verdict(wt, branch)
        if not keep and os.name == "nt":
            keep = ("Windows lists only executables, not working directories or open files, "
                    "so no target/ is deleted there")
        return keep, why

    def merged_verdict(self, wt, branch):
        """(reason to keep or None, why it may go), judged by the PRs of the worktree's HEAD."""
        if branch:
            prs, keep = self.pr_verdict(branch)
            return (f"{branch}: {keep}", "") if keep else (None, f"{branch}: {self.describe(prs)}")
        if self.prs is None:
            return f"PR state unknown (gh failed: {self.pr_error})", ""
        head = wt.get("HEAD", "")
        heads = sorted({p["headRefName"] for ps in self.prs.values() for p in ps
                        if p.get("headRefOid") == head})
        if not heads:
            return "detached HEAD is no PR's head commit", ""
        for name in heads:
            prs, keep = self.pr_verdict(name)
            if keep:
                return f"detached HEAD is {name}'s head: {keep}", ""
        return None, f"detached HEAD is {heads[0]}'s head: {self.describe(self.prs[heads[0]])}"

    def clear_target(self, target, why):
        """Delete one `target/` that passed its checks; the bytes it held, or None on failure."""
        size = disk_bytes(target)
        if not self.dry_run:
            try:
                shutil.rmtree(target)
            except OSError as error:
                self.say(f"failed to clear {self.shown(target)} ({why}): {error}")
                return None
        self.say(f"{self.would('cleared', 'clear')} {self.shown(target)} ({why}): "
                 f"{gib(size)}{'' if self.dry_run else ' freed'}")
        return size

    def target_pass(self, removed):
        """Clear the `target/` of every merged worktree that stays; `removed` are paths gone.

        The process table is read again for each `target/`, just before it is judged and
        cleared, so a build started during the run is seen. A failed delete counts as kept.
        """
        cleared, kept, freed = 0, 0, 0
        for wt in self.worktrees():
            if os.path.realpath(wt["worktree"]) in removed:
                continue
            target = os.path.join(wt["worktree"], "target")
            if not (os.path.isdir(target) or is_link(target)):
                continue
            self.load_processes()
            keep, why = self.target_verdict(wt, target)
            size = None if keep else self.clear_target(target, why)
            if keep:
                self.say(f"kept {self.shown(target)}: {keep}")
            if size is None:
                kept += 1
            else:
                cleared, freed = cleared + 1, freed + size
        self.say(f"target directories: {self.would('cleared', 'clear')} {cleared} "
                 f"({gib(freed)}), kept {kept}")

    def disk_report(self):
        """Free space on the repository's filesystem and, on WSL, on the Windows drives."""
        places = [(f"{slashed(self.main)} filesystem", self.main)]
        if on_wsl():
            mounts = wsl_disk_mounts()
            if not mounts:
                self.say("disk: no Windows drive under /mnt holds a WSL virtual disk")
            places += [(f"Windows drive {os.path.basename(m).upper()}: ({m}, holds a WSL "
                        "virtual disk)", m) for m in mounts]
        for label, path in places:
            try:
                usage = shutil.disk_usage(path)
            except OSError as error:
                self.say(f"disk: {label}: unreadable ({error})")
                continue
            self.say(disk_line(label, usage.free, usage.total))

    # Branches

    def branch_pass(self, gone, busy):
        out = self.git("for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads/")
        for line in out.stdout.splitlines():
            branch, _, tip = line.partition(" ")
            if branch in gone or branch.startswith("worktree-agent-"):
                continue
            if branch in PROTECTED:
                continue
            if branch == self.current_branch:
                self.say(f"kept branch {branch}: checked out where the cleanup started")
                continue
            if branch in busy:
                self.say(f"kept branch {branch}: checked out in a kept worktree")
                continue
            prs, keep = self.pr_verdict(branch)
            if keep:
                self.say(f"kept branch {branch}: {keep}")
            elif not self.pushed(tip, branch, prs):
                self.say(f"kept branch {branch}: unpushed commits ({self.describe(prs)})")
            else:
                self.delete_branch(branch)

    # The merged PR's remote branch

    def origin_repo(self):
        # The configured URL, before any `insteadOf` rewrite, names the GitHub repository.
        url = self.git("config", "--get", "remote.origin.url").stdout.strip()
        match = re.search(r"github\.com[:/]+([^/]+/[^/]+?)(?:\.git)?/?$", url)
        return match.group(1).lower() if match else None

    def delete_remote_branch(self, selector, repo, start):
        """Delete the merged PR's branch on origin while it still points at the PR's head."""
        args = ["gh", "pr", "view", *([selector] if selector else []),
                "--json", "number,state,headRefName,headRefOid,url"]
        if repo:
            args += ["--repo", repo]
        out = run(args, start, timeout=60)
        if out.returncode != 0:
            self.say(f"remote branch left: gh pr view failed: {out.stderr.strip()}")
            return
        try:
            pr = json.loads(out.stdout)
        except ValueError:
            self.say("remote branch left: gh pr view printed something that is not JSON")
            return
        branch, number = pr["headRefName"], pr["number"]
        url_repo = re.search(r"github\.com/([^/]+/[^/]+)/pull/", pr.get("url", ""))
        if pr["state"] != "MERGED":
            self.say(f"remote branch left: PR #{number} is {pr['state'].lower()}, not merged")
        elif branch in PROTECTED:
            self.say(f"remote branch left: {branch} is protected")
        elif not url_repo or url_repo.group(1).lower() != self.origin_repo():
            self.say(f"remote branch left: PR #{number} is not in origin's repository")
        else:
            self.delete_origin_ref(branch, pr["headRefOid"], number)

    def delete_origin_ref(self, branch, head, number):
        remote = self.git("ls-remote", "origin", f"refs/heads/{branch}")
        tip = remote.stdout.split()[0] if remote.stdout.split() else ""
        if remote.returncode != 0:
            self.say(f"remote branch left: git ls-remote failed: {remote.stderr.strip()}")
        elif not tip:
            self.say(f"remote branch {branch}: already deleted (PR #{number})")
        elif tip != head:
            self.say(f"remote branch {branch} left: it has moved past PR #{number}'s head")
        elif self.dry_run:
            self.say(f"would delete remote branch origin/{branch} (PR #{number} merged)")
        else:
            # The lease makes the push fail if the branch moved after ls-remote read it.
            out = self.git("push", f"--force-with-lease=refs/heads/{branch}:{head}",
                           "origin", "--delete", branch)
            verb = "deleted" if out.returncode == 0 else f"failed to delete ({out.stderr.strip()})"
            self.say(f"{verb} remote branch origin/{branch} (PR #{number} merged)")

    # The whole run

    def execute(self, merged_pr=None, start=None, targets_only=False):
        stamp = datetime.datetime.now().isoformat(timespec="seconds")
        modes = [m for m, on in (("dry run", self.dry_run), ("targets only", targets_only)) if on]
        shown = f" ({', '.join(modes)})" if modes else ""
        self.say(f"post-merge cleanup{shown} in {slashed(self.main)} at {stamp}")
        if targets_only:
            self.load_prs()
            self.target_pass(set())
            self.disk_report()
            return 0
        fetch = ["fetch", "origin"] if self.dry_run else ["fetch", "--prune", "origin"]
        out = self.git(*fetch)
        if out.returncode != 0:
            self.say(f"stopped: git {' '.join(fetch)} failed: {out.stderr.strip()}")
            return 1
        if merged_pr is not None:
            self.delete_remote_branch(*merged_pr, start or self.main)
            self.git(*fetch)
        self.load_prs()
        self.load_processes()
        gone, busy, removed = set(), set(), set()
        root = os.path.realpath(self.worktree_root) + os.sep
        for wt in self.worktrees():
            if not os.path.realpath(wt["worktree"]).startswith(root):
                busy.add(wt.get("branch", "").removeprefix("refs/heads/"))
                continue
            if self.keep_worktrees:
                remove, reason = False, "worktrees are removed only by hand (/cleanup)"
            else:
                remove, reason, _ = self.worktree_verdict(wt)
            if remove:
                gone |= self.remove_worktree(wt, reason)
                if self.dry_run or not os.path.exists(wt["worktree"]):
                    removed.add(os.path.realpath(wt["worktree"]))
            else:
                self.say(f"kept worktree {self.shown(wt['worktree'])}: {reason}")
                busy.add(wt.get("branch", "").removeprefix("refs/heads/"))
        self.branch_pass(gone, busy)
        self.target_pass(removed)
        if not self.dry_run:
            self.git("worktree", "prune")
        self.say(f"{self.would('ran', 'run')} git worktree prune; remote branches otherwise untouched")
        self.disk_report()
        return 0


def locked(common):
    """Hold the run lock in the git common directory until the process exits."""
    handle = open(os.path.join(common, LOCK_NAME), "a")  # noqa: SIM115 - held for the run
    if os.name == "nt":
        # msvcrt locks the bytes from the file position on. The file is open for append, which
        # puts that position at the end, so move it to 0 first; Windows lets a program lock byte
        # 0 of an empty file.
        os.lseek(handle.fileno(), 0, os.SEEK_SET)
        while True:
            try:
                msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
                break
            except OSError as error:
                # LK_LOCK gives up with EDEADLOCK after 10 tries a second apart; flock waits on.
                if error.errno != errno.EDEADLOCK:
                    raise
    else:
        fcntl.flock(handle, fcntl.LOCK_EX)
    return handle


def cli(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dry-run", action="store_true", help="report, change nothing")
    parser.add_argument("--keep-worktrees", action="store_true",
                        help="leave every agent worktree; clean branches and still clear "
                             "merged worktrees' target/")
    parser.add_argument("--targets-only", action="store_true",
                        help="only clear merged worktrees' target/ and report disk space")
    parser.add_argument("--cwd", default=os.getcwd(), help="checkout to start from")
    parser.add_argument("--pr", help="merged PR (number, URL or branch; '' for the current one)")
    parser.add_argument("--repo", default="", help="gh --repo for --pr")
    parser.add_argument("--pr-from", help="directory gh resolves --pr in (default: --cwd)")
    args = parser.parse_args(argv)
    if args.targets_only and args.pr is not None:
        parser.error("--targets-only touches no branch, so it takes no --pr")
    try:
        job = Cleanup(args.cwd, args.dry_run, args.keep_worktrees)
    except RuntimeError as error:
        print(f"post-merge cleanup: {error}", file=sys.stderr)
        return 1
    _lock = locked(job.common)
    merged = (args.pr, args.repo) if args.pr is not None else None
    return job.execute(merged, args.pr_from or args.cwd, args.targets_only)


# --- PostToolUse(Bash) hook -----------------------------------------------------------------


def hook_context(text):
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PostToolUse", "additionalContext": text,
    }}))


def pr_arguments(cwd, selector, repo, cd):
    """The `--pr` arguments for one merge, or None when the PR cannot be pinned down.

    gh resolves a PR number or a bare merge in the directory it runs in, so after a `cd` the PR
    is looked up from the `cd` target. A target that is not an existing directory taken
    literally (`$VAR`, `~`, `-`, a typo) is ambiguous.
    """
    args = ["--pr", selector, "--repo", repo]
    if cd is None:
        return args
    target = os.path.normpath(os.path.join(cwd, cd))
    return args + ["--pr-from", target] if os.path.isdir(target) else None


def hook():
    """Start a background cleanup for each `gh pr merge` in the command; never fails the call.

    The run keeps every worktree (`--keep-worktrees`); only `/cleanup` removes one. It still
    clears the `target/` of each merged worktree that passes the checks in the module docstring.

    The background run checks the PR is merged before touching its remote branch, so a failed or
    `--auto` merge (Codex runs this hook after failed commands too) removes nothing it should not.
    """
    try:
        data = json.load(sys.stdin)
        command = data.get("tool_input", {}).get("command", "")
        merges = merge_invocations(command) if isinstance(command, str) else []
        if not merges:
            return
        cwd = data.get("cwd") or os.getcwd()
        common = run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd)
        if common.returncode != 0 or not common.stdout.strip():
            return  # not in a checkout: nothing of ours to clean, and nowhere to log
        log_path = os.path.join(os.path.realpath(common.stdout.strip()), LOG_NAME)
        with open(log_path, "a") as log:
            for selector, repo, cd in merges:
                pr_args = pr_arguments(cwd, selector, repo, cd)
                if pr_args is None:
                    log.write(f"remote branch left: `gh pr merge` ran after `cd {cd}`, "
                              "so which PR it merged is not known\n")
                    log.flush()
                    pr_args = []
                subprocess.Popen(  # noqa: S603 - our own script, list arguments
                    [sys.executable, "-X", "utf8", "-B", os.path.abspath(__file__), "--cwd", cwd,
                     "--keep-worktrees", *pr_args],
                    cwd=cwd, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
        hook_context(
            f"post-merge cleanup started in the background; its report is appended to {log_path}"
        )
    except Exception as error:  # noqa: BLE001 - a cleanup failure must never fail the session
        hook_context(f"post-merge cleanup did not start: {type(error).__name__}: {error}")


if __name__ == "__main__":
    if sys.argv[1:] == ["--hook"]:
        hook()
        sys.exit(0)
    sys.exit(cli(sys.argv[1:]))
