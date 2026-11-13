// SPDX-License-Identifier: AGPL-3.0-or-later
// VisuallyHidden: content for screen readers only, React Aria's (docs/specs/m0/13-base-ui-components.md
// M0 inventory). It draws nothing, so it has no recipe.
import {
  VisuallyHidden as AriaVisuallyHidden,
  type VisuallyHiddenProps as AriaVisuallyHiddenProps,
} from "react-aria-components";

export interface VisuallyHiddenProps extends AriaVisuallyHiddenProps {
  /** A `data-slot` for a part that is visually hidden; `visually-hidden` by default. */
  "data-slot"?: string;
}

/** A `div` unless `elementType` names another element; use `span` inside text. */
export function VisuallyHidden({ "data-slot": slot, ...props }: VisuallyHiddenProps) {
  return <AriaVisuallyHidden data-slot={slot ?? "visually-hidden"} {...props} />;
}
