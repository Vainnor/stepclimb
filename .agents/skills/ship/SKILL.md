---
name: ship
description: "Ship the current branch as a pull request into next, gated by just ci on the machine the agent works on (there is no hosted CI) and a review of the committed HEAD. Use only when the user asks to ship the current branch as a pull request ($ship)."
---

# ship

This skill is the Codex entry to the Claude Code `/ship` command. The procedure lives in
`.claude/commands/ship.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `$ship` ([short PR title] (default: from the commits)).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
