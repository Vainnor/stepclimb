# 0002. License: AGPL-3.0-or-later, Apache-2.0 for SDK packages

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> License and governance; `LICENSE`; `packages/api-client/LICENSE`

## Context

Self-hosters get every feature for free, and the maintainer offers managed hosting. The plan
wants a license that keeps hosted modifications open, while SDKs that other systems embed must
not carry that obligation.

## Decision

The server, web app and native apps are **AGPL-3.0-or-later** (`LICENSE` at the root).
`packages/api-client`, and any other SDK package the plan adds, is **Apache-2.0**. Contributions
come under the DCO (Developer Certificate of Origin). Every source file carries an SPDX header;
`just spdx-check` enforces it.

## Consequences

`deny.toml` allows only licenses compatible with AGPL-3.0-or-later (permissive, MPL-2.0, the
GPL-3.0 family) and refuses GPL-2.0-only. A dependency bundled into an Apache-2.0 package must be
permissive. `CONTRIBUTING.md` explains the DCO sign-off.
