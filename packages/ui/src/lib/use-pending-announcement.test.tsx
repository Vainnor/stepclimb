// SPDX-License-Identifier: AGPL-3.0-or-later
// React Aria delays the first announcement on a page by 100 ms, unless the environment is React's
// act environment. This file renders outside act, as a browser does, so its first pending button
// is the page's first announcement; it has its own file so no earlier test created the announcer.
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Button } from "../primitives/button";
import { uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

let actEnvironment: boolean | undefined;
beforeEach(() => {
  actEnvironment = globalThis.IS_REACT_ACT_ENVIRONMENT;
  globalThis.IS_REACT_ACT_ENVIRONMENT = false;
});
afterEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = actEnvironment;
});

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function referencedIds() {
  return [...document.querySelectorAll("[data-live-announcer] [aria-labelledby]")].flatMap((node) =>
    (node.getAttribute("aria-labelledby") ?? "").split(/\s+/),
  );
}

describe("usePendingAnnouncementId", () => {
  it("takes back the page's first, delayed announcement of a button already gone", async () => {
    const container = document.body.appendChild(document.createElement("div"));
    const root = createRoot(container);
    const draw = (isPending: boolean) =>
      flushSync(() =>
        root.render(
          <StringsProvider catalogs={[uiMessages]} isDevelopment>
            <Button id="sign-in" isPending={isPending}>
              Sign in
            </Button>
          </StringsProvider>,
        ),
      );
    draw(false);
    document.getElementById("sign-in")?.focus();
    await wait(0);
    draw(true);
    expect(document.querySelector("[data-live-announcer]")).not.toBeNull();
    expect(referencedIds()).toEqual([]);
    const added: string[] = [];
    const recorder = new MutationObserver((records) => {
      for (const record of records)
        for (const node of record.addedNodes)
          if (node instanceof Element) added.push(node.getAttribute("aria-labelledby") ?? "");
    });
    const announcer = document.querySelector("[data-live-announcer]");
    if (announcer === null) throw new Error("React Aria created no announcer");
    recorder.observe(announcer, { childList: true, subtree: true });
    flushSync(() => root.unmount());
    const seen: string[][] = [];
    const probe = setInterval(() => seen.push(referencedIds()), 5);
    await wait(250);
    clearInterval(probe);
    recorder.disconnect();
    // React Aria did add the delayed announcement after the unmount, and no task ever saw it.
    expect(added).toContain("sign-in");
    expect(seen.length).toBeGreaterThan(10);
    expect(seen.flat()).not.toContain("sign-in");
    container.remove();
  });
});
