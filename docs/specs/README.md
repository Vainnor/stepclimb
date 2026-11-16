# Specs

One spec per feature, written before the code (plan, Technical -> How it's built). Agents work
from a spec, one issue at a time; each issue is one reviewable PR. Rules:
`.claude/rules/research-and-decisions.md` -> Specs. Template: `0000-template.md`.

A spec decides nothing the plan leaves open: such points are under its **Open questions** for the
owner. For the M0 specs the owner accepted every recommended answer on 13 Nov 2026. Code that
departs from its spec updates the spec in the same PR.

## M0 Foundations (full specs)

| # | Spec | Review |
| --- | --- | --- |
| 01 | [Repository and CI](m0/01-repo-and-ci.md) | standard |
| 02 | [Organizations and accounts](m0/02-organizations-and-accounts.md) | maintainer line-by-line |
| 03 | [Users and identities](m0/03-users-and-identities.md) | maintainer line-by-line |
| 04 | [Local accounts: passkeys, password and TOTP](m0/04-local-accounts.md) | maintainer line-by-line |
| 05 | [OpenID Connect sign-in](m0/05-oidc.md) | maintainer line-by-line |
| 06 | [Generic OAuth 2 sign-in and the VATSIM preset](m0/06-oauth2-and-vatsim-preset.md) | maintainer line-by-line |
| 07 | [Admission rules and the admission webhook](m0/07-admission.md) | maintainer line-by-line |
| 08 | [Sessions and CSRF](m0/08-sessions-and-csrf.md) | maintainer line-by-line |
| 09 | [Permission catalog, roles and `Require<P>`](m0/09-permissions-and-roles.md) | maintainer line-by-line |
| 10 | [Entitlements framework](m0/10-entitlements-framework.md) | maintainer line-by-line |
| 11 | [App shell](m0/11-app-shell.md) | standard |
| 12 | [Tokens build and theme generator](m0/12-tokens-and-theme-generator.md) | standard |
| 13 | [Base UI components](m0/13-base-ui-components.md) | standard |
| 14 | [Compose deploy](m0/14-compose-deploy.md) | standard |

## M1 Learning (full specs)

The owner approved a 17-spec split of M1. Open questions carry coordinator decisions pending the
owner's review, except those marked "open, for the owner", which change an owner-accepted spec or
narrow, move or extend what the plan says, and wait for the owner. The full list is in each spec's
Open questions.

| # | Spec | Review |
| --- | --- | --- |
| 01 | [Job queue and outbox](m1/01-job-queue-and-outbox.md) | standard |
| 02 | [Courses and enrollments](m1/02-courses-and-enrollments.md) | maintainer line-by-line |
| 03 | [Modules and items](m1/03-modules-and-items.md) | standard |
| 04 | [Unlock and completion rules](m1/04-unlock-and-completion-rules.md) | maintainer line-by-line |
| 05 | [Progress and events](m1/05-progress-and-events.md) | standard; issue I3 maintainer line-by-line |
| 06 | [Files](m1/06-files.md) | maintainer line-by-line |
| 07 | [Pages and the editor](m1/07-pages-and-the-editor.md) | standard |
| 08 | [Uploaded video](m1/08-uploaded-video.md) | maintainer line-by-line |
| 09 | [Linked video and watch tracking](m1/09-linked-video-and-watch-tracking.md) | standard |
| 10 | [Learner dashboard and Next up](m1/10-learner-dashboard-and-next-up.md) | standard |
| 11 | [Announcements and in-app notifications](m1/11-announcements-and-in-app-notifications.md) | maintainer line-by-line |
| 12 | [Email](m1/12-email.md) | maintainer line-by-line |
| 13 | [Assignments, due dates, reminders and escalation](m1/13-assignments-due-dates-reminders-and-escalation.md) | maintainer line-by-line |
| 14 | [Recurrence and expiring completions](m1/14-recurrence-and-expiring-completions.md) | maintainer line-by-line |
| 15 | [Installable web app and offline reading](m1/15-installable-web-app-and-offline-reading.md) | standard |
| 16 | [Learning UI components](m1/16-learning-ui-components.md) | standard |
| 17 | [Tauri iPhone test](m1/17-tauri-iphone-test.md) | standard |

## Later milestones (stubs)

| Milestone | Stub |
| --- | --- |
| M1 Learning | [m1.md](m1.md) (full specs in `m1/`, above) |
| M2 Assessment | [m2.md](m2.md) |
| M3 Activities | [m3.md](m3.md) |
| M4 Integrations and pilot readiness | [m4.md](m4.md) |
| M5 Apps, scenarios and ATC games | [m5.md](m5.md) |
| M6 Managed launch | [m6.md](m6.md) |
