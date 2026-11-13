# SPDX-License-Identifier: AGPL-3.0-or-later
"""PreToolUse(Write|Edit|MultiEdit|NotebookEdit): refuse writes of local-only material.

Blocks any file under references/ except references/README.md and references/INDEX.md, and any
file under private/ except private/README.md (.claude/rules/content-and-ip.md). references/ holds
local reference material; private/ holds real member exports, organization data sets and
database dumps, which never enter git.
"""

import os
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402

HOOK = "content-guard"
REFERENCE_ALLOWED = {"references/README.md", "references/INDEX.md"}
PRIVATE_ALLOWED = {"private/README.md"}


def main():
    try:
        data = hookio.read_input()
        tool_input = hookio.tool_input(data)
    except ValueError:
        hookio.block(HOOK, "could not parse the hook input, so the path was not checked")
    path = tool_input.get("file_path") or tool_input.get("notebook_path") or ""
    if not path:
        sys.exit(0)
    if not os.path.isabs(path):
        path = os.path.join(data.get("cwd") or os.getcwd(), path)
    # The path as written catches references/ even when that directory is a symlink out of the
    # repo; the resolved path catches a link elsewhere in the tree that points into references/.
    for candidate in (os.path.normpath(os.path.abspath(path)), os.path.realpath(path)):
        for root in roots(candidate):
            check(candidate, root)
    sys.exit(0)


def roots(path):
    """The project directory the tool reports, if `path` is inside it, and every repo holding it."""
    found = hookio.repo_roots(path)
    project = os.environ.get("CLAUDE_PROJECT_DIR")
    if project:
        for base in {os.path.normpath(os.path.abspath(project)), os.path.realpath(project)}:
            if os.path.commonpath([base, path]) == base:
                found.append(base)
    return found


def check(path, root):
    """Block when `path`, read against `root`, is under references/ or private/."""
    rel = os.path.relpath(path, root).replace(os.sep, "/")
    # macOS and Windows file systems ignore case, so References/x.pdf is references/x.pdf there.
    top = rel.split("/")[0].lower()
    if top == "references" and rel.lower() not in {p.lower() for p in REFERENCE_ALLOWED}:
        hookio.block(
            HOOK,
            f"{rel} is local-only reference material; only references/README.md and "
            "references/INDEX.md are written by tools (.claude/rules/content-and-ip.md)",
        )
    if top == "private" and rel.lower() not in {p.lower() for p in PRIVATE_ALLOWED}:
        hookio.block(
            HOOK,
            f"{rel} is local-only private data (member exports, organization data sets, dumps); "
            "only private/README.md is written by tools (.claude/rules/content-and-ip.md)",
        )


if __name__ == "__main__":
    hookio.run_guard(HOOK, main)
