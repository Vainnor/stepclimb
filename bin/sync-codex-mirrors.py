# SPDX-License-Identifier: AGPL-3.0-or-later
"""Write the Codex mirrors of .claude/agents/ and .claude/commands/ (docs/agents.md).

`python3 bin/sync-codex-mirrors.py` rewrites .codex/agents/<name>.toml and
.agents/skills/<name>/ from the front matter of each Claude agent and command. The mirrors only
point back at the Claude files; `just agents-check` fails when one drifts. Run it after adding or
changing an agent's or command's front matter, and commit the result. A mirror whose Claude file
was deleted is not removed: delete it by hand (agents-check reports it).
"""

import json
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TRIGGERS = {
    "adr": "the user asks to draft or record an architecture decision",
    "cleanup": "the user asks to clean up merged branches, worktrees or build output",
    "project-status": "the user asks for the project's status report",
    "review": "the user asks to review the current branch or a PR before it merges",
    "ship": "the user asks to ship the current branch as a pull request",
}


def front_matter(path):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    match = re.match(r"---\n(.*?)\n---\n", text, re.S)
    return dict(re.findall(r"^([\w-]+): (.*)$", match.group(1), re.M))


def agent_toml(file, meta):
    name = meta["name"]
    tools = [t.strip() for t in meta["tools"].split(",")]
    with open(os.path.join(ROOT, ".claude", "agents", file), encoding="utf-8") as f:
        builds = re.search(r"`(cargo|just|pnpm) [a-z]", f.read()) is not None
    edits = bool({"Edit", "Write"} & set(tools))
    lines = [
        f"# Codex custom agent mirroring .claude/agents/{file} (docs/agents.md). The prompt stays in",
        "# that file; `description` must match its front matter (`just agents-check` checks this).",
        f'name = "{name}"',
        f"description = {json.dumps(meta['description'], ensure_ascii=False)}",
    ]
    read_only = not edits and not builds
    if read_only:
        lines += [
            "# Claude Code gives this agent no edit tools. The Codex subagents docs say a custom",
            "# agent's sandbox_mode does not override the parent session's; it is set so any Codex",
            "# release that applies it runs the agent read-only, and the instructions say the same.",
            'sandbox_mode = "read-only"',
        ]
    rule = (f"Claude Code gives this agent only these tools: {', '.join(tools)}. Codex cannot "
            "enforce that list, so hold to it yourself.")
    if read_only and "Bash" not in tools:
        rule += (" Do not create, edit or delete files: no apply_patch, and no shell command that"
                 " writes. Use the shell only to read files (cat, sed -n, rg, ls); run nothing else.")
    elif read_only:
        rule += (" Do not create, edit or delete files: no apply_patch, and no shell command that"
                 " writes.")
    elif not edits:
        rule += (" Do not create, edit or delete files: no apply_patch, and no shell command that"
                 " writes, apart from the build and cache output of the commands that file names.")
    if "WebSearch" not in tools and "WebFetch" not in tools:
        rule += " Do not use the web."
    lines += [
        'developer_instructions = """',
        f"You are the `{name}` agent. Your instructions are the body of `.claude/agents/{file}`, "
        "below its front matter; Claude Code uses the same file. Read it in full before doing "
        "anything else and follow it.",
        "",
        rule,
        '"""',
    ]
    return "\n".join(lines) + "\n"


def skill(name, meta):
    hint = meta.get("argument-hint", "").strip().strip('"')
    description = f"{meta['description']}. Use only when {TRIGGERS[name]} (${name})."
    return f"""---
name: {name}
description: {json.dumps(description, ensure_ascii=False)}
---

# {name}

This skill is the Codex entry to the Claude Code `/{name}` command. The procedure lives in
`.claude/commands/{name}.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `${name}` ({hint}).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
"""


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def main():
    for file in sorted(os.listdir(os.path.join(ROOT, ".claude", "agents"))):
        meta = front_matter(os.path.join(ROOT, ".claude", "agents", file))
        write(os.path.join(ROOT, ".codex", "agents", meta["name"] + ".toml"), agent_toml(file, meta))
    for file in sorted(os.listdir(os.path.join(ROOT, ".claude", "commands"))):
        name = file[:-3]
        meta = front_matter(os.path.join(ROOT, ".claude", "commands", file))
        skill_dir = os.path.join(ROOT, ".agents", "skills", name)
        write(os.path.join(skill_dir, "SKILL.md"), skill(name, meta))
        write(os.path.join(skill_dir, "agents", "openai.yaml"),
              f"# Run only when the user names ${name}: the repo's command skills write files,\n"
              "# commit, push, delete branches or spawn agents.\npolicy:\n"
              "  allow_implicit_invocation: false\n")


if __name__ == "__main__":
    main()
