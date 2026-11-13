# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared helpers for the Python hook scripts: read the hook JSON, find the repo root, block."""

import json
import os
import subprocess
import sys


def read_input():
    """The hook payload as a dict. Raises ValueError when stdin is not a JSON object."""
    data = json.load(sys.stdin)
    if not isinstance(data, dict):
        raise ValueError("hook input is not a JSON object")
    return data


def repo_root(path):
    """The nearest ancestor of `path` holding `.git` (a directory, or a file in a worktree)."""
    current = os.path.abspath(path)
    if not os.path.isdir(current):
        current = os.path.dirname(current)
    while True:
        if os.path.exists(os.path.join(current, ".git")):
            return current
        parent = os.path.dirname(current)
        if parent == current:
            return None
        current = parent


def repo_roots(path):
    """Every ancestor of `path` holding `.git`, innermost first.

    A nested repository (a `.git` inside a subfolder, or a symlinked folder that is a repo) and
    an agent worktree inside the main checkout each add a root; a guard checks the path against
    all of them.
    """
    roots = []
    current = os.path.abspath(path)
    if not os.path.isdir(current):
        current = os.path.dirname(current)
    while True:
        if os.path.exists(os.path.join(current, ".git")):
            roots.append(current)
        parent = os.path.dirname(current)
        if parent == current:
            return roots
        current = parent


def current_branch(cwd):
    """The checked-out branch in `cwd`, or "" when detached, not a repo, or git is missing."""
    try:
        out = subprocess.run(
            ["git", "-C", cwd, "branch", "--show-current"],
            capture_output=True, text=True, timeout=5, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return out.stdout.strip()


def tool_input(data):
    """The payload's `tool_input` object. Raises ValueError when it is present but not an object."""
    value = data.get("tool_input", {})
    if not isinstance(value, dict):
        raise ValueError("tool_input is not a JSON object")
    return value


def block(hook, message):
    """Print the reason to stderr and exit 2, which makes Claude Code and Codex refuse the call."""
    print(f"{hook}: blocked; {message}", file=sys.stderr)
    sys.exit(2)


def run_guard(hook, main):
    """Run a guard hook's `main`, blocking on any unexpected error.

    Both tools refuse a tool call only on exit 2; a traceback exits 1 and lets the call through,
    so a guard that crashes must block instead.
    """
    try:
        main()
    except SystemExit:
        raise
    except Exception as error:  # noqa: BLE001 - any failure in a guard must block
        block(hook, f"the hook failed ({type(error).__name__}: {error}), so the call was not checked")
