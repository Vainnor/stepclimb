// SPDX-License-Identifier: AGPL-3.0-or-later
// Prose: long-form content in the reading face, at a 62 to 68 character measure and the reading
// size and line height (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 12 Q4). The
// styles of the elements inside (headings, lists, code, tables that scroll on their own) are in
// styles/base.css, under `[data-slot="prose"]`, because the content is HTML the server sanitized
// and carries no classes.
import { type ReactNode, useEffect, useRef } from "react";
import { tv } from "../lib/tv";

export const prose = tv({
  base: "max-w-[66ch] font-read text-read leading-read text-fg-default break-words",
});

export interface ProseProps {
  /** The content: elements rendered from the server's sanitized HTML. */
  children: ReactNode;
  /** Layout only. */
  className?: string;
}

/** The elements base.css lets scroll sideways on their own. */
const SCROLLERS = "table, pre";

/**
 * Makes every table or code block that is wider than the column a tab stop, so a keyboard user
 * can scroll it (WCAG 2.1.1; axe's scrollable-region-focusable), and takes the tab stop back when
 * it fits again.
 */
export function markScrollers(root: HTMLElement): void {
  for (const el of root.querySelectorAll<HTMLElement>(SCROLLERS)) {
    if (el.scrollWidth > el.clientWidth) el.tabIndex = 0;
    else el.removeAttribute("tabindex");
  }
}

export function Prose({ children, className }: ProseProps) {
  const ref = useRef<HTMLDivElement>(null);
  // Runs after every render, since the content may change; a resize can make a table overflow.
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    markScrollers(root);
    const observer = new ResizeObserver(() => markScrollers(root));
    observer.observe(root);
    return () => observer.disconnect();
  });
  return (
    <div ref={ref} data-slot="prose" className={prose({ className })}>
      {children}
    </div>
  );
}
