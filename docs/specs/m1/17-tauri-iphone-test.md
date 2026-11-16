# Tauri iPhone test

- **Status:** Draft
- **Milestone:** M1
- **Plan:** `docs/plan/technical.md` -> Web app and native apps (iPhone test: "A throwaway Tauri app
  on a real iPhone must: sign in with VATSIM through the system browser; render a page and a timed
  cloze question from Stepclimb UI with correct keyboard behavior, scrolling and safe areas;
  receive an APNs push. If it fails: the iPhone app is built with Expo instead, reusing Stepclimb
  UI's web components through Expo's DOM components. Desktop stays on Tauri."; Tauri app at 1.0:
  sign-in with PKCE in the system browser, APNs through a community plugin or a small Swift plugin
  of our own; Distribution: the maintainer's Apple developer account, set up before M5),
  Milestones (M1: one-week Tauri iPhone test), How it's built (what agents don't speed up:
  real-device testing); ADR 0011; `.claude/rules/cross-platform.md`
- **Depends on:** `../m0/08-sessions-and-csrf.md` (B30, B31, B45: the OAuth 2.1 server and the
  `stepclimb-app` client), `../m0/06-oauth2-and-vatsim-preset.md` (the VATSIM preset),
  `../m0/11-app-shell.md` (the Tauri shell), `07-pages-and-the-editor.md` (PageReader),
  `16-learning-ui-components.md` (the M1 components the test renders)
- **Review:** standard

## Summary

Before M5 commits to Tauri on iOS, the plan asks for a few days' test on a real iPhone: a
throwaway Tauri app must sign in with VATSIM through the system browser, render a page and a timed
cloze question from Stepclimb UI with the keyboard, scrolling and safe areas right, and receive an
APNs push. If any of the three fails, ADR 0011 moves the iPhone app to Expo. An agent can write the
throwaway app and every server-side path; only the owner can build it on a Mac, install it on their
iPhone, sign in with a real VATSIM account and receive a real push. This spec splits the work that
way and says what is recorded as the result.

## Behavior

### The throwaway app

- **B1.** The test app lives on the branch `spike/m1/iphone-test` under `spikes/iphone-test/`
  (Q1). It is never merged to `next`; only its result is (B12). It is a Tauri 2 app with an iOS
  target and the bundle identifier `dev.stepclimb.shell`, so it uses the `stepclimb-app` OAuth
  client and its registered redirect `dev.stepclimb.shell:/oauth/callback` (`08` B45) with no server
  change.
- **B2.** On first launch it asks for a server URL (a text field, 16 px or larger) and stores it;
  the owner's test server is the homelab's HTTPS instance (Q3).
- **B3.** It renders three screens from the `web/` build and Stepclimb UI, loaded from the app
  bundle as `apps/app` does:
  1. a page: PageReader (`07` B22) with a synthetic test page that has every M1 block (headings,
     a long table that scrolls on its own, a callout, a code block, an image, a transcript block),
     long enough to scroll several screens;
  2. a timed cloze question: a static mock built for the test from M0 and M1 components (a
     paragraph with three inline text fields and one select, a countdown timer that turns warning
     at 5 minutes and danger at 1, and a Submit button), since the real cloze renderer is M2;
     nothing is graded and no answer is stored;
  3. a status screen: the signed-in user from `GET /api/v1/me`, the APNs device token, and the
     last push received.

### Sign-in through the system browser

- **B4.** Sign-in follows `08` B31: the app generates the PKCE verifier and `state`, opens
  `/oauth/authorize` in the system browser session (Q2), the user signs in through the
  organization's sign-in page with the VATSIM preset (`06`), confirms the app, and the browser
  returns to `dev.stepclimb.shell:/oauth/callback`. The app checks `state`, exchanges the code at
  `POST /oauth/token`, and stores the tokens in the iOS Keychain. A token refresh is exercised once
  by the owner (B10).
- **B5.** The VATSIM preset needs a VATSIM Connect client registered by the owner with the
  homelab server's callback URL (Q4); for agent testing the same flow runs against the mock
  identity provider (ADR 0027) on Linux desktop Tauri, where deep links also work.

### APNs

- **B6.** The app registers for remote notifications with a small Swift plugin of our own or a
  community Tauri plugin, chosen in I3 after `dependency-researcher` (the plan names both), shows
  the device token, and displays a received push's title and body.
- **B7.** The push is sent by a test script in the spike (`spikes/iphone-test/send-push`), which
  signs an APNs provider token (JWT, ES256) with the owner's APNs auth key and posts one alert to
  the sandbox APNs endpoint over HTTP/2 for the device token; or, equally, from Apple's own push
  testing tool in the developer account. Stepclimb's server sends no push in M1 (Web Push is M4,
  APNs M5).
- **B8.** Push needs a paid Apple Developer Program membership: evidence points at the free
  personal team not offering the Push Notifications capability, which I3 confirms against
  Apple's current documentation before the owner's run. The plan creates the account before M5;
  this test is in M1 (Q5, the owner's).

### What counts as passing

- **B9.** The owner runs the checklist on their iPhone (current iOS, recorded with the model):
  - **Sign-in:** VATSIM sign-in through the system browser returns to the app signed in, with no
    password typed into the app; a killed and relaunched app is still signed in; a refresh works.
  - **Page:** smooth scrolling through the page, the table scrolls sideways on its own, no
    rubber-band bounce on fixed chrome, text is selectable in the reading column and not on
    controls, the content sits inside the safe areas in portrait and landscape and around
    the notch or Dynamic Island.
  - **Cloze:** tapping a field does not zoom; the keyboard does not cover the focused field or the
    Submit button; moving between fields with the keyboard's next arrow works; the timer keeps time
    across the keyboard opening and closing and while the app is in the background for 1 minute.
  - **Push:** a push arrives with the app in the background and in the foreground.
- **B10.** Each check is pass or fail with a screenshot or screen recording from the device, kept
  outside the repository unless it holds no personal data (no VATSIM account details, no real
  names).
- **B11.** Any failed check that a fix in the throwaway app cannot clear within the test's few
  days fails the test (plan). A failure caused by the test setup (the server, the VATSIM client,
  the account) is fixed and rerun, not counted.

### Recording the result

- **B12.** The result is a new ADR, "Result of the M1 iPhone test", Accepted by the owner: the date,
  device and iOS version, each check's outcome, and the consequence under ADR 0011: Tauri stays
  for iOS, or the iPhone app moves to Expo with desktop on Tauri. `docs/adr/README.md` and the
  M5 stub are updated in the same PR. It also records what the components learned (for example a
  keyboard fix), and `16` B2's iOS rows move from experimental for the components it checked.

## API

None added. The test uses `/oauth/authorize`, `POST /oauth/token`, `GET /api/v1/me` (`08`, `03`)
and the sign-in routes of `06`. No server change is expected; one that the test finds needed is
its own issue against the spec it belongs to.

## Data

None. The test server's database holds the owner's test user only; no data from the test enters
the repository (`content-and-ip.md`).

## Permissions

None added.

## Test cases

What an agent runs (Linux, x86_64):

- **T1** (B3). The three screens render in Playwright's WebKit at 390 x 844 with safe-area insets
  simulated, with screenshots in the default and vZDC themes, light and dark; the cloze mock's
  fields are 16 px or larger; axe finds no violation.
- **T2** (B4, B5). The spike's desktop build on Linux signs in through the mock identity provider
  with the system browser and the `dev.stepclimb.shell:` deep link, receives tokens, and calls
  `/api/v1/me`; a wrong `state` is refused.
- **T3** (B7). The push script builds a JWT the APNs documentation's format describes (header
  `alg: ES256`, `kid`; claims `iss`, `iat`), checked against a local HTTP/2 stub that verifies the
  signature with the test key; no network.

What only the owner runs (a Mac with Xcode, the iPhone, the Apple account, a VATSIM account):

- **T4** (B9). The checklist of B9 on the device, recorded per B10.

## Out of scope

- The real iOS app: server discovery, QR pairing, offline downloads in SQLite, APNs from the
  server, TestFlight and the App Store: M5.
- Expo work, if the test fails: a new spec after the ADR.
- Android: not scheduled (plan).
- Signing and provisioning for distribution: an ADR when M5 iOS work starts.

## Open questions

Q1 to Q4 were delegated by the owner to the coordinator; each recommendation is taken and stands
until the owner reviews it. Q5 is the owner's and stays open.

- **Q1. Where the throwaway app lives.** Options: (a) a branch with `spikes/iphone-test/`, never
  merged, the result recorded by an ADR on `next`; (b) the iOS target added to `apps/app` on
  `next`; (c) a separate repository. Recommendation: (a). The plan calls it throwaway, and adding
  an iOS target to `apps/app` now would put M5 work under `just app-build` before its ADR.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q2. How the system browser is opened.** Options: (a) `ASWebAuthenticationSession` through a
  Tauri plugin (community or a few lines of Swift in the spike), which iOS designs for this and
  which returns the callback to the app; (b) open Safari with the URL and receive the deep link
  through the deep-link plugin. Recommendation: (a), with (b) as the fallback if no plugin works;
  both satisfy the plan's "through the system browser".
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q3. Which server the test signs in to.** Options: (a) the homelab instance over HTTPS, which
  the owner runs (the plan's development and pilot host); (b) a laptop on the local network with a
  self-signed certificate, which iOS refuses without a profile. Recommendation: (a).
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q4. The VATSIM Connect client.** A real VATSIM sign-in needs a client registered with VATSIM
  for the homelab callback URL, an outside account step only the owner can take. Options: (a) the
  owner registers a development client (VATSIM's sandbox or production, the owner's choice) before
  the test; (b) test with the mock identity provider only, which no longer checks "sign in with
  VATSIM". Recommendation: (a); this question records the owner step, and I4 lists it.
  Coordinator decision (overnight, 2026-11-16): (a), pending owner review.
- **Q5. When the Apple Developer account is needed (open, for the owner).** The plan creates the
  account before M5; the APNs check of this M1 test needs it (B8), and so does keeping the app
  installed longer than a free provisioning profile lasts. The account is the maintainer's (plan,
  Distribution); what is open is when it is created, since it is an outside account and a yearly
  cost. Options: (a) create it before this test, around week 3 of M1, so all three checks run;
  (b) run sign-in, page and cloze with a free personal team now and the push check when the
  account exists in M5, keeping ADR 0011's decision open until then; (c) move the whole test to
  M5. Recommendation: (a). The push check is the one most likely to differ between Tauri and Expo,
  and the plan wants the decision made in M1.

## Issues

Agent work, on `spike/m1/iphone-test` (one PR each against that branch, reviewed but not merged
to `next`), then the owner's run and the ADR.

1. **I1. Spike shell and screens.** The Tauri 2 app in `spikes/iphone-test/` with an iOS target
   (generated by `tauri ios init` on the owner's Mac, or written to its documented layout and
   checked there), the server URL field, the page, the cloze mock and the status screen. Covers B1
   to B3; T1. Depends on: `07-pages-and-the-editor.md` I4, `16-learning-ui-components.md` I7.
2. **I2. Sign-in through the system browser.** PKCE, `state`, the deep link, the token exchange
   and refresh, Keychain storage (a plugin chosen after `dependency-researcher`), verified on Linux
   desktop with the mock identity provider. Covers B4, B5; T2. Depends on: I1.
3. **I3. APNs path.** The push registration plugin, the device token on the status screen, the
   send script with its local stub test, and the check of B8 against Apple's documentation.
   Covers B6 to B8; T3. Depends on: I1.
4. **I4. The owner's run.** Owner steps, in order: the Apple account (Q5), an APNs auth key
   (stored outside the repository and handed to the script by a file path), a VATSIM Connect client
   for the homelab (Q4), the homelab server updated to the M1 build, building the spike with Xcode
   on the Mac and installing it on the iPhone, running B9's checklist. Agent help: a written run
   sheet in the branch's README, fixes during the test days. Covers B9 to B11; T4. Depends on: I2,
   I3; the owner's answers to Q4 and Q5.
5. **I5. The result ADR.** Covers B12. Depends on: I4.
