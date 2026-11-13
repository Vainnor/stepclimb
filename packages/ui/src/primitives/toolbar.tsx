// SPDX-License-Identifier: AGPL-3.0-or-later
// Toolbar: a row (or column) of controls that is one tab stop; arrow keys move between its items
// (docs/specs/m0/13-base-ui-components.md M0 inventory).
import {
  Toolbar as AriaToolbar,
  type ToolbarProps as AriaToolbarProps,
  composeRenderProps,
} from "react-aria-components";
import { tv } from "../lib/tv";

export const toolbar = tv({
  base: "flex flex-wrap items-center gap-1 data-[orientation=vertical]:flex-col data-[orientation=vertical]:items-stretch",
});

export type ToolbarProps = AriaToolbarProps;

/** Name it with `aria-label`. A Separator inside it takes `orientation="vertical"` in a row. */
export function Toolbar({ className, ...props }: ToolbarProps) {
  return (
    <AriaToolbar
      data-slot="toolbar"
      {...props}
      className={composeRenderProps(className, (cls) => toolbar({ className: cls }))}
    />
  );
}
