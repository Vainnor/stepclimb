// SPDX-License-Identifier: AGPL-3.0-or-later
// The branding screen's accessibility (docs/specs/m1/16-learning-ui-components.md B2, B14): the
// saved brand with its preview and report, and the server's contrast refusal shown at the brand
// color with its report, in the default and vZDC brands, light and dark, at 1280 and 375 px. As
// in files.spec.ts, `/api/v1/me` and the branding routes are answered in the browser with
// synthetic data; the rest is the built binary serving the built SPA. Each state is also saved as
// a screenshot in the test's output folder, for the pull request.
import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

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
  { width: 1280, height: 900 },
  { width: 375, height: 740 },
];

/** Every brand's stylesheet from the token build; the vZDC brand is drawn from its rules. */
const allBrands = readFileSync(
  new URL("../../packages/tokens/build/css/variables.css", import.meta.url),
  "utf8",
);

const orgId = "01900000-0000-7000-8000-000000000001";

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
    organization: { id: orgId, slug: "example", name: "Example Training" },
    grants: [{ scope: { type: "organization", id: orgId }, permissions: ["branding.manage"] }],
    features: ["custom_branding"],
    appearance: { theme, density: "comfortable" },
    default_appearance: "system",
    embedding: { return_origins: [] },
  };
}

/** A report in the server's shape: every appearance passing, or light failing two pairs. */
function report(failing: boolean) {
  const pair = (foreground: string, background: string, ratio: number, minimum: number) => ({
    foreground,
    background,
    ratio,
    minimum,
    pass: ratio >= minimum,
  });
  return (["light", "dark", "high-contrast"] as const).map((appearance) => {
    const pairs =
      failing && appearance === "light"
        ? [pair("fg-soft", "bg-canvas", 3.12, 4.5), pair("fg-soft", "bg-raised", 3.05, 4.5)]
        : [pair("fg-strong", "bg-canvas", 15.8, appearance === "high-contrast" ? 7 : 4.5)];
    return { appearance, pass: pairs.every((p) => p.pass), pairs, warnings: [] };
  });
}

const branding = {
  inputs: {
    primary: `#${(0x007373).toString(16).padStart(6, "0")}`,
    ui_font: "Atkinson Hyperlegible Next",
    reading_font: "serif",
    corner_style: "default",
    default_appearance: "system",
  },
  generator: 1,
  css_hash: "a".repeat(64),
  report: report(false),
  fallback: null,
};

async function serveBranding(page: Page, brand: string, theme: string) {
  if (brand !== "default") {
    await page.route("**/theme.css*", (route) =>
      route.fulfill({ contentType: "text/css", body: allBrands }),
    );
  }
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route("**/api/v1/admin/branding", (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: branding });
    return route.fulfill({
      status: 422,
      contentType: "application/problem+json",
      body: JSON.stringify({
        type: "about:blank",
        title: "Unprocessable Entity",
        status: 422,
        detail: "The theme fails the contrast report.",
        code: "branding.contrast_failed",
        report: report(true),
      }),
    });
  });
}

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
      test(`the branding screen has no axe violations, ${brand} ${theme} at ${viewport.width} px`, async ({
        page,
      }, info) => {
        await page.setViewportSize(viewport);
        await serveBranding(page, brand, theme);
        const shot = (state: string) =>
          page.screenshot({ path: info.outputPath(`${name}-${state}.png`), fullPage: true });

        await test.step("the saved brand with its preview and report", async () => {
          await page.goto("/admin/branding");
          await drawBrand(page, brand);
          await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
          await expect(page.getByRole("heading", { level: 1, name: "Branding" })).toBeVisible();
          await expect(page.getByRole("textbox", { name: "Brand color" })).toHaveValue(/^#/);
          await expect(page.getByRole("region", { name: "Contrast report" })).toHaveAttribute(
            "data-pass",
            "true",
          );
          expect(await violations(page)).toEqual([]);
          await shot("saved");
        });

        await test.step("the server refuses the colors", async () => {
          await page.getByRole("button", { name: "Save branding" }).click();
          const field = page.getByRole("textbox", { name: "Brand color" });
          // The form validates natively: the input is invalid to the browser, and its
          // description carries the server's refusal.
          await expect(field).toHaveAccessibleDescription(/too close in contrast to read/);
          expect(await field.evaluate((input: HTMLInputElement) => input.validity.valid)).toBe(
            false,
          );
          await expect(page.getByRole("region", { name: "Contrast report" })).toHaveAttribute(
            "data-pass",
            "false",
          );
          expect(await violations(page)).toEqual([]);
          await shot("refused");
        });
      });
    }
  }
}
