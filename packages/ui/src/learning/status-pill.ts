// SPDX-License-Identifier: AGPL-3.0-or-later
// The pill a course's or a module's status is drawn in, in words ("Draft", "Unpublished"):
// CourseHeader's status and ModuleBoard's publish state. Not a learning state, which StatusChip
// draws with its icon (docs/specs/m1/16-learning-ui-components.md B3).
import { tv } from "../lib/tv";

export const statusPill = tv({
  base: [
    "inline-flex items-center rounded-pill border border-line-strong bg-raised px-2.5 py-0.5",
    "text-caption font-control text-fg-default",
  ],
});
