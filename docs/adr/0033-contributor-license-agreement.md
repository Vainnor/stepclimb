# 0033. A Contributor License Agreement alongside the DCO

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decider:** the owner, 2026-11-16 (issue #313)
- **Related:** ADR 0002 (contributions; its decision text is unchanged); `docs/plan/technical.md`
  -> License and governance ("Contributions under the DCO"); ADR 0007; ADR 0021;
  `CONTRIBUTING.md`; `docs/cla/`; `docs/specs/m0/01-repo-and-ci.md` B17

## Context

ADR 0002 takes contributions under the Developer Certificate of Origin (DCO) alone. The DCO
certifies that a contributor has the right to submit a change; the contributor keeps the
copyright and licenses it only under the project's licenses. The maintainer therefore cannot
offer a commercial license next to AGPL-3.0-or-later (dual licensing), or relicense, for any code
that contains an outside contribution unless every contributor agrees.

The history so far is the maintainer's own commits, so adding a CLA now costs nothing in consent
to collect. After the first outside pull request lands, it would.

Facts checked on 16 Nov 2026:

- The Apache Software Foundation's contributor agreements page links the Individual and
  Corporate CLA forms and gives no permission to reuse them; the site footer's Apache-2.0 notice
  does not say it covers the forms
  (https://www.apache.org/licenses/contributor-agreements.html).
- The Harmony agreements, version 1.0 (https://www.harmonyagreements.org/agreements), are
  licensed CC BY 3.0 (the notice is in each document). They come as Individual and Entity
  versions, as a license grant or as a copyright assignment. Outbound License Option Five lets
  the project license a contribution under any license, commercial or proprietary included, on
  the condition that it also licenses it under the licenses in use on the submission date.
- CLA Assistant (https://github.com/cla-assistant/cla-assistant, Apache-2.0, commits in October
  and November 2026) is a GitHub App that records click-through signatures on pull requests. It
  reads the CLA from a GitHub Gist and needs no Actions workflow (ADR 0021). It has no flow for a
  company signing for its employees.

## Options

### Option A: Keep the DCO alone
No change. Dual licensing stays closed for any file an outside contributor touches.

### Option B: A license-grant CLA alongside the DCO
Contributors keep their copyright and grant the maintainer a license that includes the right to
relicense. The contributor keeps the same rights in their own work as before.

### Option C: A copyright-assignment CLA alongside the DCO
The maintainer owns each contribution. Stronger; the contributor gives up the copyright in their
own work, which is a larger ask.

For the text: Apache's forms (no reuse license found) or Harmony's (CC BY 3.0). For signatures:
CLA Assistant, or a hand-kept list the reviewer checks.

## Recommendation

Option B on Harmony's license-grant text with Option Five, individual signatures through CLA
Assistant.

## Decision

Option B. Contributions come under the DCO, as before, and a CLA adapted from the Harmony 1.0
license-grant agreements with Outbound License Option Five: the individual CLA
(`docs/cla/individual.md`), signed through CLA Assistant, and the entity CLA
(`docs/cla/entity.md`), signed as a copy emailed to the security contact address and listed in
`docs/cla/entities.md`. "We" in both is the maintainer, Carson Berget, under Delaware law. The
text is published without a lawyer's review; it is reviewed before the first commercial license
is sold.

## Consequences

- Every outside pull request needs a CLA signature before it merges, in addition to the DCO box
  in the PR template. The maintainer's own pull requests and release PRs do not.
- CLA Assistant is installed on the repository and linked to a Gist holding
  `docs/cla/individual.md`. It acts only on public repositories: its pull request webhook skips a
  repository whose `private` flag is set (`src/server/src/webhooks/pull_request.js` in
  https://github.com/cla-assistant/cla-assistant, checked 16 Nov 2026).
- CLA Assistant cannot record an entity signature, so an employee covered by the entity CLA also
  signs the individual CLA, as its section 3(c) allows (`CONTRIBUTING.md`).
- A new CLA version means a new Gist revision, and CLA Assistant asks contributors to sign again.
- Dual licensing also needs the shipped code free of dependencies a proprietary license cannot
  cover. `deny.toml` still allows GPL-3.0, LGPL-3.0 and AGPL-3.0 dependencies; that is a separate
  decision, made when a commercial license is first offered.
- A lawyer reviews both texts before the first commercial license.
