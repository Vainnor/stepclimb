---
paths:
  - "docs/research/**"
  - "docs/adr/**"
  - "docs/plan/**"
  - "docs/specs/**"
---
# Research and Decision Rules (not loaded at startup; read in full before writing research, an ADR, a spec or a plan file)

---

## Research documents (`docs/research/NN-topic.md`)

- Sections: Summary, Key facts, Details, How this affects Stepclimb, Open questions, Sources. A
  status line under the title gives the date and overall confidence.
- **Every fact is tagged** `[verified]` with a source number, or `[inferred]` with the facts it
  rests on. A claim with neither is removed.
- **Source hierarchy**: primary (specifications such as RFCs, W3C, IMS/1EdTech, SCIM, Standard
  Webhooks; official docs and release notes; FAA publications; VATSIM and vNAS documentation)
  over secondary (well-known references, vendor blogs) over community (forums, videos). Label
  secondary and community sources as such.
- Give dates and versions: a crate or package version and its release date, a spec's version, the
  date a page was accessed.
- Summarize and link. Never reproduce long passages from copyrighted sources.

## Decision records (`docs/adr/NNNN-title.md`)

The plan puts decisions in `docs/adr/` (Technical -> Licence and governance).

- One decision per file, numbered in order, never renumbered. Template: `0000-template.md`.
- **Status**: `Open` (needs the owner), `Proposed` (a recommendation awaiting the owner),
  `Accepted`, `Superseded by NNNN`, `Rejected`.
- **Open and Proposed decisions belong to the owner.** An agent may write the options, the
  evidence and a recommendation; only the owner moves one to Accepted. Never implement an open
  decision's outcome ahead of that.
- The plan's "Decided" list is accepted by the owner; its ADRs say so and link the plan.
- Any architectural change, crate boundary, dependency pin of a framework (axum, sqlx, utoipa,
  React, Tauri), platform limitation or departure from the plan gets an ADR.
- An accepted ADR is not edited to change its decision; a new ADR supersedes it.
- `docs/adr/README.md` lists every ADR with its status; update it in the same change.

## Specs (`docs/specs/`)

- One spec per feature, written before the code (plan, Technical -> How it's built). Sections:
  Behavior, API, Data, Permissions, Test cases, Out of scope, Open questions.
- A spec decides nothing the plan leaves open: such a point goes under Open questions for the
  owner.
- Code that departs from its spec updates the spec in the same PR.

## The plan (`docs/plan/`)

`docs/plan/` is the in-repo source of truth, one file per tab of the plan artifact. A change to it
is the owner's; propose it, don't make it.
