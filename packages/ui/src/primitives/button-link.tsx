// SPDX-License-Identifier: AGPL-3.0-or-later
// ButtonLink: React Aria's Link drawn as a Button, for navigation that reads as an action, such as
// a sign-in provider's button (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B14).
// It keeps a link's behavior: an `href` the browser shows and can open in a new tab, and
// navigation through the app's RouterProvider, which `routerOptions` reach. The label wraps, since
// it can come from configuration, so the control grows in height instead of overflowing. Under a
// coarse pointer it is at least the touch size, as base.css makes every button.
import {
  Link as AriaLink,
  type LinkProps as AriaLinkProps,
  composeRenderProps,
} from "react-aria-components";
import { tv, type VariantProps } from "../lib/tv";
import { button } from "./button";

export const buttonLink = tv({
  extend: button,
  base: [
    "h-auto min-h-(--sc-control-h) py-2 whitespace-normal text-center cursor-pointer",
    "data-[disabled]:cursor-not-allowed",
    // base.css gives buttons the touch size under a coarse pointer; a link needs it here.
    "pointer-coarse:min-h-(--sc-control-touch) pointer-coarse:min-w-(--sc-control-touch)",
  ],
  variants: {
    size: { sm: "h-auto min-h-(--sc-control-h-sm)", md: "", lg: "h-auto min-h-12" },
  },
});

export interface ButtonLinkProps extends AriaLinkProps, VariantProps<typeof buttonLink> {}

export function ButtonLink({ intent, size, className, ...props }: ButtonLinkProps) {
  return (
    <AriaLink
      data-slot="button-link"
      {...props}
      className={composeRenderProps(className, (cls) =>
        buttonLink({ intent, size, className: cls }),
      )}
    />
  );
}
