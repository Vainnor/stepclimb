# SPDX-License-Identifier: AGPL-3.0-or-later
"""SessionStart: print the branch, working-tree state, open PRs, worktrees and project status.

Stdout becomes session context. Every probe has a timeout and every failure is skipped, so the
hook never fails a session.
"""

import os
import re
import subprocess
import sys


def run(args, cwd, timeout=5):
    try:
        out = subprocess.run(
            args, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout.rstrip() if out.returncode == 0 else None


def status_line(root):
    try:
        with open(os.path.join(root, "AGENTS.md"), encoding="utf-8") as f:
            for line in f:
                if re.match(r"^\*\*Status:", line):
                    return line.strip()
    except OSError:
        pass
    return None


def main():
    root = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    lines = ["## Stepclimb session context"]
    branch = run(["git", "branch", "--show-current"], root)
    if branch is not None:
        lines.append(f"- Branch: {branch or '(detached)'}")
    status = run(["git", "status", "--short"], root)
    if status is not None:
        changed = status.splitlines()
        lines.append(f"- Working tree: {'clean' if not changed else f'{len(changed)} changed'}")
        lines.extend(f"    {c}" for c in changed[:10])
    if run(["gh", "auth", "status"], root) is not None:
        prs = run(
            ["gh", "pr", "list", "--limit", "10", "--json", "number,title,headRefName",
             "--template", "{{range .}}#{{.number}} {{.title}} ({{.headRefName}})\n{{end}}"],
            root,
        )
        if prs is not None:
            lines.append(f"- Open PRs: {'none' if not prs else ''}")
            lines.extend(f"    {p}" for p in prs.splitlines())
    worktrees = run(["git", "worktree", "list"], root)
    if worktrees is not None and len(worktrees.splitlines()) > 1:
        lines.append("- Worktrees:")
        lines.extend(f"    {w}" for w in worktrees.splitlines())
    phase = status_line(root)
    if phase:
        lines.append(f"- AGENTS.md: {phase}")
    print("\n".join(lines))


if __name__ == "__main__":
    try:
        main()
    finally:
        sys.exit(0)
