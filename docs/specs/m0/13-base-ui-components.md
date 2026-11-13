# Base UI components

- **Status:** Draft
- **Milestone:** M0
- **Plan:** `docs/plan/ui.md` -> Library architecture, Component inventory (the M0 rows), Component
  conventions, Definition of done for a component, Pitfalls we refuse, What we borrow, Principles;
  `docs/plan/technical.md` -> Architecture and stack (Web, Biome), License and governance
  (`packages/ui` stays in the repo), Testing (Web)
- **Depends on:** 01-repo-and-ci (the CI gates), 11-app-shell I1 (`useStepclimbStrings()`),
  12-tokens-and-theme-generator (the complete token set, the high-contrast theme, generated
  extreme brands)
- **Review:** standard

## Summary

Stepclimb UI's M0 components in `packages/ui` (`@stepclimb/ui`): the Foundations, Actions,
Overlays and Navigation rows of the plan's inventory and the M0 part of Forms and Data, each built
on React Aria Components to the plan's conventions and definition of done. It also sets up what
holds every later component to the same bar: the lint checks the plan requires, Storybook in every
theme at 1280 and 375 px, axe, Playwright screenshots of every story, component status,
Changesets and `packages/ui/THIRD_PARTY.md`.

## What exists today

Read on 2026-11-12.

- `packages/ui/src/primitives/button.tsx`: the plan's convention example, with one change for
  Tailwind 4 (`h-(--sc-control-h)` for the plan's `h-[--sc-control-h]`, recorded in ADR 0013).
  Intents primary, secondary (default), ghost, danger; sizes sm, md, lg; `data-slot="button"`;
  `composeRenderProps` so `className` adds to the recipe. No loading state.
- `button.test.tsx`: seven Vitest tests (native button with its slot, secondary by default, the
  recipes, press by click, Enter and Space, `data-focus-visible`, disabled, `className` keeps the
  recipe). `button.stories.tsx`: one story per intent, Small, Disabled and a Themes story with
  default and vZDC in light and dark side by side.
- `src/index.ts` exports `Button`, `ButtonProps` and `button`. `src/styles/index.css` imports
  Tailwind, the two token files and `base.css`; `base.css` themes `html` (UI font, canvas, text
  color, brand caret, tabular numerals), `::selection`, and sets `color-scheme: dark` only under
  `[data-sc-theme="dark"]`, not under the OS dark setting.
- Dependencies: `react-aria-components` 1.22.1, `tailwind-variants` 4.0.0, `tailwind-merge`
  3.7.0. No Phosphor, no axe, no Changesets, no `THIRD_PARTY.md`.
- `apps/ui-docs/.storybook/`: Storybook 10.6.5 (`@storybook/react-vite`) reading
  `packages/ui/src/**/*.stories.tsx`, with the Tailwind Vite plugin, the fontsource fonts, and a
  decorator setting `data-sc-brand` (default, vZDC) and `data-sc-theme` (light, dark) from two
  toolbar globals. No high contrast, density, viewport or status.
- Biome 2.6.4 (`biome.json`) with the recommended preset; `just web-lint` runs
  `biome check --error-on-warnings .`, so a warning fails the gate.
- In the pinned React Aria Components 1.22.1 (checked in `node_modules`): `Toolbar`,
  `ToggleButtonGroup`, `Group`, `Keyboard`, `VisuallyHidden`, `Sheet`, `Autocomplete`, `Tree`,
  `GridList`, `Table`, `Meter`, `ProgressBar` and `Breadcrumbs` are stable exports; Toast is
  exported only as `UNSTABLE_Toast`, `UNSTABLE_ToastRegion` and `UNSTABLE_ToastQueue`.

## Behavior

### Conventions every component follows

From `docs/plan/ui.md` -> Component conventions and `.claude/rules/ui.md`.

- **B1.** One file per component in `src/primitives/`, `src/patterns/` or `src/theme/` as the
  plan's layout places it (Button, Field, TextField, Select, Dialog, Menu, Tabs, Table, Toast in
  primitives; AppShell, SideNav, BottomTabs, CommandPalette, EmptyState, DataTable in patterns;
  ThemeProvider and icons in theme). Each file exports its parts and its `tv()` recipe, and
  `src/index.ts` re-exports them.
- **B2.** Every rendered part carries `data-slot="<component>-<part>"` (the root uses the bare
  component name, as `button` does). Styling reads React Aria's `data-*` state attributes
  directly. No `asChild`.
- **B3.** React Aria Components is the only primitive layer; no other headless library is
  mixed in. A component the library does not provide (Avatar, EmptyState, Pagination) is built
  from React Aria parts and plain elements.
- **B4.** `className` is for layout only; color and type changes go through variants (B20).
- **B5.** No hard-coded strings: every label, including accessible names that are not visible
  (a close button's label, a copy confirmation), comes from `useStepclimbStrings()`
  (11-app-shell B23). Components never hard-code nouns.
- **B6.** Logical properties only (`ms-`, `pe-`, `start-`), so right-to-left works.
- **B7.** Every stateful component works controlled and uncontrolled (`value` and `defaultValue`,
  `isOpen` and `defaultOpen`, following React Aria's names).
- **B8.** Semantic tokens only. Tailwind's palette does not exist (12-tokens-and-theme-generator);
  a raw color fails the lint (B18).
- **B9.** Touch targets are at least 44 px on coarse pointers (`--sc-control-touch`), and text
  inputs are at least 16 px on touch so iOS does not zoom on focus.
- **B10.** Icons are Phosphor (`@phosphor-icons/react`) only, through `Icon`. Weights encode
  state where state is shown: regular = available, fill = complete
  (`docs/plan/ui.md` -> Library architecture). Decorative icons are `aria-hidden`; an icon that is
  the only content of a control requires an accessible label.
- **B11.** Motion uses CSS transitions and the Web Animations API with the motion tokens; no
  motion library. With `prefers-reduced-motion: reduce`, transitions are instant. No section
  entrances, no shimmer for loads under 300 ms.
- **B12.** Primary is opt-in (`intent="primary"`); secondary is the default, so a view has one
  primary action unless a developer asks for more.

### M0 inventory

Each row's components meet B1 to B12 and the definition of done (B21 to B28). "React Aria basis"
names the React Aria Components export each is built on.

| Group | Component | React Aria basis | M0 behavior beyond the conventions |
| --- | --- | --- | --- |
| Foundations | ThemeProvider | none (context) | Sets `data-sc-theme`, `data-sc-density` and, in Storybook, `data-sc-brand` on its element; exposes the current appearance; follows `prefers-color-scheme` when no theme is set (B13) |
| | Icon | none | Wraps a Phosphor icon with token sizes and the state weights of B10 |
| | Text, Heading | `Text`, `Heading` | Type scale variants only; Heading's level is required, separate from its size |
| | Link | `Link` | Navigates through the app's `RouterProvider` (11-app-shell B1); external links say so to screen readers |
| | Kbd | `Keyboard` | Mono, for shortcuts |
| | VisuallyHidden | `VisuallyHidden` | |
| | Separator | `Separator` | Uses `line` or `line-soft` |
| | Prose | none | Styles server-sanitized HTML: the reading face, a 62 to 68 character measure, reading size and line height (12 Q4), tables that scroll on their own, mono only for code |
| Actions | Button | `Button` | Adds the loading state (`isPending`), with the label kept for screen readers |
| | IconButton | `Button` | Requires an accessible label; shows it as a Tooltip |
| | ButtonGroup | `Group` | |
| | ToggleButton | `ToggleButton`, `ToggleButtonGroup` | |
| | Menu | `MenuTrigger`, `Menu`, `MenuItem`, `SubmenuTrigger`, `Popover` | Opens as a bottom Sheet under 768 px if Q10 says so |
| | Toolbar | `Toolbar` | Arrow-key movement between items |
| | CopyButton | `Button` | Copies with the Clipboard API and announces the result through a live region; on failure says the copy failed |
| Forms (M0 part, Q1) | Field | `Label`, `Text slot="description"`, `FieldError` | The shared label, description and error layout every field uses |
| | TextField, TextArea | `TextField`, `Input`, `TextArea` | Error text says what is wrong and how to fix it |
| | NumberField | `NumberField` | Tabular numerals; locale formatting from React Aria |
| | SearchField | `SearchField` | Clear button with a label from strings |
| | Select | `Select`, `SelectValue`, `ListBox`, `Popover` | |
| | ComboBox | `ComboBox`, `ListBox`, `Popover` | Async loading state |
| | Checkbox, CheckboxGroup | `Checkbox`, `CheckboxGroup` | Indeterminate state |
| | RadioGroup | `RadioGroup`, `Radio` | |
| | Switch | `Switch` | |
| Overlays | Dialog | `DialogTrigger`, `ModalOverlay`, `Modal`, `Dialog` | For confirmations and short inputs only, never for work (`docs/plan/ui.md` -> Pitfalls: modals for work); scrim is `bg-overlay`, the one place blur is allowed |
| | AlertDialog | `Dialog` with `role="alertdialog"` | Names the action on its confirm button; destructive confirms use the danger intent |
| | Sheet (side and bottom) | `Sheet` (stable in 1.22.1) or `ModalOverlay` and `Modal` | Side sheet for editing on wide screens, bottom sheet on phones; respects safe areas |
| | Popover | `Popover` | One soft shadow (floating layer) |
| | Tooltip | `TooltipTrigger`, `Tooltip` | Never the only place information lives |
| | Toast | `UNSTABLE_ToastRegion`, `UNSTABLE_ToastQueue` (Q7) | Queue API for the app; error toasts stay until dismissed |
| | CommandPalette | `Autocomplete`, `Menu`, `ModalOverlay` | The component and its keyboard model; the app wires its commands from M1 |
| Navigation | AppShell | none (landmarks) | `header`, `nav` and `main` landmarks, skip link, the SideNav and BottomTabs breakpoints of 11-app-shell B3 |
| | SideNav | `Link` in a `nav` | `aria-current="page"` on the active entry; the organization's logo slot; the collapsed variant if 11 Q1 asks for it |
| | BottomTabs | `Link` in a `nav` | Navigation links, not ARIA tabs; fixed inside the bottom safe area; no rubber-band bounce |
| | TopBar | none (`header`) | Title and an inline-end slot for actions and the user menu; sticky on mobile, where blur is allowed |
| | Breadcrumbs | `Breadcrumbs`, `Breadcrumb` | |
| | Tabs | `Tabs`, `TabList`, `Tab`, `TabPanel` | |
| | Pagination | `Button` in a `nav` | Cursor-based Previous and Next (Q9) |
| | OrgSwitcher | `Select` or `MenuTrigger` | Rendered by the app only in multi-organization mode (11-app-shell B19) |
| Data (M0 part, Q1) | DataTable | `Table`, `TableHeader`, `Column`, `TableBody`, `Row`, `Cell` | Sorting, selection, a loading row, an empty state, cursor pagination; compact density for dense views |
| | DataList | `GridList` | |
| | Avatar | none | Image with an initials fallback from the display name; pill radius |
| | Meter, ProgressBar | `Meter`, `ProgressBar` | Value label through strings |
| | Skeleton | none | Appears only after 300 ms; no shimmer |
| | EmptyState | none | One sentence on why it is empty and at most one primary action |
| | ErrorState | none | What happened and how to fix it; a retry action slot |

### Base styles (`src/styles/base.css`)

- **B13.** `color-scheme` follows the active theme in every case: dark under
  `[data-sc-theme="dark"]` and under the OS dark setting when no theme is set. The skeleton's
  `base.css` already does this; T6 is the test still to write.
- **B14.** Selection, caret, scrollbars, focus rings and tabular numerals are themed from tokens
  (`docs/plan/ui.md` -> Pitfalls: browser defaults left in). The focus ring is `ring`, 2 px with a
  `bg-canvas` offset, shown on `:focus-visible` only.
- **B15.** Fixed and sticky bars use `env(safe-area-inset-*)`; the document does not rubber-band
  under fixed chrome (`overscroll-behavior`); controls are not text-selectable.
- **B16.** Under `prefers-reduced-motion: reduce` every transition and animation is instant.
- **B17.** Under `forced-colors: active`, focus rings, control borders and selected states stay
  visible (see 12 Q5).

### Lint checks

The plan requires three, in `docs/plan/ui.md` -> Pitfalls we refuse and Component conventions.
How each is enforced is Open question Q2.

- **B18.** **Raw colors fail.** A hex color, `rgb()`, `rgba()`, `hsl()`, `oklch()` or other color
  function, a named CSS color, or a Tailwind arbitrary color (`bg-[#fff]`,
  `text-[oklch(...)]`) in any TS, TSX or CSS file outside `packages/tokens` is an error. A class
  from Tailwind's removed palette (`bg-blue-500`), which would otherwise generate nothing
  silently, is also an error.
- **B19.** **Emoji and other icon sets are flagged.** An emoji in JSX text or a string literal in
  `packages/ui` or `web/src` is flagged, and so is an import from any icon package other than
  `@phosphor-icons/react` (for example `lucide-react`, `react-icons`, `@heroicons/react`,
  `@radix-ui/react-icons`, `@tabler/icons-react`).
- **B20.** **`className` color and type changes warn.** A color, background, border-color,
  font-size or font-weight utility passed in `className` to a Stepclimb UI component warns
  (`docs/plan/ui.md`: "the lint warns otherwise"). Whether that warning fails the gate is Open
  question Q3, because `just web-lint` runs with `--error-on-warnings`.

### Definition of done

From `docs/plan/ui.md` -> Definition of done for a component.

- **B21.** **States.** Each component has a story for default, hover, pressed, focus-visible,
  disabled, loading, error, empty and long content, for the states it has.
- **B22.** **Themes.** Storybook renders every story in light, dark and high contrast, in the
  default brand, the vZDC example and two extreme brands (Q5), in comfortable and compact
  density. The toolbar picks each; a matrix story per component shows them side by side.
- **B23.** **Widths.** Storybook has 1280 and 375 px viewports; every story is checked at both.
- **B24.** **Accessibility.** axe reports zero violations for every story in light, dark and high
  contrast; contrast is at least 4.5:1 for text and 3:1 for UI. The keyboard-only walkthrough and
  VoiceOver names are checked by hand and recorded in the component's status (Q8).
- **B25.** **Touch.** 44 px targets (B9) are checked in the 375 px stories. The iOS Safari and
  Tauri iOS web-view check is by hand (Q8).
- **B26.** **Tests.** Vitest and Testing Library behavior tests per component (keyboard, press,
  controlled and uncontrolled, accessible names), and a Playwright screenshot of every story.
- **B27.** **Docs.** A Storybook page per component with usage, do and don't examples and props
  generated from the TypeScript types.
- **B28.** **Status.** Each component is `experimental`, `stable` or `deprecated`, and Storybook
  shows it on the component's page and in the sidebar. An experimental component lists which
  definition-of-done rows it does not meet yet (`.claude/rules/ui.md`). Every M0 component starts
  experimental; one moves to stable when every row is met, in a PR that says so.

### Visual and accessibility tests

- **B29.** A Playwright suite runs against the static Storybook build: it lists every story from
  Storybook's index, and for each one in light, dark and high contrast at 1280 and 375 px takes a
  screenshot compared against a committed baseline and runs axe, failing on any difference beyond
  a set threshold or any violation. It runs in `just ci`. The matrix size and where baselines live
  are Open question Q4.

### Lint enforcement (where)

- **B30.** The checks of B18 to B20 run in `just web-lint`, part of `just ci`. If Q2 puts them in
  Biome, they also run in `.githooks/pre-commit`, which already runs `biome check` on staged web
  files; a script would be added to that hook beside it. A finding names the file, the line, the
  rule and the fix ("use a token utility such as `bg-brand`").

### Versions and notices

- **B31.** Changesets manages versions of `@stepclimb/ui` and `@stepclimb/tokens` inside the
  repository: both stay `"private": true` and are never published (ADR 0015). A PR that changes
  either package's public surface adds a changeset; whether CI requires one is Open question Q6.
  A breaking change to a stable component ships with a codemod (none is stable in M0).
- **B32.** `packages/ui/THIRD_PARTY.md` lists every piece of adapted MIT code (shadcn/ui, Kibo UI,
  AlignUI, Magic UI, Libraries.dev, InstUI, Immich UI per `docs/plan/ui.md` -> What we borrow)
  with its source, its license notice and the file it went into. It exists from the first PR of
  this spec, empty of entries if nothing is adapted yet, and every PR that adapts code adds its
  entry.

## API

None. The components call no endpoint; the app passes data in.

## Data

None.

## Permissions

None. Components hide nothing on permission; the app decides what to render (11-app-shell B5).

## Test cases

- **T1.** (B2) A test per component checks each part's `data-slot`.
- **T2.** (B5) A test renders every component with a strings provider whose messages are all
  `«id»` and finds no visible text or accessible name that is not wrapped that way.
- **T3.** (B7) Each stateful component's test drives it controlled and uncontrolled.
- **T4.** (B9) At 375 px with a coarse pointer emulated, every interactive story's targets
  measure at least 44 by 44 px (Playwright).
- **T5.** (B11, B16) With reduced motion emulated, no story has a running transition after an
  interaction.
- **T6.** (B13) With the OS dark setting emulated and no theme attribute, `color-scheme` is dark.
- **T7.** (B18) The lint fails on each of: `#fff` in a TSX string, `rgb(0 0 0)` in CSS,
  `bg-[#000]`, `text-blue-500`, `oklch(0.5 0.1 200)` in `packages/ui`; it passes on the same
  `oklch()` in `packages/tokens`. Each case is a fixture the lint test runs.
- **T8.** (B19) The lint flags an emoji in JSX and an import from `lucide-react`; it accepts
  `@phosphor-icons/react`.
- **T9.** (B20) `className="text-danger-fg"` on `Button` produces the warning;
  `className="ms-auto"` does not.
- **T10.** (B29) The Playwright story suite fails when a story's rendering changes (proved once by
  changing a token) and when a story has an axe violation (proved once with an unlabeled
  IconButton).
- **T11.** (B28) A component without a status fails a Storybook index check.
- **T12.** Per component, the Vitest behavior tests the inventory table implies: Dialog traps and
  returns focus and closes on Escape; AlertDialog's confirm names its action; Menu, Toolbar,
  Tabs and DataTable move with arrow keys; CopyButton announces success and failure; Skeleton
  does not render before 300 ms; BottomTabs and SideNav mark the active entry with
  `aria-current`; Avatar falls back to initials when the image fails; Pagination calls back with
  the next and previous cursors.

## Out of scope

- The rest of Forms (TagField, Slider, DatePicker, DateRangePicker, TimeField, ColorField,
  FileDrop) and Data (Tree, StatusChip) as Q1 settles, and every later group (Learning,
  Authoring, Assessment, Activities, Reporting, Managed hosting, App shell): their milestones'
  specs.
- Wiring the components into routes, sign-in and `/me`: 11-app-shell.
- Token values and the theme generator: 12-tokens-and-theme-generator.
- Commands for the CommandPalette: from M1, as features add them.

## Open questions

- **Q1. The M0 part of Forms and Data.** The plan marks both rows "M0–M1" without splitting them.
  Options: (a) M0 builds what the M0 screens use: Field, TextField, TextArea, NumberField,
  SearchField, Select, ComboBox, Checkbox(Group), RadioGroup, Switch, and DataTable, DataList,
  Avatar, Meter, ProgressBar, Skeleton, EmptyState, ErrorState; M1 builds TagField, Slider,
  DatePicker, DateRangePicker, TimeField, ColorField, FileDrop, Tree and StatusChip; (b) the whole
  of both rows in M0. Recommendation: (a), adding ColorField to M0 if 12 Q14 puts a branding
  screen in M0. StatusChip needs the learning-state tokens (12 I1) and is used first by M1's
  module rows.
- **Q2. How the lint checks are enforced.** Options: (a) Biome 2 GritQL plugins for raw colors,
  emoji and `className` utilities, with Biome's `noRestrictedImports` for icon packages;
  (b) `noRestrictedImports` for icons plus a small script in `bin/` (like
  `bin/check-spdx.sh`) scanning TS, TSX and CSS for the color and emoji patterns; (c) add ESLint
  and Stylelint with custom rules. Recommendation: (b) first, because the patterns are regular,
  a script is easy to test with fixtures and needs no new tool, and the plan names Biome as the
  linter; move to (a) if a check needs the syntax tree (B20 does: it must know a `className` is
  on a Stepclimb UI component). Also open: whether the same check enforces logical properties
  (`ml-`, `pl-`, `left-`), which `.claude/rules/ui.md` requires but the plan lists no lint for.
  Recommendation: yes, as an error.
- **Q3. A warning in a gate that fails on warnings.** The plan says `className` color changes
  "warn"; `just web-lint` passes `--error-on-warnings`. Options: (a) the check reports at
  warning level and the gate fails on it like any warning, with a suppression comment for a
  justified case; (b) the check runs outside the gate and only reports; (c) it is an error.
  Recommendation: (a): a warning that never stops a merge is ignored, and the suppression comment
  keeps the plan's "warn" meaning that exceptions exist.
- **Q4. Screenshot matrix and baselines.** The plan's table says every story is a visual test "in
  all three themes at 1280 and 375 px"; the definition of done also lists brands and densities.
  Options: (a) every story in 3 themes at 2 widths (6 screenshots), plus each component's matrix
  story covering brands and densities; (b) every story in every theme, brand and density at both
  widths (36). Baselines: (a) committed PNGs under `apps/ui-docs/`, rendered in Playwright's
  pinned Docker image so fonts match across machines; (b) Git LFS; (c) not committed, compared
  against the target branch's build in CI. Recommendation: matrix (a), baselines (a);
  Playwright's own image keeps the comparison stable between a laptop and CI.
- **Q5. The two extreme brands.** Options: (a) a very light, saturated yellow (around `#FFE600`)
  and a very dark navy (around `#0A1A3F`); (b) a saturated magenta and a grey with almost no
  chroma. Recommendation: (a): they push the brand step choice to both ends of the lightness
  range, which is where contrast fails first.
- **Q6. Requiring a changeset.** Options: (a) CI fails a PR that changes `packages/ui` or
  `packages/tokens` without a changeset (`changeset status`); (b) changesets are added by hand at
  release time. Recommendation: (a), so the changelog is written when the change is fresh.
- **Q7. Toast on an unstable React Aria export.** In 1.22.1 the toast parts are `UNSTABLE_`
  exports. Options: (a) build Toast on them, wrapped so only `toast.tsx` imports them, and record
  the pin in an ADR; (b) build our own region with an ARIA live region and React Aria's focus
  utilities; (c) defer Toast until the export is stable. Recommendation: (a): React Aria's
  version handles focus and landmark navigation, and the wrapper contains a later API change to
  one file.
- **Q8. The hand-checked rows.** The keyboard walkthrough, VoiceOver names and the iOS Safari and
  Tauri iOS web-view check cannot be automated, and the Tauri iOS build first exists in the M1
  iPhone test. Options: (a) the maintainer records each hand check in the component's status,
  and every component stays experimental on the iOS row until the M1 iPhone test; (b) an iOS
  simulator Safari run stands in for the device in M0. Recommendation: (a): the plan's iPhone test
  is the place a real device enters, and the status says what is missing.
- **Q9. Pagination with cursors.** The plan's list endpoints use cursor pagination, which has no
  page numbers. Options: (a) Previous and Next only, with a count where the API gives one;
  (b) a "Load more" pattern; (c) numbered pages for client-side lists and Previous and Next for
  server lists. Recommendation: (a) for M0, with (c)'s numbered variant added when a client-side
  list needs it.

- **Q10. Menus and Selects on phones.** The plan says touch-first and puts the ProgressRail in a
  bottom sheet on mobile, but says nothing about menus. Options: (a) Menu, Select and ComboBox
  open as a bottom Sheet under 768 px; (b) they stay popovers at every width.
  Recommendation: (a), at the same breakpoint as BottomTabs (11-app-shell B3).

## Issues

In dependency order. Each is one PR.

- **I1. ThemeProvider and the Storybook matrix.** Deliverable: `ThemeProvider` in `src/theme/`,
  the Storybook toolbar for theme (light, dark, high contrast), brand (default, vZDC, the two
  extreme brands once 12 I4 generates them), density and the 1280 and 375 px viewports;
  component status shown in Storybook; the Tokens page reading 12's docs tables. Covers B22,
  B23, B28; T11. Depends on: 12 I1, 12 I6.
- **I2. Story screenshots and axe in CI.** Deliverable: the Playwright suite over the static
  Storybook build, baselines for the existing Button stories, the recipe added to `just ci`.
  Covers B24, B29; T10. Depends on: I1; Q4 answered.
- **I3. Lint checks.** Deliverable: the raw-color, emoji, icon-import and `className` checks with
  their fixtures, run by `just web-lint`. Covers B18 to B20, B30; T7 to T9. Depends on: Q2 and Q3
  answered.
- **I4. Changesets and third-party notices.** Deliverable: Changesets configured for the two
  private packages, `packages/ui/THIRD_PARTY.md`, and the CI check if Q6 says so. Covers B31,
  B32. Depends on: Q6 answered.
- **I5. Base styles.** Deliverable: `base.css` completed: `color-scheme` under the OS setting,
  scrollbars, focus rings, safe areas, overscroll, unselectable controls, reduced motion,
  forced colors, touch target and input sizes. Covers B9, B13 to B17; T4 to T6. Depends on: I2.
- **I6. Foundations.** Deliverable: Icon (adds `@phosphor-icons/react`), Text, Heading, Link,
  Kbd, VisuallyHidden, Separator, Prose, with stories, tests and docs pages. Covers B1 to B12 for
  these, B21, B26, B27; T1 to T3, T12 (Link). Depends on: I1, I3, I5; 11 I1 (strings).
- **I7. Actions.** Deliverable: Button's loading state, IconButton, ButtonGroup, ToggleButton,
  Menu, Toolbar, CopyButton, Tooltip (needed by IconButton), Popover. Covers the Actions row and
  Tooltip and Popover of Overlays; T12 (Menu, Toolbar, CopyButton). Depends on: I6.
- **I8. Shell navigation.** Deliverable: AppShell, SideNav, BottomTabs, TopBar, OrgSwitcher.
  Covers those Navigation rows; T12 (SideNav, BottomTabs). Depends on: I7; 11 Q1 answered.
- **I9. Data states.** Deliverable: Avatar, Skeleton, EmptyState, ErrorState, Meter,
  ProgressBar. Covers those Data rows; T12 (Skeleton, Avatar). Depends on: I6.
- **I10. Text fields.** Deliverable: Field, TextField, TextArea, NumberField, SearchField.
  Covers those Forms rows. Depends on: I6; Q1 answered.
- **I11. Choice fields.** Deliverable: Select, ComboBox, Checkbox and CheckboxGroup, RadioGroup,
  Switch. Covers those Forms rows. Depends on: I10, I7 (Popover).
- **I12. Dialogs and sheets.** Deliverable: Dialog, AlertDialog, Sheet (side and bottom).
  Covers those Overlays rows; T12 (Dialog, AlertDialog). Depends on: I7.
- **I13. Toast and CommandPalette.** Deliverable: Toast with its queue, CommandPalette with its
  keyboard model. Covers those Overlays rows. Depends on: I12; Q7 answered.
- **I14. Page navigation.** Deliverable: Breadcrumbs, Tabs, Pagination. Covers those Navigation
  rows; T12 (Tabs, Pagination). Depends on: I7; Q9 answered.
- **I15. Tables and lists.** Deliverable: DataTable and DataList, with the M0 Data components Q1
  adds. Covers those Data rows; T12 (DataTable). Depends on: I9, I11 (selection checkboxes),
  I14 (Pagination).
