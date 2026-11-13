---
paths:
  - "apps/app/**"
  - "justfile"
  - "just/**"
  - "web/index.html"
  - "web/vite.config.ts"
---
# Cross-Platform Rules (not loaded at startup; read in full before touching the Tauri app, the web app's shell, or the gates)

One React app runs in browsers, as an installable web app, and inside the Tauri 2 shell on macOS,
Windows, Linux and iOS (plan, Technical -> Web app and native apps; ADR 0011).

---

## Browsers and the web app

- Current Chrome, Edge, Firefox and Safari, desktop and mobile. A feature missing in one of them
  needs a fallback or an ADR.
- Phone first: 44 px touch targets, a bottom tab bar under 768 px, layouts checked at 375 px.
- Installable: manifest and service worker; Web Push on iOS works only once the app is on the
  home screen (iOS 16.4+).

## iOS WKWebView (Tauri iOS and Safari)

- Inputs are 16 px or larger, so focusing one does not zoom the page.
- Respect safe areas (`env(safe-area-inset-*)`, `viewport-fit=cover`); fixed bars sit inside them.
- No rubber-band bounce on fixed chrome; no text selection on controls.
- The software keyboard resizes the visual viewport: test that the focused field and the primary
  action stay visible.

## Desktop (Tauri)

- The shell adds native features only through Tauri plugins with the narrowest capability in
  `apps/app/src-tauri/capabilities/`. It has no second UI codebase.
- Platform-specific code lives in small named modules selected with `cfg`, never inline in shared
  logic.
- No signing or store work until M5; `just app-build` builds unsigned.

## The gate where you work (ADR 0021)

- There is no hosted CI. `just ci` on the committed HEAD, on the machine you work on, is the merge
  gate; every "done" names that OS. Never add a GitHub Actions workflow: it spends the private
  repository's minutes.
- A change to `apps/app` also runs `just app-build` on the local OS. A change that needs macOS or
  Windows (platform code, signing, the iOS build) runs there on a machine or VM the owner
  provides; ask for it, and say in the PR which OSes ran and which did not.
- Jobs of about an hour or more (long property runs, load tests, cold image builds for several
  architectures) go to a VM the owner provides. Ask first; report machine, commit, what ran and
  the result.
