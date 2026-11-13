# SPDX-License-Identifier: AGPL-3.0-or-later
"""Codex SessionStart: print what Claude Code loads at startup and Codex does not.

Codex loads AGENTS.md natively, but never CLAUDE.md, and it gets the rules without `paths:` front
matter only from codex-path-rules on its first tool calls. A SessionStart hook's plain stdout
becomes developer context (Codex hooks docs), so this prints CLAUDE.md minus its `@AGENTS.md`
import line, which Codex has already loaded, then each `.claude/rules/*.md` file that has no
front matter, read in place. Exits 1 when a file cannot be read, so Codex shows the hook as failed
instead of starting a session without it.
"""

import os
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402

HOOK = "codex-instructions"
HEADER = (
    "# CLAUDE.md (the way of working; AGENTS.md holds the project facts). "
    "Printed by the Codex SessionStart hook in .codex/config.toml.\n"
)


def codex_text(claude_md):
    """CLAUDE.md as Codex should see it: without the `@AGENTS.md` import Codex already loaded."""
    lines = [line for line in claude_md.splitlines() if line.strip() != "@AGENTS.md"]
    return HEADER + "\n".join(lines).strip() + "\n"


def always_loaded_rules(rules_dir):
    """Sorted names of the rule files Claude Code loads every session: those with no front matter."""
    names = []
    for name in sorted(os.listdir(rules_dir)):
        path = os.path.join(rules_dir, name)
        if not name.endswith(".md") or os.path.islink(path) or not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8") as f:
            if f.readline().rstrip() != "---":
                names.append(name)
    return names


def rule_text(name, body):
    return f"\n# .claude/rules/{name} (always loaded)\n\n{body.strip()}\n"


def project_root():
    root = os.environ.get("CLAUDE_PROJECT_DIR")
    if root:
        return root
    try:
        cwd = hookio.read_input().get("cwd") or os.getcwd()
    except ValueError:
        cwd = os.getcwd()
    return hookio.repo_root(cwd) or cwd


def read(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError as error:
        print(f"{HOOK}: could not read {path}: {error}", file=sys.stderr)
        sys.exit(1)


def main():
    root = project_root()
    out = [codex_text(read(os.path.join(root, "CLAUDE.md")))]
    rules_dir = os.path.join(root, ".claude", "rules")
    for name in always_loaded_rules(rules_dir):
        out.append(rule_text(name, read(os.path.join(rules_dir, name))))
    sys.stdout.write("".join(out))


if __name__ == "__main__":
    main()
