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
 * Records, on `window.__scVisualFailures`, what Storybook's channel reports of a failed play step:
 * an `errored` render phase, the play function's exception and errors left unhandled while it
 * played. `storybook/test` sets `throwPlayFunctionExceptions: false`, so a failed `expect` in a play
 * function only logs: the story still reaches `finished` and shows no error display, and the
 * channel is the only place it shows. A render error shows Storybook's error display, which
 * `openStory` reads. The channel is hooked as Storybook installs it, before any story code runs, and
 * `window.__scVisualHooked` says it was.
 */
function recordFailures(): void {
  type Channel = { on(event: string, listener: (payload: unknown) => void): void };
  const w = window as unknown as { __scVisualFailures?: string[]; __scVisualHooked?: boolean };
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
    w.__scVisualHooked = true;
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
 * the play function threw (a failed `expect`), an error went unhandled in the page, or the
 * failure hooks never reached Storybook's channel. `expectNoPageErrors` checks the page again
 * after the screenshot.
 */
export async function openStory(page: Page, story: Story, theme: Theme): Promise<void> {
  await page.emulateMedia({
    colorScheme: theme === "dark" ? "dark" : "light",
    contrast: theme === "high-contrast" ? "more" : "no-preference",
  });
  watchPageErrors(page);
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
    const w = window as { __scVisualFailures?: string[]; __scVisualHooked?: boolean };
    if (w.__scVisualHooked !== true) return "Storybook channel not hooked";
    const failures = w.__scVisualFailures ?? [];
    if (document.body.classList.contains("sb-show-errordisplay")) {
      failures.push(document.querySelector("#error-message")?.textContent ?? "an error");
    }
    const reasons = failures.filter((f) => f !== "errored");
    return failures.length === 0 ? null : reasons.join("; ") || "a render phase errored";
  });
  if (failed !== null) throw new Error(`story ${story.id} failed: ${failed}`);
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  expectNoPageErrors(page, story);
}

/** Every error the page left unhandled, from the first `openStory` on it. */
const pageErrors = new WeakMap<Page, string[]>();

function watchPageErrors(page: Page): void {
  if (pageErrors.has(page)) return;
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on("pageerror", (error) => errors.push(error.message.split("\n").slice(0, 12).join("\n")));
}

/** Fails if the page left an error unhandled: while rendering, playing, or after the play step. */
export function expectNoPageErrors(page: Page, story: Story): void {
  const errors = pageErrors.get(page) ?? [];
  if (errors.length > 0) {
    throw new Error(`story ${story.id} failed: unhandled in the page: ${errors.join("; ")}`);
  }
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
