---
paths:
  - "docs/**"
  - "README.md"
  - "CONTRIBUTING.md"
  - "SECURITY.md"
  - "references/*.md"
---
# Anti-AI-Slop Rules (not loaded at startup; read in full before writing docs, a commit body, a PR description, an issue or UI copy)

Governs word choice in anything a person reads: docs, research, ADRs, specs, README, commit
bodies, PR text, issues, code comments, and UI copy. `engineering-prose.md` covers how prose
should read; this file owns the banned lists.

Source: [Wikipedia: Signs of AI writing](https://en.wikipedia.org/wiki/Wikipedia:Signs_of_AI_writing).

---

## Banned vocabulary

These words don't appear in Stepclimb prose unless there is a specific technical reason (a
`robust` statistic, the npm `ecosystem`). Default: cut and replace.

**Verbs**: delve, showcase, underscore, foster, garner, enhance, navigate (figurative), embark,
harness, empower, leverage, utilize, facilitate, commence, transform, revolutionize, unlock
(figurative), elevate, streamline, supercharge.

**Adjectives**: robust, vibrant, intricate, multifaceted, comprehensive, holistic, seamless,
bespoke, cutting-edge, paramount, pivotal, crucial, vital, noteworthy, profound, groundbreaking,
immersive, myriad, plethora.

**Buzzwords**: synergy, paradigm, tapestry, realm, landscape (figurative), beacon, game-changer,
journey (figurative), testament.

**Hedges**: arguably, potentially, it's worth noting, it could be said, one might argue.

**Transitions**: furthermore, moreover, additionally (in body prose).

Grep a draft for these before committing.

## Banned patterns

| Pattern | Why |
| --- | --- |
| "It's not just X, it's Y" / "not because X but because Y" | Negative-parallelism template |
| "Whether you're X or Y" | Hollow hedging list |
| "From X to Y" (repeated) | Spacer phrase |
| "Imagine X" / "Picture this" | Filler intro |
| "Designed to / Built to" | Avoids saying what it does |
| Triadic lists on autopilot | Rule of three |
| "In this document, we will..." / "In conclusion" | Meta-commentary |
| "The cause? A stale cache." | Rhetorical question, then answer |
| "Studies show", "experts agree" | Ghost citation: name the source or drop the claim |
| Grandiose stakes | A new question type does not "redefine learning" |

UI copy also follows the plan's "Vague copy" pitfall: buttons name their action, errors say what
happened and how to fix it (`ui.md`).

## Punctuation

- Em dashes: at most about one per 500 words. Use commas, periods or parentheses.
- Straight quotes in source files and Markdown.

## Voice

- Specific over abstract: "the attempt auto-submits 0 s after the deadline" beats "reliable
  timing".
- Plain over Latinate: "use" not "utilize", "start" not "commence", "help" not "empower".
- Numbers, not adjectives. Name the file, function, ADR or URL.

## Validate before claiming

Every factual claim traces to code you read this session or a cited source. Counts are counted,
not estimated. A feature is said to exist only after you found its implementation. If you can't
validate a claim, ask or leave it out.

## Self-audit before commit

- [ ] Banned-vocabulary grep clean?
- [ ] Em dashes under budget?
- [ ] Every claim traced to code or a source?
- [ ] Numbers carry units and sources?
- [ ] American English (`engineering-prose.md`)?
