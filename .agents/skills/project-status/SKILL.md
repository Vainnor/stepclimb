---
name: project-status
description: "Report the project's status (milestones M0 to M6 and 1.0, issues done, in progress and to do, open PRs and decisions, code size) with bar charts, from the repository and GitHub. Use only when the user asks for the project's status report ($project-status)."
---

# project-status

This skill is the Codex entry to the Claude Code `/project-status` command. The procedure lives in
`.claude/commands/project-status.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `$project-status` ([brief] (brief: headline, bar charts and in-progress list only)).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
