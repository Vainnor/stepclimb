// SPDX-License-Identifier: AGPL-3.0-or-later
// Prose: long-form content in the reading face, at a 62 to 68 character measure and the reading
// size and line height (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 12 Q4). The
// styles of the elements inside (headings, lists, code, tables that scroll on their own) are in
// styles/base.css, under `[data-slot="prose"]`, because the content is HTML the server sanitized
// and carries no classes. That HTML can be given as `html`, inserted as it is: the server is the
// one that sanitizes it, so only HTML from the server's renderer goes there.
import { type ReactNode, useEffect, useRef } from "react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";

/**
 * The measure is in `em` of the reading size, not `ch`: `ch` is the width of "0", wider than the
 * average letter, so a measure of `66ch` holds about 75 characters. 31.5em holds 62 to 68 characters of the
 * reading face at the reading size (apps/ui-docs/visual/base.spec.ts counts them).
 */
export const prose = tv({
  base: "max-w-[31.5em] font-read text-read leading-read text-fg-default break-words",
});

export type ProseProps = {
  /** Layout only. */
  className?: string;
} & (
  | {
      /** The content as elements. */
      children: ReactNode;
      html?: never;
    }
  | {
      /** The content as the server's sanitized HTML, inserted as given (spec 07 B14, B22). */
      html: string;
      children?: never;
    }
);

/** The elements base.css lets scroll sideways on their own. */
const SCROLLERS = "table, pre";
/**
 * Set on a scroller Prose made a tab stop, listing the attributes Prose added
 * (`data-sc-scroller="tabindex role aria-label"`), so exactly those are taken back.
 */
const MARK = "data-sc-scroller";

/** What a scroller needs to be a named tab stop: a code block also gets a group role and name. */
function scrollerAttributes(el: HTMLElement, codeLabel: string): [string, string][] {
  const attributes: [string, string][] = [["tabindex", "0"]];
  if (el.tagName === "PRE") attributes.push(["role", "group"], ["aria-label", codeLabel]);
  return attributes;
}

/**
 * Makes every table or code block that is wider than the column a tab stop, so a keyboard user
 * can scroll it (WCAG 2.1.1; axe's scrollable-region-focusable), and takes the tab stop back when
 * it fits again. A code block also gets `codeLabel` as its name, as a group, so the tab stop is
 * announced as something; a table is announced by its own role and caption. An attribute the
 * content already set is left as it is, and never taken back.
 */
export function markScrollers(root: HTMLElement, codeLabel: string): void {
  for (const el of root.querySelectorAll<HTMLElement>(SCROLLERS)) {
    const marked = el.getAttribute(MARK);
    if (el.scrollWidth > el.clientWidth) {
      if (marked !== null) continue;
      const added = scrollerAttributes(el, codeLabel).filter(([name]) => !el.hasAttribute(name));
      for (const [name, value] of added) el.setAttribute(name, value);
      el.setAttribute(MARK, added.map(([name]) => name).join(" "));
    } else if (marked !== null) {
      for (const name of marked.split(" ").filter(Boolean)) el.removeAttribute(name);
      el.removeAttribute(MARK);
    }
  }
}

/**
 * Calls `onChange` whenever the column's width changes or a web font finishes loading: a font
 * swap widens a code block's text without resizing anything a ResizeObserver sees.
 */
function watchLayout(root: HTMLElement, onChange: () => void): () => void {
  const observer = new ResizeObserver(onChange);
  observer.observe(root);
  const fonts = "fonts" in document ? document.fonts : undefined;
  let active = true;
  fonts?.addEventListener("loadingdone", onChange);
  void fonts?.ready.then(() => {
    if (active) onChange();
  });
  return () => {
    active = false;
    observer.disconnect();
    fonts?.removeEventListener("loadingdone", onChange);
  };
}

export function Prose({ children, html, className }: ProseProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { t } = useStepclimbStrings();
  const codeLabel = t("prose.codeBlock");
  // Runs after every render, since the content may change.
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    const mark = () => markScrollers(root, codeLabel);
    mark();
    return watchLayout(root, mark);
  });
  if (html !== undefined) {
    return (
      <div
        ref={ref}
        data-slot="prose"
        className={prose({ className })}
        // biome-ignore lint/security/noDangerouslySetInnerHtml: HTML the server's renderer sanitized (spec 07 B14), inserted as given (B22)
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  }
  return (
    <div ref={ref} data-slot="prose" className={prose({ className })}>
      {children}
    </div>
  );
}
