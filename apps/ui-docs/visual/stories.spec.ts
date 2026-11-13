// SPDX-License-Identifier: AGPL-3.0-or-later
// Every story in light, dark and high contrast at 1280 and 375 px (the two projects): a screenshot
// compared with its committed baseline, and axe with zero violations of WCAG 2.2 A and AA
// (docs/specs/m0/13-base-ui-components.md B24, B29; Q4 matrix (a)). A matrix story draws every
// brand, theme and density itself, so it runs once per width.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { isMatrix, openStory, type Story, stories, type Theme, themes } from "./storybook";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's violations in the story, one line per failing node. */
async function axeViolations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).include("#storybook-root").withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

function check(story: Story, theme: Theme, name: string) {
  test(`${story.title} / ${story.name} (${name})`, async ({ page }) => {
    await openStory(page, story, theme);
    await expect(page).toHaveScreenshot(`${story.id}--${name}.png`, { fullPage: true });
    expect(await axeViolations(page)).toEqual([]);
  });
}

for (const story of stories()) {
  if (isMatrix(story)) check(story, "light", "matrix");
  else for (const theme of themes) check(story, theme, theme);
}
