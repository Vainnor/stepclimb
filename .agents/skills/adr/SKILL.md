---
name: adr
description: "Draft a new architecture decision record in docs/adr/ from the template, as Proposed or Open. Use only when the user asks to draft or record an architecture decision ($adr)."
---

# adr

This skill is the Codex entry to the Claude Code `/adr` command. The procedure lives in
`.claude/commands/adr.md`, one copy for both tools (docs/agents.md). Read that file in full
and follow it, reading its front matter as metadata. When following it in Codex:

- `$ARGUMENTS` is what the user wrote after `$adr` (<short title of the decision>).
- `AskUserQuestion` means ask the user directly, recommended option first.
- A named agent (`code-reviewer`, `implementer`) is the Codex custom agent of that name in
  `.codex/agents/`; a `/command` it names is the skill of the same name.
