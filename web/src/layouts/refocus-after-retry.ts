// SPDX-License-Identifier: AGPL-3.0-or-later
// Focus after a "Try again" that took its own button away (docs/specs/m0/11-app-shell.md B6,
// B28, B29). When the retry starts or works, the error or notice that held the button is replaced
// and the browser drops focus to the page body; a keyboard or screen-reader user would start again
// from the top. Focus goes to the view's `h1` instead, the same place a navigation puts it. A
// query with no data shows its loading state while it fetches again, so the `h1` can arrive a
// moment after the button left.
import { useEffect, useRef } from "react";
import { focusViewHeading } from "./root";

/** How long to wait for the next view's `h1`; a slower view leaves focus where the browser put it. */
const WAIT_FOR_HEADING_MS = 10_000;

/**
 * Focuses the page's `h1` once there is one, unless focus has gone somewhere other than the page
 * body by then: the user moved on, and keeps their place.
 */
function focusHeadingOnceDrawn(): void {
  const settle = (): boolean => {
    const active = document.activeElement;
    if (active !== null && active !== document.body) return true;
    if (document.querySelector("h1") === null) return false;
    focusViewHeading();
    return true;
  };
  if (settle()) return;
  const observer = new MutationObserver(() => {
    if (settle()) stop();
  });
  const timer = setTimeout(() => stop(), WAIT_FOR_HEADING_MS);
  function stop() {
    observer.disconnect();
    clearTimeout(timer);
  }
  observer.observe(document.body, { childList: true, subtree: true });
}

/**
 * Returns the function a "Try again" calls when pressed. When the component that called this
 * unmounts after such a press, focus that fell to the page body goes to the view's `h1`.
 */
export function useRefocusAfterRetry(): () => void {
  const retried = useRef(false);
  useEffect(
    () => () => {
      if (retried.current) focusHeadingOnceDrawn();
    },
    [],
  );
  return () => {
    retried.current = true;
  };
}
