---
paths:
  - "apps/app/**"
  - ".github/workflows/**"
  - "web/index.html"
  - "web/vite.config.ts"
---
# Cross-Platform Rules (not loaded at startup; read in full before touching the Tauri app, the web app's shell, or CI)

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
- No signing or store work until M5; CI builds unsigned.

## CI

- `just ci` runs on every PR in GitHub Actions on Linux. The Tauri build runs on Linux for every
  PR and on macOS and Windows for pushes to `main` (owner, 2026-11-12), on standard hosted
  runners only. Enabling a paid or larger runner is the owner's call.
- A change names the OSes it was built and run on, and the ones it was not.
