// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: renders under a StringsProvider with Stepclimb UI's catalog, or with one whose
// every message is its id in «guillemets», so a test can tell catalog text from hard-coded text
// (docs/specs/m0/13-base-ui-components.md T2).
import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import { type Catalog, uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";

const marked: Catalog = {
  en: Object.fromEntries(Object.keys(uiMessages.en).map((id) => [id, `«${id}»`])),
};

export function renderWithStrings(ui: ReactElement, { marked: useMarked = false } = {}) {
  const catalogs = [useMarked ? marked : uiMessages];
  return render(
    <StringsProvider catalogs={catalogs} isDevelopment>
      {ui}
    </StringsProvider>,
  );
}

/**
 * Text in `root` that is not inside «…»: every visible or screen-reader text node and every
 * `aria-label`, with whitespace and punctuation outside the marks ignored.
 */
export function unmarkedText(root: HTMLElement): string[] {
  const texts: string[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode())
    texts.push(node.textContent ?? "");
  for (const el of root.querySelectorAll("[aria-label]"))
    texts.push(el.getAttribute("aria-label") ?? "");
  return texts
    .map((t) => t.replace(/«[^»]*»/g, "").replace(/[\s\p{P}]/gu, ""))
    .filter((t) => t.length > 0);
}
