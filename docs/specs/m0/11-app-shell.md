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
  default appearance applies until the user chooses. Where the choice is stored, and how it is
  applied before first paint without an inline script, is Open question Q6. Density is set the
  same way with `data-sc-density`.
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
- **B26.** A message id missing from the catalog fails the Vitest suite (a test walks the source
  for ids and checks the catalog), and at runtime renders the id in development builds and the
  English default in production builds, never an empty string.

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

This spec adds one route to the server and consumes others.

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /theme.css` | none; the organization comes from the request's host as 02 resolves it | `200 text/css`, cache headers per Q4 | `404` only if 02 finds no organization for the host | public (the sign-in screen needs it) |

`/theme.css` is in the OpenAPI document under the `operations` tag beside `/healthz`, with a
`text/css` response, so the generated client and the Tauri app (M5) can find it. It is not under
`/api/v1` because the plan names the path (`docs/plan/ui.md` -> Organization theming).

Consumed, defined elsewhere: `GET /api/v1/me` (03), `GET /.well-known/stepclimb` (08 B38), the
sign-in endpoints and their `code` values (04, 05, 06, 07), `POST /auth/sign-out` (08). The web app
reaches all of them only through `@stepclimb/api-client`, except the provider start URLs (B14),
which are full-page navigations by design.

## Data

No tables. The appearance choice and terminology overrides are stored wherever Q6 and Q7 decide.

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
  lacks (proved once by adding one).
- **T11.** (B25) With an override Course -> "Training module", every message using the term
  renders the override, in singular and plural.
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

There is no protected route in this spec, so no four-case permission test; `/theme.css` gets the
cross-organization test that 02 defines for host resolution: one organization's host never
receives another's theme.

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

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

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
  02-organizations-and-accounts (organization from host).
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
- **I5. Sign-in screens.** Deliverable: the method list, provider buttons, the local-account step
  routes (email, passkey, password, TOTP, magic link sent), error messages per problem `code`,
  callback error display. Covers B13 to B18; T6, T8, T16, T19 (sign-in screen). Depends on: I3;
  04, 05, 06, 07, 08 (endpoints and codes; 08 I9 for `/.well-known/stepclimb`).
- **I6. Appearance choice and OrgSwitcher.** Deliverable: the appearance and density settings
  applied before first paint as Q6 decides, and a test that the OrgSwitcher stays out of the
  DOM (B19). Covers B19, B21; T9. Depends on: I4; Q6 answered; 12 I4 (high-contrast theme).
- **I7. Error, empty and offline states, and the shell's end-to-end checks.** Deliverable:
  not-found, route-error, server-unreachable and empty states in both layouts, the Playwright
  checks at 1280 and 375 px with axe in three themes. Covers B27 to B30; T12, T14, T19.
  Depends on: I4, I5; 13 I9 (EmptyState, ErrorState, Skeleton).
