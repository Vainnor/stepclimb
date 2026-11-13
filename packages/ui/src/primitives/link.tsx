// SPDX-License-Identifier: AGPL-3.0-or-later
// Link: React Aria's Link, which navigates through the app's RouterProvider (spec 11 B1) for an
// in-app `href`. A link to another site says so to screen readers
// (docs/specs/m0/13-base-ui-components.md M0 inventory).
import {
  Link as AriaLink,
  type LinkProps as AriaLinkProps,
  composeRenderProps,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { VisuallyHidden } from "./visually-hidden";

export const link = tv({
  base: [
    "rounded-xs text-brand-fg underline underline-offset-2 decoration-1 outline-none",
    "transition-colors duration-fast ease-out-expo cursor-pointer",
    "data-[hovered]:decoration-2",
    "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    "data-[disabled]:text-fg-disabled data-[disabled]:no-underline data-[disabled]:cursor-not-allowed",
  ],
});

/**
 * True when `href` leaves the app: an absolute `http:` or `https:` URL, or a protocol-relative
 * one, on another origin than `origin`. A path, a fragment, `mailto:` and `tel:` stay.
 */
export function isExternalHref(href: string | undefined, origin: string): boolean {
  if (href === undefined || !/^(?:https?:)?\/\//i.test(href)) return false;
  return new URL(href, origin).origin !== origin;
}

export type LinkProps = AriaLinkProps;

export function Link({ className, children, ...props }: LinkProps) {
  const { t } = useStepclimbStrings();
  const external = isExternalHref(props.href, window.location.origin);
  return (
    <AriaLink
      data-slot="link"
      {...props}
      className={composeRenderProps(className, (cls) => link({ className: cls }))}
    >
      {composeRenderProps(children, (content) => (
        <>
          {content}
          {external ? (
            // The space sits inside the hidden span, so the visible underline ends with the text.
            <VisuallyHidden data-slot="link-external">{` ${t("link.external")}`}</VisuallyHidden>
          ) : null}
        </>
      ))}
    </AriaLink>
  );
}
