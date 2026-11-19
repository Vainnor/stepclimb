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

/**
 * Grows the viewport to the story's height when the story is taller. Playwright captures an element
 * taller than the viewport with Chromium's `captureBeyondViewport`, which turns touch emulation off
 * for the capture, so a tall story at 375 px would be drawn for a fine pointer. A viewport the
 * story fits keeps the capture inside it. The width stays, and so do mobile and touch emulation.
 */
async function fitViewport(page: Page): Promise<void> {
  const viewport = page.viewportSize();
  if (viewport === null) throw new Error("the story suite needs a fixed viewport");
  const root = page.locator("#storybook-root");
  let height = viewport.height;
  // A second pass covers a story whose layout moves with the viewport's height; a story that grows
  // with every resize has no height that fits it.
  for (let pass = 0; pass < 3; pass++) {
    const box = await root.boundingBox();
    if (box === null) throw new Error("the story drew no #storybook-root box");
    const needed = Math.ceil(box.y + box.height);
    if (needed <= height) return;
    height = needed;
    await page.setViewportSize({ width: viewport.width, height });
  }
  throw new Error(`the story keeps growing with the viewport, past ${height} px`);
}

for (const [story, theme, name] of runs) {
  test(`${story.title} / ${story.name} (${name})`, async ({ page, hasTouch }) => {
    await openStory(page, story, theme);
    if (hasTouch) await fitViewport(page);
    // The story's own box: the decorator draws it only as tall as its content.
    await expect(page.locator("#storybook-root")).toHaveScreenshot(`${story.id}--${name}.png`);
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
