---
description: Draft a new architecture decision record in docs/adr/ from the template, as Proposed or Open
argument-hint: <short title of the decision>
---

Draft an ADR for: $ARGUMENTS

1. Read `.claude/rules/research-and-decisions.md` in full and `docs/adr/README.md`.
2. Search `docs/adr/` and `docs/plan/` for an existing decision on the same topic. If one exists,
   say so and ask whether to supersede it instead of writing a second. If the plan already
   decides it, say so: the plan wins.
3. Take the next free number. Copy `docs/adr/0000-template.md` to
   `docs/adr/NNNN-<kebab-title>.md`.
4. Fill in context, options (at least two, with the evidence for each from `docs/research/`, the
   plan or a cited source), consequences and a recommendation. Status is `Proposed` if you
   recommend one option, `Open` if the evidence does not settle it. Never write `Accepted`: only
   the owner accepts a decision.
5. Add the row to `docs/adr/README.md`.
6. Put the decision to the user with `AskUserQuestion`: the options, your recommendation first.
   Do not implement any option until the user accepts one.
