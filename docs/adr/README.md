# Decisions

Architecture decision records. Rules: `.claude/rules/research-and-decisions.md`. Template:
`0000-template.md`. **Open** and **Proposed** decisions belong to the owner; agents do not
implement their outcome before the owner accepts one. ADRs 0001 to 0017 record the plan's
Decided list (`docs/plan/technical.md` -> Decided, and `docs/plan/ui.md` -> UI decisions).

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-name.md) | Name: Stepclimb | Accepted |
| [0002](0002-license.md) | License: AGPL-3.0-or-later, Apache-2.0 for SDK packages | Accepted |
| [0003](0003-single-maintainer.md) | A single maintainer | Accepted |
| [0004](0004-no-moodle-compatibility.md) | No Moodle compatibility | Accepted |
| [0005](0005-general-purpose-scope.md) | General-purpose scope, with connectors for VATSIM | Accepted |
| [0006](0006-hosting-modes.md) | Hosting modes: self-hosted single or multi-organization, managed | Accepted |
| [0007](0007-billing-and-entitlements.md) | Billing and entitlements; self-hosted gets everything | Accepted |
| [0008](0008-email-senders.md) | Email senders: platform and organization | Accepted |
| [0009](0009-grading-on-the-server-only.md) | Grading on the server only | Accepted |
| [0010](0010-feedback-settings.md) | Feedback settings enforced by the server | Accepted |
| [0011](0011-tauri-first-expo-fallback.md) | Tauri first for the apps, Expo as the iOS fallback | Accepted |
| [0012](0012-technical-choices.md) | Technical choices: tokens, OAuth server, job queue, search, messages, backups | Accepted |
| [0013](0013-react-aria-and-tailwind.md) | React Aria Components with Tailwind 4 | Accepted |
| [0014](0014-fonts-and-default-brand.md) | Fonts and the default brand | Accepted |
| [0015](0015-ui-library-in-repo-only.md) | The UI library lives in the repository only | Accepted |
| [0016](0016-live-control-out-of-scope.md) | Live control simulation is out of scope | Accepted |
| [0017](0017-scorm-in-1x.md) | SCORM playback in 1.x | Accepted |
| [0018](0018-repository-and-ci.md) | Repository setup and CI | Accepted (CI amended by 0021) |
| [0019](0019-minio-image.md) | Which MinIO image the Compose stack uses | Accepted (Chainguard image, revisit in M1) |
| [0020](0020-american-english.md) | American English in docs, comments and UI copy | Accepted |
| [0021](0021-no-hosted-ci.md) | No hosted CI: the merge gate runs locally | Accepted |
| [0022](0022-configuration-names-and-file.md) | Configuration names and the stepclimb.toml layout | Accepted |
| [0023](0023-container-healthcheck-subcommand.md) | Container health checks through `stepclimb healthcheck` | Accepted |
| [0024](0024-toast-on-unstable-react-aria-exports.md) | Toast on React Aria's unstable toast exports | Accepted |
| [0025](0025-outbound-http-client.md) | Outbound HTTP: reqwest with rustls, ring and the system trust store | Accepted |
| [0026](0026-webauthn-rs-for-passkeys.md) | webauthn-rs 0.6 for passkeys | Accepted |
| [0027](0027-mock-identity-provider.md) | The mock identity provider is a test-only workspace crate | Accepted |
| [0028](0028-oidc-discovery-and-authorization.md) | OIDC discovery and the authorization request on openidconnect's types | Accepted |
