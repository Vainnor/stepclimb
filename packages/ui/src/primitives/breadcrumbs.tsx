// SPDX-License-Identifier: AGPL-3.0-or-later
// Breadcrumbs: the path from the top of a section to the current page
// (docs/specs/m0/13-base-ui-components.md M0 inventory). A `nav` landmark named from the strings
// catalog holds React Aria's Breadcrumbs list; the last crumb is the current page, marked with
// `aria-current="page"` and not a link. Links navigate through the app's RouterProvider.
import { CaretRightIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  Breadcrumb as AriaBreadcrumb,
  Breadcrumbs as AriaBreadcrumbs,
  type BreadcrumbsProps as AriaBreadcrumbsProps,
  Link as AriaLink,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const breadcrumbs = tv({
  slots: {
    root: "font-ui text-sm",
    list: "flex flex-wrap items-center gap-x-1 gap-y-0.5",
    item: "flex min-w-0 items-center gap-1",
    link: [
      "inline-flex items-center rounded-xs text-fg-sub underline-offset-2 outline-none",
      "pointer-coarse:min-h-(--sc-control-touch) pointer-coarse:min-w-(--sc-control-touch)",
      "data-[hovered]:text-fg-strong data-[hovered]:underline",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
      "data-[current]:font-medium data-[current]:text-fg-strong data-[current]:no-underline",
      "data-[disabled]:cursor-default",
    ],
    separator: "shrink-0 text-fg-soft rtl:-scale-x-100",
  },
});

const styles = breadcrumbs();

export interface BreadcrumbsProps
  extends Omit<AriaBreadcrumbsProps<object>, "className" | "children" | "items"> {
  /** The crumbs, `Breadcrumb`s from the top down; the last is the current page. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

export function Breadcrumbs({ children, className, ...props }: BreadcrumbsProps) {
  const { t } = useStepclimbStrings();
  return (
    <nav
      data-slot="breadcrumbs"
      aria-label={t("breadcrumbs.label")}
      className={styles.root({ className })}
    >
      {/* React Aria names the list too, with its own translation unless given one. */}
      <AriaBreadcrumbs
        data-slot="breadcrumbs-list"
        aria-label={t("breadcrumbs.label")}
        {...props}
        className={styles.list()}
      >
        {children}
      </AriaBreadcrumbs>
    </nav>
  );
}

export interface BreadcrumbProps {
  /** Where the crumb goes. The last crumb, the current page, is not a link. */
  href?: string;
  /** The page's name. */
  children: ReactNode;
  /** Unique among the crumbs; needed when two have the same text. */
  id?: string;
}

/** One crumb: a link, then a caret unless it is the current page. */
export function Breadcrumb({ href, children, id }: BreadcrumbProps) {
  return (
    <AriaBreadcrumb data-slot="breadcrumb" id={id} className={styles.item()}>
      {({ isCurrent }) => (
        <>
          <AriaLink data-slot="breadcrumb-link" href={href} className={styles.link()}>
            {children}
          </AriaLink>
          {isCurrent ? null : (
            <Icon icon={CaretRightIcon} size="sm" className={styles.separator()} />
          )}
        </>
      )}
    </AriaBreadcrumb>
  );
}
