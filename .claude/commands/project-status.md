---
description: Report the project's status (milestones M0 to M6 and 1.0, issues done, in progress and to do, open PRs and decisions, code size) with bar charts, from the repository and GitHub
argument-hint: "[brief] (brief: headline, bar charts and in-progress list only)"
---

Report Stepclimb's status. `$ARGUMENTS` is empty for the full report or `brief`. Every figure comes
from the fact sheet or a file you read this run; this command changes nothing.

## Collect

1. Run `.claude/scripts/project-status.sh` and read all of its output. A section that says it
   failed or was skipped is a gap: carry it into the report as one, never fill it in.
2. Read `docs/plan/technical.md` -> Milestones: each milestone's scope and weeks, and the specs
   index `docs/specs/README.md`.

## Judge

- A milestone's progress is its closed issues over all its issues on GitHub (the fact sheet's
  milestone table). An issue is **done** when closed by a merged PR, **in progress** when an open
  PR or a running agent covers it, **to do** otherwise.
- A milestone with no issues yet is "not broken into issues", not 0% done; say so beside the bar.
- An issue in no milestone is listed separately; never count it toward one.
- Never guess. A figure the fact sheet does not give is a gap.

## Session context (coordinator only)

When the coordinator runs this, add what the session knows: running agents and pending owner
decisions from this session's status table. Label all of it **session context**, apart from repo
facts, and let a running agent mark an issue in progress only when it names the issue.

## Report

Answer first (`response-style.md`). With `brief`, give only sections 1, 2 and 4.

1. **Headline**: the current milestone (the earliest with open issues), its percentage with what
   it counts, and `next`'s short SHA.
2. **Bar charts**, always, in one fenced block so the columns line up: one bar per milestone (M0
   to M6, 1.0). Each bar is 20 cells: done cells = round(20 x done / total), in-progress cells =
   round(20 x (done + in progress) / total) minus the done cells, the rest to do. Draw done as
   `█`, in progress as `▒`, to do as `░`, with done/total beside it. If the session has an HTML
   render tool (T3's `html_preview` and `html_render`), also render the same charts as
   horizontal bars in inline SVG or CSS, readable in light and dark, with no external scripts.
3. **Done**: what merged in the last 7 days, then each milestone's closed issues with PR numbers.
4. **In progress**: open PRs with their check state, and running agents (session context).
5. **To do**: the current milestone's open issues in order; then each later milestone in one line.
6. **Waiting on the owner**: Open and Proposed ADRs, specs' open questions, and pending owner
   decisions (session context).
7. **Numbers**: lines of Rust and TypeScript and test counts per crate and package, ADR counts by
   status, specs, merged PRs.
8. The status table (`response-style.md`).

Every percentage states what it counts. No adjectives in place of numbers.
