// SPDX-License-Identifier: AGPL-3.0-or-later
// The course layout's accessibility (docs/specs/m1/03-modules-and-items.md B16, B17): an item
// opened by its deep link with the "Back to" link, a deep link whose module does not hold the item
// (the not-found view), and a locked item, whose answer carries its title and no link (spec 04
// B12), in the default and vZDC brands, light and dark, at 1280 and 375 px. As in files.spec.ts,
// `/api/v1/me` and the item read are answered in the browser with synthetic data; the rest is the
// built binary serving the built SPA. Each state is also saved as a screenshot in the test's
// output folder, for the pull request.
import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "./csp";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's WCAG 2.2 AA violations on the whole page, one line per node. */
async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

const brands = ["default", "vzdc"] as const;
const themes = ["light", "dark"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

/**
 * Every brand's stylesheet from the token build. The stack serves the default brand; the vZDC
 * brand is drawn from the build's `[data-sc-brand="vzdc"]` rules, as sign-in-methods.spec.ts does.
 */
const allBrands = readFileSync(
  new URL("../../packages/tokens/build/css/variables.css", import.meta.url),
  "utf8",
);

const courseId = "01900000-0000-7000-8000-0000000000c1";
const moduleId = "01900000-0000-7000-8000-0000000000d1";
const otherModuleId = "01900000-0000-7000-8000-0000000000d2";
const itemId = "01900000-0000-7000-8000-0000000000e1";
const lockedId = "01900000-0000-7000-8000-0000000000e2";
const deepLink = `/c/${courseId}/m/${moduleId}/i/${itemId}`;

function meWith(theme: string) {
  return {
    user: {
      id: "01900000-0000-7000-8000-000000000002",
      display_name: "Avery Example",
      given_name: "Avery",
      family_name: "Example",
      email: "avery@example.test",
      locale: "en",
      tz: "UTC",
      status: "active",
      source: "local",
    },
    organization: {
      id: "01900000-0000-7000-8000-000000000001",
      slug: "example",
      name: "Example Training",
    },
    grants: [],
    features: [],
    appearance: { theme, density: "comfortable" },
    default_appearance: "system",
    embedding: { return_origins: ["https://host.example"] },
  };
}

function item(id: string, title: string, extra: Record<string, unknown>) {
  return {
    id,
    module_id: moduleId,
    course_id: courseId,
    kind: "external_link",
    title,
    position: 0,
    published: true,
    requirement: null,
    lock: null,
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
    ...extra,
  };
}

async function serveCourse(page: Page, brand: string, theme: string) {
  if (brand !== "default") {
    await page.route("**/theme.css*", (route) =>
      route.fulfill({ contentType: "text/css", body: allBrands }),
    );
  }
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route(`**/api/v1/items/${itemId}`, (route) =>
    route.fulfill({
      json: item(itemId, "Radio phraseology", { url: "https://example.org/phraseology" }),
    }),
  );
  // A locked item's answer: its title, without its link (spec 04 B12).
  await page.route(`**/api/v1/items/${lockedId}`, (route) =>
    route.fulfill({
      json: item(lockedId, "Tower procedures", {
        lock: { reasons: [{ kind: "unlock_at", at: "2026-12-01T00:00:00Z" }] },
      }),
    }),
  );
}

/** Marks the loaded page with `brand`, as Storybook does, once `serveCourse` gave it the rules. */
async function drawBrand(page: Page, brand: string) {
  if (brand === "default") return;
  await page.evaluate((b) => {
    document.documentElement.dataset.scBrand = b;
  }, brand);
  await expect(page.locator("html")).toHaveAttribute("data-sc-brand", brand);
}

for (const brand of brands) {
  for (const theme of themes) {
    for (const viewport of widths) {
      const name = `${brand}-${theme}-${viewport.width}`;
      test(`the course layout has no axe violations, ${brand} ${theme} at ${viewport.width} px`, async ({
        page,
      }, info) => {
        await page.setViewportSize(viewport);
        await serveCourse(page, brand, theme);
        const shot = (state: string) =>
          page.screenshot({ path: info.outputPath(`${name}-${state}.png`), fullPage: true });

        await test.step("an item with the return link", async () => {
          await page.goto(`${deepLink}?return=${encodeURIComponent("https://host.example/x")}`);
          await drawBrand(page, brand);
          await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
          await expect(
            page.getByRole("heading", { level: 2, name: "Radio phraseology" }),
          ).toBeVisible();
          await expect(page.getByRole("link", { name: /^Back to host\.example/ })).toBeVisible();
          expect(await violations(page)).toEqual([]);
          await shot("return-link");
        });

        await test.step("a deep link whose module does not hold the item", async () => {
          const wrong = `/c/${courseId}/m/${otherModuleId}/i/${itemId}`;
          await page.goto(wrong);
          await drawBrand(page, brand);
          await expect(
            page.getByRole("heading", { level: 2, name: `There is no page at ${wrong}` }),
          ).toBeVisible();
          expect(await violations(page)).toEqual([]);
          await shot("not-found");
        });

        await test.step("a locked item", async () => {
          await page.goto(`/c/${courseId}/m/${moduleId}/i/${lockedId}`);
          await drawBrand(page, brand);
          await expect(
            page.getByRole("heading", { level: 2, name: "Tower procedures" }),
          ).toBeVisible();
          expect(await violations(page)).toEqual([]);
          await shot("locked-item");
        });
      });
    }
  }
}
