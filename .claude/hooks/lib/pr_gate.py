# SPDX-License-Identifier: AGPL-3.0-or-later
"""PreToolUse(Bash): refuse `gh pr create` until `just ci` has passed on the committed HEAD.

`just ci` ends with bin/ci-stamp.sh, which writes HEAD's commit to `<git dir>/stepclimb-ci-pass`
when the tree is clean. This hook allows a command that opens a PR only when that stamp names the
current HEAD and the tree is still clean, so what the PR shows is what passed. It reads the
command text only (as attribution-gate does): a PR opened by a script it cannot see is not
checked, and CI on the PR is the backstop.
"""

import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402

HOOK = "pr-gate"
OPENS_A_PR = re.compile(r"(^|[\s;&|(`])gh\s+(-R\s+\S+\s+|--repo[\s=]\S+\s+)?pr\s+create\b")


def git(cwd, *args):
    out = subprocess.run(["git", "-C", cwd, *args], capture_output=True, text=True, timeout=10,
                         check=False)
    if out.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {out.stderr.strip()}")
    return out.stdout.strip()


def verdict(cwd):
    """None when a PR may be opened from `cwd`, else the reason it may not."""
    git_dir = git(cwd, "rev-parse", "--absolute-git-dir")
    head = git(cwd, "rev-parse", "HEAD")
    try:
        with open(os.path.join(git_dir, "stepclimb-ci-pass"), encoding="utf-8") as f:
            stamp = f.read().strip()
    except FileNotFoundError:
        return "no `just ci` pass is recorded for this worktree; run `just ci` on the committed HEAD"
    if stamp != head:
        return (f"the last `just ci` pass was on {stamp[:12]}, not HEAD {head[:12]}; run `just ci` "
                "again")
    if git(cwd, "status", "--porcelain", "--untracked-files=normal"):
        return "the working tree has changes since `just ci` passed; commit them and run it again"
    return None


def main():
    try:
        data = hookio.read_input()
        command = hookio.tool_input(data).get("command", "") or ""
        if not isinstance(command, str):
            raise ValueError("tool_input.command is not a string")
    except ValueError:
        hookio.block(HOOK, "could not parse the hook input, so the command was not checked")
    if not OPENS_A_PR.search(command):
        sys.exit(0)
    reason = verdict(data.get("cwd") or os.getcwd())
    if reason:
        hookio.block(HOOK, reason)
    sys.exit(0)


if __name__ == "__main__":
    hookio.run_guard(HOOK, main)
