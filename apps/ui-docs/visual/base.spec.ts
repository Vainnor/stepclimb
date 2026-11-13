// SPDX-License-Identifier: AGPL-3.0-or-later
// packages/ui's base.css in a real browser (docs/specs/m0/13-base-ui-components.md B9, B13 to
// B17; T4 to T6): touch targets, reduced motion, the OS color scheme, forced colors, overscroll and
// unselectable controls, over the stories of the static Storybook build.
import { expect, type Locator, type Page, test } from "@playwright/test";
import { markStoryRoots, openStory, STORY_ROOT, type Story, stories } from "./storybook";

const all = stories();
const story = (id: string): Story => {
  const found = all.find((s) => s.id === id);
  if (!found) throw new Error(`no story ${id} in the Storybook index`);
  return found;
};

/** What a person can press or type into. */
const INTERACTIVE = [
  "button",
  "[role=button]",
  "[role=checkbox]",
  "[role=gridcell]",
  "[role=menuitem]",
  "[role=menuitemcheckbox]",
  "[role=menuitemradio]",
  "[role=option]",
  "[role=radio]",
  "[role=switch]",
  "[role=tab]",
  "[role=treeitem]",
  "input:not([type=hidden])",
  "select",
  "textarea",
  "a[href]",
].join(", ");

/**
 * Targets in the story and its portaled overlays smaller than 44 by 44 px. A link set inline in text is left out: its size
 * follows the line it sits in (WCAG 2.5.8's inline exception). So is anything hidden or visually
 * hidden (1 px), which no one can tap.
 */
async function smallTargets(page: Page): Promise<string[]> {
  await markStoryRoots(page);
  return page.evaluate(
    ([roots, selector]) => {
      const out: string[] = [];
      for (const el of document.querySelectorAll<HTMLElement>(`:is(${roots}) :is(${selector})`)) {
        const rect = el.getBoundingClientRect();
        if (!el.checkVisibility() || rect.width <= 1 || rect.height <= 1) continue;
        if (el.matches("a") && getComputedStyle(el).display === "inline") continue;
        if (rect.width < 44 || rect.height < 44) {
          out.push(`${el.outerHTML.slice(0, 80)} is ${rect.width}x${rect.height}`);
        }
      }
      return out;
    },
    [STORY_ROOT, INTERACTIVE],
  );
}

/**
 * Whether something else is drawn over the middle of `target`, such as a modal's underlay over the
 * page behind it. No one can hover what is covered.
 */
async function isCovered(target: Locator): Promise<boolean> {
  return target.evaluate((el) => {
    // The first box: a link that wraps has one per line, and the middle of their union can fall
    // between them.
    const rect = el.getClientRects()[0] ?? el.getBoundingClientRect();
    const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    return hit === null || !(hit === el || el.contains(hit));
  });
}

/** Whether `target` is drawn at a size a person can point at: not hidden, nor visually hidden. */
async function isPointable(target: Locator): Promise<boolean> {
  if (!(await target.isVisible())) return false;
  const box = await target.boundingBox();
  return box !== null && box.width > 1 && box.height > 1;
}

test.describe("on a phone (375 px, coarse pointer) @phone", () => {
  for (const s of all) {
    test(`T4: ${s.title} / ${s.name} has targets of at least 44 px`, async ({ page }) => {
      await openStory(page, s, "light");
      expect(await smallTargets(page)).toEqual([]);
    });
  }

  test("text inputs are at least 16 px, so iOS does not zoom on focus", async ({ page }) => {
    await openStory(page, story("primitives-button--primary"), "light");
    const size = await page.evaluate(() => {
      const input = document.createElement("input");
      input.className = "text-sm";
      document.querySelector("#storybook-root")?.append(input);
      return getComputedStyle(input).fontSize;
    });
    expect(size).toBe("16px");
  });
});

test.describe("at 1280 px @desktop", () => {
  test.describe("with reduced motion", () => {
    test.use({ reducedMotion: "reduce" });
    for (const s of all) {
      test(`T5: ${s.title} / ${s.name} runs no transition after an interaction`, async ({
        page,
      }) => {
        await openStory(page, s, "light");
        await markStoryRoots(page);
        // The story's controls and those of an overlay it opened.
        const targets = page.locator(`:is(${STORY_ROOT}) :is(${INTERACTIVE})`);
        let visible = 0;
        let exercised = 0;
        for (let i = 0; i < (await targets.count()) && exercised < 4; i++) {
          const target = targets.nth(i);
          if (!(await isPointable(target))) continue;
          visible++;
          if (await isCovered(target)) continue;
          await target.hover();
          await target.focus();
          const running = await page.evaluate(
            () => document.getAnimations().filter((a) => a.playState === "running").length,
          );
          expect(running).toBe(0);
          exercised++;
          // A tooltip the target opened can cover the next one; Escape closes it, as a person would.
          if (await page.getByRole("tooltip").first().isVisible())
            await page.keyboard.press("Escape");
        }
        // A story with something to press had at least one of its controls exercised.
        if (visible > 0) expect(exercised).toBeGreaterThan(0);
      });
    }
  });

  test.describe("T6: with no theme on the page", () => {
    for (const scheme of ["dark", "light"] as const) {
      test(`color-scheme follows the OS ${scheme} setting`, async ({ page }) => {
        await openStory(page, story("primitives-button--primary"), "light");
        await page.emulateMedia({ colorScheme: scheme });
        const html = await page.evaluate(() => ({
          theme: document.documentElement.getAttribute("data-sc-theme"),
          scheme: getComputedStyle(document.documentElement).colorScheme,
        }));
        expect(html).toEqual({ theme: null, scheme });
      });
    }
  });

  test.describe("with forced colors", () => {
    test.beforeEach(async ({ page }) => {
      await openStory(page, story("primitives-button--primary"), "light");
      await page.emulateMedia({ forcedColors: "active" });
    });

    test("a focused control keeps a visible focus outline", async ({ page }) => {
      await page.keyboard.press("Tab");
      const button = page.locator("[data-slot=button]");
      await expect(button).toHaveAttribute("data-focus-visible", "true");
      const outline = await button.evaluate((el) => {
        const style = getComputedStyle(el);
        return { style: style.outlineStyle, width: style.outlineWidth };
      });
      expect(outline).toEqual({ style: "solid", width: "2px" });
    });

    test("a filled button keeps a border", async ({ page }) => {
      const border = await page.locator("[data-slot=button]").evaluate((el) => {
        const style = getComputedStyle(el);
        return { style: style.borderTopStyle, width: style.borderTopWidth };
      });
      expect(border).toEqual({ style: "solid", width: "1px" });
    });

    test("a selected or pressed control keeps an outline", async ({ page }) => {
      const outline = await page.evaluate(() => {
        const toggle = document.createElement("button");
        toggle.setAttribute("aria-pressed", "true");
        document.querySelector("#storybook-root")?.append(toggle);
        return getComputedStyle(toggle).outlineStyle;
      });
      expect(outline).toBe("solid");
    });
  });

  test.describe("with forced colors, rules stay visible", () => {
    for (const [id, selector] of [
      ["foundations-separator--horizontal", "[data-slot=separator]"],
      ["foundations-prose--lesson", "[data-slot=prose] hr"],
    ] as const) {
      test(`${selector} in ${id}`, async ({ page }) => {
        await openStory(page, story(id), "light");
        await page.emulateMedia({ forcedColors: "active" });
        const border = await page.locator(selector).evaluate((el) => {
          const style = getComputedStyle(el);
          return { style: style.borderBlockStartStyle, width: style.borderBlockStartWidth };
        });
        expect(border).toEqual({ style: "solid", width: "1px" });
      });
    }
  });

  test("the document does not rubber-band, and controls are not text-selectable", async ({
    page,
  }) => {
    await openStory(page, story("primitives-button--primary"), "light");
    const styles = await page.evaluate(() => ({
      overscroll: getComputedStyle(document.documentElement).overscrollBehaviorY,
      select: getComputedStyle(document.querySelector("[data-slot=button]") as Element).userSelect,
    }));
    expect(styles).toEqual({ overscroll: "none", select: "none" });
  });

  test("caret and scrollbar colors follow a nested theme", async ({ page }) => {
    await openStory(page, story("primitives-button--primary"), "dark");
    // The page itself light, the story's ThemeProvider dark.
    await page.emulateMedia({ colorScheme: "light" });
    const colors = await page.evaluate(() => {
      const read = (el: Element) => {
        const style = getComputedStyle(el);
        return { caret: style.caretColor, scrollbar: style.scrollbarColor };
      };
      const provider = document.querySelector("[data-slot=theme-provider]");
      if (!provider) throw new Error("the story has no ThemeProvider");
      return { page: read(document.documentElement), dark: read(provider) };
    });
    expect(colors.dark.caret).not.toBe(colors.page.caret);
    expect(colors.dark.scrollbar).not.toBe(colors.page.scrollbar);
  });

  test("Prose sets a measure of 62 to 68 characters at the reading size", async ({ page }) => {
    await openStory(page, story("foundations-prose--lesson"), "light");
    const lines = await page.evaluate(() => {
      // Characters on each line of every paragraph but its last line, which ends early.
      const counts: number[] = [];
      for (const p of document.querySelectorAll('[data-slot="prose"] p')) {
        // A character starts a new line when it sits wholly below the line so far; inline code,
        // in a smaller face, shares its line with the text around it.
        const perLine: number[] = [];
        let lineBottom = Number.NEGATIVE_INFINITY;
        const walker = document.createTreeWalker(p, NodeFilter.SHOW_TEXT);
        const range = document.createRange();
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          for (let i = 0; i < (node.textContent ?? "").length; i++) {
            range.setStart(node, i);
            range.setEnd(node, i + 1);
            const rect = range.getClientRects()[0];
            if (!rect) continue;
            if (rect.top >= lineBottom) {
              perLine.push(0);
              lineBottom = rect.bottom;
            }
            lineBottom = Math.max(lineBottom, rect.bottom);
            perLine[perLine.length - 1] = (perLine.at(-1) ?? 0) + 1;
          }
        }
        counts.push(...perLine.slice(0, -1));
      }
      return counts;
    });
    expect(lines.length).toBeGreaterThanOrEqual(4);
    const mean = lines.reduce((a, b) => a + b, 0) / lines.length;
    expect(mean, `characters per line: ${lines.join(", ")}`).toBeGreaterThanOrEqual(62);
    expect(mean, `characters per line: ${lines.join(", ")}`).toBeLessThanOrEqual(68);
  });

  test("keyboard focus on an unstyled element draws the ring", async ({ page }) => {
    await openStory(page, story("primitives-button--primary"), "light");
    await page.evaluate(() => {
      const el = document.createElement("div");
      el.tabIndex = 0;
      document.querySelector("#storybook-root")?.prepend(el);
    });
    await page.keyboard.press("Tab");
    const focused = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      return { slot: el.dataset.slot ?? null, shadow: getComputedStyle(el).boxShadow };
    });
    expect(focused.slot).toBeNull();
    expect(focused.shadow).toContain("0px 0px 0px 4px");
  });
});
