# Engineering Prose Rules (always loaded)

Scope: commit message bodies, PR descriptions, review write-ups, session reports, and docs in
`docs/`. Chat replies are `response-style.md`. `anti-ai-slop.md` owns the banned vocabulary and
patterns and is path-scoped: read it in full before writing a commit body, PR description or doc.

The goal is prose a competent engineer would write: specific, direct, American English, and
willing to commit to a claim.

---

## The four structural fixes

- **Specific over abstract.** A number with its unit, a `file:line`, a named function, an ADR
  number. "Moving the deadline check into the submit handler stops a late autosave from
  counting" beats "improved attempt handling".
- **Active voice, name the actor.** "The server refuses the late answer", not "the answer is
  refused".
- **Lead with the point.** The change or finding first, then the detail.
- **Vary sentence length on purpose.** Uniform mid-length sentences are the core machine texture.

## Lists

Default to prose. Use a list only for parallel, discrete items: steps, options, failing cases. No
bold lead-in on every bullet outside rulebooks. Each bullet carries weight.

## PR descriptions

- **Write for a junior developer** who knows Rust and React but nothing about this corner of
  Stepclimb or of learning platforms. Say in ordinary words what changes before naming any type.
  Define domain terms the first time (attempt, feedback setting, entitlement, admission).
- **Draw a before/after diagram inline** wherever there is a before: a data flow diagram when the
  change is about how data moves, a state diagram for an attempt, session or sync run.
- State the gates you ran and the ones you did not, with the reason.
- For authentication, sessions, permissions, grading, feedback visibility, billing and data
  deletion: say so in the first line; those PRs get the maintainer's line-by-line review.
- For visual changes: the screenshot or Storybook check (`testing.md`).
- `Closes #N` when it maps to an issue.

## Commit messages

- **Subject**: imperative, present tense, at most about 70 characters, `type(scope): summary`
  (`feat(auth): add passkey registration`). The git log is the reference.
- **Body**: what changed and why, not the debugging story or the diff restated.
- **No attribution of any kind** (`AGENTS.md` non-negotiable 1). `Closes #N` is the only trailer.

## Write the mechanism only after you have proved it

A causal claim in a commit, PR, doc or comment is read later as settled fact. Verify first. If
the reasoning is load-bearing and unchecked, write "evidence points at", or state only what you
did.

## American English

`-ize`, `color`, `behavior`, `center`, `license` (noun and verb), `organization`, `enrollment`.
The owner chose American English for Stepclimb (`docs/adr/0020-american-english.md`); the plan's
own British spellings are converted when quoted into the repo.

## Fewest words

Cut, in order: the second copy of a sentence, the qualifying clause that restates the fact, the
set-up ("It is worth understanding that"), and the closing summary.
