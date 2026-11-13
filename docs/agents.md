# Agent setup: Claude Code and Codex

Ported from Mainline's setup (same design; Stepclimb's rules, agents and commands).

Stepclimb's agent instructions, rules, hooks, agents and commands live once, under `.claude/`,
`AGENTS.md` and `CLAUDE.md`. Claude Code reads them natively. Codex reads `AGENTS.md` natively
and the rest through `.codex/config.toml`, `.codex/agents/` and `.agents/skills/`, which point
back at the `.claude/` files instead of copying them. `just agents-check` fails when a pointer
drifts from its source.

```
                     Claude Code                       Codex (0.129.0 or newer)
AGENTS.md            @import in CLAUDE.md              loaded natively
CLAUDE.md            loaded natively                   SessionStart hook prints it
.claude/rules/*.md   loaded natively (paths:)          codex-path-rules hook (paths:)
  without paths:     loaded every session              SessionStart hook prints them, and
                                                       codex-path-rules injects them again
.claude/hooks/*.sh   .claude/settings.json             .codex/config.toml [hooks]
.claude/agents/*.md  loaded natively                   .codex/agents/<name>.toml points here
.claude/commands/    /adr /review /ship /cleanup       $adr $review $ship $cleanup
                     /project-status                   $project-status skills
permissions.allow    .claude/settings.json             .codex/rules/stepclimb.rules (git status
                                                       only; see Permissions)
```

## Add a path-scoped rule

Create `.claude/rules/<name>.md` as a regular file, not a symlink: codex-path-rules ignores
symlinked rule files.

```markdown
---
paths:
  - "crates/stepclimb-auth/**"
---
# Auth Rules (not loaded at startup; read in full before ...)
```

Both tools load it when a tool call touches a matching path. Globs may use `*`, `**`, `?` and
`{a,b}`. Keep the body under 6,000 characters, or Codex gets a truncated copy; `just agents-check`
enforces the limit and the closing `---`. Add the row to the table in `CLAUDE.md`.

A rule without front matter loads in every session of both tools:
`.claude/hooks/codex-instructions.sh` prints it at Codex startup. `just agents-check` lists the
five such rules by name and checks the startup output fits under the hook's
`additionalContextLimit` in `.codex/config.toml`, so adding one means updating both.

## Add a command or an agent

A command is `.claude/commands/<name>.md`. For Codex, add `.agents/skills/<name>/SKILL.md` that
names the command file and repeats its description followed by a "Use only when ..." sentence,
and `.agents/skills/<name>/agents/openai.yaml` with `allow_implicit_invocation: false`. Copy an
existing skill directory. Codex skips symlinked `SKILL.md` files, so the skill is a small real
file that points at the command.

An agent is `.claude/agents/<name>.md`. For Codex, `.codex/agents/<name>.toml` has the same
`name` and `description` and `developer_instructions` that tell the agent to read the `.md` file.
`python3 bin/sync-codex-mirrors.py` writes it, and the command skills below, from the front
matter; run it after any change to an agent's or command's front matter. An agent with no Edit or Write tool whose instructions run no `cargo`, `just` or
`pnpm` command also gets `sandbox_mode = "read-only"`: builds write `target/` and
`node_modules/`, and `dependency-researcher` needs `~/.cargo` and the network, so it prompts under
any sandbox.
`just agents-check` checks both. The Codex subagents docs say a custom agent's sandbox setting
does not override the parent session's, so the TOML also states the Claude tool list as an
instruction.

## Add a hook

Put the script in `.claude/hooks/` and register it in `.claude/settings.json`. For Codex, add a
`[[hooks.<Event>]]` entry to `.codex/config.toml` using the same `/bin/sh -c '...'` wrapper as
the existing entries: Codex runs hook commands through the user's login shell, and the wrapper
finds the repo root with `git rev-parse`. Codex sends the same `tool_input.command` as Claude Code
for Bash. For file edits it sends an `apply_patch` body with no `file_path`, either as the
`apply_patch` tool or as a Bash command (`apply_patch <<'EOF'`) that Codex intercepts. Run a file
hook through `.claude/hooks/codex-adapter.sh <script>`; it calls the hook once per file in an
`apply_patch` tool patch. With matcher `^(Bash|apply_patch)$` it also refuses any shell command
carrying a patch, because the hook cannot see the directory a shell patch lands in.

Start Python as `python3 -X utf8`: the hook input, the rule files and the hook output are UTF-8,
and Windows' default encoding is not. Only exit 2 with a reason on stderr blocks, in both tools;
any other exit lets the call run. A guard written in Python wraps its `main` in
`hookio.run_guard`, so a crash blocks instead.

## Permissions

Claude Code's `permissions.allow` maps to `.codex/rules/stepclimb.rules`, but a Codex `allow` rule
also runs the command outside the sandbox, and a prefix rule cannot exclude an argument. Allow a
command there only if no argument can make it write a file or run another program; today that is
`git status` alone. Everything else goes through the approval prompt.

## Set up Codex on a machine

1. Install Codex 0.129.0 or newer, then codex-path-rules 0.5.0 from its release tag (it needs
   Rust 1.97.1 or newer; this repo's 1.98 works):

   ```sh
   cargo +1.98 install --locked --git https://github.com/bengous/codex-path-rules --tag v0.5.0
   codex-path-rules --version     # codex-path-rules 0.5.0
   codex-path-rules --self-test   # path-rules self-test passed
   ```

   The binary must be on the `PATH` Codex starts hooks with (`~/.cargo/bin`). The tag is commit
   `ab85fd561a3cecde9baf74879dd847a65fa85b7f`; its GPG signature (key
   `E9B4A41981A19E54FE65B1D6E214839DF8E3F9C6`) was not verified. The binary built from it on
   macOS arm64 had SHA-256
   `0208bd216e8a63d10c7f842bbb7f8ea27d777cd8db4cd3de2aa327baa15d07ed`. Its source has no network
   or subprocess code, and it exits 0 on any error, so a failure loads no rule and reports
   nothing. It deletes session state older than 7 days from `CODEX_PATH_RULES_CACHE`
   (default `$XDG_CACHE_HOME/codex-path-rules` or `~/.cache/codex-path-rules`), so never point
   that variable at a shared folder.
2. Start Codex at the repo root, since codex-path-rules reads `.claude/rules` from the session's
   working directory, then trust the project when asked, then open `/hooks` and review and
   trust each hook. Codex skips untrusted hooks and re-asks after any hook command changes.
3. If you keep user-level Claude rules in `~/.claude/rules/`, launch Codex with
   `CODEX_PATH_RULES_EXTRA_DIRS="$HOME/.claude/rules"`.
4. Codex commit attribution is an account-level setting, read only for a ChatGPT sign-in and off
   unless enabled. When it is on,
   Codex tells the model to add a `Co-authored-by: Codex` trailer, which the attribution hooks
   refuse. To check, have Codex make a harmless test commit on a scratch branch.
5. Check the install:

   | Check | How |
   | --- | --- |
   | Version | `codex --version` is 0.129.0 or newer; `codex features list` shows hooks enabled |
   | Startup instructions | Before any tool call, ask "List the non-negotiables, the standing prohibitions and the rule files you were given", then "What is the last heading of testing.md as given at startup?" The answer must be `## Visual changes`; anything else means the output was cut |
   | Hooks | `/hooks` lists 11 handlers (Mainline's 10 plus `pr-gate`) |
   | Rule injection | `cat crates/stepclimb-assess/src/lib.rs` shows "Path rules loaded" with `assessment-integrity.md` within two calls; repeating it loads nothing; `cat justfile` loads no path-scoped rule |
   | Guards | `git push origin HEAD:next`, `gh pr create` without a `just ci` pass, `references/x.pdf` created with apply_patch, and `apply_patch <<'EOF'` adding `private/y.csv` are all refused |
   | Execution policy | `codex execpolicy check --pretty --rules .codex/rules/stepclimb.rules -- <cmd>`: `git status --short` allows; `git diff`, `git log --output=x`, `just ci`, `just ci install-hooks`, `cargo check` and `cargo --config build.rustc-wrapper=x check` match nothing and prompt; nothing is forbidden |
   | Read-only agents | `docs-reviewer` asked to create a file refuses or is denied |
   | Skills | `/skills` lists `adr`, `cleanup`, `project-status`, `review`, `ship` |

## Known limits in Codex

- The always-loaded rules (`code-quality`, `testing`, `content-and-ip`, `response-style`,
  `engineering-prose`) reach Codex twice: at startup, and again from codex-path-rules on the
  first tool calls, which has no setting to skip them (a setting worth asking upstream for). The
  second copy fills most of the first injection batch, so a path rule such as
  `assessment-integrity` can arrive one tool call late.
- codex-path-rules finds paths in common read commands (`cat`, `sed`, `rg`, `head`) and patch
  headers. A path written through a shell redirect, `cargo`, `just` or a script is not seen, so
  no rule loads for it. MCP tools are not matched.
- Agent tool lists and the commands' `$ARGUMENTS` are instructions in Codex, not mechanisms.
- content-guard checks files written by a patch, in either tool. A file written by another shell
  command (`cp`, a redirect) is not checked in either tool; `.gitignore` and git-guard keep it out
  of commits.
- Not verified: sessions with more than one environment, where the patch tool's executor cwd
  and the hook's cwd may differ, and whether every model is offered the `apply_patch` tool. In
  either case content-guard may check a patch against the wrong directory or never see it.
- Codex runs matching hooks concurrently, so no hook may depend on another's result.
- The hook commands use `/bin/sh` and bash. Their tests (`just agents-check`) run on Linux in CI
  and have not been run on native Windows or macOS for Stepclimb; neither tool has run the hooks
  there.
