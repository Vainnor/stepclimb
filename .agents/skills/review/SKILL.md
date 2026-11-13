---
name: review
description: "Review the current branch or a PR before it merges, with code-reviewer, docs-reviewer, security-reviewer and ui-reviewer in parallel as they apply, plus the pitfall scan. Use only when the user asks to review the current branch or a PR before it merges ($review)."
---

# review

This skill is the Codex entry to the Claude Code `/review` command. The procedure lives in
`.claude/commands/review.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `$review` ([PR number] (default: current branch against origin/next)).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
