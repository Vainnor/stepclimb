## What changes

<!-- In plain words, for someone who knows Rust and React but not this corner of Stepclimb. -->

## Before and after

<!-- A diagram or a screenshot where there is a before. Visual changes: both example brands, light and dark. -->

## Spec

<!-- The spec and behaviors this implements, e.g. docs/specs/m0/08-sessions-and-csrf.md B1-B6. -->

## Review

<!-- Tick a requirement when it is met or does not apply, and say "(not applicable: why)" for the second. An unticked requirement means work is still outstanding. The first box is a flag: tick it only if the change touches those areas. -->

- [ ] Touches authentication, sessions, permissions, grading, feedback visibility, billing or data deletion (maintainer line-by-line review)
- [ ] Every new route has `Require<P>`, its four-case test and its cross-organization test
- [ ] `.sqlx/`, `openapi/stepclimb.json` and the API client regenerated where needed

## Certification

- [ ] I certify this contribution under the [Developer Certificate of Origin](https://developercertificate.org/) (`CONTRIBUTING.md`)
<!-- Tick one of the two CLA boxes. -->

- [ ] I signed the [Individual CLA](https://github.com/Vainnor/stepclimb/blob/next/docs/cla/individual.md) through CLA Assistant, or my employer signed the [Entity CLA](https://github.com/Vainnor/stepclimb/blob/next/docs/cla/entity.md) (`CONTRIBUTING.md`)
- [ ] This is the maintainer's own pull request, which needs no CLA (ADR 0033)

## Gates

<!-- `just ci` summary lines on the committed HEAD and the OS; anything not run, and why. -->

Closes #
