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

1. **Push.** APNs on iOS delivers to an app only for a sender holding the app owner's credentials.
   The signing key is one you "request from your developer account", the token's `iss` is "the
   10-character Team ID", and "APNs relates a team ID and associated bundle IDs to a connection"
   [1]. The `apns-topic` header is required and "in general, the topic is your app's bundle ID"
   [2]. A self-hosted server cannot hold the maintainer's key, so it cannot push to the official
   app directly. FCM on Android (plan: after 1.0) works the same way by inference: Nextcloud's
   documentation says pushes "need to be signed with a generated push key or certificate from the
   same developer account" for both [3].
2. **Version skew.** Self-hosted servers update on their own schedule, so the app will meet
   servers older and newer than itself. The discovery document has no version today.
3. **Selling from the app.** The App Store Review Guidelines (last updated October 16, 2026) [5]
   restrict buttons and links to buy outside in-app purchase on most storefronts; the United
   States storefront needs no entitlement for them (3.1.1(a)). Section 3.1.3(b) lets an app give
   access to things bought elsewhere only if they are also sold in the app. Two sections could
   cover Stepclimb instead:
   - 3.1.3(c), "Enterprise Services", applies only if the app "is only sold directly by you to
     organizations or groups for their employees or students"; "consumer, single user, or family
     sales must use in-app purchase". A club or a VATSIM facility has members, not employees or
     students, so this may not fit.
   - 3.1.3(f), "Free Stand-alone Apps", covers a free companion to a paid web-based tool, provided
     there is "no purchasing inside the app, or calls to action for purchase outside of the app".
   Which section Apple applies is not settled; see Consequences.

## Options

### Option A: one app, a push relay the maintainer runs, version in discovery, billing on the web

- **Push relay.** The maintainer runs a small push relay that holds the APNs (later FCM)
  credentials.
  - Any Stepclimb server, managed or self-hosted, registers with it once for a relay key and
    sends it a wake-up for a device token.
  - The payload carries no learning content: a generic title, a count, and at most an opaque id.
  - The app wakes, then fetches the notification from the user's own server with its own token.
  - Nextcloud runs this pattern for its official apps [3]. Because the notification comes from the
    proxy, Apple and Google never learn which Nextcloud server sent it. Separately, the server
    encrypts the content to the device's public key, so the proxy cannot read it. Matrix's Sygnal
    gateway is the same shape: it holds the vendor's APNs key, team id and the app's bundle id as
    topic [4].
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
  Technical -> Data model (core tables)); a relay kind or relay settings get added beside it.
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

## Decision

Option A, accepted by the owner on 2026-11-17. One Stepclimb app serves every server. The
maintainer runs a content-free push relay that any server can opt in to. The discovery document
carries version fields, and billing stays on the web with no purchasing or purchase links in the
app.

## Consequences

- **Bundle identifier.** The app's bundle identifier is a contract with every server: the
  compiled-in client accepts only `dev.stepclimb.shell:/oauth/callback` (`clients.rs`), the Tauri
  shell uses `dev.stepclimb.shell` (`apps/app/src-tauri/tauri.conf.json`), and the APNs topic is
  the bundle id. The App Store build ships under `dev.stepclimb.shell`, or the change lands in the
  server before M5, so that older self-hosted servers keep accepting the official app's sign-in.
- **Discovery.** Add `version`, `api_versions` and `min_app_version` to `/.well-known/stepclimb`,
  in its spec and the OpenAPI document. The app checks them when a server is linked and on each
  launch. When this lands is for the discovery and app specs to set; it has to precede the first
  App Store release.
- **Relay spec (before M5).**
  - The relay API: registration, the relay key, send and revoke.
  - The payload: no content, or content encrypted to the device as in Nextcloud.
  - Rate limits and abuse handling.
  - Where it runs, and whether it is open source in this repository. It should be, under the same
    license.
  - How a self-hosted server opts in. The recommendation is a setting that is off by default,
    since it sends device tokens and a count to the maintainer's service, and the self-hosting
    guide says so.
- **Notifications spec.** Treat APNs delivery through the relay as one more channel, following
  the same preferences (plan, Technical -> Notifications).
- **App spec, open question.** How the app's account list is keyed: by server, or by server and
  organization. That depends on how managed hosting resolves organizations (a subdomain or a
  custom domain each, per the plan), which is not built yet.
- **App Store submission (M5) and the billing spec (M6).** Settle which guideline section covers
  the app, 3.1.3(f) or 3.1.3(c), against the guidelines current at submission. Under both, the
  app keeps no purchasing and no calls to action, and the operator console and Stripe flows stay
  web-only.
- **Revisit.** If Apple's reading differs at submission, or when Android is scheduled (FCM through
  the same relay).

## Sources

1. Apple, "Establishing a token-based connection to APNs", accessed 2026-11-17:
   https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns
   (primary; read through its JSON form at developer.apple.com/tutorials/data/documentation)
2. Apple, "Sending notification requests to APNs", accessed 2026-11-17:
   https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns
   (primary)
3. Nextcloud notifications app, "Push notifications v2", accessed 2026-11-17:
   https://github.com/nextcloud/notifications/blob/master/docs/push-v2.md (vendor documentation)
4. element-hq/sygnal README, accessed 2026-11-17: https://github.com/element-hq/sygnal (vendor
   documentation; the earlier matrix-org repository is archived)
5. Apple, App Store Review Guidelines, last updated October 16, 2026, sections 3.1.1, 3.1.3(b),
   3.1.3(c) and 3.1.3(f), accessed 2026-11-17: https://developer.apple.com/app-store/review/guidelines/
   (primary)
