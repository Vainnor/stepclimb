# The Stepclimb plan

**Stepclimb LMS: final plan, v1.0, 12 Nov 2026.** An open-source learning platform for any
organization, built in Rust and TypeScript. Self-host it, or use managed hosting. vZDC's
training is the first deployment. Independent open-source project, one maintainer.

This folder is the in-repository source of truth: the plan artifact, one file per tab, as
Markdown. Source: [the plan artifact](https://claude.ai/artifact/ABMCETKdv4QtXiWhdrJ8VH).

| Tab | File | What it covers |
| --- | --- | --- |
| Product | [product.md](product.md) | Who it is for, what it does, ways to run it, the roadmap |
| vZDC brief | [vzdc-brief.md](vzdc-brief.md) | The pilot proposal for vZDC: needs, costs, risks, terms, timeline, asks |
| Technical | [technical.md](technical.md) | Architecture, tenancy, identity, content, assessments, activities, integration, data model, API, apps, hosting, billing, security, testing, milestones, the decided list |
| UI | [ui.md](ui.md) | Stepclimb UI: principles, library architecture, tokens, theming, components, layouts, pitfalls, conventions, definition of done |

How the plan becomes work: decisions in [`docs/adr/`](../adr/README.md), feature specs in
[`docs/specs/`](../specs/README.md), issues on the GitHub Project, one PR per issue.

Conversion notes: spelling is American (ADR 0020), as the owner chose on 12 Nov 2026; the UI tab's
interactive live preview and full-page mockups are not reproduced; in-page links between tabs are
plain text. Where this repository and the plan disagree, the plan wins.