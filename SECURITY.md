# Security policy

## Reporting a vulnerability

Please report security problems privately by email to **contact@vainnor.com**. Do not open a
public issue, pull request or discussion about it.

GitHub's private vulnerability reporting (the "Security" tab, "Report a vulnerability") is not
available yet: the repository is private, and GitHub offers the feature only on public
repositories. When the repository goes public, the maintainer turns it on in the repository's
settings (Security, "Private vulnerability reporting") and this section will name it as a second
channel. Until then, email is the only one.

Include what you found, how to reproduce it, the version or commit, and the impact you expect. You
will get an acknowledgement within 7 days. Stepclimb has one maintainer, so fixes take as long as
they take; you will hear how it is going, and you will be credited in the advisory unless you ask
not to be.

## Scope

In scope: the server (`crates/`), the web app (`web/`), the apps (`apps/app`), the packages in
`packages/`, and the deploy files in `deploy/`. Especially: authentication, sessions, permissions,
cross-organization access, answers or hidden feedback reaching a client early, and data export and
deletion.

Out of scope: problems that need a compromised server or administrator account, missing hardening
headers without an attack, and denial of service by volume.

## Supported versions

Stepclimb is pre-alpha; nothing is supported for production use yet. From 1.0, security fixes go to
the current minor release and are backported to the previous minor release for 6 months.
