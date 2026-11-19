// SPDX-License-Identifier: AGPL-3.0-or-later
// Every story in light, dark and high contrast at 1280 and 375 px (the two projects): a screenshot
// compared with its committed baseline, and axe with zero violations of WCAG 2.2 A and AA
// (docs/specs/m0/13-base-ui-components.md B24, B29; Q4 matrix (a)). A matrix story draws every
// brand, theme and density itself, so it runs once per width.

import { readdirSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import {
  expectNoPageErrors,
  isMatrix,
  markStoryRoots,
  openStory,
  STORY_ROOT,
  type Story,
  stories,
  type Theme,
  themes,
} from "./storybook";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's violations in the story and its portaled overlays, one line per failing node. */
async function axeViolations(page: Page): Promise<string[]> {
  await markStoryRoots(page);
  const result = await new AxeBuilder({ page }).include(STORY_ROOT).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

/** Each story's runs: a matrix story once per width, any other once per theme and width. */
const runs: [Story, Theme, string][] = stories().flatMap((story) =>
  isMatrix(story)
    ? [[story, "light", "matrix"] as [Story, Theme, string]]
    : themes.map((theme): [Story, Theme, string] => [story, theme, theme]),
);
const widths = ["1280", "375"];

/** Where the story's box ends, from the top of the document, whatever the page's scroll. */
const storyBottom = (page: Page): Promise<number> =>
  page.evaluate(() => {
    const root = document.querySelector("#storybook-root");
    if (root === null) throw new Error("the story drew no #storybook-root");
    return Math.ceil(root.getBoundingClientRect().bottom + window.scrollY);
  });

/** Resizes past this many are a story that will not settle. */
const MAX_RESIZES = 8;

/**
 * Grows the viewport to the story's height when the story is taller. Playwright captures an element
 * taller than the viewport with Chromium's `captureBeyondViewport`, which turns touch emulation off
 * for the capture, so a tall story at 375 px would be drawn for a fine pointer. A viewport the
 * story fits keeps the capture inside it. The width stays, and so do mobile and touch emulation.
 * A story that overflows by the same amount after a resize follows the viewport's height and fits
 * none; one whose layout moves with the height for another reason gets more resizes, up to a cap.
 */
async function fitViewport(page: Page): Promise<void> {
  const viewport = page.viewportSize();
  if (viewport === null) throw new Error("the story suite needs a fixed viewport");
  let height = viewport.height;
  let overflow: number | null = null;
  for (let resizes = 0; ; resizes++) {
    const needed = await storyBottom(page);
    if (needed <= height) return;
    if (needed - height === overflow) {
      throw new Error(
        `the story follows the viewport's height, ${overflow} px past it at ${height} px`,
      );
    }
    if (resizes === MAX_RESIZES) {
      throw new Error(`the story still overflows the viewport after ${MAX_RESIZES} resizes`);
    }
    overflow = needed - height;
    height = needed;
    await page.setViewportSize({ width: viewport.width, height });
  }
}

/** Fails if the story ends below the viewport: it grew after `fitViewport`, so lost touch. */
async function expectFits(page: Page): Promise<void> {
  const height = page.viewportSize()?.height;
  const bottom = await storyBottom(page);
  expect(bottom, "the story grew past the viewport after it was fitted").toBeLessThanOrEqual(
    height ?? 0,
  );
}

for (const [story, theme, name] of runs) {
  test(`${story.title} / ${story.name} (${name})`, async ({ page, hasTouch }) => {
    await openStory(page, story, theme);
    if (hasTouch) await fitViewport(page);
    // The story's own box: the decorator draws it only as tall as its content.
    await expect(page.locator("#storybook-root")).toHaveScreenshot(`${story.id}--${name}.png`);
    if (hasTouch) await expectFits(page);
    expect(await axeViolations(page)).toEqual([]);
    expectNoPageErrors(page, story);
  });
}

test("every baseline belongs to a story @desktop", () => {
  const expected = new Set(
    runs.flatMap(([story, , name]) => widths.map((w) => `${story.id}--${name}--${w}.png`)),
  );
  const files = readdirSync(new URL("./baselines/", import.meta.url));
  const orphans = files.filter((file) => !expected.has(file));
  expect(orphans, "baselines no story draws; delete them").toEqual([]);
});
