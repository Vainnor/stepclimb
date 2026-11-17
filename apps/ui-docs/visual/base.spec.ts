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
  "[role=columnheader]",
  "[role=gridcell]",
  "[role=menuitem]",
  "[role=menuitemcheckbox]",
  "[role=menuitemradio]",
  "[role=option]",
  "[role=radio]",
  "[role=spinbutton]",
  "[role=switch]",
  "[role=tab]",
  "[role=textbox]",
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
 * What a person points at to reach `target`: the target itself, the label it sits in when the
 * label is drawn over it (a checkbox, radio or switch whose native input is visually hidden inside
 * its label), or null when something else is drawn over its middle, such as a modal's underlay
 * over the page behind it. No one can hover what is covered.
 */
async function pointTarget(target: Locator): Promise<Locator | null> {
  const hit = await target.evaluate((el) => {
    // The first box: a link that wraps has one per line, and the middle of their union can fall
    // between them.
    const rect = el.getClientRects()[0] ?? el.getBoundingClientRect();
    const found = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    if (found !== null && (found === el || el.contains(found))) return "self";
    const label = el.closest("label");
    return found !== null && label?.contains(found) ? "label" : "covered";
  });
  if (hit === "self") return target;
  return hit === "label" ? target.locator("xpath=ancestor::label[1]") : null;
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
          const pointed = await pointTarget(target);
          if (pointed === null) continue;
          await pointed.hover();
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

  test.describe("with forced colors, a selected radio and switch stay visible", () => {
    for (const [id, selector, width] of [
      ["forms-radiogroup--default", "[data-slot=radio-dot]", "5px"],
      ["forms-switch--on", "[data-slot=switch-thumb]", "7px"],
    ] as const) {
      test(`${selector} in ${id}`, async ({ page }) => {
        await openStory(page, story(id), "light");
        await page.emulateMedia({ forcedColors: "active" });
        // Forced colors drop backgrounds; the mark is a border, which they keep.
        const border = await page.locator(selector).evaluate((el) => {
          const style = getComputedStyle(el);
          return { style: style.borderTopStyle, width: style.borderTopWidth };
        });
        expect(border).toEqual({ style: "solid", width });
      });
    }
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

  test("T2: with forced colors, every StatusChip keeps its own icon and label", async ({
    page,
  }) => {
    // docs/specs/m1/16-learning-ui-components.md B3: color is never the only signal.
    await openStory(page, story("data-statuschip--all-states"), "light");
    await page.emulateMedia({ forcedColors: "active" });
    const chips = await page.locator("[data-slot=status-chip]").evaluateAll((els) =>
      els.map((el) => {
        const icon = el.querySelector("svg");
        const box = icon?.getBoundingClientRect();
        return {
          drawing: icon?.innerHTML ?? "",
          label: el.textContent ?? "",
          // Forced colors paint the icon in the text color, not the canvas color.
          visible:
            box !== undefined &&
            box.width > 0 &&
            icon !== null &&
            getComputedStyle(icon).color !== getComputedStyle(el).backgroundColor,
        };
      }),
    );
    expect(chips).toHaveLength(6);
    expect(chips.every((c) => c.visible)).toBe(true);
    expect(new Set(chips.map((c) => c.drawing)).size).toBe(6);
    expect(new Set(chips.map((c) => c.label)).size).toBe(6);
  });

  test("T2: with forced colors, every ProgressRail node keeps its own icon and name", async ({
    page,
  }) => {
    // docs/specs/m1/16-learning-ui-components.md B3, B8: color is never the only signal.
    await openStory(page, story("learning-progressrail--all-states"), "light");
    await page.emulateMedia({ forcedColors: "active" });
    const nodes = await page.locator("[data-slot=progress-rail-link]").evaluateAll((els) =>
      els.map((el) => {
        const icon = el.querySelector("[data-slot=progress-rail-node] svg");
        const box = icon?.getBoundingClientRect();
        return {
          drawing: icon?.innerHTML ?? "",
          name: el.getAttribute("aria-label") ?? "",
          visible:
            box !== undefined &&
            box.width > 0 &&
            icon !== null &&
            getComputedStyle(icon).color !== getComputedStyle(el).backgroundColor,
        };
      }),
    );
    expect(nodes).toHaveLength(6);
    expect(nodes.every((n) => n.visible)).toBe(true);
    expect(new Set(nodes.map((n) => n.drawing)).size).toBe(6);
    expect(new Set(nodes.map((n) => n.name)).size).toBe(6);
  });

  test("T2: with forced colors, every ItemRow and ModuleRow state keeps its own icon and label", async ({
    page,
  }) => {
    // docs/specs/m1/16-learning-ui-components.md B3, B9: color is never the only signal.
    const states = async (id: string, row: string) => {
      await openStory(page, story(id), "light");
      await page.emulateMedia({ forcedColors: "active" });
      return page.locator(`[data-slot=${row}]`).evaluateAll((els) =>
        els.map((el) => {
          const chip = el.querySelector("[data-slot=status-chip]");
          const icon = chip?.querySelector("svg") ?? null;
          const box = icon?.getBoundingClientRect();
          return {
            state: el.getAttribute("data-state") ?? "",
            drawing: icon?.innerHTML ?? "",
            label: chip?.textContent ?? "",
            visible:
              chip !== null &&
              box !== undefined &&
              box.width > 0 &&
              icon !== null &&
              getComputedStyle(icon).color !== getComputedStyle(chip).backgroundColor,
          };
        }),
      );
    };
    const items = await states("learning-itemrow--all-states", "item-row");
    const modules = [
      ...(await states("learning-modulerow--all-states", "module-row")),
      ...(await states("learning-modulerow--locked", "module-row")),
    ];
    for (const rows of [items, modules]) {
      expect(rows).toHaveLength(6);
      expect(rows.every((r) => r.visible)).toBe(true);
      expect(new Set(rows.map((r) => r.state)).size).toBe(6);
      expect(new Set(rows.map((r) => r.drawing)).size).toBe(6);
      expect(new Set(rows.map((r) => r.label)).size).toBe(6);
    }
  });

  test("T5: dragging a ModuleBoard row with the pointer makes the move and announces it", async ({
    page,
  }) => {
    // docs/specs/m1/16-learning-ui-components.md B13, T5: React Aria's drag and drop in a browser.
    await openStory(page, story("authoring-moduleboard--default"), "light");
    const order = () =>
      page.locator("[data-slot=module-board] [role=row][aria-label]").evaluateAll((rows) =>
        rows.map((r) => r.getAttribute("aria-label")),
      );
    const before = await order();
    // Over the top edge of "Taxi diagram": the place before it, the first of Ground control.
    await page
      .getByRole("row", { name: /^Readbacks/ })
      .dragTo(page.getByRole("row", { name: /^Taxi diagram/ }), {
        targetPosition: { x: 40, y: 4 },
      });
    await expect(page.getByRole("status")).toHaveText(
      "Moved Readbacks to position 1 of 2 in Ground control.",
    );
    const after = await order();
    expect(after).toEqual(
      before.filter((n) => n !== "Readbacks").flatMap((n) => (n === "Taxi diagram" ? ["Readbacks", n] : [n])),
    );
  });

  test("T4: a completed item's segment fills over 320 ms, the course's rail once", async ({
    page,
  }) => {
    await openStory(page, story("learning-progressrail--completion"), "light");
    const next = page.getByRole("button", { name: "Complete the next item" });
    const fills = page.locator("[data-slot=progress-rail-fill]");
    // Computed times in milliseconds: Chromium serializes them in seconds.
    const motion = () =>
      fills.evaluateAll((els) =>
        els.map((el) => {
          const style = getComputedStyle(el);
          const ms = (s: string) => Math.round(Number.parseFloat(s) * 100000) / 100;
          return {
            scale: style.scale,
            transition: ms(style.transitionDuration),
            animation: style.animationName,
            ease: style.animationTimingFunction,
            duration: ms(style.animationDuration),
            delay: ms(style.animationDelay),
          };
        }),
      );
    const before = await motion();
    expect(before.map((m) => [m.scale, m.transition, m.animation])).toEqual([
      ["1 0", 320, "none"],
      ["1 0", 320, "none"],
      ["1 0", 320, "none"],
    ]);
    // The scale change starts a transition on the first segment; the event outlives it.
    await fills.first().evaluate((el) => {
      const runs: string[] = [];
      (window as { railRuns?: string[] }).railRuns = runs;
      el.addEventListener("transitionrun", (event) =>
        runs.push((event as TransitionEvent).propertyName),
      );
    });
    await next.click();
    await expect
      .poll(() => page.evaluate(() => (window as { railRuns?: string[] }).railRuns))
      .toEqual(["scale"]);
    // The last completion completes the course: every segment fills in turn within 320 ms, on the
    // exponential ease-out token.
    await next.click();
    await next.click();
    await next.click();
    // Mid-animation, so the scale is left out here and read once the fill has run.
    expect((await motion()).map(({ scale: _, ...m }) => m)).toEqual([
      {
        transition: 320,
        animation: "sc-rail-fill",
        ease: "cubic-bezier(0.16, 1, 0.3, 1)",
        duration: 106.67,
        delay: 0,
      },
      {
        transition: 320,
        animation: "sc-rail-fill",
        ease: "cubic-bezier(0.16, 1, 0.3, 1)",
        duration: 106.67,
        delay: 106.67,
      },
      {
        transition: 320,
        animation: "sc-rail-fill",
        ease: "cubic-bezier(0.16, 1, 0.3, 1)",
        duration: 106.67,
        delay: 213.33,
      },
    ]);
    await expect.poll(async () => (await motion()).map((m) => m.scale)).toEqual(["1", "1", "1"]);
  });

  test.describe("T4: with reduced motion", () => {
    test.use({ reducedMotion: "reduce" });
    test("the rail fills with no animation", async ({ page }) => {
      await openStory(page, story("learning-progressrail--completion"), "light");
      const next = page.getByRole("button", { name: "Complete the next item" });
      for (let i = 0; i < 4; i++) {
        await next.click();
        const running = await page.evaluate(
          () => document.getAnimations().filter((a) => a.playState === "running").length,
        );
        expect(running).toBe(0);
      }
      const fills = await page.locator("[data-slot=progress-rail-fill]").evaluateAll((els) =>
        els.map((el) => {
          const style = getComputedStyle(el);
          return [style.scale, style.transitionDuration, style.animationDuration];
        }),
      );
      expect(fills).toEqual([
        ["1", "0s", "0s"],
        ["1", "0s", "0s"],
        ["1", "0s", "0s"],
      ]);
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

  test("Prose makes a code block a named tab stop when a late font makes it overflow", async ({
    page,
  }) => {
    // The mono font arrives a second late, after Prose first measured the block in the fallback
    // face, as on a slow connection.
    await page.route(/atkinson-hyperlegible-mono.*\.woff2$/, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      await route.continue();
    });
    await openStory(page, story("foundations-prose--lesson"), "light");
    const pre = page.locator('[data-slot="prose"] pre');
    const overflows = await pre.evaluate((el) => el.scrollWidth > el.clientWidth);
    expect(overflows).toBe(true);
    await expect(pre).toHaveAttribute("tabindex", "0");
    await expect(page.getByRole("group", { name: "Code block" })).toBeVisible();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await expect(pre).toBeFocused();
  });

  test("DataTable's loading live region is rendered before it has anything to say", async ({
    page,
  }) => {
    // Not loading: the region is empty. A live region that is display: none, or that appears only
    // with its text, is often not announced when the text arrives.
    await openStory(page, story("data-datatable--default"), "light");
    const region = page.locator("[data-slot=data-table-status]");
    await expect(region).toHaveAttribute("role", "status");
    await expect(region).toHaveText("");
    expect(await region.evaluate((el) => getComputedStyle(el).display)).not.toBe("none");
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
