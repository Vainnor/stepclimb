---
name: cleanup
description: "Remove merged local branches and clean agent worktrees (the only path that removes one) under .claude/worktrees, and clear the target/ of every merged worktree, after checking nothing unpushed would be lost. Use only when the user asks to clean up merged branches, worktrees or build output ($cleanup)."
---

# cleanup

This skill is the Codex entry to the Claude Code `/cleanup` command. The procedure lives in
`.claude/commands/cleanup.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `$cleanup` ([--targets-only] (otherwise none: the script judges every worktree and branch)).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
