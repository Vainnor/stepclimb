# 0024. Toast on React Aria's unstable toast exports

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decider:** the owner, accepting the recommendation of `docs/specs/m0/13-base-ui-components.md`
  Q7 on 2026-11-13
- **Related:** ADR 0013; `docs/specs/m0/13-base-ui-components.md` Q7, M0 inventory (Toast);
  `packages/ui/src/primitives/toast.tsx`

## Context

Stepclimb UI needs a Toast in M0: spec 11 B12 shows an error toast when sign-out fails. React Aria
Components is the only primitive layer (ADR 0013). In the pinned `react-aria-components` 1.22.1,
the toast parts are exported only as `UNSTABLE_Toast`, `UNSTABLE_ToastRegion`,
`UNSTABLE_ToastList`, `UNSTABLE_ToastContent` and `UNSTABLE_ToastQueue` (read in
`node_modules/react-aria-components/dist/types/exports/Toast.d.ts`). An `UNSTABLE_` export can
change shape or name in a minor release.

What those parts do, read in React Aria 3.53.1's `useToast` and `useToastRegion` and React
Stately 3.51.0's `ToastQueue`: each toast's content is a `role="alert"` live region and focus does
not move to it; the region is a landmark with `role="region"`, reachable with F6; hovering the
region or focusing inside it pauses every toast's timer and leaving resumes them; when the focused
toast closes, focus moves to the next toast or back to where it was before the region.

## Options

### Option A: build on the unstable exports, wrapped in one file

`toast.tsx` imports them and exports Stepclimb's own API (`createToastQueue()`, `ToastRegion`,
`ToastMessage`); no other file imports them. A later rename or change of shape touches one file.

### Option B: our own region

An ARIA live region, a landmark and React Aria's focus utilities, written here. It repeats the
focus handling and timer pausing React Aria already has, and adds code to review.

### Option C: defer Toast

Spec 11's sign-out error would have no toast in M0.

## Recommendation

Option A, as spec 13 Q7 recommends.

## Decision

Option A. Toast is built on React Aria's `UNSTABLE_` toast exports, imported only in
`packages/ui/src/primitives/toast.tsx`; the rest of Stepclimb uses `createToastQueue()` and
`ToastRegion`.

## Consequences

`toast.test.tsx` fails if any other file of `packages/ui` mentions `UNSTABLE_Toast`. Every update
of `react-aria-components` reruns the Toast tests; when React Aria makes the toast exports stable,
`toast.tsx` switches to the stable names and this record is superseded. The queue's public API
does not expose React Aria's `ToastQueue`, so that switch changes no caller.
