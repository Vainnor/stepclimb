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
 * Records, on `window.__scVisualFailures`, every render phase Storybook reports as `errored` and the
 * error it carries. `storybook/test` sets `throwPlayFunctionExceptions: false`, so a failed `expect`
 * in a play function only logs: the story still reaches `finished` and shows no error display. The
 * phase and the exception arrive on Storybook's channel, which this hooks as Storybook installs it,
 * before any story code runs.
 */
function recordFailures(): void {
  type Channel = { on(event: string, listener: (payload: unknown) => void): void };
  const w = window as unknown as { __scVisualFailures?: string[] };
  if (w.__scVisualFailures !== undefined) return;
  const failures: string[] = [];
  w.__scVisualFailures = failures;
  // The first lines: testing-library's errors go on to print the whole document.
  const message = (error: unknown): string => {
    const text =
      typeof error === "object" && error !== null && "message" in error
        ? String((error as { message: unknown }).message)
        : String(error);
    return text.split("\n").slice(0, 12).join("\n");
  };
  const hooked = new WeakSet<object>();
  const hook = (channel: Channel) => {
    if (hooked.has(channel)) return;
    hooked.add(channel);
    channel.on("storyRenderPhaseChanged", (payload) => {
      if ((payload as { newPhase?: string }).newPhase === "errored") failures.push("errored");
    });
    channel.on("playFunctionThrewException", (error) => failures.push(message(error)));
    channel.on("unhandledErrorsWhilePlaying", (errors) => {
      for (const error of errors as unknown[]) failures.push(message(error));
    });
  };
  let current: unknown;
  Object.defineProperty(window, "__STORYBOOK_ADDONS_CHANNEL__", {
    configurable: true,
    get: () => current,
    set: (next: unknown) => {
      current = next;
      if (typeof next === "object" && next !== null && "on" in next) hook(next as Channel);
    },
  });
}

/**
 * Opens `story` in `theme`: the Storybook theme global for stories in the toolbar's theme, and the
 * matching OS setting (dark, more contrast) for stories that follow it. Resolves once the story
 * has rendered, its play function has run and its fonts have loaded; fails if rendering errored,
 * the play function threw (a failed `expect`) or an error went unhandled while it played.
 */
export async function openStory(page: Page, story: Story, theme: Theme): Promise<void> {
  await page.emulateMedia({
    colorScheme: theme === "dark" ? "dark" : "light",
    contrast: theme === "high-contrast" ? "more" : "no-preference",
  });
  await page.addInitScript(recordFailures);
  await page.goto(`/iframe.html?id=${story.id}&viewMode=story&globals=theme:${theme}`);
  // Storybook's last render phase is `finished`, after the play function, whether or not the
  // story threw.
  await page.waitForFunction(() => {
    const preview = (window as { __STORYBOOK_PREVIEW__?: { currentRender?: { phase?: string } } })
      .__STORYBOOK_PREVIEW__;
    return preview?.currentRender?.phase === "finished";
  });
  const failed = await page.evaluate(() => {
    const failures = (window as { __scVisualFailures?: string[] }).__scVisualFailures ?? [];
    if (document.body.classList.contains("sb-show-errordisplay")) {
      failures.push(document.querySelector("#error-message")?.textContent ?? "an error");
    }
    const reasons = failures.filter((f) => f !== "errored");
    return failures.length === 0 ? null : reasons.join("; ") || "a render phase errored";
  });
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
