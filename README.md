# Stepclimb

Stepclimb is an open-source learning platform for any team that has to train people and prove it:
a company onboarding staff, a club certifying members, a school, or a VATSIM facility. Courses
unlock step by step. Exams and practice games are graded on the server, so results can be trusted.
Training can recur and expire, managers see their own team, and everything can be synced with,
controlled by, or exported to the systems an organization already runs.

> **Status: pre-alpha.** The repository skeleton is in place and milestone M0 (Foundations) is
> specified in [`docs/specs/m0/`](docs/specs/README.md). Nothing here is ready to run for real
> users yet.

## Who it's for

| Organization | Typical use |
| --- | --- |
| Companies and teams | Onboarding, annual compliance training with due dates, reminders and escalation to managers, certificates that expire, audit-ready reports |
| Clubs, volunteer groups and non-profits | Member training and sign-offs, on the free non-profit plan or self-hosted |
| Schools and training providers | Courses, quizzes and exams with item analysis, a gradebook and certificates |
| VATSIM facilities | VATSIM sign-in, VATUSA roster sync, and the ATC pack: flight plan editor game, CRC/ERAM command drills and ATC scenarios |

## What it does

- **Learning:** courses of modules that unlock by prerequisite, with "next up" always visible;
  rich pages, files and video (uploaded or linked, with watch tracking); due dates, reminders and
  escalation; recurring training and completions that expire; certificates with verification
  links.
- **Assessment and practice:** ten question types, versioned banks, random draws, timed exams;
  feedback chosen per quiz or game (full, hint, result only, or none); an activity engine for
  games and simulations; optional packs, starting with the ATC pack.
- **People and access:** passkeys, any OpenID Connect or OAuth 2 provider (SAML later); roles at
  organization, account or course level; user sync, SCIM, and permissions managed by another
  system if wanted.
- **Data and integration:** a reporting API, scheduled exports, signed webhooks, a content API, a
  course archive format, and connectors (VATSIM and VATUSA first) built on the same public API.

## Ways to run it

| Option | Details |
| --- | --- |
| Self-hosted | Free under AGPL-3.0, with every feature included. One binary plus PostgreSQL, with Docker Compose, Helm, or a plain binary. |
| Managed | Hosted by the maintainer, on plans that unlock features and limits, including a free non-profit plan. |
| Apps | An installable web app on any device, plus desktop (macOS, Windows, Linux) and iOS apps built with Tauri. |

## Roadmap

| Stage | Contents |
| --- | --- |
| Before 1.0 | Everything above except managed hosting, in milestones M0 to M5. The pilot runs on vZDC's theory training. |
| Managed launch (M6) | Plans, billing, the non-profit program, the operator console. |
| 1.x | SCORM 1.2 and 2004 playback, assignments and rubrics, SAML, LTI, translations, Open Badges, Android, WASM plugins. |

The full plan is in [`docs/plan/`](docs/plan/README.md), decisions are in
[`docs/adr/`](docs/adr/README.md), and feature specs are in [`docs/specs/`](docs/specs/README.md).

## Development

Rust (axum, sqlx, PostgreSQL 16+), React 19 with Vite and TanStack Router, Tailwind 4 with an
in-repo component library on React Aria, and Tauri 2 for the apps.

```sh
nix develop          # or install the tools listed in AGENTS.md -> Commands
just install-hooks   # once per clone
just up              # PostgreSQL and MinIO in Docker Compose
just serve           # the server on http://127.0.0.1:8080, with the web app built in
just ci              # every gate a pull request must pass
```

[`AGENTS.md`](AGENTS.md) describes the workspace, the commands and the rules for contributors,
human or agent. See [`CONTRIBUTING.md`](CONTRIBUTING.md) before opening a pull request.

## License

AGPL-3.0-or-later ([`LICENSE`](LICENSE)). The TypeScript API client in `packages/api-client` is
Apache-2.0 ([`packages/api-client/LICENSE`](packages/api-client/LICENSE)) so other systems can
embed it. Security reports: [`SECURITY.md`](SECURITY.md).
