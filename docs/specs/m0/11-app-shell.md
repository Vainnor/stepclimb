# App shell

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/ui.md` -> Principles, Organization theming (Delivery, Words), Key layouts,
  Component conventions, Pitfalls we refuse; `docs/plan/technical.md` -> Architecture and stack
  (Web), Tenancy and structure, Sessions and tokens, API surface, Web app and native apps,
  Hosting and operations, Testing, Milestones, Decided (English only at 1.0, ICU messages)
- **Depends on:** 03-users-and-identities (`GET /api/v1/me`), 04-local-accounts, 05-oidc,
  06-oauth2-and-vatsim-preset, 07-admission, 08-sessions-and-csrf, 09-permissions-and-roles,
  02-organizations-and-accounts (single- and multi-organization mode), 12-tokens-and-theme-generator
  (the theme CSS), 13-base-ui-components (AppShell, SideNav, BottomTabs, TopBar, OrgSwitcher,
  EmptyState, ErrorState)
- **Review:** standard. The sign-in screens call the authentication APIs of 04 to 08, which get
  the maintainer's line-by-line review; the screens hold no authentication logic of their own,
  but a PR that changes what they send to those APIs says so in its first line.

## Summary

The web app's frame for M0: the routed `web/` SPA with the AppShell layout (SideNav at 1024 px
and wider, BottomTabs under 768 px, a TopBar), the sign-in screens that drive the M0
authentication specs, the signed-in user loaded from `GET /api/v1/me`, the organization's theme
delivered as `/theme.css` before the bundle, and every string going through ICU messages with the
organization's terminology. It is the screen staff see at the vZDC week-2 checkpoint
(`docs/plan/vzdc-brief.md`: sign in with VATSIM on the test server and see the branded app).

## What exists today

Read on 2026-11-12; the spec builds on it.

- `web/src/router.tsx`: a TanStack Router tree with one route, `/`, rendering `HealthRoute`
  (`web/src/routes/index.tsx`), which reads `/healthz` through `@stepclimb/api-client`
  (`web/src/api.ts`, base URL `location.origin`).
- `web/src/messages.ts`: plain English constants, with a comment that ICU messages and
  `useStepclimbStrings()` arrive with this spec.
- `web/index.html`: `lang="en"`, `viewport-fit=cover`, no stylesheet link; the theme comes from
  the bundle (`web/src/styles.css` imports `@stepclimb/ui/styles.css`, which imports
  `@stepclimb/tokens/variables.css`).
- `web/vite.config.ts`: the dev server proxies `/healthz`, `/readyz`, `/api` and `/docs` to
  `stepclimb serve`.
- `crates/stepclimb-server/src/spa.rs`: serves `web/dist` embedded in release builds; any path
  outside `/api/` without a file extension gets `index.html`; `assets/` is cached immutable,
  everything else `no-cache`. `web/e2e/smoke.spec.ts` already checks that `/c/example/m/1/i/1`
  is served the SPA.
- `apps/app`: a Tauri 2 shell loading `web/dist`, no native features, identifier
  `dev.stepclimb.shell`; `just app-build` builds it unsigned for the local OS (there is no hosted
  CI, ADR 0021).

## Behavior

### Routing and layout

- **B1.** The web app routes with TanStack Router. React Aria's `RouterProvider` is wired to
  TanStack Router's `navigate` and `useHref`, so every Stepclimb UI `Link` and link-like item
  navigates client-side (`docs/plan/ui.md` -> Component conventions).
- **B2.** The route tree has two layouts: a **signed-out layout** (sign-in screens: no SideNav,
  BottomTabs or TopBar; the organization's logo or name, then the form) and a **signed-in
  layout** (AppShell). Every signed-in route renders inside AppShell.
- **B3.** At 1024 px and wider, AppShell shows the SideNav at the inline start: the
  organization's logo at the top, the navigation entries, and at the bottom the organization's
  configured back link where one is set (the plan's "← vZDC"; see Open questions Q8). Under
  768 px, the SideNav is replaced by BottomTabs fixed to the bottom inside the safe area
  (`docs/plan/ui.md` -> Key layouts). The layout between 768 and 1023 px is Open question Q1.
- **B4.** The TopBar shows the current view's title and, at the inline end, the user menu
  (Avatar, the user's display name, Sign out). Search and the notification bell in the plan's
  layout come with the features they open (M1 onward) and are absent in M0.
- **B5.** Navigation entries are data, not markup: each entry names its route, its Phosphor icon,
  its label message id and, optionally, the permission it needs. The shell shows an entry only
  when the user holds that permission according to `/me` (B11). Hiding an entry is presentation;
  the server still refuses the route's API calls (09-permissions-and-roles). Which entries M0
  shows is Open question Q2.
- **B6.** Every route has a document title of the form `<view> · <organization name>`, set from
  messages, and moves focus to the view's `h1` after client-side navigation so screen readers
  announce the new page. AppShell carries a skip link to `main` as its first focusable element.
- **B7.** The deep-link route `/c/:course/m/:module/i/:item` (with its return URL) is M1
  (`docs/plan/technical.md` -> Integration platform, Embedding). In M0 the server already serves
  the SPA for it (spa.rs); the router renders the not-found view (B27) for it, like any unknown
  path.
- **B8.** Layouts are phone-first: every interactive element is at least 44 px on touch, inputs
  are 16 px or larger so focusing one does not zoom, fixed bars sit inside
  `env(safe-area-inset-*)`, fixed chrome does not rubber-band, and controls are not
  text-selectable (`docs/plan/ui.md` -> Pitfalls: web-view tells;
  `.claude/rules/cross-platform.md`). The same build runs in the Tauri shell unchanged.

### Signed-in user (`/me`)

- **B9.** On start, and before rendering any signed-in route, the app loads `GET /api/v1/me`
  (03-users-and-identities) through `@stepclimb/api-client` with TanStack Query. While it loads,
  the shell renders nothing but the page background for the first 300 ms, then a Skeleton of the
  shell; no shimmer appears for loads under 300 ms (`docs/plan/ui.md` -> Pitfalls: animation
  everywhere).
- **B10.** A 401 from `/me` sends the browser to the sign-in screen with the requested path as
  the return target (B15). Any later API call that answers 401 (the session expired after 12
  hours idle or 30 days, 08-sessions-and-csrf) does the same, after clearing the query cache, so
  no previous user's data stays on screen.
- **B11.** The shell reads from `/me` only what 03-users-and-identities B22 defines: the user's
  `display_name`, the `organization` (`name`, `slug`), `grants` (permissions per scope) and
  `features` (entitlements, 10-entitlements-framework). B5's permission checks read `grants`.
  Courses and next-up items, which the plan also lists for `/me` and `/me/dashboard`, are M1.
- **B12.** Sign out calls `POST /auth/sign-out` (08-sessions-and-csrf B13), clears the
  query cache and goes to the sign-in screen. A failed sign-out shows an error Toast and leaves
  the user signed in; it never pretends to have succeeded.

### Sign-in screens

- **B13.** The sign-in screen lists the sign-in methods the organization has enabled and nothing
  else: local accounts (email, then passkey first, password and TOTP as fallback, and the magic
  link when enabled; 04-local-accounts), each OpenID Connect provider (05-oidc) and each OAuth 2
  provider, including the VATSIM preset with its configured label such as "Sign in with VATSIM"
  (06-oauth2-and-vatsim-preset). The list, with each provider's `key`, `kind` and configured
  `label`, comes from `GET /.well-known/stepclimb` (08-sessions-and-csrf B38); labels are never
  in the code.
- **B14.** A provider button starts the browser flow at `GET /auth/{key}/start` (05-oidc,
  06-oauth2-and-vatsim-preset) as a full-page navigation, not a fetch, so the
  provider's redirect and the `__Host-sc_session` cookie (08) work as the browser expects.
- **B15.** The return target is a path on the same origin, carried through sign-in and opened
  after it. The client accepts only a value that starts with a single `/`, and the server
  endpoints of 04 to 06 validate it again; anything else falls back to `/`.
- **B16.** The local-account screens follow 04-local-accounts' steps: email entry; passkey
  ceremony through the browser's WebAuthn API; password entry when the user chooses it or has no
  passkey; the TOTP code step after a password; and "check your email" after a magic-link
  request. Each step is its own route under `/sign-in`, so Back works and a refresh does not lose
  the step's place (inputs such as a password are never put in the URL).
- **B17.** Sign-in failures show the message for the problem+json `code` the server returned
  (`docs/plan/technical.md` -> API surface: stable `code` values), next to the field it concerns
  or at the top of the form. Each message says what happened and what to do next
  (`docs/plan/ui.md` -> Pitfalls: vague copy). The codes come from 04 to 08; at minimum the
  screens handle: wrong credentials, a suspended user (07 step 1), refused by admission rules or
  the admission webhook (07), sign-up not allowed (07 step 5), a provider error on callback (05,
  06), and rate limiting. An unknown code shows a generic message with the code in mono, so a
  user can report it.
- **B18.** After a provider callback the server redirects to the return target; the SPA does not
  parse tokens or provider responses. A failed or refused browser flow redirects to the sign-in
  route with `?error=<code>` (05-oidc API, 07-admission B2), and the screen shows that code as in
  B17, then removes the parameter from the address bar.

### Organizations

- **B19.** In single-organization mode the OrgSwitcher is not rendered at all, not merely hidden
  with CSS (`docs/plan/technical.md` -> Tenancy and structure). A user belongs to one
  organization and M0 has no API that lists organizations (02-organizations-and-accounts B10,
  B12), and how a request finds its organization on a multi-organization instance is 02's Q2. So
  in M0 the shell never renders the OrgSwitcher; what it does once 02 Q2 is settled is Open
  question Q3.

### Theme delivery

- **B20.** `web/index.html` links `/theme.css` in `<head>`, before the module script, so the
  organization's theme applies before the app's JavaScript runs and the default theme never
  flashes (`docs/plan/ui.md` -> Organization theming, Delivery). The server answers `/theme.css`
  with the organization's generated theme (12-tokens-and-theme-generator), or the default theme
  when the organization has none. The response is `text/css`, cached and fingerprinted, so a
  saved branding change reaches the next page load without a hard refresh. How the fingerprint
  reaches the page is Open question Q4; whether the bundle still carries default color variables
  is Open question Q5.
- **B21.** The page sets `data-sc-theme` on `<html>` from the user's appearance choice, and
  leaves it unset to follow `prefers-color-scheme` (theme resolution order, `docs/plan/ui.md` ->
  What we borrow). The choices are light, dark, high contrast and system; the organization's
  default appearance applies until the user chooses (not built yet: issue #162). Where the
  choice is stored, and how it is applied before first paint without an inline script, is Open
  question Q6. Density is set the same way with `data-sc-density`.
- **B22.** The Vite dev server proxies `/theme.css` (and `/auth`) to `stepclimb serve`, as it
  does `/api`, so development matches production.

### Strings

- **B23.** Every user-visible string in `web/` and `packages/ui` is an ICU message looked up by
  id through `useStepclimbStrings()`; no component renders a literal string
  (`docs/plan/ui.md` -> Component conventions; `docs/plan/technical.md` -> Decided: English only
  at 1.0, every string through ICU messages). Plurals, numbers and dates use ICU syntax, not
  string concatenation.
- **B24.** `useStepclimbStrings()` lives in `packages/ui` with a provider. `packages/ui` ships the
  English messages its own components need (for example a Dialog's close label); `web/` ships
  the app's English messages and passes both to the provider. 1.0 ships English only; the
  catalog shape leaves room for translations (1.x) without changing call sites.
- **B25.** Organization terminology overrides replace nouns in messages, for example Course ->
  "Training module" for vZDC (`docs/plan/technical.md` -> vZDC deployment). An override names a
  term, not a message id, and carries its singular and plural forms, so every message that uses
  "course" changes together. Components never hard-code nouns (`docs/plan/ui.md` -> Organization
  theming, Words). Where overrides are stored and how they reach the client is Open question Q7.
  How messages write terms, how an override is capitalized and which catalog rules the test
  enforces are Open question Q12.
- **B26.** A message id missing from the catalog fails the Vitest suite (a test walks the source
  for ids and checks the catalog), and at runtime renders the id in development builds and the
  English default in production builds, never an empty string. What "English default" means,
  how lookups must be written for the test to see them, and what a missing value does are Open
  question Q12.

### Error and empty states

- **B27.** Unknown paths render a not-found view (ErrorState) inside the layout the user would
  see: the signed-in layout when signed in, the signed-out layout otherwise. It names what was
  not found and offers a link home.
- **B28.** A route that fails to load renders ErrorState inside AppShell, with the problem
  `title` from the server where there is one, and a "Try again" action that retries the query.
  The shell itself stays usable, so the user can go to another page.
- **B29.** When the server does not answer at all (network error, not an HTTP status), the shell
  says the server could not be reached and offers to retry; it does not log the user out.
- **B30.** An empty list renders EmptyState with one sentence on why it is empty and, when the
  user can act, one primary action (`docs/plan/ui.md` -> Principles: one primary action per
  view).

### Native shell

- **B31.** `apps/app` keeps loading `web/dist` with no native features and keeps building with
  `just app-build`. Server discovery through `/.well-known/stepclimb`, PKCE sign-in,
  the keychain, downloads and notifications are M5 (`docs/specs/m5.md`). The final bundle
  identifier is Open question Q9.

### Out of M0 by the Milestones table

- **B32.** The installable web app (manifest, service worker, offline reading) is M1
  (`docs/plan/technical.md` -> Milestones: "Installable web app with offline reading"). M0 ships
  no manifest and no service worker.

## API

This spec adds two routes to the server and consumes others.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /theme.css` | none; the organization comes from the request's host as 02 resolves it | `200 text/css`, cache headers per Q4 | `404` only if 02 finds no organization for the host | public (the sign-in screen needs it) |
| `PUT /api/v1/me/appearance` | `{ theme, density }` | `200` the same, and the `__Host-sc_appearance` cookie | `400`, `401`, `403 auth.csrf_refused` | authenticated user (added by the I2 and I6 coordinator decisions, 2026-11-14) |

`/theme.css` is in the OpenAPI document under the `operations` tag beside `/healthz`, with a
`text/css` response, so the generated client and the Tauri app (M5) can find it. It is not under
`/api/v1` because the plan names the path (`docs/plan/ui.md` -> Organization theming).

Consumed, defined elsewhere: `GET /api/v1/me` (03), `GET /.well-known/stepclimb` (08 B38), the
sign-in endpoints and their `code` values (04, 05, 06, 07), `POST /auth/sign-out` (08). The web app
reaches all of them only through `@stepclimb/api-client`, except the provider start URLs (B14),
which are full-page navigations by design.

## Data

No tables. The appearance choice is two columns of `users` (coordinator decisions below); terminology
overrides are stored wherever Q7 decides.

## Permissions

No catalog entries. The shell reads the permissions `/me` returns to hide navigation entries
(B5); enforcement stays on the server (09). `/theme.css` is public: it holds only what the
sign-in screen already shows.

## Test cases

Web (Vitest and Testing Library, in `web/src`):

- **T1.** (B1) Pressing a Stepclimb UI `Link` navigates through TanStack Router without a full
  page load.
- **T2.** (B5) A navigation entry that needs a permission is absent when `/me` lacks it and
  present when `/me` has it.
- **T3.** (B9) A `/me` answer within 300 ms shows no Skeleton; a slower one shows it.
- **T4.** (B10) A 401 from `/me` routes to `/sign-in` with the requested path as return target;
  a 401 from a later call does the same and leaves no cached data rendered.
- **T5.** (B12) A failed sign-out shows an error Toast and keeps the signed-in layout.
- **T6.** (B13) Given a methods list with local accounts disabled and two providers, the screen
  shows two provider buttons with their configured labels and no email field.
- **T7.** (B15) Return targets `//evil.example`, `https://evil.example/x`, `/\evil.example` and
  `javascript:alert(1)` fall back to `/`; `/courses?x=1` is kept.
- **T8.** (B17) Each handled problem `code` renders its message; an unknown code renders the
  generic message with the code visible.
- **T9.** (B19) With one organization, the OrgSwitcher is not in the DOM.
- **T10.** (B23, B26) The catalog test fails when a component uses a message id the catalog
  lacks (proved once by adding one), or looks a message up in a way the test cannot read (Q12).
- **T11.** (B25) With an override for Course stored in its mid-sentence form ("training
  module", "training modules"), every message using the term renders the override, in singular
  and plural, capitalized ("Training module") where the message uses `{Course}` or `{Courses}`.
  Tested in `packages/ui` (`src/strings/strings.test.tsx`), where the provider lives, against a
  fixture catalog: M0's own catalogs use no term yet.
- **T12.** (B27, B28, B29) Not found, a failing query and a network error each render their
  state, and the retry action refetches.

End to end (Playwright against the Compose stack, `just e2e`):

- **T13.** (B20) `/theme.css` is requested before the first module script in document order, and
  a screenshot taken at `DOMContentLoaded` with JavaScript disabled already has the theme's
  canvas color.
- **T14.** (B20) After an organization's theme changes, the next page load uses the new CSS
  without clearing the browser cache.
- **T15.** (B3, B8) At 1280 px the SideNav is visible and BottomTabs absent; at 375 px the
  reverse. Every tab target is at least 44 by 44 px.
- **T16.** (B14, B18) Sign-in with the mock OIDC provider (`docs/plan/technical.md` -> Testing)
  lands on the return target with `/me` loaded. Shared with 05-oidc's end-to-end test.
- **T17.** (B6) After client navigation, focus is on the new view's `h1` and `document.title`
  names the view.
- **T18.** (B7) `/c/example/m/1/i/1` returns 200 (as today) and renders the not-found view.
- **T19.** axe reports zero violations on the sign-in screen and the signed-in home in light,
  dark and high contrast at 1280 and 375 px.

Server (crate tests):

- **T20.** (B20) `GET /theme.css` answers `text/css` with the cache headers Q4 settles, for an
  organization with branding and one without.

`PUT /api/v1/me/appearance` needs a user and no permission, so the permission suite gives it the
401, 2xx and cross-organization cases of an `AuthOnly` entry (09 B20); there is no 403 or 404
case. `/theme.css` gets the cross-organization test that 02 defines for host resolution: one
organization's host never receives another's theme.

## Out of scope

- The components themselves (AppShell, SideNav, BottomTabs, TopBar, OrgSwitcher, Avatar,
  EmptyState, ErrorState, Skeleton, Toast): 13-base-ui-components. This spec composes them.
- Generating the theme CSS and the branding API: 12-tokens-and-theme-generator.
- What the sign-in endpoints do, admission, sessions and CSRF: 04 to 08.
- Dashboard, courses, deep links, search, notifications, the installable web app: M1
  (`docs/specs/m1.md`).
- Tauri server discovery, PKCE sign-in and native features: M5 (`docs/specs/m5.md`).
- Translations: 1.x.

## Open questions

The owner accepted the recommendations of Q1 to Q11 on 13 Nov 2026; Q12 awaits the owner's
review. Where a behavior or an issue says "pending Q<n>" or "Q<n> answered", build the
recommended option; the owner can still overrule one, which then updates this spec.

- **Q1. Layout between 768 and 1023 px.** The plan gives SideNav at 1024 px and up and
  BottomTabs under 768 px. Options: (a) BottomTabs up to 1023 px; (b) a collapsed SideNav (icons
  only, labels in tooltips) from 768 px; (c) the full SideNav from 768 px. Recommendation: (b),
  because tablets in landscape keep a side rail and the icons already exist; it needs a
  collapsed SideNav variant in 13.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Navigation entries in M0.** The plan's layouts show Dashboard, Courses, Calendar and
  Grades (side) and Home, Courses, Notifications, Profile (tabs), none of which exist in M0.
  Options: (a) show only destinations that exist (Home and the user's account page, plus the
  admin screens 02 to 10 add, gated by permission); (b) show the plan's full set with empty
  states. Recommendation: (a); an entry to an empty page is half-built scaffolding (`AGENTS.md`
  non-negotiable 4). Each milestone adds its entry with its feature.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. What the OrgSwitcher does once multi-organization mode resolves organizations.** Users
  belong to one organization (`users.org_id`); a person can have accounts in several, each with
  its own session. Options: (a) it lists the organizations the browser holds sessions for and
  goes to each one's origin; (b) it lists every organization on the instance that allows
  sign-up; (c) it stays unrendered until M6's operator console. Recommendation: (a), once 02 Q2
  settles host or path addressing; it needs no listing API, since the browser knows its own
  sessions only through each origin.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. How `/theme.css` is fingerprinted.** `index.html` is a static embedded file today.
  Options: (a) the server rewrites `index.html` on the way out to link
  `/theme.css?v=<hash of the theme>` and serves `theme.css` immutable; (b) a stable `/theme.css`
  with `no-cache` and a strong `ETag`, so each load revalidates (a 304 is cheap); (c) the server
  sets a `Link: </theme-<hash>.css>; rel=preload` header. Recommendation: (a), which matches the
  plan's "cached and fingerprinted" with no revalidation round trip; the Tauri app (M5) reads the
  same hash from a `theme` field this spec adds to `/.well-known/stepclimb` (08 B38 leaves
  branding fields to the spec that owns them).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. Whether the bundle keeps the default color variables.** Today the bundle imports
  `variables.css`; a later bundle stylesheet would override `/theme.css`, which loads first, at
  equal specificity. Options: (a) the web bundle carries no color variables and `/theme.css`
  always supplies them (Storybook keeps importing `variables.css`); (b) the bundle's variables sit
  in a lower cascade layer, so unlayered `/theme.css` wins whatever the order; (c) `/theme.css`
  uses a more specific selector. Recommendation: (a): one source per page and nothing to
  out-rank. It is coordinated with 12's server output.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. Where the user's appearance choice lives, and how it applies before first paint.** The
  strict CSP the plan asks for rules out an inline bootstrap script. Options: (a) a user profile
  field returned by `/me`, rendered by the server into `<html data-sc-theme>` (needs the
  `index.html` rewrite of Q4a); (b) a cookie read by the server for the same rewrite;
  (c) `localStorage`, applied by an external blocking script before the bundle.
  Recommendation: (a), so the choice follows the user across devices and the Tauri app, with (b)
  for the signed-out screens.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. Where terminology overrides live and how they reach the client.** Options: (a) a
  `terminology` key in `organizations.settings` owned by this spec (02 B14: each settings key is
  owned by the spec that uses it), edited through `PATCH /api/v1/organization/settings`, and
  returned to the client in `/me` and in `/.well-known/stepclimb` for signed-out screens; (b) a
  separate public `/api/v1/strings` endpoint returning the merged catalog. Recommendation: (a); the
  overrides are a few terms, not a catalog. Also open: which terms are overridable in M0 (the plan's
  example is Course; Module and Item are the obvious others).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. The organization's back link.** The plan's layout shows "← vZDC" and the vZDC deployment
  lists a "Back to vZDC" link, but the branding inputs do not list it. Options: (a) a branding
  field (label and URL) in 12; (b) an organization setting in 02. Recommendation: (a), since it
  sits with the logo.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q9. The Tauri bundle identifier.** `apps/app/src-tauri/tauri.conf.json` has
  `dev.stepclimb.shell` as a placeholder. The identifier is permanent once an app ships to the
  App Store (M5). Options: (a) a reverse-DNS name under a domain the maintainer owns; (b) keep
  the placeholder until the Apple developer account exists before M5. Recommendation: (b), with
  the identifier settled in an ADR before the first TestFlight build.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q10. The ICU message library.** The plan decides ICU messages, not the library. Options:
  (a) FormatJS (`intl-messageformat`, optionally `react-intl`); (b) `@messageformat/core`;
  (c) a thin wrapper over `Intl.PluralRules` and `Intl.NumberFormat` with a small ICU subset.
  Recommendation: (a) `intl-messageformat` alone behind `useStepclimbStrings()`, the most used ICU
  implementation; run `dependency-researcher` on size and release dates before pinning.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q11. Scalar's script and the strict CSP.** The plan wants a strict CSP (`docs/plan/
  technical.md` -> Security and privacy); the Scalar page at `/docs` loads its script from a CDN
  by default. Options: (a) self-host the Scalar asset in the binary and serve `/docs` under the
  same strict CSP; (b) a per-route CSP for `/docs` that allows the CDN; (c) leave `/docs` without
  a CSP in M0. Recommendation: (a): no third-party script on the instance's origin, and `/docs`
  works on an offline network. Which spec sets the CSP header for the SPA is also open (08 or
  this one); this spec only requires that the SPA needs no inline script or style beyond what
  React Aria injects.
  **Decided:** the recommendation (owner, 13 Nov 2026).

- **Q12. How messages write terms, and the rules the catalog test enforces.** B25 and B26 leave
  the message format open. Raised by the coordinator on PR #112 (issue I1), 13 Nov 2026.
  **Built as the recommendation, pending the owner's review.**
  - *Override format.* Options: (a) store an override's forms as they read mid-sentence
    ("training module"); messages use `{course}` and `{courses}` mid-sentence and `{Course}` and
    `{Courses}` at a sentence start, which upper-case the first letter in the active locale and
    never lower-case anything, so "LOA" survives; (b) store the capitalized form and lower-case it
    mid-sentence, which breaks acronyms; (c) store all four forms. Recommendation: (a).
  - *Articles.* Options: (a) no message puts "a" or "an" directly before a term argument (or a
    tag, plural or select that starts with one), since the article depends on the override ("a
    course", "an exam"); the catalog test refuses it; (b) overrides carry their article.
    Recommendation: (a); English-only at 1.0 does not justify a grammar per term.
  - *Nouns as text.* The catalog test reads each parsed ICU message and refuses a term's default
    noun in literal text, including plural and select option values; select keys are not text.
    Alternative: a regular expression over the raw message, which also flags select keys.
    Recommendation: the parsed message.
  - *"English default" in B26.* Options: (a) the English catalog's entry for the id, used when
    the active locale's catalog lacks it; an id English also lacks renders as the id in both
    builds; (b) an English default written at each call site, as FormatJS's `defaultMessage`.
    Recommendation: (a): the catalog stays the one source of English text.
  - *Lookups the test can read.* Options: (a) the lookup function is always called `t` and called
    directly with a string-literal id; the test fails on any other first argument, on an optional
    call of `t` and on a destructure of `useStepclimbStrings()` that renames `t`, unless the line
    carries a `strings-check-ignore` comment; (b) typed ids checked by the compiler. Recommendation:
    (a), which is the test B26 already names; (b) can be added later.
  - *Missing values.* A message formatted without a value for one of its arguments throws, as
    `intl-messageformat` does. A `null` or `undefined` value, which `intl-messageformat` renders
    as nothing, throws too. Alternative: render the argument's name. Recommendation: throw.

Coordinator decisions (2026-11-14), on the owner's delegation, while building I3 and I4:

- **`/sign-in` before I5.** The route exists in the signed-out layout so B10's redirect has
  somewhere to land. Until I5 it shows an `h1` "Sign in" and one sentence saying this version
  cannot sign in from the browser. I5 replaces the view and keeps the route and its search.
- **The return target's parameter is `return_to`**, the name the server's flows use (04, 05,
  08 B19). The client keeps a value only by 08 B19's rule: one leading `/` not followed by `/`
  or `\`, every character from `!` to `~`, no `.` or `..` path segment (literal or `%2e`), at
  most 2,048 characters; anything else is `/`. The sign-in route applies the rule when it reads
  the parameter. Sign-out goes to `/sign-in` without one.
- **Signed-out screens name the product until I5.** The signed-out layout shows "Stepclimb" and
  sets the document title to `<view> · Stepclimb`, because the organization's name reaches a
  signed-out browser only through `/.well-known/stepclimb` (08 I9), which I5 reads.
- **An entry's permission is held at any scope.** B5 shows an entry when any grant in `/me`
  lists the permission, organization or account, so a sub-account admin sees the screen their
  role reaches.
- **Home in M0** was the first navigation entry (Q2); the account page (issue #179, below)
  added the second. Home shows an EmptyState, "You have no {courses} yet." The skeleton route
  that read `/healthz` is removed; the probes stay.
- **The not-found view asks `/me`** to choose its layout: AppShell when it answers 200, the
  signed-out layout for a 401 or any failure. It never redirects to sign in.
- **A `/me` failure other than a 401** shows ErrorState in the signed-out layout with "Try
  again", and never signs the user out. A problem `code` the app knows gets a plain message from
  the strings catalog saying what happened and what to do: `org.none` (the server has no
  organization yet) is the one M0 maps. Any other code shows the problem's `title`, and no
  answer at all shows "The server could not be reached". I7 brings the other route errors and
  the offline state.
- **No automatic retries.** Queries and mutations fail on the first error, so a 401 reaches the
  session handler at once and every error view offers "Try again".
- **A 401 is a session end only inside the signed-in layout.** Several at once leave once. A
  401 that arrived elsewhere (the not-found view) leaves the user's data cached beside the
  error; the signed-in layout draws its skeleton, never that data, while it ends the session.
- **SideNav logo.** Until 12's branding fields exist (logo, Q8's back link), the SideNav shows
  the organization's name, its initials when collapsed, and no back link.
- **The focused `h1` draws no ring.** B6 focuses it by script; it is not a control. The rule
  is in Stepclimb UI's `base.css`, for any `h1` with `tabindex="-1"`.
- **End-to-end tests answer `/me` in the browser.** The e2e stack has no organization and nothing
  can sign in before I5, so T15 and T17 fulfill `/api/v1/me` with a synthetic user in Playwright;
  T18 and an anonymous browser's redirect to `/sign-in` run against the real server.

Coordinator decisions (2026-11-14), on the owner's delegation, while building I2 and I6:

- **The fingerprint (Q4).** The server writes `index.html` for each request: its `/theme.css`
  link becomes `/theme.css?v=<css_hash>`, the SHA-256 the branding record already stores (12).
  `/theme.css` answers `Cache-Control: public, max-age=31536000, immutable` when `v` is the
  served stylesheet's hash and `no-cache` otherwise (no `v`, as the Vite dev server's page links
  it, or a page from before a branding change), so no cache keeps a stylesheet under a hash that
  is not its own. `index.html` is `private, no-cache` with `Vary: Cookie`. When the request's
  organization cannot be found, the link stays unfingerprinted and `/theme.css` gives the same
  refusal; a database error there is logged and has the same effect, since the app's own `/me`
  request then shows the failure. A stored hash that is not 64 lowercase hex digits also leaves
  the link unfingerprinted, so nothing else reaches the attribute. A server error while finding
  the page's session is logged and the page drawn from the appearance cookie.
- **An instance without an organization serves the default theme.** `/theme.css` answers the
  default brand's stylesheet for `org.none` instead of 503, because with Q5 the page has no
  other colors and the "no organization yet" screen must still be drawn. Two or more
  organizations still answer 404 `org.not_found`.
- **The `theme` field of `/.well-known/stepclimb` (Q4)** is `{stylesheet}`, the absolute URL
  `<public URL>/theme.css?v=<hash>` (08 B38, coordinator decision 2026-11-15, issue I9).
- **The bundle without color variables (Q5).** `@stepclimb/tokens` builds `structure.css`, every
  variable no theme sets (space, type, control heights, motion, the pill radius, the floating
  shadow, compact density), and `theme.css`, the default brand's stylesheet. `@stepclimb/ui/app.css`
  imports `structure.css` where `styles.css` imports `variables.css`; `web/` imports `app.css`,
  Storybook keeps `styles.css`. A tokens test checks that the two files declare no variable in
  common and together give every element what `variables.css` gives it.
- **The Tauri shell's theme.** `apps/app` loads `web/dist` with no server, so `vite build` copies
  the default brand's stylesheet to `web/dist/theme.css`. The server never serves that file: its
  `/theme.css` route answers first. Linking a server's theme is M5.
- **Where the choice lives (Q6), for the owner to confirm.** Two columns of `users`, `theme` (`system`, `light`, `dark`,
  `high-contrast`) and `density` (`comfortable`, `compact`), defaulting to `system` and
  `comfortable`. `GET /api/v1/me` returns them as `appearance`; `PUT /api/v1/me/appearance` sets
  both (authenticated user, no permission, CSRF-checked) and answers them with
  `__Host-sc_appearance=<theme>.<density>` (one year, `HttpOnly`, `SameSite=Lax`), the cookie
  of Q6 (b). `index.html` takes the signed-in user's choice, or without a session the cookie's;
  an unreadable or repeated cookie is no choice. 03 lists no profile route for M0; this narrow,
  self-only preference route is the one exception, and 03 says so. The coordinator kept both the
  columns and the route after review (2026-11-14); the owner confirms or overrules them.
- **No organization default appearance yet.** B21's "the organization's default appearance"
  has no input: 12's branding inputs do not include one. Until #162 adds it, a user who has not
  chosen follows the operating system (`data-sc-theme` unset). #162 also re-renders stored
  stylesheets when the generator version changes. I6 therefore covers B21 except the
  organization default.
- **The choice is in the user menu,** as two submenus, Theme (Match my device, Light, Dark, High
  contrast) and Density (Comfortable, Compact), each marking the current value. The app saves
  first and applies the answer to `<html>`; a failed save shows an error toast and changes
  nothing. While a save is pending, both submenu items are disabled and the one being saved reads
  "Theme: saving Dark", so a second quick choice cannot send the first one's old value back. The app also applies `/me`'s choice when it loads, for a page the server did not
  write (the Vite dev server, the Tauri shell).
- **The OrgSwitcher (B19, Q3)** stays out of the shell: M0 resolves one organization per
  instance (`RequestOrg` answers 404 with two or more), so there is no second session to list.
  A Vitest test checks it is not in the DOM; Q3's behavior is built with 02 Q2's resolution.

Coordinator decisions (2026-11-15), on the owner's delegation, while building I5:

- **Only the methods with server routes.** The screens drive enrollment by link, passkey
  registration and passkey sign-in (04 I2 to I4). Password, TOTP and the magic link arrive with
  04 I6 to I8, and provider buttons with 05 and 06; each adds its step route under `/sign-in`.
- **The screen does not read the discovery document yet.** The server serves
  `/.well-known/stepclimb` (08 I9), but the sign-in screen reads the organization's methods and
  name from it (B13) only with issue #171. Until then, `/sign-in` offers
  the local email and passkey steps, and the passkey start route's 404 `auth.method_disabled`
  tells the screen the method is off, which it shows as B17's message. The signed-out layout keeps
  the product name. T6 is tested with #171.
- **The steps (B16).** `/sign-in` takes the email; `/sign-in/passkey` signs in. The address goes
  to the passkey step in the history entry's state, never in the URL, so it stays out of the
  server's access log; the email step's own entry keeps it too, so Back finds it filled in. The
  passkey step opened without one (a typed URL) goes back to the email step with its
  `return_to`. The step asks the start route for the request options when it opens, so the
  button goes straight to the browser's passkey prompt, which some browsers allow only right
  after a press. After `auth.flow_invalid`, `auth.flow_expired` or `auth.passkey_invalid` (the
  server deleted the flow) it asks for new options.
- **The enrollment page.** `/enroll` reads the token from the fragment only (a `?token=` query is
  ignored) and sends it only in request bodies. Once read, the page moves the token into the
  history entry's state and replaces the entry without the fragment, so the address bar no longer
  shows it while a reload still finds it; a new link pasted into the same tab (only the fragment
  changes) replaces it. It asks no passkey name, so the server's
  `Passkey` applies. A link whose `methods` holds only `password` says the browser cannot set
  that up yet. `auth.enrollment_invalid` and `auth.enrollment_expired`, from either route, end
  the page in an ErrorState without "Try again"; so does `auth.method_disabled`. The dead-link
  page links to `/sign-in`. "Try again" after a failed start also clears the failed
  registration's message. After sign-in
  the return target replaces the page's history entry, so Back does not reopen the link.
- **Completing sign-in.** The app clears its query cache and opens the server's `return_to`
  after checking it again by B15's rule, so a target the server let through but the client
  refuses opens `/`.
- **Messages (B17).** Each of `auth.enrollment_invalid`, `auth.enrollment_expired`,
  `auth.method_disabled`, `auth.suspended`, `auth.flow_invalid`, `auth.flow_expired`,
  `auth.passkey_invalid`, `auth.rate_limited` (or any 429) and `org.none` has its own message;
  a 429 names the wait when `Retry-After` gives seconds. A ceremony the browser ends has its own
  too: cancelled or timed out (`NotAllowedError`, `AbortError`), a passkey this device already
  holds for the account (`InvalidStateError` while registering), a browser without WebAuthn's
  JSON methods (`parseCreationOptionsFromJSON`, Baseline 2025), a page that is not a secure
  context (`window.isSecureContext`; it is told to open the `https` address), and any other
  refusal. Any other code shows the general message and the code in Stepclimb UI's `Code`. A
  failure that belongs to no field is Stepclimb UI's `FormError` at body size. The copy names no
  server setting (a link's lifetime, a flow's), so a changed setting cannot make it wrong.
- **The callback error (B18)** moved to #171 with the first flow that redirects with `?error=`
  (05 or 06), and was built there (below).
- **End to end.** The e2e stack's public URL is `http://localhost:<port>`, since a relying party
  ID cannot be an IP address (04 B8), and `just e2e` creates its one organization and admin with
  `stepclimb admin create`, whose printed link the Playwright test opens (04 T20). Its sign-in
  rate limit is 200 a minute, since every test comes from one address.

Coordinator decisions (2026-11-15), on the owner's delegation, while building I7:

- **What "could not be reached" means (B29).** A request whose `fetch` rejected for any reason but
  an abort the app made, or that got 502, 503 or 504 without an `application/problem+json` body:
  a proxy in front of the server said it got no answer (the Caddy example answers 502 for a stopped
  server), while the server's own 503s, such as `org.none`, are problem+json. The API client turns
  both into its own error, so the views tell them apart from an HTTP error answer and from a fault
  in the page. A fault in the page shows the general "Could not load" message (on the sign-in
  screens, "Something went wrong while signing you in"), never "The server could not be reached".
- **The server decides, not the browser's network flag.** The query client sends every query and
  mutation whatever `navigator.onLine` says (TanStack Query's `networkMode: "always"`), so a
  browser that thinks it is offline gets the server's answer, or the unreachable state, instead of
  a request paused for good. Queries still refetch when the network returns.
- **The offline state** is a notice above the view in the signed-in layout. It shows while a query
  whose data is on screen (a view still observes it) last refetched without an answer (a tab
  coming back into view, the network returning). The view keeps its data, the user stays signed
  in, "Try again" refetches those queries, and the notice goes once they answer, by itself when the
  network returns. A query no view shows any more neither keeps the notice up nor is refetched. A
  first load that gets no answer is not this state: it shows the view's error (B28) or, for `/me`,
  the signed-out layout's.
- **Focus after "Try again" (B6).** When a retry works and the error or notice that held the
  button goes away, focus moves to the view's `h1`, as after a navigation, instead of falling to
  the page body.
- **Route errors (B28)** are the router's error component for every route, drawn inside the
  layout the failed route belongs to: under the TopBar's `h1` in the signed-in layout, as the
  view's `h1` in the signed-out layout, and in the signed-out frame for a failure in a layout
  itself. "Try again" resets the query error boundary and reruns the route's loaders. A query's
  401 is never drawn as an error: the session handler leaves first (B10).
- **The empty state in M0 (B30)** is Home's, the only list M0 draws. It offers no action, since
  M0 has nothing a user can do there.
- **T19's signed-in home** answers `/me` in the browser, with the theme in its `appearance`, at
  1280 and 375 px; the sign-in screens' half of T19 came with I5. The not-found view in both
  layouts gets axe at both widths, and the two unreachable states in light, dark and high contrast
  at both widths.
- **T14** signs in with the passkey the enrollment test registered, changes the organization's
  primary color through `PUT /api/v1/admin/branding`, opens the next page in the same browser and
  restores the branding. Branding is the whole instance's, so `sign-in.spec.ts` runs as its own
  Playwright project after every other file.
- **The user menu's name (B4).** The button's accessible name is unchanged, "{name}, account
  menu"; it now comes from visually hidden text instead of an `aria-label`. The Avatar and the
  visible name are decorative and hidden from assistive technology. With the `aria-label`, axe's
  `label-content-name-mismatch` rule, which T19 runs, read the Avatar's initials as label text.

Coordinator decisions (2026-11-15), on the owner's delegation, while building issue #179 (the
account page Q2 (a) names):

- **The page is "Account", at `/account`,** with the navigation entry "Account" (Phosphor
  `UserCircle`) after Home, for every signed-in user. **Decided by the owner (15 Nov 2026):**
  "Account" at `/account`, not the plan's "Profile". Q2 (a) calls it "the user's account page"
  and the user menu is already the "account menu"; "Profile" would promise a profile editor M0
  does not have. The TopBar's `h1` is "Account"; the page holds sections, each
  with its own `h2`. In M0 the one section is Passkeys (04 B13); the password (04 I6) and TOTP
  (04 I7) add theirs.
- **Passkeys** lists each passkey's name, the date it was added and the date it last signed in
  ("Not used to sign in yet" before then), dates in ICU's `date, medium` in the browser's time
  zone, oldest first as the server answers. Each has Rename and Delete; "Add a passkey" is the
  view's one primary action. An empty list is an EmptyState without an action, since Add is
  beside it.
- **Adding** opens a Dialog with an optional Name field (a short input, which `docs/plan/ui.md`
  allows a dialog). Opening it calls the start route, so its button goes straight to the
  browser's prompt; a blank name is left out of the request and the server's `Passkey` applies.
  After `auth.flow_invalid`, `auth.flow_expired` or `auth.passkey_invalid` it asks for new
  options. `local.passkeys_not_allowed` and `auth.method_disabled` from the start route leave
  only Cancel; after any other start failure the dialog's button reads "Try again" and starts
  again. While a registration or a rename is in flight, pressing Enter in the name field sends
  nothing more. Without WebAuthn, or outside a secure context, the section says why in place of
  the button; list, rename and delete still work.
- **Renaming** opens a Dialog with the name filled in; deleting asks in an AlertDialog ("Delete
  {name}?", the danger "Delete passkey", focus on Cancel) and sends nothing until confirmed. A
  name the server refuses (422 on `/name`) is shown at the field. After a delete, a success toast
  names the passkey and focus moves to the section's `h2`, since the row that held it is gone;
  Stepclimb UI's `base.css` draws no ring on a heading with `tabindex="-1"` (extended from `h1`
  to every heading level). The `local.last_method` message stays only while the list it was
  refused against is on screen: an add, a rename or a refreshed list that differs removes it.
- **Messages (B17).** `local.last_method`, `local.passkeys_not_allowed` (it says the organization's
  sign-in settings for the user's provider do not allow passkeys, and that an administrator of
  the organization can allow them in Stepclimb),
  `auth.method_disabled`, `auth.flow_invalid`, `auth.flow_expired`, `auth.passkey_invalid` and
  `passkey.not_found` (which also refreshes the list) each have their own message; any other code
  shows the general message and the code in `Code`. A passkey this device already holds for the
  account (`InvalidStateError`) has its own message; the other ceremony endings reuse the sign-in
  screens' messages. While passkeys are off, the list's `auth.method_disabled` replaces the list
  with that message and no "Try again"; any other list failure is `LoadError` with "Try again".
- **End to end.** `web/e2e/sign-in.spec.ts` drives the page against the real server with the
  virtual authenticator (add, the refusal of a device that already holds one, rename, delete
  after confirming, and the refused delete of the last passkey); `web/e2e/account.spec.ts` runs
  axe on the list, the add dialog and the delete confirmation in light, dark and high contrast at
  1280 and 375 px with `/me` and the passkey routes answered in the browser.

Coordinator decisions (2026-11-15), on the owner's delegation, while building issue #171 (the method
list, provider buttons, the callback error and the password step that I5 left):

- **The method list (B13).** `/sign-in` reads `/.well-known/stepclimb` and draws what its `sign_in`
  lists, in its order: the local entry's email step first when its `methods` hold `passkey` or
  `password`, then a Separator, then one button per provider. `totp` is a second step after a
  password and the magic link has no screen yet (04 I8), so neither draws anything on its own. A
  document that offers nothing the browser can use shows an EmptyState saying so, with no action.
  While the document loads the view shows its `h1` and a Skeleton; a failure is `LoadError` with
  "Try again" (`org.none` keeps its message). The view's one primary action is Continue, or a
  provider's button when it is the only way in; with two or more providers and no local sign-in,
  no button is primary, since none is preferred.
- **The organization's name.** The signed-out layout's header and the sign-in steps' document
  titles use the document's `name`. While it loads the header keeps its line empty; when it
  cannot load, and in the frames drawn outside the layout (not found, a failed layout), the
  product name stays. The document is kept a minute in the query cache, so the layout and the
  steps share one request; every page load asks again.
- **Provider buttons (B14)** are Stepclimb UI's `ButtonLink` (13, added for this): an anchor to
  `/auth/<key>/start`, the key percent-encoded as one path segment, with `?return_to=` when the
  page has one. A provider whose key is `.` or `..` is left out: the browser would resolve
  `/auth/../start` to another path, and the key rule spec 06 Q5 decided (lowercase letters,
  digits and hyphens) is not enforced on the server until providers can be created (06 I4, I5).
  The link passes `routerOptions={{ reloadDocument: true }}`, and the app's React
  Aria `RouterProvider` hands such a link to the browser (`location.assign`) instead of TanStack
  Router. A provider whose `label` is `null` reads "Sign in with <key>".
- **The callback error (B18).** The route keeps any string `error` from the search, the screen
  reads it once into its own state, shows its message as a `FormError` above the methods, and
  replaces the history entry without `error` (keeping `return_to`). Focus moves to the message
  once, so a screen reader reads it first. Each code the provider routes
  and admission send today has its own message: `auth.rate_limited`, `auth.provider_not_found`,
  `auth.provider_unavailable`, `auth.provider_denied`, `auth.provider_error`,
  `auth.id_token_invalid`, `auth.subject_missing`, `auth.flow_expired`, `auth.flow_invalid`,
  `auth.suspended`, `auth.admission_refused`, `auth.signup_closed` and
  `entitlement.limit_reached`; `auth.admission_refused` (07, the organization's rules) has the
  same message where a local sign-in answers it as a problem. Anyone can write a link to
  `/sign-in?error=...`, so an unknown value is shown in `Code` only when it has a sign-in code's
  shape: two or three lowercase words joined by dots, the first `auth`, `admission`,
  `entitlement`, `local` or `org`, at most 64 characters. Anything else gets the general message
  alone and never reaches the page.
- **The password step (B16).** The email step goes to `/sign-in/passkey` when passkeys are
  offered and to `/sign-in/password` when only passwords are. Each of the two steps links to the
  other when the document offers both, handing the address on in the history entry's state; the
  passkey step does not guess from the server's options whether the user has a passkey (Q9 may
  change what those reveal). The step posts the address, the password and `return_to` to
  `POST /auth/local/password` and completes sign-in as the passkey step does. The form holds the
  address in a hidden, read-only `autocomplete="username"` field, so a password manager files the
  password under the account. The password lives
  only in the field and the request body; while a request is in flight, Enter sends nothing more.
  `auth.credentials_invalid` and the password step's `auth.method_disabled` have their own
  messages; its 429 message names no network, since the route also counts failed checks per
  account (08 B40).
- **The TOTP step (B16; 04 B15, B20, B21).** A right password for an account with TOTP answers
  `{next: "totp"}` and no session; the password step then opens `/sign-in/totp` with the address
  in the history entry's state (a typed URL without it goes back to the email step). The field
  asks for the app's code with `autocomplete="one-time-code"` and `inputmode="numeric"`; "Use a
  recovery code instead" swaps it for a recovery code field (and back), emptying it and moving
  focus to it. The step posts `{code}` to `POST /auth/local/totp`; while a request is in flight
  it sends nothing more. The server uses the flow up on a wrong code and refuses a missing,
  finished or expired one, so after `auth.totp_invalid`, `auth.flow_invalid` or
  `auth.flow_expired` the step replaces its history entry with the password step, carrying the
  reason in the entry's state; the password step shows it once above the form, focused (the
  navigation focuses an element marked `data-focus-on-arrival` in place of the `h1`), until the
  next attempt. Other failures (`auth.suspended`, `auth.admission_refused`,
  `auth.method_disabled`, a 429) stay on the step with their messages; "Start again" goes to the
  email step. The step does not check the code's form: the server takes six digits with spaces,
  or a recovery code in any letter case.
- **The magic link (04 I8, #28)** has no server routes yet, so no step is drawn for it.
- **End to end.** `just e2e` inserts one `oidc` provider, `example-id`, whose config holds only a
  `label`, into the stack's organization, since no route creates providers yet (06 I5). Its start
  route answers `/sign-in?error=auth.provider_unavailable` without contacting any server, so the
  test checks the button's full-page navigation, the 303 and the message after it. Sign-in at a
  real provider (T16) waits for the mock OpenID Connect server in the e2e stack (06 I7, #39). The password
  test adds a user through `POST /api/v1/users`, sets a password through an enrollment link and
  signs in on the screen; a second test turns TOTP on for that user through `/api/v1/me/totp`,
  computing codes from the secret as an app would (RFC 6238), and signs in with a wrong code (back
  to the password), the app's code and a recovery code. The method list's other states run axe in the browser with synthetic
  documents, in light, dark and high contrast at 1280 and 375 px, in the default brand and in the
  vZDC example brand (its rules from the token build, since changing the stack's branding would
  reach every other test).

## Issues

In dependency order. Each is one PR.

- **I1. `useStepclimbStrings()` and ICU messages.** Deliverable: a strings provider and hook in
  `packages/ui` with the chosen ICU library, the English catalogs for `packages/ui` and `web/`,
  `web/src/messages.ts` replaced, terminology override merge with a fixed in-code override for
  tests, and the missing-id catalog test. Covers B23 to B26; T10, T11. Depends on: Q10 answered;
  01-repo-and-ci (CI gates).
- **I2. `/theme.css` route and theme link.** Deliverable: the server route serving the default
  theme CSS from the tokens build (and the organization's when 12 I8 lands), cache and
  fingerprint headers, the `<link>` in `web/index.html` before the bundle, the Vite proxy for
  `/theme.css` and `/auth`, the route in the OpenAPI document. Covers B20, B22; T13, T20.
  Depends on: Q4 and Q5 answered; 12 I1 (the built default theme with every token);
  02-organizations-and-accounts (organization from host). The server route itself, with
  `Cache-Control: no-cache`, landed with 12 I8 (coordinator decision, 2026-11-14, in spec 12);
  this issue replaces its cache headers with Q4's fingerprint and adds the rest.
- **I3. Router, layouts and AppShell wiring.** Deliverable: the route tree with signed-out and
  signed-in layouts, React Aria `RouterProvider` wired to TanStack Router, AppShell with SideNav,
  BottomTabs and TopBar, data-driven navigation entries, document titles, focus on navigation,
  the skip link. Covers B1 to B8; T1, T15, T17, T18. Depends on: I1; 13 I8 (AppShell, SideNav,
  BottomTabs, TopBar); Q1 and Q2 answered.
- **I4. `/me` load, session expiry and sign-out.** Deliverable: the `/me` query gating signed-in
  routes, the delayed Skeleton, 401 handling with return target and cache clearing, the user menu
  and sign-out, navigation entries gated by permission. Covers B5, B9 to B12, B15; T2 to T5, T7.
  Depends on: I3; 03-users-and-identities (the `/me` response); 08 (sign-out endpoint and
  session expiry); 13 I13 (Toast).
- **I5. Sign-in screens.** Deliverable: the enrollment page, the email and passkey step routes,
  error messages per problem `code` for those routes. Covers B15, B16 and B17 for the local
  passkey method; T7, T8, T19 (sign-in screen and enrollment page). Depends on: I3; 04 I2 to I4;
  08 (codes). Narrowed by the coordinator (2026-11-15): the method list (B13), provider buttons
  (B14), the callback error (B18), the password, TOTP and magic-link steps, and T6 and T16 moved
  to issue #171, which depends on 08 I9 (`/.well-known/stepclimb`), 04 I6 to I8, 05 and 06. #171
  built B13, B14, B18, the password and TOTP steps, and T6; the magic-link step goes with 04 I8,
  and T16 with the mock OpenID Connect server (#39).
- **I6. Appearance choice and OrgSwitcher.** Deliverable: the appearance and density settings
  applied before first paint as Q6 decides, and a test that the OrgSwitcher stays out of the
  DOM (B19). Covers B19, B21; T9. Depends on: I4; Q6 answered; 12 I4 (high-contrast theme).
- **I7. Error, empty and offline states, and the shell's end-to-end checks.** Deliverable:
  not-found, route-error, server-unreachable and empty states in both layouts, the Playwright
  checks at 1280 and 375 px with axe in three themes. Covers B27 to B30; T12, T14, T19.
  Depends on: I4, I5; 13 I9 (EmptyState, ErrorState, Skeleton).
