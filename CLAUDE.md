@AGENTS.md

# CLAUDE.md

How to work in this repository. Project facts (layout, non-negotiables, git workflow, commands)
are in `AGENTS.md`, imported above; this file and `.claude/rules/` are the way of working.

## Domain rules (`.claude/rules/`)

The rule files are the single source of truth for this codebase's conventions; when they conflict
with intuition, follow the rule. **Always** files load every session. **Path-scoped** files load
when you Read, Write or Edit a file matching their `paths:` frontmatter, which can be after the
work has started, so read them in full at the trigger named below.

| File | Loads | Scope |
| --- | --- | --- |
| `code-quality.md` | Always | Error handling, **unjustified scope**, complexity, comments describe the code as it is, **verify before writing the mechanism down**, sibling sweeps |
| `testing.md` | Always | Reading results, the plan's test layers, **four-case permission and cross-organization tests**, controllable clock, screenshot checks for visual changes |
| `content-and-ip.md` | Always | **Canvas and Moodle are UX references only**, **no vNAS assets**, NASR/CIFP and ODbL attribution, **no copyrighted course material or real member data in git**, `references/` and `private/` |
| `response-style.md` | Always | How to answer in chat: answer first, no recaps, diagrams inline, the status table |
| `engineering-prose.md` | Always | Commit bodies, PR descriptions, research and design docs, American English |
| `architecture.md` | Path-scoped: `crates/**`, `migrations/**`, `Cargo.toml`, `Cargo.lock`, `bin/check-layering.sh`, `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml`, `packages/api-client/**`. Read in full before writing Rust, a migration or adding a dependency | **Layering**, **server authority**, **every query org-scoped**, **public API only**, sqlx and migrations, **pinned versions, 14 days old** |
| `security-and-tenancy.md` | Path-scoped: `crates/stepclimb-{auth,server,core}/**`, `migrations/**`. Read in full before writing or reviewing auth, sessions, permissions, entitlements or any query | `OrgScope`, **`Require<P>` and `Require<Feature>`**, sessions and CSRF, identities and admission, secrets |
| `assessment-integrity.md` | Path-scoped: `crates/stepclimb-{assess,activities,activities-atc}/**`. Read in full before writing or reviewing grading, question types or activities | **Grading on the server**, **no answers in renders or problems**, seeded determinism, `validate` before serving, **feedback settings enforced by the server**, regrade audit |
| `ui.md` | Path-scoped: `packages/{ui,tokens}/**`, `web/**`, `apps/ui-docs/**`. Read in full before writing a component, token, page or UI copy | **Semantic tokens only**, **React Aria only**, `data-slot`, no hard-coded strings, logical properties, the pitfalls we refuse, the definition of done |
| `cross-platform.md` | Path-scoped: `apps/app/**`, `justfile`, `just/**`, `web/index.html`, `web/vite.config.ts`. Read in full before touching the Tauri app, the web shell or the gates | Browsers, **iOS WKWebView** (16 px inputs, safe areas, keyboard), Tauri desktop, **no hosted CI: the gate runs where you work**, other OSes and long jobs |
| `research-and-decisions.md` | Path-scoped: `docs/research/**`, `docs/adr/**`, `docs/plan/**`, `docs/specs/**`. Read in full before writing research, an ADR, a spec or a plan file | Verified vs inferred, source hierarchy, ADR format and lifecycle, specs decide nothing the plan leaves open, the plan is the owner's |
| `anti-ai-slop.md` | Path-scoped: `docs/**`, `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `references/*.md`. Read in full before writing docs, a commit body, a PR description, an issue or UI copy | Banned vocabulary and patterns, claim validation |

**Pointers, not copies.** When a convention has a home in `.claude/rules/` or `AGENTS.md`, this
file points at it in one line. Two copies drift; the rule file wins.

Agents in `.claude/agents/`:

| Agent | Use for |
| --- | --- |
| `implementer` | One PR-sized task from a brief or issue to an open PR against `next` |
| `code-reviewer` | Fresh-eyes review of a Rust or TypeScript diff or PR (read-only) |
| `docs-reviewer` | Fresh-eyes review of docs, specs, ADRs, PR bodies and commits (read-only) |
| `security-reviewer` | Fresh-eyes review of auth, sessions, permissions, tenancy, feedback leakage |
| `ui-reviewer` | Fresh-eyes review of a UI change against the plan's UI tab |
| `gate-doctor` | Root cause of a red `just ci` or `just app-build` |
| `domain-researcher` | LMS standards (SCIM, LTI, QTI, SCORM, Standard Webhooks, FSRS) and ATC/VATSIM procedure, sourced |
| `dependency-researcher` | Before adding or bumping a crate or npm package |

Commands in `.claude/commands/`: `/adr` (draft a decision record), `/review` (the reviewers in
parallel plus `.claude/scripts/pitfall-scan.sh`), `/ship` (gate, commit, review, PR, CI), `/cleanup`
(merged branches, agent worktrees, merged worktrees' `target/`; `--targets-only` for build output
alone), `/project-status` (milestones M0 to M6 and 1.0 done, in progress and to do, with bar
charts).

Goals in `.claude/goals/` (each at most 4,000 characters, for a scheduled or long-running agent):
`ticket-worker` (next M0 issue to a PR), `ticket-reviewer` (first review of an open PR, never an
approval), `regression-checker` (`main` health, an issue per proven regression). Scripts in
`.claude/scripts/`: `pitfall-scan.sh`, `board-move.sh <issue> <status>` (the Project board),
`post-merge-cleanup.sh`, `project-status.sh`.

Hooks (`.claude/settings.json`): `attribution-gate` and `git-guard` check Bash commands (no AI
attribution; no push to or commit on `main`/`next`; no force push without lease; no
`git add -f` of `references/` or `private/`); `pr-gate` refuses `gh pr create` until `just ci`
passed on the committed HEAD with a clean tree (`bin/ci-stamp.sh`); `content-guard` refuses writes under `references/`
and `private/`; `format-on-edit` runs rustfmt on edited `.rs` files and Biome on TypeScript,
JSON and CSS; `post-merge-cleanup` runs `.claude/scripts/post-merge-cleanup.sh --keep-worktrees`
in the background after a `gh pr merge` (merged branches, the PR's remote branch, the `target/`
of every merged worktree, prune; never a worktree, which only `/cleanup` removes), logging to
`.git/stepclimb-post-merge-cleanup.log`; `session-context` prints branch, PRs and worktrees at
session start. They need `python3`. Codex runs the same rules, hooks, agents and commands through
`.codex/` and `.agents/skills/`; edit them only here, run `python3 bin/sync-codex-mirrors.py`, and
see `docs/agents.md`.

---

## Standing prohibitions and the permission boundary

- **You do not create or switch branches, and you do not commit or push, unless the user asks**
  or a session goal explicitly hands it to you. Say what you would commit and give the message.
- **You do not decide what is the owner's to decide.** Open and Proposed ADRs in `docs/adr/`,
  the open questions in `docs/specs/`, and anything the plan leaves open are put to the user with
  options and a recommendation; never settle one by implementing it.
- **Nothing outside this repository is modified** without asking, including other repos under
  `~/Programing`.
- **No AI attribution, anywhere** (`AGENTS.md` non-negotiable 1). Nothing lifts this rule.
- **Nothing that costs money or creates an external account** (a paid runner, a hosted service,
  an app-store account) without asking.

**Silence is not consent, and a redirect is not a delegation.** When the user answers something
else or says nothing, the decision is still theirs and still open. Ask again, or say what you are
blocked on and stop.

**When you judge that an instruction lifts a gate, say so as you pass it**, naming the scope:
"working autonomously" lifts *asking*, not the prohibitions above.

**Say which "done" you mean.** "Compiles", "`just ci` green on x86_64 Linux at the committed
HEAD" and "merged" are different states. There is no hosted CI (ADR 0021): `just ci` where you
work is the gate, and every "done" names that OS.

---

## Core rules

### Zero tolerance

1. **ZERO ASSUMPTIONS.** If you are guessing, stop and ask. A behavior the spec does not state is
   a question.
2. **ZERO UNTESTED CODE.** Every function with behavior gets a test as it is written. Every
   protected route gets its four-case and cross-organization tests (`testing.md`).
3. **ZERO UNVERIFIED COMPLETIONS.** Do not report done without running the check that proves it.
4. **ZERO FALSE CONFIDENCE.** A test that passes against values you made up proves the code
   agrees with you. Prefer evidence you did not author: the spec's test cases, a known-case file
   the TA wrote, a screenshot of the running app.
5. **ZERO SCAFFOLDING AS COMPLETION.** A type, table or endpoint nothing calls is not a feature.
   A feature has its data, the logic that uses it, the route or job that invokes it, the UI where
   the spec has one, and tests across that chain.
6. **ZERO TRUST IN THE CLIENT.** Grades, deadlines, unlocks and feedback visibility are decided on
   the server (`assessment-integrity.md`). An answer that reaches a client early is a CRITICAL
   bug.
7. **ZERO BAND-AID FIXES.** No retries, fallbacks or catch-alls that hide a broken primary path.
8. **ZERO PREMATURE "FOUND IT".** Before claiming a root cause, explain why the symptom appears,
   why other paths don't show it, and what the fix changes for them. Otherwise write "evidence
   points at".
9. **ZERO PER-BUG STORIES IN COMMENTS.** Comments describe the code as it is now. History belongs
   in the commit and the test.
10. **ZERO CONTEXT EXCUSES.** You cannot observe your context limit; never stop or abbreviate
    believing you are near it. Keep the status table current.
11. **ZERO REPEATED SUGGESTIONS.** When the user rejects an approach, accept it and don't
    re-suggest it.
12. **End every reply that did work with the status table** (`response-style.md`).

### Advise as a senior engineer

The owner writes Rust and React, runs a homelab, and reviews authentication, permissions, grading
and billing line by line. Recommend, don't defer: give the call you would make and why, then let
the owner override. Never present a neutral menu. Weigh scope first: one maintainer reviews
everything, so name the review cost of every proposal and what it displaces. Verify library
behavior against current docs or a cheap spike before advising on it, and say when a
recommendation depends on that result.

### Editing CLAUDE.md, AGENTS.md, rules, agents and commands

- Fewest words that carry the rule. Rule first; one line of why only if it is not self-evident.
- Before adding, search every guidance file for rules on the same topic. Extend or replace rather
  than adding a second.
- A project fact goes in `AGENTS.md`; a way of working goes here or in `.claude/`.
- A path-scoped rule file starts with `paths:` frontmatter, then a heading saying it is not
  loaded at startup and when to read it, and gets a row in the table above.

### Monitor every shell you start

A command you background is yours until you have read its result. Print the verdict (exit code
and summary line) at the end of the command, and never report a suite as passing without reading
its summary line.

### Before shipping a change, ask what would make it wrong

1. **What did I check, and what did I assume?** Name them separately.
2. **What else touches what I changed?** A permission change reaches every route that uses it; a
   token change reaches every component and theme. Grep for the other readers and name them.
3. **If my verification is lying, how would I know?** Prefer a mutation that must turn the test
   red, the spec's own test cases, or the running app.

### Static checks are not runtime evidence

`cargo check`, clippy, tsc, Biome, the layering script and a dry run prove the code builds and the
graph is shaped right, never that it behaves. Never cite one as proof something works.

### When to ask vs work autonomously

**Ask** when a requirement is unclear, several valid approaches exist, the plan or spec leaves the
point open, the change is architectural (it needs an ADR), or it touches an open decision.
**Work autonomously** when the requirement is clear, a documented pattern applies, it is an
obvious bug, or you are writing tests for specified behavior.

### Quality checklist

Before marking any task complete:

- `just ci` green on the committed HEAD on the machine you work on, summary lines read, OS named;
  `just app-build` too for a change to `apps/app`.
- Every new route has `Require<P>`, its four-case test and its cross-organization test.
- A new query filters by the `OrgScope`; `.sqlx/` regenerated.
- A new or changed endpoint is in the OpenAPI document; the client regenerated.
- A new dependency is pinned exactly, at least 14 days old, passes `just deny` and
  `just layering`, and was checked by `dependency-researcher` if it is large.
- An architectural change has an ADR in `docs/adr/`.
- A visual change has a screenshot in the PR, both example brands, light and dark.
- Docs and the spec that describe the changed behavior were updated in the same change.

## Tool selection

Use an Explore agent for open "how does X work" questions across crates and packages; use Grep
and Read for a known symbol. If you are about to answer "how does X work" after only a grep, stop
and read.
