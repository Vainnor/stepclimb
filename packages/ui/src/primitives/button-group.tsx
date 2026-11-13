// SPDX-License-Identifier: AGPL-3.0-or-later
// ButtonGroup: related buttons side by side, as one `group` for assistive technology
// (docs/specs/m0/13-base-ui-components.md M0 inventory). Arrow keys do not move between them; a
// set of controls that should move with arrow keys is a Toolbar.
import {
  Group as AriaGroup,
  type GroupProps as AriaGroupProps,
  composeRenderProps,
} from "react-aria-components";
import { tv } from "../lib/tv";

export const buttonGroup = tv({ base: "inline-flex flex-wrap items-center gap-2" });

export type ButtonGroupProps = AriaGroupProps;

/** Name the group with `aria-label` when its buttons need the context. */
export function ButtonGroup({ className, ...props }: ButtonGroupProps) {
  return (
    <AriaGroup
      data-slot="button-group"
      {...props}
      className={composeRenderProps(className, (cls) => buttonGroup({ className: cls }))}
    />
  );
}
