// SPDX-License-Identifier: AGPL-3.0-or-later
// What the story suite reads of the static Storybook build: the stories in its index, and a story
// opened in its own frame in a theme, waited on until it has rendered and played.
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";

export const themes = ["light", "dark", "high-contrast"] as const;
export type Theme = (typeof themes)[number];

/** A story that draws every brand, theme and density itself (`ThemeMatrix`) carries this tag. */
export const MATRIX_TAG = "theme-matrix";

export interface Story {
  id: string;
  title: string;
  name: string;
  tags: string[];
}

interface IndexEntry {
  type: string;
  id: string;
  title: string;
  name: string;
  tags?: string[];
}

/** Every story in `storybook-static/index.json`, in index order; docs pages are left out. */
export function stories(): Story[] {
  const url = new URL("../storybook-static/index.json", import.meta.url);
  const index: { entries: Record<string, IndexEntry> } = JSON.parse(readFileSync(url, "utf8"));
  return Object.values(index.entries)
    .filter((entry) => entry.type === "story")
    .map(({ id, title, name, tags }) => ({ id, title, name, tags: tags ?? [] }));
}

export const isMatrix = (story: Story) => story.tags.includes(MATRIX_TAG);

/**
 * Opens `story` in `theme`: the Storybook theme global for stories in the toolbar's theme, and the
 * matching OS setting (dark, more contrast) for stories that follow it. Resolves once the story
 * has rendered, its play function has run and its fonts have loaded; fails if it errored.
 */
export async function openStory(page: Page, story: Story, theme: Theme): Promise<void> {
  await page.emulateMedia({
    colorScheme: theme === "dark" ? "dark" : "light",
    contrast: theme === "high-contrast" ? "more" : "no-preference",
  });
  await page.goto(`/iframe.html?id=${story.id}&viewMode=story&globals=theme:${theme}`);
  // Storybook's last render phase is `finished`, after the play function, whether or not the
  // story threw; an error shows Storybook's error display instead of the story.
  await page.waitForFunction(() => {
    const preview = (window as { __STORYBOOK_PREVIEW__?: { currentRender?: { phase?: string } } })
      .__STORYBOOK_PREVIEW__;
    return preview?.currentRender?.phase === "finished";
  });
  const failed = await page.evaluate(() =>
    document.body.classList.contains("sb-show-errordisplay")
      ? (document.querySelector("#error-message")?.textContent ?? "an error")
      : null,
  );
  if (failed !== null) throw new Error(`story ${story.id} failed: ${failed}`);
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

/** The attribute `markStoryRoots` sets on everything the story drew. */
export const STORY_ROOT = "[data-sc-visual-root]";

/**
 * Marks the story's root and every element the story portaled to the body (React Aria's overlays
 * and its top layer), leaving out Storybook's own nodes: its `sb-` wrappers, `#storybook-docs`,
 * scripts and styles. Axe and the target check read what `STORY_ROOT` selects.
 */
export async function markStoryRoots(page: Page): Promise<void> {
  await page.evaluate(
    (attribute) => {
      for (const el of document.body.children) {
        const own =
          el.id === "storybook-docs" ||
          [...el.classList].some((c) => c.startsWith("sb-")) ||
          ["SCRIPT", "STYLE", "LINK", "TEMPLATE"].includes(el.tagName);
        if (el.id === "storybook-root" || !own) el.setAttribute(attribute, "");
      }
    },
    STORY_ROOT.slice(1, -1),
  );
}
