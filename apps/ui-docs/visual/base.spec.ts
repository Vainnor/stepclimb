// SPDX-License-Identifier: AGPL-3.0-or-later
// packages/ui's base.css in a real browser (docs/specs/m0/13-base-ui-components.md B9, B13 to
// B17; T4 to T6): touch targets, reduced motion, the OS color scheme, forced colors, overscroll and
// unselectable controls, over the stories of the static Storybook build.
import { expect, type Page, test } from "@playwright/test";
import { openStory, type Story, stories } from "./storybook";

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
  "[role=menuitem]",
  "[role=option]",
  "[role=radio]",
  "[role=switch]",
  "[role=tab]",
  "input:not([type=hidden])",
  "select",
  "textarea",
  "a[href]",
].join(", ");

/**
 * Targets in the story smaller than 44 by 44 px. A link set inline in text is left out: its size
 * follows the line it sits in (WCAG 2.5.8's inline exception). So is anything hidden or visually
 * hidden (1 px), which no one can tap.
 */
function smallTargets(page: Page): Promise<string[]> {
  return page.evaluate((selector) => {
    const out: string[] = [];
    for (const el of document.querySelectorAll<HTMLElement>(`#storybook-root :is(${selector})`)) {
      const rect = el.getBoundingClientRect();
      if (!el.checkVisibility() || rect.width <= 1 || rect.height <= 1) continue;
      if (el.matches("a") && getComputedStyle(el).display === "inline") continue;
      if (rect.width < 44 || rect.height < 44) {
        out.push(`${el.outerHTML.slice(0, 80)} is ${rect.width}x${rect.height}`);
      }
    }
    return out;
  }, INTERACTIVE);
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
        const targets = page.locator(`#storybook-root :is(${INTERACTIVE})`);
        for (let i = 0; i < Math.min(await targets.count(), 4); i++) {
          const target = targets.nth(i);
          if (!(await target.isVisible())) continue;
          await target.hover();
          await target.focus();
          const running = await page.evaluate(
            () => document.getAnimations().filter((a) => a.playState === "running").length,
          );
          expect(running).toBe(0);
        }
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
