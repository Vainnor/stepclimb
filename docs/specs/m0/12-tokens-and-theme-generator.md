# Tokens build and theme generator

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/ui.md` -> Library architecture, Tokens, Organization theming, Principles
  (accessible is the floor), Definition of done (Themes); `docs/plan/technical.md` -> Decided
  (Terrazzo for design tokens), API surface (`/api/v1/admin/branding`), Data model
  (`organizations.branding`)
- **Depends on:** 01-repo-and-ci (CI gates), 02-organizations-and-accounts (the organization and
  its `branding` column), 09-permissions-and-roles (the permission for branding and the `Require`
  extractor), 11-app-shell (the `/theme.css` route)
- **Review:** maintainer line by line for I8 (a permission, a feature check and an audit
  entry); standard for the rest

## Summary

`packages/tokens` turns one set of DTCG design-token JSON into the CSS variables, the Tailwind
theme and the docs tables every Stepclimb surface uses, and holds the theme generator: an
organization's brand color in, accessible light, dark and high-contrast palettes out, with a
contrast report that stops a failing palette from being saved. The generator runs at build time
for the default themes and on the server when an organization admin saves branding.

## What exists today

Read on 2026-11-12. This spec describes it, then adds to it.

- **Source.** DTCG 2025.10 JSON in `packages/tokens/tokens/`: `base.json` (space, radius, text
  sizes, weights, control heights, motion durations, the `out-expo` easing, the reading and mono
  font stacks) and `themes/{default,vzdc}-{light,dark}.json` (every color token and the UI font
  per theme). `resolver.json` puts the `base` set first, then a `theme` modifier with those four
  contexts, `default-light` by default.
- **Build.** `terrazzo.config.ts` with `@terrazzo/cli`, `@terrazzo/parser` and
  `@terrazzo/plugin-css` at 2.10.0 writes `build/css/variables.css`: every token as `--sc-*`
  (`color.bg-canvas` -> `--sc-bg-canvas`, `radius.sm` -> `--sc-radius-sm`). Default light is on
  `:root, [data-sc-theme="light"]`; default dark on `[data-sc-theme="dark"]` and inside
  `@media (prefers-color-scheme: dark)` on `:root:not([data-sc-theme="light"])`; the vZDC example
  on `[data-sc-brand="vzdc"]` in the same pattern. Permutations after the first carry only
  `color.*` and `font.ui`.
- **Tailwind theme.** A local plugin in the same file writes `build/css/tailwind-theme.css`: an
  `@theme inline` block that starts with `--color-*: initial;` (Tailwind's palette removed) and
  maps each token onto its variable, dropping the `bg-` of surfaces (`--color-canvas`); each
  motion token becomes an `@utility duration-<name>`, since Tailwind 4's theme has no duration
  namespace.
- **Tests.** `test/build.test.ts` (Vitest) reads the built files and checks the selectors, the
  `sc-` prefix on every variable, the vZDC brand value `oklch(31% 0.1 25)`, the removed palette,
  the mappings and the duration utilities.
- **Wiring.** `just web-build` runs `pnpm --filter @stepclimb/tokens build` before the web
  build; `packages/ui/src/styles/index.css` imports both built files; `build/` is ignored by
  git.
- **Not there yet.** No `src/generate.ts`; no `bg-overlay`, `fg-disabled`, `info*` or
  `state-*` tokens; no high-contrast theme; no `[data-sc-density="compact"]`; no docs tables;
  no line-height, reading-size or shadow tokens.

The plan's layout puts the token JSON at `packages/tokens/src/tokens/*.json`; the repository has
it at `packages/tokens/tokens/`. See Open question Q1.

## Behavior

### Token set

- **B1.** Every token in the plan's Tokens table exists in every theme, named as the plan names
  it: surfaces `bg-canvas`, `bg-raised`, `bg-sunken`, `bg-overlay`; text `fg-strong`,
  `fg-default`, `fg-sub`, `fg-soft`, `fg-disabled`; lines `line-strong`, `line`, `line-soft`;
  brand `brand`, `brand-hover`, `brand-soft`, `brand-fg`, `on-brand`, `ring`; status
  `success|warning|danger|info` each with `-soft` and `-fg`; learning `state-locked`,
  `state-available`, `state-progress`, `state-complete`, `state-overdue`, `state-review`. Values
  the plan does not give are Open questions Q2 (info), Q3 (learning-state targets) and Q4 (the
  rest).
- **B2.** Learning-state tokens are DTCG aliases (`{color.<target>}`), not values, so each state
  is retuned in one place (`docs/plan/ui.md` -> Tokens: "Aliases, so learning states can be
  retuned in one place"). They resolve per theme like any other color.
- **B3.** The scales match the plan: type 12, 13, 14, 16, 18, 20, 24, 30, 38 px; weights 400,
  500, 700; space 2 to 72 px; radius xs 3, sm 5, md 8, pill; control heights comfortable 40 and
  32, compact 32 and 26, 44 minimum on touch; motion 120, 180, 320 ms with an exponential
  ease-out. `base.json` already holds all of these except the compact heights.
- **B4.** `[data-sc-density="compact"]` sets `--sc-control-h: 2rem` and
  `--sc-control-h-sm: 1.625rem` (the plan's generated CSS), on any element, so a view (the
  grading queue) can be compact inside a comfortable page. Comfortable is the default with no
  attribute. Density never changes `--sc-control-touch`.
- **B5.** A high-contrast theme exists for every brand and applies on
  `[data-sc-theme="high-contrast"]` (the plan's generated CSS). `@media (prefers-contrast: more)`
  also selects it on `:root` when the page sets neither `light` nor `dark`, and wins over
  `prefers-color-scheme: dark` there, as ThemeProvider decides (Q5). `forced-colors` is handled
  in the base styles (13), not by a theme.
- **B6.** The plan's other named-but-unvalued tokens are added once Q4 gives values: the reading
  size and line height (17 to 18 px at 1.65 to 1.7), the floating-layer shadow (one soft shadow;
  panels use a 1 px line and no shadow), and the overlay scrim.
- **B7.** Token changes are made in the JSON (colors: the generator's inputs and stops, B23) and
  rebuilt; nothing edits `build/` (`.claude/rules/ui.md`).

### Build outputs

- **B8.** `variables.css` keeps today's naming and selectors and gains the high-contrast and
  density selectors. Its order is fixed: default light, default dark by attribute, default dark
  by media query, then each further theme, so the theme resolution order holds (`data-sc-theme`
  first, then `prefers-color-scheme`; `docs/plan/ui.md` -> What we borrow).
- **B9.** `tailwind-theme.css` maps every new token. `state-*` and `info*` become color utilities
  (`text-state-overdue`, `bg-info-soft`); `bg-overlay` becomes `bg-overlay`.
- **B10.** The build writes the docs tables: for each group in the plan's Tokens table, one row
  per token with its name, CSS variable, Tailwind utility, description and value in every theme
  of the default brand. Storybook renders them on a Tokens page (13-base-ui-components). The
  tables are generated from the same JSON as the CSS, so they cannot drift.

### Theme generator (`packages/tokens/src/generate.ts`)

- **B11.** `generate.ts` is TypeScript with no DOM dependency (`docs/plan/ui.md` -> Library
  architecture), so it runs in Node at build time and in the browser for a live preview. It is
  deterministic: the same inputs and generator version give byte-identical output.
- **B12.** Input: the primary color, plus the other branding inputs the plan lists that affect
  tokens (optional accent, UI font, reading font serif or sans, corner style; Q6 to Q8). The
  primary color is accepted as a CSS hex color at minimum (vZDC's is `#500E0E`), converted to
  OKLCH.
- **B13.** The generator builds a 12-step OKLCH scale at the brand's hue, and the neutral scale at
  the same hue with low chroma (the default theme's neutrals carry 0.003 to 0.014). Each color is
  gamut-mapped into sRGB before it is measured or written, because WCAG contrast is defined on
  sRGB. The step lightness and chroma curves are Open question Q9.
- **B14.** In each of light, dark and high contrast, `brand` is the scale step that clears 4.5:1
  against `on-brand`, where `on-brand` is the scale's light end in light themes and its dark end
  in dark themes (as the hand-written defaults do: `on-brand` is `oklch(0.99 0.005 195)` in
  default light and `oklch(0.18 0.02 195)` in default dark). Which step to take when several
  clear is part of Q9. "Clears" is measured as B20 measures it, on the displayed 8-bit colors.
- **B15.** The other brand tokens come from the same scale: `brand-hover` one step past `brand`
  away from `on-brand`, `brand-soft` a tint near the canvas, `brand-fg` the step that reads as
  text on `brand-soft` and on `bg-canvas`, `ring` the step that clears 3:1 against `bg-canvas`
  and `bg-raised`.
- **B16.** Dark and high-contrast themes are derived from the same input, not authored
  (`docs/plan/ui.md` -> Organization theming). Each appearance has its own brand and neutral
  stops, fitted so the default brand reproduces its themes (Q9). In dark, `on-brand` is the
  scale's dark end and `brand` the passing step nearest lightness 0.76; in high contrast the
  neutrals are grey, `brand` the passing step nearest 0.38 and `brand-fg` one step past it. The
  high-contrast target is Q10's: 7:1 text, 4.5:1 UI.
- **B17.** Status colors (`success`, `warning`, `danger`, `info`) do not follow the brand. The
  hand-written vZDC themes already carry the default's status values unchanged; the generator
  keeps that, with one set per appearance including high contrast.
- **B18.** Organizations change color, logo, font and words, but not spacing, hierarchy or
  contrast (`docs/plan/ui.md` -> Principles). The generator therefore outputs only color tokens,
  `font.ui`, `font.read` and the radius tokens corner style controls; it never outputs space,
  type size, weight or control height.
- **B19.** Output is a theme object per appearance, serializable both to DTCG JSON (the build's
  input) and to the CSS of B8, with the same variable names, so a server-rendered `/theme.css`
  and `variables.css` are interchangeable.

### Contrast report and guardrails

- **B20.** `contrastReport(theme)` measures every required pair with the WCAG 2 contrast ratio
  and returns, per pair, both tokens, the ratio, the minimum and pass or fail. Contrast is
  measured on the 8-bit sRGB values a display shows: each color's sRGB channels are clipped to 0
  to 1 and rounded to one of 256 levels, then the WCAG 2 formula is applied. One function,
  `contrast()` in `src/color.ts`, does this for the generator's step choice and for the report
  (coordinator decision, 13 Nov 2026, PR #124). The pairs:

  | Pair | Minimum | Why |
  | --- | --- | --- |
  | `fg-strong`, `fg-default`, `fg-sub`, `fg-soft` on `bg-canvas`, `bg-raised`, `bg-sunken` | 4.5:1 | Text; the plan sets `fg-soft` (placeholder, locked) at 4.5:1 or more |
  | `on-brand` on `brand` and on `brand-hover` | 4.5:1 | Text on the primary action |
  | `brand-fg` on `brand-soft` and on `bg-canvas` | 4.5:1 | Text on tints, links |
  | `<status>-fg` on `<status>-soft` and on `bg-canvas`, for each status | 4.5:1 | Feedback text |
  | `brand`, `ring`, `line-strong` on `bg-canvas` and `bg-raised` | 3:1 | UI components and focus (WCAG 1.4.11) |
  | each `state-*` on `bg-canvas` and `bg-raised` | 3:1 | Learning-state icons and shapes |
  | `line-strong` on `bg-sunken` | 3:1 | Input borders on wells and inputs (decided by the owner, 13 Nov 2026) |

  `fg-disabled` is not checked: WCAG exempts disabled controls. The high-contrast theme's pairs
  use Q10's minimums. A learning state is measured as the token it aliases (Q3).

  The report also carries warnings, which never fail a theme. One exists: `brand-near-danger`,
  when `brand`'s hue is under 30 degrees from `danger`'s and `brand`'s chroma is at least 0.04,
  since a learner may then take an in-progress state for an overdue one (coordinator decision,
  13 Nov 2026).
- **B21.** A palette with any failing pair cannot be saved (`docs/plan/ui.md` -> Organization
  theming, Guardrails). The server runs the report on the generated theme before saving and
  refuses with the report attached. The browser preview runs the same function, so the screen
  can show the report before the admin presses Save.
- **B22.** The build fails when a committed theme (default, the vZDC example, or a generated
  default) has a failing pair, so the shipped defaults meet WCAG 2.2 AA in every theme.

### Build time and server

- **B23.** At build time the generator produces the default brand's themes (and the vZDC
  example's, which Storybook uses). Its output replaces the hand-written theme JSON (Q11):
  `src/themes.ts` names each brand's inputs, and the Terrazzo config generates every theme, runs
  the report (B22), and writes a copy of `resolver.json` with each theme context filled inline
  to `build/tokens/resolver.json`, which Terrazzo reads. The source resolver's theme contexts are
  empty. Generated colors are inside sRGB (B13), so `variables.css` has no `color-gamut`
  overrides.
- **B24.** When an organization admin saves branding, the server validates the inputs, generates
  the theme, runs the report (B21), stores the inputs and the rendered CSS with its hash, and
  serves that CSS at `/theme.css` (11-app-shell B20) from then on. How the TypeScript generator
  runs for a Rust server is Open question Q12. Whatever the answer, the server's output for a
  set of inputs equals `generate.ts`'s output: a shared set of golden vectors (inputs and
  expected CSS) runs against both in CI.
- **B25.** An organization without saved branding gets the default theme at `/theme.css`.
- **B26.** Custom CSS (scoped under `.sc-custom`, marked unsupported across upgrades) is an
  advanced branding setting in the plan with no milestone; it is Open question Q13.

## API

| Method and path | Request | Response | Errors | Permission |
| --- | --- | --- | --- | --- |
| `GET /api/v1/admin/branding` | none | `200` the saved inputs, the generator version, the CSS hash and the contrast report | `401`, `403`, `404` (problem+json) | branding permission (below), organization scope |
| `PUT /api/v1/admin/branding` | the inputs of B12 (JSON) | `200` as GET | `422 branding.invalid_input` (a field fails validation; `errors[]` names it), `422 branding.contrast_failed` (the report, failing pairs first, in a `report` extension member), `403 entitlement.feature_unavailable` (the plan lacks `custom_branding`), `401`, `403`, `404` | branding permission and the `custom_branding` feature, organization scope |

Both are in the OpenAPI document with their schemas, including the report. The codes follow the
`area.reason` form. The web app calls them only through `@stepclimb/api-client`. `GET /theme.css`
is 11-app-shell's.

## Data

- `organizations.branding` (`jsonb NOT NULL DEFAULT '{}'`, in
  `migrations/20261112000001_organizations.sql`) holds the saved inputs, the generator version,
  the rendered CSS, its hash and the contrast report of its themes. An empty object means the
  default theme (B25). No new table.
- The JSON is versioned from its first release (`.claude/rules/architecture.md` -> Crate
  boundaries: data formats are versioned): a top-level `version` field.
- Logos and the favicon need file storage, which is M1 (Q6).

## Permissions

One catalog entry for editing branding, named in 09-permissions-and-roles' conventions, held by
the built-in Organization admin role at organization scope (`docs/plan/technical.md` ->
Permissions and roles: "Everything, including providers, branding and integrations"). No other
built-in role holds it. **Coordinator decision (2026-11-14).** `PUT /api/v1/admin/branding`
also takes `Require<feature::CustomBranding>` (`custom_branding`), which
`10-entitlements-framework.md` Q1 (decided by the owner, 13 Nov 2026) names for this spec;
`GET` takes none. Self-hosted runs the `unlimited` plan, so the feature is always on there, and
which managed plans include it is M6's to decide.

## Test cases

Generator and build (Vitest in `packages/tokens`):

- **T1.** (B1, B9) Every token of the plan's table exists in every theme in `variables.css` and
  has a Tailwind mapping.
- **T2.** (B2) Changing a learning state's alias target in the JSON changes that state's value in
  every theme and nothing else.
- **T3.** (B4) `[data-sc-density="compact"]` sets the two compact heights; `--sc-control-touch`
  is unchanged.
- **T4.** (B5, B8) The high-contrast selector exists for each brand; the light theme still wins
  over the OS dark setting when `data-sc-theme="light"`.
- **T5.** (B11) Generating twice from the same inputs gives byte-identical CSS.
- **T6.** (B13) Every emitted color is inside sRGB.
- **T7.** (B14, B20) Property test over generated brand colors (a seeded sweep of hue, chroma and
  lightness, including near-white yellow, near-black navy, pure red and grey): every generated
  theme's report passes in light, dark and high contrast. The seed is named in the test.
- **T8.** (B17, B18) A generated theme contains no space, type-size, weight or control-height
  tokens, and its status tokens equal the defaults.
- **T9.** (B20) A hand-built theme with `fg-soft` at 4.4:1 on `bg-canvas` fails the report on that
  pair only.
- **T10.** (B22) The build fails when a committed theme fails the report (proved once by breaking
  a value).
- **T11.** (B10) The docs tables list every token in `variables.css`, and no other.
- **T12.** (B24) The golden vectors pass against `generate.ts`.

Server (crate `tests/`, `#[sqlx::test]`):

- **T13.** (B24) The golden vectors pass against the server's generator.
- **T14.** (B21) `PUT /api/v1/admin/branding` with inputs whose theme fails answers 422
  `branding.contrast_failed` with the failing pairs, and the stored branding is unchanged.
- **T15.** (B24, B25) After a successful PUT, `/theme.css` serves the new CSS with a new hash;
  before any PUT it serves the default.
- **T16.** Four-case permission test on both endpoints: anonymous 401, signed in without the
  permission 403, wrong scope 404, Organization admin 2xx.
- **T17.** Cross-organization: an admin of organization A cannot read or write B's branding, and
  A's save never changes B's `/theme.css`.
- **T18.** With the organization's plan lacking `custom_branding`, `PUT` answers 403
  `entitlement.feature_unavailable` and changes nothing; `GET` still answers 200.
- **T19.** A change writes one `organization.update_branding` audit entry; a refused or unchanged
  `PUT` writes none.

## Out of scope

- The branding screen (live preview, contrast report display, BrandPreview): the component is in
  the plan's Authoring row (M1 to M2); when the screen ships is Q14.
- Logo and favicon upload: needs file storage (M1); Q6.
- `ThemeProvider` and the Storybook theme matrix: 13-base-ui-components.
- Serving `/theme.css` and linking it before the bundle: 11-app-shell.
- The Tauri app loading each linked server's `theme.css`: M5.

## Open questions

The owner accepted every recommendation below on 13 Nov 2026. Where a behavior or an issue
says "pending Q<n>" or "Q<n> answered", build the recommended option; the owner can still
overrule one, which then updates this spec.

- **Q1. Token source path.** The plan's layout has `packages/tokens/src/tokens/*.json`; the
  repository has `packages/tokens/tokens/*.json`, and `.claude/rules/ui.md` names the latter.
  Options: (a) move to `src/tokens/` to match the plan and update the rule, README and config;
  (b) keep `tokens/` and record the departure in an ADR. Recommendation: (a): the plan wins, and
  the move is one small commit before `src/generate.ts` lands beside it.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q2. Values for `info`, `info-soft` and `info-fg`.** The plan lists `info` among the status
  tokens and gives no values. Options: (a) a blue hue (around 250) at the lightness and chroma of
  the other statuses in each appearance; (b) `info` aliases the brand tokens; (c) a neutral
  (grey) info. Recommendation: (a): info must stay distinct from the brand, which marks the next
  action, and from the vZDC red; it must pass B20 in every theme.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q3. Learning-state alias targets.** The plan says each state has one shape, icon and color
  and color is never the only signal, but not which color. Options, recommended first:
  locked -> `fg-soft`, available -> `fg-sub`, progress -> `brand`, complete -> `success`,
  overdue -> `danger`, review -> `warning`; or the same with available -> `brand` and progress ->
  `info`. Recommendation: the first set: the brand marks the thing in progress, matching the
  rail fill (`docs/plan/ui.md` -> ProgressRail).
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q4. Values the plan names without giving.** `bg-overlay` (the modal scrim, which the plan
  allows to blur), `fg-disabled`, the floating-layer shadow, the reading size within 17 to 18 px
  and line height within 1.65 to 1.7, and the high-contrast neutrals. Recommendation: the
  generator derives `bg-overlay` and `fg-disabled` from the neutral scale; the owner picks the
  reading size and line height (18 px at 1.7 is inside both ranges) and approves the shadow on a
  Storybook screenshot.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q5. What selects high contrast.** Options: (a) only `data-sc-theme="high-contrast"`, chosen
  by the user; (b) also `@media (prefers-contrast: more)` when no theme is set; (c) also
  `forced-colors: active`, where the browser's system colors take over and the theme only keeps
  borders visible. Recommendation: (a) plus (b), and a `forced-colors` check in the base styles
  (13) rather than a theme.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q6. Logos and favicon in M0.** The vZDC week-2 checkpoint is "see the branded app";
  uploads arrive with files in M1. Options: (a) M0 stores small logo images (SVG or PNG with a
  size cap, SVG sanitized) inside the branding record; (b) M0 shows the organization's name as
  text, logos come with M1 files; (c) logos as URLs to an external host. Recommendation: (b),
  with the brand color and font already giving the checkpoint a branded app; (a) adds an upload
  path that M1 replaces.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q7. The optional accent color.** It is a branding input with no token in the plan's table.
  Options: (a) add `accent`, `accent-soft` and `accent-fg` tokens and say where components use
  them; (b) store the input and use it nowhere until a component needs it; (c) drop it until a
  milestone names a use. Recommendation: (c) for M0, so no token exists without a consumer.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q8. Corner style and fonts.** Neither the corner-style values nor the UI-font choices are
  listed. Options for corners: (a) a fixed set such as square, default, round that scales `xs`,
  `sm` and `md` (pill unchanged); (b) a free radius. For fonts: (a) a fixed list of bundled
  OFL fonts (Atkinson Hyperlegible Next, Roboto for vZDC, and a few others) served by the
  instance; (b) any Google Fonts family; (c) an uploaded font file. Recommendation: corners (a),
  fonts (a): no third-party font requests under the strict CSP, and every font in the list is
  checked once for legibility.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q9. The 12-step scale.** The plan fixes 12 steps in OKLCH but not their lightness and chroma
  curves, nor which step `brand` takes when several clear 4.5:1. Options: (a) fixed lightness
  stops per appearance, with chroma scaled from the input's and clamped to sRGB, and `brand` the
  passing step nearest the input's lightness; (b) stops fitted so the default brand reproduces
  the plan's values (`oklch(0.50 0.09 195)` light, `oklch(0.76 0.10 190)` dark,
  `oklch(0.38 0.12 195)` high contrast). Recommendation: (b), then (a)'s rule for choosing the
  step: the default brand then comes out of the generator unchanged.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q10. The high-contrast target.** WCAG 2.2 AA (the plan's floor) needs 4.5:1 and 3:1. Options:
  (a) 7:1 text and 4.5:1 UI (WCAG AAA text); (b) AA, with stronger lines and no tints.
  Recommendation: (a), which is what a separate high-contrast theme is for.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q11. Generated or hand-written default themes.** Options: (a) the build generates the default
  and vZDC themes from their brand colors and the hand-written JSON is deleted; (b) the
  hand-written JSON stays and a test checks the generator's output is within a small color
  difference of it. Recommendation: (a) once Q9 makes the default reproduce the plan's values;
  one path for every brand.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q12. How the TypeScript generator runs on the Rust server.** The plan says "TS" and "on the
  server", and the server is one Rust binary using under 100 MB idle. Options: (a) port the
  generator to Rust in `stepclimb-core` (or the server), with the TS version kept for build time
  and the browser preview, and the golden vectors of B24 holding them equal; (b) the server
  shells out to Node, which adds a runtime to the image and the bare-metal binary; (c) embed a
  JavaScript engine (for example QuickJS through a Rust binding) running the bundled
  `generate.js`; (d) write the generator in Rust and compile it to WASM for the build and the
  browser, departing from the plan's "TS". Recommendation: (a): no new runtime in the single
  binary, and two implementations of a small, pure function are cheap to keep equal with shared
  vectors. (d) needs an ADR as a departure from the plan.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q13. Custom CSS.** Options: (a) not in M0; (b) M0 stores an admin's CSS, appends it to
  `/theme.css` under `.sc-custom`, and labels it unsupported. Recommendation: (a): it needs
  sanitizing against the strict CSP and is not in the M0 row.
  **Decided:** the recommendation (owner, 13 Nov 2026).
- **Q14. When the branding screen ships.** The API is in the plan's Administration endpoints and
  the generator is M0; the screen needs a ColorField (Forms, M0 to M1) and BrandPreview
  (Authoring, M1 to M2). Options: (a) M0 ships the API only, configured by an admin through the
  API or `stepclimb.toml`, and the screen in M1; (b) a minimal screen in M0 (color field, font
  and appearance selects, the report as a table, no preview component). Recommendation: (b),
  so vZDC staff can brand the test server at the week-2 checkpoint without the API.
  **Decided:** the recommendation (owner, 13 Nov 2026).

### Coordinator decisions (2026-11-13)

Taken on the owner's delegation while building I3 to I5 (PR for #75, #76, #77); the owner can
overrule any of them.

- **Coordinator decision (2026-11-13): WCAG AA over the plan's example values.** The plan's
  principle, WCAG 2.2 AA in every theme, wins where its example values fail B20. Each value moves
  by the smallest lightness change that passes on 8-bit sRGB for every brand hue (the neutrals
  carry the brand's hue), keeping hue and chroma: light `warning` 0.72 to 0.663 (so
  `state-review` clears 3:1 on `bg-canvas` and `bg-raised`), light `line-strong` 0.78 to 0.655
  (3:1 on both; then 0.633 by the owner's decision below), light `fg-soft` 0.55 to 0.533 (4.5:1
  on `bg-sunken`, which also lifts the placeholder text of Select, ComboBox and TextField), and dark `line-strong` 0.42 to 0.507 (3:1
  on both, the same rule applied to the dark theme). `docs/plan/` is unchanged; the owner may
  update the plan's table.
- **Owner decision (13 Nov 2026): `line-strong` on `bg-sunken`.** B20 also requires
  `line-strong` on `bg-sunken` at the UI minimum (3:1, 4.5:1 in high contrast) in every theme.
  Light `line-strong` darkens again by the smallest change that passes on `bg-canvas`,
  `bg-raised` and `bg-sunken` for every brand hue: 0.655 to 0.633. Dark (0.507) and high contrast
  (0.25) already pass.
- **Coordinator decision (2026-11-13): OS contrast setting.** `prefers-contrast: more` selects
  high contrast (B5). `shadow.float` stays as it is in every theme; dark relies on the 1 px line.
- **Coordinator decision (2026-11-13): Q8's open values.** Corner style `square` sets every
  radius to 0 except `pill`; `round` doubles `xs`, `sm` and `md` (`pill` unchanged). A sans
  reading font is the organization's UI font. The M0 UI fonts are Atkinson Hyperlegible Next and
  Roboto only. The generator does not take corner style or reading font yet; they arrive with the
  branding inputs of I7 and I8.
- **Coordinator decision (2026-11-13): vZDC's brand and danger.** vZDC's brand red sits about 1
  degree of hue from `danger`, so in-progress and overdue look alike apart from their shapes and
  icons. Accepted for M0; the report's `brand-near-danger` warning (B20) flags it.
- **Coordinator decision (2026-11-13): dark and high-contrast brand steps (Q9).** Light keeps
  Q9's rule (the passing step nearest the input's lightness). Dark and high contrast aim at the
  plan's lightness for the appearance, 0.76 and 0.38, since there the canvas and the minimums
  fix how light the brand must be. The generated dark brand keeps the input's hue (195 for the
  default brand) where the plan writes 190, a difference of 0.009 in OKLab.
- **Coordinator decision (2026-11-13): no wide-gamut overrides.** The hand-written themes gave
  P3 and Rec. 2020 displays the plan's unmapped colors through `@media (color-gamut: ...)`. The
  generator writes sRGB colors only (B13), so those overrides are gone. A wide-gamut display now
  shows the sRGB value. The largest difference in the default brand is high contrast's `brand`
  and `ring` (`oklch(0.38 0.12 195)`, outside sRGB), 0.055 in OKLab; where a browser clipped the
  unmapped value into sRGB, as the story screenshots did, that is 0, 84, 86 before and 0, 79, 80
  after, at most 5 levels per channel.

### Coordinator decisions (2026-11-14)

Taken on the owner's delegation while building I7 and I8 (PR for #79, #80); the owner can
overrule any of them.

- **Coordinator decision (2026-11-14): the Rust port and its vectors (Q12).** The generator is
  ported to `stepclimb_core::theme`, operation for operation, including V8's `Math.hypot`,
  `Math.round` and `toFixed` rounding, so both give the same doubles. The golden vectors are
  `packages/tokens/test/golden/theme-vectors.json`, written by the TypeScript generator
  (`pnpm --filter @stepclimb/tokens golden`): 14 named inputs with their full stylesheet and every
  contrast ratio, 240 seeded inputs with the stylesheet's SHA-256 and the verdicts, and 15 bad
  inputs with the field each names. Vitest fails when the file is stale (T12); the Rust test
  fails on any other byte, a ratio more than 1e-9 apart, or another verdict (T13).
- **Coordinator decision (2026-11-14): corner style and reading font are generator inputs.**
  `BrandInputs` gains `readingFont` (`serif`, the default, or `sans`) and `cornerStyle`
  (`square`, `default`, `round`), and every theme carries `font.read` and `radius.xs|sm|md`
  (B18). The build's theme permutations include them, so a shipped brand's choice reaches
  `variables.css`; the default and vZDC brands keep the default values.
- **Coordinator decision (2026-11-14): chroma is capped at 0.5.** An `oklch()` input with a
  chroma above 0.5 (or a hue too large to be finite) is refused, in both generators, as
  `branding.invalid_color`. Without the cap, a chroma of 1e150 left no brand step that clears
  4.5:1 (the server answered 500) and 1e100 took 60 ms of gamut mapping on the request's thread;
  sRGB's most saturated colors are near 0.32 and P3's near 0.37. `primary` is stored trimmed and
  at most 64 characters (`branding.too_long`).
- **Coordinator decision (2026-11-14): the stylesheet.** `/theme.css` is one stylesheet per
  organization, `brandStylesheet()` in `src/stylesheet.ts`: the default brand's selectors of
  `variables.css` (light on `:root` and `[data-sc-theme="light"]`, dark and high contrast by
  attribute, then by OS setting), each rule with every color, the learning-state aliases, both
  fonts and the three radii. For the default brand it gives every element the values
  `variables.css` gives it (`test/stylesheet.test.ts`).
- **Coordinator decision (2026-11-14): the API's inputs and answer.** `PUT` replaces the whole
  set: `primary` is required, an omitted or `null` choice takes its default, and an unknown member
  is a 400. A value that is not a choice is a 422 `branding.invalid_input`, its `errors` entry
  coded `branding.invalid_color` or `branding.too_long` (`primary`) or `branding.not_a_choice`.
  `GET` for an organization without saved branding answers the default brand's inputs, hash and
  report. Both answer `inputs`, `generator`, `css_hash` (SHA-256, lowercase hex) and `report`,
  the light, dark and high-contrast reports. The stored record is `{"version": 1, "inputs",
  "generator", "css", "css_hash", "report"}`, written together at save time; `GET` answers it
  as stored and never generates, so the report always describes the stylesheet `/theme.css`
  serves. Saving the same inputs, generator version and stylesheet again writes nothing.
  Regenerating saved branding when `GENERATOR_VERSION` changes is a later decision.
- **Coordinator decision (2026-11-14): the contrast guard and the chroma cap.** The cap keeps
  every accepted input inside the range the generator's stops were fitted for. Within it, no
  input tried gives a failing theme: T7's sweep, a test at the cap for five hues, and the 254
  golden vectors. That is evidence, not proof, so the guard stays (B21) and so does the
  generator's no-step error (a 500). T14's 422 `branding.contrast_failed` is therefore not
  reached through the API today: it is tested on the guard with a doctored theme and on the 422
  body, and only `theme::render` can make the `Rendered` value `branding::save` takes, so a
  stylesheet that skipped the report cannot be stored. T14's invalid-input cases run through
  the API and check the stored branding is unchanged.
- **Coordinator decision (2026-11-14): `custom_branding` gates the change.** Spec 10 Q1 (decided
  by the owner after this spec's Permissions section) checks `custom_branding` on 12's routes.
  `PUT` takes `Require<CustomBranding>` before `Require<BrandingManage>`; `GET` takes only the
  permission, as reading roles needs no `custom_roles`. Self-hosted's unlimited plan has the
  feature.
- **Coordinator decision (2026-11-14): branding changes are audited.** A change writes
  `organization.update_branding` (target type `organization`, as `organization.update_settings`),
  in `branding::save`'s transaction after the update, by the signed-in user with the client
  address. `data` holds the inputs and `css_hash` before (`null` when none was saved) and after,
  never the stylesheet. A refused or unchanged save writes none (09 B22, which lists the row).
- **Coordinator decision (2026-11-14): the server half of `/theme.css` lands with I8.** 11 I2
  (#67) is not built, and T15 and T17 need the route. I8 adds `GET /theme.css` (public,
  organization from `RequestOrg`, `text/css; charset=utf-8`, `Cache-Control: no-cache`, in the
  OpenAPI document under `operations`). 11 I2 keeps the `<link>` in `index.html`, the fingerprint
  and its cache headers (11 Q4), the Vite proxy and the bundle without color variables (11 Q5).

## Issues

In dependency order. Each is one PR.

- **I1. Complete the token set.** Deliverable: `bg-overlay`, `fg-disabled`, `info*`, the
  learning-state aliases, the compact density selector and the remaining plan tokens in every
  theme, with their Tailwind mappings and tests. Covers B1 to B9; T1 to T4. Depends on: Q1,
  Q2, Q3 and Q4 answered.
- **I2. Generator core.** Deliverable: `src/generate.ts` with color parsing, OKLCH conversion,
  sRGB gamut mapping, the WCAG contrast ratio, the 12-step brand and neutral scales and the brand
  step choice for light, with unit and property tests. Covers B11 to B15, B18, B19; T5, T6,
  T8. Depends on: I1; Q9 answered.
- **I3. Contrast report and build guard.** Deliverable: `contrastReport()`, the build failing on
  a committed theme with a failing pair. Covers B20, B22; T9, T10. Depends on: I2.
- **I4. Dark and high-contrast derivation.** Deliverable: dark and high-contrast output from the
  generator, the `[data-sc-theme="high-contrast"]` theme for the default and vZDC brands, the
  property sweep across all three appearances. Covers B5, B16, B17; T7. Depends on: I3; Q5 and
  Q10 answered.
- **I5. Generated default themes in the build.** Deliverable: the default and vZDC themes come
  from the generator (or are checked against it), and `variables.css` keeps its selectors and
  order. Covers B8, B23. Depends on: I4; Q11 answered.
- **I6. Docs tables.** Deliverable: the generated tables in `build/` and the data the Storybook
  Tokens page reads. Covers B10; T11. Depends on: I1. (13 I1 renders the page.)
- **I7. Server-side generator.** Deliverable: the generator running for the server as Q12
  decides, the shared golden vectors, CI running them against both. Covers B24 (generation);
  T12, T13. Depends on: I4; Q12 answered.
- **I8. Branding API and per-organization theme.** Deliverable: `GET` and `PUT
  /api/v1/admin/branding` with validation, the guardrail, the stored CSS, hash and report in
  `organizations.branding`, `/theme.css` serving it, the permission entry, the `custom_branding`
  feature check on `PUT` (10 Q1), the `organization.update_branding` audit entry, OpenAPI and
  client regenerated. Covers B21, B24, B25; T14 to T19. Depends on: I7; 11 I2 (`/theme.css`);
  09 (the permission and `Require`); 10 (`Require<Feature>`); 02 (organization scope).
  Maintainer review: line by line (permissions, entitlements, audit).
