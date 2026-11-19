# UI

The UI tab of the Stepclimb plan (final plan v1.0, 12 Nov 2026), as Markdown. Source: [the plan artifact](https://claude.ai/artifact/ABMCETKdv4QtXiWhdrJ8VH). Converted for the repository with American spelling (`docs/adr/0020-american-english.md`); the wording is otherwise the plan's. Changes to this file are the owner's (`.claude/rules/research-and-decisions.md`).

Stepclimb UI is our own component library inside the repo, the way Canvas has InstUI and Immich has `@immich/ui`. It's used by the web app and, through Tauri, by every native app.

- **Own the code**: `packages/ui` in the monorepo. Components are copied in and owned, the way shadcn works.
- **React Aria Components underneath**: Accessible behavior for date pickers, drag-and-drop, trees, tables and color pickers.
- **Tailwind 4 + tailwind-variants**: Styling only through semantic tokens. Raw colors fail the lint.
- **One token source**: Design-token JSON generates the CSS variables, the Tailwind theme and the docs.
- **Brand belongs to the org**: A brand color, logo and font go in. Accessible light, dark and high-contrast themes come out.
- **Calm by default**: Reading and exams come first, with one authored motion moment.

## Principles

- **Reading first.** Long-form text uses a reading face at a 62–68 character measure, and the app chrome recedes while reading or in an exam.
- **State is always visible.** Locked, available, in progress, complete, overdue and needs-review each have one shape, icon and color everywhere. Color is never the only signal.
- **One primary action per view.** The brand color marks the next thing to do.
- **The org's brand, our structure.** Orgs change color, logo, font and words, but not spacing, hierarchy or contrast.
- **Accessible is the floor:** WCAG 2.2 AA in every theme, a high-contrast theme, full keyboard support, 44px touch targets, reduced motion respected, logical CSS properties for right-to-left languages.
- **Touch-first.** The same components run in a phone browser and in the iOS app's web view, so layouts, keyboard handling and safe areas are designed for phones from the start.
- **Built, not assembled.** Selection, caret, scrollbars, focus rings and tabular numerals are themed.

## Live preview

The plan artifact shows sample components in the default and vZDC themes here, as an interactive preview. It is not reproduced in Markdown; open the artifact to see it.

## What we borrow

| Source | We take | We don't use |
| --- | --- | --- |
| [shadcn/ui](https://ui.shadcn.com/) | The ownership model, `cn()` plus variant recipes, `data-slot` styling hooks, composable parts | Its default look: zinc greys, black buttons, one radius, Inter/Geist |
| [Kibo UI](https://www.kibo-ui.com/) | Patterns for the module board, file dropzone, editor wrapper and calendar | Its Radix-based code. We rebuild on React Aria. |
| [AlignUI](https://alignui.com) | Token naming by role and strength, product density, Figma–code name parity | Pro components and its SaaS look |
| [Magic UI](https://magicui.design) | The technique for one moment: the rail fill and course-completion reveal | Marquees, shimmer, bento grids, animated gradients |
| [Libraries.dev](https://libraries.dev/) | Theme resolution order: `data-theme`, then `prefers-color-scheme` | The WebGL effects (beams, orbs, metal) |
| [InstUI](https://github.com/instructure/instructure-ui) | A first-class high-contrast theme, codemods for breaking changes, live docs | One package per component |
| [Immich UI](https://github.com/immich-app/ui) | Tailwind 4 + headless primitives + tailwind-variants + one icon set + in-repo docs | Svelte, the separate repo |

All of these are MIT. Adapted code keeps its notice in `packages/ui/THIRD_PARTY.md`.

## Library architecture

```text
packages/
  tokens/            W3C design-token JSON + theme generator (TS, no DOM)
    src/tokens/*.json      space, radius, type, motion, elevation
    src/generate.ts        brand color → light / dark / high-contrast palettes (OKLCH)
    build/                 → css/variables.css, css/tailwind-theme.css, docs tables
  ui/                @stepclimb/ui (React 19, React Aria Components, Tailwind 4)
    src/primitives/        Button, Field, TextField, Select, Dialog, Menu, Tabs, Table, Toast…
    src/patterns/          AppShell, SideNav, BottomTabs, CommandPalette, EmptyState, DataTable, FileDrop…
    src/learning/          ProgressRail, ModuleRow, NextUp, PageReader, StatusChip, Certificate…
    src/assessment/        QuizShell, QuestionNavigator, Timer, SaveState, question renderers…
    src/activities/        ActivityShell, StepFeedback, drill games, FlightPlanEditor, CommandLine, SessionReplay
    src/editor/            Tiptap wrapper, toolbar, slash menu, embeds
    src/theme/             ThemeProvider (org theme, light/dark/high-contrast, density), icons
    src/styles/            base.css (browser surfaces, prose, safe areas)
apps/
  app/               Tauri shell: loads web/ and adds server picker, downloads, notifications
  ui-docs/           Storybook: every component, every state, all themes, both densities
```

| Concern | Choice | Why |
| --- | --- | --- |
| Primitives | React Aria Components | Covers the widgets an LMS needs (dates with time zones, drag-to-reorder, trees, grids, number and color fields) with tested accessibility. One primitive layer, never mixed. |
| Styling | Tailwind 4, `tailwind-variants`, `tailwind-merge` | One recipe per component. React Aria's `data-*` states are styled directly. |
| Tokens | DTCG JSON → Terrazzo or Style Dictionary | One source for CSS variables, the Tailwind theme and the docs. Figma can import the same file. |
| Icons | Phosphor (`@phosphor-icons/react`) | One set, with weights that encode state (regular = available, fill = complete). |
| Type | Atkinson Hyperlegible Next (UI), Source Serif 4 (reading), Atkinson Hyperlegible Mono (data) | Atkinson was designed for legibility. The serif is only for long-form pages and orgs can switch it to sans. |
| Rich text | Tiptap, wrapped in `src/editor` | Matches the stored ProseMirror JSON, with our own toolbar and blocks. |
| Charts | Hand-built SVG components | Only four are needed: histogram, question stats, distractor bars, heat map. |
| Motion | CSS transitions and the Web Animations API | No motion library. |
| Docs and tests | Storybook, Vitest, Playwright screenshots, axe | Every story is a visual test in all three themes at 1280 and 375 px wide. |

## Tokens

| Group | Tokens | Use |
| --- | --- | --- |
| Surfaces | `bg-canvas` `bg-raised` `bg-sunken` `bg-overlay` | Page, panels and popovers, wells and inputs, modal scrim |
| Text | `fg-strong` `fg-default` `fg-sub` `fg-soft` `fg-disabled` | Headings, body, secondary, placeholder and locked (≥ 4.5:1), disabled |
| Lines | `line-strong` `line` `line-soft` | Input borders, dividers, row separators |
| Brand | `brand` `brand-hover` `brand-soft` `brand-fg` `on-brand` `ring` | Primary action and progress, tints, text on tint, text on brand, focus ring |
| Status | `success\|warning\|danger\|info` with `-soft` and `-fg` | Feedback, grades and callouts only |
| Learning | `state-locked\|available\|progress\|complete\|overdue\|review` | Aliases, so learning states can be retuned in one place |

| Scale | Values |
| --- | --- |
| Type (px) | 12 · 13 · 14 · 16 · 18 · 20 · 24 · 30 · 38. Reading text 17–18 px at 1.65–1.7 line height. |
| Weight | 400 body · 500 controls · 700 headings |
| Space (px) | 2 · 4 · 6 · 8 · 12 · 16 · 20 · 24 · 32 · 40 · 56 · 72 |
| Radius (px) | xs 3 · sm 5 (controls) · md 8 (panels) · pill for chips and avatars only |
| Control height | comfortable 40 / 32 · compact 32 / 26 · 44 minimum on touch |
| Elevation | Panels use a 1px line, no shadow. One soft shadow for floating layers. |
| Motion (ms) | fast 120 · base 180 · slow 320, with an exponential ease-out |

```css
/* generated: packages/tokens/build/css/variables.css */
:root, [data-sc-theme="light"] { --sc-brand: oklch(0.50 0.09 195); --sc-bg-canvas: oklch(0.985 0.004 195); … }
[data-sc-theme="dark"]          { --sc-brand: oklch(0.76 0.10 190); --sc-bg-canvas: oklch(0.17 0.008 195); … }
[data-sc-theme="high-contrast"] { --sc-brand: oklch(0.38 0.12 195); --sc-line: oklch(0.35 0 0); … }
[data-sc-density="compact"]     { --sc-control-h: 2rem; --sc-control-h-sm: 1.625rem; }

/* generated: packages/tokens/build/css/tailwind-theme.css */
@theme inline {
  --color-*: initial;                       /* Tailwind's default palette removed: bg-blue-500 doesn't exist */
  --color-canvas: var(--sc-bg-canvas);  --color-raised: var(--sc-bg-raised);  --color-sunken: var(--sc-bg-sunken);
  --color-fg-strong: var(--sc-fg-strong);  --color-brand: var(--sc-brand);  --color-on-brand: var(--sc-on-brand); …
  --radius-sm: 5px;  --radius-md: 8px;
  --font-ui: var(--sc-font-ui);  --font-read: var(--sc-font-read);  --font-mono: var(--sc-font-mono);
}
```

## Organization theming

- **Inputs**: Primary color, optional accent, logos (light, dark, compact), favicon, UI font, reading font (serif or sans), default appearance, corner style.
- **Generator**: `generate.ts` converts the brand color to OKLCH, builds a 12-step scale, tints the neutrals with the brand hue at low chroma, and picks the brand step that clears 4.5:1 against `on-brand` in each theme. It then derives dark and high-contrast versions. It runs at build time for defaults and on the server when an admin saves branding.
- **Guardrails**: The branding screen shows a live preview and a contrast report, and palettes that fail AA can't be saved. Custom CSS is an advanced setting, scoped under `.sc-custom` and marked unsupported across upgrades.
- **Delivery**: The server renders `/theme.css` (cached and fingerprinted), loaded before the app bundle so the default theme never flashes. The Tauri app loads the same file from each linked server.
- **Words**: Components never hard-code nouns. Labels come through i18n with the org's terminology overrides.

## Component inventory

| Group | Components | Milestone |
| --- | --- | --- |
| Foundations | ThemeProvider, Icon, Text, Heading, Link, Kbd, VisuallyHidden, Separator, Prose | M0 |
| Actions | Button, IconButton, ButtonGroup, ToggleButton, Menu, Toolbar, CopyButton | M0 |
| Forms | Field, TextField, TextArea, NumberField, SearchField, Select, ComboBox, TagField, Checkbox(Group), RadioGroup, Switch, Slider, DatePicker, DateRangePicker, TimeField, ColorField, FileDrop | M0–M1 |
| Overlays | Dialog, AlertDialog, Sheet (side and bottom), Popover, Tooltip, Toast, CommandPalette | M0 |
| Navigation | AppShell, SideNav, BottomTabs, TopBar, Breadcrumbs, Tabs, Pagination, OrgSwitcher | M0 |
| Data | DataTable, DataList, Tree, Avatar, StatusChip, Meter, ProgressBar, Skeleton, EmptyState, ErrorState | M0–M1 |
| Learning | ProgressRail, ModuleRow, ItemRow, NextUp, CourseHeader, PageReader, Callout, Announcement, RequirementLabel, Certificate, DueBadge, AssignmentList, ComplianceTable, DownloadButton (offline) | M1 |
| Authoring | Editor, SlashMenu, BlockToolbar, ModuleBoard, QuestionEditor, PoolBuilder, VersionDiff, BrandPreview | M1–M2 |
| Assessment | QuizShell, QuestionNavigator, Timer, SaveState, FlagToggle, renderers for all 10 question types, ReviewFeedback, GradingPane, RubricGrid | M2 |
| Activities | ActivityShell, StepFeedback, StreakMeter, MatchingGame, ClozeGame, OrderingGame, FlightPlanEditor, HeadingPicker, SprintHeader, ReviewQueue, RuleForm, CommandLine (M5), ScenarioPlayer and ScenarioGraphEditor (M5), TrafficPicture (M5), FlagProblem, VideoPlayer (HLS and embeds, watch tracking, M1), VideoUploader (M1) | M3 |
| Reporting | ScoreHistogram, QuestionStats, DistractorBars, RosterHeatmap, ActivityReport, MistakeBreakdown, SessionReplay, AuditEntry | M2–M3 |
| Managed hosting | PlanBadge, UpgradePrompt, UsageMeter, BillingSettings, SenderDomainSetup, OperatorConsole views | M6 |
| App shell | ServerPicker, ServerSwitcher, DownloadsManager, UpdatePrompt (desktop), NotificationSettings | M5 |

## Signature components

### ProgressRail

- **What it is**: A module's items drawn as a route: a line with one node per item, where completed segments fill with the brand color. It's the product's recurring motif.
- **States**: Complete (filled with a check), current (ring with a dot), available (outline), locked (lock icon, plus what unlocks it), overdue (danger ring plus due label).
- **Behavior**: An ordered list of links with arrow-key navigation. Locked items stay focusable and explain their requirement. On mobile it opens as a bottom sheet.
- **Motion**: On completion, the segment fills over 320 ms. When a course completes, the whole rail fills once and the certificate appears. This is the only celebratory animation, and it's instant with reduced motion.

### QuizShell

- **Focus mode**: The app shell hides. Only the title, timer, save state, navigator and an exit with confirmation remain.
- **Timer**: Tabular numbers. It turns warning at 5 minutes and danger at 1, and screen readers announce 5, 1 and 0. The server deadline is the truth.
- **Save state**: Always visible: "Saving…", "Saved", or "Offline, 2 answers waiting" with a retry button.
- **Navigator**: Squares show answered, current, flagged and unanswered. Before submitting, a review screen lists unanswered and flagged questions.

### ActivityShell and FlightPlanEditor

- **Feedback**: What appears follows the feedback setting. With Full, a result appears inline beside the field after each step: a check or cross, the rule broken in one sentence, and a "why" link to the lesson page. Result only shows just the check or cross. None shows the "Saved" indicator. It's never a modal and never covers the work. Correct answers get a quiet tick, not a celebration.
- **Mode label**: The header always says which mode is on ("Practice: answers explained", "Test: results after submitting"), so students know what to expect before they start.
- **Rhythm**: The next problem is one key away (Enter). Problem count, streak and accuracy sit in a slim header. An optional round timer uses the QuizShell timer.
- **Flight plan editor**: A dark, dense, monospace panel in the style of a scope's flight plan editor, drawn with our own tokens (it's the one place the UI goes dark in a light theme). It's fully keyboard-driven, with Tab between fields. On phones it becomes a stacked form with the same fields.
- **Command line**: A mono input with command history (up arrow) and syntax hints after a wrong attempt. A hardware keyboard is recommended, but it works with the iOS keyboard.
- **Flag**: "This looks wrong" is always available in the header and sends the exact problem.

### PageReader

- **Layout**: A centred reading column of about 66 characters, with the rail on the right on wide screens. "Mark as done" sits after the last paragraph, followed by Previous/Next.
- **Blocks**: Callouts use an icon and a tinted surface. Phraseology blocks use mono with labeled controller and pilot lines. Tables scroll on their own. Images can be zoomed.

## Key layouts

```text
COURSE HOME (≥1024px)
┌────────────┬─────────────────────────────────────────────────────────────┐
│ [org logo] │  S1 Ground Theory                        search  bell  avatar│
│ Dashboard  ├─────────────────────────────────────────────────────────────┤
│ Courses  ▸ │  ┌ Next up ───────────────────────────────────────────────┐ │
│ Calendar   │  │ Module 2 · Taxi instructions   ▓▓▓▓▓░░░░░   [Continue →]│ │
│ Grades     │  └────────────────────────────────────────────────────────┘ │
│            │  ● Clearance delivery ───────────────────────── Complete    │
│            │  ◉ Ground control ───────────────────────────── 2 / 4       │
│ ← vZDC     │  ○ Local control    Unlocks after Ground control            │
└────────────┴─────────────────────────────────────────────────────────────┘

QUIZ (focus mode)
┌──────────────────────────────────────────────────────────────────────────┐
│ ✕ Exit   Ground control check                14:32 left       Saved      │
│                Question 4 of 10 · 2 points             Flag              │
│                Complete the departure clearance readback.                │
│ [1][2•][3][(4)][5][6][7][8][9][10]                  Previous   [ Next ]  │
└──────────────────────────────────────────────────────────────────────────┘

GRADING QUEUE (compact density, keys j/k, g = grade)
┌──────────────────────────────┬───────────────────────────────────────────┐
│ Needs review (7)       ▾ All │ Student A · Ground control check · 17/20  │
│ ▸ Student A   Q7 essay  18:04│ Q7  Explain how you'd sequence…           │
│   Student D   Q3 short  17:40│ Score [ 2 ] / 3   Feedback [          ]   │
│                              │                     [Save & next  ⏎]      │
└──────────────────────────────┴───────────────────────────────────────────┘
```

Under 768px wide, the side nav becomes bottom tabs (Home, Courses, Notifications, Profile), and the rail and grading list become full-screen views with back navigation. The same layouts run in the Tauri app.

## Full-page mockups

The plan artifact draws six screens at desktop and phone width, in the default and vZDC brands,
light and dark: course home, lesson page, timed quiz in focus mode, grading queue, the flight plan
editor activity, and phone course home and quiz. They are interactive HTML and are not reproduced
in Markdown; open the artifact's UI tab to see them.

## Pitfalls we refuse

- **The default shadcn look**: Tinted neutrals, the org's brand as primary, radius by role. *Check:* Tailwind's default palette is removed.
- **Raw colors in components**: *Check:* lint fails on hex, `rgb()`, `oklch()` and arbitrary colors outside `packages/tokens`.
- **Gradient text, glows, beams, orbs**: Emphasis comes from weight, size and position.
- **Glass as decoration**: Blur only on the modal scrim and sticky mobile bars.
- **Card grids as structure**: No rows of identical icon cards and no nested cards. Lists, tables and the rail carry structure.
- **Hero-metric dashboards**: The dashboard leads with Next up and deadlines. Statistics live in reports.
- **Eyebrow labels and fake numbering**: Numbers only where order is real: modules and questions.
- **Coloured side stripes**: Callouts use an icon and a tinted surface.
- **Emoji or mixed icon sets**: Phosphor only. *Check:* lint flags emoji in JSX and other icon imports.
- **Sparkle and "AI" decoration**: Only for features that actually use AI, and labeled plainly.
- **Animation everywhere**: No section entrances, no shimmer for loads under 300 ms, no confetti. One moment: the rail fill.
- **Modals for work**: Editing and grading use full pages or sheets. Dialogs are for confirmations and short inputs.
- **Monospace as costume**: Only for times, codes, IDs, scores and phraseology.
- **Browser defaults left in**: Selection, caret, scrollbars, focus and numerals are themed in `base.css`.
- **Vague copy**: Buttons name their action. Errors say what happened and how to fix it.
- **Web-view tells in the apps**: No rubber-band bounce on fixed chrome, no zoom on input focus (16px+ inputs), safe areas respected, no text selection on controls.

## Component conventions

```tsx
// packages/ui/src/primitives/button.tsx
import { Button as AriaButton, type ButtonProps as AriaButtonProps, composeRenderProps } from "react-aria-components"
import { tv, type VariantProps } from "tailwind-variants"

export const button = tv({
  base: [
    "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-sm font-medium",
    "h-[--sc-control-h] px-4 text-sm transition-colors duration-fast ease-out-expo outline-none",
    "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    "data-[disabled]:opacity-50 data-[disabled]:cursor-not-allowed",
  ],
  variants: {
    intent: {
      primary:   "bg-brand text-on-brand data-[hovered]:bg-brand-hover",
      secondary: "bg-raised text-fg-strong border border-line-strong data-[hovered]:bg-sunken",
      ghost:     "text-fg-default data-[hovered]:bg-sunken",
      danger:    "bg-raised text-danger-fg border border-line-strong data-[hovered]:bg-danger-soft",
    },
    size: { sm: "h-[--sc-control-h-sm] px-3 text-[13px]", md: "", lg: "h-12 px-5 text-base" },
  },
  defaultVariants: { intent: "secondary", size: "md" },   // primary is opt-in: one per view
})

export interface ButtonProps extends AriaButtonProps, VariantProps<typeof button> {}

export function Button({ intent, size, className, ...props }: ButtonProps) {
  return (
    <AriaButton data-slot="button" {...props}
      className={composeRenderProps(className, (cls) => button({ intent, size, className: cls }))} />
  )
}
```

- One file per component, exporting its parts and its `tv()` recipe.
- `data-slot` on every part, with React Aria's state attributes for styling. No `asChild`. Links go through React Aria's `RouterProvider` wired to TanStack Router.
- `className` is for layout only. Color and type changes go through variants, and the lint warns otherwise.
- No hard-coded strings: labels come from `useStepclimbStrings()`.
- Logical properties only (`ms-`, `pe-`, `start-`).
- Every component works controlled or uncontrolled.

## Definition of done for a component

| Check | Requirement |
| --- | --- |
| States | Default, hover, pressed, focus-visible, disabled, loading, error, empty, long content |
| Themes | Light, dark, high contrast, the default brand and two extreme brands, in both densities |
| Accessibility | Zero axe violations, keyboard-only walkthrough, VoiceOver names checked, contrast ≥ 4.5:1 text and ≥ 3:1 UI |
| Responsive and touch | 1280 and 375 px stories, 44px targets, checked in iOS Safari and the Tauri iOS web view (keyboard, safe areas) |
| Tests | Vitest behavior tests and Playwright screenshots of every story |
| Docs | A Storybook page with usage, do and don't examples, and generated props |

Components move from experimental to stable to deprecated, and Storybook shows the status. Versions are managed with Changesets. Breaking changes to stable components ship with a codemod.

## UI decisions

- **Default brand color:** the deep sea-green shown on this page.
- **Reading face default:** serif (Source Serif 4). Organizations can switch to sans.
- **Distribution:** the library lives only in the repo. It isn't published to npm or a shadcn registry.
