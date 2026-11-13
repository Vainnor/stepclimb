# Product

The Product tab of the Stepclimb plan (final plan v1.0, 12 Nov 2026), as Markdown. Source: [the plan artifact](https://claude.ai/artifact/ABMCETKdv4QtXiWhdrJ8VH). Converted for the repository with American spelling (`docs/adr/0020-american-english.md`); the wording is otherwise the plan's. Changes to this file are the owner's (`.claude/rules/research-and-decisions.md`).

### Stepclimb in one paragraph

Stepclimb is an open-source learning platform for any team that has to train people and prove it. That could be a company onboarding staff, a club certifying members, a school, or a VATSIM facility. Courses unlock step by step. Exams and practice games are graded on the server, so results can be trusted. Training can recur and expire, managers see their own team, and everything can be synced with, controlled by, or exported to the systems an organization already runs.

## Who it's for

| Organization | Typical use |
| --- | --- |
| Companies and teams | Onboarding, annual compliance training with due dates, reminders and escalation to managers, certificates that expire, audit-ready reports. |
| Clubs, volunteer groups and non-profits | Member training and sign-offs, on the free non-profit plan or self-hosted. |
| Schools and training providers | Courses, quizzes and exams with item analysis, a gradebook and certificates. |
| VATSIM facilities | VATSIM sign-in, VATUSA roster sync, and the ATC pack: flight plan editor game, CRC/ERAM command drills and ATC scenarios. |

## What it does

**Learning**

- Courses of modules that unlock by prerequisite, with "next up" always visible
- Rich pages, files, and video: upload your own or link YouTube, Vimeo and others, with watch tracking either way
- Due dates, reminders and escalation; recurring training; completions that expire
- Certificates with verification links

**Assessment and practice**

- Ten question types, versioned banks, random draws, timed exams
- Feedback chosen per quiz or game: full, hint, result only, or none
- An activity engine for games and simulations, including branching scenarios
- Optional packs, starting with the ATC pack for VATSIM training

**People and access**

- Sign in with passkeys, any OIDC or OAuth 2 provider, and SAML later
- Roles at organization, account or course level; managers see their team
- User sync, SCIM, and permissions managed by another system if wanted

**Data and integration**

- Reporting API, scheduled exports, signed webhooks
- Content API and a course archive format
- Connectors (VATSIM, VATUSA first) built on the same public APIs

## Ways to run it

| Option | Details |
| --- | --- |
| Self-hosted | Free under AGPL-3.0, with every feature included. One binary plus PostgreSQL, with Docker Compose, Helm, or a plain binary. |
| Managed | Hosted by the maintainer, on plans that unlock features and limits. Includes a free non-profit plan, and organizations can also be granted a plan directly without billing. |
| Apps | An installable web app on any device, with desktop (macOS, Windows, Linux) and iOS apps built with Tauri. |

## Roadmap

| Stage | Contents |
| --- | --- |
| Before 1.0 | Everything above except managed hosting. The pilot runs on vZDC's theory training. See Milestones. |
| Managed launch | Plans, billing, the non-profit program, the operator console. |
| 1.x | SCORM 1.2 and 2004 playback, assignments and rubrics, SAML, LTI, translations, Open Badges, Android, WASM plugins. |
