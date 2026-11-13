// SPDX-License-Identifier: AGPL-3.0-or-later
// The shell's two breakpoints (spec 11 B3 and Q1), as media queries for components that change
// shape in JavaScript rather than in CSS. They match Tailwind's `md` (48rem) and `lg` (64rem) at
// the default 16 px root size.

/** At this width and wider: side navigation, popovers and side sheets. Under it: bottom tabs and bottom sheets. */
export const WIDE_QUERY = "(min-width: 768px)";

/** At this width and wider the SideNav shows its labels; from 768 px to here it is collapsed to icons. */
export const SIDE_NAV_EXPANDED_QUERY = "(min-width: 1024px)";
