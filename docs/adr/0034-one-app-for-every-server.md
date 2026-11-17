# 0034. One app for every server, with a push relay

- **Status:** Accepted
- **Date:** 2026-11-17
- **Decider:** the owner (Vainnor), in session on 2026-11-17
- **Related:** `docs/plan/technical.md` -> Tauri app, Notifications, Plans and billing; ADR 0006
  (hosting modes), ADR 0007 (billing), ADR 0011 (Tauri first), ADR 0032 (app tokens);
  `docs/specs/m1/17-tauri-iphone-test.md`; issue #346

## Context

The owner plans to run managed hosting, where organizations pay for their instance, and to
publish one Stepclimb app under the maintainer's Apple developer account (plan, Technical ->
Tauri app -> Distribution; ADR 0011). Organizations that self-host should be able to use that
same app. Asking a self-hosting organization to pay for an Apple account and publish its own
build is not realistic for a club or a school.

Most of this is already decided or built:

- **The plan.** It says a user enters a server URL or scans a QR code, the app reads
  `/.well-known/stepclimb`, and "several servers can be linked". Sign-in is PKCE in the system
  browser, with tokens in the OS keychain.
- **Discovery.** `/.well-known/stepclimb` exists (`crates/stepclimb-server/src/discovery.rs`). It
  returns the organization's name, sign-in methods, theme URL and OAuth endpoints, with no
  secrets.
- **The OAuth client.** Every server compiles in the app's public OAuth 2.1 client,
  `stepclimb-app` (`crates/stepclimb-auth/src/oauth_server/clients.rs`). It has private-use-scheme
  and loopback redirects (RFC 8252). A self-hosted server therefore accepts the official app with
  no registration step, and so does a managed one.

What the plan does not settle:

1. **Push.** APNs on iOS, and FCM on Android once that is scheduled (plan: after 1.0), deliver to
   an app only when the sender holds the app owner's credentials. Evidence points at this:
   Nextcloud's push documentation says pushes "need to be signed with a generated push key or
   certificate from the same developer account" [1], and Sygnal's APNs settings take the vendor's
   key, team id and the app's bundle id as topic [2]. Apple's own page did not load when checked
   on 2026-11-17, so this is inferred, not verified. A self-hosted server cannot hold the
   maintainer's key, so it cannot push to the official app directly.
2. **Version skew.** Self-hosted servers update on their own schedule, so the app will meet
   servers older and newer than itself. The discovery document has no version today.
3. **Selling from the app.** Apple's guidelines restrict in-app links and buttons to buy outside
   in-app purchase on most storefronts. Section 3.1.3(b) lets an app give access to things bought
   elsewhere only if they are also sold in the app [3]. Section 3.1.3(c) has an exception for
   services sold directly to organizations for their members. Whether managed-hosting plans fit
   it is inferred, not verified: the guideline text came back as a summary when checked.

## Options

### Option A: one app, a push relay the maintainer runs, version in discovery, billing on the web

- **Push relay.** The maintainer runs a small push relay that holds the APNs (later FCM)
  credentials.
  - Any Stepclimb server, managed or self-hosted, registers with it once for a relay key and
    sends it a wake-up for a device token.
  - The payload carries no learning content: a generic title, a count, and at most an opaque id.
  - The app wakes, then fetches the notification from the user's own server with its own token.
  - This is the pattern Nextcloud runs for its official apps. It goes further and encrypts the
    content with a device key, so "Google and Apple don't even know the Nextcloud server sending
    the notification" [1]. Matrix's Sygnal is the same shape [2].
- **Version in discovery.** `/.well-known/stepclimb` gains the server's version, the API versions
  it serves and the minimum app version it supports. The app gains its own minimum server
  version, so either side can say "update" instead of failing in an unclear way.
- **Billing on the web.** The app sells nothing and links to no purchase page. Organizations buy
  plans on the website; learners never buy anything. The app signs in to whatever the
  organization has.

Costs:

- **The relay.** One small service to run and monitor, its secrets, and abuse controls: a key per
  server, rate limits, and revoking a server's key.
- **No content in pushes.** The notification text is generic until the app fetches the real
  notification. The data model already holds a `push_subscriptions` row with kind `apns` (plan,
  Technical -> Data); a relay kind or relay settings get added beside it.
- **Review.** One ADR now and a relay spec before M5. It changes no M1 code.

### Option B: one app, no native push for self-hosted servers

The app works with any server, but only the managed service sends APNs pushes, signed directly
by the maintainer's key. Self-hosted users get in-app notifications, email, and Web Push from the
installable web app. The plan already has Web Push, which needs no relay because browsers run
their own push services; on iPhone it works only after the web app is added to the home screen
(plan, Technical -> Web app).

Costs: no relay to run. Self-hosted users of the native app get no lock-screen notifications,
which makes the app weaker exactly where the owner wants it to be equal.

### Option C: self-hosting organizations publish their own app

Each organization builds and signs the app under its own Apple account with its own push key.

Costs: $99 a year and an App Store review for every organization, plus a second listing per
organization that users must find. This is what the owner wants to avoid.

## Recommendation

Option A. Sign-in and multiple servers already work under it with no new code. The relay is the
one new piece, and it is small, a pattern other self-hostable projects already run, and kept
content-free, so it holds no learner data. Option B leaves self-hosters with a worse app, and
Option C is the cost this ADR exists to avoid.

Two points were inferred, not verified, and are checked before M5 (see Decision):

- **APNs credentials.** Apple's APNs documentation should confirm that the signing key is the
  developer account's and that `apns-topic` is the bundle id.
- **Section 3.1.3(c).** Read the guideline text itself to confirm that an organization-bought
  plan, used by its members in a free app with no purchase links, fits the exception on every
  storefront.

## Decision

Option A, accepted by the owner on 2026-11-17. One Stepclimb app serves every server. The
maintainer runs a content-free push relay that any server can opt in to. The discovery document
carries version fields, and billing stays on the web. The two points marked inferred above, the
APNs credentials and section 3.1.3(c), are checked against Apple's full text in the relay spec
before M5. If either reads differently, the relay spec or a superseding ADR says so.

## Consequences


- **Discovery (M4 or earlier).** Add `version`, `api_versions` and `min_app_version` to
  `/.well-known/stepclimb`, in its spec and the OpenAPI document. The app checks them when a
  server is linked and on each launch.
- **Relay spec (before M5).**
  - The relay API: registration, the relay key, send and revoke.
  - The payload: no content, or content encrypted to the device as in Nextcloud.
  - Rate limits and abuse handling.
  - Where it runs, and whether it is open source in this repository. It should be, under the same
    license.
  - How a self-hosted server opts in. It should be a setting that is off by default, since it
    sends device tokens and a count to the maintainer's service, and the self-hosting guide says
    so.
- **Notifications spec.** Treat APNs delivery through the relay as one more channel, following
  the same preferences (plan, Technical -> Notifications).
- **App.** The account list is keyed by server and organization. A managed user in two
  organizations sees two entries on one server, and a self-hosted server adds its own.
- **Billing (M6).** Billing stays on the website. The app shows no prices and no purchase links,
  and the operator console and Stripe flows stay web-only.
- **Revisit.** If Apple's rules or the 3.1.3(c) reading turn out differently, or when Android is
  scheduled (FCM through the same relay).

## Sources

1. Nextcloud notifications app, "Push notifications v2", accessed 2026-11-17:
   https://github.com/nextcloud/notifications/blob/master/docs/push-v2.md (vendor documentation)
2. matrix-org/sygnal README, accessed 2026-11-17: https://github.com/matrix-org/sygnal, now
   maintained at element-hq/sygnal (vendor documentation)
3. Apple, App Store Review Guidelines, section 3.1, accessed 2026-11-17:
   https://developer.apple.com/app-store/review/guidelines/ (primary; fetched as a summary, so
   the quoted conditions need checking against the full text)
