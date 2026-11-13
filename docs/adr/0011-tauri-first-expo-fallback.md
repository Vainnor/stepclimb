# 0011. Tauri first for the apps, Expo as the iOS fallback

- **Status:** Accepted
- **Date:** 2026-11-12
- **Decider:** the owner, in the plan v1.0 (12 Nov 2026), Technical -> Decided
- **Related:** `docs/plan/technical.md` -> Web app and native apps; `.claude/rules/cross-platform.md`

## Context

One UI codebase should serve the web, desktop and iOS.

## Decision

The apps are a Tauri 2 shell (`apps/app`) around the same `web/` build and Stepclimb UI, for
macOS, Windows, Linux and iOS. A few-day iPhone test around week 3 must show VATSIM sign-in through
the system browser, a page and a timed cloze question with correct keyboard behavior, scrolling and
safe areas, and an APNs push. If it fails, the iPhone app is built with Expo, reusing Stepclimb
UI's web components through Expo's DOM components; desktop stays on Tauri. Android is not
scheduled.

## Consequences

`apps/app` builds unsigned in CI from M0. The Apple developer account is the maintainer's,
created before M5.
