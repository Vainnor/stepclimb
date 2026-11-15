// SPDX-License-Identifier: AGPL-3.0-or-later
// React Aria's Button announces a focused button turning pending, and turning back, through its
// live announcer: a `role="img"` node whose `aria-labelledby` names the button's id (and, when the
// button has an `aria-label` or `aria-labelledby`, its spinner's id), removed after 7 s. A button
// that unmounts inside those 7 s leaves the node pointing at ids no element has, which axe reports
// as `role-img-alt`. This hook removes those nodes when the button unmounts. No announcement that
// could still be read is lost: a screen reader reads a node of the announcer's
// `aria-relevant="additions"` regions when it is added, and a node whose label is already gone
// reads nothing; the one React Aria adds after the button has gone is removed before the browser
// renders it.
import { useId, useLayoutEffect } from "react";

/** React Aria delays a page's first announcement this long, until its live regions are attached. */
const FIRST_ANNOUNCEMENT_DELAY_MS = 100;

function withdraw(ids: ReadonlySet<string>) {
  for (const node of document.querySelectorAll("[data-live-announcer] [aria-labelledby]")) {
    const refs = (node.getAttribute("aria-labelledby") ?? "").split(/\s+/);
    if (refs.some((ref) => ids.has(ref))) node.remove();
  }
}

/**
 * Withdraws the announcements React Aria adds to `announcer` for `ids` until its delayed first
 * announcement is due. The observer's callback runs in the microtask after the node is added,
 * before the browser renders or runs another task.
 */
function withdrawLateAnnouncements(announcer: Element, ids: ReadonlySet<string>) {
  const observer = new MutationObserver(() => withdraw(ids));
  observer.observe(announcer, { childList: true, subtree: true });
  setTimeout(() => observer.disconnect(), FIRST_ANNOUNCEMENT_DELAY_MS);
}

/**
 * The id to give a React Aria `Button`: `id` when the caller passed one, else a generated one.
 * When the id leaves the document, the announcements that point at it or at an element inside the
 * button go too, including one React Aria adds after the button has gone (its first, delayed one).
 */
export function usePendingAnnouncementId(id: string | undefined): string {
  const generated = useId();
  const buttonId = id ?? generated;
  useLayoutEffect(
    () => () => {
      // React Aria creates the announcer, synchronously, on its first announcement; without it
      // there is nothing to withdraw now or later.
      const announcer = document.querySelector("[data-live-announcer]");
      if (announcer === null) return;
      // A layout cleanup runs before React removes the element, so its descendants' ids are known.
      const ids = new Set([buttonId]);
      for (const el of document.getElementById(buttonId)?.querySelectorAll("[id]") ?? [])
        ids.add(el.id);
      withdraw(ids);
      withdrawLateAnnouncements(announcer, ids);
    },
    [buttonId],
  );
  return buttonId;
}
